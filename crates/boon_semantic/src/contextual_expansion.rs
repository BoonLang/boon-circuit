use crate::execution::{
    SemanticBlockBinding, SemanticCall, SemanticCallArgument, SemanticCallContextArgument,
    SemanticCallContextBinding, SemanticCallContextId, SemanticCallEntry, SemanticCallId,
    SemanticCallOccurrence, SemanticCallParameterBinding, SemanticCallParameterBindingKind,
    SemanticCallable, SemanticCallableContext, SemanticCallableContextParameter,
    SemanticCallableId, SemanticCallableKind, SemanticCallableParameter,
    SemanticContextualMaterialization, SemanticContextualOperationKind, SemanticContextualOrderKey,
    SemanticExecutionImageColumnsV1, SemanticExprId, SemanticExpression, SemanticExpressionKind,
    SemanticExpressionOrigin, SemanticFunction, SemanticFunctionParameter, SemanticLocalBindingId,
    SemanticMaterializationId, SemanticMaterializationLocalId, SemanticMaterializationResultKind,
    SemanticParameterId, SemanticPatternBinding, SemanticRecordField, SemanticRoot, SemanticScope,
    SemanticScopeId, SemanticSelectArm, SemanticSelectKind, SemanticSourceDef, SemanticSourceId,
    SemanticSourceOrigin, SemanticSourceRead, SemanticStateDef, SemanticStateId,
    SemanticStateLifetimeDeriverV1, SemanticStatement, SemanticStatementId, SemanticStatementKind,
    SemanticStatementOrigin, SemanticStaticOwner, SemanticTextSegment, SemanticValueId,
    SemanticValueMember, SemanticValueOrigin, SemanticValueProvenance,
    checked_semantic_root_specs_v1,
};
use crate::{
    ExecutionPending, OutCallInstanceId, OutInputValue, OutNetId, ResolvedOutGraph as OutNet,
    ScopedCheckedExpr, SemanticImageBuilder, StaticOwnerId, execution_construction_routes_v3,
};
use boon_checked::{
    CheckedCallEntry, CheckedCallId, CheckedCallableKind, CheckedContextBinding,
    CheckedContextualOperation, CheckedDeclarationKind, CheckedExprId, CheckedExpression,
    CheckedExpressionKind, CheckedImageHandoffV3, CheckedMatchPattern, CheckedParameterKind,
    CheckedParameterRequirement, CheckedPassedAccess, CheckedProgramFields, CheckedResourceBinding,
    CheckedStateKind, CheckedTextSegment, CheckedValueUse, ContextFormalId, DeclId, FlowMode,
    FlowType, Type, is_renderable_type,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

fn semantic_parameter_id(callable: SemanticCallableId, ordinal: usize) -> SemanticParameterId {
    SemanticParameterId { callable, ordinal }
}

fn provisional_semantic_source_id(id: boon_checked::CheckedSourceId) -> SemanticSourceId {
    SemanticSourceId(id.0 as usize)
}

fn provisional_semantic_state_id(id: boon_checked::CheckedStateId) -> SemanticStateId {
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
    statements_by_id: BTreeMap<boon_checked::CheckedStatementId, Option<usize>>,
    scopes_by_id: BTreeMap<boon_checked::LexicalScopeId, Option<usize>>,
    calls_by_id: BTreeMap<CheckedCallId, Option<usize>>,
    callables_by_declaration: BTreeMap<DeclId, Option<usize>>,
    declarations_by_scope_and_name:
        BTreeMap<boon_checked::LexicalScopeId, BTreeMap<String, Option<DeclId>>>,
    pattern_bindings_by_declaration: BTreeMap<DeclId, Option<usize>>,
    statements_by_value: BTreeMap<CheckedExprId, Vec<usize>>,
    element_contexts_by_declaration: BTreeMap<DeclId, Option<(usize, usize)>>,
}

impl CheckedProgramLookup {
    fn new(program: &CheckedProgramFields) -> Self {
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
        program: &'a CheckedProgramFields,
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
        program: &'a CheckedProgramFields,
        declaration: DeclId,
    ) -> Option<&'a boon_checked::CheckedDeclaration> {
        self.declarations_by_id
            .get(&declaration)
            .copied()
            .flatten()
            .and_then(|index| program.declarations.get(index))
            .filter(|candidate| candidate.id == declaration)
    }

    fn statement<'a>(
        &self,
        program: &'a CheckedProgramFields,
        statement: boon_checked::CheckedStatementId,
    ) -> Option<&'a boon_checked::CheckedStatement> {
        self.statements_by_id
            .get(&statement)
            .copied()
            .flatten()
            .and_then(|index| program.statements.get(index))
            .filter(|candidate| candidate.id == statement)
    }

    fn scope<'a>(
        &self,
        program: &'a CheckedProgramFields,
        scope: boon_checked::LexicalScopeId,
    ) -> Option<&'a boon_checked::CheckedScope> {
        self.scopes_by_id
            .get(&scope)
            .copied()
            .flatten()
            .and_then(|index| program.scopes.get(index))
            .filter(|candidate| candidate.id == scope)
    }

    fn call<'a>(
        &self,
        program: &'a CheckedProgramFields,
        call: CheckedCallId,
    ) -> Option<&'a boon_checked::CheckedCall> {
        self.calls_by_id
            .get(&call)
            .copied()
            .flatten()
            .and_then(|index| program.calls.get(index))
            .filter(|candidate| candidate.id == call)
    }

    fn callable<'a>(
        &self,
        program: &'a CheckedProgramFields,
        declaration: DeclId,
    ) -> Option<&'a boon_checked::CheckedCallableSignature> {
        self.callables_by_declaration
            .get(&declaration)
            .copied()
            .flatten()
            .and_then(|index| program.callables.get(index))
            .filter(|callable| callable.decl_id == declaration)
    }

    fn declaration_in_exact_scope(
        &self,
        scope: boon_checked::LexicalScopeId,
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
        program: &'a CheckedProgramFields,
        declaration: DeclId,
    ) -> Option<&'a boon_checked::CheckedPatternBinding> {
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
        program: &'a CheckedProgramFields,
        declaration: DeclId,
    ) -> Option<(
        &'a boon_checked::CheckedCall,
        &'a boon_checked::CheckedCallContext,
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
    next_local_binding: usize,
}

impl SemanticExpressionArena {
    fn push(
        &mut self,
        checked_expression: CheckedExprId,
        checked_scope: boon_checked::LexicalScopeId,
        checked_span: boon_checked::CheckedSpan,
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
            flow_type: boon_checked::FlowType {
                mode: FlowMode::Continuous,
                ty: Type::VariantSet(
                    vec![
                        boon_checked::Variant::Tag("Ascending".to_owned()),
                        boon_checked::Variant::Tag("Descending".to_owned()),
                    ]
                    .into(),
                ),
            },
            effect: boon_checked::CheckedEffectSummary::default(),
            owner,
            provenance: runtime_value_provenance(),
            resource_binding_path: None,
            kind: SemanticExpressionKind::Tag("Ascending".to_owned()),
        },
    )
}

pub(crate) fn derive_contextual_materializations(
    program: &CheckedProgramFields,
    out_net: &OutNet,
    retained_ordinary_declarations: &BTreeSet<DeclId>,
    retain_ordinary_calls: bool,
) -> Result<
    (
        Vec<SemanticContextualMaterialization>,
        SemanticExpressionArena,
        SemanticExpressionBuilderIndexes,
        BTreeSet<SemanticCallableId>,
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
    let mut required_ordinary_definitions = BTreeSet::new();
    let builder_indexes = SemanticExpressionBuilderIndexes::new(
        program,
        out_net,
        retained_ordinary_declarations,
        &lookup,
    )?;
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
            &builder_indexes,
            locals,
            &item_types_by_owner,
            &materializations_by_owner,
            &materialization_result_types,
        );
        if retain_ordinary_calls {
            builder.enable_ordinary_call_boundaries();
        }
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
        let item_type = item_type.into_owned();
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
            SemanticContextualOperationKind::Map => Type::List(Type::shared(body_type.clone())),
            SemanticContextualOperationKind::Filter
            | SemanticContextualOperationKind::Retain
            | SemanticContextualOperationKind::Remove
            | SemanticContextualOperationKind::SortBy
            | SemanticContextualOperationKind::ThenBy => {
                Type::List(Type::shared(item_type.clone()))
            }
            SemanticContextualOperationKind::Every
            | SemanticContextualOperationKind::Any
            | SemanticContextualOperationKind::Find => candidate.result_type,
        };
        let result_type = erase_runtime_type_vars(&result_type);
        required_ordinary_definitions.extend(builder.ordinary_definition_scheduled.iter().copied());
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
            body,
            direction,
            inherited_order,
            row_local: SemanticMaterializationLocalId(0),
            item_type: item_type.clone(),
            result_type: result_type.clone(),
        });
        item_types_by_owner.insert(candidate.owner, item_type);
        materialization_result_types.insert(materialization_id, result_type);
    }
    Ok((
        result,
        arena,
        builder_indexes,
        required_ordinary_definitions,
    ))
}

fn concrete_type_in_frame(out_net: &OutNet, ty: &Type, frame: Option<OutCallInstanceId>) -> Type {
    let ty = frame.map_or_else(
        || ty.clone(),
        |instance| out_net.apply_type_substitutions(instance, ty),
    );
    erase_runtime_type_vars(&ty)
}

pub(crate) fn erase_runtime_type_vars(ty: &Type) -> Type {
    match ty {
        Type::Var(_) => Type::Unknown,
        Type::List(item) => Type::List(Type::shared(erase_runtime_type_vars(item))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(erase_runtime_type_vars(key)),
            value: Box::new(erase_runtime_type_vars(value)),
        },
        Type::Set(item) => Type::Set(Type::shared(erase_runtime_type_vars(item))),
        Type::Function { args, result } => Type::Function {
            args: args.iter().map(erase_runtime_type_vars).collect(),
            result: Box::new(boon_checked::FlowType {
                mode: result.mode,
                ty: erase_runtime_type_vars(&result.ty),
            }),
        },
        Type::Object(shape) => Type::object(boon_checked::ObjectShape {
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
                    boon_checked::Variant::Tag(tag) => boon_checked::Variant::Tag(tag.clone()),
                    boon_checked::Variant::Tagged { tag, fields } => {
                        boon_checked::Variant::Tagged {
                            tag: tag.clone(),
                            fields: boon_checked::ObjectShape {
                                fields: fields
                                    .fields
                                    .iter()
                                    .map(|(name, ty)| (name.clone(), erase_runtime_type_vars(ty)))
                                    .collect(),
                                field_order: fields.field_order.clone(),
                                open: fields.open,
                            }
                            .into(),
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

fn runtime_flush_boundary_flow_type(mut success: FlowType, flush_type: Type) -> FlowType {
    success.ty = erase_runtime_type_vars(&boon_checked::canonical_union_type(vec![
        success.ty, flush_type,
    ]));
    if success.mode == FlowMode::Absent {
        success.mode = FlowMode::Continuous;
    }
    success
}

pub(crate) fn refine_runtime_occurrence_type(
    existing: &Type,
    expected: &Type,
) -> Result<Type, String> {
    let existing = erase_runtime_type_vars(existing);
    let expected = erase_runtime_type_vars(expected);
    let existing_closed = boon_checked::type_is_recursively_closed(&existing);
    let expected_closed = boon_checked::type_is_recursively_closed(&expected);
    if expected_closed {
        if existing_closed {
            if boon_checked::resolved_type_is_assignable_to(&expected, &existing) {
                return Ok(expected);
            }
            if boon_checked::resolved_type_is_assignable_to(&existing, &expected) {
                return Ok(existing);
            }
            return Err(format!(
                "closed runtime type {existing:?} is incompatible with required type {expected:?}"
            ));
        }
        // A closed instantiated occurrence is already the exact callable
        // contract. `specialize_checked_call_result` would return it unchanged,
        // so avoid recursively re-walking the same structural type at every
        // transparent wrapper layer.
        return Ok(expected);
    }
    let refined = erase_runtime_type_vars(&boon_checked::specialize_checked_call_result(
        &expected, &existing,
    ));
    Ok(refined)
}

/// Merge a checked definition-local storage template with one exact semantic
/// runtime occurrence. Compatible state domains widen structurally, while a
/// checked fixed-BYTES contract remains authoritative over dynamic storage.
pub(crate) fn contextualize_runtime_storage_type(
    checked: &Type,
    runtime: &Type,
) -> Result<Type, String> {
    refine_runtime_occurrence_type(checked, runtime)?;

    fn merge(checked: &Type, runtime: &Type) -> Type {
        if checked == runtime {
            return checked.clone();
        }
        match (checked, runtime) {
            (
                Type::Bytes(boon_checked::BytesType::Fixed(_)),
                Type::Bytes(boon_checked::BytesType::Dynamic),
            ) => checked.clone(),
            (Type::List(checked), Type::List(runtime)) => {
                Type::List(Type::shared(merge(checked, runtime)))
            }
            (Type::Set(checked), Type::Set(runtime)) => {
                Type::Set(Type::shared(merge(checked, runtime)))
            }
            (
                Type::Map {
                    key: checked_key,
                    value: checked_value,
                },
                Type::Map {
                    key: runtime_key,
                    value: runtime_value,
                },
            ) => Type::Map {
                key: Box::new(merge(checked_key, runtime_key)),
                value: Box::new(merge(checked_value, runtime_value)),
            },
            (Type::Object(checked), Type::Object(runtime))
                if checked.fields.keys().eq(runtime.fields.keys()) =>
            {
                let widened = boon_checked::widen_structural_type(
                    &Type::Object(checked.clone()),
                    &Type::Object(runtime.clone()),
                );
                let Type::Object(widened) = widened else {
                    unreachable!("widening compatible records must retain a record")
                };
                Type::object(boon_checked::ObjectShape {
                    fields: widened
                        .fields
                        .iter()
                        .map(|(name, fallback)| {
                            let ty = match (checked.fields.get(name), runtime.fields.get(name)) {
                                (Some(checked), Some(runtime)) => merge(checked, runtime),
                                _ => fallback.clone(),
                            };
                            (name.clone(), ty)
                        })
                        .collect(),
                    field_order: widened.field_order.clone(),
                    open: widened.open,
                })
            }
            _ => boon_checked::widen_structural_type(checked, runtime),
        }
    }

    Ok(merge(
        &erase_runtime_type_vars(checked),
        &erase_runtime_type_vars(runtime),
    ))
}

fn refine_runtime_call_boundary_type(
    existing: &Type,
    formal_scheme: &Type,
    instantiated_formal: &Type,
) -> Result<Type, String> {
    // A recursively closed call occurrence is provider authority. A generic
    // parameter/result scheme can constrain its own variables, but an
    // unrelated alpha with the same ordinal in an ancestor frame must not
    // reinterpret that already-concrete occurrence. Closed schemes still take
    // the strict compatibility path below.
    if boon_checked::type_is_recursively_closed(existing)
        && !boon_checked::type_is_recursively_closed(formal_scheme)
    {
        return Ok(existing.clone());
    }
    refine_runtime_occurrence_type(existing, instantiated_formal)
}

fn exact_expression_in_call_frame<'a>(
    expressions: &'a [SemanticExpression],
    origins: &[SemanticExpressionOrigin],
    expression: SemanticExprId,
    frame: OutCallInstanceId,
) -> Result<Option<&'a SemanticExpression>, String> {
    let definition = expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .ok_or_else(|| {
            format!("call-result refinement references missing expression {expression}")
        })?;
    let origin = origins
        .get(expression.as_usize())
        .filter(|origin| {
            origin.expression == expression
                && origin.checked_expression == definition.checked_expr_id
        })
        .ok_or_else(|| {
            format!("call-result refinement expression {expression} has no exact checked origin")
        })?;
    Ok((origin.call_instance == Some(frame)).then_some(definition))
}

/// Pushes one closed, concrete user-call result back into state occurrences
/// that structurally contribute to that same result.
///
/// Checked type variables are definition-local ordinals, so a frame-wide
/// substitution by raw `TypeVar` would alias unrelated nested call schemes.
/// This walk instead follows only exact, transparent result structure in the
/// same call frame. Unsupported or ambiguous carriers are not authorities and
/// therefore stop the walk without following their dependency edges.
fn refine_call_result_state_occurrences(
    expressions: &mut [SemanticExpression],
    origins: &[SemanticExpressionOrigin],
    hold_owners: &BTreeMap<CheckedExprId, DeclId>,
    root: SemanticExprId,
    frame: OutCallInstanceId,
    callable: DeclId,
    expected: &Type,
) -> Result<(), String> {
    if !boon_checked::type_is_recursively_closed(expected) {
        return Ok(());
    }

    let mut pending = vec![(root, expected.clone())];
    let mut structural_authorities = BTreeMap::<SemanticExprId, Type>::new();
    let mut state_refinements = BTreeMap::<SemanticExprId, Type>::new();
    while let Some((expression, expected)) = pending.pop() {
        let Some(definition) =
            exact_expression_in_call_frame(expressions, origins, expression, frame)?
        else {
            continue;
        };
        if let Some(previous) = structural_authorities.get(&expression) {
            if previous == &expected {
                continue;
            }
            return Err(format!(
                "call frame {frame} expression {expression} is reached by incompatible result authorities {previous:?} and {expected:?}",
            ));
        }
        structural_authorities.insert(expression, expected.clone());

        if let Some(owner) = hold_owners.get(&definition.checked_expr_id).copied() {
            if owner != callable {
                return Err(format!(
                    "call frame {frame} callable {} reached HOLD expression {expression} owned by callable {}",
                    callable.0, owner.0,
                ));
            }
            if !matches!(&definition.kind, SemanticExpressionKind::Hold { .. }) {
                return Err(format!(
                    "call frame {frame} HOLD expression {expression} checked {} has non-HOLD semantic kind {:?}",
                    definition.checked_expr_id.0, definition.kind,
                ));
            }
            refine_runtime_occurrence_type(&definition.flow_type.ty, &expected).map_err(
                |error| {
                    format!(
                        "call frame {frame} cannot refine state expression {expression} checked {} from {:?} to {expected:?}: {error}",
                        definition.checked_expr_id.0, definition.flow_type.ty,
                    )
                },
            )?;
            // The exact call-result field is the occurrence authority for the
            // returned state. Compatibility above rejects a stale/disjoint
            // checked state, but a narrower definition-local state must not
            // override the concrete invocation's widened memory domain.
            let refined = expected;
            if !boon_checked::type_is_recursively_closed(&refined) {
                return Err(format!(
                    "call frame {frame} state expression {expression} refinement remained unresolved: {refined:?}",
                ));
            }
            state_refinements.insert(expression, refined);
            continue;
        }

        match (&definition.kind, expected) {
            (SemanticExpressionKind::Object(fields), Type::Object(shape)) => {
                if fields.len() != shape.fields.len() || fields.iter().any(|field| field.spread) {
                    continue;
                }
                let mut names = BTreeSet::new();
                let Some(fields) = fields
                    .iter()
                    .map(|field| {
                        if !names.insert(field.name.as_str()) {
                            return None;
                        }
                        shape
                            .fields
                            .get(&field.name)
                            .cloned()
                            .map(|expected| (field.value, expected))
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                pending.extend(fields);
            }
            (SemanticExpressionKind::Block { result, .. }, expected) => {
                pending.push((*result, expected));
            }
            (SemanticExpressionKind::Draining { input }, expected) => {
                pending.push((*input, expected));
            }
            (SemanticExpressionKind::Project { input, fields }, expected) if fields.is_empty() => {
                pending.push((*input, expected))
            }
            _ => {}
        }
    }
    for (expression, refined) in state_refinements {
        expressions[expression.as_usize()].flow_type.ty = refined;
    }
    Ok(())
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
                    _ => boon_checked::canonical_union_type(projected),
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
            let shape = shape.into_owned();
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
    Some(Type::object(boon_checked::ObjectShape {
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
            Some(Type::VariantSet(
                vec![boon_checked::Variant::Tagged {
                    tag: tag.clone(),
                    fields: shape,
                }]
                .into(),
            ))
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
                .then(|| Type::List(Type::shared(first)))
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
                .then(|| Type::Set(Type::shared(first)))
        }
        SemanticExpressionKind::Block { result, .. } => expressions
            .get(result.as_usize())
            .map(|expression| expression.flow_type.ty.clone()),
        SemanticExpressionKind::When { arms, .. } if !arms.is_empty() => {
            let first = expressions
                .get(arms[0].output.as_usize())?
                .flow_type
                .ty
                .clone();
            arms.iter()
                .skip(1)
                .all(|arm| {
                    expressions
                        .get(arm.output.as_usize())
                        .is_some_and(|expression| expression.flow_type.ty == first)
                })
                .then_some(first)
        }
        _ => None,
    }
}

fn semantic_callable_inventory(
    program: &CheckedProgramFields,
    semantic_scope_ids: &BTreeMap<boon_checked::LexicalScopeId, SemanticScopeId>,
    completed_context_formals: &BTreeMap<ContextFormalId, FlowType>,
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
            context_parameter: callable.context_formal.and_then(|formal| {
                completed_context_formals
                    .get(&formal)
                    .cloned()
                    .map(|flow_type| SemanticCallableContextParameter {
                        id: semantic_parameter_id(id, callable.parameters.len()),
                        formal,
                        name: "PASSED".to_owned(),
                        flow_type,
                    })
            }),
            result: callable.result.clone(),
            role: callable.role,
            effect: callable.effect,
            body: callable.body,
            result_expression: callable.result_expression,
            contextual_operation: callable.contextual_operation,
            semantic_root: None,
        });
    }
    Ok((callables, callable_ids))
}

fn context_projection_scaffold(fields: &[String], leaf: Type) -> Type {
    let Some((field, remaining)) = fields.split_first() else {
        return leaf;
    };
    Type::object(boon_checked::ObjectShape::from_ordered_fields(
        [(field.clone(), context_projection_scaffold(remaining, leaf))],
        true,
    ))
}

fn add_missing_context_projection(ty: &Type, fields: &[String], leaf: Type) -> Type {
    let Some((field, remaining)) = fields.split_first() else {
        return ty.clone();
    };
    let Type::Object(shape) = ty else {
        return context_projection_scaffold(fields, leaf);
    };
    let mut shape = shape.as_ref().clone();
    if let Some(existing) = shape.fields.get(field).cloned() {
        shape.fields.insert(
            field.clone(),
            add_missing_context_projection(&existing, remaining, leaf),
        );
    } else {
        shape
            .fields
            .insert(field.clone(), context_projection_scaffold(remaining, leaf));
        shape.field_order.push(field.clone());
    }
    Type::object(shape)
}

/// Dense compatibility assembly weaves child-owner expressions into one
/// callable body, but the callable context interface intentionally contains
/// only its owner-local sparse surface. Retained semantic definitions need the
/// union of those exact body reads. Complete only missing paths here; existing
/// scheme leaves keep their alpha correlation with parameters/results.
fn completed_context_formal_flow_types(
    program: &CheckedProgramFields,
    lookup: &CheckedProgramLookup,
) -> Result<BTreeMap<ContextFormalId, FlowType>, ExpansionError> {
    let mut owners = BTreeMap::new();
    let mut completed = BTreeMap::new();
    for formal in &program.context_formals {
        if owners.insert(formal.id, formal.callable).is_some()
            || completed
                .insert(formal.id, formal.scheme.flow_type.clone())
                .is_some()
        {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "checked context formal {} is defined more than once",
                formal.id.0,
            )));
        }
    }
    for expression in &program.expressions {
        let CheckedExpressionKind::Passed {
            formal, projection, ..
        } = &expression.kind
        else {
            continue;
        };
        let callable = owners.get(formal).copied().ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "checked PASSED expression {} references missing context formal {}",
                expression.id.0, formal.0,
            ))
        })?;
        let enclosing = enclosing_function_owner(program, lookup, expression.scope_id);
        if enclosing != Some(callable) {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "checked PASSED expression {} belongs to callable {enclosing:?} instead of context owner {}",
                expression.id.0, callable.0,
            )));
        }
        let flow_type = completed
            .get_mut(formal)
            .expect("context-formal owner and flow indexes are built together");
        flow_type.ty = add_missing_context_projection(
            &flow_type.ty,
            projection,
            erase_runtime_type_vars(&expression.flow_type.ty),
        );
    }
    Ok(completed)
}

fn semantic_call_inventory(
    program: &CheckedProgramFields,
    semantic_scope_ids: &BTreeMap<boon_checked::LexicalScopeId, SemanticScopeId>,
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
    program: &CheckedProgramFields,
    execution: &SemanticExecutionImageColumnsV1,
) -> Result<(), String> {
    let lookup = CheckedProgramLookup::new(program);
    let completed_context_formals =
        completed_context_formal_flow_types(program, &lookup).map_err(|error| error.to_string())?;
    let semantic_scope_ids = program
        .scopes
        .iter()
        .enumerate()
        .map(|(index, scope)| (scope.id, SemanticScopeId(index)))
        .collect::<BTreeMap<_, _>>();
    let (expected_callables, callable_ids) =
        semantic_callable_inventory(program, &semantic_scope_ids, &completed_context_formals)
            .map_err(|error| error.to_string())?;
    let (expected_calls, _) = semantic_call_inventory(program, &semantic_scope_ids, &callable_ids)
        .map_err(|error| error.to_string())?;
    let mut actual_callables = execution.callables.clone();
    for callable in &mut actual_callables {
        callable.semantic_root = None;
    }
    if actual_callables != expected_callables {
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
    program: &CheckedProgramFields,
    checked_handoff: CheckedImageHandoffV3,
    out_net: &OutNet,
    materializations: &[SemanticContextualMaterialization],
    mut arena: SemanticExpressionArena,
    builder_indexes: &SemanticExpressionBuilderIndexes,
    required_ordinary_definitions: &BTreeSet<SemanticCallableId>,
    retain_ordinary_calls: bool,
) -> Result<SemanticImageBuilder<ExecutionPending>, ExpansionError> {
    let execution_routes = execution_construction_routes_v3(&checked_handoff, out_net)
        .map_err(ExpansionError::InvalidLocalBindings)?;
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
    let (mut callables, callable_ids) = semantic_callable_inventory(
        program,
        &semantic_scope_ids,
        &builder_indexes.completed_context_formals,
    )?;
    let (calls, call_ids) = semantic_call_inventory(program, &semantic_scope_ids, &callable_ids)?;
    let call_occurrences = out_net
        .call_instances
        .iter()
        .enumerate()
        .map(|(index, occurrence)| {
            if occurrence.id.as_usize() != index {
                return Err(ExpansionError::InvalidLocalBindings(format!(
                    "OUT call occurrence {} is not dense at index {index}",
                    occurrence.id
                )));
            }
            let (call, context_ordinals) = match occurrence.provenance.call_id {
                Some(checked_call) => {
                    let call = call_ids.get(&checked_call).copied().ok_or_else(|| {
                        ExpansionError::InvalidLocalBindings(format!(
                            "OUT call occurrence {} references missing checked call {}",
                            occurrence.id, checked_call.0
                        ))
                    })?;
                    let checked = lookup.call(program, checked_call).ok_or_else(|| {
                        ExpansionError::InvalidLocalBindings(format!(
                            "OUT call occurrence {} references absent checked call {}",
                            occurrence.id, checked_call.0
                        ))
                    })?;
                    (
                        Some(call),
                        checked
                            .contexts
                            .iter()
                            .map(|context| context.signature)
                            .collect(),
                    )
                }
                None => (None, Vec::new()),
            };
            Ok(SemanticCallOccurrence {
                id: occurrence.id,
                parent: occurrence.parent,
                call,
                context_ordinals,
            })
        })
        .collect::<Result<Vec<_>, ExpansionError>>()?;
    let materializations_by_owner = materializations
        .iter()
        .map(|materialization| (materialization.owner, materialization.id))
        .collect::<BTreeMap<_, _>>();
    let materialization_result_types = materializations
        .iter()
        .map(|materialization| (materialization.id, materialization.result_type.clone()))
        .collect::<BTreeMap<_, _>>();
    let inherited_local_types = BTreeMap::new();
    let mut builder = SemanticExpressionBuilder::new(
        program,
        &lookup,
        out_net,
        builder_indexes,
        BTreeMap::new(),
        &inherited_local_types,
        &materializations_by_owner,
        &materialization_result_types,
    );
    if retain_ordinary_calls {
        builder.enable_ordinary_call_boundaries();
    }
    for callable in required_ordinary_definitions {
        builder.schedule_ordinary_definition(*callable);
    }
    let included = program
        .statements
        .iter()
        .filter(|statement| {
            !matches!(
                statement.kind,
                boon_checked::CheckedStatementKind::Function { .. }
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
            boon_checked::CheckedStatementKind::Function { declaration }
            | boon_checked::CheckedStatementKind::Field { declaration } => Some(*declaration),
            boon_checked::CheckedStatementKind::Source { declaration, .. }
            | boon_checked::CheckedStatementKind::Hold { declaration, .. }
            | boon_checked::CheckedStatementKind::List { declaration, .. } => *declaration,
            boon_checked::CheckedStatementKind::Block
            | boon_checked::CheckedStatementKind::Spread
            | boon_checked::CheckedStatementKind::Expression => None,
        };
        // Checked declaration alphas belong to definition/owner-local inference
        // namespaces. Semantic expressions deliberately erase any alpha that
        // survives occurrence contextualization because those raw ordinals
        // have no runtime identity. Keep the named statement boundary in the
        // same canonical runtime namespace as its value; exact SOURCE/event
        // payloads remain owned independently by resource and view contracts.
        let mut statement_flow_type = declaration
            .and_then(|declaration| lookup.declaration(program, declaration))
            .map(|declaration| FlowType {
                mode: declaration.flow_type.mode,
                ty: erase_runtime_type_vars(&declaration.flow_type.ty),
            });
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
                    let boundary_expression =
                        builder.flush_boundary_origin_for_value(expression, value);
                    builder.wrap_flush_boundary(boundary_expression, value, None)
                } else {
                    Ok(value)
                }
            })
            .transpose()?;
        if matches!(
            statement.kind,
            boon_checked::CheckedStatementKind::Field { .. }
        ) && let (Some(checked), Some(value)) = (statement_flow_type.as_ref(), value)
        {
            let runtime = builder
                .expressions
                .get(value.as_usize())
                .filter(|expression| expression.id == value)
                .map(|expression| expression.flow_type.clone())
                .ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "semantic field statement {semantic_statement} references missing value {value}"
                    ))
                })?;
            refine_runtime_occurrence_type(&checked.ty, &runtime.ty).map_err(|error| {
                ExpansionError::InvalidLocalBindings(format!(
                    "semantic field statement {semantic_statement} checked type {:?} is incompatible with contextual value {value} type {:?}: {error}",
                    checked.ty, runtime.ty,
                ))
            })?;
            // A semantic statement is a runtime occurrence, not a checked
            // definition template. Its exact expanded value owns the field
            // flow; checked named-value metadata separately retains public
            // refinements such as fixed-BYTES contracts.
            statement_flow_type = Some(runtime);
        }
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
            boon_checked::CheckedStatementKind::Function { .. } => unreachable!(),
            boon_checked::CheckedStatementKind::Field { declaration } => {
                let (name, path) = declaration_parts(Some(*declaration));
                SemanticStatementKind::Field {
                    name: name.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
                    path: path.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
                }
            }
            boon_checked::CheckedStatementKind::Source { declaration, event } => {
                let (name, path) = declaration_parts(*declaration);
                SemanticStatementKind::Source {
                    name,
                    path,
                    event: event.clone(),
                }
            }
            boon_checked::CheckedStatementKind::Hold {
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
            boon_checked::CheckedStatementKind::List {
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
            boon_checked::CheckedStatementKind::Block => SemanticStatementKind::Block,
            boon_checked::CheckedStatementKind::Spread => SemanticStatementKind::Spread,
            boon_checked::CheckedStatementKind::Expression => SemanticStatementKind::Expression,
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
            flow_type: statement_flow_type,
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
    builder.expand_pending_ordinary_definitions()?;
    let ordinary_definition_roots = builder.ordinary_definition_roots.clone();

    statements.sort_by_key(|statement| statement.id);
    let offset = arena.expressions.len();
    let local_expressions = builder.finish();
    for statement in &mut statements {
        if let Some(value) = &mut statement.value {
            *value = rebase_expr_id(*value, offset);
        }
    }
    append_expression_arena_without_roots(&mut arena, local_expressions);
    for (callable, root) in ordinary_definition_roots {
        let definition = callables
            .get_mut(callable.as_usize())
            .filter(|definition| definition.id == callable)
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "ordinary callable {callable} has no dense semantic definition"
                ))
            })?;
        definition.semantic_root = Some(rebase_expr_id(root, offset));
    }
    if std::env::var_os("BOON_SEMANTIC_TRACE").is_some() {
        let ordinary = callables
            .iter()
            .filter(|callable| callable.semantic_root.is_some())
            .map(|callable| callable.name.as_str())
            .collect::<Vec<_>>();
        eprintln!(
            "boon_semantic ordinary_callable_boundaries count={} sample={:?}",
            ordinary.len(),
            ordinary.iter().take(16).collect::<Vec<_>>()
        );
    }
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
                        flow_type: boon_checked::FlowType {
                            mode: FlowMode::PresentOrAbsent,
                            ty: Type::Absent,
                        },
                        effect: boon_checked::CheckedEffectSummary {
                            emits_source: true,
                            ..boon_checked::CheckedEffectSummary::default()
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
                    effect: boon_checked::CheckedEffectSummary {
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
                    effect: boon_checked::CheckedEffectSummary::default(),
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
                lifetime: crate::SemanticStateLifetimeV1::Persistent,
            });
        }
    }
    let mut state_lifetime_deriver = SemanticStateLifetimeDeriverV1::new(&arena.expressions)
        .map_err(ExpansionError::InvalidLocalBindings)?;
    for state in &mut states {
        state.lifetime = state_lifetime_deriver
            .derive(state.expression)
            .map_err(ExpansionError::InvalidLocalBindings)?;
    }
    remap_checked_resource_ids(
        program,
        &mut arena.expressions,
        &arena.checked_expression_origins,
        &semantic_source_by_checked_instance,
        &semantic_state_by_checked_instance,
    )?;
    let execution = SemanticExecutionImageColumnsV1 {
        materializations: materializations.to_vec(),
        expressions: arena.expressions,
        statements,
        scopes,
        callables,
        calls,
        call_occurrences,
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
    };
    SemanticImageBuilder::execution_pending(checked_handoff, execution_routes, execution)
        .map_err(ExpansionError::InvalidLocalBindings)
}

struct ExactResourceInstanceContext<'a> {
    expressions: &'a [SemanticExpression],
    origins: &'a [SemanticExpressionOrigin],
    statements: &'a [SemanticStatement],
    materializations: &'a [SemanticContextualMaterialization],
    out_net: &'a OutNet,
    checked_statement: boon_checked::CheckedStatementId,
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
    let owned_statement_ids = candidates
        .iter()
        .filter_map(|(_, statement)| statement.map(|statement| statement.id))
        .collect::<BTreeSet<_>>();
    let mut declaration_statements = statements
        .iter()
        .filter(|statement| {
            matches!(
                statement.origin,
                SemanticStatementOrigin::Checked { statement }
                    if statement == checked_statement
            ) && statement.declaration == Some(declaration)
                && statement.checked_resources.contains(&checked_binding)
                && owned_statement_ids.contains(&statement.id)
        })
        .map(|statement| statement.id)
        .collect::<Vec<_>>();
    if declaration_statements.is_empty() {
        // A statement root can be established before its expression-origin
        // ownership is attached. Recover that exact checked declaration by
        // reachability instead of synthesizing a second statement for the
        // same SOURCE/state authority.
        for statement in statements.iter().filter(|statement| {
            matches!(
                statement.origin,
                SemanticStatementOrigin::Checked { statement }
                    if statement == checked_statement
            ) && statement.declaration == Some(declaration)
                && statement.checked_resources.contains(&checked_binding)
        }) {
            let Some(root) = statement.value else {
                continue;
            };
            if candidates.iter().any(|(candidate, _)| {
                arena_expression_reaches_in_slice(expressions, root, *candidate)
            }) {
                declaration_statements.push(statement.id);
            }
        }
    }
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

fn arena_expression_reaches_in_slice(
    expressions: &[SemanticExpression],
    root: SemanticExprId,
    target: SemanticExprId,
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if id == target {
            return true;
        }
        let Some(expression) = expressions
            .get(id.as_usize())
            .filter(|candidate| candidate.id == id)
        else {
            return false;
        };
        pending.extend(arena_expression_children(&expression.kind));
    }
    false
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
        SemanticExpressionKind::Call {
            arguments,
            context_argument,
            ..
        } => arguments
            .iter()
            .map(|argument| argument.value)
            .chain(context_argument.iter().map(|argument| argument.value))
            .collect(),
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
    program: &CheckedProgramFields,
    lookup: &CheckedProgramLookup,
    semantic_scope_ids: &BTreeMap<boon_checked::LexicalScopeId, SemanticScopeId>,
    arena: &mut SemanticExpressionArena,
    statements: &mut Vec<SemanticStatement>,
    expression: SemanticExprId,
    suggested_statement: Option<SemanticStatementId>,
    checked_statement: boon_checked::CheckedStatementId,
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
        boon_checked::CheckedStatementKind::Function { .. } => {
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
        boon_checked::CheckedStatementKind::Field { declaration } => {
            let (name, path) = declaration_parts(Some(*declaration));
            SemanticStatementKind::Field {
                name: name.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
                path: path.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
            }
        }
        boon_checked::CheckedStatementKind::Source { declaration, event } => {
            let (name, path) = declaration_parts(*declaration);
            SemanticStatementKind::Source {
                name,
                path,
                event: event.clone(),
            }
        }
        boon_checked::CheckedStatementKind::Hold {
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
        boon_checked::CheckedStatementKind::List {
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
        boon_checked::CheckedStatementKind::Block => SemanticStatementKind::Block,
        boon_checked::CheckedStatementKind::Spread => SemanticStatementKind::Spread,
        boon_checked::CheckedStatementKind::Expression => SemanticStatementKind::Expression,
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
    program: &CheckedProgramFields,
    expressions: &mut [SemanticExpression],
    origins: &[SemanticExpressionOrigin],
    semantic_sources: &BTreeMap<
        (
            boon_checked::CheckedSourceId,
            Option<StaticOwnerId>,
            Option<OutCallInstanceId>,
        ),
        SemanticSourceId,
    >,
    semantic_states: &BTreeMap<
        (
            boon_checked::CheckedStateId,
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
        .is_some_and(|branch| {
            branch.flow_type.mode == FlowMode::Continuous
                && !branch.effect.invokes_host
                && !branch.effect.emits_source
        })
}

fn synthesize_statement_owned_states(
    program: &CheckedProgramFields,
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
                effect: boon_checked::CheckedEffectSummary {
                    reads_state: true,
                    writes_state: true,
                    ..boon_checked::CheckedEffectSummary::default()
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
    let local_binding_offset = target.next_local_binding;
    target.next_local_binding = target
        .next_local_binding
        .saturating_add(source.next_local_binding);
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
            context_argument,
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
            if let Some(argument) = context_argument {
                rebase(&mut argument.value);
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
    program: &CheckedProgramFields,
    scope: boon_checked::LexicalScopeId,
) -> Option<&boon_checked::CheckedScope> {
    program
        .scopes
        .iter()
        .find(|candidate| candidate.id == scope)
}

fn declaration_in_exact_scope(
    lookup: &CheckedProgramLookup,
    scope: boon_checked::LexicalScopeId,
    name: &str,
) -> Option<DeclId> {
    lookup.declaration_in_exact_scope(scope, name)
}

fn declaration_in_lexical_scope(
    program: &CheckedProgramFields,
    lookup: &CheckedProgramLookup,
    mut scope: boon_checked::LexicalScopeId,
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
    program: &CheckedProgramFields,
    mut scope: boon_checked::LexicalScopeId,
) -> bool {
    let mut visited = BTreeSet::new();
    while visited.insert(scope) {
        let Some(current) = checked_scope(program, scope) else {
            return false;
        };
        if current.kind == boon_checked::CheckedScopeKind::Function {
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
    program: &CheckedProgramFields,
    lookup: &CheckedProgramLookup,
    target: DeclId,
) -> Option<String> {
    let declaration = lookup.declaration(program, target)?;
    let mut segments = vec![declaration.name.clone()];
    let mut scope = declaration.scope_id;
    let mut visited = BTreeSet::new();
    while scope != program.root_scope && visited.insert(scope) {
        let current = lookup.scope(program, scope)?;
        if current.kind == boon_checked::CheckedScopeKind::Function {
            break;
        }
        if let Some(owner) = current.owner
            && let Some(owner) = lookup.declaration(program, owner)
            && matches!(
                owner.kind,
                boon_checked::CheckedDeclarationKind::Field
                    | boon_checked::CheckedDeclarationKind::Source
                    | boon_checked::CheckedDeclarationKind::Hold
                    | boon_checked::CheckedDeclarationKind::List
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
    program: &CheckedProgramFields,
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
    fn projected_declaration_dependency(
        &self,
        target: DeclId,
        owner: Option<StaticOwnerId>,
        projection: &[String],
    ) -> Option<(SemanticExprId, usize)> {
        let mut dependency = self
            .declarations
            .get(&(target, owner))
            .or_else(|| self.declarations.get(&(target, None)))
            .copied()?;
        let mut consumed = 0;
        while let Some(field_name) = projection.get(consumed) {
            let Some(expression) = self
                .expressions
                .get(dependency.as_usize())
                .filter(|candidate| candidate.id == dependency)
            else {
                break;
            };
            let fields = match &expression.kind {
                SemanticExpressionKind::Object(fields)
                | SemanticExpressionKind::TaggedObject { fields, .. } => fields,
                _ => break,
            };
            if fields.iter().any(|field| field.spread) {
                break;
            }
            let mut matching = fields
                .iter()
                .filter(|field| field.name.as_str() == field_name.as_str());
            let Some(field) = matching.next() else {
                break;
            };
            if matching.next().is_some() {
                break;
            }
            dependency = field.value;
            consumed += 1;
        }
        Some((dependency, consumed))
    }

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
                        const MAX_CYCLE_ENTRIES: usize = 32;
                        let mut chain = visiting
                            .iter()
                            .chain(std::iter::once(&id))
                            .take(MAX_CYCLE_ENTRIES)
                            .filter_map(|expression| {
                                self.expressions
                                    .get(expression.as_usize())
                                    .filter(|candidate| candidate.id == *expression)
                                    .map(|definition| {
                                        format!(
                                            "{}(checked={},owner={:?})",
                                            expression,
                                            definition.checked_expr_id.0,
                                            definition.owner,
                                        )
                                    })
                            })
                            .collect::<Vec<_>>();
                        if visiting.len().saturating_add(1) > MAX_CYCLE_ENTRIES {
                            chain.push(format!(
                                "... {} more visiting expressions",
                                visiting.len().saturating_add(1) - MAX_CYCLE_ENTRIES
                            ));
                        }
                        return Err(ExpansionError::InvalidLocalBindings(format!(
                            "provenance cycle reaches expression {id}: {}",
                            chain.join(" -> ")
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
                        SemanticExpressionKind::CanonicalRead {
                            target, projection, ..
                        }
                        | SemanticExpressionKind::Drain {
                            target, projection, ..
                        } => self
                            .projected_declaration_dependency(*target, owner, projection)
                            .map(|(value, _)| value)
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
                        } => {
                            match self.projected_declaration_dependency(target, owner, &projection)
                            {
                                Some((value, consumed)) if value != id => {
                                    cached(value)?.projected(&projection[consumed..])
                                }
                                _ => expression.provenance,
                            }
                        }
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

pub(crate) struct SemanticExpressionBuilderIndexes {
    callable_ids: BTreeMap<DeclId, SemanticCallableId>,
    call_ids: BTreeMap<CheckedCallId, SemanticCallId>,
    producer_callable_ids: BTreeMap<crate::ProducerFunctionId, SemanticCallableId>,
    ordinary_callable_ids: BTreeSet<SemanticCallableId>,
    completed_context_formals: BTreeMap<ContextFormalId, FlowType>,
    hold_owners: BTreeMap<CheckedExprId, DeclId>,
    callables_with_holds: BTreeSet<DeclId>,
}

fn ordinary_template_boundary_type(ty: &Type) -> bool {
    ordinary_template_boundary_type_inner(ty)
}

fn ordinary_template_boundary_type_inner(ty: &Type) -> bool {
    match ty {
        Type::Text | Type::Number | Type::Bytes(_) | Type::Absent | Type::Bits { .. } => true,
        Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            boon_checked::Variant::Tag(_) => true,
            boon_checked::Variant::Tagged { fields, .. } => fields
                .fields
                .values()
                .all(ordinary_template_boundary_type_inner),
        }),
        Type::Object(shape) => shape
            .fields
            .values()
            .all(ordinary_template_boundary_type_inner),
        Type::List(item) | Type::Set(item) => ordinary_template_boundary_type_inner(item),
        Type::Map { key, value } => {
            ordinary_template_boundary_type_inner(key)
                && ordinary_template_boundary_type_inner(value)
        }
        Type::Union(members) => {
            !members.is_empty() && members.iter().all(ordinary_template_boundary_type_inner)
        }
        // A retained definition is a template. Its FunctionParameter leaves
        // are bound by each SemanticExpressionKind::Call occurrence, whose
        // arguments and result flow type are the compact invocation overlay.
        // Open checked boundary types therefore do not require a cloned body;
        // concrete resource/effect ownership remains excluded independently.
        Type::UnresolvedShape { .. } | Type::Var(_) | Type::Unknown => true,
        Type::RenderContract => true,
        Type::Function { .. } => false,
    }
}

fn enclosing_function_owner(
    program: &CheckedProgramFields,
    lookup: &CheckedProgramLookup,
    mut scope: boon_checked::LexicalScopeId,
) -> Option<DeclId> {
    let mut visited = BTreeSet::new();
    while visited.insert(scope) {
        let definition = lookup.scope(program, scope)?;
        if definition.kind == boon_checked::CheckedScopeKind::Function {
            return definition.owner;
        }
        scope = definition.parent?;
    }
    None
}

fn ordinary_callable_base_candidate(
    program: &CheckedProgramFields,
    callable: &boon_checked::CheckedCallableSignature,
) -> bool {
    ordinary_callable_base_rejection(program, callable).is_none()
}

fn ordinary_callable_base_rejection(
    program: &CheckedProgramFields,
    callable: &boon_checked::CheckedCallableSignature,
) -> Option<&'static str> {
    if callable.kind != CheckedCallableKind::User {
        return Some("not_user");
    }
    if callable.effect != boon_checked::CheckedEffectSummary::default() {
        return Some("effectful");
    }
    if !callable.contexts.is_empty() {
        return Some("element_context");
    }
    if callable.contextual_operation.is_some() {
        return Some("contextual_operation");
    }
    if let Some(formal) = callable.context_formal {
        let Some(formal) = program.context_formal(formal) else {
            return Some("missing_context_formal");
        };
        if !ordinary_template_boundary_type(&formal.scheme.flow_type.ty) {
            return Some("context_type");
        }
    }
    if callable.result_expression.is_none() {
        return Some("no_result");
    }
    if !ordinary_template_boundary_type(&callable.result.ty) {
        return Some("result_type");
    }
    if callable.parameters.iter().any(|parameter| {
        parameter.kind != CheckedParameterKind::Value
            || parameter.evaluation_scope != boon_checked::CheckedEvaluationScope::Parent
            || !matches!(parameter.requirement, CheckedParameterRequirement::Required)
    }) {
        return Some("parameter_contract");
    }
    if callable
        .parameters
        .iter()
        .any(|parameter| !ordinary_template_boundary_type(&parameter.flow_type.ty))
    {
        return Some("parameter_type");
    }
    None
}

fn ordinary_callable_body_dependencies(
    program: &CheckedProgramFields,
    lookup: &CheckedProgramLookup,
    callable: &boon_checked::CheckedCallableSignature,
    candidates: &BTreeSet<DeclId>,
) -> Option<BTreeSet<DeclId>> {
    let Some(root) = callable.result_expression else {
        return None;
    };
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    let mut dependencies = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        if program
            .resource_projection_requirements
            .iter()
            .any(|requirement| {
                requirement.expression == expression_id && !requirement.source_origins.is_empty()
            })
        {
            return None;
        }
        let Some(expression) = lookup.expression(program, expression_id) else {
            return None;
        };
        if expression.effect != boon_checked::CheckedEffectSummary::default() {
            return None;
        }
        match &expression.kind {
            CheckedExpressionKind::Read { target, source, .. } => {
                if source.is_some()
                    || enclosing_function_owner(program, lookup, expression.scope_id)
                        != Some(callable.decl_id)
                {
                    return None;
                }
                let Some(declaration) = lookup.declaration(program, *target) else {
                    return None;
                };
                // Element-state reads need a call-context placeholder in the
                // retained body. Constructor-local contexts are overlaid
                // below; keep state-reading definitions specialized until the
                // corresponding value placeholder is represented explicitly.
                if declaration.kind == CheckedDeclarationKind::ElementState {
                    return None;
                }
                match enclosing_function_owner(program, lookup, declaration.scope_id) {
                    Some(owner) if owner == callable.decl_id => {
                        if let Some(value) = declaration.value
                            && !callable
                                .parameters
                                .iter()
                                .any(|parameter| parameter.decl_id == *target)
                        {
                            pending.push(value);
                        }
                    }
                    None if canonical_declaration_path(program, lookup, *target).is_some() => {
                        // Program-root values already have stable canonical
                        // read authority in the shared semantic body. They are
                        // not lexical captures and must not force one body copy
                        // per call occurrence.
                    }
                    Some(_) | None => return None,
                }
            }
            CheckedExpressionKind::Call { call } => {
                let Some(call) = lookup.call(program, *call) else {
                    return None;
                };
                if call
                    .entries
                    .iter()
                    .any(|entry| !matches!(entry, CheckedCallEntry::Input { .. }))
                {
                    return None;
                }
                let Some(target) = lookup.callable(program, call.callable) else {
                    return None;
                };
                let context_matches = match target.context_formal {
                    None => matches!(call.context_binding, CheckedContextBinding::None),
                    Some(_) => matches!(
                        call.context_binding,
                        CheckedContextBinding::Explicit { .. }
                            | CheckedContextBinding::Inherited { .. }
                    ),
                };
                if !context_matches {
                    return None;
                }
                match target.kind {
                    CheckedCallableKind::Builtin
                        if target.effect == boon_checked::CheckedEffectSummary::default()
                            && ((target.contexts.is_empty()
                                && target.context_formal.is_none())
                                || boon_checked::is_registered_render_constructor(
                                    &call.function,
                                )) => {}
                    CheckedCallableKind::User if candidates.contains(&target.decl_id) => {
                        dependencies.insert(target.decl_id);
                    }
                    CheckedCallableKind::User
                    | CheckedCallableKind::Builtin
                    | CheckedCallableKind::External => return None,
                }
                pending.extend(call.entries.iter().filter_map(|entry| match entry {
                    CheckedCallEntry::Input { value, .. } => Some(*value),
                    CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => None,
                }));
            }
            CheckedExpressionKind::TextTemplate { segments } => {
                pending.extend(segments.iter().filter_map(|segment| match segment {
                    CheckedTextSegment::Dynamic { value } => Some(*value),
                    CheckedTextSegment::Static { .. } => None,
                }))
            }
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields } => {
                pending.extend(fields.iter().map(|field| field.value));
            }
            CheckedExpressionKind::Flush { payload } => pending.push(*payload),
            CheckedExpressionKind::When { input, arms } => {
                pending.push(*input);
                pending.extend(arms.iter().copied());
            }
            CheckedExpressionKind::Infix { left, right, .. }
            | CheckedExpressionKind::MapEntry {
                key: left,
                value: right,
            } => {
                pending.push(*left);
                pending.push(*right);
            }
            CheckedExpressionKind::MatchArm { output, .. } => {
                pending.extend(*output);
                pending.extend(ordinary_statement_child_values(
                    program,
                    lookup,
                    expression_id,
                ));
            }
            CheckedExpressionKind::Block { bindings, result } => {
                pending.extend(bindings.iter().map(|binding| binding.value));
                pending.extend(*result);
            }
            CheckedExpressionKind::List { items, .. }
            | CheckedExpressionKind::Bytes { items, .. }
            | CheckedExpressionKind::Map { entries: items }
            | CheckedExpressionKind::Set { items } => pending.extend(items.iter().copied()),
            CheckedExpressionKind::Passed {
                formal,
                access: CheckedPassedAccess::Read,
                ..
            } if callable.context_formal == Some(*formal) => {}
            CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::Bits { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Absent
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::Delimiter => {}
            CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Draining { .. }
            | CheckedExpressionKind::Hold { .. }
            | CheckedExpressionKind::Latest { .. }
            | CheckedExpressionKind::While { .. }
            | CheckedExpressionKind::Then { .. }
            | CheckedExpressionKind::Invalid { .. } => return None,
        }
    }
    Some(dependencies)
}

/// Return expression values represented by the statement tree beneath a
/// structural expression. In list-shaped `WHEN` arms the checked expression's
/// direct output is a delimiter; the actual field/spread values live in the
/// arm statement's children and are part of the callable body just as much as
/// an ordinary object expression's fields are.
fn ordinary_statement_child_values(
    program: &CheckedProgramFields,
    lookup: &CheckedProgramLookup,
    parent_expression: CheckedExprId,
) -> Vec<CheckedExprId> {
    let Some(statement) = lookup
        .statement_indices_for_value(parent_expression)
        .iter()
        .filter_map(|index| program.statements.get(*index))
        .find(|statement| {
            statement.value == Some(parent_expression) && !statement.children.is_empty()
        })
    else {
        return Vec::new();
    };
    let mut pending = statement.children.iter().rev().copied().collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut values = Vec::new();
    while let Some(statement) = pending.pop() {
        if !visited.insert(statement) {
            continue;
        }
        let Some(statement) = lookup.statement(program, statement) else {
            continue;
        };
        match statement.value {
            Some(value) if value == parent_expression => {
                pending.extend(statement.children.iter().rev().copied());
            }
            Some(value) => values.push(value),
            None => pending.extend(statement.children.iter().rev().copied()),
        }
    }
    values
}

pub(crate) fn ordinary_callable_declarations(program: &CheckedProgramFields) -> BTreeSet<DeclId> {
    let trace = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
    let started = trace.then(std::time::Instant::now);
    let lookup = CheckedProgramLookup::new(program);
    let base_candidates = program
        .callables
        .iter()
        .filter(|callable| ordinary_callable_base_candidate(program, callable))
        .map(|callable| callable.decl_id)
        .collect::<BTreeSet<_>>();
    let mut dependents = BTreeMap::<DeclId, BTreeSet<DeclId>>::new();
    let mut pending_rejections = Vec::new();
    for callable in program
        .callables
        .iter()
        .filter(|callable| base_candidates.contains(&callable.decl_id))
    {
        match ordinary_callable_body_dependencies(program, &lookup, callable, &base_candidates) {
            Some(dependencies) => {
                for dependency in dependencies {
                    dependents
                        .entry(dependency)
                        .or_default()
                        .insert(callable.decl_id);
                }
            }
            None => pending_rejections.push(callable.decl_id),
        }
    }
    let direct_rejection_count = pending_rejections.len();
    let mut retained = base_candidates.clone();
    while let Some(rejected) = pending_rejections.pop() {
        if !retained.remove(&rejected) {
            continue;
        }
        pending_rejections.extend(
            dependents
                .get(&rejected)
                .into_iter()
                .flat_map(|callers| callers.iter().copied()),
        );
    }
    if trace {
        let mut rejections = BTreeMap::<&'static str, Vec<String>>::new();
        for callable in program
            .callables
            .iter()
            .filter(|callable| callable.kind == CheckedCallableKind::User)
        {
            let reason = ordinary_callable_base_rejection(program, callable)
                .or_else(|| (!retained.contains(&callable.decl_id)).then_some("body_not_closed"));
            if let Some(reason) = reason {
                rejections
                    .entry(reason)
                    .or_default()
                    .push(callable.name.clone());
            }
        }
        eprintln!(
            "boon_semantic ordinary_callable_analysis base={} direct_rejections={} propagated_rejections={} retained={} elapsed_ms={:.3}",
            base_candidates.len(),
            direct_rejection_count,
            base_candidates
                .len()
                .saturating_sub(retained.len())
                .saturating_sub(direct_rejection_count),
            retained.len(),
            started
                .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
        );
        eprintln!(
            "boon_semantic ordinary_callable_rejections {:?}",
            rejections
                .into_iter()
                .map(|(reason, names)| {
                    let count = names.len();
                    let sample = names.into_iter().take(12).collect::<Vec<_>>();
                    (reason, count, sample)
                })
                .collect::<Vec<_>>()
        );
    }
    retained
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StaticSelectorValue {
    Number(boon_data::ExactNumber),
    Text(String),
    Tag(String),
    Bits(boon_data::Bits),
}

impl StaticSelectorValue {
    fn matches(&self, pattern: &CheckedMatchPattern) -> bool {
        match (self, pattern) {
            (_, CheckedMatchPattern::Wildcard | CheckedMatchPattern::Binding { .. }) => true,
            (Self::Number(actual), CheckedMatchPattern::Number { value }) => actual == value,
            (Self::Text(actual), CheckedMatchPattern::Text { value }) => actual == value,
            (Self::Tag(actual), CheckedMatchPattern::Tag { name, .. }) => actual == name,
            (Self::Bits(actual), CheckedMatchPattern::Bits { value }) => actual == value,
            _ => false,
        }
    }
}

impl SemanticExpressionBuilderIndexes {
    fn new(
        program: &CheckedProgramFields,
        out_net: &OutNet,
        retained_ordinary_declarations: &BTreeSet<DeclId>,
        lookup: &CheckedProgramLookup,
    ) -> Result<Self, ExpansionError> {
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
        let ordinary_callable_ids = retained_ordinary_declarations
            .iter()
            .copied()
            .filter_map(|callable| callable_ids.get(&callable).copied())
            .collect();
        let completed_context_formals = completed_context_formal_flow_types(program, lookup)?;
        let hold_owners: BTreeMap<CheckedExprId, DeclId> = program
            .states
            .iter()
            .filter(|state| state.kind == CheckedStateKind::Hold)
            .filter_map(|state| {
                let expression = lookup.expression(program, state.expression)?;
                enclosing_function_owner(program, lookup, expression.scope_id)
                    .map(|owner| (state.expression, owner))
            })
            .collect();
        let callables_with_holds = hold_owners.values().copied().collect();
        Ok(Self {
            callable_ids,
            call_ids,
            producer_callable_ids,
            ordinary_callable_ids,
            completed_context_formals,
            hold_owners,
            callables_with_holds,
        })
    }
}

pub(crate) struct SemanticExpressionBuilder<'a> {
    program: &'a CheckedProgramFields,
    lookup: &'a CheckedProgramLookup,
    out_net: &'a OutNet,
    indexes: &'a SemanticExpressionBuilderIndexes,
    locals: BTreeMap<OutNetId, (StaticOwnerId, SemanticMaterializationLocalId)>,
    inherited_local_types: &'a BTreeMap<StaticOwnerId, Type>,
    local_types: BTreeMap<(StaticOwnerId, SemanticMaterializationLocalId), Type>,
    materializations_by_owner: &'a BTreeMap<StaticOwnerId, SemanticMaterializationId>,
    materialization_result_types: &'a BTreeMap<SemanticMaterializationId, Type>,
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
    ordinary_definition_scheduled: BTreeSet<SemanticCallableId>,
    ordinary_definition_order: Vec<SemanticCallableId>,
    ordinary_definition_roots: BTreeMap<SemanticCallableId, SemanticExprId>,
    current_ordinary_definition: Option<SemanticCallableId>,
    retain_ordinary_calls: bool,
    trace_expression_milestone: Option<usize>,
}

impl<'a> SemanticExpressionBuilder<'a> {
    fn new(
        program: &'a CheckedProgramFields,
        lookup: &'a CheckedProgramLookup,
        out_net: &'a OutNet,
        indexes: &'a SemanticExpressionBuilderIndexes,
        locals: BTreeMap<OutNetId, (StaticOwnerId, SemanticMaterializationLocalId)>,
        inherited_local_types: &'a BTreeMap<StaticOwnerId, Type>,
        materializations_by_owner: &'a BTreeMap<StaticOwnerId, SemanticMaterializationId>,
        materialization_result_types: &'a BTreeMap<SemanticMaterializationId, Type>,
    ) -> Self {
        Self {
            program,
            lookup,
            out_net,
            indexes,
            locals,
            inherited_local_types,
            local_types: BTreeMap::new(),
            materializations_by_owner,
            materialization_result_types,
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
            ordinary_definition_scheduled: BTreeSet::new(),
            ordinary_definition_order: Vec::new(),
            ordinary_definition_roots: BTreeMap::new(),
            current_ordinary_definition: None,
            retain_ordinary_calls: false,
            trace_expression_milestone: std::env::var_os("BOON_SEMANTIC_TRACE")
                .is_some()
                .then_some(100_000),
        }
    }

    fn enable_ordinary_call_boundaries(&mut self) {
        self.retain_ordinary_calls = true;
    }

    fn schedule_ordinary_definition(&mut self, callable: SemanticCallableId) {
        if self.ordinary_definition_scheduled.insert(callable) {
            self.ordinary_definition_order.push(callable);
        }
    }

    fn expand_pending_ordinary_definitions(&mut self) -> Result<(), ExpansionError> {
        let mut next = 0;
        while next < self.ordinary_definition_order.len() {
            let semantic_callable = self.ordinary_definition_order[next];
            next += 1;
            if self
                .ordinary_definition_roots
                .contains_key(&semantic_callable)
            {
                continue;
            }
            let callable = self
                .program
                .callables
                .get(semantic_callable.as_usize())
                .cloned()
                .ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "ordinary semantic callable {semantic_callable} has no checked definition"
                    ))
                })?;
            let checked_callable = callable.decl_id;
            if self.indexes.callable_ids.get(&checked_callable).copied() != Some(semantic_callable)
            {
                return Err(ExpansionError::InvalidLocalBindings(format!(
                    "ordinary callable {semantic_callable} has stale checked identity {checked_callable:?}"
                )));
            }
            let checked_root = callable
                .result_expression
                .ok_or(ExpansionError::MissingFunctionResult(checked_callable))?;
            let previous_definition = self.current_ordinary_definition.replace(semantic_callable);
            let root = self.expand_with_inherited_owner(
                ScopedCheckedExpr {
                    expression: checked_root,
                    frame: None,
                    evaluation_port: None,
                    value_frame: None,
                },
                None,
            );
            self.current_ordinary_definition = previous_definition;
            let root = root?;
            let boundary_expression = self.flush_boundary_origin_for_value(checked_root, root);
            let root = self.wrap_flush_boundary(boundary_expression, root, None)?;
            self.ordinary_definition_roots
                .insert(semantic_callable, root);
        }
        Ok(())
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
                ..
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
            next_local_binding: self.next_local_binding,
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
        scope: boon_checked::LexicalScopeId,
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
                        declaration.kind == boon_checked::CheckedDeclarationKind::ElementState
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
                            let projected =
                                self.project(&expression, owner, input, binding_fields)?;
                            let required = concrete_type_in_frame(
                                self.out_net,
                                &expression.flow_type.ty,
                                scoped.frame,
                            );
                            Ok(self.wrap_type_refinement(&expression, owner, projected, required))
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
                        .or_else(|| {
                            (local == SemanticMaterializationLocalId(0))
                                .then(|| self.inherited_local_types.get(&local_owner))
                                .flatten()
                        })
                        .cloned()
                        .and_then(|ty| project_concrete_type(ty, &projection));
                    let constructor_projection = projection.clone();
                    let local_expression = self.push(
                        &expression,
                        owner,
                        SemanticExpressionKind::MaterializationLocal {
                            owner: local_owner,
                            local,
                            projection,
                            constructor_projection,
                        },
                    );
                    if let Some(ty) = local_type {
                        self.expressions[local_expression.as_usize()].flow_type.ty =
                            erase_runtime_type_vars(&ty);
                    }
                    return Ok(local_expression);
                }
                if let Some(semantic_callable) = self.current_ordinary_definition {
                    if let Some(parameter) = self
                        .program
                        .callables
                        .get(semantic_callable.as_usize())
                        .and_then(|callable| {
                            callable
                                .parameters
                                .iter()
                                .find(|parameter| parameter.decl_id == target)
                        })
                    {
                        let mut flow_type = parameter.flow_type.clone();
                        flow_type.ty = project_concrete_type(flow_type.ty, &projection)
                            .ok_or_else(|| {
                                ExpansionError::InvalidLocalBindings(format!(
                                    "ordinary callable {semantic_callable} parameter {} cannot project checked fields {:?}",
                                    parameter.ordinal, projection
                                ))
                            })?;
                        let parameter = self.push(
                            &expression,
                            owner,
                            SemanticExpressionKind::FunctionParameter {
                                parameter: semantic_parameter_id(
                                    semantic_callable,
                                    parameter.ordinal,
                                ),
                                projection,
                            },
                        );
                        self.expressions[parameter.as_usize()].flow_type = flow_type;
                        return Ok(parameter);
                    }
                }
                if let Some((frame, actual, argument_owner)) = scoped.frame.and_then(|frame| {
                    self.out_net.call_instances[frame.as_usize()]
                        .inputs
                        .iter()
                        .find(|binding| binding.formal == target)
                        .map(|binding| {
                            (
                                frame,
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
                            let instance = &self.out_net.call_instances[frame.as_usize()];
                            if let Some(parameter) = self
                                .lookup
                                .callable(self.program, instance.provenance.callable)
                                .and_then(|callable| {
                                    callable
                                        .parameters
                                        .iter()
                                        .find(|parameter| parameter.decl_id == target)
                                })
                            {
                                let required = concrete_type_in_frame(
                                    self.out_net,
                                    &parameter.flow_type.ty,
                                    Some(frame),
                                );
                                let existing =
                                    self.expressions[expanded.as_usize()].flow_type.ty.clone();
                                let refined = refine_runtime_call_boundary_type(
                                    &existing,
                                    &parameter.flow_type.ty,
                                    &required,
                                )
                                .map_err(|error| {
                                    let expanded = &self.expressions[expanded.as_usize()];
                                    let checked_call = instance
                                        .provenance
                                        .call_id
                                        .and_then(|call| self.lookup.call(self.program, call));
                                    let checked_expression = self
                                        .lookup
                                        .expression(self.program, expanded.checked_expr_id);
                                    ExpansionError::InvalidLocalBindings(format!(
                                        "call frame {frame} {:?} function {:?} formal {} \
                                                 `{}` scheme \
                                                 {:?} cannot refine expression {} checked {:?} \
                                                 checked definition {:?} kind {:?} flow {:?}: \
                                                 {error}",
                                        instance.provenance,
                                        checked_call.map(|call| call.function.as_str()),
                                        target.0,
                                        parameter.name,
                                        parameter.flow_type,
                                        expanded.id,
                                        expanded.checked_expr_id,
                                        checked_expression,
                                        expanded.kind,
                                        expanded.flow_type
                                    ))
                                })?;
                                let expanded = self.wrap_type_refinement(
                                    &expression,
                                    owner,
                                    expanded,
                                    refined,
                                );
                                return self.project(&expression, owner, expanded, projection);
                            }
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
                                .indexes
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
                if declaration.kind == boon_checked::CheckedDeclarationKind::PatternBinding {
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
                if declaration.kind == boon_checked::CheckedDeclarationKind::Field
                    && declaration_is_function_local(self.program, declaration.scope_id)
                    && !self.program.states.iter().any(|state| {
                        state.declaration == declaration.id
                            || declaration.value == Some(state.expression)
                    })
                    && !self
                        .program
                        .sources
                        .iter()
                        .any(|source| source.declaration == declaration.id)
                    && declaration.value.is_some()
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
                if let Some(semantic_callable) = self.current_ordinary_definition {
                    if access != CheckedPassedAccess::Read {
                        return Err(ExpansionError::InvalidPassedDrainTarget(scoped.expression));
                    }
                    let callable = self
                        .program
                        .callables
                        .get(semantic_callable.as_usize())
                        .ok_or_else(|| {
                            ExpansionError::InvalidLocalBindings(format!(
                                "ordinary semantic callable {semantic_callable} has no checked definition"
                            ))
                        })?;
                    if callable.context_formal != Some(formal) {
                        return Err(ExpansionError::MismatchedPassedFormal {
                            expression: scoped.expression,
                            expected: callable.context_formal.unwrap_or(formal),
                            found: formal,
                        });
                    }
                    let mut flow_type = self
                        .indexes
                        .completed_context_formals
                        .get(&formal)
                        .cloned()
                        .ok_or_else(|| {
                            ExpansionError::InvalidLocalBindings(format!(
                                "ordinary callable {semantic_callable} has missing PASSED formal {}",
                                formal.0
                            ))
                        })?;
                    flow_type.ty = project_concrete_type(flow_type.ty, &projection).ok_or_else(
                        || {
                            ExpansionError::InvalidLocalBindings(format!(
                                "ordinary callable {semantic_callable} PASSED parameter cannot project checked fields {:?}",
                                projection
                            ))
                        },
                    )?;
                    let parameter = self.push(
                        &expression,
                        owner,
                        SemanticExpressionKind::FunctionParameter {
                            parameter: semantic_parameter_id(
                                semantic_callable,
                                callable.parameters.len(),
                            ),
                            projection,
                        },
                    );
                    self.expressions[parameter.as_usize()].flow_type = flow_type;
                    return Ok(parameter);
                }
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
                let arms = self
                    .statically_selected_arm(input, &arms)
                    .map_or(arms, |selected| vec![selected]);
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
                        let value = self.wrap_flush_boundary(boundary_expression, value, owner)?;
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
                let result = self.wrap_flush_boundary(boundary_expression, result, owner)?;
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
            .call_instance_for_checked_call(call_id, scoped.frame);
        let has_out = checked_call
            .entries
            .iter()
            .any(|entry| !matches!(entry, CheckedCallEntry::Input { .. }));
        let retained_user_call = self.retain_ordinary_calls
            && callable.kind == CheckedCallableKind::User
            // The call node is the invocation overlay. Its exact occurrence
            // result and type substitutions select the same static branch
            // when the shared body is lowered, so syntax-discriminated pure
            // calls do not require a cloned semantic body.
            && self
                .indexes
                .callable_ids
                .get(&callable.decl_id)
                .is_some_and(|callable| self.indexes.ordinary_callable_ids.contains(callable));
        let instance_less_ordinary_call = instance.is_none()
            && self.current_ordinary_definition.is_some()
            && !has_out
            && callable.effect == boon_checked::CheckedEffectSummary::default()
            && match callable.kind {
                CheckedCallableKind::User => retained_user_call,
                CheckedCallableKind::Builtin => {
                    (callable.contexts.is_empty() && callable.context_formal.is_none())
                        || boon_checked::is_registered_render_constructor(&checked_call.function)
                }
                CheckedCallableKind::External => false,
            };
        if instance.is_none() && !instance_less_ordinary_call {
            if std::env::var_os("BOON_SEMANTIC_TRACE").is_some() {
                eprintln!(
                    "boon_semantic missing_call_instance checked_call={} function={} owner={:?} expression={} span={:?} frame={:?} frame_provenance={:?} current_ordinary={:?} retained_user_call={} has_out={} call_contexts={} callable_effect={:?} callable_context_formal={:?}",
                    call_id.0,
                    checked_call.function,
                    checked_call.owner_callable,
                    checked_call.expression.0,
                    checked_call.span,
                    scoped.frame,
                    scoped.frame.and_then(|frame| self
                        .out_net
                        .call_instances
                        .get(frame.as_usize())
                        .map(|instance| instance.provenance)),
                    self.current_ordinary_definition,
                    retained_user_call,
                    has_out,
                    checked_call.contexts.len(),
                    callable.effect,
                    callable.context_formal,
                );
            }
            return Err(ExpansionError::MissingCallInstance {
                call: call_id,
                frame: scoped.frame,
            });
        }
        if has_out {
            let instance = instance.ok_or(ExpansionError::MissingCallInstance {
                call: call_id,
                frame: scoped.frame,
            })?;
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
        if callable.kind == CheckedCallableKind::User && !retained_user_call {
            let instance = instance.ok_or(ExpansionError::MissingCallInstance {
                call: call_id,
                frame: scoped.frame,
            })?;
            let result_expression = callable
                .result_expression
                .ok_or(ExpansionError::MissingFunctionResult(callable.decl_id))?;
            let call_owner = self.out_net.owner_for_call(instance).or(owner);
            let concrete_result = self.out_net.call_instances[instance.as_usize()]
                .result
                .clone();
            let result = self.expand_with_inherited_owner(
                ScopedCheckedExpr {
                    expression: result_expression,
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
            let existing_result = self.expressions[result.as_usize()].flow_type.clone();
            let refined_result_type = refine_runtime_call_boundary_type(
                &existing_result.ty,
                &callable.result.ty,
                &concrete_result.ty,
            )
            .map_err(|error| {
                let call_instance = &self.out_net.call_instances[instance.as_usize()];
                let checked_call = call_instance
                    .provenance
                    .call_id
                    .and_then(|call| self.lookup.call(self.program, call));
                let checked_result = self.lookup.expression(self.program, result_expression);
                let expanded_result = &self.expressions[result.as_usize()];
                ExpansionError::InvalidLocalBindings(format!(
                    "user call {instance} provenance {:?} function {:?} result expression \
                             {result} checked {:?} expanded kind {:?} flow {:?} cannot refine its \
                             occurrence type to {:?}: {error}",
                    call_instance.provenance,
                    checked_call.map(|call| call.function.as_str()),
                    checked_result,
                    expanded_result.kind,
                    expanded_result.flow_type,
                    concrete_result,
                ))
            })?;
            self.expressions[result.as_usize()].flow_type = FlowType {
                mode: concrete_result.mode,
                ty: refined_result_type,
            };
            let frame_callable = self.out_net.call_instances[instance.as_usize()]
                .provenance
                .callable;
            if frame_callable != callable.decl_id {
                return Err(ExpansionError::InvalidLocalBindings(format!(
                    "user call {instance} expands callable {} through a frame owned by callable {}",
                    callable.decl_id.0, frame_callable.0,
                )));
            }
            if self.indexes.callables_with_holds.contains(&frame_callable) {
                let occurrence_result_type =
                    self.expressions[result.as_usize()].flow_type.ty.clone();
                refine_call_result_state_occurrences(
                    &mut self.expressions,
                    &self.checked_expression_origins,
                    &self.indexes.hold_owners,
                    result,
                    instance,
                    frame_callable,
                    &occurrence_result_type,
                )
                .map_err(ExpansionError::InvalidLocalBindings)?;
            }
            let boundary_expression =
                self.flush_boundary_origin_for_value(result_expression, result);
            return self.wrap_flush_boundary(boundary_expression, result, call_owner);
        }
        if !retained_user_call
            && !matches!(checked_call.context_binding, CheckedContextBinding::None)
        {
            return Err(ExpansionError::PassOnNonexpandedCall(call_id));
        }
        let mut arguments = Vec::new();
        if let Some(instance) = instance {
            let inputs = self.out_net.call_instances[instance.as_usize()]
                .inputs
                .clone();
            let argument_owner = self.out_net.owner_for_call_evaluation(instance);
            arguments.reserve(inputs.len());
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
                // OUT topology is derived before semantic BLOCK frames exist,
                // so inherit the call site's lexical value frame.
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
        } else {
            arguments.reserve(checked_call.entries.len());
            for entry in &checked_call.entries {
                let CheckedCallEntry::Input {
                    formal,
                    value,
                    from_pipe,
                    ..
                } = entry
                else {
                    return Err(ExpansionError::MissingCallInstance {
                        call: call_id,
                        frame: scoped.frame,
                    });
                };
                let parameter = callable
                    .parameters
                    .iter()
                    .find(|parameter| parameter.decl_id == *formal)
                    .ok_or(ExpansionError::MissingFormal {
                        callable: callable.decl_id,
                        formal: *formal,
                    })?;
                arguments.push(SemanticCallArgument {
                    formal: *formal,
                    ordinal: parameter.ordinal,
                    name: parameter.name.clone(),
                    checked_value: *value,
                    value: self.expand_in_frame(*value, scoped.frame, scoped.value_frame)?,
                    from_pipe: *from_pipe,
                });
            }
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
        let context_argument = if retained_user_call {
            match (callable.context_formal, instance) {
                (Some(formal), Some(instance)) => {
                    let passed = self.out_net.call_instances[instance.as_usize()]
                        .passed
                        .ok_or(ExpansionError::MissingPassedContext(expression.id))?;
                    if passed.formal != formal {
                        return Err(ExpansionError::MismatchedPassedFormal {
                            expression: expression.id,
                            expected: formal,
                            found: passed.formal,
                        });
                    }
                    let argument_owner = self
                        .out_net
                        .owner_for_call_evaluation(passed.evaluation_call);
                    let value = self.expand_with_inherited_owner(passed.value, argument_owner)?;
                    Some(SemanticCallContextArgument {
                        formal,
                        checked_value: Some(passed.value.expression),
                        value: self
                            .capture_ordinary_context_argument(expression, owner, formal, value)?,
                    })
                }
                (Some(formal), None) => match checked_call.context_binding {
                    CheckedContextBinding::Explicit { value, .. } => {
                        let checked_value = value;
                        let value =
                            self.expand_in_frame(value, scoped.frame, scoped.value_frame)?;
                        Some(SemanticCallContextArgument {
                            formal,
                            checked_value: Some(checked_value),
                            value: self.capture_ordinary_context_argument(
                                expression, owner, formal, value,
                            )?,
                        })
                    }
                    CheckedContextBinding::Inherited {
                        formal: inherited_formal,
                    } => {
                        let current_callable = self
                            .current_ordinary_definition
                            .ok_or(ExpansionError::MissingPassedContext(expression.id))?;
                        let current_definition = self
                            .program
                            .callables
                            .get(current_callable.as_usize())
                            .ok_or_else(|| {
                                ExpansionError::InvalidLocalBindings(format!(
                                    "ordinary semantic callable {current_callable} has no checked definition"
                                ))
                            })?;
                        if current_definition.context_formal != Some(inherited_formal) {
                            return Err(ExpansionError::MismatchedPassedFormal {
                                expression: expression.id,
                                expected: current_definition
                                    .context_formal
                                    .unwrap_or(inherited_formal),
                                found: inherited_formal,
                            });
                        }
                        let flow_type = self
                            .program
                            .context_formal(inherited_formal)
                            .map(|formal| formal.scheme.flow_type.clone())
                            .ok_or(ExpansionError::MissingPassedContext(expression.id))?;
                        let value = self.push(
                            expression,
                            owner,
                            SemanticExpressionKind::FunctionParameter {
                                parameter: semantic_parameter_id(
                                    current_callable,
                                    current_definition.parameters.len(),
                                ),
                                projection: Vec::new(),
                            },
                        );
                        self.expressions[value.as_usize()].flow_type = flow_type;
                        Some(SemanticCallContextArgument {
                            formal,
                            checked_value: None,
                            value: self.capture_ordinary_context_argument(
                                expression, owner, formal, value,
                            )?,
                        })
                    }
                    CheckedContextBinding::None => {
                        return Err(ExpansionError::MissingPassedContext(expression.id));
                    }
                },
                (None, _) => None,
            }
        } else {
            None
        };
        let kind = match callable.kind {
            CheckedCallableKind::User if retained_user_call => SemanticCallableKind::User,
            CheckedCallableKind::Builtin => SemanticCallableKind::Builtin,
            CheckedCallableKind::External => SemanticCallableKind::External,
            CheckedCallableKind::User => unreachable!("specialized user calls are expanded above"),
        };
        let contexts = match instance {
            Some(instance) => checked_call
                .contexts
                .iter()
                .map(|context| SemanticCallContextId {
                    call_instance: instance,
                    ordinal: context.signature,
                })
                .collect(),
            // A retained definition carries only checked context ordinals.
            // The invocation overlay resolves them to concrete call-instance
            // contexts during executable lowering.
            None => Vec::new(),
        };
        let semantic_call = self
            .indexes
            .call_ids
            .get(&call_id)
            .copied()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "checked call {} has no semantic call identity",
                    call_id.0
                ))
            })?;
        let semantic_callable = self
            .indexes
            .callable_ids
            .get(&checked_call.callable)
            .copied()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "checked call {} callable {} has no semantic identity",
                    call_id.0, checked_call.callable.0
                ))
            })?;
        if retained_user_call {
            self.schedule_ordinary_definition(semantic_callable);
        }
        let call_expression = self.push(
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
                context_argument,
                contexts,
            },
        );
        if retained_user_call {
            let checked_root = callable
                .result_expression
                .ok_or(ExpansionError::MissingFunctionResult(callable.decl_id))?;
            let boundary_expression =
                self.flush_boundary_origin_for_value(checked_root, call_expression);
            self.wrap_flush_boundary(boundary_expression, call_expression, owner)
        } else {
            Ok(call_expression)
        }
    }

    fn capture_ordinary_context_argument(
        &mut self,
        origin: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        formal: ContextFormalId,
        actual: SemanticExprId,
    ) -> Result<SemanticExprId, ExpansionError> {
        let scheme = self
            .indexes
            .completed_context_formals
            .get(&formal)
            .cloned()
            .ok_or(ExpansionError::MissingPassedContext(origin.id))?;
        self.capture_ordinary_context_type(
            origin,
            owner,
            actual,
            &mut Vec::new(),
            &scheme.ty,
            scheme.mode,
        )
    }

    fn capture_ordinary_context_type(
        &mut self,
        origin: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        actual: SemanticExprId,
        path: &mut Vec<String>,
        required: &Type,
        mode: FlowMode,
    ) -> Result<SemanticExprId, ExpansionError> {
        let Type::Object(shape) = required else {
            return self.project(origin, owner, actual, path.clone());
        };
        if shape.fields.is_empty() {
            return self.project(origin, owner, actual, path.clone());
        }

        let mut names = Vec::with_capacity(shape.fields.len());
        let mut seen = BTreeSet::new();
        for name in shape.field_order.iter().chain(shape.fields.keys()) {
            if shape.fields.contains_key(name) && seen.insert(name.clone()) {
                names.push(name.clone());
            }
        }
        let mut fields = Vec::with_capacity(names.len());
        for name in names {
            let field_type = &shape.fields[&name];
            path.push(name.clone());
            let value =
                self.capture_ordinary_context_type(origin, owner, actual, path, field_type, mode)?;
            path.pop();
            fields.push(SemanticRecordField {
                declaration: None,
                name,
                value,
                spread: false,
            });
        }
        let captured = self.push(origin, owner, SemanticExpressionKind::Object(fields));
        self.expressions[captured.as_usize()].flow_type = FlowType {
            mode,
            ty: erase_runtime_type_vars(required),
        };
        Ok(captured)
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
        fields: Vec<boon_checked::CheckedRecordField>,
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
        let Some(SemanticExpressionKind::CanonicalRead {
            target, projection, ..
        }) = self
            .expressions
            .get(semantic_expression.as_usize())
            .map(|expression| &expression.kind)
        else {
            return checked_expression;
        };
        if !projection.is_empty() {
            // The target's FLUSH payload belongs to the whole named value.
            // A projected read carries only the successful member type; it
            // must not widen that member with a sibling whole-record control
            // payload. Runtime FLUSH already prevents the target authority
            // from committing on the failing path.
            return checked_expression;
        }
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
    ) -> Result<SemanticExprId, ExpansionError> {
        let origin = self
            .lookup
            .expression(self.program, checked_expression)
            .cloned()
            .ok_or(ExpansionError::MissingExpression(checked_expression))?;
        let Some(flush_type) = origin.flush_type.clone() else {
            return Ok(input);
        };
        let mut flow_type = self
            .expressions
            .get(input.as_usize())
            .filter(|expression| expression.id == input)
            .map(|expression| expression.flow_type.clone())
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "FLUSH boundary for checked expression {} references missing semantic input {input}",
                    checked_expression.0,
                ))
            })?;
        flow_type = runtime_flush_boundary_flow_type(flow_type, flush_type);
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
        let selector_binding = self
            .lookup
            .expression(self.program, input)
            .and_then(|selector| match &selector.kind {
                CheckedExpressionKind::Read {
                    target, projection, ..
                } if projection.is_empty() => Some((*target, Vec::new())),
                _ => None,
            });
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
            let mut frame_bindings = bindings
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
            if let Some(selector_binding) = &selector_binding
                && !frame_bindings
                    .iter()
                    .any(|(declaration, _)| *declaration == selector_binding.0)
            {
                frame_bindings.push(selector_binding.clone());
            }
            let value_frame = if frame_bindings.is_empty() {
                scoped.value_frame
            } else {
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

    fn statically_selected_arm(
        &self,
        input: SemanticExprId,
        arms: &[CheckedExprId],
    ) -> Option<CheckedExprId> {
        let selector = self.static_selector_value(input)?;
        arms.iter().copied().find(|arm| {
            self.lookup
                .expression(self.program, *arm)
                .and_then(|expression| match &expression.kind {
                    CheckedExpressionKind::MatchArm { pattern, .. } => Some(pattern),
                    _ => None,
                })
                .is_some_and(|pattern| selector.matches(pattern))
        })
    }

    fn static_selector_value(&self, expression: SemanticExprId) -> Option<StaticSelectorValue> {
        let mut expression = expression;
        let mut projection = Vec::<String>::new();
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert((expression, projection.clone())) {
                return None;
            }
            let definition = self.expressions.get(expression.as_usize())?;
            if matches!(
                definition.kind,
                SemanticExpressionKind::Call { .. }
                    | SemanticExpressionKind::MaterializationLocal { .. }
            ) && let Some(tag) = crate::out_net::singleton_tag_for_type_projection(
                &definition.flow_type.ty,
                &projection,
            ) {
                return Some(StaticSelectorValue::Tag(tag));
            }
            match &definition.kind {
                SemanticExpressionKind::Project { input, fields } => {
                    let mut combined = fields.clone();
                    combined.extend(projection);
                    projection = combined;
                    expression = *input;
                }
                SemanticExpressionKind::Block { result, .. }
                | SemanticExpressionKind::Flush { payload: result }
                | SemanticExpressionKind::FlushBoundary { input: result }
                | SemanticExpressionKind::Draining { input: result }
                    if projection.is_empty() =>
                {
                    expression = *result;
                }
                SemanticExpressionKind::When { arms, .. }
                    if projection.is_empty() && arms.len() == 1 =>
                {
                    expression = arms[0].output;
                }
                SemanticExpressionKind::Object(fields)
                | SemanticExpressionKind::TaggedObject { fields, .. }
                    if !projection.is_empty() && fields.iter().all(|field| !field.spread) =>
                {
                    let field = projection.remove(0);
                    expression = fields
                        .iter()
                        .rev()
                        .find(|candidate| candidate.name == field)?
                        .value;
                }
                SemanticExpressionKind::Number(value) if projection.is_empty() => {
                    return Some(StaticSelectorValue::Number(value.clone()));
                }
                SemanticExpressionKind::Text(value) if projection.is_empty() => {
                    return Some(StaticSelectorValue::Text(value.clone()));
                }
                SemanticExpressionKind::Tag(value) if projection.is_empty() => {
                    return Some(StaticSelectorValue::Tag(value.clone()));
                }
                SemanticExpressionKind::TaggedObject { tag, .. } if projection.is_empty() => {
                    return Some(StaticSelectorValue::Tag(tag.clone()));
                }
                SemanticExpressionKind::Bits(value) if projection.is_empty() => {
                    return Some(StaticSelectorValue::Bits(value.clone()));
                }
                _ => return None,
            }
        }
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
                boon_checked::CheckedStatementKind::Field { declaration } => (
                    Some(declaration),
                    self.lookup
                        .declaration(self.program, declaration)
                        .ok_or(ExpansionError::MissingDeclaration(declaration))?
                        .name
                        .clone(),
                    false,
                ),
                boon_checked::CheckedStatementKind::Spread => (None, String::new(), true),
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
                    boon_checked::CheckedCallEntry::Input {
                        value,
                        from_pipe: true,
                        ..
                    } => Some(*value),
                    boon_checked::CheckedCallEntry::Input { .. }
                    | boon_checked::CheckedCallEntry::FreshOut { .. }
                    | boon_checked::CheckedCallEntry::ForwardOut { .. } => None,
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
        let mut direct_constructor_projection = Vec::new();
        loop {
            let Some(first) = fields.first() else {
                if direct_constructor_projection.is_empty() {
                    return Ok(input);
                }
                if let Some(projected) =
                    self.attach_constructor_projection(input, &direct_constructor_projection)
                {
                    return Ok(projected);
                }
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
            direct_constructor_projection.push(first.clone());
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
                constructor_projection,
            } => {
                let mut projection = projection.clone();
                projection.extend(fields.iter().cloned());
                let mut constructor_projection = if direct_constructor_projection.is_empty() {
                    constructor_projection.clone()
                } else {
                    direct_constructor_projection
                };
                constructor_projection.extend(fields);
                SemanticExpressionKind::MaterializationLocal {
                    owner: *local_owner,
                    local: *local,
                    projection,
                    constructor_projection,
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

    fn attach_constructor_projection(
        &mut self,
        input: SemanticExprId,
        constructor_projection: &[String],
    ) -> Option<SemanticExprId> {
        let original = self.expressions.get(input.as_usize())?.clone();
        let kind = match original.kind.clone() {
            SemanticExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
                ..
            } => SemanticExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
                constructor_projection: constructor_projection.to_vec(),
            },
            SemanticExpressionKind::Project {
                input: nested,
                fields,
            } => {
                let nested = self.attach_constructor_projection(nested, constructor_projection)?;
                match self.expressions[nested.as_usize()].kind.clone() {
                    SemanticExpressionKind::MaterializationLocal {
                        owner,
                        local,
                        mut projection,
                        constructor_projection,
                    } => {
                        projection.extend(fields);
                        SemanticExpressionKind::MaterializationLocal {
                            owner,
                            local,
                            projection,
                            constructor_projection,
                        }
                    }
                    SemanticExpressionKind::FunctionParameter {
                        parameter,
                        mut projection,
                    } => {
                        projection.extend(fields);
                        SemanticExpressionKind::FunctionParameter {
                            parameter,
                            projection,
                        }
                    }
                    _ => SemanticExpressionKind::Project {
                        input: nested,
                        fields,
                    },
                }
            }
            SemanticExpressionKind::FlushBoundary { input: nested } => {
                SemanticExpressionKind::FlushBoundary {
                    input: self.attach_constructor_projection(nested, constructor_projection)?,
                }
            }
            SemanticExpressionKind::Draining { input: nested } => {
                SemanticExpressionKind::Draining {
                    input: self.attach_constructor_projection(nested, constructor_projection)?,
                }
            }
            SemanticExpressionKind::Block { bindings, result } => SemanticExpressionKind::Block {
                bindings,
                result: self.attach_constructor_projection(result, constructor_projection)?,
            },
            _ => return None,
        };
        let id = SemanticExprId(self.expressions.len());
        let mut replacement = original;
        replacement.id = id;
        replacement.value_id = SemanticValueId(id.as_usize());
        replacement.kind = kind;
        self.expressions.push(replacement);
        let mut origin = self
            .checked_expression_origins
            .get(input.as_usize())?
            .clone();
        origin.expression = id;
        self.checked_expression_origins.push(origin);
        Some(id)
    }

    fn wrap_type_refinement(
        &mut self,
        expression: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        input: SemanticExprId,
        required: Type,
    ) -> SemanticExprId {
        let required = erase_runtime_type_vars(&required);
        if matches!(required, Type::Unknown | Type::UnresolvedShape { .. })
            || self.expressions[input.as_usize()].flow_type.ty == required
        {
            return input;
        }
        let refined = self.push(
            expression,
            owner,
            SemanticExpressionKind::Project {
                input,
                fields: Vec::new(),
            },
        );
        self.expressions[refined.as_usize()].flow_type.ty = required;
        refined
    }

    fn evaluation_owner(&self, scoped: ScopedCheckedExpr) -> Option<StaticOwnerId> {
        if let Some(port) = scoped.evaluation_port {
            return self.out_net.owner_for_net(self.out_net.net_for_port(port));
        }
        let expression = self.lookup.expression(self.program, scoped.expression)?;
        let mut scope = Some(expression.scope_id);
        while let Some(scope_id) = scope {
            let checked_scope = self.lookup.scope(self.program, scope_id)?;
            if checked_scope.kind == boon_checked::CheckedScopeKind::RepeatedOutput {
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
        if self
            .trace_expression_milestone
            .is_some_and(|milestone| self.expressions.len() >= milestone)
        {
            eprintln!(
                "boon_semantic expression_growth expressions={} memo={} checked_expression={} frame={frame:?} owner={owner:?}",
                self.expressions.len(),
                self.memo.len(),
                expression.id.0,
            );
            self.trace_expression_milestone = self
                .trace_expression_milestone
                .map(|milestone| milestone.saturating_add(100_000));
        }
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

    fn semantic_graph(source: &str) -> SemanticExecutionImageColumnsV1 {
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

    fn semantic_execution_before_resources(
        source: &str,
    ) -> (CheckedProgramFields, SemanticExecutionImageColumnsV1) {
        let parsed = boon_parser::parse_source("semantic-contextual-expansion.bn", source).unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (program, checked_handoff) = checked
            .program
            .expect("valid fixture has a checked program")
            .into_parts();
        crate::validate_contextual_bindings(&program)
            .expect("valid fixture has exact contextual bindings");
        let producer_roots =
            crate::resolve_producer_roots(&program, &[]).expect("fixture has no producer errors");
        let retained = ordinary_callable_declarations(&program);
        let out = crate::out_net::OutNet::<crate::OutPortContractV1>::
            try_build_with_retained_definitions(
                &program,
                producer_roots,
                &retained,
                |call, _, entry| crate::provisional_out_port_contract(&program, call, entry),
                |kind, _, _, _, _| kind == CheckedCallableKind::Builtin,
            )
            .expect("valid fixture has an OUT graph");
        assert!(!out.has_errors(), "OUT diagnostics: {:#?}", out.diagnostics);
        let mut out = out.graph;
        crate::resolve_out_contracts(&program, &mut out)
            .expect("valid fixture resolves OUT contracts");
        crate::validate_out_contracts(&program, &out)
            .expect("valid fixture validates OUT contracts");
        let (materializations, arena, indexes, required) =
            derive_contextual_materializations(&program, &out, &retained, true)
                .expect("valid fixture derives contextual materializations");
        let builder = derive_semantic_execution_graph(
            &program,
            checked_handoff,
            &out,
            &materializations,
            arena,
            &indexes,
            &required,
            true,
        )
        .expect("valid fixture derives the pending execution graph");
        (program, builder.execution().clone())
    }

    fn body_shape(
        graph: &SemanticExecutionImageColumnsV1,
        expression: SemanticExprId,
    ) -> BodyShape {
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
        graph: &SemanticExecutionImageColumnsV1,
    ) -> &SemanticContextualMaterialization {
        let [materialization] = graph.materializations.as_slice() else {
            panic!(
                "expected one semantic materialization, found {}",
                graph.materializations.len()
            );
        };
        materialization
    }

    fn provenance_test_expression(
        id: usize,
        kind: SemanticExpressionKind,
        provenance: SemanticValueProvenance,
    ) -> SemanticExpression {
        SemanticExpression {
            id: SemanticExprId(id),
            value_id: SemanticValueId(id),
            checked_expr_id: CheckedExprId(id as u32),
            flow_type: FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Text,
            },
            effect: boon_checked::CheckedEffectSummary::default(),
            owner: None,
            provenance,
            resource_binding_path: None,
            kind,
        }
    }

    #[test]
    fn projected_canonical_read_skips_unrelated_explicit_record_fields() {
        let store = DeclId(41);
        let selected_provenance = SemanticValueProvenance {
            members: vec![SemanticValueMember {
                path: Vec::new(),
                origin: SemanticValueOrigin::Source {
                    source: SemanticSourceId(7),
                    owner: None,
                },
            }],
        };
        let expressions = vec![
            provenance_test_expression(
                0,
                SemanticExpressionKind::Source {
                    binding_path: "selected-source".to_owned(),
                },
                selected_provenance.clone(),
            ),
            provenance_test_expression(
                1,
                SemanticExpressionKind::Object(vec![SemanticRecordField {
                    declaration: None,
                    name: "leaf".to_owned(),
                    value: SemanticExprId(0),
                    spread: false,
                }]),
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                2,
                SemanticExpressionKind::CanonicalRead {
                    target: store,
                    path: "store".to_owned(),
                    projection: vec!["selected".to_owned(), "leaf".to_owned()],
                    source: None,
                },
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                3,
                SemanticExpressionKind::Object(vec![
                    SemanticRecordField {
                        declaration: None,
                        name: "selected".to_owned(),
                        value: SemanticExprId(1),
                        spread: false,
                    },
                    SemanticRecordField {
                        declaration: None,
                        name: "recursive".to_owned(),
                        value: SemanticExprId(2),
                        spread: false,
                    },
                ]),
                runtime_value_provenance(),
            ),
        ];
        let mut resolver = LocalProvenanceResolver {
            expressions: &expressions,
            bindings: BTreeMap::new(),
            declarations: BTreeMap::from([((store, None), SemanticExprId(3))]),
            cache: BTreeMap::new(),
        };

        assert_eq!(
            resolver
                .resolve(SemanticExprId(2))
                .expect("the exact projected read must not traverse its recursive sibling"),
            selected_provenance
        );
        assert_eq!(
            resolver.cache.keys().copied().collect::<Vec<_>>(),
            [SemanticExprId(0), SemanticExprId(2)],
            "provenance demand must skip both explicit record wrappers"
        );
    }

    #[test]
    fn runtime_occurrence_refinement_keeps_the_narrower_compatible_closed_type() {
        let narrow = Type::VariantSet(vec![boon_checked::Variant::Tag("Closed".to_owned())].into());
        let wide = Type::VariantSet(
            vec![
                boon_checked::Variant::Tag("Closed".to_owned()),
                boon_checked::Variant::Tag("Open".to_owned()),
            ]
            .into(),
        );
        let disjoint =
            Type::VariantSet(vec![boon_checked::Variant::Tag("Missing".to_owned())].into());

        assert_eq!(
            refine_runtime_occurrence_type(&wide, &narrow).unwrap(),
            narrow
        );
        assert_eq!(
            refine_runtime_occurrence_type(&narrow, &wide).unwrap(),
            narrow
        );
        assert!(refine_runtime_occurrence_type(&wide, &disjoint).is_err());
    }

    #[test]
    fn closed_runtime_call_actual_is_authoritative_over_a_generic_formal_frame_collision() {
        let actual = Type::VariantSet(
            vec![
                boon_checked::Variant::Tag("Dark".to_owned()),
                boon_checked::Variant::Tag("Light".to_owned()),
            ]
            .into(),
        );

        assert_eq!(
            refine_runtime_call_boundary_type(
                &actual,
                &Type::Var(boon_checked::TypeVar(0)),
                &Type::Text,
            )
            .unwrap(),
            actual,
            "an unrelated instantiated alpha must not overwrite a closed argument occurrence",
        );
    }

    #[test]
    fn closed_runtime_call_formal_still_rejects_an_incompatible_closed_actual() {
        let actual = Type::VariantSet(
            vec![
                boon_checked::Variant::Tag("Dark".to_owned()),
                boon_checked::Variant::Tag("Light".to_owned()),
            ]
            .into(),
        );

        assert!(
            refine_runtime_call_boundary_type(&actual, &Type::Text, &Type::Text).is_err(),
            "a concrete formal remains a strict contract",
        );
    }

    #[test]
    fn closed_call_result_refines_only_its_exact_structural_state_occurrence() {
        let frame = OutCallInstanceId(7);
        let callable = DeclId(41);
        let formatter = Type::VariantSet(
            vec![
                boon_checked::Variant::Tag("Binary".to_owned()),
                boon_checked::Variant::Tag("Hexadecimal".to_owned()),
            ]
            .into(),
        );
        let expected = Type::object(boon_checked::ObjectShape::from_ordered_fields(
            [("formatter".to_owned(), formatter.clone())],
            false,
        ));
        let mut expressions = vec![
            provenance_test_expression(
                0,
                SemanticExpressionKind::Tag("Binary".to_owned()),
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                1,
                SemanticExpressionKind::Hold {
                    initial: SemanticExprId(0),
                    name: "formatter".to_owned(),
                    binding_path: "formatter".to_owned(),
                    updates: Vec::new(),
                },
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                2,
                SemanticExpressionKind::Object(vec![SemanticRecordField {
                    declaration: None,
                    name: "formatter".to_owned(),
                    value: SemanticExprId(1),
                    spread: false,
                }]),
                runtime_value_provenance(),
            ),
        ];
        expressions[1].flow_type.ty =
            Type::VariantSet(vec![boon_checked::Variant::Tag("Binary".to_owned())].into());
        expressions[2].flow_type.ty = expected.clone();
        let origins = expressions
            .iter()
            .map(|expression| SemanticExpressionOrigin {
                expression: expression.id,
                checked_expression: expression.checked_expr_id,
                checked_scope: boon_checked::LexicalScopeId(0),
                checked_span: boon_checked::CheckedSpan::default(),
                owning_statement: None,
                call_instance: Some(frame),
            })
            .collect::<Vec<_>>();

        refine_call_result_state_occurrences(
            &mut expressions,
            &origins,
            &BTreeMap::from([(CheckedExprId(1), callable)]),
            SemanticExprId(2),
            frame,
            callable,
            &expected,
        )
        .expect("closed result field must refine its exact HOLD occurrence");

        assert_eq!(expressions[1].flow_type.ty, formatter);
        assert_eq!(
            expressions[0].flow_type.ty,
            Type::Text,
            "the state initializer is not a result-field authority"
        );
    }

    #[test]
    fn closed_call_result_does_not_treat_a_spread_as_state_authority() {
        let frame = OutCallInstanceId(9);
        let callable = DeclId(43);
        let mut expressions = vec![
            provenance_test_expression(
                0,
                SemanticExpressionKind::Tag("Binary".to_owned()),
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                1,
                SemanticExpressionKind::Hold {
                    initial: SemanticExprId(0),
                    name: "formatter".to_owned(),
                    binding_path: "formatter".to_owned(),
                    updates: Vec::new(),
                },
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                2,
                SemanticExpressionKind::Object(vec![SemanticRecordField {
                    declaration: None,
                    name: String::new(),
                    value: SemanticExprId(1),
                    spread: true,
                }]),
                runtime_value_provenance(),
            ),
        ];
        expressions[1].flow_type.ty = Type::Unknown;
        let expected = Type::object(boon_checked::ObjectShape::from_ordered_fields(
            [("formatter".to_owned(), Type::Text)],
            false,
        ));
        let origins = expressions
            .iter()
            .map(|expression| SemanticExpressionOrigin {
                expression: expression.id,
                checked_expression: expression.checked_expr_id,
                checked_scope: boon_checked::LexicalScopeId(0),
                checked_span: boon_checked::CheckedSpan::default(),
                owning_statement: None,
                call_instance: Some(frame),
            })
            .collect::<Vec<_>>();

        refine_call_result_state_occurrences(
            &mut expressions,
            &origins,
            &BTreeMap::from([(CheckedExprId(1), callable)]),
            SemanticExprId(2),
            frame,
            callable,
            &expected,
        )
        .expect("a spread is not an exact structural authority");
        assert_eq!(
            expressions[1].flow_type.ty,
            Type::Unknown,
            "the HOLD beneath a spread must remain unchanged",
        );
    }

    #[test]
    fn closed_call_result_ignores_block_dependency_edges() {
        let frame = OutCallInstanceId(10);
        let callable = DeclId(44);
        let mut expressions = vec![
            provenance_test_expression(
                0,
                SemanticExpressionKind::Tag("Binary".to_owned()),
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                1,
                SemanticExpressionKind::Hold {
                    initial: SemanticExprId(0),
                    name: "formatter".to_owned(),
                    binding_path: "formatter".to_owned(),
                    updates: Vec::new(),
                },
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                2,
                SemanticExpressionKind::Text("visible".to_owned()),
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                3,
                SemanticExpressionKind::Block {
                    bindings: vec![SemanticBlockBinding {
                        id: SemanticLocalBindingId(0),
                        declaration: DeclId(45),
                        value: SemanticExprId(1),
                    }],
                    result: SemanticExprId(2),
                },
                runtime_value_provenance(),
            ),
        ];
        expressions[1].flow_type.ty = Type::Unknown;
        expressions[3].flow_type.ty = Type::Text;
        let origins = expressions
            .iter()
            .map(|expression| SemanticExpressionOrigin {
                expression: expression.id,
                checked_expression: expression.checked_expr_id,
                checked_scope: boon_checked::LexicalScopeId(0),
                checked_span: boon_checked::CheckedSpan::default(),
                owning_statement: None,
                call_instance: Some(frame),
            })
            .collect::<Vec<_>>();

        refine_call_result_state_occurrences(
            &mut expressions,
            &origins,
            &BTreeMap::from([(CheckedExprId(1), callable)]),
            SemanticExprId(3),
            frame,
            callable,
            &Type::Text,
        )
        .expect("block result authority must not follow binding dependencies");

        assert_eq!(expressions[1].flow_type.ty, Type::Unknown);
    }

    #[test]
    fn closed_call_result_deduplicates_identical_shared_state_authority() {
        let frame = OutCallInstanceId(11);
        let callable = DeclId(46);
        let mut expressions = vec![
            provenance_test_expression(
                0,
                SemanticExpressionKind::Tag("Binary".to_owned()),
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                1,
                SemanticExpressionKind::Hold {
                    initial: SemanticExprId(0),
                    name: "formatter".to_owned(),
                    binding_path: "formatter".to_owned(),
                    updates: Vec::new(),
                },
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                2,
                SemanticExpressionKind::Object(vec![
                    SemanticRecordField {
                        declaration: None,
                        name: "primary".to_owned(),
                        value: SemanticExprId(1),
                        spread: false,
                    },
                    SemanticRecordField {
                        declaration: None,
                        name: "secondary".to_owned(),
                        value: SemanticExprId(1),
                        spread: false,
                    },
                ]),
                runtime_value_provenance(),
            ),
        ];
        expressions[1].flow_type.ty = Type::Unknown;
        let expected = Type::object(boon_checked::ObjectShape::from_ordered_fields(
            [
                ("primary".to_owned(), Type::Text),
                ("secondary".to_owned(), Type::Text),
            ],
            false,
        ));
        expressions[2].flow_type.ty = expected.clone();
        let origins = expressions
            .iter()
            .map(|expression| SemanticExpressionOrigin {
                expression: expression.id,
                checked_expression: expression.checked_expr_id,
                checked_scope: boon_checked::LexicalScopeId(0),
                checked_span: boon_checked::CheckedSpan::default(),
                owning_statement: None,
                call_instance: Some(frame),
            })
            .collect::<Vec<_>>();

        refine_call_result_state_occurrences(
            &mut expressions,
            &origins,
            &BTreeMap::from([(CheckedExprId(1), callable)]),
            SemanticExprId(2),
            frame,
            callable,
            &expected,
        )
        .expect("identical authorities for a shared HOLD must deduplicate");

        assert_eq!(expressions[1].flow_type.ty, Type::Text);
    }

    #[test]
    fn closed_call_result_refinement_is_atomic_on_incompatible_authority() {
        let frame = OutCallInstanceId(12);
        let callable = DeclId(47);
        let mut expressions = vec![
            provenance_test_expression(
                0,
                SemanticExpressionKind::Tag("Binary".to_owned()),
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                1,
                SemanticExpressionKind::Hold {
                    initial: SemanticExprId(0),
                    name: "first".to_owned(),
                    binding_path: "first".to_owned(),
                    updates: Vec::new(),
                },
                runtime_value_provenance(),
            ),
            provenance_test_expression(
                2,
                SemanticExpressionKind::Object(vec![
                    SemanticRecordField {
                        declaration: None,
                        name: "first".to_owned(),
                        value: SemanticExprId(1),
                        spread: false,
                    },
                    SemanticRecordField {
                        declaration: None,
                        name: "second".to_owned(),
                        value: SemanticExprId(1),
                        spread: false,
                    },
                ]),
                runtime_value_provenance(),
            ),
        ];
        expressions[1].flow_type.ty = Type::Unknown;
        let expected = Type::object(boon_checked::ObjectShape::from_ordered_fields(
            [
                ("first".to_owned(), Type::Text),
                ("second".to_owned(), Type::Number),
            ],
            false,
        ));
        expressions[2].flow_type.ty = expected.clone();
        let origins = expressions
            .iter()
            .map(|expression| SemanticExpressionOrigin {
                expression: expression.id,
                checked_expression: expression.checked_expr_id,
                checked_scope: boon_checked::LexicalScopeId(0),
                checked_span: boon_checked::CheckedSpan::default(),
                owning_statement: None,
                call_instance: Some(frame),
            })
            .collect::<Vec<_>>();

        let error = refine_call_result_state_occurrences(
            &mut expressions,
            &origins,
            &BTreeMap::from([(CheckedExprId(1), callable)]),
            SemanticExprId(2),
            frame,
            callable,
            &expected,
        )
        .expect_err("one shared HOLD cannot have incompatible result authorities");

        assert!(error.contains("incompatible result authorities"), "{error}");
        assert_eq!(
            expressions[1].flow_type.ty,
            Type::Unknown,
            "the first collected authority must not commit before full validation",
        );
    }

    #[test]
    fn retained_context_template_includes_missing_child_owned_passed_paths() {
        let parsed = boon_parser::parse_source(
            "semantic-retained-context-child.bn",
            concat!(
                "FUNCTION view(child_boundary) {\n",
                "    [selected: PASSED.store.active_scope == TEXT { selected }]\n",
                "}\n",
            ),
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (mut program, _) = checked
            .program
            .expect("valid contextual callable")
            .into_parts();
        let callable = program
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("view"))
            .expect("view callable");
        let child_declaration = callable.parameters[0].decl_id;
        let callable = callable.decl_id;
        let formal = program
            .callables
            .iter()
            .find(|candidate| candidate.decl_id == callable)
            .and_then(|candidate| candidate.context_formal)
            .expect("view PASSED formal");
        let passed = program
            .expressions
            .iter()
            .position(|expression| {
                matches!(
                    &expression.kind,
                    CheckedExpressionKind::Passed {
                        formal: expression_formal,
                        projection,
                        ..
                    } if *expression_formal == formal
                        && projection.as_slice() == ["store", "active_scope"]
                )
            })
            .expect("child-owned PASSED expression");
        program.expressions[passed].declaration = Some(child_declaration);
        let lookup = CheckedProgramLookup::new(&program);
        let passed = &program.expressions[passed];
        assert_ne!(
            passed.declaration,
            Some(callable),
            "the regression must cross a child record-field declaration",
        );
        assert_eq!(
            enclosing_function_owner(&program, &lookup, passed.scope_id),
            Some(callable),
        );
        program
            .context_formals
            .iter_mut()
            .find(|candidate| candidate.id == formal)
            .expect("view context scheme")
            .scheme
            .flow_type
            .ty = Type::object(boon_checked::ObjectShape {
            fields: BTreeMap::new(),
            field_order: Vec::new(),
            open: true,
        });

        let completed = completed_context_formal_flow_types(&program, &lookup)
            .expect("completed retained context templates")
            .remove(&formal)
            .expect("completed retained context template");
        assert!(
            project_concrete_type(
                completed.ty,
                &["store".to_owned(), "active_scope".to_owned()],
            )
            .is_some(),
            "the retained template must recover exact child-owned PASSED demand paths",
        );
    }

    #[test]
    fn named_statement_erases_child_owned_passed_alphas_like_its_semantic_value() {
        let (checked, graph) = semantic_execution_before_resources(
            r#"
store: [
    elements: [
        focus_probe: SOURCE
        text_input: SOURCE
    ]
]

panel: view(PASS: [store: store])

FUNCTION view() {
    [
        event: [
            click: PASSED.store.elements.focus_probe
            change: PASSED.store.elements.text_input
        ]
    ]
}
"#,
        );
        let statement = graph
            .statements
            .iter()
            .find(|statement| {
                matches!(
                    &statement.kind,
                    SemanticStatementKind::Field { name, .. } if name == "panel"
                )
            })
            .expect("top-level panel statement");
        let declaration = statement.declaration.expect("panel declaration");
        let checked_declaration = checked
            .declarations
            .iter()
            .find(|candidate| candidate.id == declaration)
            .expect("checked panel declaration");
        let canonical_checked_flow = FlowType {
            mode: checked_declaration.flow_type.mode,
            ty: erase_runtime_type_vars(&checked_declaration.flow_type.ty),
        };
        let Type::Object(checked_panel) = &checked_declaration.flow_type.ty else {
            panic!(
                "checked panel declaration is not an object: {:?}",
                checked_declaration.flow_type,
            );
        };
        let Some(Type::Object(checked_event)) = checked_panel.fields.get("event") else {
            panic!(
                "checked panel declaration has no event object: {:?}",
                checked_declaration.flow_type,
            );
        };
        assert!(matches!(
            checked_event.fields.get("click"),
            Some(Type::Var(_))
        ));
        assert!(matches!(
            checked_event.fields.get("change"),
            Some(Type::Var(_))
        ));
        assert_ne!(
            checked_declaration.flow_type, canonical_checked_flow,
            "the regression requires a definition-local alpha at the checked statement boundary",
        );
        let value = statement.value.expect("panel statement expression");
        let statement_flow = statement.flow_type.as_ref().expect("panel statement flow");
        let value_flow = &graph.expressions[value.as_usize()].flow_type;
        assert_eq!(statement_flow, &canonical_checked_flow);
        assert_eq!(statement_flow, value_flow);

        let Type::Object(panel) = &statement_flow.ty else {
            panic!("panel statement is not an object: {statement_flow:?}");
        };
        let Some(Type::Object(event)) = panel.fields.get("event") else {
            panic!("panel statement has no event object: {statement_flow:?}");
        };
        assert_eq!(event.fields.get("click"), Some(&Type::Unknown));
        assert_eq!(event.fields.get("change"), Some(&Type::Unknown));
    }

    #[test]
    fn flush_boundary_flow_uses_runtime_success_authority_and_erases_checked_alphas() {
        let rejected =
            Type::VariantSet(vec![boon_checked::Variant::Tag("Rejected".to_owned())].into());
        let concrete = runtime_flush_boundary_flow_type(
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Number,
            },
            rejected.clone(),
        );
        assert_eq!(
            concrete,
            FlowType {
                mode: FlowMode::Continuous,
                ty: boon_checked::canonical_union_type(vec![Type::Number, rejected.clone()]),
            },
            "a concrete semantic occurrence remains the successful-value authority",
        );
        let generic = runtime_flush_boundary_flow_type(
            FlowType {
                mode: FlowMode::Absent,
                ty: Type::Var(boon_checked::TypeVar(71)),
            },
            rejected.clone(),
        );
        assert_eq!(generic.mode, FlowMode::Continuous);
        assert_eq!(
            generic.ty,
            boon_checked::canonical_union_type(vec![Type::Unknown, rejected]),
        );
    }

    #[test]
    fn ordinary_dependency_analysis_visits_structural_match_arm_statements() {
        let parsed = boon_parser::parse_source(
            "semantic-ordinary-structural-arm.bn",
            r#"
result: wrapper()

FUNCTION wrapper() {
    Choice |> WHEN {
        Choice => [
            value: nested()
        ]
    }
}

FUNCTION nested() {
    1
}
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let program = checked.program.expect("valid fixture has checked program");
        let lookup = CheckedProgramLookup::new(&program);
        let candidates = program
            .callables
            .iter()
            .filter(|callable| ordinary_callable_base_candidate(&program, callable))
            .map(|callable| callable.decl_id)
            .collect::<BTreeSet<_>>();
        let wrapper = program
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("wrapper"))
            .expect("wrapper callable");
        let nested = program
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("nested"))
            .expect("nested callable");
        let dependencies =
            ordinary_callable_body_dependencies(&program, &lookup, wrapper, &candidates)
                .expect("pure structural wrapper is an ordinary candidate");
        assert_eq!(
            dependencies,
            BTreeSet::from([nested.decl_id]),
            "the delimiter-backed arm must expose its nested call dependency"
        );
    }

    #[test]
    fn ordinary_definition_retains_uninstantiated_nested_call_and_exact_pass_capture() {
        let graph = semantic_graph(
            r#"
theme: [mode: Light, ignored: SOURCE]
result: choose(request: Primary, PASS: theme)

FUNCTION choose(request) {
    request |> WHEN {
        Primary => 1
        Secondary => nested()
    }
}

FUNCTION nested() {
    PASSED.mode |> WHEN {
        Light => 2
        Dark => 3
    }
}
"#,
        );

        let calls = graph
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                SemanticExpressionKind::Call {
                    function,
                    callable_kind: SemanticCallableKind::User,
                    instance,
                    context_argument,
                    ..
                } => Some((function.as_str(), *instance, context_argument.as_ref())),
                _ => None,
            })
            .collect::<Vec<_>>();
        let (_, nested_instance, nested_context) = calls
            .iter()
            .find(|(function, _, _)| function.ends_with("nested"))
            .copied()
            .expect("nested ordinary call remains explicit in the shared body");
        assert_eq!(
            nested_instance, None,
            "an unselected nested pure call must not fabricate an OUT instance"
        );
        assert_eq!(
            nested_context.and_then(|argument| argument.checked_value),
            None,
            "inherited PASSED is represented by the owner's hidden parameter"
        );

        let (_, choose_instance, choose_context) = calls
            .iter()
            .find(|(function, instance, _)| function.ends_with("choose") && instance.is_some())
            .copied()
            .expect("top-level choose call has one concrete occurrence");
        assert!(choose_instance.is_some());
        let capture = choose_context.expect("choose receives its exact PASSED capture");
        let SemanticExpressionKind::Object(fields) =
            &graph.expressions[capture.value.as_usize()].kind
        else {
            panic!("ordinary PASSED capture is not an object");
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["mode"],
            "unused source-bearing context fields must not cross the retained boundary"
        );
        assert!(
            graph.expressions.len() < 80,
            "tiny nested-call fixture expanded to {} semantic expressions",
            graph.expressions.len()
        );
    }

    #[test]
    fn contextual_materializations_share_one_ordinary_callable_definition() {
        let graph = semantic_graph(
            r#"
first:
    LIST { [value: 1] }
    |> List/map(item, new: decorate(row: item))

second:
    LIST { [value: 2] }
    |> List/map(item, new: decorate(row: item))

FUNCTION decorate(row) {
    [value: row.value, doubled: double(value: row.value)]
}

FUNCTION double(value) {
    value * 2
}
"#,
        );

        assert_eq!(graph.materializations.len(), 2);
        let decorate = graph
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("decorate"))
            .expect("decorate callable");
        let double = graph
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("double"))
            .expect("double callable");
        assert!(
            decorate.semantic_root.is_some() && double.semantic_root.is_some(),
            "materialization-only ordinary calls must publish their shared definitions"
        );
        assert_eq!(
            graph
                .expressions
                .iter()
                .filter(|expression| matches!(
                    &expression.kind,
                    SemanticExpressionKind::Call {
                        callable,
                        callable_kind: SemanticCallableKind::User,
                        ..
                    } if *callable == decorate.id
                ))
                .count(),
            2,
            "each materialization keeps one call edge to the same definition"
        );
        assert!(
            graph.expressions.len() < 80,
            "shared materialization helper expanded to {} semantic expressions",
            graph.expressions.len()
        );
    }

    #[test]
    fn ordinary_definition_retains_canonical_program_root_reads() {
        let graph = semantic_graph(
            r#"
store: [offset: 2]
first: add(value: 1)
second: add(value: 3)

FUNCTION add(value) {
    value + store.offset
}
"#,
        );

        let add = graph
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("add"))
            .expect("add callable");
        assert!(
            add.semantic_root.is_some(),
            "a pure canonical program-root read must remain in one shared body"
        );
        assert_eq!(
            graph
                .expressions
                .iter()
                .filter(|expression| matches!(
                    &expression.kind,
                    SemanticExpressionKind::Call {
                        callable,
                        callable_kind: SemanticCallableKind::User,
                        ..
                    } if *callable == add.id
                ))
                .count(),
            2,
            "both occurrences must retain call edges to the shared definition"
        );
        assert!(graph.expressions.iter().any(|expression| matches!(
            &expression.kind,
            SemanticExpressionKind::CanonicalRead {
                projection,
                ..
            } if projection.len() == 1 && projection[0] == "offset"
        )));
    }

    #[test]
    fn ordinary_template_uses_call_overlays_for_open_parameter_and_result_types() {
        let graph = semantic_graph(
            r#"
number_box: box_value(value: 1)
text_box: box_value(value: TEXT { hello })
number_value: identity(value: number_box.value)
text_value: identity(value: text_box.value)

FUNCTION box_value(value) {
    [value: value]
}

FUNCTION identity(value) {
    value
}
"#,
        );

        for name in ["box_value", "identity"] {
            let callable = graph
                .callables
                .iter()
                .find(|callable| callable.name.ends_with(name))
                .unwrap_or_else(|| panic!("missing {name} callable"));
            assert!(
                callable.semantic_root.is_some(),
                "pure open-boundary callable {name} must retain one template body"
            );
            assert_eq!(
                graph
                    .expressions
                    .iter()
                    .filter(|expression| matches!(
                        &expression.kind,
                        SemanticExpressionKind::Call {
                            callable: candidate,
                            callable_kind: SemanticCallableKind::User,
                            ..
                        } if *candidate == callable.id
                    ))
                    .count(),
                2,
                "each {name} invocation must remain one compact call overlay"
            );
        }

        assert!(
            graph.expressions.len() < 40,
            "open-boundary templates expanded to {} semantic expressions",
            graph.expressions.len()
        );
    }

    #[test]
    fn source_bearing_row_projection_stays_contextual() {
        let graph = semantic_graph(
            r#"
rows:
    LIST { [name: TEXT { first }] }
    |> List/map(item, new: selectable(row: item))

result:
    rows
    |> List/map(item, new: copied_controls(row: item))

FUNCTION selectable(row) {
    [controls: [press: SOURCE], name: row.name]
}

FUNCTION copied_controls(row) {
    row.controls
}
"#,
        );

        let callable = graph
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("copied_controls"))
            .expect("copied_controls callable");
        assert_eq!(
            callable.semantic_root, None,
            "a helper that projects SOURCE-bearing row data must remain contextual"
        );
        assert!(graph.expressions.iter().all(|expression| {
            !matches!(
                &expression.kind,
                SemanticExpressionKind::Call {
                    callable: candidate,
                    callable_kind: SemanticCallableKind::User,
                    ..
                } if *candidate == callable.id
            )
        }));
    }

    #[test]
    fn function_local_source_has_one_declaration_per_contextual_materialization() {
        let graph = semantic_graph(
            r#"
store: [
    first:
        LIST { [name: TEXT { first }] }
        |> List/map(item, new: selectable_row(row: item))
    second:
        LIST { [name: TEXT { second }] }
        |> List/map(item, new: selectable_row(row: item))
]

FUNCTION selectable_row(row) {
    [controls: [select: SOURCE], name: row.name]
}
"#,
        );
        assert_eq!(graph.sources.len(), 2, "{:#?}", graph.sources);
        assert_eq!(
            graph
                .sources
                .iter()
                .filter_map(|source| match source.origin {
                    SemanticSourceOrigin::Checked { source } => Some(source),
                    SemanticSourceOrigin::ProducerInvocation { .. } => None,
                })
                .collect::<BTreeSet<_>>()
                .len(),
            1,
            "both concrete resources must retain one checked SOURCE identity"
        );
        assert_eq!(
            graph
                .sources
                .iter()
                .filter_map(|source| source.owner)
                .collect::<BTreeSet<_>>()
                .len(),
            2,
            "each contextual materialization must own its own SOURCE resource"
        );
        assert_eq!(
            graph
                .sources
                .iter()
                .map(|source| source.statement)
                .collect::<BTreeSet<_>>()
                .len(),
            2,
            "each concrete SOURCE must bind its exact semantic declaration statement"
        );
    }

    #[test]
    fn function_local_hold_update_reads_the_previous_state_without_recursive_expansion() {
        let graph = semantic_graph(
            r#"
rows: LIST { [selected: False] }
result:
    rows
    |> List/map(item, new: stateful_row(row: item))

FUNCTION stateful_row(row) {
    [
        controls: [toggle: SOURCE]
        selected:
            row.selected |> HOLD selected {
                controls.toggle |> THEN { selected }
            }
    ]
}
"#,
        );

        let state = graph.states.first().expect("one contextual HOLD state");
        let expression = &graph.expressions[state.expression.as_usize()];
        assert!(
            matches!(
                &expression.kind,
                SemanticExpressionKind::Hold { updates, .. } if !updates.is_empty()
            ),
            "contextual HOLD did not retain its update branches: {expression:#?}",
        );
        assert!(graph.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                SemanticExpressionKind::CanonicalRead { target, .. }
                    if graph
                        .statements
                        .iter()
                        .any(|statement| statement.declaration == Some(*target))
            )
        }));
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
                        boon_checked::Variant::Tagged { tag, .. } if tag == "InvalidUpdate"
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
    fn function_contextual_source_resolves_unique_root_named_value() {
        let graph = semantic_graph(
            r#"
store: [
    rows: LIST { [value: 1] }
]

FUNCTION row_values() {
    rows |> List/map(item, new: item.value)
}

result: row_values()
"#,
        );
        assert_eq!(
            only_materialization(&graph).operation,
            SemanticContextualOperationKind::Map
        );
    }

    #[test]
    fn select_arm_forwarding_keeps_call_occurrence_refinements_isolated() {
        semantic_graph(
            r#"
FUNCTION identity(value) {
    value
}

FUNCTION choose(value) {
    value |> WHEN {
        First => identity(value: value)
        __ => identity(value: value)
    }
}

first: choose(value: First)
second: choose(value: Second)
"#,
        );
    }

    #[test]
    fn nested_static_dispatch_keeps_the_checked_record_occurrence() {
        let graph = semantic_graph(
            r#"
FUNCTION backend_get(request) {
    request |> WHEN {
        Count => 1
        Range => [extend: 2, compress: 3]
    }
}

FUNCTION get(request, backend) {
    backend |> WHEN {
        First => backend_get(request: request)
        Second => backend_get(request: request)
    }
}

FUNCTION range() {
    get(request: Range, backend: First)
}

result: range()
"#,
        );

        assert!(graph.expressions.iter().any(|expression| matches!(
            &expression.flow_type.ty,
            Type::Object(shape)
                if shape.fields.get("extend") == Some(&Type::Number)
                    && shape.fields.get("compress") == Some(&Type::Number)
        )));
    }

    #[test]
    fn static_dispatch_does_not_expand_unselected_callable_bodies() {
        fn dispatch_graph(branch_count: usize) -> SemanticExecutionImageColumnsV1 {
            let mut source = String::new();
            for index in 0..branch_count {
                source.push_str(&format!(
                    "FUNCTION branch_{index}() {{\n    {index}\n}}\n\n"
                ));
            }
            source.push_str("FUNCTION dispatch(choice) {\n    choice |> WHEN {\n");
            for index in 0..branch_count {
                source.push_str(&format!("        Choice{index} => branch_{index}()\n"));
            }
            source.push_str("    }\n}\n\nresult: dispatch(choice: Choice0)\n");
            semantic_graph(&source)
        }

        let small = dispatch_graph(4);
        let large = dispatch_graph(64);
        assert!(large.expressions.len() <= small.expressions.len() + 4);
        let selected = large
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                SemanticExpressionKind::When { arms, .. } => Some(arms),
                _ => None,
            })
            .expect("static dispatcher retains its semantic selection");
        assert_eq!(selected.len(), 1);
        assert!(matches!(
            &selected[0].pattern,
            CheckedMatchPattern::Tag { name, .. } if name == "Choice0"
        ));
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
