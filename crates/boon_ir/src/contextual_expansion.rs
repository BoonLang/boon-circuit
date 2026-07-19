use super::out_net::{OutCallInstanceId, OutNet, OutNetId, ScopedCheckedExpr};
use super::{
    ContextualMaterialization as ConcreteMaterialization,
    ContextualOperationKind as ConcreteContextualOperation,
    ExecutableCallArgument as ConcreteCallArgument, ExecutableCallableKind,
    ExecutableExprId as ConcreteExprId, ExecutableExpression as ConcreteExpression,
    ExecutableExpressionKind as ConcreteExpressionKind, ExecutableFunction,
    ExecutableFunctionParameter, ExecutableParameterId, ExecutableProgram,
    ExecutableRecordField as ConcreteRecordField, ExecutableRoot, ExecutableSelectArm,
    ExecutableSourceDef, ExecutableSourceId, ExecutableStateDef, ExecutableStateId,
    ExecutableStatement, ExecutableStatementId, ExecutableStatementKind, FunctionId,
    MaterializationLocalId, MaterializationResultKind, StaticOwnerId,
};
use boon_typecheck::{
    CheckedCallEntry, CheckedCallId, CheckedCallableKind, CheckedContextualOperation,
    CheckedExprId, CheckedExpression, CheckedExpressionKind, CheckedParameterKind, CheckedProgram,
    CheckedValueUse, DeclId, FlowMode, Type,
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
    MissingFormal {
        callable: DeclId,
        formal: DeclId,
    },
    MissingFunctionResult(DeclId),
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
    MissingOwnerScope(OutNetId),
    MissingMaterializationOwner(StaticOwnerId),
    MissingMaterialization(CheckedCallId),
    AmbiguousMaterialization(CheckedCallId),
    StandaloneFunctionHasOut(DeclId),
    StandaloneFunctionHasPass(CheckedCallId),
    RecursiveStandaloneFunction(DeclId),
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
            Self::MissingOwnerScope(net) => {
                write!(formatter, "OUT net {} has no fresh owner scope", net)
            }
            Self::MissingMaterializationOwner(owner) => write!(
                formatter,
                "static owner {} has no contextual materialization net",
                owner.0
            ),
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
            Self::StandaloneFunctionHasOut(callable) => write!(
                formatter,
                "standalone pure function {} contains an OUT call",
                callable.0
            ),
            Self::StandaloneFunctionHasPass(call) => {
                write!(formatter, "standalone pure call {} contains PASS", call.0)
            }
            Self::RecursiveStandaloneFunction(callable) => write!(
                formatter,
                "standalone pure function {} is recursive",
                callable.0
            ),
        }
    }
}

pub(crate) fn derive_contextual_materializations(
    program: &CheckedProgram,
    out_net: &OutNet,
) -> Result<(Vec<ConcreteMaterialization>, Vec<ConcreteExpression>), ExpansionError> {
    struct Candidate {
        owner: StaticOwnerId,
        net: OutNetId,
        producer: OutCallInstanceId,
        operation: ConcreteContextualOperation,
        source: ScopedCheckedExpr,
        body: ScopedCheckedExpr,
        item_type: Type,
        result_type: Type,
    }

    let mut candidates = Vec::new();
    for checked_call in &program.calls {
        let callable = program
            .callables
            .iter()
            .find(|callable| callable.decl_id == checked_call.callable)
            .ok_or(ExpansionError::MissingCallable(checked_call.callable))?;
        let Some(operation) = callable.contextual_operation else {
            continue;
        };
        let (operation_kind, list_formal, row_formal, body_formal) =
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
                    .map(|binding| binding.value)
                    .ok_or(ExpansionError::MissingOperationInput {
                        call: checked_call.id,
                        formal,
                    })
            };
            let list_expression = concrete_input(list_formal)?;
            let body_expression = concrete_input(body_formal)?;
            let item_type = checked_expression(program, list_expression.expression)
                .and_then(|expression| match &expression.flow_type.ty {
                    Type::List(item) => Some((**item).clone()),
                    _ => None,
                })
                .unwrap_or(Type::Unknown);
            out_net
                .owner_scope_for_net(producer.net)
                .ok_or(ExpansionError::MissingOwnerScope(producer.net))?;
            candidates.push(Candidate {
                owner: producer.owner,
                net: producer.net,
                producer: producer.call,
                operation: operation_kind,
                source: list_expression,
                body: body_expression,
                item_type,
                result_type: checked_call.result.ty.clone(),
            });
        }
    }
    candidates.sort_by_key(|candidate| candidate.owner);
    let materializations_by_owner = candidates
        .iter()
        .enumerate()
        .map(|(id, candidate)| (candidate.owner, id))
        .collect::<BTreeMap<_, _>>();
    let net_by_owner = candidates
        .iter()
        .map(|candidate| (candidate.owner, candidate.net))
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::with_capacity(candidates.len());
    let mut expressions = Vec::new();
    for (id, candidate) in candidates.into_iter().enumerate() {
        let mut locals = BTreeMap::new();
        let mut owner = Some(candidate.owner);
        while let Some(current) = owner {
            let net = net_by_owner
                .get(&current)
                .copied()
                .ok_or(ExpansionError::MissingMaterializationOwner(current))?;
            locals.insert(net, (current, MaterializationLocalId(0)));
            owner = out_net
                .static_owners
                .get(current.as_usize())
                .and_then(|owner| owner.parent);
        }
        let mut builder =
            ConcreteExpressionBuilder::new(program, out_net, locals, &materializations_by_owner);
        let parent_owner = out_net
            .static_owners
            .get(candidate.owner.as_usize())
            .ok_or(ExpansionError::MissingMaterializationOwner(candidate.owner))?
            .parent;
        let local_source = builder.expand_with_inherited_owner(candidate.source, parent_owner)?;
        let local_body =
            builder.expand_with_inherited_owner(candidate.body, Some(candidate.owner))?;
        let [source, body] = append_expression_arena(
            &mut expressions,
            builder.finish(),
            [local_source, local_body],
        );
        result.push(ConcreteMaterialization {
            id,
            owner: candidate.owner,
            operation: candidate.operation,
            result_kind: exposed_value_use(program, out_net, candidate.producer),
            source,
            body,
            row_local: MaterializationLocalId(0),
            source_list_id: None,
            source_scope_id: None,
            item_type: candidate.item_type,
            result_type: candidate.result_type,
        });
    }
    Ok((result, expressions))
}

pub(crate) fn derive_executable_program(
    program: &CheckedProgram,
    out_net: &OutNet,
    materializations: &[ConcreteMaterialization],
    distributed_references: &super::DistributedReferences,
    mut expressions: Vec<ConcreteExpression>,
) -> Result<ExecutableProgram, ExpansionError> {
    let materializations_by_owner = materializations
        .iter()
        .map(|materialization| (materialization.owner, materialization.id))
        .collect::<BTreeMap<_, _>>();
    let mut builder = ConcreteExpressionBuilder::new(
        program,
        out_net,
        BTreeMap::new(),
        &materializations_by_owner,
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
                builder.expand(ScopedCheckedExpr {
                    expression,
                    frame: None,
                    evaluation_port: None,
                    value_frame: None,
                })
            })
            .transpose()?;
        let declaration_parts = |declaration: Option<DeclId>| {
            declaration
                .and_then(|declaration| {
                    let checked = program
                        .declarations
                        .iter()
                        .find(|candidate| candidate.id == declaration)?;
                    Some((
                        checked.name.clone(),
                        canonical_declaration_path(program, declaration)?,
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
                .and_then(|declaration| {
                    program
                        .declarations
                        .iter()
                        .find(|candidate| candidate.id == declaration)
                })
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
    statements.sort_by_key(|statement| statement.id);
    let root_checked_expressions = distributed_references
        .pure_calls
        .iter()
        .flat_map(|call| &call.arguments)
        .map(|argument| CheckedExprId(argument.expr_id.as_usize() as u32))
        .collect::<BTreeSet<_>>()
        .into_iter();
    let mut roots = Vec::new();
    for checked_expr_id in root_checked_expressions {
        let expression = builder.expand(ScopedCheckedExpr {
            expression: checked_expr_id,
            frame: None,
            evaluation_port: None,
            value_frame: None,
        })?;
        roots.push(ExecutableRoot {
            checked_expr_id,
            expression,
        });
    }
    let offset = expressions.len();
    let local_expressions = builder.finish();
    for statement in &mut statements {
        if let Some(value) = &mut statement.value {
            *value = rebase_expr_id(*value, offset);
        }
    }
    for root in &mut roots {
        root.expression = rebase_expr_id(root.expression, offset);
    }
    append_expression_arena_without_roots(&mut expressions, local_expressions);

    let mut functions = Vec::new();
    for callable in program.callables.iter().filter(|callable| {
        callable.kind == CheckedCallableKind::User
            && callable.result.mode == FlowMode::Continuous
            && callable.effect == boon_typecheck::CheckedEffectSummary::default()
            && callable
                .parameters
                .iter()
                .all(|parameter| parameter.kind == CheckedParameterKind::Value)
    }) {
        let function = FunctionId(callable.decl_id.0 as usize);
        let mut function_builder = ConcreteExpressionBuilder::new(
            program,
            out_net,
            BTreeMap::new(),
            &materializations_by_owner,
        );
        let (parameters, root) =
            match function_builder.expand_standalone_function(callable, function) {
                Ok(result) => result,
                Err(
                    ExpansionError::StandaloneFunctionHasOut(_)
                    | ExpansionError::StandaloneFunctionHasPass(_)
                    | ExpansionError::RecursiveStandaloneFunction(_),
                ) => continue,
                Err(error) => return Err(error),
            };
        let function_offset = expressions.len();
        let function_expressions = function_builder.finish();
        append_expression_arena_without_roots(&mut expressions, function_expressions);
        functions.push(ExecutableFunction {
            id: function,
            name: callable.name.clone(),
            parameters,
            result_type: callable.result.clone(),
            root: rebase_expr_id(root, function_offset),
        });
    }
    synthesize_statement_owned_states(program, &mut expressions, &mut statements)?;
    let sources = expressions
        .iter()
        .filter_map(|expression| {
            let binding_path = match &expression.kind {
                ConcreteExpressionKind::Source { binding_path } => binding_path.clone(),
                ConcreteExpressionKind::Call { .. } if expression.effect.emits_source => {
                    resource_binding_path(program, expression.checked_expr_id)?
                }
                _ => return None,
            };
            Some((expression, binding_path))
        })
        .map(|(expression, binding_path)| {
            Ok(ExecutableSourceDef {
                id: ExecutableSourceId(0),
                declaration: resource_declaration(program, expression.checked_expr_id).ok_or(
                    ExpansionError::MissingSourceDeclaration(expression.checked_expr_id),
                )?,
                expression: expression.id,
                binding_path,
                owner: expression.owner,
            })
        })
        .collect::<Result<Vec<_>, ExpansionError>>()?
        .into_iter()
        .enumerate()
        .map(|(id, mut source)| {
            source.id = ExecutableSourceId(id);
            source
        })
        .collect();
    let states = expressions
        .iter()
        .filter_map(|expression| {
            let binding_path = match &expression.kind {
                ConcreteExpressionKind::Hold { binding_path, .. } => binding_path.clone(),
                ConcreteExpressionKind::Latest { branches }
                    if executable_latest_has_initial(&expressions, branches) =>
                {
                    resource_binding_path(program, expression.checked_expr_id)?
                }
                ConcreteExpressionKind::Call {
                    callable_kind: ExecutableCallableKind::Builtin,
                    ..
                } if expression.effect.writes_state => {
                    resource_binding_path(program, expression.checked_expr_id)?
                }
                _ => return None,
            };
            Some((expression, binding_path))
        })
        .map(|(expression, binding_path)| {
            Ok(ExecutableStateDef {
                id: ExecutableStateId(0),
                declaration: resource_declaration(program, expression.checked_expr_id).ok_or(
                    ExpansionError::MissingStateDeclaration(expression.checked_expr_id),
                )?,
                expression: expression.id,
                binding_path,
                owner: expression.owner,
            })
        })
        .collect::<Result<Vec<_>, ExpansionError>>()?
        .into_iter()
        .enumerate()
        .map(|(id, mut state)| {
            state.id = ExecutableStateId(id);
            state
        })
        .collect();
    Ok(ExecutableProgram {
        expressions,
        statements,
        sources,
        states,
        roots,
        functions,
    })
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
        if let Some(existing) = expressions.iter().find(|expression| {
            expression.owner == owner
                && resource_declaration(program, expression.checked_expr_id) == Some(declaration)
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

fn append_expression_arena<const N: usize>(
    target: &mut Vec<ConcreteExpression>,
    source: Vec<ConcreteExpression>,
    roots: [ConcreteExprId; N],
) -> [ConcreteExprId; N] {
    let offset = target.len();
    append_expression_arena_without_roots(target, source);
    roots.map(|root| rebase_expr_id(root, offset))
}

fn append_expression_arena_without_roots(
    target: &mut Vec<ConcreteExpression>,
    mut source: Vec<ConcreteExpression>,
) {
    let offset = target.len();
    for expression in &mut source {
        expression.id = rebase_expr_id(expression.id, offset);
        rebase_expression_kind(&mut expression.kind, offset);
    }
    target.extend(source);
}

fn rebase_expr_id(expression: ConcreteExprId, offset: usize) -> ConcreteExprId {
    ConcreteExprId(expression.as_usize() + offset)
}

fn rebase_expression_kind(kind: &mut ConcreteExpressionKind, offset: usize) {
    let rebase = |expression: &mut ConcreteExprId| {
        *expression = rebase_expr_id(*expression, offset);
    };
    match kind {
        ConcreteExpressionKind::CanonicalRead { .. }
        | ConcreteExpressionKind::ExternalRead { .. }
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
        ConcreteExpressionKind::TaggedObject { fields, .. }
        | ConcreteExpressionKind::Object(fields)
        | ConcreteExpressionKind::Record(fields) => {
            for field in fields {
                rebase(&mut field.value);
            }
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
) -> (ConcreteContextualOperation, DeclId, DeclId, DeclId) {
    match operation {
        CheckedContextualOperation::Map { list, row, body } => {
            (ConcreteContextualOperation::Map, list, row, body)
        }
        CheckedContextualOperation::Filter {
            list,
            row,
            predicate,
        } => (ConcreteContextualOperation::Filter, list, row, predicate),
        CheckedContextualOperation::Retain {
            list,
            row,
            predicate,
        } => (ConcreteContextualOperation::Retain, list, row, predicate),
        CheckedContextualOperation::Every {
            list,
            row,
            predicate,
        } => (ConcreteContextualOperation::Every, list, row, predicate),
        CheckedContextualOperation::Any {
            list,
            row,
            predicate,
        } => (ConcreteContextualOperation::Any, list, row, predicate),
        CheckedContextualOperation::Find {
            list,
            row,
            predicate,
        } => (ConcreteContextualOperation::Find, list, row, predicate),
    }
}

fn exposed_value_use(
    program: &CheckedProgram,
    out_net: &OutNet,
    producer: OutCallInstanceId,
) -> MaterializationResultKind {
    let mut exposed = producer;
    while let Some(parent) = out_net.call_instances[exposed.as_usize()].parent {
        let current_expression = out_net.call_instances[exposed.as_usize()]
            .provenance
            .expression;
        let parent_callable = out_net.call_instances[parent.as_usize()]
            .provenance
            .callable;
        let is_canonical_result = program
            .callables
            .iter()
            .find(|callable| callable.decl_id == parent_callable)
            .is_some_and(|callable| callable.result_expression == Some(current_expression));
        if !is_canonical_result {
            break;
        }
        exposed = parent;
    }
    let expression = out_net.call_instances[exposed.as_usize()]
        .provenance
        .expression;
    if program.statements.iter().any(|statement| {
        statement.value == Some(expression) && statement.value_use == CheckedValueUse::RenderSlot
    }) {
        MaterializationResultKind::RenderSlot
    } else {
        MaterializationResultKind::RuntimeValue
    }
}

fn checked_expression(
    program: &CheckedProgram,
    expression: CheckedExprId,
) -> Option<&CheckedExpression> {
    program
        .expressions
        .iter()
        .find(|candidate| candidate.id == expression)
}

fn declaration_is_function_local(
    program: &CheckedProgram,
    mut scope: boon_typecheck::LexicalScopeId,
) -> bool {
    let mut visited = BTreeSet::new();
    while visited.insert(scope) {
        let Some(current) = program
            .scopes
            .iter()
            .find(|candidate| candidate.id == scope)
        else {
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

fn canonical_declaration_path(program: &CheckedProgram, target: DeclId) -> Option<String> {
    let declaration = program
        .declarations
        .iter()
        .find(|declaration| declaration.id == target)?;
    let mut segments = vec![declaration.name.clone()];
    let mut scope = declaration.scope_id;
    let mut visited = BTreeSet::new();
    while scope != program.root_scope && visited.insert(scope) {
        let current = program
            .scopes
            .iter()
            .find(|candidate| candidate.id == scope)?;
        if current.kind == boon_typecheck::CheckedScopeKind::Function {
            break;
        }
        if let Some(owner) = current.owner
            && let Some(owner) = program
                .declarations
                .iter()
                .find(|declaration| declaration.id == owner)
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

fn resource_binding_path(program: &CheckedProgram, expression: CheckedExprId) -> Option<String> {
    let mut candidates = BTreeSet::new();
    for declaration in &program.declarations {
        let Some(root) = declaration.value else {
            continue;
        };
        let Some(projection) = checked_projection_to_expression(program, root, expression) else {
            continue;
        };
        let mut path = canonical_declaration_path(program, declaration.id)?;
        if !projection.is_empty() {
            path.push('.');
            path.push_str(&projection.join("."));
        }
        candidates.insert(path);
    }
    for callable in &program.callables {
        let Some(root) = callable.result_expression else {
            continue;
        };
        let Some(projection) = checked_projection_to_expression(program, root, expression) else {
            continue;
        };
        if !projection.is_empty() {
            candidates.insert(projection.join("."));
        }
    }
    for path in program.statements.iter().filter_map(|statement| {
        if statement.value != Some(expression) {
            return None;
        }
        let boon_typecheck::CheckedStatementKind::Source {
            declaration: Some(declaration),
            ..
        } = &statement.kind
        else {
            return None;
        };
        canonical_declaration_path(program, *declaration)
    }) {
        candidates.insert(path);
    }
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    let [path] = candidates.as_slice() else {
        return None;
    };
    Some(path.clone())
}

fn resource_declaration(program: &CheckedProgram, expression: CheckedExprId) -> Option<DeclId> {
    checked_expression(program, expression)?.declaration
}

fn checked_projection_to_expression(
    program: &CheckedProgram,
    root: CheckedExprId,
    target: CheckedExprId,
) -> Option<Vec<String>> {
    fn visit(
        program: &CheckedProgram,
        current: CheckedExprId,
        target: CheckedExprId,
        visiting: &mut BTreeSet<CheckedExprId>,
    ) -> Option<Vec<String>> {
        if current == target {
            return Some(Vec::new());
        }
        if !visiting.insert(current) {
            return None;
        }
        let expression = checked_expression(program, current)?;
        let direct = |child, visiting: &mut BTreeSet<_>| visit(program, child, target, visiting);
        let result = match &expression.kind {
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields }
            | CheckedExpressionKind::Record { fields } => fields.iter().find_map(|field| {
                let mut projection = direct(field.value, visiting)?;
                projection.insert(0, field.name.clone());
                Some(projection)
            }),
            CheckedExpressionKind::Call { call } => program
                .calls
                .iter()
                .find(|candidate| candidate.id == *call)
                .into_iter()
                .flat_map(|call| &call.entries)
                .find_map(|entry| match entry {
                    CheckedCallEntry::Input { value, .. } => direct(*value, visiting),
                    CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => None,
                }),
            CheckedExpressionKind::Draining { input }
            | CheckedExpressionKind::Hold { initial: input, .. } => direct(*input, visiting),
            CheckedExpressionKind::When { input, arms }
            | CheckedExpressionKind::While { input, arms } => direct(*input, visiting)
                .or_else(|| arms.iter().find_map(|arm| direct(*arm, visiting))),
            CheckedExpressionKind::Then { input, output } => direct(*input, visiting)
                .or_else(|| output.and_then(|output| direct(output, visiting))),
            CheckedExpressionKind::Infix { left, right, .. } => {
                direct(*left, visiting).or_else(|| direct(*right, visiting))
            }
            CheckedExpressionKind::MatchArm { output, .. } => {
                output.and_then(|output| direct(output, visiting))
            }
            CheckedExpressionKind::Block { bindings, result } => bindings
                .iter()
                .find_map(|binding| direct(binding.value, visiting))
                .or_else(|| result.and_then(|result| direct(result, visiting))),
            CheckedExpressionKind::List { items, .. }
            | CheckedExpressionKind::Bytes { items, .. } => {
                items.iter().find_map(|item| direct(*item, visiting))
            }
            CheckedExpressionKind::Read { .. }
            | CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Bool { .. }
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Latest
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => None,
        };
        visiting.remove(&current);
        result
    }

    visit(program, root, target, &mut BTreeSet::new())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ExpansionKey {
    expression: CheckedExprId,
    frame: Option<OutCallInstanceId>,
    value_frame: Option<usize>,
    evaluation_owner: Option<StaticOwnerId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StandaloneValueBinding {
    Parameter(ExecutableParameterId),
    Expression(ConcreteExprId),
    Deferred(ScopedCheckedExpr),
}

pub(crate) struct ConcreteExpressionBuilder<'a> {
    program: &'a CheckedProgram,
    out_net: &'a OutNet,
    locals: BTreeMap<OutNetId, (StaticOwnerId, MaterializationLocalId)>,
    materializations_by_owner: &'a BTreeMap<StaticOwnerId, usize>,
    expressions: Vec<ConcreteExpression>,
    memo: BTreeMap<ExpansionKey, ConcreteExprId>,
    visiting: BTreeSet<ExpansionKey>,
    visiting_stack: Vec<ExpansionKey>,
    owner_stack: Vec<Option<StaticOwnerId>>,
    value_frames: Vec<BTreeMap<DeclId, StandaloneValueBinding>>,
    standalone_call_stack: Vec<DeclId>,
}

impl<'a> ConcreteExpressionBuilder<'a> {
    pub(crate) fn new(
        program: &'a CheckedProgram,
        out_net: &'a OutNet,
        locals: BTreeMap<OutNetId, (StaticOwnerId, MaterializationLocalId)>,
        materializations_by_owner: &'a BTreeMap<StaticOwnerId, usize>,
    ) -> Self {
        Self {
            program,
            out_net,
            locals,
            materializations_by_owner,
            expressions: Vec::new(),
            memo: BTreeMap::new(),
            visiting: BTreeSet::new(),
            visiting_stack: Vec::new(),
            owner_stack: Vec::new(),
            value_frames: Vec::new(),
            standalone_call_stack: Vec::new(),
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
                    self.program
                        .expressions
                        .iter()
                        .find(|expression| expression.id == key.expression)
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
        let result = self.expand_uncached(expression, evaluation_owner);
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

    fn expand_standalone_function(
        &mut self,
        callable: &boon_typecheck::CheckedCallableSignature,
        function: FunctionId,
    ) -> Result<(Vec<ExecutableFunctionParameter>, ConcreteExprId), ExpansionError> {
        if callable
            .parameters
            .iter()
            .any(|parameter| parameter.kind != CheckedParameterKind::Value)
        {
            return Err(ExpansionError::StandaloneFunctionHasOut(callable.decl_id));
        }
        let mut parameters = callable
            .parameters
            .iter()
            .map(|parameter| ExecutableFunctionParameter {
                id: ExecutableParameterId {
                    function,
                    ordinal: parameter.ordinal,
                },
                name: parameter.name.clone(),
                flow_type: parameter.flow_type.clone(),
            })
            .collect::<Vec<_>>();
        parameters.sort_by_key(|parameter| parameter.id.ordinal);
        let bindings = callable
            .parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.decl_id,
                    StandaloneValueBinding::Parameter(ExecutableParameterId {
                        function,
                        ordinal: parameter.ordinal,
                    }),
                )
            })
            .collect();
        let frame = self.value_frames.len();
        self.value_frames.push(bindings);
        let result = callable
            .result_expression
            .ok_or(ExpansionError::MissingFunctionResult(callable.decl_id))?;
        self.standalone_call_stack.push(callable.decl_id);
        let root = self.expand(ScopedCheckedExpr {
            expression: result,
            frame: None,
            evaluation_port: None,
            value_frame: Some(frame),
        });
        self.standalone_call_stack.pop();
        Ok((parameters, root?))
    }

    fn expand_uncached(
        &mut self,
        scoped: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
    ) -> Result<ConcreteExprId, ExpansionError> {
        let expression = self
            .program
            .expressions
            .iter()
            .find(|expression| expression.id == scoped.expression)
            .cloned()
            .ok_or(ExpansionError::MissingExpression(scoped.expression))?;
        let kind = match expression.kind.clone() {
            CheckedExpressionKind::Read { target, projection } => {
                if let Some(binding) = scoped
                    .value_frame
                    .and_then(|frame| self.value_frames.get(frame))
                    .and_then(|bindings| bindings.get(&target))
                    .copied()
                {
                    return match binding {
                        StandaloneValueBinding::Parameter(parameter) => Ok(self.push(
                            &expression,
                            owner,
                            ConcreteExpressionKind::FunctionParameter {
                                parameter,
                                projection,
                            },
                        )),
                        StandaloneValueBinding::Expression(input) => {
                            self.project(&expression, owner, input, projection)
                        }
                        StandaloneValueBinding::Deferred(input) => {
                            let expanded = self.expand(input)?;
                            self.project(&expression, owner, expanded, projection)
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
                    return Ok(self.push(
                        &expression,
                        owner,
                        ConcreteExpressionKind::MaterializationLocal {
                            owner: local_owner,
                            local,
                            projection,
                        },
                    ));
                }
                if let Some(actual) = scoped.frame.and_then(|frame| {
                    self.out_net.call_instances[frame.as_usize()]
                        .inputs
                        .iter()
                        .find(|binding| binding.formal == target)
                        .map(|binding| binding.value)
                }) {
                    let expanded = self.expand(actual)?;
                    return self.project(&expression, owner, expanded, projection);
                }
                let declaration = self
                    .program
                    .declarations
                    .iter()
                    .find(|declaration| declaration.id == target)
                    .ok_or(ExpansionError::MissingDeclaration(target))?;
                if declaration.kind == boon_typecheck::CheckedDeclarationKind::PatternBinding {
                    let binding = self
                        .program
                        .pattern_bindings
                        .iter()
                        .find(|binding| binding.declaration == target)
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
                        checked_expression(self.program, value).is_some_and(|value| {
                            !value.effect.writes_state
                                && !value.effect.emits_source
                                && !value.effect.invokes_host
                                && !matches!(
                                    value.kind,
                                    CheckedExpressionKind::Hold { .. }
                                        | CheckedExpressionKind::Latest
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
                    storage_binding: None,
                    path: canonical_declaration_path(self.program, target)
                        .ok_or(ExpansionError::MissingDeclaration(target))?,
                    projection,
                }
            }
            CheckedExpressionKind::Passed { projection } => {
                let passed = scoped
                    .frame
                    .and_then(|frame| self.out_net.call_instances[frame.as_usize()].passed)
                    .ok_or(ExpansionError::MissingPassedContext(scoped.expression))?;
                let expanded = self.expand(passed)?;
                return self.project(&expression, owner, expanded, projection);
            }
            CheckedExpressionKind::ExternalRead { canonical_path } => {
                ConcreteExpressionKind::ExternalRead { canonical_path }
            }
            CheckedExpressionKind::Drain { target, projection } => ConcreteExpressionKind::Drain {
                target,
                storage_binding: None,
                path: canonical_declaration_path(self.program, target)
                    .ok_or(ExpansionError::MissingDeclaration(target))?,
                projection,
            },
            CheckedExpressionKind::Text { value } => ConcreteExpressionKind::Text(value),
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
                binding_path: resource_binding_path(self.program, scoped.expression)
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
                binding_path: resource_binding_path(self.program, scoped.expression)
                    .unwrap_or_else(|| name.clone()),
                name,
                updates: self.expand_statement_child_values(scoped)?,
            },
            CheckedExpressionKind::Latest => ConcreteExpressionKind::Latest {
                branches: self.expand_statement_child_values(scoped)?,
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
                for binding in bindings {
                    values.insert(
                        binding.declaration,
                        StandaloneValueBinding::Deferred(ScopedCheckedExpr {
                            expression: binding.value,
                            frame: scoped.frame,
                            evaluation_port: None,
                            value_frame: Some(frame),
                        }),
                    );
                }
                self.value_frames.push(values);
                let result = result.ok_or(ExpansionError::MissingExpression(scoped.expression))?;
                return self.expand(ScopedCheckedExpr {
                    expression: result,
                    frame: scoped.frame,
                    evaluation_port: None,
                    value_frame: Some(frame),
                });
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
            .program
            .calls
            .iter()
            .find(|call| call.id == call_id)
            .cloned()
            .ok_or(ExpansionError::MissingCall(call_id))?;
        let callable = self
            .program
            .callables
            .iter()
            .find(|callable| callable.decl_id == checked_call.callable)
            .cloned()
            .ok_or(ExpansionError::MissingCallable(checked_call.callable))?;
        if !self.standalone_call_stack.is_empty() {
            return self.expand_standalone_call(
                expression,
                scoped,
                owner,
                &checked_call,
                &callable,
            );
        }
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
            return Ok(self.push(
                expression,
                owner,
                ConcreteExpressionKind::Materialize { materialization },
            ));
        }
        if callable.kind == CheckedCallableKind::User {
            let result = callable
                .result_expression
                .ok_or(ExpansionError::MissingFunctionResult(callable.decl_id))?;
            return self.expand(ScopedCheckedExpr {
                expression: result,
                frame: Some(instance),
                evaluation_port: None,
                value_frame: None,
            });
        }
        if checked_call.pass.is_some() {
            return Err(ExpansionError::PassOnNonexpandedCall(call_id));
        }
        let inputs = self.out_net.call_instances[instance.as_usize()]
            .inputs
            .clone();
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
                value: self.expand(input.value)?,
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
        Ok(self.push(
            expression,
            owner,
            ConcreteExpressionKind::Call {
                callable_kind: kind,
                name: callable.name.clone(),
                arguments,
            },
        ))
    }

    fn expand_standalone_call(
        &mut self,
        expression: &CheckedExpression,
        scoped: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
        checked_call: &boon_typecheck::CheckedCall,
        callable: &boon_typecheck::CheckedCallableSignature,
    ) -> Result<ConcreteExprId, ExpansionError> {
        if checked_call
            .entries
            .iter()
            .any(|entry| !matches!(entry, CheckedCallEntry::Input { .. }))
        {
            return Err(ExpansionError::StandaloneFunctionHasOut(callable.decl_id));
        }
        if checked_call.pass.is_some() {
            return Err(ExpansionError::StandaloneFunctionHasPass(checked_call.id));
        }

        if callable.kind == CheckedCallableKind::User {
            if self.standalone_call_stack.contains(&callable.decl_id) {
                return Err(ExpansionError::RecursiveStandaloneFunction(
                    callable.decl_id,
                ));
            }
            let mut bindings = BTreeMap::new();
            for entry in &checked_call.entries {
                let CheckedCallEntry::Input { formal, value, .. } = entry else {
                    unreachable!("OUT entries were rejected above");
                };
                bindings.insert(
                    *formal,
                    StandaloneValueBinding::Expression(self.expand_in_frame(
                        *value,
                        scoped.frame,
                        scoped.value_frame,
                    )?),
                );
            }
            let frame = self.value_frames.len();
            self.value_frames.push(bindings);
            let result = callable
                .result_expression
                .ok_or(ExpansionError::MissingFunctionResult(callable.decl_id))?;
            self.standalone_call_stack.push(callable.decl_id);
            let expanded = self.expand(ScopedCheckedExpr {
                expression: result,
                frame: None,
                evaluation_port: None,
                value_frame: Some(frame),
            });
            self.standalone_call_stack.pop();
            return expanded;
        }

        let mut arguments = Vec::with_capacity(checked_call.entries.len());
        for entry in &checked_call.entries {
            let CheckedCallEntry::Input {
                formal,
                value,
                from_pipe,
                ..
            } = entry
            else {
                unreachable!("OUT entries were rejected above");
            };
            let parameter = callable
                .parameters
                .iter()
                .find(|parameter| parameter.decl_id == *formal)
                .ok_or(ExpansionError::MissingFormal {
                    callable: callable.decl_id,
                    formal: *formal,
                })?;
            arguments.push(ConcreteCallArgument {
                ordinal: parameter.ordinal,
                name: parameter.name.clone(),
                value: self.expand_in_frame(*value, scoped.frame, scoped.value_frame)?,
                from_pipe: *from_pipe,
            });
        }
        let callable_kind = match callable.kind {
            CheckedCallableKind::Builtin => ExecutableCallableKind::Builtin,
            CheckedCallableKind::External => ExecutableCallableKind::External,
            CheckedCallableKind::User => unreachable!("user calls were expanded above"),
        };
        Ok(self.push(
            expression,
            owner,
            ConcreteExpressionKind::Call {
                callable_kind,
                name: callable.name.clone(),
                arguments,
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
            let Some(expression) = self
                .program
                .expressions
                .iter()
                .find(|expression| expression.id == *child)
                .cloned()
            else {
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
            let frame = self.value_frames.len();
            let mut values = scoped
                .value_frame
                .and_then(|frame| self.value_frames.get(frame))
                .cloned()
                .unwrap_or_default();
            for binding in bindings {
                let projection = self
                    .program
                    .pattern_bindings
                    .iter()
                    .find(|candidate| candidate.declaration == *binding)
                    .ok_or(ExpansionError::MissingDeclaration(*binding))?
                    .projection
                    .clone();
                let value = self.project(&expression, owner, input, projection)?;
                values.insert(*binding, StandaloneValueBinding::Expression(value));
            }
            self.value_frames.push(values);
            arms.push(ExecutableSelectArm {
                pattern: pattern.clone(),
                output: self.expand_in_frame(*output, scoped.frame, Some(frame))?,
            });
        }
        Ok(arms)
    }

    fn semantic_statement_child_values(
        &self,
        parent_expression: CheckedExprId,
    ) -> Vec<CheckedExprId> {
        let Some(statement) = self.program.statements.iter().find(|statement| {
            statement.value == Some(parent_expression) && !statement.children.is_empty()
        }) else {
            return Vec::new();
        };
        let statements = self
            .program
            .statements
            .iter()
            .map(|statement| (statement.id, statement))
            .collect::<BTreeMap<_, _>>();
        let mut pending = statement.children.iter().rev().copied().collect::<Vec<_>>();
        let mut values = Vec::new();
        let mut visited = BTreeSet::new();
        while let Some(child) = pending.pop() {
            if !visited.insert(child) {
                continue;
            }
            let Some(statement) = statements.get(&child).copied() else {
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
                storage_binding,
                path,
                projection,
            } => {
                let mut projection = projection.clone();
                projection.extend(fields);
                ConcreteExpressionKind::CanonicalRead {
                    target: *target,
                    storage_binding: *storage_binding,
                    path: path.clone(),
                    projection,
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
        Ok(self.push(expression, owner, kind))
    }

    fn evaluation_owner(&self, scoped: ScopedCheckedExpr) -> Option<StaticOwnerId> {
        if let Some(port) = scoped.evaluation_port {
            return self.out_net.owner_for_net(self.out_net.net_for_port(port));
        }
        let expression = self
            .program
            .expressions
            .iter()
            .find(|expression| expression.id == scoped.expression)?;
        let mut scope = Some(expression.scope_id);
        while let Some(scope_id) = scope {
            let checked_scope = self
                .program
                .scopes
                .iter()
                .find(|scope| scope.id == scope_id)?;
            if checked_scope.kind == boon_typecheck::CheckedScopeKind::RepeatedOutput {
                let output = checked_scope.owner?;
                let net = self.out_net.output_net_in_frame(scoped.frame, output)?;
                return self.out_net.owner_for_net(net);
            }
            scope = checked_scope.parent;
        }
        None
    }

    fn push(
        &mut self,
        expression: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        kind: ConcreteExpressionKind,
    ) -> ConcreteExprId {
        let id = ConcreteExprId(self.expressions.len());
        self.expressions.push(ConcreteExpression {
            id,
            checked_expr_id: expression.id,
            flow_type: expression.flow_type.clone(),
            effect: expression.effect,
            owner,
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
        let executable = derive_executable_program(
            &checked,
            &out_net.graph,
            &materializations,
            &super::super::DistributedReferences::default(),
            expressions,
        )
        .unwrap();
        let [materialization] = materializations
            .try_into()
            .unwrap_or_else(|values: Vec<_>| {
                panic!("expected one materialization, found {}", values.len())
            });
        (materialization, executable)
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
    fn nested_materialization_source_inherits_its_parent_owner() {
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
        assert_eq!(
            out_net.graph.static_owners[inner.owner.as_usize()].parent,
            Some(outer.owner)
        );
        assert_eq!(
            expressions[inner.source.as_usize()].owner,
            Some(outer.owner),
            "a nested collection source evaluates once in its lexical parent row"
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
        let executable = derive_executable_program(
            &checked,
            &out_net.graph,
            &materializations,
            &super::super::DistributedReferences::default(),
            expressions,
        )
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
