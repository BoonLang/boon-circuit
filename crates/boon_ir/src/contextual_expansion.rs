use super::out_net::{OutCallInstanceId, OutInputValue, OutNet, OutNetId, ScopedCheckedExpr};
use super::{
    ContextualMaterialization as ConcreteMaterialization,
    ContextualOperationKind as ConcreteContextualOperation,
    ExecutableBlockBinding as ConcreteBlockBinding, ExecutableCallArgument as ConcreteCallArgument,
    ExecutableCallContextId, ExecutableCallableKind, ExecutableExprId as ConcreteExprId,
    ExecutableExpression as ConcreteExpression, ExecutableExpressionKind as ConcreteExpressionKind,
    ExecutableFunction, ExecutableFunctionParameter, ExecutableLocalBindingId,
    ExecutablePatternBinding, ExecutableProgram, ExecutableRecordField as ConcreteRecordField,
    ExecutableSelectArm, ExecutableSourceDef, ExecutableSourceId, ExecutableSourceOrigin,
    ExecutableStateDef, ExecutableStateId, ExecutableStatement, ExecutableStatementId,
    ExecutableStatementKind, ExecutableTextSegment as ConcreteTextSegment, ExecutableValueMember,
    ExecutableValueOrigin, ExecutableValueProvenance, MaterializationLocalId,
    MaterializationResultKind, StaticOwnerId,
};
use boon_typecheck::{
    CheckedCallEntry, CheckedCallId, CheckedCallableKind, CheckedContextualOperation,
    CheckedDeclarationKind, CheckedExprId, CheckedExpression, CheckedExpressionKind,
    CheckedProgram, CheckedTextSegment, CheckedValueUse, DeclId, FlowMode, Type,
    apply_checked_type_substitutions, is_renderable_type,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

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
    operation: ConcreteContextualOperation,
    evaluation_owner: Option<StaticOwnerId>,
    source: ScopedCheckedExpr,
    body: ScopedCheckedExpr,
    direction: Option<ScopedCheckedExpr>,
    result_type: Type,
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
    expressions: &mut Vec<ConcreteExpression>,
    checked_expression: CheckedExprId,
    owner: Option<StaticOwnerId>,
) -> ConcreteExprId {
    let id = ConcreteExprId(expressions.len());
    expressions.push(ConcreteExpression {
        id,
        checked_expr_id: checked_expression,
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
        kind: ConcreteExpressionKind::Tag("Ascending".to_owned()),
    });
    id
}

pub(crate) fn derive_contextual_materializations(
    program: &CheckedProgram,
    out_net: &OutNet,
) -> Result<(Vec<ConcreteMaterialization>, Vec<ConcreteExpression>), ExpansionError> {
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
                ConcreteContextualOperation::SortBy | ConcreteContextualOperation::ThenBy
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
        .map(|(id, candidate)| (candidate.owner, id))
        .collect::<BTreeMap<_, _>>();
    let net_by_owner = candidates
        .iter()
        .map(|candidate| (candidate.owner, candidate.net))
        .collect::<BTreeMap<_, _>>();
    let mut materialization_result_types = BTreeMap::new();
    let mut result = Vec::with_capacity(candidates.len());
    let mut expressions = Vec::new();
    let mut item_types_by_owner = BTreeMap::new();
    for (id, candidate) in candidates.iter().cloned().enumerate() {
        let inherited_candidates = inherited_order_candidates[id]
            .iter()
            .map(|candidate| &candidates[*candidate])
            .collect::<Vec<_>>();
        let mut locals = BTreeMap::new();
        let mut owner = Some(candidate.owner);
        while let Some(current) = owner {
            if let Some(net) = net_by_owner.get(&current).copied() {
                locals.insert(net, (current, MaterializationLocalId(0)));
            }
            owner = out_net
                .static_owners
                .get(current.as_usize())
                .and_then(|owner| owner.parent);
        }
        for inherited in &inherited_candidates {
            locals.insert(inherited.net, (candidate.owner, MaterializationLocalId(0)));
        }
        let mut builder = ConcreteExpressionBuilder::new(
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
            MaterializationLocalId(0),
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
                ))
            })
            .collect::<Result<Vec<_>, ExpansionError>>()?;
        let body_type = builder.expressions[local_body.as_usize()]
            .flow_type
            .ty
            .clone();
        let result_type = match candidate.operation {
            ConcreteContextualOperation::Map => Type::List(Box::new(body_type.clone())),
            ConcreteContextualOperation::Filter
            | ConcreteContextualOperation::Retain
            | ConcreteContextualOperation::Remove
            | ConcreteContextualOperation::SortBy
            | ConcreteContextualOperation::ThenBy => Type::List(Box::new(item_type.clone())),
            ConcreteContextualOperation::Every
            | ConcreteContextualOperation::Any
            | ConcreteContextualOperation::Find => candidate.result_type,
        };
        let result_type = erase_runtime_type_vars(&result_type);
        let mut expanded = builder.finish();
        let local_direction = local_direction.or_else(|| {
            matches!(
                candidate.operation,
                ConcreteContextualOperation::SortBy | ConcreteContextualOperation::ThenBy
            )
            .then(|| {
                push_default_order_direction(
                    &mut expanded,
                    candidate.checked_expression,
                    candidate.evaluation_owner,
                )
            })
        });
        let local_inherited_order = local_inherited_order
            .into_iter()
            .map(|(operation, body, direction, checked_expression, owner)| {
                let direction = direction.unwrap_or_else(|| {
                    push_default_order_direction(&mut expanded, checked_expression, owner)
                });
                (operation, body, direction)
            })
            .collect::<Vec<_>>();
        let offset = expressions.len();
        append_expression_arena_without_roots(&mut expressions, expanded);
        let source = rebase_expr_id(local_source, offset);
        let body = rebase_expr_id(local_body, offset);
        let direction = local_direction.map(|direction| rebase_expr_id(direction, offset));
        let inherited_order = local_inherited_order
            .into_iter()
            .map(|(operation, body, direction)| super::ContextualOrderKey {
                operation,
                body: rebase_expr_id(body, offset),
                direction: rebase_expr_id(direction, offset),
            })
            .collect();
        let result_kind = match &result_type {
            Type::List(item) if is_renderable_type(item) => MaterializationResultKind::RenderSlot,
            _ => MaterializationResultKind::RuntimeValue,
        };
        result.push(ConcreteMaterialization {
            id,
            owner: candidate.owner,
            operation: candidate.operation,
            result_kind,
            source,
            source_row_predecessors: Vec::new(),
            body,
            direction,
            inherited_order,
            row_local: MaterializationLocalId(0),
            source_list_id: None,
            source_scope_id: None,
            target_list_id: None,
            target_scope_id: None,
            item_type: item_type.clone(),
            result_type: result_type.clone(),
        });
        item_types_by_owner.insert(candidate.owner, item_type);
        materialization_result_types.insert(id, result_type);
    }
    Ok((result, expressions))
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

fn erase_runtime_type_vars(ty: &Type) -> Type {
    match ty {
        Type::Var(_) => Type::Unknown,
        Type::List(item) => Type::List(Box::new(erase_runtime_type_vars(item))),
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
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Skip
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => ty.clone(),
    }
}

fn project_concrete_type(mut ty: Type, fields: &[String]) -> Option<Type> {
    for field in fields {
        let Type::Object(shape) = ty else {
            return None;
        };
        ty = shape.fields.get(field)?.clone();
    }
    Some(ty)
}

fn concrete_record_type(
    expressions: &[ConcreteExpression],
    fields: &[ConcreteRecordField],
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
    expressions: &[ConcreteExpression],
    kind: &ConcreteExpressionKind,
) -> Option<Type> {
    match kind {
        ConcreteExpressionKind::Object(fields) | ConcreteExpressionKind::Record(fields) => {
            concrete_record_type(expressions, fields)
        }
        ConcreteExpressionKind::TaggedObject { tag, fields } => {
            let Type::Object(shape) = concrete_record_type(expressions, fields)? else {
                return None;
            };
            Some(Type::VariantSet(vec![boon_typecheck::Variant::Tagged {
                tag: tag.clone(),
                fields: shape,
            }]))
        }
        ConcreteExpressionKind::List { items, .. } if !items.is_empty() => {
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
        ConcreteExpressionKind::Block { result, .. } => expressions
            .get(result.as_usize())
            .map(|expression| expression.flow_type.ty.clone()),
        _ => None,
    }
}

pub(crate) fn derive_executable_program(
    program: &CheckedProgram,
    out_net: &OutNet,
    materializations: &[ConcreteMaterialization],
    mut expressions: Vec<ConcreteExpression>,
) -> Result<ExecutableProgram, ExpansionError> {
    let lookup = CheckedProgramLookup::new(program);
    let materializations_by_owner = materializations
        .iter()
        .map(|materialization| (materialization.owner, materialization.id))
        .collect::<BTreeMap<_, _>>();
    let materialization_result_types = materializations
        .iter()
        .map(|materialization| (materialization.id, materialization.result_type.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut builder = ConcreteExpressionBuilder::new(
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
    let mut statements = Vec::with_capacity(included.len());
    for statement in program
        .statements
        .iter()
        .filter(|statement| included.contains(&statement.id))
    {
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
                builder.expand_with_inherited_owner(
                    ScopedCheckedExpr {
                        expression,
                        frame: None,
                        evaluation_port: None,
                        value_frame: None,
                    },
                    None,
                )
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
                ExecutableStatementKind::Field {
                    name: name.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
                    path: path.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
                }
            }
            boon_typecheck::CheckedStatementKind::Source { declaration, event } => {
                let (name, path) = declaration_parts(*declaration);
                ExecutableStatementKind::Source {
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
                ExecutableStatementKind::Hold {
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
                ExecutableStatementKind::List {
                    name,
                    path,
                    capacity: *capacity,
                }
            }
            boon_typecheck::CheckedStatementKind::Block => ExecutableStatementKind::Block,
            boon_typecheck::CheckedStatementKind::Spread => ExecutableStatementKind::Spread,
            boon_typecheck::CheckedStatementKind::Expression => ExecutableStatementKind::Expression,
        };
        statements.push(ExecutableStatement {
            id: ExecutableStatementId(statement.id.0 as usize),
            declaration,
            flow_type: declaration
                .and_then(|declaration| lookup.declaration(program, declaration))
                .map(|declaration| declaration.flow_type.clone()),
            kind,
            value,
            value_use: match statement.value_use {
                CheckedValueUse::RuntimeValue => MaterializationResultKind::RuntimeValue,
                CheckedValueUse::RenderSlot => MaterializationResultKind::RenderSlot,
            },
            children: statement
                .children
                .iter()
                .filter(|child| included.contains(child))
                .map(|child| ExecutableStatementId(child.0 as usize))
                .collect(),
        });
    }
    let mut producer_bodies = Vec::with_capacity(out_net.producer_roots().len());
    for producer in out_net.producer_roots() {
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
        producer_bodies.push((producer, result, owner, body));
    }

    statements.sort_by_key(|statement| statement.id);
    let roots = Vec::new();
    let offset = expressions.len();
    let local_expressions = builder.finish();
    for statement in &mut statements {
        if let Some(value) = &mut statement.value {
            *value = rebase_expr_id(*value, offset);
        }
    }
    append_expression_arena_without_roots(&mut expressions, local_expressions);
    let mut functions = Vec::with_capacity(producer_bodies.len());
    let mut producer_sources = Vec::new();
    for (producer, checked_result, owner, body) in producer_bodies {
        let body = rebase_expr_id(body, offset);
        let invocation_source = if producer.spec.mode == crate::ProducerFunctionMode::Invocation {
            let identity = crate::producer_identity_text(producer.spec.identity);
            let binding_path = format!("_producer_{identity}_invoke");
            let source = ConcreteExprId(expressions.len());
            expressions.push(ConcreteExpression {
                id: source,
                checked_expr_id: checked_result,
                flow_type: boon_typecheck::FlowType {
                    mode: FlowMode::PresentOrAbsent,
                    ty: Type::Unknown,
                },
                effect: boon_typecheck::CheckedEffectSummary {
                    emits_source: true,
                    ..boon_typecheck::CheckedEffectSummary::default()
                },
                owner: Some(owner),
                provenance: ExecutableValueProvenance {
                    members: vec![ExecutableValueMember {
                        path: Vec::new(),
                        origin: ExecutableValueOrigin::ProducerSource {
                            function: producer.spec.function,
                            identity: producer.spec.identity,
                            owner,
                        },
                    }],
                },
                resource_binding_path: Some(binding_path.clone()),
                kind: ConcreteExpressionKind::Source {
                    binding_path: binding_path.clone(),
                },
            });
            producer_sources.push((
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
            let root = ConcreteExprId(expressions.len());
            expressions.push(ConcreteExpression {
                id: root,
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
                provenance: expressions[body.as_usize()].provenance.clone(),
                resource_binding_path: None,
                kind: ConcreteExpressionKind::Then {
                    input: source,
                    output: Some(body),
                },
            });
            root
        } else {
            body
        };
        statements.push(ExecutableStatement {
            id: producer.spec.result_statement,
            declaration: Some(producer.spec.result_declaration),
            flow_type: Some(producer.spec.result_type.clone()),
            kind: ExecutableStatementKind::Field {
                name: "result".to_owned(),
                path: producer.spec.result_path.clone(),
            },
            value: Some(root),
            value_use: MaterializationResultKind::RuntimeValue,
            children: Vec::new(),
        });
        functions.push(ExecutableFunction {
            id: producer.spec.function,
            identity: producer.spec.identity,
            name: producer.spec.function_name.clone(),
            parameters: producer
                .spec
                .parameters
                .iter()
                .map(|parameter| ExecutableFunctionParameter {
                    id: parameter.parameter,
                    name: parameter.name.clone(),
                    flow_type: parameter.flow_type.clone(),
                })
                .collect(),
            result_type: producer.spec.result_type.clone(),
            root,
            invocation_source,
        });
    }
    statements.sort_by_key(|statement| statement.id);
    synthesize_statement_owned_states(program, &lookup, &mut expressions, &mut statements)?;
    resolve_executable_local_provenance(&mut expressions, &statements)?;
    let mut sources = Vec::new();
    let mut source_instances = BTreeSet::new();
    for checked_source in &program.sources {
        let fallback_path = program.semantic_path(&checked_source.path).ok_or(
            ExpansionError::MissingSourceDeclaration(checked_source.expression),
        )?;
        for expression in expressions.iter().filter(|expression| {
            expression.checked_expr_id == checked_source.expression
                && (matches!(expression.kind, ConcreteExpressionKind::Source { .. })
                    || expression.effect.emits_source)
        }) {
            if !source_instances.insert((checked_source.id, expression.owner)) {
                continue;
            }
            sources.push(ExecutableSourceDef {
                id: ExecutableSourceId(sources.len()),
                origin: ExecutableSourceOrigin::Checked {
                    source: checked_source.id,
                },
                declaration: checked_source.declaration,
                expression: expression.id,
                binding_path: fallback_path.clone(),
                owner: expression.owner,
            });
        }
    }
    for (function, identity, declaration, expression, binding_path, owner) in producer_sources {
        sources.push(ExecutableSourceDef {
            id: ExecutableSourceId(sources.len()),
            origin: ExecutableSourceOrigin::ProducerInvocation { function, identity },
            declaration,
            expression,
            binding_path,
            owner: Some(owner),
        });
    }
    let mut states = Vec::new();
    let mut state_instances = BTreeSet::new();
    for checked_state in &program.states {
        let fallback_path = program.semantic_path(&checked_state.path).ok_or(
            ExpansionError::MissingStateDeclaration(checked_state.expression),
        )?;
        for expression in expressions.iter().filter(|expression| {
            expression.checked_expr_id == checked_state.expression
                && (matches!(
                    expression.kind,
                    ConcreteExpressionKind::Hold { .. } | ConcreteExpressionKind::Latest { .. }
                ) || expression.effect.writes_state)
        }) {
            if !state_instances.insert((checked_state.id, expression.owner)) {
                continue;
            }
            let initial = concrete_state_initial_expression(&expressions, expression.id).ok_or(
                ExpansionError::MissingStateInitializer(checked_state.expression),
            )?;
            states.push(ExecutableStateDef {
                id: ExecutableStateId(states.len()),
                checked_state: checked_state.id,
                declaration: checked_state.declaration,
                expression: expression.id,
                initial,
                binding_path: fallback_path.clone(),
                owner: expression.owner,
            });
        }
    }
    Ok(ExecutableProgram {
        expressions,
        statements,
        sources,
        states,
        roots,
        functions,
    })
}

fn concrete_state_initial_expression(
    expressions: &[ConcreteExpression],
    root: ConcreteExprId,
) -> Option<ConcreteExprId> {
    let mut current = root;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        let expression = expressions
            .get(current.as_usize())
            .filter(|candidate| candidate.id == current)?;
        current = match &expression.kind {
            ConcreteExpressionKind::Hold { initial, .. } => *initial,
            ConcreteExpressionKind::Latest { branches } => *branches.first()?,
            ConcreteExpressionKind::Call { arguments, .. } if expression.effect.writes_state => {
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
    expressions: &[ConcreteExpression],
    branches: &[ConcreteExprId],
) -> bool {
    branches
        .first()
        .and_then(|branch| expressions.get(branch.as_usize()))
        .is_some_and(|branch| branch.flow_type.mode == FlowMode::Continuous)
}

fn synthesize_statement_owned_states(
    program: &CheckedProgram,
    lookup: &CheckedProgramLookup,
    expressions: &mut Vec<ConcreteExpression>,
    statements: &mut [ExecutableStatement],
) -> Result<(), ExpansionError> {
    let statement_values = statements
        .iter()
        .map(|statement| (statement.id, (statement.value, statement.children.clone())))
        .collect::<BTreeMap<_, _>>();
    for statement in statements {
        let ExecutableStatementKind::Hold {
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
        let initial_expression = expressions
            .get(initial.as_usize())
            .filter(|expression| expression.id == initial)
            .ok_or(ExpansionError::MissingExpression(CheckedExprId(
                initial.0 as u32,
            )))?;
        let Some(declaration) = statement.declaration else {
            continue;
        };
        let owner = initial_expression.owner;
        if matches!(
            initial_expression.kind,
            ConcreteExpressionKind::Hold { .. } | ConcreteExpressionKind::Latest { .. }
        ) || matches!(
            initial_expression.kind,
            ConcreteExpressionKind::Call {
                callable_kind: ExecutableCallableKind::Builtin,
                ..
            } if initial_expression.effect.writes_state
        ) {
            continue;
        }
        if let Some(existing) = expressions.iter().find(|expression| {
            expression.owner == owner
                && resource_declaration(program, lookup, expression.checked_expr_id)
                    == Some(declaration)
                && (matches!(expression.kind, ConcreteExpressionKind::Hold { .. })
                    || matches!(
                        &expression.kind,
                        ConcreteExpressionKind::Latest { branches }
                            if executable_latest_has_initial(expressions, branches)
                    )
                    || matches!(
                        expression.kind,
                        ConcreteExpressionKind::Call {
                            callable_kind: ExecutableCallableKind::Builtin,
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
        let id = ConcreteExprId(expressions.len());
        let provenance = program
            .states
            .iter()
            .find(|state| {
                state.declaration == declaration
                    && state.expression == initial_expression.checked_expr_id
            })
            .map_or_else(runtime_value_provenance, |state| {
                ExecutableValueProvenance {
                    members: vec![ExecutableValueMember {
                        path: Vec::new(),
                        origin: ExecutableValueOrigin::State {
                            state: state.id,
                            owner: initial_expression.owner,
                        },
                    }],
                }
            });
        expressions.push(ConcreteExpression {
            id,
            checked_expr_id: initial_expression.checked_expr_id,
            flow_type: statement
                .flow_type
                .clone()
                .unwrap_or_else(|| initial_expression.flow_type.clone()),
            effect: boon_typecheck::CheckedEffectSummary {
                reads_state: true,
                writes_state: true,
                ..boon_typecheck::CheckedEffectSummary::default()
            },
            owner: initial_expression.owner,
            provenance,
            resource_binding_path: Some(binding_path.clone()),
            kind: ConcreteExpressionKind::Hold {
                initial,
                name: state_name,
                binding_path,
                updates,
            },
        });
        statement.value = Some(id);
    }
    Ok(())
}

fn append_expression_arena_without_roots(
    target: &mut Vec<ConcreteExpression>,
    mut source: Vec<ConcreteExpression>,
) {
    let offset = target.len();
    let local_binding_offset = target
        .iter()
        .flat_map(|expression| match &expression.kind {
            ConcreteExpressionKind::Block { bindings, .. } => {
                bindings.iter().map(|binding| binding.id).collect()
            }
            _ => Vec::new(),
        })
        .map(ExecutableLocalBindingId::as_usize)
        .max()
        .map_or(0, |maximum| maximum + 1);
    for expression in &mut source {
        expression.id = rebase_expr_id(expression.id, offset);
        rebase_expression_kind(&mut expression.kind, offset, local_binding_offset);
    }
    target.extend(source);
}

fn rebase_expr_id(expression: ConcreteExprId, offset: usize) -> ConcreteExprId {
    ConcreteExprId(expression.as_usize() + offset)
}

fn rebase_expression_kind(
    kind: &mut ConcreteExpressionKind,
    offset: usize,
    local_binding_offset: usize,
) {
    let rebase = |expression: &mut ConcreteExprId| {
        *expression = rebase_expr_id(*expression, offset);
    };
    match kind {
        ConcreteExpressionKind::CanonicalRead { .. }
        | ConcreteExpressionKind::ExternalRead { .. }
        | ConcreteExpressionKind::ElementState { .. }
        | ConcreteExpressionKind::Drain { .. }
        | ConcreteExpressionKind::Text(_)
        | ConcreteExpressionKind::Number(_)
        | ConcreteExpressionKind::BytesByte(_)
        | ConcreteExpressionKind::Bool(_)
        | ConcreteExpressionKind::Tag(_)
        | ConcreteExpressionKind::Source { .. }
        | ConcreteExpressionKind::Materialize { .. }
        | ConcreteExpressionKind::Delimiter
        | ConcreteExpressionKind::MaterializationLocal { .. }
        | ConcreteExpressionKind::FunctionParameter { .. } => {}
        ConcreteExpressionKind::LocalRead { binding, .. } => {
            *binding = ExecutableLocalBindingId(binding.as_usize() + local_binding_offset);
        }
        ConcreteExpressionKind::TextTemplate { segments } => {
            for value in segments.iter_mut().filter_map(|segment| match segment {
                ConcreteTextSegment::Static { .. } => None,
                ConcreteTextSegment::Dynamic { value } => Some(value),
            }) {
                rebase(value);
            }
        }
        ConcreteExpressionKind::TaggedObject { fields, .. }
        | ConcreteExpressionKind::Object(fields)
        | ConcreteExpressionKind::Record(fields) => {
            for field in fields {
                rebase(&mut field.value);
            }
        }
        ConcreteExpressionKind::Block { bindings, result } => {
            for binding in bindings {
                binding.id = ExecutableLocalBindingId(binding.id.as_usize() + local_binding_offset);
                rebase(&mut binding.value);
            }
            rebase(result);
        }
        ConcreteExpressionKind::Call { arguments, .. } => {
            for argument in arguments {
                rebase(&mut argument.value);
            }
        }
        ConcreteExpressionKind::Draining { input }
        | ConcreteExpressionKind::Project { input, .. } => rebase(input),
        ConcreteExpressionKind::Hold {
            initial, updates, ..
        } => {
            rebase(initial);
            for update in updates {
                rebase(update);
            }
        }
        ConcreteExpressionKind::Latest { branches } => {
            for branch in branches {
                rebase(branch);
            }
        }
        ConcreteExpressionKind::When { input, arms } => {
            rebase(input);
            for arm in arms {
                rebase(&mut arm.output);
            }
        }
        ConcreteExpressionKind::Then { input, output } => {
            rebase(input);
            if let Some(output) = output {
                rebase(output);
            }
        }
        ConcreteExpressionKind::Infix { left, right, .. } => {
            rebase(left);
            rebase(right);
        }
        ConcreteExpressionKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                rebase(output);
            }
        }
        ConcreteExpressionKind::List { items, .. }
        | ConcreteExpressionKind::Bytes { items, .. } => {
            for item in items {
                rebase(item);
            }
        }
    }
}

fn contextual_operation_formals(
    operation: CheckedContextualOperation,
) -> (
    ConcreteContextualOperation,
    DeclId,
    DeclId,
    DeclId,
    Option<DeclId>,
) {
    match operation {
        CheckedContextualOperation::Map { list, row, body } => {
            (ConcreteContextualOperation::Map, list, row, body, None)
        }
        CheckedContextualOperation::Filter {
            list,
            row,
            predicate,
        } => (
            ConcreteContextualOperation::Filter,
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
            ConcreteContextualOperation::Retain,
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
            ConcreteContextualOperation::Remove,
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
            ConcreteContextualOperation::Every,
            list,
            row,
            predicate,
            None,
        ),
        CheckedContextualOperation::Any {
            list,
            row,
            predicate,
        } => (ConcreteContextualOperation::Any, list, row, predicate, None),
        CheckedContextualOperation::Find {
            list,
            row,
            predicate,
        } => (
            ConcreteContextualOperation::Find,
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
            ConcreteContextualOperation::SortBy,
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
            ConcreteContextualOperation::ThenBy,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConcreteValueBinding {
    Local(ExecutableLocalBindingId),
    Expression(ConcreteExprId),
}

fn runtime_value_provenance() -> ExecutableValueProvenance {
    ExecutableValueProvenance {
        members: vec![ExecutableValueMember {
            path: Vec::new(),
            origin: ExecutableValueOrigin::Runtime,
        }],
    }
}

fn normalize_value_provenance(
    mut provenance: ExecutableValueProvenance,
) -> ExecutableValueProvenance {
    provenance.members.sort();
    provenance.members.dedup();
    provenance
}

fn record_value_provenance(
    expressions: &[ConcreteExpression],
    fields: &[ConcreteRecordField],
) -> ExecutableValueProvenance {
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
    normalize_value_provenance(ExecutableValueProvenance { members })
}

struct LocalProvenanceResolver<'a> {
    expressions: &'a [ConcreteExpression],
    bindings: BTreeMap<ExecutableLocalBindingId, (DeclId, ConcreteExprId)>,
    declarations: BTreeMap<(DeclId, Option<StaticOwnerId>), ConcreteExprId>,
    cache: BTreeMap<ConcreteExprId, ExecutableValueProvenance>,
    visiting: BTreeSet<ConcreteExprId>,
    stack: Vec<ConcreteExprId>,
}

impl<'a> LocalProvenanceResolver<'a> {
    fn resolve(
        &mut self,
        expression_id: ConcreteExprId,
    ) -> Result<ExecutableValueProvenance, ExpansionError> {
        if let Some(cached) = self.cache.get(&expression_id) {
            return Ok(cached.clone());
        }
        if !self.visiting.insert(expression_id) {
            let start = self
                .stack
                .iter()
                .position(|candidate| *candidate == expression_id)
                .unwrap_or(0);
            let mut cycle = self.stack[start..]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            cycle.push(expression_id.to_string());
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "provenance cycle {}",
                cycle.join(" -> ")
            )));
        }
        self.stack.push(expression_id);
        let expression = self
            .expressions
            .get(expression_id.as_usize())
            .filter(|candidate| candidate.id == expression_id)
            .cloned()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "provenance references missing expression {expression_id}"
                ))
            })?;
        let owner = expression.owner;
        let resolved = match expression.kind {
            ConcreteExpressionKind::LocalRead {
                binding,
                declaration,
                projection,
            } => {
                let (bound_declaration, value) =
                    self.bindings.get(&binding).copied().ok_or_else(|| {
                        ExpansionError::InvalidLocalBindings(format!(
                            "expression {expression_id} references missing local binding {binding}"
                        ))
                    })?;
                if bound_declaration != declaration {
                    return Err(ExpansionError::InvalidLocalBindings(format!(
                        "expression {expression_id} declaration {} differs from binding {binding} declaration {}",
                        declaration.0, bound_declaration.0
                    )));
                }
                self.resolve(value)?.projected(&projection)
            }
            ConcreteExpressionKind::Object(fields)
            | ConcreteExpressionKind::Record(fields)
            | ConcreteExpressionKind::TaggedObject { fields, .. } => {
                if fields.is_empty() {
                    runtime_value_provenance()
                } else {
                    let mut members = Vec::new();
                    for field in fields {
                        for mut member in self.resolve(field.value)?.members {
                            if !field.spread {
                                member.path.insert(0, field.name.clone());
                            }
                            members.push(member);
                        }
                    }
                    normalize_value_provenance(ExecutableValueProvenance { members })
                }
            }
            ConcreteExpressionKind::Block { result, .. } => self.resolve(result)?,
            ConcreteExpressionKind::Project { input, fields } => {
                self.resolve(input)?.projected(&fields)
            }
            ConcreteExpressionKind::CanonicalRead {
                target,
                projection,
                source: _,
                ..
            } => match self
                .declarations
                .get(&(target, owner))
                .or_else(|| self.declarations.get(&(target, None)))
                .copied()
            {
                Some(value) if value != expression_id => {
                    self.resolve(value)?.projected(&projection)
                }
                _ => expression.provenance,
            },
            ConcreteExpressionKind::Drain {
                target, projection, ..
            } => match self
                .declarations
                .get(&(target, owner))
                .or_else(|| self.declarations.get(&(target, None)))
                .copied()
            {
                Some(value) if value != expression_id => {
                    self.resolve(value)?.projected(&projection)
                }
                _ => expression.provenance,
            },
            ConcreteExpressionKind::Draining { input } => self.resolve(input)?,
            ConcreteExpressionKind::Latest { branches } => {
                let mut members = Vec::new();
                for branch in branches {
                    members.extend(self.resolve(branch)?.members);
                }
                normalize_value_provenance(ExecutableValueProvenance { members })
            }
            ConcreteExpressionKind::When { arms, .. } => {
                let mut members = Vec::new();
                for arm in arms {
                    members.extend(self.resolve(arm.output)?.members);
                }
                if members.is_empty() {
                    runtime_value_provenance()
                } else {
                    normalize_value_provenance(ExecutableValueProvenance { members })
                }
            }
            ConcreteExpressionKind::Then {
                output: Some(output),
                ..
            }
            | ConcreteExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => self.resolve(output)?,
            _ => expression.provenance,
        };
        self.stack.pop();
        self.visiting.remove(&expression_id);
        self.cache.insert(expression_id, resolved.clone());
        Ok(resolved)
    }
}

fn resolve_executable_local_provenance(
    expressions: &mut [ConcreteExpression],
    statements: &[ExecutableStatement],
) -> Result<(), ExpansionError> {
    let mut bindings = BTreeMap::new();
    for binding in expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            ConcreteExpressionKind::Block { bindings, .. } => Some(bindings.as_slice()),
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
        if binding != ExecutableLocalBindingId(index) {
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
        visiting: BTreeSet::new(),
        stack: Vec::new(),
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

pub(crate) struct ConcreteExpressionBuilder<'a> {
    program: &'a CheckedProgram,
    lookup: &'a CheckedProgramLookup,
    out_net: &'a OutNet,
    locals: BTreeMap<OutNetId, (StaticOwnerId, MaterializationLocalId)>,
    local_types: BTreeMap<(StaticOwnerId, MaterializationLocalId), Type>,
    materializations_by_owner: &'a BTreeMap<StaticOwnerId, usize>,
    materialization_result_types: &'a BTreeMap<usize, Type>,
    expressions: Vec<ConcreteExpression>,
    memo: BTreeMap<ExpansionKey, ConcreteExprId>,
    visiting: BTreeSet<ExpansionKey>,
    visiting_stack: Vec<ExpansionKey>,
    owner_stack: Vec<Option<StaticOwnerId>>,
    frame_stack: Vec<Option<OutCallInstanceId>>,
    value_frames: Vec<BTreeMap<DeclId, ConcreteValueBinding>>,
    next_local_binding: usize,
}

impl<'a> ConcreteExpressionBuilder<'a> {
    fn new(
        program: &'a CheckedProgram,
        lookup: &'a CheckedProgramLookup,
        out_net: &'a OutNet,
        locals: BTreeMap<OutNetId, (StaticOwnerId, MaterializationLocalId)>,
        local_types: BTreeMap<StaticOwnerId, Type>,
        materializations_by_owner: &'a BTreeMap<StaticOwnerId, usize>,
        materialization_result_types: &'a BTreeMap<usize, Type>,
    ) -> Self {
        Self {
            program,
            lookup,
            out_net,
            locals,
            local_types: local_types
                .into_iter()
                .map(|(owner, ty)| ((owner, MaterializationLocalId(0)), ty))
                .collect(),
            materializations_by_owner,
            materialization_result_types,
            expressions: Vec::new(),
            memo: BTreeMap::new(),
            visiting: BTreeSet::new(),
            visiting_stack: Vec::new(),
            owner_stack: Vec::new(),
            frame_stack: Vec::new(),
            value_frames: Vec::new(),
            next_local_binding: 0,
        }
    }

    fn set_local_type(&mut self, owner: StaticOwnerId, local: MaterializationLocalId, ty: Type) {
        self.local_types.insert((owner, local), ty);
    }

    fn direct_resource_provenance(
        &self,
        expression: CheckedExprId,
        owner: Option<StaticOwnerId>,
    ) -> Option<ExecutableValueProvenance> {
        if let Some(source) = self
            .program
            .sources
            .iter()
            .find(|source| source.expression == expression)
        {
            return Some(ExecutableValueProvenance {
                members: vec![ExecutableValueMember {
                    path: Vec::new(),
                    origin: ExecutableValueOrigin::Source {
                        source: source.id,
                        owner,
                    },
                }],
            });
        }
        self.program
            .states
            .iter()
            .find(|state| state.expression == expression)
            .map(|state| ExecutableValueProvenance {
                members: vec![ExecutableValueMember {
                    path: Vec::new(),
                    origin: ExecutableValueOrigin::State {
                        state: state.id,
                        owner,
                    },
                }],
            })
    }

    fn value_provenance(
        &self,
        expression: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        kind: &ConcreteExpressionKind,
    ) -> ExecutableValueProvenance {
        if let Some(provenance) = self.direct_resource_provenance(expression.id, owner) {
            return provenance;
        }
        let child = |id: ConcreteExprId| {
            self.expressions
                .get(id.as_usize())
                .map(|expression| expression.provenance.clone())
                .unwrap_or_else(runtime_value_provenance)
        };
        match kind {
            ConcreteExpressionKind::Object(fields)
            | ConcreteExpressionKind::Record(fields)
            | ConcreteExpressionKind::TaggedObject { fields, .. } => {
                record_value_provenance(&self.expressions, fields)
            }
            ConcreteExpressionKind::Block { result, .. } => child(*result),
            ConcreteExpressionKind::Project { input, fields } => child(*input).projected(fields),
            ConcreteExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => ExecutableValueProvenance {
                members: vec![ExecutableValueMember {
                    path: Vec::new(),
                    origin: ExecutableValueOrigin::MaterializationLocal {
                        owner: *owner,
                        local: *local,
                        projection: projection.clone(),
                    },
                }],
            },
            ConcreteExpressionKind::Draining { input } => child(*input),
            ConcreteExpressionKind::Latest { branches } => {
                let members = branches
                    .iter()
                    .flat_map(|branch| child(*branch).members)
                    .collect();
                normalize_value_provenance(ExecutableValueProvenance { members })
            }
            ConcreteExpressionKind::When { arms, .. } => {
                let members = arms
                    .iter()
                    .flat_map(|arm| child(arm.output).members)
                    .collect::<Vec<_>>();
                if members.is_empty() {
                    runtime_value_provenance()
                } else {
                    normalize_value_provenance(ExecutableValueProvenance { members })
                }
            }
            ConcreteExpressionKind::Then {
                output: Some(output),
                ..
            }
            | ConcreteExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => child(*output),
            ConcreteExpressionKind::Source { .. } => {
                debug_assert!(
                    false,
                    "checked SOURCE expression has no CheckedSource provenance"
                );
                ExecutableValueProvenance::default()
            }
            ConcreteExpressionKind::CanonicalRead {
                source: Some(source),
                ..
            } if source.payload_projection.is_empty() => ExecutableValueProvenance {
                members: vec![ExecutableValueMember {
                    path: Vec::new(),
                    origin: ExecutableValueOrigin::Source {
                        source: source.source,
                        owner,
                    },
                }],
            },
            ConcreteExpressionKind::CanonicalRead { .. }
            | ConcreteExpressionKind::LocalRead { .. }
            | ConcreteExpressionKind::ExternalRead { .. }
            | ConcreteExpressionKind::ElementState { .. }
            | ConcreteExpressionKind::Drain { .. }
            | ConcreteExpressionKind::Text(_)
            | ConcreteExpressionKind::TextTemplate { .. }
            | ConcreteExpressionKind::Number(_)
            | ConcreteExpressionKind::BytesByte(_)
            | ConcreteExpressionKind::Bool(_)
            | ConcreteExpressionKind::Tag(_)
            | ConcreteExpressionKind::Call { .. }
            | ConcreteExpressionKind::Materialize { .. }
            | ConcreteExpressionKind::Hold { .. }
            | ConcreteExpressionKind::Then { output: None, .. }
            | ConcreteExpressionKind::Infix { .. }
            | ConcreteExpressionKind::MatchArm { output: None, .. }
            | ConcreteExpressionKind::List { .. }
            | ConcreteExpressionKind::Bytes { .. }
            | ConcreteExpressionKind::Delimiter
            | ConcreteExpressionKind::FunctionParameter { .. } => runtime_value_provenance(),
        }
    }

    pub(crate) fn expand(
        &mut self,
        expression: ScopedCheckedExpr,
    ) -> Result<ConcreteExprId, ExpansionError> {
        let inherited_owner = self.owner_stack.last().copied().flatten();
        let evaluation_owner = self.evaluation_owner(expression).or(inherited_owner);
        let key = ExpansionKey {
            expression: expression.expression,
            frame: expression.frame,
            value_frame: expression.value_frame,
            evaluation_owner,
        };
        if let Some(existing) = self.memo.get(&key).copied() {
            return Ok(existing);
        }
        if !self.visiting.insert(key) {
            let cycle_start = self
                .visiting_stack
                .iter()
                .position(|candidate| *candidate == key)
                .unwrap_or(0);
            let chain = self.visiting_stack[cycle_start..]
                .iter()
                .copied()
                .chain(std::iter::once(key))
                .map(|key| {
                    self.lookup
                        .expression(self.program, key.expression)
                        .map(|expression| {
                            format!(
                                "{}:{:?}@{:?}:line{}",
                                key.expression.0, expression.kind, key.frame, expression.span.line
                            )
                        })
                        .unwrap_or_else(|| format!("{}@{:?}", key.expression.0, key.frame))
                })
                .collect();
            return Err(ExpansionError::ExpressionCycle {
                expression: expression.expression,
                frame: expression.frame,
                chain,
            });
        }
        self.visiting_stack.push(key);
        self.owner_stack.push(evaluation_owner);
        self.frame_stack.push(expression.frame);
        let result = self.expand_uncached(expression, evaluation_owner);
        let popped_frame = self
            .frame_stack
            .pop()
            .expect("contextual expansion frame stack contains active expression");
        debug_assert_eq!(popped_frame, expression.frame);
        let popped_owner = self
            .owner_stack
            .pop()
            .expect("contextual expansion owner stack contains active expression");
        debug_assert_eq!(popped_owner, evaluation_owner);
        let popped = self
            .visiting_stack
            .pop()
            .expect("contextual expansion stack contains active expression");
        debug_assert_eq!(popped, key);
        self.visiting.remove(&key);
        let result = result?;
        self.memo.insert(key, result);
        Ok(result)
    }

    pub(crate) fn finish(self) -> Vec<ConcreteExpression> {
        self.expressions
    }

    fn expand_with_inherited_owner(
        &mut self,
        expression: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
    ) -> Result<ConcreteExprId, ExpansionError> {
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
    ) -> Result<ConcreteExprId, ExpansionError> {
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
                        ConcreteExpressionKind::ElementState {
                            context: ExecutableCallContextId {
                                call_instance: instance.as_usize(),
                                ordinal: context.signature,
                            },
                            projection,
                        },
                    ));
                }
                if let Some(binding) = scoped
                    .value_frame
                    .and_then(|frame| self.value_frames.get(frame))
                    .and_then(|bindings| bindings.get(&target))
                    .copied()
                {
                    return match binding {
                        ConcreteValueBinding::Local(binding) => Ok(self.push(
                            &expression,
                            owner,
                            ConcreteExpressionKind::LocalRead {
                                binding,
                                declaration: target,
                                projection,
                            },
                        )),
                        ConcreteValueBinding::Expression(input) => {
                            self.project(&expression, owner, input, projection)
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
                        ConcreteExpressionKind::MaterializationLocal {
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
                                .unwrap_or(Type::Unknown);
                            let parameter = self.push(
                                &expression,
                                owner,
                                ConcreteExpressionKind::FunctionParameter {
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
                ConcreteExpressionKind::CanonicalRead {
                    target,
                    path: canonical_declaration_path(self.program, self.lookup, target)
                        .ok_or(ExpansionError::MissingDeclaration(target))?,
                    projection,
                    source,
                }
            }
            CheckedExpressionKind::Passed { projection } => {
                let passed = scoped
                    .frame
                    .and_then(|frame| self.out_net.call_instances[frame.as_usize()].passed)
                    .ok_or(ExpansionError::MissingPassedContext(scoped.expression))?;
                let argument_owner = self
                    .out_net
                    .owner_for_call_evaluation(passed.evaluation_call);
                let expanded = self.expand_with_inherited_owner(passed.value, argument_owner)?;
                return self.project(&expression, owner, expanded, projection);
            }
            CheckedExpressionKind::ExternalRead { canonical_path } => {
                if let Some((target, projection)) =
                    self.resolve_ambient_read(scoped.frame, expression.scope_id, &canonical_path)
                {
                    ConcreteExpressionKind::CanonicalRead {
                        target,
                        path: canonical_declaration_path(self.program, self.lookup, target)
                            .ok_or(ExpansionError::MissingDeclaration(target))?,
                        projection,
                        source: None,
                    }
                } else if canonical_path.contains('/') {
                    ConcreteExpressionKind::ExternalRead { canonical_path }
                } else {
                    return Err(ExpansionError::UnresolvedAmbientRead {
                        expression: scoped.expression,
                        path: canonical_path,
                    });
                }
            }
            CheckedExpressionKind::Drain { target, projection } => ConcreteExpressionKind::Drain {
                target,
                path: canonical_declaration_path(self.program, self.lookup, target)
                    .ok_or(ExpansionError::MissingDeclaration(target))?,
                projection,
            },
            CheckedExpressionKind::Text { value } => ConcreteExpressionKind::Text(value),
            CheckedExpressionKind::TextTemplate { segments } => {
                ConcreteExpressionKind::TextTemplate {
                    segments: segments
                        .into_iter()
                        .map(|segment| match segment {
                            CheckedTextSegment::Static { value } => {
                                Ok(ConcreteTextSegment::Static { value })
                            }
                            CheckedTextSegment::Dynamic { value } => self
                                .expand_in_frame(value, scoped.frame, scoped.value_frame)
                                .map(|value| ConcreteTextSegment::Dynamic { value }),
                        })
                        .collect::<Result<Vec<_>, ExpansionError>>()?,
                }
            }
            CheckedExpressionKind::Number { value } => ConcreteExpressionKind::Number(value),
            CheckedExpressionKind::BytesByte { value } => ConcreteExpressionKind::BytesByte(value),
            CheckedExpressionKind::Bool { value } => ConcreteExpressionKind::Bool(value),
            CheckedExpressionKind::Tag { name } => ConcreteExpressionKind::Tag(name),
            CheckedExpressionKind::TaggedObject { tag, fields } => {
                ConcreteExpressionKind::TaggedObject {
                    tag,
                    fields: self.expand_fields(scoped.frame, scoped.value_frame, fields)?,
                }
            }
            CheckedExpressionKind::Source => ConcreteExpressionKind::Source {
                binding_path: self
                    .concrete_resource_binding_path(scoped.expression, scoped.frame, owner)
                    .ok_or(ExpansionError::MissingSourceDeclaration(scoped.expression))?,
            },
            CheckedExpressionKind::Call { call } => {
                return self.expand_call(&expression, scoped, owner, call);
            }
            CheckedExpressionKind::Draining { input } => ConcreteExpressionKind::Draining {
                input: self.expand_in_frame(input, scoped.frame, scoped.value_frame)?,
            },
            CheckedExpressionKind::Hold { initial, name } => ConcreteExpressionKind::Hold {
                initial: self.expand_in_frame(initial, scoped.frame, scoped.value_frame)?,
                binding_path: self
                    .concrete_resource_binding_path(scoped.expression, scoped.frame, owner)
                    .unwrap_or_else(|| name.clone()),
                name,
                updates: self.expand_statement_child_values(scoped)?,
            },
            CheckedExpressionKind::Latest { branches } => ConcreteExpressionKind::Latest {
                branches: self.expand_many(scoped.frame, scoped.value_frame, branches)?,
            },
            CheckedExpressionKind::When { input, arms }
            | CheckedExpressionKind::While { input, arms } => {
                let input = self.expand_in_frame(input, scoped.frame, scoped.value_frame)?;
                ConcreteExpressionKind::When {
                    input,
                    arms: self.expand_select_arms(scoped, owner, input, &arms)?,
                }
            }
            CheckedExpressionKind::Then { input, output } => ConcreteExpressionKind::Then {
                input: self.expand_in_frame(input, scoped.frame, scoped.value_frame)?,
                output: output
                    .map(|output| self.expand_in_frame(output, scoped.frame, scoped.value_frame))
                    .transpose()?,
            },
            CheckedExpressionKind::Infix { left, op, right } => ConcreteExpressionKind::Infix {
                left: self.expand_in_frame(left, scoped.frame, scoped.value_frame)?,
                op,
                right: self.expand_in_frame(right, scoped.frame, scoped.value_frame)?,
            },
            CheckedExpressionKind::MatchArm {
                pattern, output, ..
            } => ConcreteExpressionKind::MatchArm {
                pattern,
                output: output
                    .map(|output| self.expand_in_frame(output, scoped.frame, scoped.value_frame))
                    .transpose()?,
            },
            CheckedExpressionKind::Block { bindings, result } => {
                let frame = self.value_frames.len();
                let mut values = scoped
                    .value_frame
                    .and_then(|frame| self.value_frames.get(frame))
                    .cloned()
                    .unwrap_or_default();
                let binding_ids = bindings
                    .iter()
                    .map(|binding| {
                        let id = ExecutableLocalBindingId(self.next_local_binding);
                        self.next_local_binding += 1;
                        (binding.declaration, id)
                    })
                    .collect::<BTreeMap<_, _>>();
                for binding in &bindings {
                    values.insert(
                        binding.declaration,
                        ConcreteValueBinding::Local(binding_ids[&binding.declaration]),
                    );
                }
                self.value_frames.push(values);
                let result = result.ok_or(ExpansionError::MissingExpression(scoped.expression))?;
                let bindings = bindings
                    .into_iter()
                    .map(|binding| {
                        Ok(ConcreteBlockBinding {
                            id: binding_ids[&binding.declaration],
                            declaration: binding.declaration,
                            value: self.expand(ScopedCheckedExpr {
                                expression: binding.value,
                                frame: scoped.frame,
                                evaluation_port: None,
                                value_frame: Some(frame),
                            })?,
                        })
                    })
                    .collect::<Result<Vec<_>, ExpansionError>>()?;
                let result = self.expand(ScopedCheckedExpr {
                    expression: result,
                    frame: scoped.frame,
                    evaluation_port: None,
                    value_frame: Some(frame),
                })?;
                ConcreteExpressionKind::Block { bindings, result }
            }
            CheckedExpressionKind::Object { fields } => ConcreteExpressionKind::Object(
                self.expand_fields(scoped.frame, scoped.value_frame, fields)?,
            ),
            CheckedExpressionKind::Record { fields } => ConcreteExpressionKind::Record(
                self.expand_fields(scoped.frame, scoped.value_frame, fields)?,
            ),
            CheckedExpressionKind::List { capacity, items } => ConcreteExpressionKind::List {
                capacity,
                items: self.expand_many(scoped.frame, scoped.value_frame, items)?,
            },
            CheckedExpressionKind::Bytes { fixed_size, items } => ConcreteExpressionKind::Bytes {
                fixed_size,
                items: self.expand_many(scoped.frame, scoped.value_frame, items)?,
            },
            CheckedExpressionKind::Delimiter => ConcreteExpressionKind::Delimiter,
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
    ) -> Result<ConcreteExprId, ExpansionError> {
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
                ConcreteExpressionKind::Materialize { materialization },
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
            return self.expand_with_inherited_owner(
                ScopedCheckedExpr {
                    expression: result,
                    frame: Some(instance),
                    evaluation_port: None,
                    value_frame: scoped.value_frame,
                },
                call_owner,
            );
        }
        if checked_call.pass.is_some() {
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
            arguments.push(ConcreteCallArgument {
                ordinal: parameter.ordinal,
                name: parameter.name.clone(),
                value: self.expand_with_inherited_owner(
                    input.checked_value().ok_or(ExpansionError::MissingFormal {
                        callable: callable.decl_id,
                        formal: input.formal,
                    })?,
                    argument_owner,
                )?,
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
        let kind = match callable.kind {
            CheckedCallableKind::Builtin => ExecutableCallableKind::Builtin,
            CheckedCallableKind::External => ExecutableCallableKind::External,
            CheckedCallableKind::User => unreachable!("user calls are expanded above"),
        };
        let contexts = checked_call
            .contexts
            .iter()
            .map(|context| ExecutableCallContextId {
                call_instance: instance.as_usize(),
                ordinal: context.signature,
            })
            .collect();
        Ok(self.push(
            expression,
            owner,
            ConcreteExpressionKind::Call {
                callable_kind: kind,
                name: callable.name.clone(),
                instance: instance.as_usize(),
                arguments,
                contexts,
            },
        ))
    }

    fn expand_in_frame(
        &mut self,
        expression: CheckedExprId,
        frame: Option<OutCallInstanceId>,
        value_frame: Option<usize>,
    ) -> Result<ConcreteExprId, ExpansionError> {
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
    ) -> Result<Vec<ConcreteExprId>, ExpansionError> {
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
    ) -> Result<Vec<ConcreteRecordField>, ExpansionError> {
        fields
            .into_iter()
            .map(|field| {
                Ok(ConcreteRecordField {
                    declaration: field.declaration,
                    name: field.name,
                    value: self.expand_in_frame(field.value, frame, value_frame)?,
                    spread: field.spread,
                })
            })
            .collect()
    }

    fn expand_statement_child_values(
        &mut self,
        scoped: ScopedCheckedExpr,
    ) -> Result<Vec<ConcreteExprId>, ExpansionError> {
        self.semantic_statement_child_values(scoped.expression)
            .into_iter()
            .map(|expression| self.expand_in_frame(expression, scoped.frame, scoped.value_frame))
            .collect()
    }

    fn expand_select_arms(
        &mut self,
        scoped: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
        input: ConcreteExprId,
        arm_ids: &[CheckedExprId],
    ) -> Result<Vec<ExecutableSelectArm>, ExpansionError> {
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
                let frame = self.value_frames.len();
                let mut values = scoped
                    .value_frame
                    .and_then(|frame| self.value_frames.get(frame))
                    .cloned()
                    .unwrap_or_default();
                for binding in bindings {
                    let projection = self
                        .lookup
                        .pattern_binding(self.program, *binding)
                        .ok_or(ExpansionError::MissingDeclaration(*binding))?
                        .projection
                        .clone();
                    let value = self.project(&expression, owner, input, projection)?;
                    values.insert(*binding, ConcreteValueBinding::Expression(value));
                }
                self.value_frames.push(values);
                Some(frame)
            };
            arms.push(ExecutableSelectArm {
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
                        Ok(ExecutablePatternBinding {
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
    ) -> Result<ConcreteExprId, ExpansionError> {
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
                Ok(ConcreteRecordField {
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
            ConcreteExpressionKind::Object(fields),
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
        values
    }

    fn project(
        &mut self,
        expression: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        input: ConcreteExprId,
        fields: Vec<String>,
    ) -> Result<ConcreteExprId, ExpansionError> {
        if fields.is_empty() {
            return Ok(input);
        }
        let projected_type = project_concrete_type(
            self.expressions[input.as_usize()].flow_type.ty.clone(),
            &fields,
        );
        let direct_field = match &self.expressions[input.as_usize()].kind {
            ConcreteExpressionKind::Object(record_fields)
            | ConcreteExpressionKind::Record(record_fields)
                if record_fields.iter().all(|field| !field.spread) =>
            {
                let matches = record_fields
                    .iter()
                    .filter(|field| field.name == fields[0])
                    .map(|field| field.value)
                    .collect::<Vec<_>>();
                matches
                    .as_slice()
                    .first()
                    .copied()
                    .filter(|_| matches.len() == 1)
            }
            ConcreteExpressionKind::TaggedObject {
                fields: record_fields,
                ..
            } if record_fields.iter().all(|field| !field.spread) => {
                let matches = record_fields
                    .iter()
                    .filter(|field| field.name == fields[0])
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
        if let Some(direct_field) = direct_field {
            return self.project(expression, owner, direct_field, fields[1..].to_vec());
        }
        let kind = match &self.expressions[input.as_usize()].kind {
            ConcreteExpressionKind::CanonicalRead {
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
                ConcreteExpressionKind::CanonicalRead {
                    target: *target,
                    path: path.clone(),
                    projection,
                    source,
                }
            }
            ConcreteExpressionKind::MaterializationLocal {
                owner: local_owner,
                local,
                projection,
            } => {
                let mut projection = projection.clone();
                projection.extend(fields);
                ConcreteExpressionKind::MaterializationLocal {
                    owner: *local_owner,
                    local: *local,
                    projection,
                }
            }
            _ => ConcreteExpressionKind::Project { input, fields },
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
        kind: ConcreteExpressionKind,
    ) -> ConcreteExprId {
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
        let id = ConcreteExprId(self.expressions.len());
        self.expressions.push(ConcreteExpression {
            id,
            checked_expr_id: expression.id,
            flow_type,
            effect: expression.effect,
            owner,
            provenance,
            resource_binding_path,
            kind,
        });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::out_net::OutNet;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum BodyShape {
        Number(String),
        Infix(Box<BodyShape>, String, Box<BodyShape>),
        Local(MaterializationLocalId, Vec<String>),
        Project(Box<BodyShape>, Vec<String>),
    }

    fn body_shape(expressions: &[ConcreteExpression], expression: ConcreteExprId) -> BodyShape {
        match &expressions[expression.as_usize()].kind {
            ConcreteExpressionKind::Number(value) => BodyShape::Number(value.clone()),
            ConcreteExpressionKind::Infix { left, op, right } => BodyShape::Infix(
                Box::new(body_shape(expressions, *left)),
                op.clone(),
                Box::new(body_shape(expressions, *right)),
            ),
            ConcreteExpressionKind::MaterializationLocal {
                local, projection, ..
            } => BodyShape::Local(*local, projection.clone()),
            ConcreteExpressionKind::Project { input, fields } => {
                BodyShape::Project(Box::new(body_shape(expressions, *input)), fields.clone())
            }
            other => panic!("unexpected concrete body node: {other:?}"),
        }
    }

    fn materialization(source: &str) -> (ConcreteMaterialization, Vec<ConcreteExpression>) {
        let parsed = boon_parser::parse_source("contextual-equivalence.bn", source).unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let (materializations, expressions) =
            derive_contextual_materializations(&checked, &out_net.graph).unwrap();
        let [materialization] = materializations
            .try_into()
            .unwrap_or_else(|values: Vec<_>| {
                panic!("expected one materialization, found {}", values.len())
            });
        (materialization, expressions)
    }

    fn erased_program(source: &str) -> (ConcreteMaterialization, ExecutableProgram) {
        let parsed = boon_parser::parse_source("contextual-erasure.bn", source).unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let (materializations, expressions) =
            derive_contextual_materializations(&checked, &out_net.graph).unwrap();
        let executable =
            derive_executable_program(&checked, &out_net.graph, &materializations, expressions)
                .unwrap();
        let [materialization] = materializations
            .try_into()
            .unwrap_or_else(|values: Vec<_>| {
                panic!("expected one materialization, found {}", values.len())
            });
        (materialization, executable)
    }

    fn executable_program(source: &str) -> (CheckedProgram, ExecutableProgram) {
        let parsed = boon_parser::parse_source("executable-expansion.bn", source).unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let (materializations, expressions) =
            derive_contextual_materializations(&checked, &out_net.graph).unwrap();
        let executable =
            derive_executable_program(&checked, &out_net.graph, &materializations, expressions)
                .unwrap();
        (checked, executable)
    }

    #[test]
    fn repeated_function_blocks_receive_distinct_local_binding_ids() {
        let (_, executable) = executable_program(
            r#"
FUNCTION wrapped(value) {
    BLOCK {
        local: value
        local
    }
}

first: wrapped(value: 1)
second: wrapped(value: 2)
"#,
        );
        let bindings = executable
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                ConcreteExpressionKind::Block { bindings, .. } if bindings.len() == 1 => {
                    Some(&bindings[0])
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), 2, "{executable:#?}");
        assert_eq!(
            bindings[0].declaration, bindings[1].declaration,
            "both call sites originate from the same checked function declaration"
        );
        assert_ne!(
            bindings[0].id, bindings[1].id,
            "each expanded call site must own a distinct lexical binding"
        );

        let read_bindings = executable
            .expressions
            .iter()
            .filter_map(|expression| match expression.kind {
                ConcreteExpressionKind::LocalRead { binding, .. } => Some(binding),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert!(
            bindings
                .iter()
                .all(|binding| read_bindings.contains(&binding.id)),
            "local reads must retain the exact call-site binding IDs: {executable:#?}"
        );
    }

    #[test]
    fn forward_local_alias_preserves_nested_source_provenance() {
        let (_, executable) = executable_program(
            r#"
row:
    BLOCK {
        forwarded: base
        base: [control: SOURCE]
        forwarded
    }
"#,
        );
        let forward_value = executable
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                ConcreteExpressionKind::Block { bindings, .. } if bindings.len() == 2 => {
                    Some(bindings[0].value)
                }
                _ => None,
            })
            .expect("two-binding forward-alias block");
        let provenance = &executable.expressions[forward_value.as_usize()].provenance;
        assert!(
            provenance.members.iter().any(|member| {
                member.path == ["control"]
                    && matches!(member.origin, ExecutableValueOrigin::Source { .. })
            }),
            "forward alias lost nested SOURCE provenance: {provenance:#?}"
        );
    }

    #[test]
    fn mapped_function_source_keeps_its_complete_nested_binding_path() {
        let (_, executable) = executable_program(
            r#"
FUNCTION new_row(row) {
    [
        controls: [select: SOURCE]
        address: row.address
    ]
}

rows:
    LIST { [address: TEXT { one }] }
    |> List/map(item, new: new_row(row: item))
"#,
        );
        let sources = executable
            .sources
            .iter()
            .map(|source| source.binding_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(sources, ["controls.select"], "{executable:#?}");
        let resource_paths = executable
            .sources
            .iter()
            .map(|source| {
                executable.expressions[source.expression.as_usize()]
                    .resource_binding_path
                    .as_deref()
            })
            .collect::<Vec<_>>();
        assert_eq!(resource_paths, [Some("rows.controls.select")]);
    }

    #[test]
    fn deferred_parent_argument_keeps_its_resource_call_owner_inside_nested_outputs() {
        let (_, executable) = executable_program(
            r#"
FUNCTION stateful_row(seed) {
    [
        update: SOURCE
        held: seed |> HOLD held { update.text }
        found: lookup(needle: held)
    ]
}

FUNCTION lookup(needle) {
    LIST { [key: TEXT { one }] }
    |> List/find(item, if: item.key == needle)
}

rows:
    LIST { [seed: TEXT { one }] }
    |> List/map(item, new: stateful_row(seed: item.seed))
"#,
        );
        let state = executable
            .states
            .iter()
            .find(|state| state.binding_path == "held")
            .expect("held state");
        let reads = executable
            .expressions
            .iter()
            .filter(|expression| {
                matches!(
                    expression.kind,
                    ConcreteExpressionKind::CanonicalRead { target, .. }
                        if target == state.declaration
                )
            })
            .collect::<Vec<_>>();
        assert!(!reads.is_empty(), "held state must have a concrete read");
        assert!(
            reads.iter().all(|read| read.owner == state.owner),
            "deferred parent argument escaped its resource-call owner: {reads:#?}"
        );
    }

    #[test]
    fn passed_value_keeps_its_call_site_owner_inside_nested_outputs() {
        let (_, executable) = executable_program(
            r#"
FUNCTION stateful_row(seed) {
    [
        update: SOURCE
        held: seed |> HOLD held { update.text }
        found: lookup(PASS: [needle: held])
    ]
}

FUNCTION lookup() {
    LIST { [key: TEXT { one }] }
    |> List/find(item, if: item.key == PASSED.needle)
}

rows:
    LIST { [seed: TEXT { one }] }
    |> List/map(item, new: stateful_row(seed: item.seed))
"#,
        );
        let state = executable
            .states
            .iter()
            .find(|state| state.binding_path == "held")
            .expect("held state");
        let reads = executable
            .expressions
            .iter()
            .filter(|expression| {
                matches!(
                    expression.kind,
                    ConcreteExpressionKind::CanonicalRead { target, .. }
                        if target == state.declaration
                )
            })
            .collect::<Vec<_>>();
        assert!(!reads.is_empty(), "PASSED value must retain the state read");
        assert!(
            reads.iter().all(|read| read.owner == state.owner),
            "PASSED value escaped its call-site owner: {reads:#?}"
        );
    }

    #[test]
    fn contextual_function_preserves_nested_pattern_payload_bindings() {
        let (_, executable) = executable_program(
            r#"
FUNCTION startup_file_record(records, file) {
    records |> List/find(item, if: item.file == file)
}

records: LIST { [file: TEXT { simple.vcd }, primary_signal: TEXT { clk }] }
rows: LIST { [file: TEXT { simple.vcd }] }
result:
    rows
    |> List/map(item, new:
        startup_file_record(records: records, file: item.file) |> WHEN {
            Found[value] => value.primary_signal
            NotFound => TEXT { none }
        }
    )
"#,
        );
        assert!(executable.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                ConcreteExpressionKind::Project { fields, .. }
                    if fields == &["primary_signal".to_owned()]
            )
        }));
    }

    #[test]
    fn nested_user_calls_inherit_passed_context() {
        let parsed = boon_parser::parse_source(
            "nested-passed-context.bn",
            r#"
FUNCTION outer() {
    inner()
}

FUNCTION inner() {
    PASSED.value
}

result: outer(PASS: [value: 42])
"#,
        )
        .unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let (materializations, expressions) =
            derive_contextual_materializations(&checked, &out_net.graph).unwrap();
        assert!(materializations.is_empty());
        let executable =
            derive_executable_program(&checked, &out_net.graph, &materializations, expressions)
                .expect("nested user calls inherit the outer PASS context");
        assert!(executable.expressions.iter().any(|expression| {
            matches!(&expression.kind, ConcreteExpressionKind::Number(value) if value == "42")
        }));
    }

    #[test]
    fn explicit_nested_pass_replaces_the_inherited_context() {
        let parsed = boon_parser::parse_source(
            "nested-passed-context-override.bn",
            r#"
FUNCTION outer() {
    inner(PASS: [value: 2])
}

FUNCTION inner() {
    PASSED.value
}

result: outer(PASS: [value: 1])
"#,
        )
        .unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let outer_call = checked
            .calls
            .iter()
            .find(|call| call.function == "outer")
            .expect("outer call");
        let inner_call = checked
            .calls
            .iter()
            .find(|call| call.function == "inner")
            .expect("inner call");
        let inner_pass = inner_call.pass.expect("inner call has an explicit PASS");
        assert_ne!(
            outer_call.pass.expect("outer call has PASS").value,
            inner_pass.value
        );

        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let outer_instance = out_net
            .graph
            .call_instance_for_checked_call(outer_call.id, None)
            .expect("outer instance");
        let inner_instance = out_net
            .graph
            .call_instance_for_checked_call(inner_call.id, Some(outer_instance))
            .expect("inner instance");
        let passed = out_net.graph.call_instances[inner_instance.as_usize()]
            .passed
            .expect("inner instance owns PASS");
        assert_eq!(passed.value.expression, inner_pass.value);
        assert_eq!(passed.value.frame, Some(outer_instance));
        assert_eq!(passed.evaluation_call, inner_instance);
    }

    #[test]
    fn nonrequiring_nested_call_does_not_receive_ambient_pass() {
        let parsed = boon_parser::parse_source(
            "nested-passed-context-isolation.bn",
            r#"
FUNCTION outer() {
    BLOCK {
        observed: PASSED.value
        passthrough(value: 7)
    }
}

FUNCTION passthrough(value) {
    value
}

result: outer(PASS: [value: 42])
"#,
        )
        .unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let outer_call = checked
            .calls
            .iter()
            .find(|call| call.function == "outer")
            .expect("outer call");
        let nested_call = checked
            .calls
            .iter()
            .find(|call| call.function == "passthrough")
            .expect("nested passthrough call");
        assert!(
            !checked
                .callables
                .iter()
                .find(|callable| callable.decl_id == nested_call.callable)
                .expect("passthrough callable")
                .requires_pass()
        );

        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let outer_instance = out_net
            .graph
            .call_instance_for_checked_call(outer_call.id, None)
            .expect("outer instance");
        let nested_instance = out_net
            .graph
            .call_instance_for_checked_call(nested_call.id, Some(outer_instance))
            .expect("nested passthrough instance");
        assert!(
            out_net.graph.call_instances[nested_instance.as_usize()]
                .passed
                .is_none(),
            "ambient PASS leaked into a callable without a checked requirement"
        );
    }

    #[test]
    fn malformed_checked_root_requirement_fails_without_ambient_pass() {
        let parsed = boon_parser::parse_source(
            "missing-root-passed-context.bn",
            r#"
FUNCTION contextual() {
    PASSED.value
}

result: contextual(PASS: [value: 42])
"#,
        )
        .unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let mut checked = output.program.expect("valid source has a checked program");
        checked
            .calls
            .iter_mut()
            .find(|call| call.function == "contextual")
            .expect("contextual call")
            .pass = None;

        let out_net = OutNet::build(&checked);
        assert!(out_net.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic,
                crate::out_net::OutNetDiagnostic::MissingPassedContext { .. }
            )
        }));
    }

    #[test]
    fn selector_context_does_not_duplicate_a_checked_host_effect_call() {
        let (checked, executable) = executable_program(
            r#"
store: [
    start: SOURCE
    result:
        RandomNotRequested |> HOLD result {
            start |> THEN {
                True |> WHEN {
                    True => Random/bytes(byte_count: 1)
                    False => SKIP
                }
            }
        }
]
"#,
        );
        let checked_call = checked
            .calls
            .iter()
            .find(|call| call.function == "Random/bytes")
            .expect("checked Random/bytes call");
        let executable_calls = executable
            .expressions
            .iter()
            .filter(|expression| expression.checked_expr_id == checked_call.expression)
            .filter(|expression| {
                matches!(
                    &expression.kind,
                    ConcreteExpressionKind::Call { name, .. } if name == "Random/bytes"
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            executable_calls.len(),
            1,
            "one semantic call frame must have one exact executable call: {executable_calls:#?}"
        );
    }

    #[test]
    fn structured_when_arm_expands_its_value_instead_of_its_pipeline_delimiter() {
        let (_, executable) =
            executable_program(include_str!("../../../testdata/typed_passkey_effects.bn"));
        let mut found_structured_registration = false;
        for expression in &executable.expressions {
            let ConcreteExpressionKind::When { arms, .. } = &expression.kind else {
                continue;
            };
            for arm in arms {
                assert!(
                    !matches!(
                        executable.expressions[arm.output.as_usize()].kind,
                        ConcreteExpressionKind::Delimiter
                    ),
                    "WHEN arm {:?} retained a pipeline delimiter as executable output",
                    arm.pattern
                );
                if matches!(
                    &arm.pattern,
                    boon_typecheck::CheckedMatchPattern::Tag { name }
                        if name == "RegistrationSucceeded"
                ) && matches!(
                    &executable.expressions[arm.output.as_usize()].kind,
                    ConcreteExpressionKind::Object(fields)
                        if fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>()
                            == ["credential_id", "label"]
                ) {
                    found_structured_registration = true;
                }
            }
        }
        assert!(
            found_structured_registration,
            "the structured registration arm must expand to its checked field values"
        );
    }

    #[test]
    fn transformed_wrappers_erase_to_the_direct_concrete_body() {
        let (direct, direct_expressions) = materialization(
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
        let (wrapped, wrapped_expressions) = materialization(
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
        let (multiply_wrapped, multiply_wrapped_expressions) = materialization(
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

        let direct_body = body_shape(&direct_expressions, direct.body);
        assert_eq!(body_shape(&wrapped_expressions, wrapped.body), direct_body);
        assert_eq!(
            body_shape(&multiply_wrapped_expressions, multiply_wrapped.body),
            direct_body
        );
        assert_eq!(direct.operation, ConcreteContextualOperation::Map);
        assert_eq!(wrapped.operation, ConcreteContextualOperation::Map);
        assert_eq!(multiply_wrapped.operation, ConcreteContextualOperation::Map);
        assert_eq!(direct.result_kind, MaterializationResultKind::RuntimeValue);
        assert_eq!(wrapped.result_kind, direct.result_kind);
        assert_eq!(multiply_wrapped.result_kind, direct.result_kind);
        assert_eq!(direct.owner, StaticOwnerId(0));
        assert_eq!(wrapped.owner, direct.owner);
        assert_eq!(multiply_wrapped.owner, direct.owner);
        assert_eq!(direct_expressions[direct.source.as_usize()].owner, None);
        assert_eq!(wrapped_expressions[wrapped.source.as_usize()].owner, None);
        assert_eq!(
            multiply_wrapped_expressions[multiply_wrapped.source.as_usize()].owner,
            None
        );
        assert_eq!(
            direct_expressions[direct.body.as_usize()].owner,
            Some(direct.owner)
        );
        assert_eq!(
            wrapped_expressions[wrapped.body.as_usize()].owner,
            Some(wrapped.owner),
            "one transparent wrapper must preserve the row evaluation owner"
        );
        assert_eq!(
            multiply_wrapped_expressions[multiply_wrapped.body.as_usize()].owner,
            Some(multiply_wrapped.owner),
            "nested transparent wrappers must preserve the row evaluation owner"
        );
    }

    #[test]
    fn named_order_chain_erases_inherited_keys_into_the_current_row_scope() {
        let parsed = boon_parser::parse_source(
            "named-order-chain-erasure.bn",
            r#"
rows: LIST {
    [name: TEXT { Alpha }, rank: 1]
    [name: TEXT { Alpha }, rank: 2]
}
primary: rows |> List/sort_by(item, key: item.name)
ordered:
    primary
    |> List/then_by(item, key: item.rank, direction: Descending)
"#,
        )
        .unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let (materializations, expressions) =
            derive_contextual_materializations(&checked, &out_net.graph).unwrap();
        let ordered = materializations
            .iter()
            .find(|materialization| {
                materialization.operation == ConcreteContextualOperation::ThenBy
            })
            .expect("then_by materialization");
        let [primary] = ordered.inherited_order.as_slice() else {
            panic!("then_by must retain one authoritative primary key: {ordered:#?}");
        };
        assert_eq!(primary.operation, ConcreteContextualOperation::SortBy);
        assert_eq!(
            body_shape(&expressions, primary.body),
            BodyShape::Local(MaterializationLocalId(0), vec!["name".to_owned()])
        );
        assert_eq!(
            body_shape(&expressions, ordered.body),
            BodyShape::Local(MaterializationLocalId(0), vec!["rank".to_owned()])
        );
        assert!(matches!(
            expressions[primary.body.as_usize()].kind,
            ConcreteExpressionKind::MaterializationLocal { owner, local, .. }
                if owner == ordered.owner && local == ordered.row_local
        ));
    }

    #[test]
    fn root_contextual_calls_reference_materializations_without_wrapper_calls() {
        let source = r#"
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
"#;
        let (materialization, executable) = erased_program(source);
        let result = executable
            .statements
            .iter()
            .find(|statement| {
                matches!(
                    &statement.kind,
                    ExecutableStatementKind::Field { path, .. } if path == "result"
                )
            })
            .expect("result statement is executable");
        let value = result.value.expect("result has a value");
        assert!(matches!(
            executable.expressions[value.as_usize()].kind,
            ConcreteExpressionKind::Materialize { materialization: id }
                if id == materialization.id
        ));
        assert!(executable.expressions.iter().all(|expression| {
            !matches!(
                &expression.kind,
                ConcreteExpressionKind::Call { name, .. } if name == "doubled" || name == "List/map"
            )
        }));
    }

    #[test]
    fn generic_user_calls_are_erased_with_their_exact_call_site_types() {
        let (_, executable) = executable_program(
            r#"
FUNCTION chunk_rows(list, size) {
    list |> List/chunk(size: size)
}

text_rows: LIST { [value: TEXT { ready }] }
number_rows: LIST { [value: 7] }
text_result: text_rows |> chunk_rows(size: 1)
number_result: number_rows |> chunk_rows(size: 1)
"#,
        );

        let result_value_type = |path: &str| {
            let statement = executable
                .statements
                .iter()
                .find(|statement| {
                    matches!(
                        &statement.kind,
                        ExecutableStatementKind::Field { path: candidate, .. }
                            if candidate == path
                    )
                })
                .unwrap_or_else(|| panic!("missing executable statement `{path}`"));
            let ty = executable.expressions
                [statement.value.expect("result has a value").as_usize()]
            .flow_type
            .ty
            .clone();
            let Type::List(chunks) = ty else {
                panic!("`{path}` did not erase to a chunk list: {ty:?}");
            };
            let Type::Object(chunk) = *chunks else {
                panic!("`{path}` chunk row is not an object");
            };
            let Some(Type::List(items)) = chunk.fields.get("items") else {
                panic!("`{path}` chunk row has no typed `items` list: {chunk:?}");
            };
            let Type::Object(item) = items.as_ref() else {
                panic!("`{path}` item is not a typed object: {items:?}");
            };
            item.fields
                .get("value")
                .cloned()
                .unwrap_or_else(|| panic!("`{path}` has no typed `value` field"))
        };

        assert_eq!(result_value_type("text_result"), Type::Text);
        assert_eq!(result_value_type("number_result"), Type::Number);
        assert!(executable.expressions.iter().all(|expression| {
            !matches!(
                &expression.kind,
                ConcreteExpressionKind::Project { fields, .. } if fields.is_empty()
            )
        }));
    }

    #[test]
    fn nested_generic_user_calls_compose_exact_call_site_types() {
        let (_, executable) = executable_program(
            r#"
FUNCTION chunk_rows(list, size) {
    list |> List/chunk(size: size)
}

FUNCTION nested_chunk_rows(rows, chunk_size) {
    rows |> chunk_rows(size: chunk_size)
}

text_rows: LIST { [value: TEXT { ready }] }
number_rows: LIST { [value: 7] }
text_result: text_rows |> nested_chunk_rows(chunk_size: 1)
number_result: number_rows |> nested_chunk_rows(chunk_size: 1)
"#,
        );

        let result_value_type = |path: &str| {
            let statement = executable
                .statements
                .iter()
                .find(|statement| {
                    matches!(
                        &statement.kind,
                        ExecutableStatementKind::Field { path: candidate, .. }
                            if candidate == path
                    )
                })
                .unwrap_or_else(|| panic!("missing executable statement `{path}`"));
            let ty = &executable.expressions
                [statement.value.expect("result has a value").as_usize()]
            .flow_type
            .ty;
            let Type::List(chunks) = ty else {
                panic!("`{path}` did not erase to a chunk list: {ty:?}");
            };
            let Type::Object(chunk) = chunks.as_ref() else {
                panic!("`{path}` chunk row is not an object");
            };
            let Some(Type::List(items)) = chunk.fields.get("items") else {
                panic!("`{path}` chunk row has no typed `items` list: {chunk:?}");
            };
            let Type::Object(item) = items.as_ref() else {
                panic!("`{path}` item is not a typed object: {items:?}");
            };
            item.fields
                .get("value")
                .cloned()
                .unwrap_or_else(|| panic!("`{path}` has no typed `value` field"))
        };

        assert_eq!(result_value_type("text_result"), Type::Text);
        assert_eq!(result_value_type("number_result"), Type::Number);
    }

    #[test]
    fn passed_list_materializations_use_the_exact_call_site_type() {
        let (_, executable) = executable_program(
            r#"
FUNCTION map_passed_rows() {
    PASSED.rows |> List/map(item, new: [value: item.value])
}

text_result:
    map_passed_rows(PASS: [rows: LIST { [value: TEXT { ready }] }])
number_result:
    map_passed_rows(PASS: [rows: LIST { [value: 7] }])
"#,
        );

        let result_value_type = |path: &str| {
            let statement = executable
                .statements
                .iter()
                .find(|statement| {
                    matches!(
                        &statement.kind,
                        ExecutableStatementKind::Field { path: candidate, .. }
                            if candidate == path
                    )
                })
                .unwrap_or_else(|| panic!("missing executable statement `{path}`"));
            let ty = &executable.expressions
                [statement.value.expect("result has a value").as_usize()]
            .flow_type
            .ty;
            let Type::List(items) = ty else {
                panic!("`{path}` did not erase to a list: {ty:?}");
            };
            let Type::Object(item) = items.as_ref() else {
                panic!("`{path}` row is not an object: {items:?}");
            };
            item.fields
                .get("value")
                .cloned()
                .unwrap_or_else(|| panic!("`{path}` has no typed `value` field"))
        };

        assert_eq!(result_value_type("text_result"), Type::Text);
        assert_eq!(result_value_type("number_result"), Type::Number);
    }

    #[test]
    fn stateful_wrapper_body_retains_the_materialization_owner() {
        let (materialization, expressions) = materialization(
            r#"
FUNCTION remember_each(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new:
            new
            |> HOLD remembered { LATEST {} }
    )
}

rows: LIST { [value: 1] }
result:
    rows
    |> remember_each(
        entry
        new: entry.value
    )
"#,
        );

        let body = &expressions[materialization.body.as_usize()];
        assert_eq!(body.owner, Some(materialization.owner));
        assert!(
            matches!(body.kind, ConcreteExpressionKind::Hold { .. }),
            "stateful wrapper body was not transparently expanded: {:?}",
            body.kind
        );
    }

    #[test]
    fn nested_materialization_source_uses_its_call_evaluation_owner() {
        let parsed = boon_parser::parse_source(
            "nested-materialization-owner.bn",
            r#"
FUNCTION expand_rows(rows, outer: OUT, new) {
    rows
    |> List/map(item: outer, new:
        List/range(from: 0, to: outer.value)
        |> List/map(item, new: item + new)
    )
}

rows: LIST { [value: 2] }
result:
    rows
    |> expand_rows(
        outer
        new: outer.value
    )
"#,
        )
        .unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let (materializations, expressions) =
            derive_contextual_materializations(&checked, &out_net.graph).unwrap();
        assert_eq!(materializations.len(), 2);
        let outer = &materializations[0];
        let inner = &materializations[1];
        let inner_producer = checked
            .calls
            .iter()
            .flat_map(|call| out_net.graph.concrete_producers_for_checked_call(call.id))
            .find(|producer| producer.owner == inner.owner)
            .expect("inner materialization has a concrete OUT producer");
        let evaluation_owner = out_net.graph.owner_for_call_evaluation(inner_producer.call);
        assert_eq!(
            out_net.graph.static_owners[inner.owner.as_usize()].parent,
            Some(outer.owner)
        );
        assert_eq!(
            expressions[inner.source.as_usize()].owner,
            evaluation_owner,
            "a nested collection source evaluates in the concrete call's origin"
        );
        assert_eq!(
            expressions[inner.body.as_usize()].owner,
            Some(inner.owner),
            "a nested collection body evaluates once per nested row"
        );
    }

    #[test]
    fn canonical_pipeline_continuations_do_not_reenter_their_parent_expression() {
        let source = r#"
store: [
    increment: SOURCE
    reset: SOURCE
    count:
        LATEST {
            0
            increment |> THEN { count + 1 }
            reset |> THEN { 0 }
        }
]
"#;
        let parsed = boon_parser::parse_source("latest-continuation.bn", source).unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("valid source has a checked program");
        let out_net = OutNet::build(&checked);
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let (materializations, expressions) =
            derive_contextual_materializations(&checked, &out_net.graph).unwrap();
        assert!(materializations.is_empty());
        let executable =
            derive_executable_program(&checked, &out_net.graph, &materializations, expressions)
                .expect("LATEST branches must form an acyclic executable graph");
        let count = executable
            .statements
            .iter()
            .find(|statement| {
                matches!(
                    &statement.kind,
                    ExecutableStatementKind::Field { path, .. } if path == "store.count"
                )
            })
            .expect("count statement is executable");
        let value = count.value.expect("count has a value");
        assert!(matches!(
            &executable.expressions[value.as_usize()].kind,
            ConcreteExpressionKind::Latest { branches } if branches.len() == 3
        ));
    }

    #[test]
    fn stateful_contextual_function_self_read_keeps_the_state_declaration() {
        let (_, executable) = erased_program(
            r#"
FUNCTION stateful_row(row) {
    [
        source: SOURCE
        completed:
            row.completed |> HOLD completed {
                source |> THEN { completed |> Bool/not() }
            }
    ]
}

rows: LIST { [completed: False] }
result: rows |> List/map(item, new: stateful_row(row: item))
"#,
        );
        let state = executable
            .states
            .iter()
            .find(|state| state.binding_path.ends_with("completed"))
            .unwrap_or_else(|| panic!("missing completed state: {executable:#?}"));
        assert!(
            executable.expressions.iter().any(|expression| {
                matches!(
                    expression.kind,
                    ConcreteExpressionKind::CanonicalRead { target, .. }
                        if target == state.declaration
                )
            }),
            "self read and state declaration diverged: {executable:#?}"
        );
    }
}
