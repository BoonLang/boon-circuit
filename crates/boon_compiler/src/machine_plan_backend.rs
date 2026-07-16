#![allow(clippy::too_many_arguments)]

use boon_ir::{
    self as ir, BytesScalarArg, DerivedValueKind, FileBytesPath, InitialValue,
    ListAppendFieldValue, ListInitializer, ListOperationKind, ListPredicate, ListProjectionKind,
    ListTextNormalization, TypedProgram, UpdateExpression, UpdateGuard, UpdateValueExpression,
};
use boon_parser::{
    AstCallArg, AstExpr, AstExprKind, AstStatement, AstStatementKind, BytesSizeSyntax,
};
use boon_plan::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

fn plan_source_id(value: ir::SourceId) -> SourceId {
    SourceId(value.0)
}

fn plan_state_id(value: ir::StateId) -> StateId {
    StateId(value.0)
}

fn plan_list_id(value: ir::ListId) -> ListId {
    ListId(value.0)
}

fn plan_field_id(value: ir::FieldId) -> FieldId {
    FieldId(value.0)
}

fn plan_scope_id(value: Option<ir::ScopeId>) -> Option<ScopeId> {
    value.map(|value| ScopeId(value.0))
}

fn demand_plan(program: &TypedProgram) -> DemandPlan {
    let observed_paths = program
        .view_bindings
        .iter()
        .flat_map(|binding| root_path_observation_variants(&binding.path))
        .collect::<BTreeSet<_>>();
    let field_ids = program
        .derived_values
        .iter()
        .filter(|derived| !derived.indexed)
        .filter(|derived| !statement_is_source_group(program, &derived.statement))
        .filter(|derived| root_path_is_observed(&observed_paths, &derived.path))
        .filter_map(|derived| match derived_output_ref(program, derived) {
            ValueRef::Field(field_id) => Some(field_id),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    DemandPlan {
        root_derived_outputs: RootOutputDemand::Selected(field_ids),
    }
}

fn effect_contracts(program: &TypedProgram) -> Result<Vec<EffectContract>, PlanError> {
    let mut effects = BTreeMap::new();
    for expression in &program.expressions {
        let host_operation = match &expression.kind {
            AstExprKind::Call { function, .. } => function.as_str(),
            AstExprKind::Pipe { op, .. } => op.as_str(),
            _ => continue,
        };
        let Some(contract) = builtin_effect_contract(host_operation)? else {
            continue;
        };
        if let Err(error) = contract.validate() {
            return Err(PlanError::new(format!(
                "host effect `{host_operation}` has no safe durable replay contract: {error}"
            )));
        }
        if let Some(existing) = effects.insert(contract.effect_id, contract.clone())
            && existing != contract
        {
            return Err(PlanError::new(format!(
                "host effect `{host_operation}` has conflicting centralized contracts"
            )));
        }
    }
    Ok(effects.into_values().collect())
}

fn effect_outbox_schemas(effects: &[EffectContract]) -> Result<Vec<EffectOutboxSchema>, PlanError> {
    let mut schemas = Vec::new();
    for contract in effects {
        let EffectReplay::Idempotent { .. } = &contract.replay else {
            continue;
        };
        let schema = builtin_effect_outbox_schema(&contract.host_operation)?.ok_or_else(|| {
            PlanError::new(format!(
                "idempotent host effect `{}` is missing a centralized intent/result outbox schema",
                contract.host_operation
            ))
        })?;
        schemas.push(schema);
    }
    schemas.sort_by_key(|schema| schema.effect_id);
    Ok(schemas)
}

fn bind_effect_outbox_invocations(
    schemas: &mut [EffectOutboxSchema],
    regions: &[OperationRegion],
) -> Result<(), PlanError> {
    let mut invocations = BTreeMap::<EffectId, Vec<EffectInvocationId>>::new();
    for invocation in regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::UpdateBranch {
                effect: Some(invocation),
                ..
            } => Some(invocation),
            _ => None,
        })
    {
        invocations
            .entry(invocation.effect_id)
            .or_default()
            .push(invocation.invocation_id);
    }
    for (effect_id, invocation_ids) in invocations {
        if let Some(schema) = schemas
            .iter_mut()
            .find(|schema| schema.effect_id == effect_id)
        {
            schema.bind_invocations(invocation_ids);
        }
    }
    Ok(())
}

fn effect_invocation_for_branch(
    branch: &boon_ir::UpdateBranch,
    expression_kind: PlanExpressionKind,
    ordered_inputs: &[ValueRef],
    output: Option<ValueRef>,
) -> Result<Option<EffectInvocationPlan>, PlanError> {
    let host_operation = match expression_kind {
        PlanExpressionKind::FileWriteBytes => "File/write_bytes",
        _ => return Ok(None),
    };
    let contract = builtin_effect_contract(host_operation)?.ok_or_else(|| {
        PlanError::new(format!(
            "effectful update has no centralized contract for `{host_operation}`"
        ))
    })?;
    contract.validate()?;
    let target = output.ok_or_else(|| {
        PlanError::new(format!(
            "effectful update `{}` has no result target",
            branch.target
        ))
    })?;
    let schema = builtin_effect_outbox_schema(host_operation)?.ok_or_else(|| {
        PlanError::new(format!(
            "effectful update has no centralized outbox schema for `{host_operation}`"
        ))
    })?;
    let DataTypePlan::Record {
        fields: intent_schema,
        open: false,
    } = schema.intent_type
    else {
        return Err(PlanError::new(format!(
            "effectful update `{host_operation}` has a non-record intent schema"
        )));
    };
    if intent_schema.len() != ordered_inputs.len() {
        return Err(PlanError::new(format!(
            "effectful update `{host_operation}` intent arity differs from its schema"
        )));
    }
    Ok(Some(EffectInvocationPlan {
        invocation_id: EffectInvocationId::from_semantic_route(
            contract.effect_id,
            &branch.source,
            &branch.target,
        )?,
        effect_id: contract.effect_id,
        intent_fields: intent_schema
            .into_iter()
            .zip(ordered_inputs.iter().cloned())
            .map(|(field, input)| EffectIntentFieldPlan {
                name: field.name,
                input,
                data_type: field.data_type,
            })
            .collect(),
        idempotency_key: EffectIdempotencyKeyPlan::InvocationTurnIntentSha256,
        result: EffectResultRoute::Target {
            target,
            policy: contract.result_policy,
        },
        barrier: contract.barrier,
    }))
}

fn host_effect_plan_op(
    program: &TypedProgram,
    declaration: &ir::HostEffectDeclaration,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    next_op: &mut usize,
) -> Result<PlanOp, PlanError> {
    let contract = builtin_effect_contract(&declaration.operation)?.ok_or_else(|| {
        PlanError::new(format!(
            "typed host effect `{}` has no centralized contract",
            declaration.operation
        ))
    })?;
    let schema = contract.schema.as_ref().ok_or_else(|| {
        PlanError::new(format!(
            "typed host effect `{}` has no centralized typed schema",
            declaration.operation
        ))
    })?;
    if semantic_data_type_plan(&declaration.intent_type).canonicalized() != schema.intent_type
        || semantic_data_type_plan(&declaration.result_type).canonicalized() != schema.result_type
    {
        return Err(PlanError::new(format!(
            "typed host effect declaration `{}` differs from the centralized schema",
            declaration.name
        )));
    }
    let ValueRef::Source(trigger_source_id) =
        index.resolve(&declaration.trigger_source).ok_or_else(|| {
            PlanError::new(format!(
                "typed host effect declaration `{}` has an unresolved trigger source `{}`",
                declaration.name, declaration.trigger_source
            ))
        })?
    else {
        return Err(PlanError::new(format!(
            "typed host effect declaration `{}` trigger is not a SOURCE",
            declaration.name
        )));
    };
    let DataTypePlan::Record {
        fields: schema_fields,
        open: false,
    } = &schema.intent_type
    else {
        return Err(PlanError::new(format!(
            "typed host effect `{}` has a non-record intent schema",
            declaration.operation
        )));
    };
    let declared_fields = declaration
        .intent_fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let mut intent_fields = Vec::with_capacity(schema_fields.len());
    for schema_field in schema_fields {
        let field = declared_fields
            .get(schema_field.name.as_str())
            .ok_or_else(|| {
                PlanError::new(format!(
                    "typed host effect declaration `{}` is missing intent field `{}`",
                    declaration.name, schema_field.name
                ))
            })?;
        if semantic_data_type_plan(&field.data_type).canonicalized() != schema_field.data_type {
            return Err(PlanError::new(format!(
                "typed host effect declaration `{}` intent field `{}` differs from its schema",
                declaration.name, field.name
            )));
        }
        let input = host_effect_intent_value_ref(
            program,
            index,
            constants,
            &declaration.trigger_source,
            field.value_expr_id.as_usize(),
        )
        .ok_or_else(|| {
            PlanError::new(format!(
                "typed host effect declaration `{}` intent field `{}` is not a lowerable value reference or constant",
                declaration.name, field.name
            ))
        })?;
        let input =
            normalize_semantic_list_memory_value_ref(program, input, &schema_field.data_type);
        intent_fields.push(EffectIntentFieldPlan {
            name: schema_field.name.clone(),
            input,
            data_type: schema_field.data_type.clone(),
        });
    }
    let DataTypePlan::Variant {
        variants: result_variants,
    } = &schema.result_type
    else {
        return Err(PlanError::new(format!(
            "correlated host effect `{}` has a non-variant result schema",
            declaration.operation
        )));
    };
    let declared_routes = declaration
        .result_routes
        .iter()
        .map(|route| (route.variant.as_str(), route))
        .collect::<BTreeMap<_, _>>();
    let mut variants = Vec::with_capacity(result_variants.len());
    for result_variant in result_variants {
        let route = declared_routes
            .get(result_variant.tag.as_str())
            .ok_or_else(|| {
                PlanError::new(format!(
                    "typed host effect declaration `{}` is missing result route `{}`",
                    declaration.name, result_variant.tag
                ))
            })?;
        let ValueRef::Source(source_id) = index.resolve(&route.source_path).ok_or_else(|| {
            PlanError::new(format!(
                "typed host effect declaration `{}` result route `{}` has unresolved SOURCE `{}`",
                declaration.name, result_variant.tag, route.source_path
            ))
        })?
        else {
            return Err(PlanError::new(format!(
                "typed host effect declaration `{}` result route `{}` is not a SOURCE",
                declaration.name, result_variant.tag
            )));
        };
        variants.push(EffectResultVariantRoute {
            tag: result_variant.tag.clone(),
            source_id,
        });
    }
    let ordered_inputs = intent_fields
        .iter()
        .map(|field| field.input.clone())
        .collect::<Vec<_>>();
    let mut inputs = vec![ValueRef::Source(trigger_source_id)];
    inputs.extend(ordered_inputs.iter().cloned());
    let indexed = program
        .sources
        .iter()
        .find(|source| source.path == declaration.trigger_source)
        .is_some_and(|source| source.scoped);
    let mut semantic_result_routes = declaration
        .result_routes
        .iter()
        .map(|route| format!("{}={}", route.variant, route.source_path))
        .collect::<Vec<_>>();
    semantic_result_routes.sort();
    let semantic_result_route = semantic_result_routes.join(";");
    Ok(op(
        next_op,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::HostEffect,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
            effect: Some(EffectInvocationPlan {
                invocation_id: EffectInvocationId::from_semantic_route(
                    contract.effect_id,
                    &declaration.trigger_source,
                    &semantic_result_route,
                )?,
                effect_id: contract.effect_id,
                intent_fields,
                idempotency_key: EffectIdempotencyKeyPlan::InvocationTurnIntentSha256,
                result: EffectResultRoute::CorrelatedSources { variants },
                barrier: contract.barrier,
            }),
        },
        unique_value_refs(inputs),
        None,
        indexed,
        0,
    ))
}

fn normalize_semantic_list_memory_value_ref(
    program: &TypedProgram,
    value_ref: ValueRef,
    expected_type: &DataTypePlan,
) -> ValueRef {
    let ValueRef::Field(field_id) = value_ref else {
        return value_ref;
    };
    if !matches!(expected_type, DataTypePlan::List { .. })
        || field_has_derived_computation(program, field_id)
    {
        return ValueRef::Field(field_id);
    }
    list_id_for_semantic_list_memory_field(program, field_id)
        .map(ValueRef::List)
        .unwrap_or(ValueRef::Field(field_id))
}

fn host_effect_intent_value_ref(
    program: &TypedProgram,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    trigger_source: &str,
    expr_id: usize,
) -> Option<ValueRef> {
    if let Some(path) = expression_path_string(program, expr_id) {
        return resolve_update_value_ref(index, trigger_source, "", false, &path);
    }
    constant_initial_expression_value(program, expr_id)
        .map(|value| ValueRef::Constant(push_plan_constant(constants, value)))
}

fn output_root_plans(
    program: &TypedProgram,
    document: Option<&DocumentPlan>,
    index: &ValueIndex,
) -> Result<Vec<OutputRootPlan>, PlanError> {
    let mut outputs = Vec::with_capacity(program.output_values.len());
    for output in &program.output_values {
        let demand = match output.demand {
            ir::SemanticOutputDemandPolicy::HostDemanded => OutputDemandPolicy::HostDemanded,
        };
        let (contract, value) = match output.contract {
            ir::SemanticOutputContractKind::RetainedVisual { kind } => {
                let document = document.ok_or_else(|| {
                    PlanError::new(format!(
                        "retained visual output root `{}` has no compiled document value",
                        output.root
                    ))
                })?;
                let contract = match kind {
                    ir::SemanticRetainedVisualKind::Document => OutputContractKind::Document,
                    ir::SemanticRetainedVisualKind::Scene => OutputContractKind::Scene,
                };
                let expected = match document.root.kind {
                    DocumentRootKind::Document => OutputContractKind::Document,
                    DocumentRootKind::Scene => OutputContractKind::Scene,
                };
                if contract != expected {
                    return Err(PlanError::new(format!(
                        "retained visual output root `{}` does not match its document value",
                        output.root
                    )));
                }
                (
                    contract,
                    OutputValueRef::RetainedVisual {
                        expression: document.root.expression,
                    },
                )
            }
            ir::SemanticOutputContractKind::HostValue => {
                let data_type = output.data_type.as_ref().ok_or_else(|| {
                    PlanError::new(format!(
                        "host output root `{}` has no closed inferred data type",
                        output.root
                    ))
                })?;
                let value = direct_statement_value_expr_id(&output.statement)
                    .and_then(|expr_id| expression_path_string(program, expr_id))
                    .and_then(|path| {
                        path.strip_prefix("store.")
                            .and_then(|local| index.resolve(local))
                            .or_else(|| index.resolve(&path))
                    })
                    .or_else(|| index.resolve(&output.value_path))
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "host output root `{}` has no executable current value `{}`",
                            output.root, output.value_path
                        ))
                    })?;
                (
                    OutputContractKind::HostValue {
                        data_type: semantic_data_type_plan(data_type),
                    },
                    OutputValueRef::RuntimeValue { value },
                )
            }
        };
        outputs.push(OutputRootPlan::new(
            output.root.clone(),
            contract,
            demand,
            value,
        )?);
    }
    outputs.sort_by(|left, right| left.name.cmp(&right.name));
    if outputs.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(PlanError::new("typed output root names must be unique"));
    }
    Ok(outputs)
}

fn host_port_plans(
    program: &TypedProgram,
    outputs: &[OutputRootPlan],
) -> Result<Vec<HostPortPlan>, PlanError> {
    let source_id = |path: &str, line: usize| {
        program
            .sources
            .iter()
            .find(|source| source.path == path)
            .map(|source| plan_source_id(source.id))
            .ok_or_else(|| {
                PlanError::new(format!(
                    "host port at line {line} references missing source `{path}`"
                ))
            })
    };
    let output_id = |name: &str, line: usize| {
        outputs
            .iter()
            .find(|output| output.name == name)
            .map(|output| output.id)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "host port at line {line} references missing output root `{name}`"
                ))
            })
    };

    program
        .host_ports
        .iter()
        .map(|port| match port {
            ir::HostPortDeclaration::HttpServer {
                line,
                request_source,
                disconnect_source,
                response_output,
            } => Ok(HostPortPlan::HttpServer {
                request_source: source_id(request_source, *line)?,
                disconnect_source: disconnect_source
                    .as_deref()
                    .map(|source| source_id(source, *line))
                    .transpose()?,
                response_output: output_id(response_output, *line)?,
            }),
            ir::HostPortDeclaration::WebSocketServer {
                line,
                open_source,
                message_source,
                close_source,
                error_source,
                actions_output,
            } => Ok(HostPortPlan::WebSocketServer {
                open_source: source_id(open_source, *line)?,
                message_source: source_id(message_source, *line)?,
                close_source: source_id(close_source, *line)?,
                error_source: source_id(error_source, *line)?,
                actions_output: output_id(actions_output, *line)?,
            }),
        })
        .collect()
}

fn statement_is_source_group(program: &TypedProgram, statement: &AstStatement) -> bool {
    !statement.children.is_empty()
        && statement.children.iter().all(|child| match child.kind {
            AstStatementKind::Source { .. } => true,
            AstStatementKind::Field { .. } => statement_is_source_group(program, child),
            _ if row_statement_is_empty_delimiter(child, program) => true,
            _ => false,
        })
}

fn root_path_observation_variants(path: &str) -> BTreeSet<String> {
    let mut variants = BTreeSet::from([path.to_owned()]);
    if let Some(passed) = path.strip_prefix("PASSED.") {
        variants.extend(root_path_observation_variants(passed));
    }
    if let Some(local) = path.strip_prefix("store.") {
        variants.insert(local.to_owned());
    } else if !path.starts_with('@') && !path.contains(':') {
        variants.insert(format!("store.{path}"));
    }
    variants
}

fn root_path_is_observed(observed_paths: &BTreeSet<String>, path: &str) -> bool {
    root_path_observation_variants(path)
        .into_iter()
        .any(|candidate| {
            observed_paths.contains(&candidate)
                || observed_paths.iter().any(|observed| {
                    observed
                        .strip_prefix(&candidate)
                        .is_some_and(|suffix| suffix.starts_with('.'))
                })
        })
}

fn ir_scope_id(value: Option<ScopeId>) -> Option<ir::ScopeId> {
    value.map(|value| ir::ScopeId(value.0))
}

fn source_payload_schema_from_ir(
    program: &TypedProgram,
    source: &ir::SourcePort,
) -> Result<SourcePayloadSchema, PlanError> {
    let value = &source.payload_schema;
    let row_lookup_field_id = value
        .row_lookup_field_name()
        .map(|field_name| {
            let scope_id = source.scope_id.ok_or_else(|| {
                PlanError::new(format!(
                    "source `{}` declares row lookup field `{field_name}` without a row scope",
                    source.path
                ))
            })?;
            let scope = program
                .row_scopes
                .iter()
                .find(|scope| scope.id == scope_id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "source `{}` row scope {} is not declared",
                        source.path, scope_id.0
                    ))
                })?;
            let semantic_path = format!("{}.{field_name}", scope.row_scope);
            program
                .semantic_index
                .fields
                .iter()
                .find(|field| field.path == semantic_path)
                .map(|field| plan_field_id(field.id))
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "source `{}` row lookup field `{semantic_path}` has no typed FieldId",
                        source.path
                    ))
                })
        })
        .transpose()?;
    Ok(SourcePayloadSchema {
        fields: value
            .fields
            .iter()
            .map(source_payload_field_from_ir)
            .collect(),
        typed_fields: value
            .typed_fields
            .iter()
            .map(source_payload_descriptor_from_ir)
            .collect(),
        row_lookup_field: value.row_lookup_field_name().map(str::to_owned),
        row_lookup_field_id,
    })
}

fn source_payload_descriptor_from_ir(
    value: &ir::SourcePayloadDescriptor,
) -> SourcePayloadDescriptor {
    SourcePayloadDescriptor {
        field: source_payload_field_from_ir(&value.field),
        data_type: semantic_data_type_plan(&value.data_type).canonicalized(),
    }
}

fn source_payload_field_from_ir(value: &ir::SourcePayloadField) -> SourcePayloadField {
    match value {
        ir::SourcePayloadField::Address => SourcePayloadField::Address,
        ir::SourcePayloadField::Bytes => SourcePayloadField::Bytes,
        ir::SourcePayloadField::Key => SourcePayloadField::Key,
        ir::SourcePayloadField::Named(name) => SourcePayloadField::Named(name.clone()),
        ir::SourcePayloadField::Text => SourcePayloadField::Text,
    }
}

fn plan_value_type_from_initial(value: &InitialValue) -> PlanValueType {
    match value {
        InitialValue::Text { .. } => PlanValueType::Text,
        InitialValue::Number { .. } => PlanValueType::Number,
        InitialValue::Byte { .. } => PlanValueType::Byte,
        InitialValue::Bool { .. } => PlanValueType::Bool,
        InitialValue::Bytes { fixed_len, .. } => PlanValueType::Bytes {
            fixed_len: fixed_len.map(|len| len as u64),
        },
        InitialValue::Enum { .. } => PlanValueType::Enum,
        InitialValue::Data { .. } => PlanValueType::Data,
        InitialValue::RootInitialField { .. } => PlanValueType::RootInitialField,
        InitialValue::RowInitialField { .. } => PlanValueType::RowInitialField,
        InitialValue::Unknown { .. } => PlanValueType::Unknown,
    }
}

fn plan_value_type_from_initial_with_row_fields(
    value: &InitialValue,
    scope_id: Option<ScopeId>,
    row_field_types: &RowInitialFieldTypeMap,
) -> PlanValueType {
    match value {
        InitialValue::RowInitialField { path } => {
            row_initial_field_value_type(row_field_types, scope_id, path)
                .unwrap_or(PlanValueType::RowInitialField)
        }
        _ => plan_value_type_from_initial(value),
    }
}

fn plan_value_type_from_initial_with_root_and_row_fields(
    state_path: &str,
    value: &InitialValue,
    scope_id: Option<ScopeId>,
    root_field_types: &RootInitialFieldTypeMap,
    row_field_types: &RowInitialFieldTypeMap,
) -> PlanValueType {
    match value {
        InitialValue::RootInitialField { .. } => root_field_types
            .get(state_path)
            .copied()
            .unwrap_or(PlanValueType::RootInitialField),
        _ => plan_value_type_from_initial_with_row_fields(value, scope_id, row_field_types),
    }
}

fn state_initial_value_type(
    program: &TypedProgram,
    state: &boon_ir::StateCell,
    root_field_types: &RootInitialFieldTypeMap,
    row_field_types: &RowInitialFieldTypeMap,
    expression_types: &BTreeMap<usize, PlanValueType>,
) -> PlanValueType {
    let declared = plan_value_type_from_initial_with_root_and_row_fields(
        &state.path,
        &state.initial_value,
        plan_scope_id(state.scope_id),
        root_field_types,
        row_field_types,
    );
    if plan_value_type_is_concrete(declared) {
        return declared;
    }
    state
        .initial_expr_id
        .and_then(|expr_id| inferred_expression_value_type(program, expr_id.0, expression_types))
        .filter(|value_type| plan_value_type_is_concrete(*value_type))
        .unwrap_or(declared)
}

fn initial_value_kind_from_ir(value: &InitialValue) -> InitialValueKind {
    match value {
        InitialValue::Text { .. } => InitialValueKind::Text,
        InitialValue::Number { .. } => InitialValueKind::Number,
        InitialValue::Byte { .. } => InitialValueKind::Byte,
        InitialValue::Bool { .. } => InitialValueKind::Bool,
        InitialValue::Bytes { .. } => InitialValueKind::Bytes,
        InitialValue::Enum { .. } => InitialValueKind::Enum,
        InitialValue::Data { .. } => InitialValueKind::Data,
        InitialValue::RootInitialField { .. } => InitialValueKind::RootInitialField,
        InitialValue::RowInitialField { .. } => InitialValueKind::RowInitialField,
        InitialValue::Unknown { .. } => InitialValueKind::Unknown,
    }
}

fn list_initializer_kind_from_ir(value: &ListInitializer) -> ListInitializerKind {
    match value {
        ListInitializer::RecordLiteral { .. } => ListInitializerKind::RecordLiteral,
        ListInitializer::Range { .. } => ListInitializerKind::Range,
        ListInitializer::Empty => ListInitializerKind::Empty,
        ListInitializer::Unknown { .. } => ListInitializerKind::Unknown,
    }
}

fn plan_range_initializer(value: &ListInitializer) -> Option<PlanRangeInitializer> {
    match value {
        ListInitializer::Range { from, to } => Some(PlanRangeInitializer {
            from: *from,
            to: *to,
        }),
        ListInitializer::RecordLiteral { .. }
        | ListInitializer::Empty
        | ListInitializer::Unknown { .. } => None,
    }
}

fn plan_derived_kind_from_ir(value: &DerivedValueKind) -> PlanDerivedKind {
    match value {
        DerivedValueKind::SourceEventTransform => PlanDerivedKind::SourceEventTransform,
        DerivedValueKind::ListView => PlanDerivedKind::ListView,
        DerivedValueKind::Aggregate => PlanDerivedKind::Aggregate,
        DerivedValueKind::Pure => PlanDerivedKind::Pure,
        DerivedValueKind::Unknown => PlanDerivedKind::Unknown,
    }
}

fn state_initial_provenance(slot: &ScalarStorageSlot) -> InitialProvenance {
    match slot.initial_value_kind {
        InitialValueKind::Unknown => InitialProvenance::MaterializedAuthority,
        InitialValueKind::Text
        | InitialValueKind::Number
        | InitialValueKind::Byte
        | InitialValueKind::Bool
        | InitialValueKind::Bytes
        | InitialValueKind::Enum
        | InitialValueKind::Data
        | InitialValueKind::RootInitialField
        | InitialValueKind::RowInitialField => InitialProvenance::ReconstructableDefault,
    }
}

#[derive(Clone)]
struct MigrationStorageDefault {
    value_type: PlanValueType,
    initial_value_kind: InitialValueKind,
    constant: Option<PlanConstantValue>,
    indexed_edge: Option<ir::MigrationEdge>,
}

fn plan_value_type_from_semantic_data_type(data_type: &DataTypePlan) -> PlanValueType {
    match data_type {
        DataTypePlan::Text => PlanValueType::Text,
        DataTypePlan::Number => PlanValueType::Number,
        DataTypePlan::Byte => PlanValueType::Byte,
        DataTypePlan::Bool => PlanValueType::Bool,
        DataTypePlan::Bytes { fixed_len } => PlanValueType::Bytes {
            fixed_len: *fixed_len,
        },
        DataTypePlan::Variant { .. } => PlanValueType::Enum,
        DataTypePlan::Null
        | DataTypePlan::Record { .. }
        | DataTypePlan::List { .. }
        | DataTypePlan::Error { .. } => PlanValueType::Data,
        DataTypePlan::Unknown => PlanValueType::Unknown,
    }
}

fn initial_value_kind_from_plan_type(value_type: PlanValueType) -> InitialValueKind {
    match value_type {
        PlanValueType::Text => InitialValueKind::Text,
        PlanValueType::Number => InitialValueKind::Number,
        PlanValueType::Byte => InitialValueKind::Byte,
        PlanValueType::Bool => InitialValueKind::Bool,
        PlanValueType::Bytes { .. } => InitialValueKind::Bytes,
        PlanValueType::Enum => InitialValueKind::Enum,
        PlanValueType::Data => InitialValueKind::Data,
        PlanValueType::RootInitialField => InitialValueKind::RootInitialField,
        PlanValueType::RowInitialField => InitialValueKind::RowInitialField,
        PlanValueType::Unknown => InitialValueKind::Unknown,
    }
}

fn deterministic_fresh_constant(data_type: &DataTypePlan) -> Option<PlanConstantValue> {
    match data_type {
        DataTypePlan::Text => Some(PlanConstantValue::Text {
            value: String::new(),
        }),
        DataTypePlan::Number => Some(PlanConstantValue::Number {
            value: FiniteReal::ZERO,
        }),
        DataTypePlan::Byte => Some(PlanConstantValue::Byte { value: 0 }),
        DataTypePlan::Bool => Some(PlanConstantValue::Bool { value: false }),
        DataTypePlan::Bytes {
            fixed_len: None | Some(0),
        } => {
            let mut hasher = Sha256::new();
            hasher.update([]);
            Some(PlanConstantValue::Bytes {
                byte_len: 0,
                sha256: format!("{:x}", hasher.finalize()),
                inline_bytes: Some(Vec::new()),
            })
        }
        DataTypePlan::Variant { variants } => {
            variants.first().map(|variant| PlanConstantValue::Enum {
                value: variant.tag.clone(),
            })
        }
        DataTypePlan::Null
        | DataTypePlan::Bytes { fixed_len: Some(_) }
        | DataTypePlan::Record { .. }
        | DataTypePlan::List { .. }
        | DataTypePlan::Error { .. }
        | DataTypePlan::Unknown => None,
    }
}

fn semantic_memory_for_state<'a>(
    program: &'a TypedProgram,
    state: &ir::StateCell,
) -> Option<&'a ir::SemanticMemory> {
    program.semantic_memory.iter().find(|memory| {
        semantic_memory_is_runtime_active(program, memory)
            && matches!(
                memory.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
                    | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. }
                    if state_id == state.id
            )
    })
}

fn state_has_active_semantic_memory(program: &TypedProgram, state: &ir::StateCell) -> bool {
    semantic_memory_for_state(program, state).is_some()
}

fn list_has_active_semantic_memory(program: &TypedProgram, list: &ir::ListMemory) -> bool {
    program.semantic_memory.iter().any(|memory| {
        semantic_memory_is_active(memory)
            && matches!(
                memory.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List { list_id, .. } if list_id == list.id
            )
    })
}

struct MigrationListStorageDefault {
    initializer_kind: ListInitializerKind,
    range: Option<PlanRangeInitializer>,
    initial_rows: Vec<PlanInitialListRow>,
}

fn migration_list_storage_default(
    program: &TypedProgram,
    list: &ir::ListMemory,
    synthetic_initial_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<Option<MigrationListStorageDefault>, PlanError> {
    let Some(destination_memory) = program.semantic_memory.iter().find(|memory| {
        semantic_memory_is_active(memory)
            && matches!(
                memory.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List { list_id, .. } if list_id == list.id
            )
    }) else {
        return Ok(None);
    };
    let Some(edge) = program.migration_edges.iter().find(|edge| {
        edge.transfer_kind == ir::MigrationTransferKind::List
            && edge.destination.memory_id == destination_memory.id
    }) else {
        return Ok(None);
    };
    if edge.transform != ir::MigrationTransform::Identity || edge.source_leaves.len() != 1 {
        return Err(PlanError::new(
            "whole-list migration default requires one identity source",
        ));
    }
    let source_memory = program
        .semantic_memory
        .get(edge.source_leaves[0].memory_id.as_usize())
        .ok_or_else(|| PlanError::new("whole-list migration default source memory is absent"))?;
    let source_list_id = match source_memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::List { list_id, .. } => list_id,
        _ => {
            return Err(PlanError::new(
                "whole-list migration default source is not a list",
            ));
        }
    };
    let source_list = program
        .lists
        .iter()
        .find(|source| source.id == source_list_id)
        .ok_or_else(|| PlanError::new("whole-list migration default source list is absent"))?;
    if matches!(source_list.initializer, ListInitializer::Unknown { .. }) {
        return Err(PlanError::new(format!(
            "whole-list migration from `{}` cannot reconstruct sparse default rows",
            source_memory.identity.semantic_path
        )));
    }
    let initial_rows = plan_initial_list_rows(
        program,
        list,
        &source_list.initializer,
        synthetic_initial_field_ids,
    );
    if initial_rows
        .iter()
        .flat_map(|row| &row.fields)
        .any(|field| field.field_id.is_none())
    {
        return Err(PlanError::new(format!(
            "whole-list migration from `{}` cannot map a default row field into `{}`",
            source_memory.identity.semantic_path, destination_memory.identity.semantic_path
        )));
    }
    Ok(Some(MigrationListStorageDefault {
        initializer_kind: list_initializer_kind_from_ir(&source_list.initializer),
        range: plan_range_initializer(&source_list.initializer),
        initial_rows,
    }))
}

fn compiled_list_storage_slot(
    program: &TypedProgram,
    list: &ir::ListMemory,
    id: PlanStorageId,
    synthetic_initial_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<ListStorageSlot, PlanError> {
    let migration_default =
        migration_list_storage_default(program, list, synthetic_initial_field_ids)?;
    Ok(ListStorageSlot {
        id,
        list_id: plan_list_id(list.id),
        scope_id: plan_scope_id(list.row_scope_id),
        row_field_ids: list_row_field_ids(program, list, synthetic_initial_field_ids),
        capacity: list.capacity,
        hidden_key_type: list.hidden_key_type.clone(),
        has_generation: list.has_generation,
        initializer_kind: migration_default.as_ref().map_or_else(
            || list_initializer_kind_from_ir(&list.initializer),
            |value| value.initializer_kind,
        ),
        range: migration_default.as_ref().map_or_else(
            || plan_range_initializer(&list.initializer),
            |value| value.range,
        ),
        initial_rows: migration_default.map_or_else(
            || {
                plan_initial_list_rows(
                    program,
                    list,
                    &list.initializer,
                    synthetic_initial_field_ids,
                )
            },
            |value| value.initial_rows,
        ),
    })
}

fn migration_identity_source_constant(
    program: &TypedProgram,
    edge: &ir::MigrationEdge,
) -> Option<PlanConstantValue> {
    if edge.transform != ir::MigrationTransform::Identity || edge.source_leaves.len() != 1 {
        return None;
    }
    let source_memory = program
        .semantic_memory
        .get(edge.source_leaves[0].memory_id.as_usize())?;
    let source_state_id = match source_memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
        | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. } => state_id,
        ir::SemanticMemoryRuntimeBacking::List { .. } => return None,
    };
    let source_state = program
        .state_cells
        .iter()
        .find(|state| state.id == source_state_id)?;
    initial_constant_value(&source_state.initial_value)
}

fn migration_storage_default(
    program: &TypedProgram,
    state: &ir::StateCell,
) -> Option<MigrationStorageDefault> {
    let memory = semantic_memory_for_state(program, state)?;
    let edge = program
        .migration_edges
        .iter()
        .find(|edge| edge.destination.memory_id == memory.id)?;
    let data_type = semantic_data_type_plan(&memory.data_type).canonicalized();
    let value_type = plan_value_type_from_semantic_data_type(&data_type);
    if value_type == PlanValueType::Unknown {
        return None;
    }
    if state.indexed && edge.transfer_kind == ir::MigrationTransferKind::IndexedField {
        return Some(MigrationStorageDefault {
            value_type,
            initial_value_kind: InitialValueKind::RowInitialField,
            constant: None,
            indexed_edge: Some(edge.clone()),
        });
    }
    let constant = migration_identity_source_constant(program, edge)
        .or_else(|| deterministic_fresh_constant(&data_type))?;
    Some(MigrationStorageDefault {
        value_type,
        initial_value_kind: initial_value_kind_from_plan_type(value_type),
        constant: Some(constant),
        indexed_edge: None,
    })
}

fn list_initial_provenance(slot: &ListStorageSlot) -> InitialProvenance {
    match slot.initializer_kind {
        ListInitializerKind::Unknown => InitialProvenance::MaterializedAuthority,
        ListInitializerKind::RecordLiteral
        | ListInitializerKind::Range
        | ListInitializerKind::Empty => InitialProvenance::ReconstructableDefault,
    }
}

fn semantic_data_type_plan(value: &ir::SemanticDataType) -> DataTypePlan {
    match value {
        ir::SemanticDataType::Null => DataTypePlan::Null,
        ir::SemanticDataType::Bool => DataTypePlan::Bool,
        ir::SemanticDataType::Number => DataTypePlan::Number,
        ir::SemanticDataType::Byte => DataTypePlan::Byte,
        ir::SemanticDataType::Text => DataTypePlan::Text,
        ir::SemanticDataType::Bytes { fixed_len } => DataTypePlan::Bytes {
            fixed_len: fixed_len.map(|len| len as u64),
        },
        ir::SemanticDataType::Variant { variants } => DataTypePlan::Variant {
            variants: variants
                .iter()
                .map(|variant| DataVariantPlan {
                    tag: variant.tag.clone(),
                    fields: variant
                        .fields
                        .iter()
                        .map(|field| DataTypeFieldPlan {
                            name: field.name.clone(),
                            data_type: semantic_data_type_plan(&field.data_type),
                        })
                        .collect(),
                    open: variant.open,
                })
                .collect(),
        }
        .canonicalized(),
        ir::SemanticDataType::Record { fields, open } => DataTypePlan::Record {
            fields: fields
                .iter()
                .map(|field| DataTypeFieldPlan {
                    name: field.name.clone(),
                    data_type: semantic_data_type_plan(&field.data_type),
                })
                .collect(),
            open: *open,
        }
        .canonicalized(),
        ir::SemanticDataType::List { item } => DataTypePlan::List {
            item: Box::new(semantic_data_type_plan(item)),
        },
        ir::SemanticDataType::Unknown { .. } => DataTypePlan::Unknown,
    }
}

fn semantic_memory_kind(kind: ir::SemanticMemoryKind) -> MemoryKind {
    match kind {
        ir::SemanticMemoryKind::RootScalar => MemoryKind::Scalar,
        ir::SemanticMemoryKind::IndexedField => MemoryKind::IndexedField,
        ir::SemanticMemoryKind::ListOwner => MemoryKind::List,
    }
}

fn semantic_memory_owner(memory: &ir::SemanticMemory) -> MemoryOwnerPath {
    MemoryOwnerPath {
        canonical_module: memory.identity.canonical_module.clone(),
        named_owner_path: memory.identity.owner_path.clone(),
    }
}

fn semantic_memory_id(memory: &ir::SemanticMemory) -> Result<MemoryId, PlanError> {
    MemoryId::from_identity(
        &semantic_memory_owner(memory),
        &memory.identity.semantic_path,
        semantic_memory_kind(memory.identity.kind),
    )
}

fn semantic_memory_is_active(memory: &ir::SemanticMemory) -> bool {
    matches!(memory.status, ir::SemanticMemoryStatus::Active)
}

fn semantic_memory_is_runtime_active(program: &TypedProgram, memory: &ir::SemanticMemory) -> bool {
    if !semantic_memory_is_active(memory) {
        return false;
    }
    let ir::SemanticMemoryRuntimeBacking::IndexedState {
        list_id: Some(list_id),
        ..
    } = memory.runtime_backing
    else {
        return true;
    };
    program.semantic_memory.iter().any(|candidate| {
        semantic_memory_is_active(candidate)
            && matches!(
                candidate.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List {
                    list_id: candidate_list_id,
                    ..
                } if candidate_list_id == list_id
            )
    })
}

fn state_for_semantic_memory<'a>(
    program: &'a TypedProgram,
    memory: &ir::SemanticMemory,
) -> Result<&'a ir::StateCell, PlanError> {
    let state_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
        | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. } => state_id,
        ir::SemanticMemoryRuntimeBacking::List { .. } => {
            return Err(PlanError::new(format!(
                "semantic memory `{}` has list backing where state backing is required",
                memory.identity.semantic_path
            )));
        }
    };
    program
        .state_cells
        .iter()
        .find(|state| state.id == state_id)
        .ok_or_else(|| {
            PlanError::new(format!(
                "semantic memory `{}` references missing state backing {}",
                memory.identity.semantic_path, state_id.0
            ))
        })
}

fn scalar_slot_for_semantic_memory<'a>(
    memory: &ir::SemanticMemory,
    scalar_slots: &'a [ScalarStorageSlot],
) -> Result<&'a ScalarStorageSlot, PlanError> {
    let state_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
        | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. } => state_id,
        ir::SemanticMemoryRuntimeBacking::List { .. } => {
            return Err(PlanError::new(format!(
                "semantic memory `{}` has no scalar runtime backing",
                memory.identity.semantic_path
            )));
        }
    };
    scalar_slots
        .iter()
        .find(|slot| slot.state_id == plan_state_id(state_id))
        .ok_or_else(|| {
            PlanError::new(format!(
                "semantic memory `{}` cannot resolve state slot {}",
                memory.identity.semantic_path, state_id.0
            ))
        })
}

fn semantic_scalar_memory_plan(
    program: &TypedProgram,
    memory: &ir::SemanticMemory,
    scalar_slots: &[ScalarStorageSlot],
) -> Result<MemoryPlan, PlanError> {
    let slot = scalar_slot_for_semantic_memory(memory, scalar_slots)?;
    let state = state_for_semantic_memory(program, memory)?;
    if memory.identity.semantic_path == format!("hold_{}", state.source_line) {
        return Err(PlanError::new(format!(
            "persistence identity cannot use anonymous line-based state `{}` at line {}; name the state under a stable semantic owner",
            memory.identity.semantic_path, state.source_line
        )));
    }
    let kind = semantic_memory_kind(memory.identity.kind);
    if kind == MemoryKind::List {
        return Err(PlanError::new(
            "list semantic memory cannot use scalar plan",
        ));
    }
    if slot.indexed != (kind == MemoryKind::IndexedField) {
        return Err(PlanError::new(format!(
            "semantic memory `{}` kind disagrees with runtime backing",
            memory.identity.semantic_path
        )));
    }
    let owner = semantic_memory_owner(memory);
    let memory_id = semantic_memory_id(memory)?;
    let data_type = semantic_data_type_plan(&memory.data_type).canonicalized();
    let mut leaves = memory
        .leaves
        .iter()
        .map(|leaf| {
            MemoryLeafPlan::new(
                memory_id,
                None,
                leaf.semantic_path.clone(),
                semantic_data_type_plan(&leaf.data_type),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    leaves.sort_by_key(|leaf| leaf.leaf_id);
    if leaves.is_empty() {
        return Err(PlanError::new(format!(
            "semantic memory `{}` has no durable leaves",
            memory.identity.semantic_path
        )));
    }
    Ok(MemoryPlan {
        runtime_slot: slot.id,
        memory_id,
        kind,
        semantic_path: memory.identity.semantic_path.clone(),
        type_fingerprint: data_type_fingerprint(&data_type)?,
        data_type,
        initial_provenance: state_initial_provenance(slot),
        owner,
        leaves,
    })
}

fn semantic_list_memory_plan(
    program: &TypedProgram,
    memory: &ir::SemanticMemory,
    list_slots: &[ListStorageSlot],
    synthetic_initial_field_ids: &BTreeMap<(String, String), FieldId>,
    index: &ValueIndex,
    include_draining_fields: bool,
) -> Result<ListMemoryPlan, PlanError> {
    let list_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::List { list_id, .. } => list_id,
        _ => {
            return Err(PlanError::new(format!(
                "semantic list `{}` has no list runtime backing",
                memory.identity.semantic_path
            )));
        }
    };
    let list = program
        .lists
        .iter()
        .find(|list| list.id == list_id)
        .ok_or_else(|| {
            PlanError::new(format!(
                "semantic list `{}` references missing list backing {}",
                memory.identity.semantic_path, list_id.0
            ))
        })?;
    let slot = list_slots
        .iter()
        .find(|slot| slot.list_id == plan_list_id(list_id))
        .ok_or_else(|| {
            PlanError::new(format!(
                "semantic list `{}` cannot resolve runtime slot {}",
                memory.identity.semantic_path, list_id.0
            ))
        })?;
    let owner = semantic_memory_owner(memory);
    let memory_id = semantic_memory_id(memory)?;
    let indexed_memory = program
        .semantic_memory
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::IndexedState {
                    list_id: Some(candidate_list),
                    ..
                } if candidate_list == list_id
            )
        })
        .collect::<Vec<_>>();
    let has_indexed_memory = !indexed_memory.is_empty();
    let semantic_list_type = semantic_data_type_plan(&memory.data_type).canonicalized();
    let DataTypePlan::List { item } = semantic_list_type.clone() else {
        return Err(PlanError::new(format!(
            "semantic list `{}` does not have a list data type",
            memory.identity.semantic_path
        )));
    };
    let DataTypePlan::Record {
        fields: semantic_row_fields,
        ..
    } = *item
    else {
        return Err(PlanError::new(format!(
            "semantic list `{}` does not have a record row type",
            memory.identity.semantic_path
        )));
    };
    let append_field_types = list_append_authoritative_field_types(program, index, &list.name)?;
    let mut row_fields = Vec::new();
    if !has_indexed_memory {
        for field in &semantic_row_fields {
            let runtime_field_id = storage_input_field_id(
                program,
                &list.name,
                &field.name,
                synthetic_initial_field_ids,
            )
            .filter(|field_id| slot.row_field_ids.contains(field_id));
            let Some(runtime_field_id) = runtime_field_id else {
                continue;
            };
            row_fields.push(MemoryLeafPlan::new(
                memory_id,
                Some(runtime_field_id),
                format!("{}.{}", memory.identity.semantic_path, field.name),
                field.data_type.clone(),
            )?);
        }
        for ((_, field_name), runtime_field_id) in
            synthetic_initial_field_ids
                .iter()
                .filter(|((list_name, _), field_id)| {
                    list_name == &list.name && slot.row_field_ids.contains(field_id)
                })
        {
            if row_fields
                .iter()
                .any(|field| field.runtime_field_id == Some(*runtime_field_id))
            {
                continue;
            }
            let field_type = semantic_row_fields
                .iter()
                .find(|field| field.name == *field_name)
                .map(|field| field.data_type.clone())
                .or_else(|| list_initializer_field_type(list, field_name))
                .or_else(|| append_field_types.get(field_name).cloned())
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "authoritative constructor field `{}.{field_name}` has no canonical row type",
                        list.name
                    ))
                })?;
            row_fields.push(MemoryLeafPlan::new(
                memory_id,
                Some(*runtime_field_id),
                format!("{}.{}", memory.identity.semantic_path, field_name),
                field_type,
            )?);
        }
    } else {
        for ((_, field_name), runtime_field_id) in
            synthetic_initial_field_ids
                .iter()
                .filter(|((list_name, _), field_id)| {
                    list_name == &list.name && slot.row_field_ids.contains(field_id)
                })
        {
            let field_type = semantic_row_fields
                .iter()
                .find(|field| field.name == *field_name)
                .map(|field| field.data_type.clone())
                .or_else(|| list_initializer_field_type(list, field_name))
                .or_else(|| append_field_types.get(field_name).cloned())
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "authoritative constructor field `{}.{field_name}` has no canonical row type",
                        list.name
                    ))
                })?;
            row_fields.push(MemoryLeafPlan::new(
                memory_id,
                Some(*runtime_field_id),
                format!("{}.$input${field_name}", memory.identity.semantic_path),
                field_type,
            )?);
        }
        for field_memory in indexed_memory.into_iter().filter(|field| {
            include_draining_fields || semantic_memory_is_runtime_active(program, field)
        }) {
            let field_id = match field_memory.runtime_backing {
                ir::SemanticMemoryRuntimeBacking::IndexedState {
                    field_id: Some(field_id),
                    ..
                } => plan_field_id(field_id),
                _ => {
                    return Err(PlanError::new(format!(
                        "indexed semantic memory `{}` has no runtime field backing",
                        field_memory.identity.semantic_path
                    )));
                }
            };
            if !slot.row_field_ids.contains(&field_id) {
                return Err(PlanError::new(format!(
                    "indexed semantic memory `{}` field {} is absent from list slot",
                    field_memory.identity.semantic_path, field_id.0
                )));
            }
            row_fields.push(MemoryLeafPlan::new(
                memory_id,
                Some(field_id),
                field_memory.identity.semantic_path.clone(),
                semantic_data_type_plan(&field_memory.data_type),
            )?);
        }
    }
    let mut runtime_field_ids = BTreeSet::new();
    if row_fields.iter().any(|field| {
        field
            .runtime_field_id
            .is_none_or(|field_id| !runtime_field_ids.insert(field_id))
    }) {
        return Err(PlanError::new(format!(
            "semantic list `{}` has duplicate or missing authoritative row field identities",
            memory.identity.semantic_path
        )));
    }
    row_fields.sort_by_key(|field| field.leaf_id);
    let row_type = DataTypePlan::Record {
        fields: row_fields
            .iter()
            .map(|field| DataTypeFieldPlan {
                name: field
                    .semantic_path
                    .rsplit_once('.')
                    .map_or_else(|| field.semantic_path.clone(), |(_, name)| name.to_owned()),
                data_type: field.data_type.clone(),
            })
            .collect(),
        open: false,
    }
    .canonicalized();
    let data_type = if has_indexed_memory || !row_fields.is_empty() {
        DataTypePlan::List {
            item: Box::new(row_type),
        }
    } else {
        semantic_list_type
    };
    ListMemoryPlan::new(
        slot.id,
        memory.identity.semantic_path.clone(),
        data_type,
        list_initial_provenance(slot),
        owner,
        list.hidden_key_type.clone(),
        list.has_generation,
        row_fields,
    )
}

fn data_type_plan_from_initial_value(value: &InitialValue) -> Option<DataTypePlan> {
    Some(match value {
        InitialValue::Text { .. } => DataTypePlan::Text,
        InitialValue::Number { .. } => DataTypePlan::Number,
        InitialValue::Byte { .. } => DataTypePlan::Byte,
        InitialValue::Bool { .. } => DataTypePlan::Bool,
        InitialValue::Bytes { fixed_len, .. } => DataTypePlan::Bytes {
            fixed_len: fixed_len.map(|len| len as u64),
        },
        InitialValue::Enum { value } => DataTypePlan::Variant {
            variants: vec![DataVariantPlan {
                tag: value.clone(),
                fields: Vec::new(),
                open: false,
            }],
        },
        InitialValue::Data { value } => data_type_plan_from_data(value),
        InitialValue::RootInitialField { .. }
        | InitialValue::RowInitialField { .. }
        | InitialValue::Unknown { .. } => return None,
    })
}

fn list_initializer_field_type(
    list: &boon_ir::ListMemory,
    field_name: &str,
) -> Option<DataTypePlan> {
    match &list.initializer {
        ListInitializer::Range { .. } if matches!(field_name, "index" | "value") => {
            Some(DataTypePlan::Number)
        }
        ListInitializer::RecordLiteral { rows } => rows
            .iter()
            .flat_map(|row| &row.fields)
            .find(|field| field.name == field_name)
            .and_then(|field| data_type_plan_from_initial_value(&field.value)),
        ListInitializer::Empty
        | ListInitializer::Unknown { .. }
        | ListInitializer::Range { .. } => None,
    }
}

fn list_append_authoritative_field_types(
    program: &TypedProgram,
    index: &ValueIndex,
    list_name: &str,
) -> Result<BTreeMap<String, DataTypePlan>, PlanError> {
    let mut field_types = BTreeMap::new();
    for operation in program
        .list_operations
        .iter()
        .filter(|operation| operation.list == list_name)
    {
        let ListOperationKind::Append { trigger, fields } = &operation.kind else {
            continue;
        };
        for field in fields {
            let data_type = match &field.value {
                ListAppendFieldValue::Source { path } => {
                    let value_ref = list_append_value_ref(program, index, trigger, path)
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "append field `{list_name}.{}` has no typed value reference",
                                field.name
                            ))
                        })?;
                    data_type_plan_for_value_ref(program, index, &value_ref).ok_or_else(|| {
                        PlanError::new(format!(
                            "append field `{list_name}.{}` has no canonical value type",
                            field.name
                        ))
                    })?
                }
                ListAppendFieldValue::Const { value } => match append_constant_value(value) {
                    PlanConstantValue::Text { .. } => DataTypePlan::Text,
                    PlanConstantValue::Number { .. } => DataTypePlan::Number,
                    PlanConstantValue::Bool { .. } => DataTypePlan::Bool,
                    _ => {
                        return Err(PlanError::new(format!(
                            "append field `{list_name}.{}` has an unsupported constant type",
                            field.name
                        )));
                    }
                },
                ListAppendFieldValue::TypedConst { value } => {
                    data_type_plan_from_initial_value(value).ok_or_else(|| {
                        PlanError::new(format!(
                            "append field `{list_name}.{}` has no canonical typed constant",
                            field.name
                        ))
                    })?
                }
            };
            if let Some(previous) = field_types.insert(field.name.clone(), data_type.clone())
                && previous != data_type
            {
                return Err(PlanError::new(format!(
                    "append field `{list_name}.{}` has conflicting canonical types",
                    field.name
                )));
            }
        }
    }
    Ok(field_types)
}

fn data_type_plan_for_value_ref(
    program: &TypedProgram,
    index: &ValueIndex,
    value_ref: &ValueRef,
) -> Option<DataTypePlan> {
    let value_type = plan_value_type_for_value_ref(program, index, value_ref)?;
    Some(match value_type {
        PlanValueType::Text => DataTypePlan::Text,
        PlanValueType::Number => DataTypePlan::Number,
        PlanValueType::Byte => DataTypePlan::Byte,
        PlanValueType::Bool => DataTypePlan::Bool,
        PlanValueType::Bytes { fixed_len } => DataTypePlan::Bytes { fixed_len },
        PlanValueType::Enum => DataTypePlan::Variant {
            variants: Vec::new(),
        },
        PlanValueType::Data => return None,
        PlanValueType::RootInitialField
        | PlanValueType::RowInitialField
        | PlanValueType::Unknown => return None,
    })
}

fn plan_value_type_for_value_ref(
    program: &TypedProgram,
    index: &ValueIndex,
    value_ref: &ValueRef,
) -> Option<PlanValueType> {
    Some(match value_ref {
        ValueRef::Field(field) => *index.field_value_type(*field)?,
        ValueRef::State(state) => {
            let path = program
                .state_cells
                .iter()
                .find(|candidate| plan_state_id(candidate.id) == *state)?
                .path
                .as_str();
            *index.state_value_type(path)?
        }
        ValueRef::Source(_) => PlanValueType::Bool,
        ValueRef::SourcePayload { source_id, field } => {
            let source = program
                .sources
                .iter()
                .find(|source| plan_source_id(source.id) == *source_id)?;
            let typed = source
                .payload_schema
                .typed_fields
                .iter()
                .find(|descriptor| source_payload_field_from_ir(&descriptor.field) == *field)
                .map(|descriptor| {
                    plan_value_type_from_semantic_data_type(&semantic_data_type_plan(
                        &descriptor.data_type,
                    ))
                });
            match typed {
                Some(PlanValueType::Unknown) => return None,
                Some(value_type) => value_type,
                None => match field {
                    SourcePayloadField::Bytes => PlanValueType::Bytes { fixed_len: None },
                    SourcePayloadField::Key => PlanValueType::Number,
                    SourcePayloadField::Address
                    | SourcePayloadField::Named(_)
                    | SourcePayloadField::Text => PlanValueType::Text,
                },
            }
        }
        ValueRef::Constant(_) | ValueRef::List(_) => return None,
    })
}

fn migration_leaf_ref(
    program: &TypedProgram,
    source: &ir::MigrationSourceLeaf,
    indexed_list_owner: Option<&MigrationListOwnerPlan>,
    data_type: DataTypePlan,
) -> Result<MigrationLeafRefPlan, PlanError> {
    let memory = program
        .semantic_memory
        .get(source.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("migration source references missing semantic memory"))?;
    MigrationLeafRefPlan::new(
        indexed_list_owner.map_or(semantic_memory_id(memory), |owner| Ok(owner.memory_id))?,
        source.semantic_path.clone(),
        data_type,
    )
}

fn migration_indexed_list_owner(
    program: &TypedProgram,
    memory: &ir::SemanticMemory,
) -> Result<MigrationListOwnerPlan, PlanError> {
    let list_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::IndexedState {
            list_id: Some(list_id),
            ..
        } => list_id,
        _ => {
            return Err(PlanError::new(format!(
                "indexed migration authority `{}` has no owning list backing",
                memory.identity.semantic_path
            )));
        }
    };
    let list_memory = program
        .semantic_memory
        .iter()
        .find(|candidate| {
            matches!(
                candidate.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List {
                    list_id: candidate_list_id,
                    ..
                } if candidate_list_id == list_id
            )
        })
        .ok_or_else(|| {
            PlanError::new(format!(
                "indexed migration authority `{}` cannot resolve owning list {}",
                memory.identity.semantic_path, list_id.0
            ))
        })?;
    MigrationListOwnerPlan::new(
        semantic_memory_owner(list_memory),
        list_memory.identity.semantic_path.clone(),
    )
}

fn migration_input_data_type(
    program: &TypedProgram,
    sources: &[&ir::MigrationSourceLeaf],
    leaves: &[MigrationLeafRefPlan],
) -> Result<DataTypePlan, PlanError> {
    let first = sources
        .first()
        .ok_or_else(|| PlanError::new("migration input has no source leaves"))?;
    if sources
        .iter()
        .any(|source| source.memory_id != first.memory_id)
    {
        return Err(PlanError::new(
            "one DRAIN input cannot span multiple semantic memories",
        ));
    }
    if sources.len() == 1 {
        return Ok(leaves[0].data_type.clone());
    }
    let memory = program
        .semantic_memory
        .get(first.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("migration input references missing semantic memory"))?;
    Ok(semantic_data_type_plan(&memory.data_type))
}

fn durable_migration_source_list_plan(
    program: &TypedProgram,
    source: &ir::MigrationSourceLeaf,
    synthetic_initial_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<ListMemoryPlan, PlanError> {
    let memory = program
        .semantic_memory
        .get(source.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("migration source references missing semantic memory"))?;
    if memory.identity.kind != ir::SemanticMemoryKind::ListOwner {
        return Err(PlanError::new(format!(
            "migration source `{}` is not a list authority",
            memory.identity.semantic_path
        )));
    }
    let list_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::List { list_id, .. } => list_id,
        _ => unreachable!("list-owner memory must have list backing"),
    };
    let list = program
        .lists
        .iter()
        .find(|list| list.id == list_id)
        .ok_or_else(|| PlanError::new("migration source list backing is absent"))?;
    let catalog_slot =
        compiled_list_storage_slot(program, list, PlanStorageId(0), synthetic_initial_field_ids)?;
    let root_field_types = root_initial_field_value_types(program);
    let row_field_types = row_initial_field_value_types(program);
    let index = ValueIndex::new(program, &root_field_types, &row_field_types);
    semantic_list_memory_plan(
        program,
        memory,
        std::slice::from_ref(&catalog_slot),
        synthetic_initial_field_ids,
        &index,
        true,
    )
}

fn durable_migration_source_type(
    program: &TypedProgram,
    source: &ir::MigrationSourceLeaf,
    synthetic_initial_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<DataTypePlan, PlanError> {
    let memory = program
        .semantic_memory
        .get(source.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("migration source references missing semantic memory"))?;
    if memory.identity.kind == ir::SemanticMemoryKind::ListOwner {
        return Ok(durable_migration_source_list_plan(
            program,
            source,
            synthetic_initial_field_ids,
        )?
        .data_type);
    }
    Ok(semantic_data_type_plan(&source.data_type))
}

fn durable_migration_destination_type(
    edge: &ir::MigrationEdge,
    memory_id: MemoryId,
    memory: &[MemoryPlan],
    lists: &[ListMemoryPlan],
) -> Result<DataTypePlan, PlanError> {
    match edge.transfer_kind {
        ir::MigrationTransferKind::List => lists
            .iter()
            .find(|list| list.memory_id == memory_id)
            .map(|list| list.data_type.clone())
            .ok_or_else(|| {
                PlanError::new("migration destination list is absent from target schema")
            }),
        ir::MigrationTransferKind::Scalar | ir::MigrationTransferKind::IndexedField => {
            let target = memory
                .iter()
                .find(|target| target.memory_id == memory_id)
                .ok_or_else(|| {
                    PlanError::new("migration destination memory is absent from target schema")
                })?;
            if target.semantic_path == edge.destination.semantic_path {
                return Ok(target.data_type.clone());
            }
            target
                .leaves
                .iter()
                .find(|leaf| leaf.semantic_path == edge.destination.semantic_path)
                .map(|leaf| leaf.data_type.clone())
                .ok_or_else(|| {
                    PlanError::new("migration destination leaf is absent from target schema")
                })
        }
    }
}

fn migration_row_field_key(semantic_path: &str) -> &str {
    semantic_path
        .rsplit_once('.')
        .map_or(semantic_path, |(_, field)| field)
}

fn migration_row_fields_by_key(
    list: &ListMemoryPlan,
) -> Result<BTreeMap<String, &MemoryLeafPlan>, PlanError> {
    let mut fields = BTreeMap::new();
    for field in &list.row_fields {
        let key = migration_row_field_key(&field.semantic_path).to_owned();
        if fields.insert(key.clone(), field).is_some() {
            return Err(PlanError::new(format!(
                "whole-list migration row schema has duplicate durable field `{key}`"
            )));
        }
    }
    Ok(fields)
}

fn migration_list_row_fields(
    program: &TypedProgram,
    edge: &ir::MigrationEdge,
    destination_memory_id: MemoryId,
    target_lists: &[ListMemoryPlan],
    synthetic_initial_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<Vec<MigrationListRowFieldPlan>, PlanError> {
    if edge.transfer_kind != ir::MigrationTransferKind::List {
        return Ok(Vec::new());
    }
    if edge.transform != ir::MigrationTransform::Identity || edge.source_leaves.len() != 1 {
        return Err(PlanError::new(
            "whole-list migration must be one identity transfer",
        ));
    }
    let source = durable_migration_source_list_plan(
        program,
        &edge.source_leaves[0],
        synthetic_initial_field_ids,
    )?;
    let destination = target_lists
        .iter()
        .find(|list| list.memory_id == destination_memory_id)
        .ok_or_else(|| PlanError::new("migration destination list is absent from target schema"))?;
    if source.has_generation != destination.has_generation {
        return Err(PlanError::new(
            "whole-list identity migration changes hidden row identity schema",
        ));
    }

    let source_fields = migration_row_fields_by_key(&source)?;
    let destination_fields = migration_row_fields_by_key(destination)?;
    if destination_fields
        .keys()
        .any(|field| !source_fields.contains_key(field))
        || source_fields
            .keys()
            .any(|field| !destination_fields.contains_key(field) && !field.starts_with("$input$"))
    {
        return Err(PlanError::new(format!(
            "whole-list identity migration from `{}` to `{}` changes durable row fields (source={:?}, destination={:?}); migrate changed row fields explicitly",
            source.semantic_path,
            destination.semantic_path,
            source_fields.keys().collect::<Vec<_>>(),
            destination_fields.keys().collect::<Vec<_>>()
        )));
    }

    source_fields
        .into_iter()
        .map(|(key, source_field)| {
            let destination = destination_fields
                .get(&key)
                .map(|destination_field| {
                    if source_field.data_type != destination_field.data_type
                        || source_field.type_fingerprint != destination_field.type_fingerprint
                    {
                        return Err(PlanError::new(format!(
                            "whole-list identity migration changes durable row field `{key}` type"
                        )));
                    }
                    MigrationDestinationPlan::new(
                        destination.memory_id,
                        destination_field.semantic_path.clone(),
                        destination_field.data_type.clone(),
                    )
                })
                .transpose()?;
            Ok(MigrationListRowFieldPlan {
                source: MigrationLeafRefPlan::new(
                    source.memory_id,
                    source_field.semantic_path.clone(),
                    source_field.data_type.clone(),
                )?,
                destination,
            })
        })
        .collect()
}

type MigrationEnvironment = BTreeMap<String, MigrationExpressionPlan>;

struct MigrationExpressionLowerer<'a> {
    program: &'a TypedProgram,
    drain_inputs: BTreeMap<usize, MigrationInputId>,
    active_functions: Vec<String>,
}

impl MigrationExpressionLowerer<'_> {
    fn lower_pipeline(
        &mut self,
        pipeline: &[ir::ExprId],
    ) -> Result<MigrationExpressionPlan, PlanError> {
        let mut previous = None;
        let environment = MigrationEnvironment::new();
        for expr_id in pipeline {
            previous = Some(self.lower_expr(expr_id.as_usize(), previous, &environment)?);
        }
        previous.ok_or_else(|| PlanError::new("migration expression pipeline is empty"))
    }

    fn lower_expr(
        &mut self,
        expr_id: usize,
        pipeline_input: Option<MigrationExpressionPlan>,
        environment: &MigrationEnvironment,
    ) -> Result<MigrationExpressionPlan, PlanError> {
        let expr = self.program.expressions.get(expr_id).ok_or_else(|| {
            PlanError::new(format!(
                "migration recipe references missing expression {expr_id}"
            ))
        })?;
        match &expr.kind {
            AstExprKind::Drain { .. } => self
                .drain_inputs
                .get(&expr_id)
                .copied()
                .map(|input_id| MigrationExpressionPlan::Input { input_id })
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "migration expression {expr_id} references an unbound DRAIN input"
                    ))
                }),
            AstExprKind::Delimiter => pipeline_input.ok_or_else(|| {
                PlanError::new(format!(
                    "migration expression {expr_id} has a pipeline placeholder without input"
                ))
            }),
            AstExprKind::Identifier(name) => environment.get(name).cloned().ok_or_else(|| {
                PlanError::new(format!(
                    "migration expression reads unbound identifier `{name}`"
                ))
            }),
            AstExprKind::Path(parts) => {
                let (root, fields) = parts
                    .split_first()
                    .ok_or_else(|| PlanError::new("migration expression contains an empty path"))?;
                let input = environment.get(root).cloned().ok_or_else(|| {
                    PlanError::new(format!(
                        "migration expression reads unbound path `{}`",
                        parts.join(".")
                    ))
                })?;
                if fields.is_empty() {
                    Ok(input)
                } else {
                    Ok(MigrationExpressionPlan::Project {
                        input: Box::new(input),
                        fields: fields.to_vec(),
                    })
                }
            }
            AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
                Ok(MigrationExpressionPlan::Text {
                    value: value.clone(),
                })
            }
            AstExprKind::Number(value) => Ok(MigrationExpressionPlan::Number {
                value: value.parse::<FiniteReal>().map_err(|error| {
                    PlanError::new(format!(
                        "migration numeric literal `{value}` is not a finite canonical Number: {error}"
                    ))
                })?,
            }),
            AstExprKind::ByteLiteral { value, .. } => {
                Ok(MigrationExpressionPlan::Byte { value: *value })
            }
            AstExprKind::Bool(value) => Ok(MigrationExpressionPlan::Bool { value: *value }),
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) => {
                Ok(MigrationExpressionPlan::Variant { tag: tag.clone() })
            }
            AstExprKind::TaggedObject { tag, fields } => Ok(MigrationExpressionPlan::Tagged {
                tag: tag.clone(),
                fields: self.lower_fields(fields, environment)?,
            }),
            AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
                Ok(MigrationExpressionPlan::Record {
                    fields: self.lower_fields(fields, environment)?,
                })
            }
            AstExprKind::ListLiteral { items, .. } => Ok(MigrationExpressionPlan::List {
                items: items
                    .iter()
                    .map(|item| self.lower_expr(*item, None, environment))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            AstExprKind::BytesLiteral { items, .. } => Ok(MigrationExpressionPlan::Bytes {
                items: items
                    .iter()
                    .map(|item| self.lower_expr(*item, None, environment))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            AstExprKind::Infix { left, op, right } => Ok(MigrationExpressionPlan::Infix {
                operator: op.clone(),
                left: Box::new(self.lower_expr(*left, None, environment)?),
                right: Box::new(self.lower_expr(*right, None, environment)?),
            }),
            AstExprKind::Call { function, args } => {
                self.lower_call(function, None, args, environment)
            }
            AstExprKind::Pipe { input, op, args } => {
                let input = self.lower_expr(*input, pipeline_input, environment)?;
                self.lower_call(op, Some(input), args, environment)
            }
            AstExprKind::When { input } => {
                let input = self.lower_expr(*input, pipeline_input, environment)?;
                let arms = self.lower_match_arms(expr_id, environment)?;
                Ok(MigrationExpressionPlan::Match {
                    input: Box::new(input),
                    arms,
                })
            }
            AstExprKind::Source
            | AstExprKind::Draining { .. }
            | AstExprKind::Hold { .. }
            | AstExprKind::Latest
            | AstExprKind::Then { .. }
            | AstExprKind::MatchArm { .. }
            | AstExprKind::Unknown(_) => Err(PlanError::new(format!(
                "expression {expr_id} is not legal in a target-neutral migration recipe"
            ))),
        }
    }

    fn lower_fields(
        &mut self,
        fields: &[boon_parser::AstRecordField],
        environment: &MigrationEnvironment,
    ) -> Result<Vec<MigrationObjectFieldPlan>, PlanError> {
        fields
            .iter()
            .map(|field| {
                if field.spread {
                    return Err(PlanError::new(
                        "migration record spread is not a closed target-neutral recipe",
                    ));
                }
                Ok(MigrationObjectFieldPlan {
                    name: field.name.clone(),
                    value: self.lower_expr(field.value, None, environment)?,
                })
            })
            .collect()
    }

    fn lower_call(
        &mut self,
        function: &str,
        input: Option<MigrationExpressionPlan>,
        args: &[AstCallArg],
        environment: &MigrationEnvironment,
    ) -> Result<MigrationExpressionPlan, PlanError> {
        if let Some(definition) = self
            .program
            .functions
            .iter()
            .find(|definition| definition.name == function)
        {
            return self.inline_function(definition, input, args, environment);
        }
        if !migration_call_is_supported(function) {
            return Err(PlanError::new(format!(
                "pure migration call `{function}` is outside the target-neutral recipe VM"
            )));
        }

        let binding = matches!(function, "List/map" | "List/retain")
            .then(|| args.first())
            .flatten()
            .filter(|argument| argument.name.is_none())
            .and_then(|argument| self.program.expressions.get(argument.value))
            .and_then(|expr| match &expr.kind {
                AstExprKind::Identifier(name) => Some(name.clone()),
                _ => None,
            });
        let mut arguments = Vec::new();
        for (index, argument) in args.iter().enumerate() {
            if index == 0 && binding.is_some() && argument.name.is_none() {
                continue;
            }
            let value = if let Some(binding) = &binding {
                let mut lambda_environment = environment
                    .iter()
                    .map(|(name, value)| (name.clone(), shift_migration_parameters(value, 1)))
                    .collect::<MigrationEnvironment>();
                lambda_environment.insert(
                    binding.clone(),
                    MigrationExpressionPlan::Parameter { index: 0 },
                );
                MigrationArgumentValuePlan::Lambda {
                    parameter_count: 1,
                    body: Box::new(self.lower_expr(argument.value, None, &lambda_environment)?),
                }
            } else {
                MigrationArgumentValuePlan::Expression {
                    value: Box::new(self.lower_expr(argument.value, None, environment)?),
                }
            };
            arguments.push(MigrationCallArgumentPlan {
                name: argument.name.clone(),
                value,
            });
        }
        Ok(MigrationExpressionPlan::Call {
            function: function.to_owned(),
            input: input.map(Box::new),
            arguments,
        })
    }

    fn inline_function(
        &mut self,
        definition: &ir::FunctionDefinition,
        input: Option<MigrationExpressionPlan>,
        args: &[AstCallArg],
        environment: &MigrationEnvironment,
    ) -> Result<MigrationExpressionPlan, PlanError> {
        if self
            .active_functions
            .iter()
            .any(|active| active == &definition.name)
        {
            return Err(PlanError::new(format!(
                "recursive migration function `{}` cannot be canonicalized",
                definition.name
            )));
        }
        let mut values = BTreeMap::<String, MigrationExpressionPlan>::new();
        let mut positional = args.iter().filter(|argument| argument.name.is_none());
        if let Some(input) = input {
            let first = definition.args.first().ok_or_else(|| {
                PlanError::new(format!(
                    "piped migration function `{}` has no input parameter",
                    definition.name
                ))
            })?;
            values.insert(first.clone(), input);
        }
        for parameter in definition.args.iter().skip(values.len()) {
            let argument = args
                .iter()
                .find(|argument| argument.name.as_deref() == Some(parameter.as_str()))
                .or_else(|| positional.next())
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "migration function `{}` is missing argument `{parameter}`",
                        definition.name
                    ))
                })?;
            values.insert(
                parameter.clone(),
                self.lower_expr(argument.value, None, environment)?,
            );
        }
        if values.len() != definition.args.len() {
            return Err(PlanError::new(format!(
                "migration function `{}` arguments cannot be bound canonically",
                definition.name
            )));
        }
        let result_expr = function_result_expr(&definition.statement).ok_or_else(|| {
            PlanError::new(format!(
                "migration function `{}` has no expression result",
                definition.name
            ))
        })?;
        self.active_functions.push(definition.name.clone());
        let result = self.lower_expr(result_expr, None, &values);
        self.active_functions.pop();
        result
    }

    fn lower_match_arms(
        &mut self,
        when_expr_id: usize,
        environment: &MigrationEnvironment,
    ) -> Result<Vec<MigrationMatchArmPlan>, PlanError> {
        let arm_ids = statement_for_expression(self.program, when_expr_id)
            .map(match_arm_ids_from_statement)
            .filter(|arms| !arms.is_empty())
            .unwrap_or_else(|| fallback_match_arm_ids(self.program, when_expr_id));
        if arm_ids.is_empty() {
            return Err(PlanError::new(format!(
                "migration WHEN expression {when_expr_id} has no canonical MatchArm children"
            )));
        }
        arm_ids
            .into_iter()
            .map(|arm_id| {
                let arm = self.program.expressions.get(arm_id).ok_or_else(|| {
                    PlanError::new("migration match arm references a missing expression")
                })?;
                let AstExprKind::MatchArm { pattern, output } = &arm.kind else {
                    return Err(PlanError::new(
                        "migration match child is not a MatchArm expression",
                    ));
                };
                let output = output
                    .ok_or_else(|| PlanError::new("migration match arm must produce a value"))?;
                Ok(MigrationMatchArmPlan {
                    pattern: pattern.clone(),
                    output: self.lower_expr(output, None, environment)?,
                })
            })
            .collect()
    }
}

fn shift_migration_parameters(
    expression: &MigrationExpressionPlan,
    amount: u16,
) -> MigrationExpressionPlan {
    let mut shifted = expression.clone();
    shift_migration_parameters_in_place(&mut shifted, amount);
    shifted
}

fn shift_migration_parameters_in_place(expression: &mut MigrationExpressionPlan, amount: u16) {
    match expression {
        MigrationExpressionPlan::Parameter { index } => *index += amount,
        MigrationExpressionPlan::Tagged { fields, .. }
        | MigrationExpressionPlan::Record { fields } => {
            for field in fields {
                shift_migration_parameters_in_place(&mut field.value, amount);
            }
        }
        MigrationExpressionPlan::Project { input, .. } => {
            shift_migration_parameters_in_place(input, amount)
        }
        MigrationExpressionPlan::Call {
            input, arguments, ..
        } => {
            if let Some(input) = input {
                shift_migration_parameters_in_place(input, amount);
            }
            for argument in arguments {
                match &mut argument.value {
                    MigrationArgumentValuePlan::Expression { value } => {
                        shift_migration_parameters_in_place(value, amount)
                    }
                    MigrationArgumentValuePlan::Lambda { body, .. } => {
                        shift_migration_parameters_in_place(body, amount)
                    }
                }
            }
        }
        MigrationExpressionPlan::Infix { left, right, .. } => {
            shift_migration_parameters_in_place(left, amount);
            shift_migration_parameters_in_place(right, amount);
        }
        MigrationExpressionPlan::List { items } | MigrationExpressionPlan::Bytes { items } => {
            for item in items {
                shift_migration_parameters_in_place(item, amount);
            }
        }
        MigrationExpressionPlan::Match { input, arms } => {
            shift_migration_parameters_in_place(input, amount);
            for arm in arms {
                shift_migration_parameters_in_place(&mut arm.output, amount);
            }
        }
        MigrationExpressionPlan::Input { .. }
        | MigrationExpressionPlan::Text { .. }
        | MigrationExpressionPlan::Number { .. }
        | MigrationExpressionPlan::Byte { .. }
        | MigrationExpressionPlan::Bool { .. }
        | MigrationExpressionPlan::Variant { .. } => {}
    }
}

fn function_result_expr(statement: &AstStatement) -> Option<usize> {
    statement.expr.or_else(|| {
        statement
            .children
            .iter()
            .find_map(|child| child.expr.or_else(|| function_result_expr(child)))
    })
}

fn statement_for_expression(program: &TypedProgram, expr_id: usize) -> Option<&AstStatement> {
    program
        .functions
        .iter()
        .find_map(|function| statement_with_expression(&function.statement, expr_id))
        .or_else(|| {
            program
                .derived_values
                .iter()
                .find_map(|value| statement_with_expression(&value.statement, expr_id))
        })
        .or_else(|| {
            program
                .output_values
                .iter()
                .find_map(|value| statement_with_expression(&value.statement, expr_id))
        })
}

fn statement_with_expression(statement: &AstStatement, expr_id: usize) -> Option<&AstStatement> {
    if statement.expr == Some(expr_id) {
        return Some(statement);
    }
    statement
        .children
        .iter()
        .find_map(|child| statement_with_expression(child, expr_id))
}

fn match_arm_ids_from_statement(statement: &AstStatement) -> Vec<usize> {
    statement
        .children
        .iter()
        .filter_map(|child| child.expr)
        .collect()
}

fn fallback_match_arm_ids(program: &TypedProgram, when_expr_id: usize) -> Vec<usize> {
    let mut arms = Vec::new();
    for expression in program.expressions.iter().skip(when_expr_id + 1) {
        match expression.kind {
            AstExprKind::Delimiter | AstExprKind::Hold { .. } | AstExprKind::Draining { .. }
                if !arms.is_empty() =>
            {
                break;
            }
            AstExprKind::MatchArm { .. } => arms.push(expression.id),
            _ => {}
        }
    }
    arms
}

fn migration_recipe(
    program: &TypedProgram,
    target_memory: &[MemoryPlan],
    target_lists: &[ListMemoryPlan],
    synthetic_initial_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<Option<MigrationRecipePlan>, PlanError> {
    if program.migration_edges.is_empty() {
        return Ok(None);
    }
    let mut transfers = Vec::with_capacity(program.migration_edges.len());
    for edge in &program.migration_edges {
        let destination_memory = program
            .semantic_memory
            .get(edge.destination.memory_id.as_usize())
            .ok_or_else(|| {
                PlanError::new("migration destination references missing semantic memory")
            })?;
        if !semantic_memory_is_active(destination_memory) {
            return Err(PlanError::new(format!(
                "migration destination `{}` is not active target authority",
                destination_memory.identity.semantic_path
            )));
        }
        let indexed_list_owner = if edge.transfer_kind == ir::MigrationTransferKind::IndexedField {
            let owner = migration_indexed_list_owner(program, destination_memory)?;
            for source in &edge.source_leaves {
                let source_memory = program
                    .semantic_memory
                    .get(source.memory_id.as_usize())
                    .ok_or_else(|| {
                        PlanError::new(
                            "indexed migration source references missing semantic memory",
                        )
                    })?;
                if migration_indexed_list_owner(program, source_memory)? != owner {
                    return Err(PlanError::new(format!(
                        "indexed migration `{}` crosses stable list owners",
                        edge.destination.semantic_path
                    )));
                }
            }
            Some(owner)
        } else {
            None
        };
        let mut grouped_sources = BTreeMap::<usize, Vec<&ir::MigrationSourceLeaf>>::new();
        for source in &edge.source_leaves {
            grouped_sources
                .entry(source.drain_expr_id.as_usize())
                .or_default()
                .push(source);
        }
        let mut drain_inputs = BTreeMap::new();
        let mut inputs = Vec::with_capacity(grouped_sources.len());
        for (drain_expr_id, sources) in grouped_sources {
            let leaves = sources
                .iter()
                .map(|source| {
                    migration_leaf_ref(
                        program,
                        source,
                        indexed_list_owner.as_ref(),
                        durable_migration_source_type(
                            program,
                            source,
                            synthetic_initial_field_ids,
                        )?,
                    )
                })
                .collect::<Result<Vec<_>, PlanError>>()?;
            let input = MigrationInputPlan::new(
                leaves.clone(),
                migration_input_data_type(program, &sources, &leaves)?,
            )?;
            drain_inputs.insert(drain_expr_id, input.input_id);
            inputs.push(input);
        }
        let transform = match &edge.transform {
            ir::MigrationTransform::Identity => {
                let input_id = inputs
                    .first()
                    .filter(|_| inputs.len() == 1)
                    .map(|input| input.input_id)
                    .ok_or_else(|| {
                        PlanError::new("identity migration must have exactly one DRAIN input")
                    })?;
                MigrationTransformPlan::Identity { input_id }
            }
            ir::MigrationTransform::PureExpression { pipeline, .. } => {
                let mut lowerer = MigrationExpressionLowerer {
                    program,
                    drain_inputs,
                    active_functions: Vec::new(),
                };
                MigrationTransformPlan::Expression {
                    root: lowerer.lower_pipeline(pipeline)?,
                }
            }
        };
        let semantic_destination_memory_id = semantic_memory_id(destination_memory)?;
        let list_row_fields = migration_list_row_fields(
            program,
            edge,
            semantic_destination_memory_id,
            target_lists,
            synthetic_initial_field_ids,
        )?;
        let destination_memory_id = indexed_list_owner
            .as_ref()
            .map_or(semantic_destination_memory_id, |owner| owner.memory_id);
        transfers.push(MigrationTransferPlan {
            transfer_kind: match edge.transfer_kind {
                ir::MigrationTransferKind::Scalar => MigrationTransferKindPlan::Scalar,
                ir::MigrationTransferKind::List => MigrationTransferKindPlan::List,
                ir::MigrationTransferKind::IndexedField => {
                    MigrationTransferKindPlan::IndexedRowField
                }
            },
            indexed_list_owner,
            list_row_fields,
            inputs,
            destination: MigrationDestinationPlan::new(
                destination_memory_id,
                edge.destination.semantic_path.clone(),
                durable_migration_destination_type(
                    edge,
                    semantic_destination_memory_id,
                    target_memory,
                    target_lists,
                )?,
            )?,
            transform,
        });
    }
    Ok(Some(MigrationRecipePlan::new(transfers)?))
}

fn validate_predecessor_binding(
    application: &ApplicationPlan,
    target_schema_version: u64,
    predecessor: &MigrationPredecessorBinding,
) -> Result<(), PlanError> {
    let canonical_application = ApplicationPlan::new(predecessor.application.identity.clone())?;
    if predecessor.application != canonical_application {
        return Err(PlanError::new(
            "migration predecessor application identity hash is invalid",
        ));
    }
    if predecessor.application.identity != application.identity {
        return Err(PlanError::new(
            "migration predecessor belongs to a different application identity",
        ));
    }
    predecessor
        .persistence
        .validate_for_application(&predecessor.application)?;
    if predecessor.persistence.schema_version >= target_schema_version {
        return Err(PlanError::new(format!(
            "migration predecessor schema version {} must precede target version {target_schema_version}",
            predecessor.persistence.schema_version
        )));
    }
    Ok(())
}

fn memory_kind_at_semantic_path(
    memory: &[MemoryPlan],
    lists: &[ListMemoryPlan],
    owner: &MemoryOwnerPath,
    semantic_path: &str,
) -> Option<MemoryKind> {
    memory
        .iter()
        .find(|candidate| candidate.owner == *owner && candidate.semantic_path == semantic_path)
        .map(|candidate| candidate.kind)
        .or_else(|| {
            lists
                .iter()
                .find(|candidate| {
                    candidate.owner == *owner && candidate.semantic_path == semantic_path
                })
                .map(|_| MemoryKind::List)
        })
}

fn prove_compatible_without_drain(
    predecessor: &PersistencePlan,
    target_memory: &[MemoryPlan],
    target_lists: &[ListMemoryPlan],
) -> Result<(), PlanError> {
    for source in &predecessor.memory {
        if let Some(target_kind) = memory_kind_at_semantic_path(
            target_memory,
            target_lists,
            &source.owner,
            &source.semantic_path,
        ) && target_kind != source.kind
        {
            return Err(PlanError::new(format!(
                "persistent memory `{}` changes kind without DRAIN",
                source.semantic_path
            )));
        }
        let Some(target) = target_memory
            .iter()
            .find(|target| target.memory_id == source.memory_id)
        else {
            continue;
        };
        if target.kind != source.kind
            || target.owner != source.owner
            || target.semantic_path != source.semantic_path
            || target.type_fingerprint != source.type_fingerprint
            || target.data_type != source.data_type
        {
            return Err(PlanError::new(format!(
                "persistent memory `{}` changes type or identity without DRAIN",
                source.semantic_path
            )));
        }
        for source_leaf in &source.leaves {
            if let Some(target_leaf) = target
                .leaves
                .iter()
                .find(|target_leaf| target_leaf.leaf_id == source_leaf.leaf_id)
                && (target_leaf.semantic_path != source_leaf.semantic_path
                    || target_leaf.type_fingerprint != source_leaf.type_fingerprint
                    || target_leaf.data_type != source_leaf.data_type)
            {
                return Err(PlanError::new(format!(
                    "persistent leaf `{}` changes type without DRAIN",
                    source_leaf.semantic_path
                )));
            }
        }
    }

    for source in &predecessor.lists {
        if let Some(target_kind) = memory_kind_at_semantic_path(
            target_memory,
            target_lists,
            &source.owner,
            &source.semantic_path,
        ) && target_kind != MemoryKind::List
        {
            return Err(PlanError::new(format!(
                "persistent list `{}` changes kind without DRAIN",
                source.semantic_path
            )));
        }
        let Some(target) = target_lists
            .iter()
            .find(|target| target.memory_id == source.memory_id)
        else {
            continue;
        };
        if target.owner != source.owner
            || target.semantic_path != source.semantic_path
            || target.hidden_key_type != source.hidden_key_type
            || target.has_generation != source.has_generation
        {
            return Err(PlanError::new(format!(
                "persistent list `{}` changes row identity without DRAIN",
                source.semantic_path
            )));
        }
        if source.row_fields.is_empty()
            && target.row_fields.is_empty()
            && (target.type_fingerprint != source.type_fingerprint
                || target.data_type != source.data_type)
        {
            return Err(PlanError::new(format!(
                "persistent list `{}` changes item type without DRAIN",
                source.semantic_path
            )));
        }
        for source_leaf in &source.row_fields {
            if let Some(target_leaf) = target
                .row_fields
                .iter()
                .find(|target_leaf| target_leaf.leaf_id == source_leaf.leaf_id)
                && (target_leaf.semantic_path != source_leaf.semantic_path
                    || target_leaf.type_fingerprint != source_leaf.type_fingerprint
                    || target_leaf.data_type != source_leaf.data_type)
            {
                return Err(PlanError::new(format!(
                    "persistent row field `{}` changes type without DRAIN",
                    source_leaf.semantic_path
                )));
            }
        }
    }
    Ok(())
}

fn source_contains_migration_leaf(
    predecessor: &PersistencePlan,
    leaf: &MigrationLeafRefPlan,
) -> bool {
    predecessor.memory.iter().any(|memory| {
        memory.memory_id == leaf.memory_id
            && memory.leaves.iter().any(|candidate| {
                candidate.leaf_id == leaf.leaf_id
                    && candidate.semantic_path == leaf.semantic_path
                    && candidate.type_fingerprint == leaf.type_fingerprint
                    && candidate.data_type == leaf.data_type
            })
    }) || predecessor.lists.iter().any(|list| {
        list.memory_id == leaf.memory_id
            && ((MemoryLeafId::from_memory_path(list.memory_id, &list.semantic_path).is_ok_and(
                |leaf_id| {
                    leaf_id == leaf.leaf_id
                        && list.semantic_path == leaf.semantic_path
                        && list.type_fingerprint == leaf.type_fingerprint
                        && list.data_type == leaf.data_type
                },
            )) || list.row_fields.iter().any(|candidate| {
                candidate.leaf_id == leaf.leaf_id
                    && candidate.semantic_path == leaf.semantic_path
                    && candidate.type_fingerprint == leaf.type_fingerprint
                    && candidate.data_type == leaf.data_type
            }))
    })
}

fn migration_source_candidates(
    predecessor: &PersistencePlan,
    leaf: &MigrationLeafRefPlan,
) -> Vec<String> {
    predecessor
        .memory
        .iter()
        .flat_map(|memory| &memory.leaves)
        .chain(predecessor.lists.iter().flat_map(|list| &list.row_fields))
        .filter(|candidate| candidate.semantic_path == leaf.semantic_path)
        .map(|candidate| {
            format!(
                "leaf_id_match={}, type={:?}",
                candidate.leaf_id == leaf.leaf_id,
                candidate.data_type
            )
        })
        .chain(
            predecessor
                .lists
                .iter()
                .filter(|list| list.semantic_path == leaf.semantic_path)
                .map(|list| {
                    format!(
                        "list_memory_id_match={}, type={:?}",
                        list.memory_id == leaf.memory_id,
                        list.data_type
                    )
                }),
        )
        .collect()
}

fn contains_migration_list_owner(lists: &[ListMemoryPlan], owner: &MigrationListOwnerPlan) -> bool {
    lists.iter().any(|list| {
        list.memory_id == owner.memory_id
            && list.semantic_path == owner.semantic_path
            && list.owner == owner.owner
    })
}

fn prove_recipe_sources_exist(
    predecessor: &PersistencePlan,
    recipe: &MigrationRecipePlan,
) -> Result<(), PlanError> {
    for transfer in &recipe.transfers {
        if let Some(owner) = &transfer.indexed_list_owner
            && !contains_migration_list_owner(&predecessor.lists, owner)
        {
            return Err(PlanError::new(format!(
                "indexed migration list owner `{}` is absent in predecessor schema {}",
                owner.semantic_path, predecessor.schema_version
            )));
        }
        for leaf in transfer.inputs.iter().flat_map(|input| &input.leaves) {
            if !source_contains_migration_leaf(predecessor, leaf) {
                let candidates = migration_source_candidates(predecessor, leaf);
                return Err(PlanError::new(format!(
                    "migration source `{}` is absent or has a different type in predecessor schema {}; expected {:?}, candidates: {}",
                    leaf.semantic_path,
                    predecessor.schema_version,
                    leaf.data_type,
                    if candidates.is_empty() {
                        "none".to_owned()
                    } else {
                        candidates.join("; ")
                    }
                )));
            }
        }
    }
    Ok(())
}

fn prove_recipe_destinations_exist(
    memory: &[MemoryPlan],
    lists: &[ListMemoryPlan],
    recipe: &MigrationRecipePlan,
) -> Result<(), PlanError> {
    for transfer in &recipe.transfers {
        if let Some(owner) = &transfer.indexed_list_owner
            && !contains_migration_list_owner(lists, owner)
        {
            return Err(PlanError::new(format!(
                "indexed migration list owner `{}` is absent in target schema",
                owner.semantic_path
            )));
        }
        let destination = &transfer.destination;
        let present = match transfer.transfer_kind {
            MigrationTransferKindPlan::Scalar => memory.iter().any(|candidate| {
                candidate.memory_id == destination.memory_id
                    && candidate.kind == MemoryKind::Scalar
                    && ((candidate.semantic_path == destination.semantic_path
                        && candidate.type_fingerprint == destination.type_fingerprint
                        && candidate.data_type == destination.data_type)
                        || candidate.leaves.iter().any(|leaf| {
                            leaf.leaf_id == destination.leaf_id
                                && leaf.semantic_path == destination.semantic_path
                                && leaf.type_fingerprint == destination.type_fingerprint
                                && leaf.data_type == destination.data_type
                        }))
            }),
            MigrationTransferKindPlan::IndexedRowField => lists.iter().any(|list| {
                list.memory_id == destination.memory_id
                    && list.row_fields.iter().any(|leaf| {
                        leaf.leaf_id == destination.leaf_id
                            && leaf.semantic_path == destination.semantic_path
                            && leaf.type_fingerprint == destination.type_fingerprint
                            && leaf.data_type == destination.data_type
                    })
            }),
            MigrationTransferKindPlan::List => lists.iter().any(|candidate| {
                candidate.memory_id == destination.memory_id
                    && candidate.semantic_path == destination.semantic_path
                    && candidate.type_fingerprint == destination.type_fingerprint
                    && candidate.data_type == destination.data_type
            }),
        };
        if !present {
            return Err(PlanError::new(format!(
                "migration destination `{}` is absent or has a different type in target schema",
                destination.semantic_path
            )));
        }
    }
    Ok(())
}

fn merge_migration_catalog(
    predecessors: &[MigrationPredecessorBinding],
    current_recipe: Option<&MigrationRecipePlan>,
    target_schema_version: u64,
) -> Result<(Vec<MigrationRecipePlan>, Vec<MigrationEdgePlan>), PlanError> {
    let mut recipes = BTreeMap::<MigrationRecipeId, MigrationRecipePlan>::new();
    let mut edges = BTreeMap::<MigrationEdgeId, MigrationEdgePlan>::new();
    for predecessor in predecessors {
        for recipe in &predecessor.persistence.migration_recipes {
            if let Some(existing) = recipes.insert(recipe.migration_recipe_id, recipe.clone())
                && existing != *recipe
            {
                return Err(PlanError::new(
                    "predecessor catalogs disagree on migration recipe content",
                ));
            }
        }
        for edge in &predecessor.persistence.migration_edges {
            if let Some(existing) = edges.insert(edge.migration_edge_id, edge.clone())
                && existing != *edge
            {
                return Err(PlanError::new(
                    "predecessor catalogs disagree on migration edge content",
                ));
            }
        }
    }
    if let Some(recipe) = current_recipe {
        if let Some(existing) = recipes.insert(recipe.migration_recipe_id, recipe.clone())
            && existing != *recipe
        {
            return Err(PlanError::new(
                "current migration recipe ID conflicts with inherited content",
            ));
        }
        for predecessor in predecessors {
            let edge = MigrationEdgePlan::new(
                predecessor.source_schema_version(),
                target_schema_version,
                predecessor.source_schema_hash(),
                recipe.migration_recipe_id,
            )?;
            if let Some(existing) = edges.insert(edge.migration_edge_id, edge.clone())
                && existing != edge
            {
                return Err(PlanError::new(
                    "current predecessor binding conflicts with inherited edge content",
                ));
            }
        }
    }
    Ok((
        recipes.into_values().collect(),
        edges.into_values().collect(),
    ))
}

fn persistence_plan(
    program: &TypedProgram,
    application: &ApplicationPlan,
    schema_version: u64,
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    synthetic_initial_field_ids: &BTreeMap<(String, String), FieldId>,
    index: &ValueIndex,
    effect_outbox: Vec<EffectOutboxSchema>,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> Result<PersistencePlan, PlanError> {
    let mut memory = Vec::new();
    let mut lists = Vec::new();
    for semantic_memory in program
        .semantic_memory
        .iter()
        .filter(|memory| semantic_memory_is_runtime_active(program, memory))
    {
        match semantic_memory.identity.kind {
            ir::SemanticMemoryKind::RootScalar | ir::SemanticMemoryKind::IndexedField => {
                memory.push(semantic_scalar_memory_plan(
                    program,
                    semantic_memory,
                    scalar_slots,
                )?);
            }
            ir::SemanticMemoryKind::ListOwner => lists.push(semantic_list_memory_plan(
                program,
                semantic_memory,
                list_slots,
                synthetic_initial_field_ids,
                index,
                false,
            )?),
        }
    }
    for predecessor in migration_predecessors {
        validate_predecessor_binding(application, schema_version, predecessor)?;
    }
    let explicit_recipe = migration_recipe(program, &memory, &lists, synthetic_initial_field_ids)?;
    if let Some(recipe) = &explicit_recipe {
        prove_recipe_destinations_exist(&memory, &lists, recipe)?;
        for predecessor in migration_predecessors {
            prove_recipe_sources_exist(&predecessor.persistence, recipe)?;
        }
    } else {
        for predecessor in migration_predecessors {
            prove_compatible_without_drain(&predecessor.persistence, &memory, &lists)?;
        }
    }
    let compatible_recipe = if explicit_recipe.is_none() && !migration_predecessors.is_empty() {
        Some(MigrationRecipePlan::new(Vec::new())?)
    } else {
        None
    };
    let current_recipe = explicit_recipe.as_ref().or(compatible_recipe.as_ref());
    let current_migration_recipe_id = current_recipe.map(|recipe| recipe.migration_recipe_id);
    let (migration_recipes, migration_edges) =
        merge_migration_catalog(migration_predecessors, current_recipe, schema_version)?;
    PersistencePlan::new_with_migrations_and_effect_outbox(
        application,
        schema_version,
        memory,
        lists,
        effect_outbox,
        migration_recipes,
        current_migration_recipe_id,
        migration_edges,
    )
}

pub fn compile_typed_program(
    program: &TypedProgram,
    target_profile: TargetProfile,
    application_identity: &ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> Result<MachinePlan, PlanError> {
    validate_number_literals(program)?;
    let effects = effect_contracts(program)?;
    let mut effect_outbox = effect_outbox_schemas(&effects)?;
    let row_initial_field_types = row_initial_field_value_types(program);
    let root_initial_field_types = root_initial_field_value_types(program);
    let expression_value_types = expression_value_type_lookup(program);
    let synthetic_initial_field_ids = synthetic_initial_list_field_ids(program);
    let index = ValueIndex::new(program, &root_initial_field_types, &row_initial_field_types);
    let mut next_op = 0usize;
    let mut unresolved_refs = BTreeSet::new();

    let source_routes = program
        .sources
        .iter()
        .enumerate()
        .map(|(route_id, source)| {
            Ok(SourceRoute {
                id: PlanSourceRouteId(route_id),
                source_id: plan_source_id(source.id),
                path: source.path.clone(),
                scoped: source.scoped,
                scope_id: plan_scope_id(source.scope_id),
                interval_ms: source.interval_ms,
                payload_schema: source_payload_schema_from_ir(program, source)?,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;

    let mut constants = Vec::new();
    let migration_storage_defaults = program
        .state_cells
        .iter()
        .map(|state| migration_storage_default(program, state))
        .collect::<Vec<_>>();
    let inferred_initial_constants = program
        .state_cells
        .iter()
        .map(|state| {
            initial_constant_value(&state.initial_value).or_else(|| {
                state.initial_expr_id.and_then(|expr_id| {
                    constant_initial_expression_value(program, expr_id.as_usize())
                })
            })
        })
        .collect::<Vec<_>>();
    let initial_constant_ids = program
        .state_cells
        .iter()
        .enumerate()
        .map(|(state_index, _state)| {
            inferred_initial_constants[state_index]
                .clone()
                .or_else(|| {
                    migration_storage_defaults[state_index]
                        .as_ref()
                        .and_then(|default| default.constant.clone())
                })
                .map(|value| push_plan_constant(&mut constants, value))
        })
        .collect::<Vec<_>>();
    let effective_initial_value_kinds = program
        .state_cells
        .iter()
        .enumerate()
        .map(|(state_index, state)| {
            migration_storage_defaults[state_index]
                .as_ref()
                .map_or_else(
                    || {
                        let kind = initial_value_kind_from_ir(&state.initial_value);
                        if kind == InitialValueKind::Unknown {
                            inferred_initial_constants[state_index]
                                .as_ref()
                                .map(initial_value_kind_from_constant)
                                .unwrap_or(kind)
                        } else {
                            kind
                        }
                    },
                    |default| default.initial_value_kind,
                )
        })
        .collect::<Vec<_>>();

    let mut scalar_slots = Vec::with_capacity(program.state_cells.len());
    for state in program
        .state_cells
        .iter()
        .filter(|state| state_has_active_semantic_memory(program, state))
    {
        let state_index = state.id.as_usize();
        let slot_id = scalar_slots.len();
        let initial_row_expression = if let Some(edge) = migration_storage_defaults[state_index]
            .as_ref()
            .and_then(|default| default.indexed_edge.as_ref())
        {
            Some(migration_indexed_default_expression(
                program,
                state,
                edge,
                &index,
                &synthetic_initial_field_ids,
                &mut constants,
            )?)
        } else {
            initial_row_expression(
                program,
                state,
                &index,
                &synthetic_initial_field_ids,
                &mut constants,
            )
        };
        scalar_slots.push(ScalarStorageSlot {
            id: PlanStorageId(slot_id),
            state_id: plan_state_id(state.id),
            value_type: migration_storage_defaults[state_index]
                .as_ref()
                .map_or_else(
                    || {
                        state_initial_value_type(
                            program,
                            state,
                            &root_initial_field_types,
                            &row_initial_field_types,
                            &expression_value_types,
                        )
                    },
                    |default| default.value_type,
                ),
            scope_id: plan_scope_id(state.scope_id),
            indexed: state.indexed,
            initial_value_kind: effective_initial_value_kinds[state_index],
            initial_constant_id: initial_constant_ids[state_index],
            initial_root_field_path: initial_root_field_path(&state.initial_value),
            initial_row_field_path: initial_row_field_path(&state.initial_value),
            initial_row_expression,
        });
    }

    let list_slot_offset = scalar_slots.len();
    let list_slots = program
        .lists
        .iter()
        .filter(|list| list_has_active_semantic_memory(program, list))
        .enumerate()
        .map(|(slot_index, list)| {
            compiled_list_storage_slot(
                program,
                list,
                PlanStorageId(list_slot_offset + slot_index),
                &synthetic_initial_field_ids,
            )
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let byte_bank_offset = scalar_slots.len() + list_slots.len();
    let byte_banks = scalar_slots
        .iter()
        .filter_map(|slot| match slot.value_type {
            PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            } => Some(ByteStorageBank {
                id: PlanStorageId(byte_bank_offset),
                state_storage_id: slot.id,
                state_id: slot.state_id,
                scope_id: slot.scope_id,
                indexed: slot.indexed,
                fixed_len,
                capacity: byte_bank_capacity_hint(slot, &list_slots),
            }),
            _ => None,
        })
        .enumerate()
        .map(|(bank_index, mut bank)| {
            bank.id = PlanStorageId(byte_bank_offset + bank_index);
            bank
        })
        .collect::<Vec<_>>();
    let byte_bank_storage_count = byte_banks.len();

    let source_ops = source_routes
        .iter()
        .map(|route| {
            op(
                &mut next_op,
                PlanOpKind::SourceRoute,
                Vec::new(),
                Some(ValueRef::Source(route.source_id)),
                false,
                0,
            )
        })
        .collect::<Vec<_>>();

    let state_ops = program
        .state_cells
        .iter()
        .filter(|state| state_has_active_semantic_memory(program, state))
        .map(|state| {
            let state_index = state.id.as_usize();
            op(
                &mut next_op,
                PlanOpKind::StateInitialize {
                    initial_value_kind: effective_initial_value_kinds[state_index],
                    initial_constant_id: initial_constant_ids[state_index],
                },
                Vec::new(),
                Some(ValueRef::State(plan_state_id(state.id))),
                state.indexed,
                0,
            )
        })
        .collect::<Vec<_>>();

    let mut derived_ops = Vec::new();
    for derived in &program.derived_values {
        let mut inputs = Vec::new();
        let unresolved = resolve_paths(&index, &derived.sources, &mut inputs, &mut unresolved_refs);
        let expression = derived_expression_for_value(
            program,
            derived,
            &index,
            &mut constants,
            &mut inputs,
            &mut unresolved_refs,
        );
        derived_ops.push(op(
            &mut next_op,
            PlanOpKind::DerivedValue {
                derived_kind: plan_derived_kind_from_ir(&derived.kind),
                startup_recompute: derived.startup_recompute,
                expression,
            },
            inputs,
            Some(derived_output_ref(program, derived)),
            derived.indexed,
            unresolved,
        ));
    }

    let mut update_ops = program
        .update_branches
        .iter()
        .map(|branch| {
            let mut inputs = Vec::new();
            let mut unresolved = 0usize;
            unresolved += resolve_path(&index, &branch.source, &mut inputs, &mut unresolved_refs);
            unresolved += collect_update_expression_refs(
                &index,
                &branch.source,
                &branch.target,
                branch.indexed,
                &branch.expression,
                &mut inputs,
                &mut unresolved_refs,
            );
            let source_guard = source_guard_for_update_guard(
                &index,
                &branch.source,
                branch.guard.as_ref(),
                &mut inputs,
                &mut unresolved_refs,
                &mut unresolved,
            );
            let output = index.resolve(&branch.target);
            if output.is_none() {
                unresolved += 1;
                unresolved_refs.insert(branch.target.clone());
            }
            let expression_kind = update_expression_kind_for_branch(
                &index,
                &branch.source,
                &branch.target,
                branch.indexed,
                &branch.expression,
            );
            let ordered_inputs = ordered_update_expression_inputs(
                &index,
                &mut constants,
                &branch.source,
                &branch.target,
                branch.indexed,
                &branch.expression,
            );
            let effect = effect_invocation_for_branch(
                branch,
                expression_kind,
                &ordered_inputs,
                output.clone(),
            )?;
            Ok(op(
                &mut next_op,
                PlanOpKind::UpdateBranch {
                    expression_kind,
                    ordered_inputs,
                    source_payload_field: source_payload_field_for_branch(
                        &index,
                        &branch.source,
                        &branch.target,
                        branch.indexed,
                        &branch.expression,
                    ),
                    update_constant_id: update_constant_id_for_expression(
                        &index,
                        &mut constants,
                        &branch.target,
                        &branch.expression,
                    ),
                    source_guard,
                    effect,
                },
                unique_value_refs(inputs),
                output,
                branch.indexed,
                unresolved,
            ))
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    for declaration in &program.host_effects {
        update_ops.push(host_effect_plan_op(
            program,
            declaration,
            &index,
            &mut constants,
            &mut next_op,
        )?);
    }

    let list_ops = program
        .list_operations
        .iter()
        .map(|list_operation| {
            let mut inputs = Vec::new();
            let mut unresolved = 0usize;
            let output = index.resolve(&list_operation.list);
            if output.is_none() {
                unresolved += 1;
                unresolved_refs.insert(list_operation.list.clone());
            }
            let mut append_plan = None;
            let mut remove_plan = None;
            let mut retain_plan = None;
            let mut count_plan = None;
            let operation_kind = match &list_operation.kind {
                ListOperationKind::Append { trigger, fields } => {
                    let trigger_ref = index.resolve(trigger);
                    if let Some(value_ref) = trigger_ref.clone() {
                        inputs.push(value_ref);
                    } else {
                        unresolved +=
                            resolve_path(&index, trigger, &mut inputs, &mut unresolved_refs);
                    }
                    let mut append_fields = Vec::new();
                    for field in fields {
                        match &field.value {
                            ListAppendFieldValue::Source { path } => {
                                let value_ref =
                                    list_append_value_ref(program, &index, trigger, path);
                                if let Some(value_ref) = value_ref.clone() {
                                    inputs.push(value_ref.clone());
                                } else {
                                    unresolved += 1;
                                    unresolved_refs.insert(path.clone());
                                }
                                append_fields.push(PlanListAppendField {
                                    name: field.name.clone(),
                                    field_id: storage_input_field_id(
                                        program,
                                        &list_operation.list,
                                        &field.name,
                                        &synthetic_initial_field_ids,
                                    ),
                                    value_ref,
                                    constant_id: None,
                                });
                            }
                            ListAppendFieldValue::Const { value } => {
                                let constant_id = append_constant_id(&mut constants, value);
                                append_fields.push(PlanListAppendField {
                                    name: field.name.clone(),
                                    field_id: storage_input_field_id(
                                        program,
                                        &list_operation.list,
                                        &field.name,
                                        &synthetic_initial_field_ids,
                                    ),
                                    value_ref: None,
                                    constant_id: Some(constant_id),
                                });
                            }
                            ListAppendFieldValue::TypedConst { value } => {
                                let Some(value) = initial_constant_value(value) else {
                                    unresolved += 1;
                                    unresolved_refs
                                        .insert(format!("{}.{}", list_operation.list, field.name));
                                    continue;
                                };
                                let constant_id = push_plan_constant(&mut constants, value);
                                append_fields.push(PlanListAppendField {
                                    name: field.name.clone(),
                                    field_id: storage_input_field_id(
                                        program,
                                        &list_operation.list,
                                        &field.name,
                                        &synthetic_initial_field_ids,
                                    ),
                                    value_ref: None,
                                    constant_id: Some(constant_id),
                                });
                            }
                        }
                    }
                    if let Some(trigger) = trigger_ref {
                        append_plan = Some(PlanListAppend {
                            trigger,
                            fields: append_fields,
                        });
                    }
                    PlanListOperationKind::Append
                }
                ListOperationKind::Remove { source, predicate } => {
                    let source_ref = index.resolve(source);
                    unresolved += resolve_path(&index, source, &mut inputs, &mut unresolved_refs);
                    if let Some(source_ref) = source_ref {
                        remove_plan = Some(PlanListRemove {
                            source: source_ref,
                            predicate: plan_list_remove_predicate(&index, predicate, &mut inputs),
                        });
                    }
                    PlanListOperationKind::Remove
                }
                ListOperationKind::Retain { target, predicate } => {
                    let target_ref = index.resolve(target);
                    unresolved += resolve_path(&index, target, &mut inputs, &mut unresolved_refs);
                    if let Some(target_ref) = target_ref {
                        retain_plan = Some(PlanListRetain {
                            target: target_ref,
                            predicate: plan_list_remove_predicate(&index, predicate, &mut inputs),
                        });
                    }
                    PlanListOperationKind::Retain
                }
                ListOperationKind::Count { target, predicate } => {
                    let target_ref = index.resolve(target);
                    unresolved += resolve_path(&index, target, &mut inputs, &mut unresolved_refs);
                    if let Some(target_ref) = target_ref {
                        count_plan = Some(PlanListCount {
                            target: target_ref,
                            predicate: plan_list_remove_predicate(&index, predicate, &mut inputs),
                        });
                    }
                    PlanListOperationKind::Count
                }
            };
            op(
                &mut next_op,
                PlanOpKind::ListOperation {
                    operation_kind,
                    append: append_plan,
                    remove: remove_plan,
                    retain: retain_plan,
                    count: count_plan,
                },
                unique_value_refs(inputs),
                output,
                true,
                unresolved,
            )
        })
        .collect::<Vec<_>>();

    let mut query_indexes = BTreeMap::<QueryIndexId, QueryIndexPlan>::new();
    let list_projection_ops = program
        .list_projections
        .iter()
        .map(|projection| {
            let mut inputs = Vec::new();
            let mut unresolved = 0usize;
            let source_ref = match index.resolve(&projection.list) {
                Some(value_ref) => {
                    inputs.push(value_ref.clone());
                    Some(value_ref)
                }
                None => {
                    unresolved += 1;
                    unresolved_refs.insert(projection.list.clone());
                    None
                }
            };
            let source_list = match source_ref {
                Some(ValueRef::List(list_id)) => Some(list_id),
                _ => None,
            };
            let output = index.resolve(&projection.target);
            if output.is_none() {
                unresolved += 1;
                unresolved_refs.insert(projection.target.clone());
            }
            let projection_plan = match (&projection.kind, source_ref.clone(), source_list) {
                (ListProjectionKind::Find { field, value }, _, Some(source_list)) => {
                    let value_ref = match index.resolve(value) {
                        Some(value_ref) => {
                            inputs.push(value_ref.clone());
                            Some(value_ref)
                        }
                        None => {
                            unresolved += 1;
                            unresolved_refs.insert(value.clone());
                            None
                        }
                    };
                    value_ref.map(|value| PlanListProjection::Find {
                        source_list,
                        field: field.clone(),
                        value,
                    })
                }
                (
                    ListProjectionKind::IndexedQuery {
                        fields,
                        selection,
                        residual,
                        limit: Some(limit),
                        cursor,
                        unique,
                        order,
                    },
                    _,
                    Some(source_list),
                ) if *limit > 0 && *limit <= boon_query::MAX_QUERY_LIMIT => {
                    let row_type = query_row_data_type(program, &index, source_list);
                    if row_type.is_none() || fields.is_empty() {
                        unresolved += 1;
                        unresolved_refs
                            .insert(format!("{}.List/query.index_projection", projection.target));
                    }
                    let row_schema_hash = row_type
                        .as_ref()
                        .and_then(|row_type| data_type_fingerprint(row_type).ok());
                    let mut query_fields = Vec::new();
                    if let Some(row_type) = row_type.as_ref() {
                        for field in fields {
                            let Some(root) = field.path.first() else {
                                unresolved += 1;
                                unresolved_refs
                                    .insert(format!("{}.List/query.fields", projection.target));
                                continue;
                            };
                            let Some(field_id) =
                                row_input_field_id_for_list_id(program, source_list, root)
                            else {
                                unresolved += 1;
                                unresolved_refs.insert(format!(
                                    "{}.{}",
                                    projection.list,
                                    field.path.join(".")
                                ));
                                continue;
                            };
                            let Some(key_type) =
                                query_key_type(row_type, &field.path, field.multi_value)
                            else {
                                unresolved += 1;
                                unresolved_refs.insert(format!(
                                    "{}.List/query.unsupported_key_type.{}",
                                    projection.target,
                                    field.path.join(".")
                                ));
                                continue;
                            };
                            let normalization = match &field.normalization {
                                ListTextNormalization::Exact => Some(QueryTextNormalization::Exact),
                                ListTextNormalization::TrimLowercase => {
                                    Some(QueryTextNormalization::TrimLowercase)
                                }
                                ListTextNormalization::Tokens => {
                                    Some(QueryTextNormalization::Tokens)
                                }
                                ListTextNormalization::Unknown { value } => {
                                    unresolved += 1;
                                    unresolved_refs.insert(format!(
                                        "{}.List/query.normalization.{value}",
                                        projection.target
                                    ));
                                    None
                                }
                            };
                            if normalization == Some(QueryTextNormalization::Tokens)
                                && key_type != QueryKeyType::Text
                            {
                                unresolved += 1;
                                unresolved_refs.insert(format!(
                                    "{}.List/query.tokens_require_text.{}",
                                    projection.target,
                                    field.path.join(".")
                                ));
                                continue;
                            }
                            if let Some(normalization) = normalization {
                                query_fields.push(QueryIndexFieldPlan {
                                    field: field_id,
                                    path: field.path.clone(),
                                    semantic_path: format!(
                                        "{}.{}",
                                        projection.list,
                                        field.path.join(".")
                                    ),
                                    key_type,
                                    normalization,
                                    multi_value: field.multi_value,
                                });
                            }
                        }
                    }
                    let order = match order {
                        ir::ListQueryOrder::Ascending => Some(QueryIndexOrder::Ascending),
                        ir::ListQueryOrder::Descending => Some(QueryIndexOrder::Descending),
                        ir::ListQueryOrder::Unknown { value } => {
                            unresolved += 1;
                            unresolved_refs
                                .insert(format!("{}.List/query.order.{value}", projection.target));
                            None
                        }
                    };
                    let selection = match selection {
                        ir::ListQuerySelection::Exact { key } => resolve_query_ref(
                            &index,
                            key,
                            &mut inputs,
                            &mut unresolved,
                            &mut unresolved_refs,
                        )
                        .map(|key| PlanQuerySelection::Exact { key }),
                        ir::ListQuerySelection::TextPrefix { leading, prefix } => {
                            let leading = leading.as_ref().and_then(|leading| {
                                resolve_query_ref(
                                    &index,
                                    leading,
                                    &mut inputs,
                                    &mut unresolved,
                                    &mut unresolved_refs,
                                )
                            });
                            resolve_query_ref(
                                &index,
                                prefix,
                                &mut inputs,
                                &mut unresolved,
                                &mut unresolved_refs,
                            )
                            .map(|prefix| PlanQuerySelection::TextPrefix { leading, prefix })
                        }
                        ir::ListQuerySelection::Range {
                            lower,
                            lower_inclusive,
                            upper,
                            upper_inclusive,
                        } => {
                            let lower = lower.as_ref().and_then(|lower| {
                                resolve_query_ref(
                                    &index,
                                    lower,
                                    &mut inputs,
                                    &mut unresolved,
                                    &mut unresolved_refs,
                                )
                            });
                            let upper = upper.as_ref().and_then(|upper| {
                                resolve_query_ref(
                                    &index,
                                    upper,
                                    &mut inputs,
                                    &mut unresolved,
                                    &mut unresolved_refs,
                                )
                            });
                            Some(PlanQuerySelection::Range {
                                lower,
                                lower_inclusive: *lower_inclusive,
                                upper,
                                upper_inclusive: *upper_inclusive,
                            })
                        }
                        ir::ListQuerySelection::Union { keys } => resolve_query_ref(
                            &index,
                            keys,
                            &mut inputs,
                            &mut unresolved,
                            &mut unresolved_refs,
                        )
                        .map(|keys| PlanQuerySelection::Union { keys }),
                        ir::ListQuerySelection::Intersection { keys } => resolve_query_ref(
                            &index,
                            keys,
                            &mut inputs,
                            &mut unresolved,
                            &mut unresolved_refs,
                        )
                        .map(|keys| PlanQuerySelection::Intersection { keys }),
                        ir::ListQuerySelection::Unknown { value } => {
                            unresolved += 1;
                            unresolved_refs
                                .insert(format!("{}.List/query.select.{value}", projection.target));
                            None
                        }
                    };
                    let residual = residual.as_ref().and_then(|residual| {
                        plan_query_residual(
                            residual,
                            &index,
                            &mut inputs,
                            &mut unresolved,
                            &mut unresolved_refs,
                        )
                    });
                    let cursor = cursor.as_ref().and_then(|cursor| {
                        resolve_query_ref(
                            &index,
                            cursor,
                            &mut inputs,
                            &mut unresolved,
                            &mut unresolved_refs,
                        )
                    });
                    match (
                        row_schema_hash,
                        (query_fields.len() == fields.len()).then_some(query_fields),
                        order,
                        selection,
                    ) {
                        (
                            Some(row_schema_hash),
                            Some(query_fields),
                            Some(order),
                            Some(selection),
                        ) => {
                            match QueryIndexPlan::new(
                                source_list,
                                projection.list.clone(),
                                row_schema_hash,
                                query_fields,
                                *unique,
                                order,
                            ) {
                                Ok(query_index) => {
                                    query_indexes
                                        .entry(query_index.id)
                                        .or_insert_with(|| query_index.clone());
                                    Some(PlanListProjection::IndexedQuery {
                                        index: query_index.id,
                                        source_list,
                                        selection,
                                        residual: residual.into_iter().collect(),
                                        limit: *limit,
                                        cursor,
                                    })
                                }
                                Err(error) => {
                                    unresolved += 1;
                                    unresolved_refs.insert(format!(
                                        "{}.List/query.index.{error}",
                                        projection.target
                                    ));
                                    None
                                }
                            }
                        }
                        _ => None,
                    }
                }
                (ListProjectionKind::IndexedQuery { limit, .. }, _, _) => {
                    unresolved += 1;
                    unresolved_refs.insert(format!(
                        "{}.List/query.{}",
                        projection.target,
                        if limit.is_some() {
                            "source_or_contract"
                        } else {
                            "limit"
                        }
                    ));
                    None
                }
                (
                    ListProjectionKind::TextPrefix {
                        field,
                        prefix,
                        limit: Some(limit),
                        normalization,
                    },
                    _,
                    Some(source_list),
                ) if *limit > 0 => {
                    let prefix_ref = match index.resolve(prefix) {
                        Some(value_ref) => {
                            inputs.push(value_ref.clone());
                            Some(value_ref)
                        }
                        None => {
                            unresolved += 1;
                            unresolved_refs.insert(prefix.clone());
                            None
                        }
                    };
                    let field_id = row_input_field_id_for_list_id(program, source_list, field);
                    if field_id.is_none() {
                        unresolved += 1;
                        unresolved_refs.insert(format!("{}.{}", projection.list, field));
                    }
                    let normalization = match normalization {
                        ListTextNormalization::Exact => Some(QueryTextNormalization::Exact),
                        ListTextNormalization::TrimLowercase => {
                            Some(QueryTextNormalization::TrimLowercase)
                        }
                        ListTextNormalization::Tokens => Some(QueryTextNormalization::Tokens),
                        ListTextNormalization::Unknown { value } => {
                            unresolved += 1;
                            unresolved_refs.insert(format!(
                                "{}.List/query_prefix.normalization.{value}",
                                projection.target
                            ));
                            None
                        }
                    };
                    match (prefix_ref, field_id, normalization) {
                        (Some(prefix), Some(field_id), Some(normalization)) => {
                            let row_type = query_row_data_type(program, &index, source_list);
                            let row_schema_hash = row_type
                                .as_ref()
                                .and_then(|row_type| data_type_fingerprint(row_type).ok());
                            let key_type = row_type.as_ref().and_then(|row_type| {
                                query_key_type(row_type, std::slice::from_ref(field), false)
                            });
                            let query_index = match (row_schema_hash, key_type) {
                                (Some(row_schema_hash), Some(key_type)) => QueryIndexPlan::new(
                                    source_list,
                                    projection.list.clone(),
                                    row_schema_hash,
                                    vec![QueryIndexFieldPlan {
                                        field: field_id,
                                        path: vec![field.clone()],
                                        semantic_path: format!("{}.{}", projection.list, field),
                                        key_type,
                                        normalization,
                                        multi_value: false,
                                    }],
                                    false,
                                    QueryIndexOrder::Ascending,
                                )
                                .ok(),
                                _ => None,
                            };
                            let query_index = match query_index {
                                Some(query_index) => query_index,
                                None => {
                                    unresolved += 1;
                                    unresolved_refs.insert(format!(
                                        "{}.List/query_prefix.index",
                                        projection.target
                                    ));
                                    return op(
                                        &mut next_op,
                                        PlanOpKind::ListProjection {
                                            projection: PlanListProjection::Unknown {
                                                summary: projection.target.clone(),
                                            },
                                        },
                                        unique_value_refs(inputs),
                                        output,
                                        true,
                                        unresolved,
                                    );
                                }
                            };
                            query_indexes
                                .entry(query_index.id)
                                .or_insert_with(|| query_index.clone());
                            Some(PlanListProjection::TextPrefix {
                                index: query_index.id,
                                source_list,
                                prefix,
                                limit: *limit,
                            })
                        }
                        _ => None,
                    }
                }
                (ListProjectionKind::TextPrefix { limit, .. }, _, _) => {
                    unresolved += 1;
                    unresolved_refs.insert(format!(
                        "{}.List/query_prefix.{}",
                        projection.target,
                        if limit.is_some() { "source" } else { "limit" }
                    ));
                    None
                }
                (
                    ListProjectionKind::Chunk {
                        size: Some(size),
                        item_field,
                        label_field,
                    },
                    Some(ValueRef::List(source_list)),
                    _,
                ) => Some(PlanListProjection::Chunk {
                    source_list,
                    size: *size,
                    item_field: item_field.clone(),
                    label_field: label_field.clone(),
                }),
                (
                    ListProjectionKind::Chunk {
                        size: Some(size),
                        item_field,
                        label_field,
                    },
                    Some(source),
                    _,
                ) => Some(PlanListProjection::ChunkValue {
                    source,
                    size: *size,
                    item_field: item_field.clone(),
                    label_field: label_field.clone(),
                }),
                (ListProjectionKind::Chunk { size: None, .. }, _, _) => {
                    unresolved += 1;
                    unresolved_refs.insert(format!("{}.List/chunk.size", projection.target));
                    None
                }
                _ => None,
            };
            op(
                &mut next_op,
                PlanOpKind::ListProjection {
                    projection: projection_plan.unwrap_or_else(|| PlanListProjection::Unknown {
                        summary: projection.target.clone(),
                    }),
                },
                unique_value_refs(inputs),
                output,
                true,
                unresolved,
            )
        })
        .collect::<Vec<_>>();

    let dependency_ops = program
        .dependencies
        .iter()
        .map(|dependency| {
            let mut inputs = Vec::new();
            let mut unresolved = 0usize;
            unresolved += resolve_path(&index, &dependency.from, &mut inputs, &mut unresolved_refs);
            let output = index.resolve(&dependency.to);
            if output.is_none() {
                unresolved += 1;
                unresolved_refs.insert(dependency.to.clone());
            }
            op(
                &mut next_op,
                PlanOpKind::DependencyEdge,
                unique_value_refs(inputs),
                output,
                dependency.indexed,
                unresolved,
            )
        })
        .collect::<Vec<_>>();

    let regions = vec![
        region(0, RegionKind::SourceRouting, source_ops),
        region(1, RegionKind::StateInitialization, state_ops),
        region(2, RegionKind::DerivedEvaluation, derived_ops),
        region(3, RegionKind::UpdateBranches, update_ops),
        region(4, RegionKind::ListOperations, list_ops),
        region(5, RegionKind::ListProjections, list_projection_ops),
        region(6, RegionKind::DependencyEdges, dependency_ops),
    ];
    let executable_fields = regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match (&op.output, &op.kind) {
            (
                Some(ValueRef::Field(field)),
                PlanOpKind::DerivedValue {
                    expression: Some(_),
                    ..
                },
            ) => Some(*field),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let document =
        super::document_plan_backend::compile_document_plan(program, &executable_fields)?;
    let outputs = output_root_plans(program, document.as_ref(), &index)?;
    let host_ports = host_port_plans(program, &outputs)?;
    let program_role = if document.is_none() && !outputs.is_empty() {
        boon_plan::ProgramRole::Server
    } else {
        boon_plan::ProgramRole::Document
    };

    let operation_count = regions.iter().map(|region| region.ops.len()).sum::<usize>();
    let unresolved_executable_ref_count = regions
        .iter()
        .flat_map(|region| &region.ops)
        .map(|op| op.unresolved_executable_ref_count)
        .sum::<usize>();
    let typed_value_ref_count = regions
        .iter()
        .flat_map(|region| &region.ops)
        .map(|op| op.inputs.len() + usize::from(op.output.is_some()))
        .sum::<usize>();
    let unknown_region_op_count = regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| is_unknown_op(op))
        .count();
    let unknown_storage_op_count = scalar_slots
        .iter()
        .filter(|slot| matches!(slot.initial_value_kind, InitialValueKind::Unknown))
        .count()
        + list_slots
            .iter()
            .filter(|slot| matches!(slot.initializer_kind, ListInitializerKind::Unknown))
            .count()
        + non_executable_constant_payload_count(&constants);
    let unknown_plan_op_count = unknown_region_op_count + unknown_storage_op_count;
    let graph_clones_per_item = program
        .lists
        .iter()
        .map(|list| list.graph_clones_per_item)
        .max()
        .unwrap_or_default();
    let constant_count = constants.len();
    let source_route_count = source_routes.len();
    let scalar_storage_count = scalar_slots.len();
    let list_storage_count = list_slots.len();
    let typed_lowering_executable =
        unresolved_executable_ref_count == 0 && unknown_plan_op_count == 0;
    let cpu_plan_executor_unsupported_op_count =
        cpu_plan_executor_unsupported_op_count(&regions, &list_slots, &scalar_slots, &constants);
    let cpu_plan_executor_complete =
        typed_lowering_executable && cpu_plan_executor_unsupported_op_count == 0;
    bind_effect_outbox_invocations(&mut effect_outbox, &regions)?;
    let application = ApplicationPlan::new(application_identity.clone())?;
    let persistence = persistence_plan(
        program,
        &application,
        schema_version,
        &scalar_slots,
        &list_slots,
        &synthetic_initial_field_ids,
        &index,
        effect_outbox,
        migration_predecessors,
    )?;
    let query_indexes = query_indexes.into_values().collect::<Vec<_>>();
    let query_collections = query_indexes
        .iter()
        .map(|index| {
            let collection = QueryCollectionPlan::new(
                index.source_list,
                index.source_semantic_path.clone(),
                index.row_schema_hash,
                schema_version,
                QueryRetentionPlan::KeepAll,
            )?;
            Ok((collection.id, collection))
        })
        .collect::<Result<BTreeMap<_, _>, PlanError>>()?
        .into_values()
        .collect();

    let mut plan = MachinePlan {
        version: PlanVersion::default(),
        target_profile,
        program_role,
        application,
        persistence,
        effects,
        outputs,
        host_ports,
        query_collections,
        query_indexes,
        demand: demand_plan(program),
        document,
        constants,
        source_routes,
        storage_layout: StorageLayout {
            scalar_slots,
            list_slots,
            byte_banks,
        },
        dirty_plan: DirtyPlan {
            dependency_edges: program.dependencies.len(),
            unresolved_dependency_edges: regions[6]
                .ops
                .iter()
                .filter(|op| op.unresolved_executable_ref_count > 0)
                .count(),
        },
        commit_plan: CommitPlan {
            update_branch_count: program.update_branches.len() + program.host_effects.len(),
            unresolved_update_branch_count: regions[3]
                .ops
                .iter()
                .filter(|op| op.unresolved_executable_ref_count > 0)
                .count(),
        },
        delta_plan: DeltaPlan {
            deltas: delta_routes(program),
        },
        capability_summary: CapabilitySummary {
            executable: cpu_plan_executor_complete,
            typed_lowering_executable,
            cpu_plan_executor_complete,
            constant_count,
            source_route_count,
            scalar_storage_count,
            list_storage_count,
            byte_bank_storage_count,
            operation_count,
            typed_value_ref_count,
            executable_string_path_count: unresolved_executable_ref_count,
            unresolved_executable_ref_count,
            unknown_plan_op_count,
            cpu_plan_executor_unsupported_op_count,
            runtime_ast_dependency_count: 0,
            graph_rebuild_count: 0,
            graph_clones_per_item,
        },
        debug_map: DebugMap {
            source_units: program
                .semantic_index
                .source_units
                .iter()
                .map(|unit| DebugEntry {
                    id: format!("source_unit:{}", unit.id),
                    label: unit.path.clone(),
                })
                .collect(),
            source_routes: program
                .sources
                .iter()
                .map(|source| DebugEntry {
                    id: format!("source:{}", source.id),
                    label: source.path.clone(),
                })
                .collect(),
            state_slots: program
                .state_cells
                .iter()
                .map(|state| DebugEntry {
                    id: format!("state:{}", state.id),
                    label: state.path.clone(),
                })
                .collect(),
            list_slots: program
                .lists
                .iter()
                .map(|list| DebugEntry {
                    id: format!("list:{}", list.id),
                    label: list.name.clone(),
                })
                .collect(),
            derived_values: program
                .derived_values
                .iter()
                .map(|value| DebugEntry {
                    id: format!("field:{}", value.id),
                    label: value.path.clone(),
                })
                .collect(),
            fields: program
                .semantic_index
                .fields
                .iter()
                .map(|field| DebugEntry {
                    id: format!("field:{}", field.id),
                    label: field.path.clone(),
                })
                .chain(synthetic_initial_field_ids.iter().map(
                    |((list_name, field_name), field_id)| {
                        DebugEntry {
                            id: format!("field:{}", field_id.0),
                            label: program
                                .lists
                                .iter()
                                .find(|list| list.name == *list_name)
                                .filter(|list| list_has_runtime_constructor_map(program, list))
                                .map(|_| format!("{list_name}.$input${field_name}"))
                                .unwrap_or_else(|| format!("{list_name}.{field_name}")),
                        }
                    },
                ))
                .collect(),
            unresolved_executable_refs: unresolved_refs.into_iter().collect(),
        },
        regions,
    };
    include_output_root_demand(&mut plan);
    Ok(plan)
}

fn validate_number_literals(program: &TypedProgram) -> Result<(), PlanError> {
    for expression in &program.expressions {
        let AstExprKind::Number(literal) = &expression.kind else {
            continue;
        };
        literal.parse::<FiniteReal>().map_err(|error| {
            PlanError::new(format!(
                "numeric literal `{literal}` is not a finite canonical Number: {error}"
            ))
        })?;
    }
    Ok(())
}

fn include_output_root_demand(plan: &mut MachinePlan) {
    let indexed = plan
        .storage_layout
        .list_slots
        .iter()
        .flat_map(|slot| slot.row_field_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    let state_labels = plan
        .debug_map
        .state_slots
        .iter()
        .map(|entry| entry.label.as_str())
        .collect::<BTreeSet<_>>();
    let field_state_aliases = plan
        .debug_map
        .fields
        .iter()
        .filter(|entry| state_labels.contains(entry.label.as_str()))
        .filter_map(|entry| {
            entry
                .id
                .rsplit(':')
                .next()
                .and_then(|value| value.parse().ok())
                .map(FieldId)
        })
        .collect::<BTreeSet<_>>();
    let Some(document) = plan.document.as_ref() else {
        return;
    };
    let fields = document
        .expressions
        .iter()
        .filter_map(|expression| match expression.op {
            DocumentExprOp::Read {
                read: DocumentRead::Field { field },
            } if !indexed.contains(&field) && !field_state_aliases.contains(&field) => Some(field),
            _ => None,
        })
        .chain(
            document
                .view_bindings
                .iter()
                .filter_map(|binding| match binding.target {
                    DocumentBindingTarget::Field { field }
                        if !indexed.contains(&field) && !field_state_aliases.contains(&field) =>
                    {
                        Some(field)
                    }
                    _ => None,
                }),
        );
    let RootOutputDemand::Selected(demanded) = &mut plan.demand.root_derived_outputs else {
        return;
    };
    demanded.extend(fields);
    demanded.sort_unstable();
    demanded.dedup();
}

fn initial_constant_value(value: &InitialValue) -> Option<PlanConstantValue> {
    match value {
        InitialValue::Text { value } => Some(PlanConstantValue::Text {
            value: value.clone(),
        }),
        InitialValue::Number { value } => Some(PlanConstantValue::Number {
            value: value.parse().ok()?,
        }),
        InitialValue::Byte { value } => Some(PlanConstantValue::Byte { value: *value }),
        InitialValue::Bool { value } => Some(PlanConstantValue::Bool { value: *value }),
        InitialValue::Bytes { bytes, .. } => {
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            Some(PlanConstantValue::Bytes {
                byte_len: bytes.len() as u64,
                sha256: format!("{:x}", hasher.finalize()),
                inline_bytes: (bytes.len() <= INLINE_BYTE_CONSTANT_LIMIT).then(|| bytes.clone()),
            })
        }
        InitialValue::Enum { value } => Some(PlanConstantValue::Enum {
            value: value.clone(),
        }),
        InitialValue::Data { value } => Some(PlanConstantValue::Data {
            value: value.clone(),
        }),
        InitialValue::RootInitialField { .. }
        | InitialValue::RowInitialField { .. }
        | InitialValue::Unknown { .. } => None,
    }
}

fn initial_value_kind_from_constant(value: &PlanConstantValue) -> InitialValueKind {
    match value {
        PlanConstantValue::Text { .. } => InitialValueKind::Text,
        PlanConstantValue::Number { .. } => InitialValueKind::Number,
        PlanConstantValue::Byte { .. } => InitialValueKind::Byte,
        PlanConstantValue::Bool { .. } => InitialValueKind::Bool,
        PlanConstantValue::Bytes { .. } => InitialValueKind::Bytes,
        PlanConstantValue::Enum { .. } => InitialValueKind::Enum,
        PlanConstantValue::Data { .. } => InitialValueKind::Data,
    }
}

fn constant_initial_expression_value(
    program: &TypedProgram,
    expr_id: usize,
) -> Option<PlanConstantValue> {
    constant_initial_expression_value_inner(program, expr_id, &mut BTreeSet::new())
}

fn constant_initial_expression_value_inner(
    program: &TypedProgram,
    expr_id: usize,
    visiting_functions: &mut BTreeSet<String>,
) -> Option<PlanConstantValue> {
    match &expr_by_id(program, expr_id)?.kind {
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
            Some(PlanConstantValue::Text {
                value: value.clone(),
            })
        }
        AstExprKind::Number(value) => value
            .parse()
            .ok()
            .map(|value| PlanConstantValue::Number { value }),
        AstExprKind::ByteLiteral { value, .. } => Some(PlanConstantValue::Byte { value: *value }),
        AstExprKind::Bool(value) => Some(PlanConstantValue::Bool { value: *value }),
        AstExprKind::Enum(value) | AstExprKind::Tag(value) => Some(PlanConstantValue::Enum {
            value: value.clone(),
        }),
        AstExprKind::BytesLiteral { items, .. } => {
            let bytes = row_static_bytes_literal(program, items)?;
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            Some(PlanConstantValue::Bytes {
                byte_len: bytes.len() as u64,
                sha256: format!("{:x}", hasher.finalize()),
                inline_bytes: (bytes.len() <= INLINE_BYTE_CONSTANT_LIMIT).then_some(bytes),
            })
        }
        AstExprKind::Call { function, args } if args.is_empty() => match function.as_str() {
            "Text/empty" => Some(PlanConstantValue::Text {
                value: String::new(),
            }),
            "Text/space" => Some(PlanConstantValue::Text {
                value: " ".to_owned(),
            }),
            _ => {
                if !visiting_functions.insert(function.clone()) {
                    return None;
                }
                let value = program
                    .functions
                    .iter()
                    .find(|definition| definition.name == *function && definition.args.is_empty())
                    .and_then(|definition| direct_statement_value_expr_id(&definition.statement))
                    .and_then(|function_expr| {
                        constant_initial_expression_value_inner(
                            program,
                            function_expr,
                            visiting_functions,
                        )
                    });
                visiting_functions.remove(function);
                value
            }
        },
        AstExprKind::Hold { initial, .. } => {
            constant_initial_expression_value_inner(program, *initial, visiting_functions)
        }
        _ => None,
    }
}

fn initial_row_field_path(value: &InitialValue) -> Option<String> {
    match value {
        InitialValue::RowInitialField { path } => Some(path.clone()),
        _ => None,
    }
}

fn migration_drain_environment_key(expr_id: usize) -> String {
    format!("$boon$migration_drain:{expr_id}")
}

fn row_lowering_context(state: &ir::StateCell) -> ir::DerivedValue {
    ir::DerivedValue {
        id: ir::FieldId(state.id.0),
        path: state.path.clone(),
        kind: DerivedValueKind::Pure,
        sources: Vec::new(),
        indexed: state.indexed,
        scope_id: state.scope_id,
        startup_recompute: false,
        statement: AstStatement {
            id: usize::MAX.saturating_sub(state.id.as_usize()),
            line: state.source_line,
            indent: 0,
            start: 0,
            end: 0,
            kind: AstStatementKind::Expression,
            expr: state.initial_expr_id.map(|expr| expr.as_usize()),
            children: Vec::new(),
        },
    }
}

fn migration_source_row_default_expression(
    program: &TypedProgram,
    source: &ir::MigrationSourceLeaf,
    target_list: &ir::ListMemory,
    index: &ValueIndex,
    synthetic_field_ids: &BTreeMap<(String, String), FieldId>,
    constants: &mut Vec<PlanConstant>,
) -> Result<PlanRowExpression, PlanError> {
    let memory = program
        .semantic_memory
        .get(source.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("indexed migration default source memory is absent"))?;
    let source_state = state_for_semantic_memory(program, memory)?;
    if let Some(constant) = initial_constant_value(&source_state.initial_value) {
        return Ok(PlanRowExpression::Constant {
            constant_id: push_plan_constant(constants, constant),
        });
    }
    if let InitialValue::RowInitialField { path } = &source_state.initial_value {
        let field = storage_input_field_id(
            program,
            &target_list.name,
            path.rsplit('.').next().unwrap_or(path),
            synthetic_field_ids,
        )
        .ok_or_else(|| {
            PlanError::new(format!(
                "indexed migration default `{}` cannot resolve source row field `{path}`",
                source.semantic_path
            ))
        })?;
        return Ok(PlanRowExpression::Field {
            input: ValueRef::Field(field),
        });
    }
    initial_row_expression(program, source_state, index, synthetic_field_ids, constants).ok_or_else(
        || {
            PlanError::new(format!(
                "indexed migration default `{}` is not reconstructable",
                source.semantic_path
            ))
        },
    )
}

fn migration_indexed_default_expression(
    program: &TypedProgram,
    state: &ir::StateCell,
    edge: &ir::MigrationEdge,
    index: &ValueIndex,
    synthetic_field_ids: &BTreeMap<(String, String), FieldId>,
    constants: &mut Vec<PlanConstant>,
) -> Result<PlanRowExpression, PlanError> {
    let scope_id = state
        .scope_id
        .ok_or_else(|| PlanError::new("indexed migration default has no row scope"))?;
    let target_list = program
        .lists
        .iter()
        .find(|list| list.row_scope_id == Some(scope_id))
        .ok_or_else(|| PlanError::new("indexed migration default has no target list"))?;
    let mut grouped = BTreeMap::<usize, Vec<&ir::MigrationSourceLeaf>>::new();
    for source in &edge.source_leaves {
        grouped
            .entry(source.drain_expr_id.as_usize())
            .or_default()
            .push(source);
    }
    let mut drain_values = BTreeMap::new();
    for (drain_expr_id, sources) in grouped {
        let mut fields = Vec::with_capacity(sources.len());
        for source in sources {
            fields.push((
                source
                    .semantic_path
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                migration_source_row_default_expression(
                    program,
                    source,
                    target_list,
                    index,
                    synthetic_field_ids,
                    constants,
                )?,
            ));
        }
        let value = if fields.len() == 1 {
            fields.pop().expect("one migration source exists").1
        } else {
            PlanRowExpression::Object {
                fields: fields
                    .into_iter()
                    .map(|(name, value)| PlanRowObjectField { name, value })
                    .collect(),
            }
        };
        drain_values.insert(drain_expr_id, LoweredRowValue::Scalar(value));
    }
    if edge.transform == ir::MigrationTransform::Identity {
        if drain_values.len() != 1 {
            return Err(PlanError::new(
                "identity indexed migration default is ambiguous",
            ));
        }
        return drain_values
            .into_values()
            .next()
            .and_then(lowered_scalar)
            .ok_or_else(|| PlanError::new("identity indexed migration default is not scalar"));
    }
    let ir::MigrationTransform::PureExpression { pipeline, .. } = &edge.transform else {
        return Err(PlanError::new(
            "indexed migration default has an unsupported transform",
        ));
    };
    let context = row_lowering_context(state);
    let mut env = drain_values
        .into_iter()
        .map(|(expr_id, value)| (migration_drain_environment_key(expr_id), value))
        .collect::<BTreeMap<_, _>>();
    let mut inputs = Vec::new();
    let expression_types = expression_value_type_lookup(program);
    let mut current = None;
    for expr_id in pipeline {
        if let Some(previous) = current.clone() {
            env.insert(ROW_PREVIOUS_BINDING.to_owned(), previous);
        }
        current = if let Some(statement) = statement_for_expression(program, expr_id.as_usize()) {
            lower_row_statement_value(
                program,
                &context,
                index,
                constants,
                &mut inputs,
                &mut env,
                &expression_types,
                statement,
            )
        } else {
            lower_row_expr(
                program,
                &context,
                index,
                constants,
                &mut inputs,
                &mut env,
                &expression_types,
                expr_id.as_usize(),
            )
        };
        if current.is_none() {
            return Err(PlanError::new(format!(
                "indexed migration default `{}` could not lower expression {}",
                state.path,
                expr_id.as_usize()
            )));
        }
    }
    current
        .and_then(lowered_scalar)
        .ok_or_else(|| PlanError::new("indexed migration default did not produce a scalar"))
}

fn initial_row_expression(
    program: &TypedProgram,
    state: &boon_ir::StateCell,
    index: &ValueIndex,
    synthetic_field_ids: &BTreeMap<(String, String), FieldId>,
    constants: &mut Vec<PlanConstant>,
) -> Option<PlanRowExpression> {
    let InitialValue::RowInitialField { path } = &state.initial_value else {
        return None;
    };
    let initial_expr = state.initial_expr_id?.0;
    let scope_id = state.scope_id?;
    let scope = program
        .row_scopes
        .iter()
        .find(|scope| scope.id == scope_id)?;
    let list = program
        .lists
        .iter()
        .find(|list| list.row_scope_id == Some(scope_id))?;
    if let Some(field) = storage_input_field_id(
        program,
        &list.name,
        path.rsplit('.').next().unwrap_or(path),
        synthetic_field_ids,
    ) {
        return Some(PlanRowExpression::Field {
            input: ValueRef::Field(field),
        });
    }
    let binding = program
        .typecheck_report
        .list_map_bindings
        .iter()
        .find(|binding| {
            binding.result_kind == boon_typecheck::ListMapResultKind::RuntimeValue
                && binding
                    .template_function
                    .as_deref()
                    .is_some_and(|function| {
                        function == scope.function
                            || function.ends_with(&format!("/{}", scope.function))
                            || scope.function.ends_with(&format!("/{function}"))
                    })
        })?;
    let function = program.functions.iter().find(|function| {
        function.name == scope.function
            || function.name.ends_with(&format!("/{}", scope.function))
            || scope.function.ends_with(&format!("/{}", function.name))
    })?;

    let context = row_lowering_context(state);

    let mut input_names = match &list.initializer {
        ListInitializer::RecordLiteral { rows } => rows
            .iter()
            .flat_map(|row| row.fields.iter().map(|field| field.name.clone()))
            .collect::<BTreeSet<_>>(),
        ListInitializer::Range { .. } => BTreeSet::from(["index".to_owned(), "value".to_owned()]),
        ListInitializer::Empty | ListInitializer::Unknown { .. } => BTreeSet::new(),
    };
    input_names.extend(
        synthetic_field_ids
            .keys()
            .filter(|(list_name, _)| list_name == &list.name)
            .map(|(_, field_name)| field_name.clone()),
    );
    let object = input_names
        .into_iter()
        .filter_map(|name| {
            storage_input_field_id(program, &list.name, &name, synthetic_field_ids).map(|field| {
                PlanRowObjectField {
                    name,
                    value: PlanRowExpression::Field {
                        input: ValueRef::Field(field),
                    },
                }
            })
        })
        .collect::<Vec<_>>();
    let mut caller_env = BTreeMap::from([(
        binding.item_binding_name.clone(),
        LoweredRowValue::Scalar(PlanRowExpression::Object { fields: object }),
    )]);
    let mut local_constants = constants.clone();
    let mut inputs = Vec::new();
    let expression_types = expression_value_type_lookup(program);
    let positional = binding
        .template_args
        .iter()
        .filter(|argument| argument.name.is_none())
        .collect::<Vec<_>>();
    let mut env = BTreeMap::new();
    for (index_in_function, parameter) in function.args.iter().enumerate() {
        let Some(argument) = binding
            .template_args
            .iter()
            .find(|argument| argument.name.as_deref() == Some(parameter.as_str()))
            .or_else(|| positional.get(index_in_function).copied())
        else {
            continue;
        };
        let Some(value) = lower_row_expr(
            program,
            &context,
            index,
            &mut local_constants,
            &mut inputs,
            &mut caller_env,
            &expression_types,
            argument.value,
        ) else {
            continue;
        };
        env.insert(parameter.clone(), value);
    }
    let expression = lower_row_expr(
        program,
        &context,
        index,
        &mut local_constants,
        &mut inputs,
        &mut env,
        &expression_types,
        initial_expr,
    )
    .and_then(lowered_scalar)?;
    *constants = local_constants;
    Some(expression)
}

fn initial_root_field_path(value: &InitialValue) -> Option<String> {
    match value {
        InitialValue::RootInitialField { path } => Some(path.clone()),
        _ => None,
    }
}

fn byte_bank_capacity_hint(
    slot: &ScalarStorageSlot,
    list_slots: &[ListStorageSlot],
) -> Option<usize> {
    if !slot.indexed {
        return Some(1);
    }
    list_slots
        .iter()
        .find(|list_slot| list_slot.scope_id == slot.scope_id)
        .and_then(|list_slot| list_slot.capacity)
}

type RowInitialFieldTypeMap = BTreeMap<(Option<ScopeId>, String), PlanValueType>;
type RootInitialFieldTypeMap = BTreeMap<String, PlanValueType>;

fn row_initial_field_value_type(
    row_field_types: &RowInitialFieldTypeMap,
    scope_id: Option<ScopeId>,
    path: &str,
) -> Option<PlanValueType> {
    row_field_types
        .get(&(scope_id, path.to_owned()))
        .copied()
        .or_else(|| {
            path.rsplit_once('.').and_then(|(_, local_name)| {
                row_field_types
                    .get(&(scope_id, local_name.to_owned()))
                    .copied()
            })
        })
        .or_else(|| row_field_types.get(&(None, path.to_owned())).copied())
}

fn row_initial_field_value_types(program: &TypedProgram) -> RowInitialFieldTypeMap {
    let mut row_field_types = RowInitialFieldTypeMap::new();

    for list in &program.lists {
        let ListInitializer::RecordLiteral { rows } = &list.initializer else {
            continue;
        };
        for row in rows {
            for field in &row.fields {
                let value_type = plan_value_type_from_initial_with_row_fields(
                    &field.value,
                    plan_scope_id(list.row_scope_id),
                    &row_field_types,
                );
                insert_row_initial_field_value_type(
                    &mut row_field_types,
                    plan_scope_id(list.row_scope_id),
                    &field.name,
                    value_type,
                );
                insert_row_initial_field_value_type(
                    &mut row_field_types,
                    plan_scope_id(list.row_scope_id),
                    &format!("{}.{}", list.name, field.name),
                    value_type,
                );
            }
        }
    }

    let expr_value_types = expression_value_type_lookup(program);
    for derived in &program.derived_values {
        let Some(value_type) = derived_value_output_type(program, derived, &expr_value_types)
        else {
            continue;
        };
        let local_name = derived.path.rsplit('.').next().unwrap_or(&derived.path);
        insert_row_initial_field_value_type(
            &mut row_field_types,
            plan_scope_id(derived.scope_id),
            local_name,
            value_type,
        );
        insert_row_initial_field_value_type(
            &mut row_field_types,
            plan_scope_id(derived.scope_id),
            &derived.path,
            value_type,
        );
    }

    row_field_types
        .into_iter()
        .filter(|(_, value_type)| plan_value_type_is_concrete(*value_type))
        .collect()
}

fn root_initial_field_value_types(program: &TypedProgram) -> RootInitialFieldTypeMap {
    let mut root_field_types = RootInitialFieldTypeMap::new();
    let source_payload_types = source_payload_value_type_lookup(program);
    let expr_value_types = expression_value_type_lookup(program);
    let derived_types = program
        .derived_values
        .iter()
        .filter(|derived| !derived.indexed)
        .filter_map(|derived| {
            Some((
                derived.path.clone(),
                derived_value_output_type(program, derived, &expr_value_types)?,
            ))
        })
        .collect::<BTreeMap<_, _>>();

    for state in &program.state_cells {
        let InitialValue::RootInitialField { path } = &state.initial_value else {
            continue;
        };
        let initial_path = canonical_sibling_path(&state.path, path);
        if let Some(value_type) = derived_types.get(&initial_path).copied() {
            insert_root_initial_field_value_type(&mut root_field_types, &state.path, value_type);
        }
        for branch in program
            .update_branches
            .iter()
            .filter(|branch| branch.target == state.path)
        {
            let Some(value_type) =
                update_expression_output_type_for_root_initial(branch, &source_payload_types)
            else {
                continue;
            };
            insert_root_initial_field_value_type(&mut root_field_types, &state.path, value_type);
        }
    }

    root_field_types
        .into_iter()
        .filter(|(_, value_type)| plan_value_type_is_concrete(*value_type))
        .collect()
}

fn source_payload_value_type_lookup(
    program: &TypedProgram,
) -> BTreeMap<(String, SourcePayloadField), PlanValueType> {
    let mut payload_types = BTreeMap::new();
    for source in &program.sources {
        for descriptor in &source.payload_schema.typed_fields {
            let value_type = plan_value_type_from_semantic_data_type(&semantic_data_type_plan(
                &descriptor.data_type,
            ));
            if value_type != PlanValueType::Unknown {
                payload_types.insert(
                    (
                        source.path.clone(),
                        source_payload_field_from_ir(&descriptor.field),
                    ),
                    value_type,
                );
            }
        }
    }
    payload_types
}

fn update_expression_output_type_for_root_initial(
    branch: &boon_ir::UpdateBranch,
    source_payload_types: &BTreeMap<(String, SourcePayloadField), PlanValueType>,
) -> Option<PlanValueType> {
    match &branch.expression {
        UpdateExpression::SourcePayload { path } | UpdateExpression::ReadPath { path } => {
            let field = source_payload_field_from_path(&branch.source, path, true)?;
            source_payload_types
                .get(&(branch.source.clone(), field))
                .copied()
        }
        UpdateExpression::Const { value } => Some(infer_static_update_value_type(value)),
        UpdateExpression::PrefixPayloadConcat { .. }
        | UpdateExpression::PrefixRootConcat { .. }
        | UpdateExpression::TextTrimOrPrevious { .. }
        | UpdateExpression::BytesToHex { .. }
        | UpdateExpression::BytesToBase64 { .. }
        | UpdateExpression::BytesToText { .. }
        | UpdateExpression::FileWriteBytes { .. } => Some(PlanValueType::Text),
        UpdateExpression::BoolNot { .. }
        | UpdateExpression::BytesIsEmpty { .. }
        | UpdateExpression::BytesEqual { .. }
        | UpdateExpression::BytesStartsWith { .. }
        | UpdateExpression::BytesEndsWith { .. } => Some(PlanValueType::Bool),
        UpdateExpression::NumberInfix { .. }
        | UpdateExpression::ProjectTime { .. }
        | UpdateExpression::TextToNumber { .. }
        | UpdateExpression::BytesLength { .. }
        | UpdateExpression::BytesReadUnsigned { .. }
        | UpdateExpression::BytesReadSigned { .. }
        | UpdateExpression::BytesFind { .. } => Some(PlanValueType::Number),
        UpdateExpression::BytesGet { .. } => Some(PlanValueType::Byte),
        UpdateExpression::BytesSet { .. }
        | UpdateExpression::BytesSlice { .. }
        | UpdateExpression::BytesTake { .. }
        | UpdateExpression::BytesDrop { .. }
        | UpdateExpression::BytesZeros { .. }
        | UpdateExpression::BytesFromHex { .. }
        | UpdateExpression::BytesFromBase64 { .. }
        | UpdateExpression::TextToBytes { .. }
        | UpdateExpression::BytesConcat { .. }
        | UpdateExpression::BytesWriteUnsigned { .. }
        | UpdateExpression::BytesWriteSigned { .. }
        | UpdateExpression::FileReadBytes { .. } => Some(PlanValueType::Bytes { fixed_len: None }),
        UpdateExpression::PreviousValue { .. }
        | UpdateExpression::ListGet { .. }
        | UpdateExpression::MatchConst { .. }
        | UpdateExpression::MatchValueConst { .. }
        | UpdateExpression::MatchTextIsEmptyConst { .. }
        | UpdateExpression::MatchInfixConst { .. }
        | UpdateExpression::ListFindValue { .. }
        | UpdateExpression::Unknown { .. } => None,
    }
}

fn infer_static_update_value_type(value: &str) -> PlanValueType {
    match value {
        "True" | "False" => PlanValueType::Bool,
        _ if value.parse::<i64>().is_ok() => PlanValueType::Number,
        _ => PlanValueType::Text,
    }
}

fn insert_root_initial_field_value_type(
    root_field_types: &mut RootInitialFieldTypeMap,
    path: &str,
    value_type: PlanValueType,
) {
    if !plan_value_type_is_concrete(value_type) {
        return;
    }
    root_field_types
        .entry(path.to_owned())
        .and_modify(|existing| {
            if *existing != value_type {
                *existing = PlanValueType::Unknown;
            }
        })
        .or_insert(value_type);
}

fn insert_row_initial_field_value_type(
    row_field_types: &mut RowInitialFieldTypeMap,
    scope_id: Option<ScopeId>,
    path: &str,
    value_type: PlanValueType,
) {
    if !plan_value_type_is_concrete(value_type) {
        return;
    }
    row_field_types
        .entry((scope_id, path.to_owned()))
        .and_modify(|existing| {
            if *existing != value_type {
                *existing = PlanValueType::Unknown;
            }
        })
        .or_insert(value_type);
}

fn plan_value_type_is_concrete(value_type: PlanValueType) -> bool {
    matches!(
        value_type,
        PlanValueType::Text
            | PlanValueType::Number
            | PlanValueType::Byte
            | PlanValueType::Bool
            | PlanValueType::Bytes { .. }
            | PlanValueType::Enum
            | PlanValueType::Data
    )
}

fn data_type_plan_from_data(value: &boon_data::Value) -> DataTypePlan {
    match value {
        boon_data::Value::Null => DataTypePlan::Null,
        boon_data::Value::Bool(_) => DataTypePlan::Bool,
        boon_data::Value::Number(_) => DataTypePlan::Number,
        boon_data::Value::Text(_) => DataTypePlan::Text,
        boon_data::Value::Bytes(_) => DataTypePlan::Bytes { fixed_len: None },
        boon_data::Value::List(values) => {
            let item = values
                .first()
                .map(data_type_plan_from_data)
                .unwrap_or(DataTypePlan::Unknown);
            DataTypePlan::List {
                item: Box::new(item),
            }
        }
        boon_data::Value::Record(fields) => DataTypePlan::Record {
            fields: fields
                .iter()
                .map(|(name, value)| DataTypeFieldPlan {
                    name: name.clone(),
                    data_type: data_type_plan_from_data(value),
                })
                .collect(),
            open: false,
        },
        boon_data::Value::Variant { tag, fields } => DataTypePlan::Variant {
            variants: vec![DataVariantPlan {
                tag: tag.clone(),
                fields: fields
                    .iter()
                    .map(|(name, value)| DataTypeFieldPlan {
                        name: name.clone(),
                        data_type: data_type_plan_from_data(value),
                    })
                    .collect(),
                open: false,
            }],
        },
        boon_data::Value::Error { fields, .. } => DataTypePlan::Error {
            fields: fields
                .iter()
                .map(|(name, value)| DataTypeFieldPlan {
                    name: name.clone(),
                    data_type: data_type_plan_from_data(value),
                })
                .collect(),
            open: false,
        },
    }
}

fn expression_value_type_lookup(program: &TypedProgram) -> BTreeMap<usize, PlanValueType> {
    program
        .typecheck_report
        .expr_type_table
        .entries
        .iter()
        .filter_map(|entry| {
            plan_value_type_from_typecheck_type(&entry.flow_type.ty)
                .map(|value_type| (entry.expr_id, value_type))
        })
        .collect()
}

fn derived_value_output_type(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
) -> Option<PlanValueType> {
    direct_statement_value_expr_id(&derived.statement)
        .and_then(|expr_id| inferred_expression_value_type(program, expr_id, expr_value_types))
        .filter(|value_type| plan_value_type_is_concrete(*value_type))
        .or_else(|| {
            (derived.kind == DerivedValueKind::SourceEventTransform)
                .then(|| {
                    let exprs = super::compiler_statement_ast_exprs(
                        &derived.statement,
                        &program.expressions,
                    );
                    let mut output_type = None;
                    for source in &derived.sources {
                        let arm = source_event_transform_arm_statement(
                            program,
                            derived,
                            &exprs,
                            source,
                            &derived.statement,
                        )?;
                        let arm_expr = expr_by_id(program, arm.expr?)?;
                        let output_expr = match arm_expr.kind {
                            AstExprKind::Then {
                                output: Some(output),
                                ..
                            } => output,
                            _ => arm_expr.id,
                        };
                        let value_type =
                            inferred_expression_value_type(program, output_expr, expr_value_types)?;
                        if !plan_value_type_is_concrete(value_type) {
                            return None;
                        }
                        match output_type {
                            Some(existing) if existing != value_type => return None,
                            Some(_) => {}
                            None => output_type = Some(value_type),
                        }
                    }
                    output_type
                })
                .flatten()
        })
}

fn inferred_expression_value_type(
    program: &TypedProgram,
    expr_id: usize,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
) -> Option<PlanValueType> {
    inferred_expression_value_type_inner(program, expr_id, expr_value_types, &mut BTreeSet::new())
}

fn inferred_expression_value_type_inner(
    program: &TypedProgram,
    expr_id: usize,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    visiting_functions: &mut BTreeSet<String>,
) -> Option<PlanValueType> {
    if let Some(value_type) = expr_value_types.get(&expr_id).copied() {
        return Some(value_type);
    }
    let expr = expr_by_id(program, expr_id)?;
    match &expr.kind {
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Some(PlanValueType::Text),
        AstExprKind::Number(_) => Some(PlanValueType::Number),
        AstExprKind::ByteLiteral { .. } => Some(PlanValueType::Byte),
        AstExprKind::Bool(_) => Some(PlanValueType::Bool),
        AstExprKind::Tag(_) | AstExprKind::Enum(_) | AstExprKind::TaggedObject { .. } => {
            Some(PlanValueType::Enum)
        }
        AstExprKind::BytesLiteral { size, items } => {
            inferred_bytes_literal_value_type(program, size, items, expr_value_types)
        }
        AstExprKind::Call { function, args } => inferred_call_value_type(
            program,
            function,
            args,
            expr_value_types,
            visiting_functions,
        ),
        AstExprKind::Pipe { input, op, args } => {
            let mut call_args = Vec::with_capacity(args.len() + 1);
            call_args.push(AstCallArg {
                name: Some("input".to_owned()),
                value: *input,
                start: expr.start,
                end: expr.end,
            });
            call_args.extend(args.iter().cloned());
            inferred_call_value_type(
                program,
                op,
                &call_args,
                expr_value_types,
                visiting_functions,
            )
        }
        AstExprKind::Infix { left, op, right } if op == "+" => {
            let left_type = inferred_expression_value_type_inner(
                program,
                *left,
                expr_value_types,
                visiting_functions,
            );
            let right_type = inferred_expression_value_type_inner(
                program,
                *right,
                expr_value_types,
                visiting_functions,
            );
            match (left_type, right_type) {
                (Some(PlanValueType::Number), Some(PlanValueType::Number)) => {
                    Some(PlanValueType::Number)
                }
                (Some(PlanValueType::Text), _) | (_, Some(PlanValueType::Text)) => {
                    Some(PlanValueType::Text)
                }
                _ => None,
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            let left_type = inferred_expression_value_type_inner(
                program,
                *left,
                expr_value_types,
                visiting_functions,
            );
            let right_type = inferred_expression_value_type_inner(
                program,
                *right,
                expr_value_types,
                visiting_functions,
            );
            (left_type == Some(PlanValueType::Number) && right_type == Some(PlanValueType::Number))
                .then_some(PlanValueType::Number)
        }
        _ => None,
    }
}

fn inferred_bytes_literal_value_type(
    program: &TypedProgram,
    size: &BytesSizeSyntax,
    items: &[usize],
    expr_value_types: &BTreeMap<usize, PlanValueType>,
) -> Option<PlanValueType> {
    match size {
        BytesSizeSyntax::Dynamic => Some(PlanValueType::Bytes { fixed_len: None }),
        BytesSizeSyntax::Fixed(len) => Some(PlanValueType::Bytes {
            fixed_len: Some(*len as u64),
        }),
        BytesSizeSyntax::Infer => {
            let mut len = 0u64;
            for item in items {
                match inferred_expression_value_type(program, *item, expr_value_types)? {
                    PlanValueType::Byte => len += 1,
                    PlanValueType::Bytes {
                        fixed_len: Some(item_len),
                    } => len += item_len,
                    PlanValueType::Bytes { fixed_len: None } => {
                        return Some(PlanValueType::Bytes { fixed_len: None });
                    }
                    _ => return None,
                }
            }
            Some(PlanValueType::Bytes {
                fixed_len: Some(len),
            })
        }
    }
}

fn inferred_call_value_type(
    program: &TypedProgram,
    function: &str,
    args: &[AstCallArg],
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    visiting_functions: &mut BTreeSet<String>,
) -> Option<PlanValueType> {
    if let Some(value_type) = inferred_builtin_call_value_type(
        program,
        function,
        args,
        expr_value_types,
        visiting_functions,
    ) {
        return Some(value_type);
    }
    if !visiting_functions.insert(function.to_owned()) {
        return None;
    }
    let result = program
        .functions
        .iter()
        .find(|candidate| candidate.name == function)
        .and_then(|definition| direct_statement_value_expr_id(&definition.statement))
        .and_then(|expr_id| {
            inferred_expression_value_type_inner(
                program,
                expr_id,
                expr_value_types,
                visiting_functions,
            )
        });
    visiting_functions.remove(function);
    result
}

fn inferred_builtin_call_value_type(
    program: &TypedProgram,
    function: &str,
    args: &[AstCallArg],
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    visiting_functions: &mut BTreeSet<String>,
) -> Option<PlanValueType> {
    match function {
        "Text/empty"
        | "Text/space"
        | "Text/trim"
        | "Text/concat"
        | "Text/substring"
        | "Text/time_range_label"
        | "Number/to_text"
        | "Number/to_codepoint_text"
        | "Number/to_ascii_text"
        | "Bytes/to_text"
        | "Bytes/to_hex"
        | "Bytes/to_base64"
        | "Error/text"
        | "File/write_bytes"
        | "File/read_text"
        | "Router/route"
        | "Router/go_to" => Some(PlanValueType::Text),
        "Number/add"
        | "Number/subtract"
        | "Number/min"
        | "Number/max"
        | "Number/bit_width"
        | "Number/ceil"
        | "Number/floor"
        | "Number/round"
        | "Number/truncate"
        | "Number/interpolate"
        | "Number/project_width"
        | "Number/project_offset"
        | "Number/project_time"
        | "List/count"
        | "List/length"
        | "List/sum"
        | "Text/find"
        | "Text/length"
        | "Text/to_number"
        | "Bytes/length"
        | "Bytes/find"
        | "Bytes/read_unsigned"
        | "Bytes/read_signed" => Some(PlanValueType::Number),
        "Bytes/get" => Some(PlanValueType::Byte),
        "Bool/not" | "Bool/and" | "Bool/toggle" | "Text/is_empty" | "Text/is_not_empty"
        | "Text/starts_with" | "Text/contains" | "Text/all_chars_in" | "Bytes/is_empty"
        | "Bytes/equal" | "Bytes/starts_with" | "Bytes/ends_with" => Some(PlanValueType::Bool),
        "Bytes/set"
        | "Bytes/slice"
        | "Bytes/take"
        | "Bytes/drop"
        | "Bytes/concat"
        | "Bytes/zeros"
        | "Text/to_bytes"
        | "Bytes/from_hex"
        | "Bytes/from_base64"
        | "Bytes/write_unsigned"
        | "Bytes/write_signed"
        | "File/read_bytes" => Some(PlanValueType::Bytes { fixed_len: None }),
        "List/find_value" => named_arg(args, "fallback").and_then(|fallback| {
            inferred_expression_value_type_inner(
                program,
                fallback.value,
                expr_value_types,
                visiting_functions,
            )
        }),
        _ => None,
    }
}

fn named_arg<'a>(args: &'a [AstCallArg], name: &str) -> Option<&'a AstCallArg> {
    args.iter().find(|arg| arg.name.as_deref() == Some(name))
}

fn plan_value_type_from_typecheck_type(ty: &boon_typecheck::Type) -> Option<PlanValueType> {
    match ty {
        boon_typecheck::Type::Text => Some(PlanValueType::Text),
        boon_typecheck::Type::Number => Some(PlanValueType::Number),
        boon_typecheck::Type::Byte => Some(PlanValueType::Byte),
        boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Dynamic) => {
            Some(PlanValueType::Bytes { fixed_len: None })
        }
        boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Fixed(len)) => {
            Some(PlanValueType::Bytes {
                fixed_len: Some(*len as u64),
            })
        }
        boon_typecheck::Type::VariantSet(variants)
            if variants.iter().all(|variant| {
                matches!(
                    variant,
                    boon_typecheck::Variant::Tag(value) if value == "True" || value == "False"
                )
            }) =>
        {
            Some(PlanValueType::Bool)
        }
        boon_typecheck::Type::VariantSet(_) => Some(PlanValueType::Enum),
        _ => None,
    }
}

fn direct_statement_value_expr_id(statement: &AstStatement) -> Option<usize> {
    if let Some(expr) = statement.expr {
        return Some(expr);
    }
    let body = statement
        .children
        .iter()
        .find(|child| matches!(child.kind, AstStatementKind::Block))
        .unwrap_or(statement);
    body.children
        .iter()
        .rev()
        .find_map(|child| match child.kind {
            AstStatementKind::Expression | AstStatementKind::List { field: None, .. } => {
                child.expr.or_else(|| direct_statement_value_expr_id(child))
            }
            AstStatementKind::Block => direct_statement_value_expr_id(child),
            AstStatementKind::Function { .. }
            | AstStatementKind::Field { .. }
            | AstStatementKind::Source { .. }
            | AstStatementKind::Hold { .. }
            | AstStatementKind::List { field: Some(_), .. }
            | AstStatementKind::Spread => None,
        })
}

fn plan_initial_list_rows(
    program: &TypedProgram,
    list: &boon_ir::ListMemory,
    initializer: &ListInitializer,
    synthetic_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Vec<PlanInitialListRow> {
    let ListInitializer::RecordLiteral { rows } = initializer else {
        return Vec::new();
    };
    rows.iter()
        .map(|row| PlanInitialListRow {
            fields: row
                .fields
                .iter()
                .filter_map(|field| {
                    initial_constant_value(&field.value).map(|value| PlanInitialListField {
                        name: field.name.clone(),
                        field_id: storage_input_field_id(
                            program,
                            &list.name,
                            &field.name,
                            synthetic_field_ids,
                        ),
                        value,
                    })
                })
                .collect(),
        })
        .collect()
}

fn row_field_id_for_list_field(
    program: &TypedProgram,
    list_name: &str,
    field_name: &str,
    synthetic_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Option<FieldId> {
    let row_scope_id = match program
        .lists
        .iter()
        .find(|list| list.name == list_name)
        .and_then(|list| list.row_scope_id)
    {
        Some(row_scope_id) => row_scope_id,
        None => {
            return synthetic_field_ids
                .get(&(list_name.to_owned(), field_name.to_owned()))
                .copied();
        }
    };
    program
        .semantic_index
        .fields
        .iter()
        .find(|field| field.scope_id == Some(row_scope_id) && field.local_name == field_name)
        .map(|field| plan_field_id(field.id))
}

fn storage_input_field_id(
    program: &TypedProgram,
    list_name: &str,
    field_name: &str,
    synthetic_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Option<FieldId> {
    synthetic_field_ids
        .get(&(list_name.to_owned(), field_name.to_owned()))
        .copied()
        .or_else(|| {
            row_field_id_for_list_field(program, list_name, field_name, synthetic_field_ids)
        })
}

fn row_field_id_for_list_id(
    program: &TypedProgram,
    list_id: ListId,
    field_name: &str,
) -> Option<FieldId> {
    let list = program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == list_id)?;
    let synthetic_field_ids = synthetic_initial_list_field_ids(program);
    row_field_id_for_list_field(program, &list.name, field_name, &synthetic_field_ids)
}

fn row_input_field_id_for_list_id(
    program: &TypedProgram,
    list_id: ListId,
    field_name: &str,
) -> Option<FieldId> {
    let list = program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == list_id)?;
    let synthetic_field_ids = synthetic_initial_list_field_ids(program);
    storage_input_field_id(program, &list.name, field_name, &synthetic_field_ids)
}

fn query_row_data_type(
    program: &TypedProgram,
    index: &ValueIndex,
    list_id: ListId,
) -> Option<DataTypePlan> {
    let list = program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == list_id)?;
    let semantic_row = program
        .semantic_memory
        .iter()
        .find(|memory| {
            matches!(
                memory.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List { list_id: owner, .. } if owner == list.id
            )
        })
        .and_then(|memory| {
            let DataTypePlan::List { item } =
                semantic_data_type_plan(&memory.data_type).canonicalized()
            else {
                return None;
            };
            matches!(&*item, DataTypePlan::Record { fields, .. } if !fields.is_empty())
                .then_some(*item)
        });
    let synthetic = synthetic_initial_list_field_ids(program);
    let runtime_row = list_row_field_ids(program, list, &synthetic)
        .into_iter()
        .map(|field_id| {
            let name = program
                .semantic_index
                .fields
                .iter()
                .find(|field| plan_field_id(field.id) == field_id)
                .map(|field| {
                    field
                        .path
                        .rsplit_once('.')
                        .map_or_else(|| field.path.clone(), |(_, name)| name.to_owned())
                })
                .or_else(|| {
                    synthetic.iter().find_map(|((owner, name), candidate)| {
                        (owner == &list.name && *candidate == field_id).then(|| name.clone())
                    })
                })?;
            let data_type =
                data_type_plan_for_value_ref(program, index, &ValueRef::Field(field_id))?;
            Some(DataTypeFieldPlan { name, data_type })
        })
        .collect::<Option<Vec<_>>>()
        .filter(|fields| !fields.is_empty())
        .map(|fields| {
            DataTypePlan::Record {
                fields,
                open: false,
            }
            .canonicalized()
        });
    let initializer_row = match &list.initializer {
        ListInitializer::RecordLiteral { rows } => {
            let mut fields = BTreeMap::<String, DataTypePlan>::new();
            for field in rows.iter().flat_map(|row| &row.fields) {
                let data_type = data_type_plan_from_initial_value(&field.value)?;
                match fields.entry(field.name.clone()) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(data_type);
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        *entry.get_mut() = merge_query_data_types(entry.get(), &data_type)?;
                    }
                }
            }
            (!fields.is_empty()).then(|| DataTypePlan::Record {
                fields: fields
                    .into_iter()
                    .map(|(name, data_type)| DataTypeFieldPlan { name, data_type })
                    .collect(),
                open: false,
            })
        }
        ListInitializer::Range { .. } => Some(DataTypePlan::Record {
            fields: vec![
                DataTypeFieldPlan {
                    name: "index".to_owned(),
                    data_type: DataTypePlan::Number,
                },
                DataTypeFieldPlan {
                    name: "value".to_owned(),
                    data_type: DataTypePlan::Number,
                },
            ],
            open: false,
        }),
        ListInitializer::Empty | ListInitializer::Unknown { .. } => {
            let fields = list_append_authoritative_field_types(program, index, &list.name).ok()?;
            (!fields.is_empty()).then(|| DataTypePlan::Record {
                fields: fields
                    .into_iter()
                    .map(|(name, data_type)| DataTypeFieldPlan { name, data_type })
                    .collect(),
                open: false,
            })
        }
    };
    let mut merged = None;
    for next in [semantic_row, runtime_row, initializer_row]
        .into_iter()
        .flatten()
    {
        merged = Some(match merged {
            Some(current) => merge_query_data_types(&current, &next)?,
            None => next,
        });
    }
    merged
}

fn merge_query_data_types(left: &DataTypePlan, right: &DataTypePlan) -> Option<DataTypePlan> {
    if left == right {
        return Some(left.clone());
    }
    match (left, right) {
        (DataTypePlan::Variant { variants: left }, DataTypePlan::Variant { variants: right }) => {
            let mut variants = left
                .iter()
                .map(|variant| (variant.tag.clone(), variant.clone()))
                .collect::<BTreeMap<_, _>>();
            for variant in right {
                if variants
                    .insert(variant.tag.clone(), variant.clone())
                    .is_some_and(|previous| previous != *variant)
                {
                    return None;
                }
            }
            Some(DataTypePlan::Variant {
                variants: variants.into_values().collect(),
            })
        }
        (DataTypePlan::List { item: left }, DataTypePlan::List { item: right }) => {
            Some(DataTypePlan::List {
                item: Box::new(merge_query_data_types(left, right)?),
            })
        }
        (
            DataTypePlan::Record {
                fields: left,
                open: left_open,
            },
            DataTypePlan::Record {
                fields: right,
                open: right_open,
            },
        ) => {
            let mut fields = left
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect::<BTreeMap<_, _>>();
            for field in right {
                match fields.entry(field.name.clone()) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(field.data_type.clone());
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        *entry.get_mut() = merge_query_data_types(entry.get(), &field.data_type)?;
                    }
                }
            }
            Some(DataTypePlan::Record {
                fields: fields
                    .into_iter()
                    .map(|(name, data_type)| DataTypeFieldPlan { name, data_type })
                    .collect(),
                open: *left_open || *right_open,
            })
        }
        _ => None,
    }
}

fn query_path_data_type<'a>(
    mut data_type: &'a DataTypePlan,
    path: &[String],
) -> Option<&'a DataTypePlan> {
    for component in path {
        let DataTypePlan::Record { fields, .. } = data_type else {
            return None;
        };
        data_type = &fields
            .iter()
            .find(|field| field.name == *component)?
            .data_type;
    }
    Some(data_type)
}

fn query_key_type(
    row_type: &DataTypePlan,
    path: &[String],
    multi_value: bool,
) -> Option<QueryKeyType> {
    let mut data_type = query_path_data_type(row_type, path)?;
    if multi_value {
        let DataTypePlan::List { item } = data_type else {
            return None;
        };
        data_type = item;
    }
    Some(match data_type {
        DataTypePlan::Bool => QueryKeyType::Bool,
        DataTypePlan::Number | DataTypePlan::Byte => QueryKeyType::Number,
        DataTypePlan::Text => QueryKeyType::Text,
        DataTypePlan::Variant { variants }
            if !variants.is_empty()
                && variants
                    .iter()
                    .all(|variant| variant.fields.is_empty() && !variant.open) =>
        {
            QueryKeyType::Tag
        }
        _ => return None,
    })
}

fn plan_query_residual(
    residual: &ir::ListQueryResidual,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    unresolved_count: &mut usize,
    unresolved: &mut BTreeSet<String>,
) -> Option<PlanQueryResidual> {
    let path_is_valid =
        |path: &[String]| !path.is_empty() && path.iter().all(|part| !part.is_empty());
    match residual {
        ir::ListQueryResidual::FieldEqual { path, value } if path_is_valid(path) => {
            resolve_query_ref(index, value, inputs, unresolved_count, unresolved).map(|value| {
                PlanQueryResidual::FieldEqual {
                    path: path.clone(),
                    value,
                }
            })
        }
        ir::ListQueryResidual::TextContains { path, needle } if path_is_valid(path) => {
            resolve_query_ref(index, needle, inputs, unresolved_count, unresolved).map(|needle| {
                PlanQueryResidual::TextContains {
                    path: path.clone(),
                    needle,
                }
            })
        }
        ir::ListQueryResidual::NumberRange {
            path,
            minimum,
            maximum,
        } if path_is_valid(path) && (minimum.is_some() || maximum.is_some()) => {
            let minimum = minimum.as_ref().and_then(|value| {
                resolve_query_ref(index, value, inputs, unresolved_count, unresolved)
            });
            let maximum = maximum.as_ref().and_then(|value| {
                resolve_query_ref(index, value, inputs, unresolved_count, unresolved)
            });
            Some(PlanQueryResidual::NumberRange {
                path: path.clone(),
                minimum,
                maximum,
            })
        }
        ir::ListQueryResidual::Wgs84Radius {
            latitude_path,
            longitude_path,
            center_latitude,
            center_longitude,
            radius_meters,
        } if path_is_valid(latitude_path) && path_is_valid(longitude_path) => {
            let center_latitude =
                resolve_query_ref(index, center_latitude, inputs, unresolved_count, unresolved);
            let center_longitude = resolve_query_ref(
                index,
                center_longitude,
                inputs,
                unresolved_count,
                unresolved,
            );
            let radius_meters =
                resolve_query_ref(index, radius_meters, inputs, unresolved_count, unresolved);
            match (center_latitude, center_longitude, radius_meters) {
                (Some(center_latitude), Some(center_longitude), Some(radius_meters)) => {
                    Some(PlanQueryResidual::Wgs84Radius {
                        latitude_path: latitude_path.clone(),
                        longitude_path: longitude_path.clone(),
                        center_latitude,
                        center_longitude,
                        radius_meters,
                    })
                }
                _ => None,
            }
        }
        ir::ListQueryResidual::Unknown { value } => {
            *unresolved_count += 1;
            unresolved.insert(format!("List/query.residual.{value}"));
            None
        }
        _ => {
            *unresolved_count += 1;
            unresolved.insert("List/query.residual.invalid".to_owned());
            None
        }
    }
}

fn list_row_field_ids(
    program: &TypedProgram,
    list: &boon_ir::ListMemory,
    synthetic_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Vec<FieldId> {
    let mut fields = BTreeSet::new();
    if let Some(row_scope_id) = list.row_scope_id {
        fields.extend(
            program
                .semantic_index
                .fields
                .iter()
                .filter(|field| field.scope_id == Some(row_scope_id))
                .map(|field| plan_field_id(field.id)),
        );
    }
    fields.extend(
        synthetic_field_ids
            .iter()
            .filter(|((list_name, _), _)| list_name == &list.name)
            .map(|(_, field_id)| *field_id),
    );
    fields.into_iter().collect()
}

fn synthetic_initial_list_field_ids(program: &TypedProgram) -> BTreeMap<(String, String), FieldId> {
    let mut next_id = program
        .semantic_index
        .fields
        .iter()
        .map(|field| field.id.0)
        .chain(program.derived_values.iter().map(|field| field.id.0))
        .max()
        .map(|id| id + 1)
        .unwrap_or(0);
    let mut ids = BTreeMap::new();
    for list in &program.lists {
        match &list.initializer {
            ListInitializer::RecordLiteral { rows } => {
                if list.row_scope_id.is_some() && !list_has_runtime_constructor_map(program, list) {
                    continue;
                }
                for row in rows {
                    for field in &row.fields {
                        if initial_constant_value(&field.value).is_none() {
                            continue;
                        }
                        ids.entry((list.name.clone(), field.name.clone()))
                            .or_insert_with(|| {
                                let id = FieldId(next_id);
                                next_id += 1;
                                id
                            });
                    }
                }
            }
            ListInitializer::Range { .. } => {
                for field_name in ["index", "value"] {
                    ids.entry((list.name.clone(), field_name.to_owned()))
                        .or_insert_with(|| {
                            let id = FieldId(next_id);
                            next_id += 1;
                            id
                        });
                }
            }
            ListInitializer::Empty | ListInitializer::Unknown { .. } => {
                for field in program
                    .list_operations
                    .iter()
                    .filter(|operation| operation.list == list.name)
                    .filter_map(|operation| match &operation.kind {
                        ListOperationKind::Append { fields, .. } => Some(fields),
                        _ => None,
                    })
                    .flatten()
                {
                    ids.entry((list.name.clone(), field.name.clone()))
                        .or_insert_with(|| {
                            let id = FieldId(next_id);
                            next_id += 1;
                            id
                        });
                }
            }
        }
    }
    ids
}

fn list_has_runtime_constructor_map(program: &TypedProgram, list: &boon_ir::ListMemory) -> bool {
    let Some(scope) = list.row_scope_id.and_then(|scope| {
        program
            .row_scopes
            .iter()
            .find(|candidate| candidate.id == scope)
    }) else {
        return false;
    };
    program
        .typecheck_report
        .list_map_bindings
        .iter()
        .any(|binding| {
            binding.result_kind == boon_typecheck::ListMapResultKind::RuntimeValue
                && binding
                    .template_function
                    .as_deref()
                    .is_some_and(|function| {
                        function == scope.function
                            || function.ends_with(&format!("/{}", scope.function))
                            || scope.function.ends_with(&format!("/{function}"))
                    })
        })
}

fn append_constant_id(constants: &mut Vec<PlanConstant>, value: &str) -> PlanConstantId {
    push_plan_constant(constants, append_constant_value(value))
}

fn list_append_value_ref(
    program: &TypedProgram,
    index: &ValueIndex,
    trigger: &str,
    path: &str,
) -> Option<ValueRef> {
    let mut sources = program
        .sources
        .iter()
        .filter(|source| source.path == trigger)
        .map(|source| source.path.as_str())
        .collect::<Vec<_>>();
    if let Some(derived) = program.derived_values.iter().find(|derived| {
        derived.path == trigger && derived.kind == DerivedValueKind::SourceEventTransform
    }) {
        sources.extend(derived.sources.iter().map(String::as_str));
    }

    let mut payload_refs = Vec::new();
    for source in sources {
        let field = index
            .source_field_payload_alias(source, path)
            .or_else(|| source_row_lookup_payload_field_from_path(index, source, path))
            .or_else(|| {
                source_payload_field_from_path(source, path, true)
                    .filter(|field| index.source_has_payload_field(source, field))
            });
        let Some(field) = field else {
            continue;
        };
        let Some(ValueRef::Source(source_id)) = index.resolve(source) else {
            continue;
        };
        let value_ref = ValueRef::SourcePayload { source_id, field };
        if !payload_refs.contains(&value_ref) {
            payload_refs.push(value_ref);
        }
    }
    match payload_refs.as_slice() {
        [value_ref] => Some(value_ref.clone()),
        [] => index.resolve(path),
        _ => None,
    }
}

fn append_constant_value(value: &str) -> PlanConstantValue {
    match value {
        "True" => PlanConstantValue::Bool { value: true },
        "False" => PlanConstantValue::Bool { value: false },
        _ => plan_number_constant(value).unwrap_or_else(|| PlanConstantValue::Text {
            value: value.to_owned(),
        }),
    }
}

fn plan_number_constant(value: &str) -> Option<PlanConstantValue> {
    value
        .parse()
        .ok()
        .map(|value| PlanConstantValue::Number { value })
}

fn plan_integer_constant(value: i64) -> Option<PlanConstantValue> {
    FiniteReal::from_i64_exact(value)
        .ok()
        .map(|value| PlanConstantValue::Number { value })
}

fn push_integer_plan_constant(
    constants: &mut Vec<PlanConstant>,
    value: i64,
) -> Option<PlanConstantId> {
    Some(push_plan_constant(constants, plan_integer_constant(value)?))
}

fn derived_expression_for_value(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    _unresolved_refs: &mut BTreeSet<String>,
) -> Option<PlanDerivedExpression> {
    source_group_derived_expression(program, derived, index, inputs)
        .or_else(|| source_key_text_trim_non_empty_expression(program, derived, index, inputs))
        .or_else(|| source_event_transform_expression(program, derived, index, constants, inputs))
        .or_else(|| bool_not_derived_expression(program, derived, index, inputs))
        .or_else(|| number_compare_const_derived_expression(program, derived, index, inputs))
        .or_else(|| root_bool_derived_expression(program, derived, index, inputs))
        .or_else(|| row_expression_for_value(program, derived, index, constants, inputs))
}

fn source_group_derived_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
) -> Option<PlanDerivedExpression> {
    if !statement_is_source_group(program, &derived.statement) {
        return None;
    }
    let expression =
        source_group_row_expression(program, &derived.statement, &derived.path, index, inputs)?;
    Some(PlanDerivedExpression::RowExpression { expression })
}

fn source_group_row_expression(
    program: &TypedProgram,
    statement: &AstStatement,
    path: &str,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
) -> Option<PlanRowExpression> {
    let mut fields = Vec::with_capacity(statement.children.len());
    for child in &statement.children {
        let (name, value) = match &child.kind {
            AstStatementKind::Source { field, .. } => {
                let field = field.as_ref()?;
                let source_path = format!("{path}.{field}");
                let source = index.resolve(&source_path)?;
                if !matches!(source, ValueRef::Source(_)) {
                    return None;
                }
                if !inputs.contains(&source) {
                    inputs.push(source.clone());
                }
                (field.clone(), PlanRowExpression::Field { input: source })
            }
            AstStatementKind::Field { name } if statement_is_source_group(program, child) => {
                let child_path = format!("{path}.{name}");
                (
                    name.clone(),
                    source_group_row_expression(program, child, &child_path, index, inputs)?,
                )
            }
            _ if row_statement_is_empty_delimiter(child, program) => continue,
            _ => return None,
        };
        fields.push(PlanRowObjectField { name, value });
    }
    Some(PlanRowExpression::Object { fields })
}

fn source_event_transform_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
) -> Option<PlanDerivedExpression> {
    if derived.kind != DerivedValueKind::SourceEventTransform {
        return None;
    }

    let mut local_constants = constants.clone();
    let mut local_inputs = inputs.clone();
    let expr_value_types = expression_value_type_lookup(program);
    let mut env = BTreeMap::new();

    let exprs = super::compiler_statement_ast_exprs(&derived.statement, &program.expressions);
    let mut arm_values = Vec::new();
    for source in &derived.sources {
        let ValueRef::Source(source_id) = index.resolve(source)? else {
            continue;
        };
        let value = source_event_transform_arm_expression(
            program,
            derived,
            index,
            &mut local_constants,
            &mut local_inputs,
            &expr_value_types,
            &exprs,
            source,
        )
        .or_else(|| {
            super::compiler_source_then_field_value(&exprs, source).map(|value| {
                row_expression_from_compiler_field_value(
                    &mut local_constants,
                    &mut local_inputs,
                    value,
                )
            })
        })
        .or_else(|| {
            source_event_transform_text_arm_expression(
                program,
                derived,
                index,
                &mut local_inputs,
                source,
            )
        });
        let value = value?;
        if !local_inputs.contains(&ValueRef::Source(source_id)) {
            local_inputs.push(ValueRef::Source(source_id));
        }
        arm_values.push((source_id, value));
    }
    if arm_values.is_empty() {
        return None;
    }
    let output_type =
        source_event_transform_output_type(program, index, &local_constants, &arm_values);
    let default = source_event_transform_default_expression(
        program,
        derived,
        index,
        &mut local_constants,
        &mut local_inputs,
        &mut env,
        &expr_value_types,
        output_type,
    )
    .unwrap_or_else(|| {
        let value = source_event_transform_fresh_value(output_type, &local_constants, &arm_values);
        row_constant_expression(&mut local_constants, &mut local_inputs, value)
    });
    let arms = arm_values
        .into_iter()
        .map(|(source_id, value)| PlanSourceEventTransformArm { source_id, value })
        .collect::<Vec<_>>();

    *constants = local_constants;
    *inputs = local_inputs;
    Some(PlanDerivedExpression::SourceEventTransform {
        default: Box::new(default),
        arms,
        router_route: super::compiler_statement_calls_router_go_to(&exprs),
    })
}

fn source_event_transform_output_type(
    program: &TypedProgram,
    index: &ValueIndex,
    constants: &[PlanConstant],
    arms: &[(SourceId, PlanRowExpression)],
) -> Option<PlanValueType> {
    let mut output_type = None;
    for (_, value) in arms {
        let Some(value_type) = row_expression_value_type(program, index, constants, value) else {
            continue;
        };
        match output_type {
            Some(existing) if existing != value_type => return None,
            Some(_) => {}
            None => output_type = Some(value_type),
        }
    }
    output_type
}

fn source_event_transform_fresh_value(
    output_type: Option<PlanValueType>,
    constants: &[PlanConstant],
    arms: &[(SourceId, PlanRowExpression)],
) -> PlanConstantValue {
    match output_type {
        Some(PlanValueType::Text) => PlanConstantValue::Text {
            value: String::new(),
        },
        Some(PlanValueType::Number) => PlanConstantValue::Number {
            value: FiniteReal::ZERO,
        },
        Some(PlanValueType::Byte) => PlanConstantValue::Byte { value: 0 },
        Some(PlanValueType::Bool) => PlanConstantValue::Bool { value: false },
        Some(PlanValueType::Bytes { fixed_len }) => {
            let bytes = vec![0; fixed_len.unwrap_or_default() as usize];
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            PlanConstantValue::Bytes {
                byte_len: bytes.len() as u64,
                sha256: format!("{:x}", hasher.finalize()),
                inline_bytes: Some(bytes),
            }
        }
        Some(PlanValueType::Enum) => arms
            .iter()
            .find_map(|(_, value)| {
                let PlanRowExpression::Constant { constant_id } = value else {
                    return None;
                };
                constants
                    .iter()
                    .find(|constant| constant.id == *constant_id)
                    .and_then(|constant| match &constant.value {
                        PlanConstantValue::Enum { .. } => Some(constant.value.clone()),
                        _ => None,
                    })
            })
            .unwrap_or_else(|| PlanConstantValue::Text {
                value: String::new(),
            }),
        Some(PlanValueType::Data) => PlanConstantValue::Data {
            value: boon_data::Value::Null,
        },
        Some(
            PlanValueType::RootInitialField
            | PlanValueType::RowInitialField
            | PlanValueType::Unknown,
        )
        | None => {
            if arms
                .iter()
                .all(|(_, value)| plan_row_expression_is_bool_constant(constants, value))
            {
                PlanConstantValue::Bool { value: false }
            } else {
                PlanConstantValue::Text {
                    value: String::new(),
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn source_event_transform_arm_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    exprs: &[AstExpr],
    source: &str,
) -> Option<PlanRowExpression> {
    let arm =
        source_event_transform_arm_statement(program, derived, exprs, source, &derived.statement)?;
    let arm_expr = expr_by_id(program, arm.expr?)?;
    let ValueRef::Source(source_id) = index.resolve(source)? else {
        return None;
    };
    if let Some(field) = index.source_field_payload_alias(source, &derived.path) {
        let input = ValueRef::SourcePayload { source_id, field };
        if !inputs.contains(&input) {
            inputs.push(input.clone());
        }
        return Some(PlanRowExpression::Field { input });
    }
    let source_port = program
        .sources
        .iter()
        .find(|candidate| candidate.path == source)?;
    let payload_fields = source_port
        .payload_schema
        .fields
        .iter()
        .chain(
            source_port
                .payload_schema
                .typed_fields
                .iter()
                .map(|descriptor| &descriptor.field),
        )
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut env = BTreeMap::new();
    if let Some(scope_id) = source_port.scope_id
        && let Some(list) = program
            .lists
            .iter()
            .find(|list| list.row_scope_id == Some(scope_id))
    {
        let current_row = LoweredRowValue::CurrentRow {
            list_id: plan_list_id(list.id),
        };
        if let Some(scope) = program.row_scopes.iter().find(|scope| scope.id == scope_id) {
            env.insert(scope.row_scope.clone(), current_row.clone());
        }
        let expression_ids = exprs.iter().map(|expr| expr.id).collect::<BTreeSet<_>>();
        for binding in &program.typecheck_report.list_map_bindings {
            if expression_ids.contains(&binding.map_expr_id) {
                env.insert(binding.item_binding_name.clone(), current_row.clone());
            }
        }
    }
    for payload_field in payload_fields {
        let field_name = match &payload_field {
            ir::SourcePayloadField::Address => "address",
            ir::SourcePayloadField::Bytes => "bytes",
            ir::SourcePayloadField::Key => "key",
            ir::SourcePayloadField::Named(name) => name,
            ir::SourcePayloadField::Text => "text",
        };
        let input = ValueRef::SourcePayload {
            source_id,
            field: source_payload_field_from_ir(&payload_field),
        };
        if !inputs.contains(&input) {
            inputs.push(input.clone());
        }
        let value = LoweredRowValue::Scalar(PlanRowExpression::Field { input });
        env.insert(format!("{source}.{field_name}"), value.clone());
        if let Some(relative) = source.strip_prefix("store.") {
            env.insert(format!("{relative}.{field_name}"), value);
        }
    }
    let lowered = match &arm_expr.kind {
        AstExprKind::Then {
            output: Some(output),
            ..
        } => statement_with_expression(arm, *output)
            .filter(|statement| !statement.children.is_empty())
            .and_then(|statement| {
                lower_row_statement_value(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    &mut env,
                    expr_value_types,
                    statement,
                )
            })
            .or_else(|| {
                (!arm.children.is_empty())
                    .then(|| {
                        lower_row_function_body(
                            program,
                            derived,
                            index,
                            constants,
                            inputs,
                            arm,
                            &mut env,
                            expr_value_types,
                        )
                    })
                    .flatten()
            })
            .or_else(|| {
                lower_row_expr(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    &mut env,
                    expr_value_types,
                    *output,
                )
            }),
        AstExprKind::Then { output: None, .. } => lower_row_function_body(
            program,
            derived,
            index,
            constants,
            inputs,
            arm,
            &mut env,
            expr_value_types,
        ),
        _ => lower_row_statement_value(
            program,
            derived,
            index,
            constants,
            inputs,
            &mut env,
            expr_value_types,
            arm,
        ),
    };
    lowered.and_then(lowered_scalar)
}

fn source_event_transform_arm_statement<'a>(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    exprs: &[AstExpr],
    source: &str,
    statement: &'a AstStatement,
) -> Option<&'a AstStatement> {
    if let Some(arm) =
        source_event_transform_direct_arm_statement(program, exprs, source, statement)
    {
        return Some(arm);
    }
    if let Some(arm) = statement.children.iter().find_map(|child| {
        source_event_transform_arm_statement(program, derived, exprs, source, child)
    }) {
        return Some(arm);
    }
    if let Some(expr_id) = statement.expr
        && expression_tree_reaches_source(program, derived, exprs, expr_id, source)
    {
        return source_event_then_continuation(program, statement).or(Some(statement));
    }
    None
}

fn source_event_transform_direct_arm_statement<'a>(
    program: &TypedProgram,
    exprs: &[AstExpr],
    source: &str,
    statement: &'a AstStatement,
) -> Option<&'a AstStatement> {
    if statement
        .expr
        .and_then(|expr_id| expr_by_id(program, expr_id))
        .is_some_and(|expr| {
            matches!(
                &expr.kind,
                AstExprKind::Then { input, .. }
                    if super::expr_tree_mentions_source(exprs, *input, source)
            )
        })
    {
        return Some(statement);
    }
    if let Some(arm) = statement.children.iter().find_map(|child| {
        source_event_transform_direct_arm_statement(program, exprs, source, child)
    }) {
        return Some(arm);
    }
    let expr_id = statement.expr?;
    super::expr_tree_mentions_source(exprs, expr_id, source)
        .then(|| source_event_then_continuation(program, statement).unwrap_or(statement))
}

fn expression_tree_reaches_source(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    exprs: &[AstExpr],
    expr_id: usize,
    source: &str,
) -> bool {
    if super::expr_tree_mentions_source(exprs, expr_id, source) {
        return true;
    }
    let mut ids = BTreeSet::new();
    super::collect_expr_ids(expr_id, exprs, &mut ids);
    exprs.iter().any(|expr| {
        if !ids.contains(&expr.id) {
            return false;
        }
        let Some(path) = expression_path_string(program, expr.id) else {
            return false;
        };
        let path = canonical_sibling_path(&derived.path, &path);
        if path == derived.path {
            return false;
        }
        program.derived_values.iter().any(|candidate| {
            candidate.path == path && candidate.sources.iter().any(|cause| cause == source)
        }) || program.possible_causes.iter().any(|candidate| {
            candidate.target == path && candidate.sources.iter().any(|cause| cause == source)
        })
    })
}

fn source_event_then_continuation<'a>(
    program: &TypedProgram,
    statement: &'a AstStatement,
) -> Option<&'a AstStatement> {
    if statement
        .expr
        .and_then(|expr| expr_by_id(program, expr))
        .is_some_and(|expr| matches!(expr.kind, AstExprKind::Then { .. }))
    {
        return Some(statement);
    }
    statement
        .children
        .iter()
        .find_map(|child| source_event_then_continuation(program, child))
}

fn plan_row_expression_is_bool_constant(
    constants: &[PlanConstant],
    expression: &PlanRowExpression,
) -> bool {
    let PlanRowExpression::Constant { constant_id } = expression else {
        return false;
    };
    constants
        .iter()
        .find(|constant| constant.id == *constant_id)
        .is_some_and(|constant| matches!(constant.value, PlanConstantValue::Bool { .. }))
}

fn source_event_transform_text_arm_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    source: &str,
) -> Option<PlanRowExpression> {
    let expression = super::compiler_source_event_transform_text_expression(
        derived,
        source,
        &program.expressions,
        &program.functions,
    );
    if std::env::var_os("BOON_COMPILER_SOURCE_EVENT_TRACE").is_some() {
        eprintln!(
            "source_event_transform_text_arm path={} source={} expression={expression:?}",
            derived.path, source
        );
    }
    match expression {
        super::CompilerDerivedTextExpression::SourceRootText { path }
        | super::CompilerDerivedTextExpression::EnterKeyRootTextTrimNonEmpty { path } => {
            source_event_transform_text_path_expression(
                program, derived, index, inputs, source, &path,
            )
        }
        _ => {
            let path =
                source_event_transform_final_then_source_text_path(program, derived, source)?;
            if std::env::var_os("BOON_COMPILER_SOURCE_EVENT_TRACE").is_some() {
                eprintln!(
                    "source_event_transform_text_arm final_then path={} source={} text_path={path}",
                    derived.path, source
                );
            }
            source_event_transform_text_path_expression(
                program, derived, index, inputs, source, &path,
            )
        }
    }
}

fn source_event_transform_text_path_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    source: &str,
    path: &str,
) -> Option<PlanRowExpression> {
    let mut input = resolve_update_value_ref(index, source, &derived.path, derived.indexed, path)?;
    if let ValueRef::SourcePayload {
        source_id: payload_source_id,
        field,
    } = &input
        && let Some(backing_state) = source_payload_backing_row_state(
            program,
            index,
            source,
            *payload_source_id,
            field,
            derived.indexed,
        )
    {
        input = backing_state;
    }
    if !inputs.contains(&input) {
        inputs.push(input.clone());
    }
    Some(PlanRowExpression::Field { input })
}

fn source_event_transform_final_then_source_text_path(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    source: &str,
) -> Option<String> {
    let exprs = super::compiler_statement_ast_exprs(&derived.statement, &program.expressions);
    exprs.iter().rev().find_map(|expr| {
        let AstExprKind::Then {
            output: Some(output),
            ..
        } = expr.kind
        else {
            return None;
        };
        let path = expression_path_string(program, output)?;
        matches!(
            source_payload_field_from_path(source, &path, true),
            Some(SourcePayloadField::Text)
        )
        .then_some(path)
    })
}

fn source_payload_backing_row_state(
    program: &TypedProgram,
    index: &ValueIndex,
    source: &str,
    source_id: SourceId,
    field: &SourcePayloadField,
    indexed: bool,
) -> Option<ValueRef> {
    program.update_branches.iter().find_map(|branch| {
        if branch.source != source || branch.indexed != indexed {
            return None;
        }
        if source_payload_field_for_expression(index, source, &branch.expression).as_ref()
            != Some(field)
        {
            return None;
        }
        let Some(ValueRef::Source(branch_source_id)) = index.resolve(&branch.source) else {
            return None;
        };
        if branch_source_id != source_id {
            return None;
        }
        match index.resolve(&branch.target)? {
            ValueRef::State(state_id) => Some(ValueRef::State(state_id)),
            _ => None,
        }
    })
}

fn source_event_transform_default_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    output_type: Option<PlanValueType>,
) -> Option<PlanRowExpression> {
    source_event_transform_default_expression_in_statement(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        output_type,
        &derived.statement,
    )
}

fn source_event_transform_default_expression_in_statement(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    output_type: Option<PlanValueType>,
    statement: &AstStatement,
) -> Option<PlanRowExpression> {
    for child in &statement.children {
        if source_event_transform_statement_mentions_source(
            program,
            derived,
            child,
            &derived.sources,
        ) {
            continue;
        }

        let mut candidate_constants = constants.clone();
        let mut candidate_inputs = inputs.clone();
        let mut candidate_env = env.clone();
        if let Some(value) = lower_row_statement_value(
            program,
            derived,
            index,
            &mut candidate_constants,
            &mut candidate_inputs,
            &mut candidate_env,
            expr_value_types,
            child,
        )
        .and_then(lowered_scalar)
        .filter(|value| {
            let candidate_type =
                row_expression_value_type(program, index, &candidate_constants, value);
            match output_type {
                Some(expected) => candidate_type == Some(expected),
                None => candidate_type.is_some(),
            }
        }) {
            *constants = candidate_constants;
            *inputs = candidate_inputs;
            *env = candidate_env;
            return Some(value);
        }
        if let Some(value) = source_event_transform_default_expression_in_statement(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            output_type,
            child,
        ) {
            return Some(value);
        }
    }
    None
}

fn source_event_transform_statement_mentions_source(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    statement: &AstStatement,
    sources: &[String],
) -> bool {
    let exprs = super::compiler_statement_ast_exprs(statement, &program.expressions);
    statement.expr.is_some_and(|expr_id| {
        sources
            .iter()
            .any(|source| expression_tree_reaches_source(program, derived, &exprs, expr_id, source))
    })
}

fn row_expression_from_compiler_field_value(
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    value: super::CompilerFieldValue,
) -> PlanRowExpression {
    let value = match value {
        super::CompilerFieldValue::Text(value) => PlanConstantValue::Text { value },
        super::CompilerFieldValue::Bool(value) => PlanConstantValue::Bool { value },
    };
    row_constant_expression(constants, inputs, value)
}

fn source_key_text_trim_non_empty_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
) -> Option<PlanDerivedExpression> {
    if derived.kind != DerivedValueKind::SourceEventTransform || derived.sources.len() != 1 {
        return None;
    }
    let source = derived.sources.first()?;
    let source_id = match index.resolve(source)? {
        ValueRef::Source(source_id) => source_id,
        _ => return None,
    };
    if let Some(expression) = source_key_text_trim_non_empty_runtime_expression(
        program, derived, index, inputs, source, source_id,
    ) {
        return Some(expression);
    }
    let source_event_statement = derived.statement.children.first()?;
    let AstExprKind::When { input } = &expr_by_id(program, source_event_statement.expr?)?.kind
    else {
        return None;
    };
    let payload_path = expression_path_string(program, *input)?;
    let key_field = source_payload_field_from_path(source, &payload_path, true)?;
    if key_field != SourcePayloadField::Key || !index.source_has_payload_field(source, &key_field) {
        return None;
    }
    let enter_arm = match_arm_child(source_event_statement, "Enter", program)?;
    let inner_expr_id = match_arm_output_id(program, enter_arm)?;
    let inner_statement = enter_arm
        .children
        .iter()
        .find(|statement| statement.expr == Some(inner_expr_id))?;
    let AstExprKind::When { input: trim_input } = &expr_by_id(program, inner_expr_id)?.kind else {
        return None;
    };
    let state_path = text_trim_input_path(program, *trim_input, &derived.path)?;
    let state =
        match resolve_update_value_ref(index, source, &derived.path, derived.indexed, &state_path)?
        {
            ValueRef::State(state_id) => ValueRef::State(state_id),
            ValueRef::SourcePayload {
                source_id,
                field: SourcePayloadField::Text,
            } => ValueRef::SourcePayload {
                source_id,
                field: SourcePayloadField::Text,
            },
            _ => return None,
        };
    if !when_has_empty_skip_and_passthrough(inner_statement, program) {
        return None;
    }
    let payload_ref = ValueRef::SourcePayload {
        source_id,
        field: key_field.clone(),
    };
    let source_ref = ValueRef::Source(source_id);
    if !inputs.contains(&source_ref) {
        inputs.push(source_ref);
    }
    if !inputs.contains(&payload_ref) {
        inputs.push(payload_ref);
    }
    if !inputs.contains(&state) {
        inputs.push(state.clone());
    }
    Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
        source_id,
        key_field,
        required_key: "Enter".to_owned(),
        state,
        skip_empty: true,
    })
}

fn source_key_text_trim_non_empty_runtime_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    source: &str,
    source_id: SourceId,
) -> Option<PlanDerivedExpression> {
    if !index.source_has_payload_field(source, &SourcePayloadField::Key) {
        return None;
    }
    let state = match super::compiler_source_event_transform_text_expression(
        derived,
        source,
        &program.expressions,
        &program.functions,
    ) {
        super::CompilerDerivedTextExpression::EnterKeyPayloadTextTrimNonEmpty => {
            if !index.source_has_payload_field(source, &SourcePayloadField::Text) {
                return None;
            }
            ValueRef::SourcePayload {
                source_id,
                field: SourcePayloadField::Text,
            }
        }
        super::CompilerDerivedTextExpression::EnterKeyRootTextTrimNonEmpty { path } => {
            match resolve_update_value_ref(index, source, &derived.path, derived.indexed, &path)? {
                ValueRef::State(state_id) => ValueRef::State(state_id),
                ValueRef::SourcePayload {
                    source_id,
                    field: SourcePayloadField::Text,
                } => ValueRef::SourcePayload {
                    source_id,
                    field: SourcePayloadField::Text,
                },
                _ => return None,
            }
        }
        _ => return None,
    };
    let key_field = SourcePayloadField::Key;
    let payload_ref = ValueRef::SourcePayload {
        source_id,
        field: key_field.clone(),
    };
    let source_ref = ValueRef::Source(source_id);
    if !inputs.contains(&source_ref) {
        inputs.push(source_ref);
    }
    if !inputs.contains(&payload_ref) {
        inputs.push(payload_ref);
    }
    if !inputs.contains(&state) {
        inputs.push(state.clone());
    }
    Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
        source_id,
        key_field,
        required_key: "Enter".to_owned(),
        state,
        skip_empty: true,
    })
}

fn bool_not_derived_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
) -> Option<PlanDerivedExpression> {
    if derived.kind != DerivedValueKind::Pure {
        return None;
    }
    let statement = derived.statement.children.first()?;
    let expr = expr_by_id(program, statement.expr?)?;
    let input_path = match &expr.kind {
        AstExprKind::Pipe { input, op, .. } if op == "Bool/not" => {
            expression_path_string(program, *input)?
        }
        AstExprKind::Call { function, args } if function == "Bool/not" => {
            expression_path_string(program, args.first()?.value)?
        }
        _ => return None,
    };
    let canonical_path = canonical_sibling_path(&derived.path, &input_path);
    let input = index.resolve(&canonical_path)?;
    inputs.push(input.clone());
    Some(PlanDerivedExpression::BoolNot { input })
}

fn number_compare_const_derived_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
) -> Option<PlanDerivedExpression> {
    if derived.kind != DerivedValueKind::Pure {
        return None;
    }
    let statement = derived.statement.children.first()?;
    let expr = expr_by_id(program, statement.expr?)?;
    let AstExprKind::Infix { left, op, right } = &expr.kind else {
        return None;
    };
    if !matches!(op.as_str(), ">" | ">=" | "<" | "<=" | "==" | "!=") {
        return None;
    }
    let left_path = expression_path_string(program, *left)?;
    let right_expr = expr_by_id(program, *right)?;
    let AstExprKind::Number(right_value) = &right_expr.kind else {
        return None;
    };
    let right = right_value.parse::<FiniteReal>().ok()?;
    let canonical_path = canonical_sibling_path(&derived.path, &left_path);
    let left = index.resolve(&canonical_path)?;
    inputs.push(left.clone());
    Some(PlanDerivedExpression::NumberCompareConst {
        left,
        op: op.clone(),
        right,
    })
}

fn root_bool_derived_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
) -> Option<PlanDerivedExpression> {
    if derived.kind != DerivedValueKind::Pure || derived.indexed {
        return None;
    }
    let statement = derived.statement.children.first()?;
    lower_root_bool_expr(program, &derived.path, index, inputs, statement.expr?)
}

fn lower_root_bool_expr(
    program: &TypedProgram,
    derived_path: &str,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    expr_id: usize,
) -> Option<PlanDerivedExpression> {
    let expr = expr_by_id(program, expr_id)?;
    match &expr.kind {
        AstExprKind::Pipe { input, op, args } if op == "Bool/and" => {
            let right = args.first()?.value;
            Some(PlanDerivedExpression::BoolAnd {
                left: Box::new(lower_root_bool_expr(
                    program,
                    derived_path,
                    index,
                    inputs,
                    *input,
                )?),
                right: Box::new(lower_root_bool_expr(
                    program,
                    derived_path,
                    index,
                    inputs,
                    right,
                )?),
            })
        }
        AstExprKind::Call { function, args } if function == "Bool/and" => {
            let left = args.first()?.value;
            let right = args.get(1)?.value;
            Some(PlanDerivedExpression::BoolAnd {
                left: Box::new(lower_root_bool_expr(
                    program,
                    derived_path,
                    index,
                    inputs,
                    left,
                )?),
                right: Box::new(lower_root_bool_expr(
                    program,
                    derived_path,
                    index,
                    inputs,
                    right,
                )?),
            })
        }
        AstExprKind::Pipe { input, op, .. } if op == "Bool/not" => {
            Some(PlanDerivedExpression::BoolNotExpression {
                input: Box::new(lower_root_bool_expr(
                    program,
                    derived_path,
                    index,
                    inputs,
                    *input,
                )?),
            })
        }
        AstExprKind::Call { function, args } if function == "Bool/not" => {
            Some(PlanDerivedExpression::BoolNotExpression {
                input: Box::new(lower_root_bool_expr(
                    program,
                    derived_path,
                    index,
                    inputs,
                    args.first()?.value,
                )?),
            })
        }
        AstExprKind::Infix { left, op, right }
            if matches!(op.as_str(), ">" | ">=" | "<" | "<=" | "==" | "!=") =>
        {
            let left_path = expression_path_string(program, *left)?;
            if let Some(right_path) = expression_path_string(program, *right) {
                let left = index.resolve(&canonical_sibling_path(derived_path, &left_path))?;
                let right = index.resolve(&canonical_sibling_path(derived_path, &right_path))?;
                if !inputs.contains(&left) {
                    inputs.push(left.clone());
                }
                if !inputs.contains(&right) {
                    inputs.push(right.clone());
                }
                return Some(PlanDerivedExpression::ValueCompare {
                    left,
                    op: op.clone(),
                    right,
                });
            }
            let right_expr = expr_by_id(program, *right)?;
            let AstExprKind::Number(right_value) = &right_expr.kind else {
                return None;
            };
            let right = right_value.parse::<FiniteReal>().ok()?;
            let canonical_path = canonical_sibling_path(derived_path, &left_path);
            let left = index.resolve(&canonical_path)?;
            if !inputs.contains(&left) {
                inputs.push(left.clone());
            }
            Some(PlanDerivedExpression::NumberCompareConst {
                left,
                op: op.clone(),
                right,
            })
        }
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LoweredRowValue {
    Scalar(PlanRowExpression),
    ListMapItem {
        binding: String,
        list_id: Option<ListId>,
    },
    CurrentRow {
        list_id: ListId,
    },
    ListRow {
        list_id: ListId,
        index: PlanRowExpression,
    },
    ListFindRow {
        list_id: ListId,
        field: FieldId,
        value: PlanRowExpression,
    },
}

const ROW_PREVIOUS_BINDING: &str = "$boon$row_previous";

fn row_expression_for_value(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
) -> Option<PlanDerivedExpression> {
    if !matches!(
        derived.kind,
        DerivedValueKind::Pure | DerivedValueKind::ListView
    ) {
        return None;
    }
    let mut local_constants = constants.clone();
    let mut local_inputs = inputs.clone();
    let mut env = BTreeMap::new();
    if let Some(scope) = derived
        .scope_id
        .and_then(|scope_id| program.row_scopes.iter().find(|scope| scope.id == scope_id))
        && let Some(list) = program
            .lists
            .iter()
            .find(|list| list.row_scope_id == Some(scope.id))
    {
        env.insert(
            scope.row_scope.clone(),
            LoweredRowValue::CurrentRow {
                list_id: plan_list_id(list.id),
            },
        );
    }
    let expr_value_types = expression_value_type_lookup(program);
    let value = lower_row_statement_value(
        program,
        derived,
        index,
        &mut local_constants,
        &mut local_inputs,
        &mut env,
        &expr_value_types,
        &derived.statement,
    )?;
    let LoweredRowValue::Scalar(expression) = value else {
        return None;
    };
    *constants = local_constants;
    *inputs = local_inputs;
    Some(PlanDerivedExpression::RowExpression { expression })
}

fn lower_row_expr(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    expr_id: usize,
) -> Option<LoweredRowValue> {
    let expr = expr_by_id(program, expr_id)?;
    if matches!(&expr.kind, AstExprKind::Call { args, .. } if args.is_empty())
        && let Some(statement) = statement_for_expression(program, expr_id)
        && statement
            .children
            .iter()
            .any(|child| matches!(child.kind, AstStatementKind::Field { .. }))
        && let Some(value) = lower_row_call_statement_with_field_args(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            statement,
            expr_id,
        )
    {
        return Some(value);
    }
    match &expr.kind {
        AstExprKind::Delimiter => env.get(ROW_PREVIOUS_BINDING).cloned(),
        AstExprKind::Drain { .. } => env.get(&migration_drain_environment_key(expr_id)).cloned(),
        AstExprKind::Identifier(name) => env
            .get(name)
            .cloned()
            .or_else(|| {
                row_field_expression(program, derived, index, inputs, name)
                    .map(LoweredRowValue::Scalar)
            })
            .or_else(|| unbound_identifier_literal(constants, inputs, name)),
        AstExprKind::Path(parts) if parts.len() == 1 => {
            let name = parts.first()?;
            env.get(name)
                .cloned()
                .or_else(|| {
                    row_field_expression(program, derived, index, inputs, name)
                        .map(LoweredRowValue::Scalar)
                })
                .or_else(|| unbound_identifier_literal(constants, inputs, name))
        }
        AstExprKind::Path(parts) if parts.len() == 2 => {
            let path = parts.join(".");
            if let Some(value) = env.get(&path).cloned() {
                return Some(value);
            }
            if let Some(value) = env.get(&parts[0]).cloned() {
                return match value {
                    LoweredRowValue::CurrentRow { list_id } => {
                        let field = row_input_field_id_for_list_id(program, list_id, &parts[1])?;
                        let input = ValueRef::Field(field);
                        if !inputs.contains(&input) {
                            inputs.push(input.clone());
                        }
                        Some(LoweredRowValue::Scalar(PlanRowExpression::Field { input }))
                    }
                    LoweredRowValue::ListRow { list_id, index } => {
                        let field = row_field_id_for_list_id(program, list_id, &parts[1])?;
                        Some(LoweredRowValue::Scalar(PlanRowExpression::ListGetField {
                            list_id,
                            index: Box::new(index),
                            field,
                        }))
                    }
                    LoweredRowValue::ListFindRow {
                        list_id,
                        field,
                        value,
                    } => {
                        let target = row_field_id_for_list_id(program, list_id, &parts[1])?;
                        Some(LoweredRowValue::Scalar(PlanRowExpression::ListFindValue {
                            list_id,
                            field,
                            value: Box::new(value),
                            target,
                            fallback: None,
                        }))
                    }
                    LoweredRowValue::ListMapItem { binding, list_id } => {
                        let row = PlanRowExpression::ListMapItem { binding };
                        if let Some(list_id) = list_id {
                            let field = row_field_id_for_list_id(program, list_id, &parts[1])
                                .or_else(|| {
                                    row_input_field_id_for_list_id(program, list_id, &parts[1])
                                })?;
                            Some(LoweredRowValue::Scalar(PlanRowExpression::ListRowField {
                                row: Box::new(row),
                                list_id,
                                field,
                            }))
                        } else {
                            Some(LoweredRowValue::Scalar(PlanRowExpression::ObjectField {
                                object: Box::new(row),
                                field: parts[1].clone(),
                            }))
                        }
                    }
                    LoweredRowValue::Scalar(object) => {
                        Some(LoweredRowValue::Scalar(PlanRowExpression::ObjectField {
                            object: Box::new(object),
                            field: parts[1].clone(),
                        }))
                    }
                };
            }
            let object = (|| {
                let (parent, _) = derived.path.rsplit_once('.')?;
                let (grandparent, _) = parent.rsplit_once('.')?;
                let candidate = format!("{grandparent}.{}", parts[0]);
                row_field_expression(program, derived, index, inputs, &candidate)
            })()
            .or_else(|| row_field_expression(program, derived, index, inputs, &parts[0]));
            if let Some(object) = object {
                return Some(LoweredRowValue::Scalar(PlanRowExpression::ObjectField {
                    object: Box::new(object),
                    field: parts[1].clone(),
                }));
            }
            row_field_expression(program, derived, index, inputs, &path)
                .map(LoweredRowValue::Scalar)
        }
        AstExprKind::Path(parts) if parts.len() > 2 => {
            let mut value = env.get(parts.first()?)?.clone();
            for field in &parts[1..] {
                value = lower_row_value_field(program, inputs, value, field)?;
            }
            Some(value)
        }
        AstExprKind::Path(parts) => {
            let path = parts.join(".");
            env.get(&path).cloned().or_else(|| {
                row_field_expression(program, derived, index, inputs, &path)
                    .map(LoweredRowValue::Scalar)
            })
        }
        AstExprKind::When { input } => lower_inline_row_select(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            expr,
            *input,
        ),
        AstExprKind::Number(value) => {
            let value = plan_number_constant(value)?;
            Some(LoweredRowValue::Scalar(row_constant_expression(
                constants, inputs, value,
            )))
        }
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
            Some(LoweredRowValue::Scalar(row_constant_expression(
                constants,
                inputs,
                PlanConstantValue::Text {
                    value: value.clone(),
                },
            )))
        }
        AstExprKind::Bool(value) => Some(LoweredRowValue::Scalar(row_constant_expression(
            constants,
            inputs,
            PlanConstantValue::Bool { value: *value },
        ))),
        AstExprKind::ByteLiteral { value, .. } => Some(LoweredRowValue::Scalar(
            row_constant_expression(constants, inputs, PlanConstantValue::Byte { value: *value }),
        )),
        AstExprKind::BytesLiteral { size: _, items } => {
            let bytes = row_static_bytes_literal(program, items)?;
            Some(LoweredRowValue::Scalar(row_bytes_constant_expression(
                constants, inputs, bytes,
            )))
        }
        AstExprKind::Enum(value) | AstExprKind::Tag(value) => {
            Some(LoweredRowValue::Scalar(row_constant_expression(
                constants,
                inputs,
                PlanConstantValue::Enum {
                    value: value.clone(),
                },
            )))
        }
        AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
            let mut object_fields = Vec::with_capacity(fields.len());
            for field in fields {
                if field.spread {
                    return None;
                }
                let value = lower_row_expr(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    field.value,
                )
                .and_then(lowered_scalar)?;
                object_fields.push(PlanRowObjectField {
                    name: field.name.clone(),
                    value,
                });
            }
            Some(LoweredRowValue::Scalar(PlanRowExpression::Object {
                fields: object_fields,
            }))
        }
        AstExprKind::ListLiteral { items, .. } => {
            let mut lowered_items = Vec::with_capacity(items.len());
            for item in items {
                lowered_items.push(
                    lower_row_expr(
                        program,
                        derived,
                        index,
                        constants,
                        inputs,
                        env,
                        expr_value_types,
                        *item,
                    )
                    .and_then(lowered_scalar)?,
                );
            }
            Some(LoweredRowValue::Scalar(PlanRowExpression::ListLiteral {
                items: lowered_items,
            }))
        }
        AstExprKind::Infix { left, op, right } if op == "+" => {
            let left_expr_id = *left;
            let right_expr_id = *right;
            let expression_value_type =
                inferred_expression_value_type(program, expr_id, expr_value_types);
            let left = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                *left,
            )?;
            let right = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                *right,
            )?;
            let left_value_type =
                lowered_row_value_type(program, index, constants, &left).or_else(|| {
                    inferred_expression_value_type(program, left_expr_id, expr_value_types)
                });
            let right_value_type = lowered_row_value_type(program, index, constants, &right)
                .or_else(|| {
                    inferred_expression_value_type(program, right_expr_id, expr_value_types)
                });
            match (expression_value_type, left_value_type, right_value_type) {
                (_, Some(PlanValueType::Number), Some(PlanValueType::Number)) => {
                    Some(LoweredRowValue::Scalar(PlanRowExpression::NumberInfix {
                        op: op.clone(),
                        left: Box::new(lowered_scalar(left)?),
                        right: Box::new(lowered_scalar(right)?),
                    }))
                }
                (Some(PlanValueType::Text), _, _)
                | (_, Some(PlanValueType::Text), _)
                | (_, _, Some(PlanValueType::Text)) => {
                    Some(LoweredRowValue::Scalar(PlanRowExpression::TextConcat {
                        parts: vec![lowered_scalar(left)?, lowered_scalar(right)?],
                    }))
                }
                (Some(PlanValueType::Number), _, _) => {
                    Some(LoweredRowValue::Scalar(PlanRowExpression::NumberInfix {
                        op: op.clone(),
                        left: Box::new(lowered_scalar(left)?),
                        right: Box::new(lowered_scalar(right)?),
                    }))
                }
                _ => Some(LoweredRowValue::Scalar(PlanRowExpression::NumberInfix {
                    op: op.clone(),
                    left: Box::new(lowered_scalar(left)?),
                    right: Box::new(lowered_scalar(right)?),
                })),
            }
        }
        AstExprKind::Infix { left, op, right }
            if matches!(
                op.as_str(),
                "%" | "/" | "-" | "*" | ">" | ">=" | "<" | "<=" | "==" | "!="
            ) =>
        {
            let left = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                *left,
            )?;
            let right = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                *right,
            )?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::NumberInfix {
                op: op.clone(),
                left: Box::new(lowered_scalar(left)?),
                right: Box::new(lowered_scalar(right)?),
            }))
        }
        AstExprKind::Call { function, args } if function == "List/get" => lower_row_list_get(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            args,
        ),
        AstExprKind::Call { function, args } if row_list_builtin(function) => {
            lower_row_list_builtin(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                function,
                None,
                args,
            )
        }
        AstExprKind::Call { function, args } if row_text_builtin(function) => {
            lower_row_text_builtin(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                function,
                None,
                args,
            )
        }
        AstExprKind::Call { function, args } if row_generic_builtin(function) => {
            lower_row_builtin_call(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                function,
                None,
                args,
            )
        }
        AstExprKind::Call { function, args } => lower_row_function_call(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            function,
            args,
        ),
        AstExprKind::Pipe { input, op, args } if op == "List/get" => {
            let mut call_args = Vec::with_capacity(args.len() + 1);
            call_args.push(AstCallArg {
                name: None,
                value: *input,
                start: expr.start,
                end: expr.end,
            });
            call_args.extend(args.iter().cloned());
            lower_row_list_get(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                &call_args,
            )
        }
        AstExprKind::Pipe { input, op, args } if row_list_builtin(op) => lower_row_list_builtin(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            op,
            Some(*input),
            args,
        ),
        AstExprKind::Pipe { input, op, args } if row_text_builtin(op) => lower_row_text_builtin(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            op,
            Some(*input),
            args,
        ),
        AstExprKind::Pipe { input, op, args } if row_generic_builtin(op) => lower_row_builtin_call(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            op,
            Some(*input),
            args,
        ),
        AstExprKind::Pipe { input, op, args } => {
            let mut call_args = Vec::with_capacity(args.len() + 1);
            call_args.push(AstCallArg {
                name: None,
                value: *input,
                start: expr.start,
                end: expr.end,
            });
            call_args.extend(args.iter().cloned());
            lower_row_function_call(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                op,
                &call_args,
            )
        }
        _ => None,
    }
}

fn lower_row_value_field(
    program: &TypedProgram,
    inputs: &mut Vec<ValueRef>,
    value: LoweredRowValue,
    field_name: &str,
) -> Option<LoweredRowValue> {
    match value {
        LoweredRowValue::CurrentRow { list_id } => {
            let field = row_input_field_id_for_list_id(program, list_id, field_name)?;
            let input = ValueRef::Field(field);
            if !inputs.contains(&input) {
                inputs.push(input.clone());
            }
            Some(LoweredRowValue::Scalar(PlanRowExpression::Field { input }))
        }
        LoweredRowValue::ListRow { list_id, index } => {
            let field = row_field_id_for_list_id(program, list_id, field_name)?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::ListGetField {
                list_id,
                index: Box::new(index),
                field,
            }))
        }
        LoweredRowValue::ListFindRow {
            list_id,
            field,
            value,
        } => {
            let target = row_field_id_for_list_id(program, list_id, field_name)?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::ListFindValue {
                list_id,
                field,
                value: Box::new(value),
                target,
                fallback: None,
            }))
        }
        LoweredRowValue::ListMapItem { binding, list_id } => {
            let row = PlanRowExpression::ListMapItem { binding };
            let expression = if let Some(list_id) = list_id {
                let field = row_field_id_for_list_id(program, list_id, field_name)
                    .or_else(|| row_input_field_id_for_list_id(program, list_id, field_name))?;
                PlanRowExpression::ListRowField {
                    row: Box::new(row),
                    list_id,
                    field,
                }
            } else {
                PlanRowExpression::ObjectField {
                    object: Box::new(row),
                    field: field_name.to_owned(),
                }
            };
            Some(LoweredRowValue::Scalar(expression))
        }
        LoweredRowValue::Scalar(object) => {
            Some(LoweredRowValue::Scalar(PlanRowExpression::ObjectField {
                object: Box::new(object),
                field: field_name.to_owned(),
            }))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_inline_row_select(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    select: &AstExpr,
    input: usize,
) -> Option<LoweredRowValue> {
    let input = lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        input,
    )
    .and_then(lowered_scalar)?;
    let candidates = program
        .expressions
        .iter()
        .filter(|candidate| {
            candidate.start >= select.start
                && candidate.end <= select.end
                && matches!(candidate.kind, AstExprKind::MatchArm { .. })
        })
        .collect::<Vec<_>>();
    let mut direct = candidates
        .iter()
        .copied()
        .filter(|candidate| {
            !candidates.iter().any(|parent| {
                parent.id != candidate.id
                    && parent.start <= candidate.start
                    && candidate.end <= parent.end
            })
        })
        .collect::<Vec<_>>();
    direct.sort_by_key(|arm| arm.start);
    let mut arms = Vec::new();
    for arm in direct {
        let AstExprKind::MatchArm { pattern, output } = &arm.kind else {
            continue;
        };
        let output = (*output)?;
        let (pattern, binding) = row_select_pattern_and_binding(pattern)?;
        let mut arm_env = env.clone();
        if let Some(binding) = binding {
            arm_env.insert(binding, LoweredRowValue::Scalar(input.clone()));
        }
        let value = lower_row_expr(
            program,
            derived,
            index,
            constants,
            inputs,
            &mut arm_env,
            expr_value_types,
            output,
        )
        .and_then(lowered_scalar)?;
        arms.push(PlanRowSelectArm { pattern, value });
    }
    (!arms.is_empty()).then_some(LoweredRowValue::Scalar(PlanRowExpression::Select {
        input: Box::new(input),
        arms,
    }))
}

#[allow(clippy::too_many_arguments)]
fn lower_row_when_statement(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    statement: &AstStatement,
    expr_id: usize,
) -> Option<LoweredRowValue> {
    let expr = expr_by_id(program, expr_id)?;
    let AstExprKind::When { input } = expr.kind else {
        return None;
    };
    if statement.children.is_empty() {
        return None;
    }
    let input = lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        input,
    )
    .and_then(lowered_scalar)?;
    let mut arms = Vec::new();
    for child in &statement.children {
        let Some(child_expr) = child.expr else {
            continue;
        };
        let arm = expr_by_id(program, child_expr)?;
        let AstExprKind::MatchArm { pattern, output } = &arm.kind else {
            continue;
        };
        let (pattern, binding) = row_select_pattern_and_binding(pattern)?;
        let mut arm_env = env.clone();
        if let Some(binding) = binding {
            arm_env.insert(binding, LoweredRowValue::Scalar(input.clone()));
        }
        let output = output
            .and_then(|output| {
                lower_row_expr(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    &mut arm_env,
                    expr_value_types,
                    output,
                )
            })
            .or_else(|| {
                child.children.first().and_then(|output| {
                    lower_row_statement_value(
                        program,
                        derived,
                        index,
                        constants,
                        inputs,
                        &mut arm_env,
                        expr_value_types,
                        output,
                    )
                })
            })
            .and_then(lowered_scalar)?;
        arms.push(PlanRowSelectArm {
            pattern,
            value: output,
        });
    }
    (!arms.is_empty()).then_some(LoweredRowValue::Scalar(PlanRowExpression::Select {
        input: Box::new(input),
        arms,
    }))
}

fn lowered_scalar(value: LoweredRowValue) -> Option<PlanRowExpression> {
    match value {
        LoweredRowValue::Scalar(expression) => Some(expression),
        LoweredRowValue::ListMapItem { binding, .. } => {
            Some(PlanRowExpression::ListMapItem { binding })
        }
        LoweredRowValue::CurrentRow { .. }
        | LoweredRowValue::ListRow { .. }
        | LoweredRowValue::ListFindRow { .. } => None,
    }
}

fn row_expression_direct_list_id(expression: &PlanRowExpression) -> Option<ListId> {
    match expression {
        PlanRowExpression::ListRef { list_id } => Some(*list_id),
        _ => None,
    }
}

fn lowered_row_value_type(
    program: &TypedProgram,
    index: &ValueIndex,
    constants: &[PlanConstant],
    value: &LoweredRowValue,
) -> Option<PlanValueType> {
    match value {
        LoweredRowValue::Scalar(expression) => {
            row_expression_value_type(program, index, constants, expression)
        }
        LoweredRowValue::ListMapItem { .. } => None,
        LoweredRowValue::CurrentRow { .. }
        | LoweredRowValue::ListRow { .. }
        | LoweredRowValue::ListFindRow { .. } => None,
    }
}

fn row_expression_value_type(
    program: &TypedProgram,
    index: &ValueIndex,
    constants: &[PlanConstant],
    expression: &PlanRowExpression,
) -> Option<PlanValueType> {
    match expression {
        PlanRowExpression::Field { input } => plan_value_type_for_value_ref(program, index, input),
        PlanRowExpression::Constant { constant_id } => constants
            .iter()
            .find(|constant| constant.id == *constant_id)
            .map(|constant| match constant.value {
                PlanConstantValue::Text { .. } => PlanValueType::Text,
                PlanConstantValue::Number { .. } => PlanValueType::Number,
                PlanConstantValue::Byte { .. } => PlanValueType::Byte,
                PlanConstantValue::Bool { .. } => PlanValueType::Bool,
                PlanConstantValue::Bytes { byte_len, .. } => PlanValueType::Bytes {
                    fixed_len: Some(byte_len),
                },
                PlanConstantValue::Enum { .. } => PlanValueType::Enum,
                PlanConstantValue::Data { .. } => PlanValueType::Data,
            }),
        PlanRowExpression::TextTrim { .. }
        | PlanRowExpression::TextSubstring { .. }
        | PlanRowExpression::TextConcat { .. }
        | PlanRowExpression::BytesToText { .. }
        | PlanRowExpression::BytesToHex { .. }
        | PlanRowExpression::BytesToBase64 { .. } => Some(PlanValueType::Text),
        PlanRowExpression::TextToBytes { .. }
        | PlanRowExpression::BytesSlice { .. }
        | PlanRowExpression::BytesTake { .. }
        | PlanRowExpression::BytesDrop { .. }
        | PlanRowExpression::BytesZeros { .. }
        | PlanRowExpression::BytesSet { .. }
        | PlanRowExpression::BytesWriteUnsigned { .. }
        | PlanRowExpression::BytesWriteSigned { .. }
        | PlanRowExpression::BytesConcat { .. }
        | PlanRowExpression::BytesFromHex { .. }
        | PlanRowExpression::BytesFromBase64 { .. } => {
            Some(PlanValueType::Bytes { fixed_len: None })
        }
        PlanRowExpression::BytesLength { .. }
        | PlanRowExpression::BytesFind { .. }
        | PlanRowExpression::BytesReadUnsigned { .. }
        | PlanRowExpression::BytesReadSigned { .. } => Some(PlanValueType::Number),
        PlanRowExpression::BytesGet { .. } => Some(PlanValueType::Byte),
        PlanRowExpression::BytesIsEmpty { .. }
        | PlanRowExpression::BytesStartsWith { .. }
        | PlanRowExpression::BytesEndsWith { .. }
        | PlanRowExpression::BytesEqual { .. } => Some(PlanValueType::Bool),
        PlanRowExpression::TextIsEmpty { .. } | PlanRowExpression::TextStartsWith { .. } => {
            Some(PlanValueType::Bool)
        }
        PlanRowExpression::TextLength { .. }
        | PlanRowExpression::TextToNumber { .. }
        | PlanRowExpression::NumberInfix { .. }
        | PlanRowExpression::ListSum { .. } => Some(PlanValueType::Number),
        PlanRowExpression::BuiltinCall { function, .. } => match function.as_str() {
            "Text/empty" | "Error/text" | "Router/route" => Some(PlanValueType::Text),
            "List/count" | "List/length" => Some(PlanValueType::Number),
            "List/join_field" => Some(PlanValueType::Text),
            _ => None,
        },
        PlanRowExpression::Select { arms, .. } => {
            let mut arm_types = arms
                .iter()
                .filter_map(|arm| row_expression_value_type(program, index, constants, &arm.value));
            let first = arm_types.next()?;
            arm_types.all(|arm_type| arm_type == first).then_some(first)
        }
        PlanRowExpression::ListGetField { field, .. }
        | PlanRowExpression::ListFindValue { target: field, .. }
        | PlanRowExpression::ListRowField { field, .. } => index.field_value_type(*field).copied(),
        PlanRowExpression::ListRef { .. }
        | PlanRowExpression::ListRange { .. }
        | PlanRowExpression::ListLiteral { .. }
        | PlanRowExpression::ListMap { .. }
        | PlanRowExpression::ListMapItem { .. }
        | PlanRowExpression::Object { .. }
        | PlanRowExpression::ObjectField { .. } => None,
    }
}

fn lower_row_number_expr(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    expr_id: usize,
) -> Option<PlanRowExpression> {
    let expr = expr_by_id(program, expr_id)?;
    if let AstExprKind::ByteLiteral { value, .. } = &expr.kind {
        return Some(row_constant_expression(
            constants,
            inputs,
            PlanConstantValue::Number {
                value: FiniteReal::new(f64::from(*value)).ok()?,
            },
        ));
    }
    if let AstExprKind::Infix { left, op, right } = &expr.kind
        && matches!(op.as_str(), "+" | "-" | "*" | "/" | "%")
    {
        return Some(PlanRowExpression::NumberInfix {
            op: op.clone(),
            left: Box::new(lower_row_number_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                *left,
            )?),
            right: Box::new(lower_row_number_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                *right,
            )?),
        });
    }
    lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        expr_id,
    )
    .and_then(lowered_scalar)
}

fn list_id_for_semantic_list_memory_field(
    program: &TypedProgram,
    field_id: FieldId,
) -> Option<ListId> {
    let field = program
        .semantic_index
        .fields
        .iter()
        .find(|field| plan_field_id(field.id) == field_id && field.kind == "list_memory")?;
    let local = field.path.rsplit_once('.').map(|(_, local)| local);
    program
        .lists
        .iter()
        .find(|list| {
            list.name == field.path
                || local.is_some_and(|local| {
                    list.name == local
                        || list
                            .name
                            .rsplit_once('.')
                            .is_some_and(|(_, list_local)| list_local == local)
                })
        })
        .map(|list| plan_list_id(list.id))
}

fn lower_row_statement_value(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    statement: &AstStatement,
) -> Option<LoweredRowValue> {
    if let Some(expr_id) = statement.expr {
        if !statement.children.is_empty()
            && let AstExprKind::Pipe { input, op, args } = &expr_by_id(program, expr_id)?.kind
            && row_list_builtin(op)
            && let Some(multiline_args) = row_list_multiline_args(program, op, args, statement)
        {
            return lower_row_list_builtin(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                op,
                Some(*input),
                &multiline_args,
            );
        }
        if !statement.children.is_empty()
            && let Some(value) = lower_row_call_statement_with_field_args(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                statement,
                expr_id,
            )
        {
            return Some(value);
        }
        if let Some(value) = lower_row_while_statement(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            statement,
            expr_id,
        ) {
            return Some(value);
        }
        if let Some(value) = lower_row_when_statement(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            statement,
            expr_id,
        ) {
            return Some(value);
        }
        if matches!(
            expr_by_id(program, expr_id)?.kind,
            AstExprKind::ListLiteral { .. }
        ) && !statement.children.is_empty()
        {
            let mut items = Vec::with_capacity(statement.children.len());
            for child in &statement.children {
                items.push(
                    lower_row_statement_value(
                        program,
                        derived,
                        index,
                        constants,
                        inputs,
                        env,
                        expr_value_types,
                        child,
                    )
                    .and_then(lowered_scalar)?,
                );
            }
            return Some(LoweredRowValue::Scalar(PlanRowExpression::ListLiteral {
                items,
            }));
        }
        if !statement.children.is_empty() {
            let mut output = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                expr_id,
            )?;
            for child in &statement.children {
                output = lower_row_pipeline_child_statement(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    output,
                    child,
                )?;
            }
            return Some(output);
        }
        return lower_row_expr(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            expr_id,
        );
    }
    if !statement.children.is_empty() {
        return lower_row_function_body(
            program,
            derived,
            index,
            constants,
            inputs,
            statement,
            env,
            expr_value_types,
        );
    }
    let expr_id = direct_statement_value_expr_id(statement)?;
    lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        expr_id,
    )
}

fn lower_row_pipeline_child_statement(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    previous: LoweredRowValue,
    statement: &AstStatement,
) -> Option<LoweredRowValue> {
    let expr_id = statement.expr?;
    let saved_previous = env.insert(ROW_PREVIOUS_BINDING.to_owned(), previous);
    let expr = expr_by_id(program, expr_id)?;
    let result = match &expr.kind {
        AstExprKind::Pipe { input, op, args } if row_list_builtin(op) => {
            let multiline_args = row_list_multiline_args(program, op, args, statement);
            lower_row_list_builtin(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                op,
                Some(*input),
                multiline_args.as_deref().unwrap_or(args),
            )
        }
        AstExprKind::Pipe { input, op, args } if row_text_builtin(op) => lower_row_text_builtin(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            op,
            Some(*input),
            args,
        ),
        AstExprKind::Pipe { input, op, args } if row_generic_builtin(op) => lower_row_builtin_call(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            op,
            Some(*input),
            args,
        ),
        _ => lower_row_statement_value(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            statement,
        ),
    };
    match saved_previous {
        Some(previous) => {
            env.insert(ROW_PREVIOUS_BINDING.to_owned(), previous);
        }
        None => {
            env.remove(ROW_PREVIOUS_BINDING);
        }
    }
    result
}

fn row_list_multiline_args(
    program: &TypedProgram,
    function: &str,
    args: &[AstCallArg],
    statement: &AstStatement,
) -> Option<Vec<AstCallArg>> {
    let mut resolved = args.to_vec();
    let mut changed = false;
    let multiline_value_arg = match function {
        "List/any" | "List/retain" => Some("if"),
        "List/map" => Some("new"),
        _ => None,
    };
    if let Some(arg_name) = multiline_value_arg
        && named_arg(args, arg_name).is_none()
    {
        let marker = args
            .iter()
            .position(|arg| row_raw_symbol(program, arg.value).as_deref() == Some(arg_name));
        if let Some(marker) = marker {
            let predicate = statement
                .children
                .iter()
                .find(|child| matches!(child.kind, AstStatementKind::Expression))?;
            let value = predicate.expr?;
            resolved.remove(marker);
            resolved.push(AstCallArg {
                name: Some(arg_name.to_owned()),
                value,
                start: predicate.start,
                end: predicate.end,
            });
            changed = true;
        }
    }
    for child in &statement.children {
        let AstStatementKind::Field { name } = &child.kind else {
            continue;
        };
        if resolved
            .iter()
            .any(|arg| arg.name.as_deref() == Some(name.as_str()))
        {
            continue;
        }
        resolved.push(AstCallArg {
            name: Some(name.clone()),
            value: child.expr?,
            start: child.start,
            end: child.end,
        });
        changed = true;
    }
    changed.then_some(resolved)
}

fn lower_row_call_statement_with_field_args(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    statement: &AstStatement,
    expr_id: usize,
) -> Option<LoweredRowValue> {
    let expr = expr_by_id(program, expr_id)?;
    let AstExprKind::Call { function, args } = &expr.kind else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    if let Some(value) = lower_row_list_find_statement(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        function,
        statement,
    ) {
        return Some(value);
    }
    let mut call_args = Vec::new();
    for child in &statement.children {
        let name = match &child.kind {
            AstStatementKind::Field { name } => Some(name.clone()),
            AstStatementKind::Expression => None,
            _ => return None,
        };
        let value = child.expr?;
        call_args.push(AstCallArg {
            name,
            value,
            start: child.start,
            end: child.end,
        });
    }
    if row_list_builtin(function) {
        return lower_row_list_builtin(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            function,
            None,
            &call_args,
        );
    }
    if row_text_builtin(function) {
        return lower_row_text_builtin(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            function,
            None,
            &call_args,
        );
    }
    if row_generic_builtin(function) {
        return lower_row_builtin_call(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            function,
            None,
            &call_args,
        );
    }
    lower_row_function_call(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        function,
        &call_args,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_row_list_find_statement(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    function: &str,
    statement: &AstStatement,
) -> Option<LoweredRowValue> {
    if function != "List/find" && function != "List/find_value" {
        return None;
    }
    let list_expr = statement
        .children
        .iter()
        .find(|child| matches!(child.kind, AstStatementKind::Expression))?
        .expr?;
    let list_id = lower_row_list_ref(program, derived, index, inputs, list_expr)?;
    let child_field = |name: &str| {
        statement.children.iter().find(
            |child| matches!(&child.kind, AstStatementKind::Field { name: field } if field == name),
        )
    };
    let field_name = row_raw_symbol(program, child_field("field")?.expr?)?;
    let field = row_field_id_for_list_id(program, list_id, &field_name)?;
    let value = lower_row_statement_value(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        child_field("value")?,
    )
    .and_then(lowered_scalar)?;
    if function == "List/find" {
        return Some(LoweredRowValue::ListFindRow {
            list_id,
            field,
            value,
        });
    }
    let target_name = row_raw_symbol(program, child_field("target")?.expr?)?;
    let target = row_field_id_for_list_id(program, list_id, &target_name)?;
    let fallback = if let Some(fallback) = child_field("fallback") {
        Some(
            lower_row_statement_value(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                fallback,
            )
            .and_then(lowered_scalar)?,
        )
    } else {
        None
    };
    Some(LoweredRowValue::Scalar(PlanRowExpression::ListFindValue {
        list_id,
        field,
        value: Box::new(value),
        target,
        fallback: fallback.map(Box::new),
    }))
}

fn lower_row_while_statement(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    statement: &AstStatement,
    expr_id: usize,
) -> Option<LoweredRowValue> {
    let expr = expr_by_id(program, expr_id)?;
    let input_id = match &expr.kind {
        AstExprKind::Pipe { input, op, args: _ } if op == "WHILE" || op == "WHEN" => *input,
        AstExprKind::When { input } => *input,
        _ => return None,
    };
    if let Some(value) = lower_row_equality_while_statement(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        statement,
        input_id,
    ) {
        return Some(value);
    }
    let input = lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        input_id,
    )?;
    let input_expression = lowered_scalar(input)?;
    let mut arms = Vec::new();
    for child in &statement.children {
        let arm_expr = expr_by_id(program, child.expr?)?;
        let AstExprKind::MatchArm { pattern, output } = &arm_expr.kind else {
            continue;
        };
        let mut arm_env = env.clone();
        let (select_pattern, binding) = row_select_pattern_and_binding(pattern)?;
        if let Some(binding) = binding {
            arm_env.insert(binding, LoweredRowValue::Scalar(input_expression.clone()));
        }
        let arm_value = lower_row_match_arm_output(
            program,
            derived,
            index,
            constants,
            inputs,
            child,
            &mut arm_env,
            expr_value_types,
            *output,
        )?;
        arms.push(PlanRowSelectArm {
            pattern: select_pattern,
            value: lowered_scalar(arm_value)?,
        });
    }
    (!arms.is_empty()).then_some(LoweredRowValue::Scalar(PlanRowExpression::Select {
        input: Box::new(input_expression),
        arms,
    }))
}

fn lower_row_equality_while_statement(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    statement: &AstStatement,
    input_id: usize,
) -> Option<LoweredRowValue> {
    let input_expr = expr_by_id(program, input_id)?;
    let AstExprKind::Infix { left, op, right } = &input_expr.kind else {
        return None;
    };
    if !matches!(op.as_str(), "==" | "!=") {
        return None;
    }
    if row_equality_rhs_is_dynamic_reference(program, derived, index, env, *right) {
        return None;
    }
    let input = lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        *left,
    )?;
    let input_expression = lowered_scalar(input)?;
    let match_pattern = row_select_pattern_for_expr(program, *right)?;
    let mut true_value = None;
    let mut false_value = None;
    for child in &statement.children {
        let arm_expr = expr_by_id(program, child.expr?)?;
        let AstExprKind::MatchArm { pattern, output } = &arm_expr.kind else {
            continue;
        };
        let label = pattern.join("");
        if label != "True" && label != "False" {
            return None;
        }
        let mut arm_env = env.clone();
        let arm_value = lower_row_match_arm_output(
            program,
            derived,
            index,
            constants,
            inputs,
            child,
            &mut arm_env,
            expr_value_types,
            *output,
        )?;
        let arm_value = lowered_scalar(arm_value)?;
        if label == "True" {
            true_value = Some(arm_value);
        } else {
            false_value = Some(arm_value);
        }
    }
    let true_value = true_value?;
    let false_value = false_value?;
    let (match_value, wildcard_value) = if op == "==" {
        (true_value, false_value)
    } else {
        (false_value, true_value)
    };
    Some(LoweredRowValue::Scalar(PlanRowExpression::Select {
        input: Box::new(input_expression),
        arms: vec![
            PlanRowSelectArm {
                pattern: match_pattern,
                value: match_value,
            },
            PlanRowSelectArm {
                pattern: PlanRowSelectPattern::Wildcard,
                value: wildcard_value,
            },
        ],
    }))
}

fn row_equality_rhs_is_dynamic_reference(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    env: &BTreeMap<String, LoweredRowValue>,
    expr_id: usize,
) -> bool {
    let Some(path) = expression_path_string(program, expr_id) else {
        return false;
    };
    if env.contains_key(&path) {
        return true;
    }
    let mut candidates = scoped_resolution_candidates(&derived.path, &path);
    if let Some((parent, _)) = derived.path.rsplit_once('.') {
        candidates.push(format!("{parent}.{path}"));
        if let Some((grandparent, _)) = parent.rsplit_once('.') {
            candidates.push(format!("{grandparent}.{path}"));
        }
    }
    candidates
        .iter()
        .any(|candidate| index.resolve(candidate).is_some())
}

fn lower_row_match_arm_output(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    arm_statement: &AstStatement,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    output: Option<usize>,
) -> Option<LoweredRowValue> {
    let Some(output) = output else {
        return lower_row_function_body(
            program,
            derived,
            index,
            constants,
            inputs,
            arm_statement,
            env,
            expr_value_types,
        );
    };
    if row_expr_is_block_marker(program, output) && !arm_statement.children.is_empty() {
        return lower_row_function_body(
            program,
            derived,
            index,
            constants,
            inputs,
            arm_statement,
            env,
            expr_value_types,
        );
    }
    if !arm_statement.children.is_empty()
        && let Some(value) = lower_row_while_statement(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            arm_statement,
            output,
        )
    {
        return Some(value);
    }
    if !arm_statement.children.is_empty()
        && let Some(value) = lower_row_when_statement(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            arm_statement,
            output,
        )
    {
        return Some(value);
    }
    lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        output,
    )
}

fn row_select_pattern_for_expr(
    program: &TypedProgram,
    expr_id: usize,
) -> Option<PlanRowSelectPattern> {
    match &expr_by_id(program, expr_id)?.kind {
        AstExprKind::Number(value) => value
            .parse::<FiniteReal>()
            .ok()
            .map(|value| PlanRowSelectPattern::Number { value }),
        AstExprKind::Bool(value) => Some(PlanRowSelectPattern::Bool { value: *value }),
        AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Identifier(value) => Some(PlanRowSelectPattern::Text {
            value: value.clone(),
        }),
        AstExprKind::Path(parts) => Some(PlanRowSelectPattern::Text {
            value: parts.join("."),
        }),
        _ => None,
    }
}

fn row_expr_is_block_marker(program: &TypedProgram, expr_id: usize) -> bool {
    let kind = expr_by_id(program, expr_id).map(|expr| &expr.kind);
    matches!(kind, Some(AstExprKind::Delimiter))
        || matches!(kind, Some(AstExprKind::Identifier(name)) if name == "BLOCK")
}

fn row_select_pattern_and_binding(
    pattern: &[String],
) -> Option<(PlanRowSelectPattern, Option<String>)> {
    let label = pattern.join("");
    match label.as_str() {
        "True" => Some((PlanRowSelectPattern::Bool { value: true }, None)),
        "False" => Some((PlanRowSelectPattern::Bool { value: false }, None)),
        "NaN" => Some((PlanRowSelectPattern::NaN, None)),
        "__" => Some((PlanRowSelectPattern::Wildcard, None)),
        _ => label
            .parse::<FiniteReal>()
            .map(|value| (PlanRowSelectPattern::Number { value }, None))
            .ok()
            .or_else(|| {
                row_text_pattern_literal(&label)
                    .map(|value| (PlanRowSelectPattern::Text { value }, None))
            })
            .or_else(|| {
                row_binding_pattern_name(&label)
                    .map(|binding| (PlanRowSelectPattern::Wildcard, Some(binding)))
            })
            .or(Some((PlanRowSelectPattern::Text { value: label }, None))),
    }
}

fn row_text_pattern_literal(label: &str) -> Option<String> {
    let text = label.trim();
    let inner = text
        .strip_prefix("TEXT")?
        .trim_start()
        .strip_prefix('{')?
        .strip_suffix('}')?;
    Some(inner.trim().to_owned())
}

fn row_binding_pattern_name(label: &str) -> Option<String> {
    let mut chars = label.chars();
    let first = chars.next()?;
    if !(first == '_' || first.is_ascii_lowercase()) {
        return None;
    }
    chars
        .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        .then(|| label.to_owned())
}

fn row_text_builtin(function: &str) -> bool {
    matches!(
        function,
        "Text/trim"
            | "Text/is_empty"
            | "Text/starts_with"
            | "Text/length"
            | "Text/to_number"
            | "Text/concat"
            | "Text/substring"
            | "Text/time_range_label"
    )
}

fn row_list_builtin(function: &str) -> bool {
    matches!(
        function,
        "List/find"
            | "List/find_value"
            | "List/range"
            | "List/map"
            | "List/any"
            | "List/sum"
            | "List/count"
            | "List/length"
            | "List/retain"
            | "List/filter_field_equal"
            | "List/filter_field_not_equal"
            | "List/filter_text_contains"
            | "List/join_field"
    )
}

fn row_generic_builtin(function: &str) -> bool {
    matches!(
        function,
        "Text/empty"
            | "Router/route"
            | "Text/to_bytes"
            | "Bytes/to_text"
            | "Bytes/to_hex"
            | "Bytes/to_base64"
            | "Bytes/from_hex"
            | "Bytes/from_base64"
            | "Bytes/is_empty"
            | "Bytes/length"
            | "Bytes/get"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/zeros"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/set"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
            | "Bytes/find"
            | "Bytes/starts_with"
            | "Bytes/ends_with"
            | "Bytes/concat"
            | "Bytes/equal"
            | "Error/new"
            | "Error/text"
            | "Number/min"
            | "Number/max"
            | "Number/ceil"
            | "Number/floor"
            | "Number/round"
            | "Number/truncate"
            | "Number/interpolate"
            | "Number/project_offset"
            | "Number/project_time"
            | "Number/project_width"
    )
}

fn lower_row_list_builtin(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    function: &str,
    piped_input: Option<usize>,
    args: &[AstCallArg],
) -> Option<LoweredRowValue> {
    match function {
        "List/range" => {
            let from = named_arg(args, "from")?.value;
            let to = named_arg(args, "to")?.value;
            let from = lower_row_number_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                from,
            )?;
            let to = lower_row_number_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                to,
            )?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::ListRange {
                from: Box::new(from),
                to: Box::new(to),
            }))
        }
        "List/find" | "List/find_value" => {
            let list_expr =
                piped_input.or_else(|| first_positional_arg(args).map(|arg| arg.value))?;
            let list_id = lower_row_list_ref(program, derived, index, inputs, list_expr)?;
            let field_name =
                named_arg(args, "field").and_then(|arg| row_raw_symbol(program, arg.value))?;
            let field = row_field_id_for_list_id(program, list_id, &field_name)?;
            let value_expr = named_arg(args, "value")?.value;
            let value = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                value_expr,
            )
            .and_then(lowered_scalar)?;
            if function == "List/find" {
                return Some(LoweredRowValue::ListFindRow {
                    list_id,
                    field,
                    value,
                });
            }
            let target_name =
                named_arg(args, "target").and_then(|arg| row_raw_symbol(program, arg.value))?;
            let target = row_field_id_for_list_id(program, list_id, &target_name)?;
            let fallback = if let Some(arg) = named_arg(args, "fallback") {
                Some(
                    lower_row_expr(
                        program,
                        derived,
                        index,
                        constants,
                        inputs,
                        env,
                        expr_value_types,
                        arg.value,
                    )
                    .and_then(lowered_scalar)?,
                )
            } else {
                None
            };
            Some(LoweredRowValue::Scalar(PlanRowExpression::ListFindValue {
                list_id,
                field,
                value: Box::new(value),
                target,
                fallback: fallback.map(Box::new),
            }))
        }
        "List/map" => {
            let input_expr = piped_input.or_else(|| positional_arg(args, 0).map(|arg| arg.value));
            let (input, implicit_input) = lower_row_list_input_expression(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                input_expr,
                piped_input.is_some(),
            )?;
            let binding_arg_index = if implicit_input { 0 } else { 1 };
            let binding = positional_arg(args, binding_arg_index)
                .and_then(|arg| row_raw_symbol(program, arg.value))?;
            let new_expr = named_arg(args, "new")?.value;
            let mut map_env = env.clone();
            map_env.insert(
                binding.clone(),
                LoweredRowValue::ListMapItem {
                    binding: binding.clone(),
                    list_id: row_expression_direct_list_id(&input),
                },
            );
            let value = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                &mut map_env,
                expr_value_types,
                new_expr,
            )
            .and_then(lowered_scalar)?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::ListMap {
                input: Box::new(input),
                binding,
                value: Box::new(value),
            }))
        }
        "List/count" | "List/length" => {
            let input_expr =
                piped_input.or_else(|| first_positional_arg(args).map(|arg| arg.value));
            let (input, _) = lower_row_list_input_expression(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                input_expr,
                piped_input.is_some(),
            )?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::BuiltinCall {
                function: function.to_owned(),
                input: Some(Box::new(input)),
                args: Vec::new(),
            }))
        }
        "List/retain" | "List/any" => {
            let input_expr = piped_input.or_else(|| positional_arg(args, 0).map(|arg| arg.value));
            let (input, implicit_input) = lower_row_list_input_expression(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                input_expr,
                piped_input.is_some(),
            )?;
            let binding_arg_index = if implicit_input { 0 } else { 1 };
            let binding = positional_arg(args, binding_arg_index)
                .and_then(|arg| row_raw_symbol(program, arg.value))?;
            let predicate_expr = named_arg(args, "if")?.value;
            let mut retain_env = env.clone();
            retain_env.insert(
                binding.clone(),
                LoweredRowValue::ListMapItem {
                    binding: binding.clone(),
                    list_id: row_expression_direct_list_id(&input),
                },
            );
            let predicate = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                &mut retain_env,
                expr_value_types,
                predicate_expr,
            )
            .and_then(lowered_scalar)?;
            let binding_value = row_constant_expression(
                constants,
                inputs,
                PlanConstantValue::Text { value: binding },
            );
            Some(LoweredRowValue::Scalar(PlanRowExpression::BuiltinCall {
                function: function.to_owned(),
                input: Some(Box::new(input)),
                args: vec![
                    PlanRowCallArg {
                        name: Some("binding".to_owned()),
                        value: binding_value,
                    },
                    PlanRowCallArg {
                        name: Some("if".to_owned()),
                        value: predicate,
                    },
                ],
            }))
        }
        "List/filter_field_equal"
        | "List/filter_field_not_equal"
        | "List/filter_text_contains"
        | "List/join_field" => {
            let input_expr =
                piped_input.or_else(|| first_positional_arg(args).map(|arg| arg.value));
            let (input, _) = lower_row_list_input_expression(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                input_expr,
                piped_input.is_some(),
            )?;
            let lowered_args = args
                .iter()
                .map(|arg| {
                    let value = if row_builtin_arg_expects_symbol(function, arg.name.as_deref()) {
                        lower_row_symbol_or_expr(
                            program,
                            derived,
                            index,
                            constants,
                            inputs,
                            env,
                            expr_value_types,
                            arg.value,
                        )?
                    } else {
                        lower_row_expr(
                            program,
                            derived,
                            index,
                            constants,
                            inputs,
                            env,
                            expr_value_types,
                            arg.value,
                        )
                        .and_then(lowered_scalar)?
                    };
                    Some(PlanRowCallArg {
                        name: arg.name.clone(),
                        value,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::BuiltinCall {
                function: function.to_owned(),
                input: Some(Box::new(input)),
                args: lowered_args,
            }))
        }
        "List/sum" => {
            let input_expr =
                piped_input.or_else(|| first_positional_arg(args).map(|arg| arg.value));
            let (input, _) = lower_row_list_input_expression(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                input_expr,
                piped_input.is_some(),
            )?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::ListSum {
                input: Box::new(input),
            }))
        }
        _ => None,
    }
}

fn lower_row_list_input_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    expr_id: Option<usize>,
    expr_is_implicit_input: bool,
) -> Option<(PlanRowExpression, bool)> {
    if let Some(expr_id) = expr_id {
        return Some((
            lower_row_list_expression(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                expr_id,
            )?,
            expr_is_implicit_input,
        ));
    }
    Some((
        env.get(ROW_PREVIOUS_BINDING)
            .cloned()
            .and_then(lowered_scalar)?,
        true,
    ))
}

fn lower_row_list_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    expr_id: usize,
) -> Option<PlanRowExpression> {
    if let Some(list_id) = lower_row_list_ref(program, derived, index, inputs, expr_id) {
        return Some(PlanRowExpression::ListRef { list_id });
    }
    let expression = lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        expr_id,
    )
    .and_then(lowered_scalar)?;
    if let PlanRowExpression::Field {
        input: ValueRef::Field(field_id),
    } = &expression
        && !field_has_derived_computation(program, *field_id)
        && let Some(list_id) = list_id_for_semantic_list_memory_field(program, *field_id)
    {
        let list_ref = ValueRef::List(list_id);
        if !inputs.contains(&list_ref) {
            inputs.push(list_ref);
        }
        return Some(PlanRowExpression::ListRef { list_id });
    }
    Some(expression)
}

fn field_has_derived_computation(program: &TypedProgram, field: FieldId) -> bool {
    program
        .derived_values
        .iter()
        .any(|derived| derived_output_ref(program, derived) == ValueRef::Field(field))
}

fn lower_row_list_ref(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    expr_id: usize,
) -> Option<ListId> {
    let list_path = expression_path_string(program, expr_id)?;
    let canonical = canonical_sibling_path(&derived.path, &list_path);
    let local = list_path.rsplit_once('.').map(|(_, local)| local);
    let candidates = [Some(canonical.as_str()), Some(list_path.as_str()), local];
    for candidate in candidates.into_iter().flatten() {
        match index.resolve(candidate) {
            Some(ValueRef::List(list_id)) => {
                let list_ref = ValueRef::List(list_id);
                if !inputs.contains(&list_ref) {
                    inputs.push(list_ref);
                }
                return Some(list_id);
            }
            Some(ValueRef::Field(field_id))
                if !field_has_derived_computation(program, field_id) =>
            {
                let list_id = list_id_for_semantic_list_memory_field(program, field_id)?;
                let list_ref = ValueRef::List(list_id);
                if !inputs.contains(&list_ref) {
                    inputs.push(list_ref);
                }
                return Some(list_id);
            }
            Some(_) => return None,
            None => {}
        }
    }
    None
}

fn first_positional_arg(args: &[AstCallArg]) -> Option<&AstCallArg> {
    positional_arg(args, 0)
}

fn positional_arg(args: &[AstCallArg], index: usize) -> Option<&AstCallArg> {
    args.iter().filter(|arg| arg.name.is_none()).nth(index)
}

fn row_raw_symbol(program: &TypedProgram, expr_id: usize) -> Option<String> {
    match &expr_by_id(program, expr_id)?.kind {
        AstExprKind::Identifier(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value) => Some(value.clone()),
        AstExprKind::Path(parts) => Some(parts.join(".")),
        _ => None,
    }
}

fn lower_row_builtin_call(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    function: &str,
    piped_input: Option<usize>,
    args: &[AstCallArg],
) -> Option<LoweredRowValue> {
    let input = match piped_input {
        Some(expr_id) => Some(
            lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                expr_id,
            )
            .and_then(lowered_scalar)?,
        ),
        None => None,
    };
    let args = args
        .iter()
        .map(|arg| {
            let value = if row_builtin_arg_expects_symbol(function, arg.name.as_deref()) {
                lower_row_symbol_or_expr(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    arg.value,
                )?
            } else if row_builtin_arg_expects_number(function, arg.name.as_deref()) {
                lower_row_number_expr(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    arg.value,
                )?
            } else {
                let value = lower_row_expr(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    arg.value,
                )?;
                lowered_scalar(value)?
            };
            Some(PlanRowCallArg {
                name: arg.name.clone(),
                value,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if function == "Text/to_bytes" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input", "text"]))?;
        let encoding = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("encoding"))
            .map(|arg| Box::new(arg.value.clone()));
        return Some(LoweredRowValue::Scalar(PlanRowExpression::TextToBytes {
            input: Box::new(input),
            encoding,
        }));
    }
    if function == "Bytes/to_text" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input", "bytes"]))?;
        let encoding = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("encoding"))
            .map(|arg| Box::new(arg.value.clone()));
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesToText {
            input: Box::new(input),
            encoding,
        }));
    }
    if function == "Bytes/to_hex" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input", "bytes"]))?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesToHex {
            input: Box::new(input),
        }));
    }
    if function == "Bytes/to_base64" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input", "bytes"]))?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesToBase64 {
            input: Box::new(input),
        }));
    }
    if function == "Bytes/from_hex" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input", "text"]))?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesFromHex {
            input: Box::new(input),
        }));
    }
    if function == "Bytes/from_base64" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input", "text"]))?;
        return Some(LoweredRowValue::Scalar(
            PlanRowExpression::BytesFromBase64 {
                input: Box::new(input),
            },
        ));
    }
    if function == "Bytes/is_empty" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesIsEmpty {
            input: Box::new(input),
        }));
    }
    if function == "Bytes/length" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesLength {
            input: Box::new(input),
        }));
    }
    if function == "Bytes/get" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let index = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("index"))
            .map(|arg| arg.value.clone())?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesGet {
            input: Box::new(input),
            index: Box::new(index),
        }));
    }
    if function == "Bytes/slice" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let offset = args
            .iter()
            .find(|arg| {
                arg.name
                    .as_deref()
                    .is_some_and(|name| name == "offset" || name == "start")
            })
            .map(|arg| arg.value.clone())?;
        let byte_count = args
            .iter()
            .find(|arg| {
                arg.name
                    .as_deref()
                    .is_some_and(|name| name == "byte_count" || name == "length" || name == "count")
            })
            .map(|arg| arg.value.clone())?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesSlice {
            input: Box::new(input),
            offset: Box::new(offset),
            byte_count: Box::new(byte_count),
        }));
    }
    if function == "Bytes/take" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let byte_count = row_call_arg_value(&args, &["byte_count", "length", "count"])?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesTake {
            input: Box::new(input),
            byte_count: Box::new(byte_count),
        }));
    }
    if function == "Bytes/drop" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let byte_count = row_call_arg_value(&args, &["byte_count", "length", "count"])?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesDrop {
            input: Box::new(input),
            byte_count: Box::new(byte_count),
        }));
    }
    if function == "Bytes/zeros" && input.is_none() {
        let byte_count = row_call_arg_value(&args, &["byte_count", "length", "count"])?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesZeros {
            byte_count: Box::new(byte_count),
        }));
    }
    if function == "Bytes/read_unsigned" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let offset = row_call_arg_value(&args, &["offset", "start"])?;
        let byte_count = row_call_arg_value(&args, &["byte_count", "length", "count"])?;
        let endian = row_call_arg_value(&args, &["endian"])?;
        return Some(LoweredRowValue::Scalar(
            PlanRowExpression::BytesReadUnsigned {
                input: Box::new(input),
                offset: Box::new(offset),
                byte_count: Box::new(byte_count),
                endian: Box::new(endian),
            },
        ));
    }
    if function == "Bytes/read_signed" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let offset = row_call_arg_value(&args, &["offset", "start"])?;
        let byte_count = row_call_arg_value(&args, &["byte_count", "length", "count"])?;
        let endian = row_call_arg_value(&args, &["endian"])?;
        return Some(LoweredRowValue::Scalar(
            PlanRowExpression::BytesReadSigned {
                input: Box::new(input),
                offset: Box::new(offset),
                byte_count: Box::new(byte_count),
                endian: Box::new(endian),
            },
        ));
    }
    if function == "Bytes/set" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let index = row_call_arg_value(&args, &["index"])?;
        let value = row_call_arg_value(&args, &["value"])?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesSet {
            input: Box::new(input),
            index: Box::new(index),
            value: Box::new(value),
        }));
    }
    if function == "Bytes/write_unsigned" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let offset = row_call_arg_value(&args, &["offset", "start"])?;
        let byte_count = row_call_arg_value(&args, &["byte_count", "length", "count"])?;
        let endian = row_call_arg_value(&args, &["endian"])?;
        let value = row_call_arg_value(&args, &["value"])?;
        return Some(LoweredRowValue::Scalar(
            PlanRowExpression::BytesWriteUnsigned {
                input: Box::new(input),
                offset: Box::new(offset),
                byte_count: Box::new(byte_count),
                endian: Box::new(endian),
                value: Box::new(value),
            },
        ));
    }
    if function == "Bytes/write_signed" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let offset = row_call_arg_value(&args, &["offset", "start"])?;
        let byte_count = row_call_arg_value(&args, &["byte_count", "length", "count"])?;
        let endian = row_call_arg_value(&args, &["endian"])?;
        let value = row_call_arg_value(&args, &["value"])?;
        return Some(LoweredRowValue::Scalar(
            PlanRowExpression::BytesWriteSigned {
                input: Box::new(input),
                offset: Box::new(offset),
                byte_count: Box::new(byte_count),
                endian: Box::new(endian),
                value: Box::new(value),
            },
        ));
    }
    if function == "Bytes/find" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let needle = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("needle"))
            .map(|arg| arg.value.clone())?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesFind {
            input: Box::new(input),
            needle: Box::new(needle),
        }));
    }
    if function == "Bytes/starts_with" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let prefix = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("prefix"))
            .map(|arg| arg.value.clone())?;
        return Some(LoweredRowValue::Scalar(
            PlanRowExpression::BytesStartsWith {
                input: Box::new(input),
                prefix: Box::new(prefix),
            },
        ));
    }
    if function == "Bytes/ends_with" {
        let input = input.or_else(|| row_call_arg_value(&args, &["input"]))?;
        let suffix = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("suffix"))
            .map(|arg| arg.value.clone())?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesEndsWith {
            input: Box::new(input),
            suffix: Box::new(suffix),
        }));
    }
    if function == "Bytes/concat" {
        let left = input.or_else(|| row_call_arg_value(&args, &["left", "input"]))?;
        let right = row_call_arg_value(&args, &["right", "with"])?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesConcat {
            left: Box::new(left),
            right: Box::new(right),
        }));
    }
    if function == "Bytes/equal" {
        let left = input.or_else(|| row_call_arg_value(&args, &["left", "input"]))?;
        let right = row_call_arg_value(&args, &["right", "with"])?;
        return Some(LoweredRowValue::Scalar(PlanRowExpression::BytesEqual {
            left: Box::new(left),
            right: Box::new(right),
        }));
    }

    Some(LoweredRowValue::Scalar(PlanRowExpression::BuiltinCall {
        function: function.to_owned(),
        input: input.map(Box::new),
        args,
    }))
}

fn row_call_arg_value(args: &[PlanRowCallArg], names: &[&str]) -> Option<PlanRowExpression> {
    args.iter()
        .find(|arg| {
            arg.name
                .as_deref()
                .is_some_and(|name| names.contains(&name))
        })
        .map(|arg| arg.value.clone())
}

fn row_builtin_arg_expects_number(function: &str, arg_name: Option<&str>) -> bool {
    matches!(
        (function, arg_name),
        ("Bytes/get", Some("index"))
            | ("Bytes/slice", Some("offset"))
            | ("Bytes/slice", Some("byte_count"))
            | ("Bytes/take", Some("byte_count" | "length" | "count"))
            | ("Bytes/drop", Some("byte_count" | "length" | "count"))
            | ("Bytes/zeros", Some("byte_count" | "length" | "count"))
            | (
                "Bytes/read_unsigned",
                Some("offset" | "start" | "byte_count" | "length" | "count")
            )
            | (
                "Bytes/read_signed",
                Some("offset" | "start" | "byte_count" | "length" | "count")
            )
            | ("Bytes/set", Some("index" | "value"))
            | (
                "Bytes/write_unsigned",
                Some("offset" | "start" | "byte_count" | "length" | "count" | "value")
            )
            | (
                "Bytes/write_signed",
                Some("offset" | "start" | "byte_count" | "length" | "count" | "value")
            )
            | (
                "Number/interpolate",
                Some("start" | "end" | "numerator" | "denominator" | "fallback")
            )
            | (
                "Number/project_offset",
                Some("time" | "viewport_start" | "viewport_end" | "canvas_width" | "fallback")
            )
            | (
                "Number/project_time",
                Some(
                    "pointer_x" | "pointer_width" | "viewport_start" | "viewport_end" | "fallback"
                )
            )
            | (
                "Number/project_width",
                Some(
                    "start_time"
                        | "end_time"
                        | "viewport_start"
                        | "viewport_end"
                        | "canvas_width"
                        | "fallback"
                )
            )
    )
}

fn row_builtin_arg_expects_symbol(function: &str, arg_name: Option<&str>) -> bool {
    matches!(
        (function, arg_name),
        (_, Some("encoding"))
            | (
                "List/filter_field_equal"
                    | "List/filter_field_not_equal"
                    | "List/filter_text_contains"
                    | "List/join_field",
                Some("field" | "prefer_field" | "empty_field")
            )
            | (
                "Bytes/read_unsigned"
                    | "Bytes/read_signed"
                    | "Bytes/write_unsigned"
                    | "Bytes/write_signed",
                Some("endian")
            )
    )
}

fn lower_row_symbol_or_expr(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    expr_id: usize,
) -> Option<PlanRowExpression> {
    match &expr_by_id(program, expr_id)?.kind {
        AstExprKind::Identifier(value) | AstExprKind::Enum(value) | AstExprKind::Tag(value) => {
            Some(row_constant_expression(
                constants,
                inputs,
                PlanConstantValue::Text {
                    value: value.clone(),
                },
            ))
        }
        _ => lower_row_expr(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            expr_id,
        )
        .and_then(lowered_scalar),
    }
}

fn lower_row_text_builtin(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    function: &str,
    piped_input: Option<usize>,
    args: &[AstCallArg],
) -> Option<LoweredRowValue> {
    let input_expr = piped_input.or_else(|| {
        args.iter()
            .find(|arg| {
                arg.name.is_none()
                    || arg.name.as_deref() == Some("input")
                    || arg.name.as_deref() == Some("text")
            })
            .map(|arg| arg.value)
    });
    let input = if let Some(input_expr) = input_expr {
        lower_row_expr(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            input_expr,
        )?
    } else {
        env.get(ROW_PREVIOUS_BINDING).cloned()?
    };
    let input = lowered_scalar(input)?;
    let expression = match function {
        "Text/trim" => PlanRowExpression::TextTrim {
            input: Box::new(input),
        },
        "Text/is_empty" => PlanRowExpression::TextIsEmpty {
            input: Box::new(input),
        },
        "Text/starts_with" => {
            let prefix_expr = args
                .iter()
                .find(|arg| arg.name.as_deref() == Some("prefix"))
                .map(|arg| arg.value)?;
            let prefix = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                prefix_expr,
            )?;
            PlanRowExpression::TextStartsWith {
                input: Box::new(input),
                prefix: Box::new(lowered_scalar(prefix)?),
            }
        }
        "Text/length" => PlanRowExpression::TextLength {
            input: Box::new(input),
        },
        "Text/to_number" => PlanRowExpression::TextToNumber {
            input: Box::new(input),
        },
        "Text/concat" => {
            let with_expr = args
                .iter()
                .find(|arg| arg.name.as_deref() == Some("with"))
                .or_else(|| args.iter().filter(|arg| arg.name.is_none()).nth(1))
                .map(|arg| arg.value)?;
            let with = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                with_expr,
            )?;
            let mut parts = vec![input];
            if let Some(separator_expr) = args
                .iter()
                .find(|arg| arg.name.as_deref() == Some("separator"))
                .map(|arg| arg.value)
            {
                let separator = lower_row_expr(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    separator_expr,
                )?;
                parts.push(lowered_scalar(separator)?);
            }
            parts.push(lowered_scalar(with)?);
            PlanRowExpression::TextConcat { parts }
        }
        "Text/substring" => {
            let start_expr = args
                .iter()
                .find(|arg| arg.name.as_deref() == Some("start"))
                .map(|arg| arg.value)?;
            let length_expr = args
                .iter()
                .find(|arg| arg.name.as_deref() == Some("length"))
                .map(|arg| arg.value)?;
            let start = lower_row_number_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                start_expr,
            )?;
            let length = lower_row_number_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                length_expr,
            )?;
            PlanRowExpression::TextSubstring {
                input: Box::new(input),
                start: Box::new(start),
                length: Box::new(length),
            }
        }
        "Text/time_range_label" => {
            let end_expr = args
                .iter()
                .find(|arg| arg.name.as_deref() == Some("end"))
                .map(|arg| arg.value)?;
            let unit_expr = args
                .iter()
                .find(|arg| arg.name.as_deref() == Some("unit"))
                .map(|arg| arg.value)?;
            let end = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                end_expr,
            )?;
            let unit = lower_row_expr(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                unit_expr,
            )?;
            let space = row_constant_expression(
                constants,
                inputs,
                PlanConstantValue::Text {
                    value: " ".to_owned(),
                },
            );
            let separator = row_constant_expression(
                constants,
                inputs,
                PlanConstantValue::Text {
                    value: " - ".to_owned(),
                },
            );
            PlanRowExpression::TextConcat {
                parts: vec![
                    input,
                    space.clone(),
                    lowered_scalar(unit.clone())?,
                    separator,
                    lowered_scalar(end)?,
                    space,
                    lowered_scalar(unit)?,
                ],
            }
        }
        _ => return None,
    };
    Some(LoweredRowValue::Scalar(expression))
}

fn row_constant_expression(
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    value: PlanConstantValue,
) -> PlanRowExpression {
    let constant_id = push_plan_constant(constants, value);
    inputs.push(ValueRef::Constant(constant_id));
    PlanRowExpression::Constant { constant_id }
}

fn unbound_identifier_literal(
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    name: &str,
) -> Option<LoweredRowValue> {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
        .then(|| {
            LoweredRowValue::Scalar(row_constant_expression(
                constants,
                inputs,
                PlanConstantValue::Enum {
                    value: name.to_owned(),
                },
            ))
        })
}

fn row_bytes_constant_expression(
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    bytes: Vec<u8>,
) -> PlanRowExpression {
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    row_constant_expression(
        constants,
        inputs,
        PlanConstantValue::Bytes {
            byte_len: bytes.len() as u64,
            sha256: format!("{:x}", hasher.finalize()),
            inline_bytes: (bytes.len() <= INLINE_BYTE_CONSTANT_LIMIT).then_some(bytes),
        },
    )
}

fn row_static_bytes_literal(program: &TypedProgram, items: &[usize]) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for item in items {
        match &expr_by_id(program, *item)?.kind {
            AstExprKind::ByteLiteral { value, .. } => bytes.push(*value),
            AstExprKind::BytesLiteral { items, .. } => {
                bytes.extend(row_static_bytes_literal(program, items)?);
            }
            _ => return None,
        }
    }
    Some(bytes)
}

fn lower_row_list_get(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    args: &[AstCallArg],
) -> Option<LoweredRowValue> {
    let list_expr = args.iter().find(|arg| arg.name.is_none())?.value;
    let list_path = expression_path_string(program, list_expr)?;
    let ValueRef::List(list_id) = index.resolve(&list_path)? else {
        return None;
    };
    inputs.push(ValueRef::List(list_id));
    let index_expr = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("index"))?
        .value;
    let index_expr = lower_row_number_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        index_expr,
    )?;
    Some(LoweredRowValue::ListRow {
        list_id,
        index: index_expr,
    })
}

fn lower_row_function_call(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
    function: &str,
    args: &[AstCallArg],
) -> Option<LoweredRowValue> {
    let function = program.functions.iter().find(|candidate| {
        candidate.name == function
            || function
                .rsplit_once('/')
                .is_some_and(|(_, suffix)| suffix == candidate.name)
    })?;
    let mut function_env = BTreeMap::new();
    let mut positional_index = 0usize;
    for arg in args {
        let arg_name = if let Some(name) = arg.name.as_ref() {
            name.clone()
        } else {
            let name = function.args.get(positional_index)?.clone();
            positional_index += 1;
            name
        };
        let value = lower_row_expr(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            arg.value,
        )?;
        function_env.insert(arg_name, value);
    }
    lower_row_function_body(
        program,
        derived,
        index,
        constants,
        inputs,
        &function.statement,
        &mut function_env,
        expr_value_types,
    )
}

fn lower_row_function_body(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    statement: &AstStatement,
    env: &mut BTreeMap<String, LoweredRowValue>,
    expr_value_types: &BTreeMap<usize, PlanValueType>,
) -> Option<LoweredRowValue> {
    let body = statement
        .children
        .iter()
        .find(|child| matches!(child.kind, AstStatementKind::Block))
        .unwrap_or(statement);
    let mut output = None;
    let mut object_fields = Vec::new();
    for child in &body.children {
        if row_statement_is_empty_delimiter(child, program) {
            continue;
        }
        if let Some(previous) = output.clone() {
            env.insert(ROW_PREVIOUS_BINDING.to_owned(), previous);
        } else {
            env.remove(ROW_PREVIOUS_BINDING);
        }
        match &child.kind {
            AstStatementKind::Field { name } => {
                let value = lower_row_statement_value(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    child,
                )?;
                if let Some(scalar) = lowered_scalar(value.clone()) {
                    object_fields.push(PlanRowObjectField {
                        name: name.clone(),
                        value: scalar,
                    });
                }
                env.insert(name.clone(), value);
            }
            AstStatementKind::Expression => {
                output = Some(lower_row_statement_value(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    child,
                )?);
            }
            AstStatementKind::List { field: None, .. } => {
                output = Some(lower_row_statement_value(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    env,
                    expr_value_types,
                    child,
                )?);
            }
            AstStatementKind::Block => {
                output = Some(lower_row_function_body(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    child,
                    env,
                    expr_value_types,
                )?);
            }
            _ => {}
        }
    }
    env.remove(ROW_PREVIOUS_BINDING);
    if output.is_none() && !object_fields.is_empty() {
        return Some(LoweredRowValue::Scalar(PlanRowExpression::Object {
            fields: object_fields,
        }));
    }
    output
}

fn row_statement_is_empty_delimiter(statement: &AstStatement, program: &TypedProgram) -> bool {
    statement.children.is_empty()
        && statement
            .expr
            .and_then(|id| program.expressions.get(id))
            .is_some_and(|expr| matches!(expr.kind, AstExprKind::Delimiter))
}

fn row_field_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    path: &str,
) -> Option<PlanRowExpression> {
    if let Some(value_ref) = explicit_input_row_field_ref(program, derived.scope_id, path) {
        inputs.push(value_ref.clone());
        return Some(PlanRowExpression::Field { input: value_ref });
    }
    let candidates = scoped_resolution_candidates(&derived.path, path);
    let value_ref = candidates
        .iter()
        .find_map(|candidate| index.resolve(candidate))
        .or_else(|| {
            synthetic_range_row_field_ref(program, plan_scope_id(derived.scope_id), path)
        })?;
    inputs.push(value_ref.clone());
    Some(PlanRowExpression::Field { input: value_ref })
}

fn explicit_input_row_field_ref(
    program: &TypedProgram,
    scope_id: Option<ir::ScopeId>,
    path: &str,
) -> Option<ValueRef> {
    let scope_id = scope_id?;
    let scope = program
        .row_scopes
        .iter()
        .find(|scope| scope.id == scope_id)?;
    let field = path.strip_prefix(&format!("{}.", scope.row_scope))?;
    if field.contains('.') {
        return None;
    }
    let list = program
        .lists
        .iter()
        .find(|list| list.row_scope_id == Some(scope_id))?;
    synthetic_initial_list_field_ids(program)
        .get(&(list.name.clone(), field.to_owned()))
        .copied()
        .map(ValueRef::Field)
}

fn synthetic_range_row_field_ref(
    program: &TypedProgram,
    scope_id: Option<ScopeId>,
    path: &str,
) -> Option<ValueRef> {
    let local = path.rsplit('.').next().unwrap_or(path);
    if !matches!(local, "index" | "value") {
        return None;
    }
    let list = program.lists.iter().find(|list| {
        list.row_scope_id == ir_scope_id(scope_id)
            && matches!(list.initializer, ListInitializer::Range { .. })
    })?;
    let ids = synthetic_initial_list_field_ids(program);
    ids.get(&(list.name.clone(), local.to_owned()))
        .copied()
        .map(ValueRef::Field)
}

fn plan_list_remove_predicate(
    index: &ValueIndex,
    predicate: &ListPredicate,
    inputs: &mut Vec<ValueRef>,
) -> PlanListRemovePredicate {
    match predicate {
        ListPredicate::AlwaysTrue => PlanListRemovePredicate::AlwaysTrue,
        ListPredicate::RowFieldBool { path } => match index.resolve(path) {
            Some(input) => {
                inputs.push(input.clone());
                PlanListRemovePredicate::RowFieldBool { input }
            }
            None => PlanListRemovePredicate::Unknown {
                summary: format!("unresolved row field bool predicate `{path}`"),
            },
        },
        ListPredicate::RowFieldBoolNot { path } => match index.resolve(path) {
            Some(input) => {
                inputs.push(input.clone());
                PlanListRemovePredicate::RowFieldBoolNot { input }
            }
            None => PlanListRemovePredicate::Unknown {
                summary: format!("unresolved row field bool-not predicate `{path}`"),
            },
        },
        ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => match (index.resolve(selector), index.resolve(row_field)) {
            (Some(selector), Some(row_field)) => {
                inputs.push(selector.clone());
                inputs.push(row_field.clone());
                PlanListRemovePredicate::SelectedFilterVisibility {
                    selector,
                    row_field,
                }
            }
            _ => PlanListRemovePredicate::Unknown {
                summary: format!(
                    "unresolved selected-filter visibility predicate selector `{selector}` row field `{row_field}`"
                ),
            },
        },
        ListPredicate::Unknown { summary } => PlanListRemovePredicate::Unknown {
            summary: summary.clone(),
        },
    }
}

fn expr_by_id(program: &TypedProgram, id: usize) -> Option<&AstExpr> {
    program.expressions.iter().find(|expr| expr.id == id)
}

fn match_arm_child<'a>(
    statement: &'a AstStatement,
    required_pattern: &str,
    program: &TypedProgram,
) -> Option<&'a AstStatement> {
    statement.children.iter().find(|child| {
        child
            .expr
            .and_then(|expr_id| match &expr_by_id(program, expr_id)?.kind {
                AstExprKind::MatchArm { pattern, .. } => {
                    Some(pattern.iter().any(|item| item == required_pattern))
                }
                _ => None,
            })
            .unwrap_or(false)
    })
}

fn match_arm_output_id(program: &TypedProgram, statement: &AstStatement) -> Option<usize> {
    let expr = expr_by_id(program, statement.expr?)?;
    let AstExprKind::MatchArm { output, .. } = &expr.kind else {
        return None;
    };
    (*output).or_else(|| statement.children.first().and_then(|child| child.expr))
}

fn expression_path_string(program: &TypedProgram, expr_id: usize) -> Option<String> {
    match &expr_by_id(program, expr_id)?.kind {
        AstExprKind::Identifier(value) => Some(value.clone()),
        AstExprKind::Path(parts) => Some(parts.join(".")),
        _ => None,
    }
}

fn text_trim_input_path(
    program: &TypedProgram,
    expr_id: usize,
    derived_path: &str,
) -> Option<String> {
    let expr = expr_by_id(program, expr_id)?;
    let path = match &expr.kind {
        AstExprKind::Pipe { input, op, .. } if op == "Text/trim" => {
            expression_path_string(program, *input)?
        }
        AstExprKind::Call { function, args } if function == "Text/trim" => {
            expression_path_string(program, args.first()?.value)?
        }
        _ => return None,
    };
    Some(canonical_sibling_path(derived_path, &path))
}

fn canonical_sibling_path(parent_path: &str, path: &str) -> String {
    if path.contains('.') {
        return path.to_owned();
    }
    parent_path
        .rsplit_once('.')
        .map(|(parent, _)| format!("{parent}.{path}"))
        .unwrap_or_else(|| path.to_owned())
}

fn scoped_resolution_candidates(parent_path: &str, path: &str) -> Vec<String> {
    if let Some((_, local_name)) = path.rsplit_once('.') {
        return vec![path.to_owned(), local_name.to_owned()];
    }

    let mut candidates = Vec::new();
    let mut scope = parent_path.rsplit_once('.').map(|(parent, _)| parent);
    while let Some(parent) = scope {
        candidates.push(format!("{parent}.{path}"));
        scope = parent.rsplit_once('.').map(|(grandparent, _)| grandparent);
    }
    candidates.push(path.to_owned());
    candidates
}

fn when_has_empty_skip_and_passthrough(statement: &AstStatement, program: &TypedProgram) -> bool {
    let mut has_empty_skip = false;
    let mut has_passthrough = false;
    for child in &statement.children {
        let Some(expr_id) = child.expr else {
            continue;
        };
        let Some(expr) = expr_by_id(program, expr_id) else {
            continue;
        };
        let AstExprKind::MatchArm { pattern, output } = &expr.kind else {
            continue;
        };
        if pattern.iter().any(|item| item == "TEXT" || item == "{}")
            && match_arm_outputs_skip(program, *output, child)
        {
            has_empty_skip = true;
        }
        if pattern.len() == 1 && match_arm_outputs_identifier(program, *output, child, &pattern[0])
        {
            has_passthrough = true;
        }
    }
    has_empty_skip && has_passthrough
}

fn match_arm_outputs_skip(
    program: &TypedProgram,
    output: Option<usize>,
    statement: &AstStatement,
) -> bool {
    match_arm_output_expr(program, output, statement).is_some_and(|expr| {
        matches!(&expr.kind, AstExprKind::Identifier(value) | AstExprKind::Tag(value) if value == "SKIP")
    })
}

fn match_arm_outputs_identifier(
    program: &TypedProgram,
    output: Option<usize>,
    statement: &AstStatement,
    expected: &str,
) -> bool {
    match_arm_output_expr(program, output, statement).is_some_and(
        |expr| matches!(&expr.kind, AstExprKind::Identifier(value) if value == expected),
    )
}

fn match_arm_output_expr<'a>(
    program: &'a TypedProgram,
    output: Option<usize>,
    statement: &AstStatement,
) -> Option<&'a AstExpr> {
    output
        .or_else(|| statement.children.first().and_then(|child| child.expr))
        .and_then(|expr_id| expr_by_id(program, expr_id))
}

fn update_constant_id_for_expression(
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    target: &str,
    expression: &UpdateExpression,
) -> Option<PlanConstantId> {
    let constant_value = match expression {
        UpdateExpression::Const { value } => {
            let target_type = index.state_value_type(target)?;
            update_constant_value(value, target_type)?
        }
        UpdateExpression::BytesGet { index, .. } => {
            let value = i64::try_from(*index).ok()?;
            plan_integer_constant(value)?
        }
        UpdateExpression::ListGet { index, .. } => {
            let value = i64::try_from(*index).ok()?;
            plan_integer_constant(value)?
        }
        _ => return None,
    };
    Some(push_plan_constant(constants, constant_value))
}

fn push_plan_constant(
    constants: &mut Vec<PlanConstant>,
    value: PlanConstantValue,
) -> PlanConstantId {
    if let Some(existing) = constants
        .iter()
        .find(|constant| constant.value == value)
        .map(|constant| constant.id)
    {
        return existing;
    }
    let id = PlanConstantId(constants.len());
    constants.push(PlanConstant { id, value });
    id
}

fn update_constant_value(value: &str, target_type: &PlanValueType) -> Option<PlanConstantValue> {
    match target_type {
        PlanValueType::Text => Some(PlanConstantValue::Text {
            value: value.to_owned(),
        }),
        PlanValueType::Number => plan_number_constant(value),
        PlanValueType::Byte => value
            .parse::<u8>()
            .ok()
            .map(|value| PlanConstantValue::Byte { value }),
        PlanValueType::Bool => match value {
            "True" => Some(PlanConstantValue::Bool { value: true }),
            "False" => Some(PlanConstantValue::Bool { value: false }),
            _ => None,
        },
        PlanValueType::Enum => Some(PlanConstantValue::Enum {
            value: value.to_owned(),
        }),
        PlanValueType::Bytes { .. } => None,
        PlanValueType::Data => None,
        PlanValueType::RootInitialField
        | PlanValueType::RowInitialField
        | PlanValueType::Unknown => match value {
            "True" => Some(PlanConstantValue::Bool { value: true }),
            "False" => Some(PlanConstantValue::Bool { value: false }),
            _ => plan_number_constant(value).or_else(|| {
                Some(PlanConstantValue::Text {
                    value: value.to_owned(),
                })
            }),
        },
    }
}

fn match_const_output_constant_value(
    value: &str,
    target_type: &PlanValueType,
) -> Option<PlanConstantValue> {
    if value == "SKIP" {
        return Some(PlanConstantValue::Text {
            value: value.to_owned(),
        });
    }
    update_constant_value(value, target_type)
}

fn op(
    next_op: &mut usize,
    kind: PlanOpKind,
    inputs: Vec<ValueRef>,
    output: Option<ValueRef>,
    indexed: bool,
    unresolved_executable_ref_count: usize,
) -> PlanOp {
    let id = PlanOpId(*next_op);
    *next_op += 1;
    PlanOp {
        id,
        kind,
        inputs,
        output,
        indexed,
        unresolved_executable_ref_count,
    }
}

fn region(id: usize, kind: RegionKind, ops: Vec<PlanOp>) -> OperationRegion {
    OperationRegion {
        id: PlanRegionId(id),
        kind,
        ops,
    }
}

fn resolve_paths(
    index: &ValueIndex,
    paths: &[String],
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
) -> usize {
    paths
        .iter()
        .map(|path| resolve_path(index, path, refs, unresolved))
        .sum()
}

fn resolve_path(
    index: &ValueIndex,
    path: &str,
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
) -> usize {
    if let Some(value_ref) = index.resolve(path) {
        refs.push(value_ref);
        0
    } else {
        unresolved.insert(path.to_owned());
        1
    }
}

fn resolve_query_ref(
    index: &ValueIndex,
    path: &str,
    refs: &mut Vec<ValueRef>,
    unresolved_count: &mut usize,
    unresolved: &mut BTreeSet<String>,
) -> Option<ValueRef> {
    match index.resolve(path) {
        Some(value_ref) => {
            refs.push(value_ref.clone());
            Some(value_ref)
        }
        None => {
            *unresolved_count += 1;
            unresolved.insert(path.to_owned());
            None
        }
    }
}

fn collect_update_expression_refs(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    expression: &UpdateExpression,
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
) -> usize {
    match expression {
        UpdateExpression::SourcePayload { path } => {
            resolve_source_payload_path(index, source, path, refs, unresolved, true)
        }
        UpdateExpression::PreviousValue { path }
        | UpdateExpression::ReadPath { path }
        | UpdateExpression::BoolNot { path }
        | UpdateExpression::TextToNumber { path }
        | UpdateExpression::BytesLength { path }
        | UpdateExpression::BytesIsEmpty { path }
        | UpdateExpression::BytesGet { path, .. }
        | UpdateExpression::BytesSet { path, .. }
        | UpdateExpression::BytesToHex { path }
        | UpdateExpression::BytesFromHex { path }
        | UpdateExpression::BytesToBase64 { path }
        | UpdateExpression::BytesFromBase64 { path }
        | UpdateExpression::BytesReadUnsigned { path, .. }
        | UpdateExpression::BytesReadSigned { path, .. }
        | UpdateExpression::BytesWriteUnsigned { path, .. }
        | UpdateExpression::BytesWriteSigned { path, .. }
        | UpdateExpression::TextToBytes { path, .. }
        | UpdateExpression::BytesToText { path, .. } => {
            resolve_update_path(index, source, target, indexed, path, refs, unresolved)
        }
        UpdateExpression::ListGet { path, .. } => {
            resolve_update_path(index, source, target, indexed, path, refs, unresolved)
        }
        UpdateExpression::BytesSlice {
            path,
            offset,
            byte_count,
        } => {
            let mut count =
                resolve_update_path(index, source, target, indexed, path, refs, unresolved);
            count += collect_bytes_scalar_arg_ref(
                index, source, target, indexed, offset, refs, unresolved,
            );
            count += collect_bytes_scalar_arg_ref(
                index, source, target, indexed, byte_count, refs, unresolved,
            );
            count
        }
        UpdateExpression::BytesTake { path, byte_count }
        | UpdateExpression::BytesDrop { path, byte_count } => {
            let mut count =
                resolve_update_path(index, source, target, indexed, path, refs, unresolved);
            count += collect_bytes_scalar_arg_ref(
                index, source, target, indexed, byte_count, refs, unresolved,
            );
            count
        }
        UpdateExpression::FileWriteBytes { bytes_path, path } => {
            let unresolved_count =
                resolve_update_path(index, source, target, indexed, bytes_path, refs, unresolved);
            unresolved_count
                + match path {
                    FileBytesPath::StaticText(_) => 0,
                    FileBytesPath::StatePath(path) => {
                        resolve_update_path(index, source, target, indexed, path, refs, unresolved)
                    }
                }
        }
        UpdateExpression::FileReadBytes { path } => match path {
            FileBytesPath::StaticText(_) => 0,
            FileBytesPath::StatePath(path) => {
                resolve_update_path(index, source, target, indexed, path, refs, unresolved)
            }
        },
        UpdateExpression::BytesZeros { .. } => 0,
        UpdateExpression::BytesConcat { left, right }
        | UpdateExpression::BytesEqual { left, right } => {
            resolve_update_path(index, source, target, indexed, left, refs, unresolved)
                + resolve_update_path(index, source, target, indexed, right, refs, unresolved)
        }
        UpdateExpression::BytesFind { haystack, needle } => {
            resolve_update_path(index, source, target, indexed, haystack, refs, unresolved)
                + resolve_update_path(index, source, target, indexed, needle, refs, unresolved)
        }
        UpdateExpression::BytesStartsWith { path, prefix } => {
            resolve_update_path(index, source, target, indexed, path, refs, unresolved)
                + resolve_update_path(index, source, target, indexed, prefix, refs, unresolved)
        }
        UpdateExpression::BytesEndsWith { path, suffix } => {
            resolve_update_path(index, source, target, indexed, path, refs, unresolved)
                + resolve_update_path(index, source, target, indexed, suffix, refs, unresolved)
        }
        UpdateExpression::Const { .. } | UpdateExpression::Unknown { .. } => 0,
        UpdateExpression::NumberInfix { left, right, .. } => {
            collect_number_operand_ref(index, source, target, indexed, left, refs, unresolved)
                + collect_number_operand_ref(
                    index, source, target, indexed, right, refs, unresolved,
                )
        }
        UpdateExpression::MatchInfixConst {
            left, right, arms, ..
        } => {
            let mut count = collect_update_value_expression_refs(
                index, source, target, indexed, left, refs, unresolved,
            ) + collect_update_value_expression_refs(
                index, source, target, indexed, right, refs, unresolved,
            );
            for arm in arms {
                count += collect_update_value_expression_refs(
                    index,
                    source,
                    target,
                    indexed,
                    &arm.output,
                    refs,
                    unresolved,
                );
            }
            count
        }
        UpdateExpression::ProjectTime {
            pointer_x,
            pointer_width,
            viewport_start,
            viewport_end,
            fallback,
        } => {
            resolve_update_path(index, source, target, indexed, pointer_x, refs, unresolved)
                + resolve_update_path(
                    index,
                    source,
                    target,
                    indexed,
                    pointer_width,
                    refs,
                    unresolved,
                )
                + resolve_update_path(
                    index,
                    source,
                    target,
                    indexed,
                    viewport_start,
                    refs,
                    unresolved,
                )
                + resolve_update_path(
                    index,
                    source,
                    target,
                    indexed,
                    viewport_end,
                    refs,
                    unresolved,
                )
                + resolve_update_path(index, source, target, indexed, fallback, refs, unresolved)
        }
        UpdateExpression::TextTrimOrPrevious { path, previous } => {
            resolve_update_path(index, source, target, indexed, path, refs, unresolved)
                + resolve_update_path(index, source, target, indexed, previous, refs, unresolved)
        }
        UpdateExpression::PrefixPayloadConcat { payload_path, .. } => {
            resolve_source_payload_path(index, source, payload_path, refs, unresolved, true)
        }
        UpdateExpression::PrefixRootConcat { path, .. } => {
            resolve_update_path(index, source, target, indexed, path, refs, unresolved)
        }
        UpdateExpression::MatchConst { input, .. } => {
            resolve_update_path(index, source, target, indexed, input, refs, unresolved)
        }
        UpdateExpression::MatchValueConst { input, arms }
        | UpdateExpression::MatchTextIsEmptyConst { input, arms } => {
            let mut count =
                resolve_update_path(index, source, target, indexed, input, refs, unresolved);
            for arm in arms {
                count += collect_update_value_expression_refs(
                    index,
                    source,
                    target,
                    indexed,
                    &arm.output,
                    refs,
                    unresolved,
                );
            }
            count
        }
        UpdateExpression::ListFindValue {
            list,
            field,
            expected,
            target: value_target,
            fallback,
        } => {
            let list_paths = scoped_resolution_candidates(target, list);
            let Some(resolved_list_path) =
                list_paths
                    .iter()
                    .find(|list_path| match index.resolve(list_path) {
                        Some(ValueRef::List(list_id)) => {
                            refs.push(ValueRef::List(list_id));
                            true
                        }
                        _ => false,
                    })
            else {
                unresolved.insert(list.clone());
                return 1;
            };
            let mut count = 0;
            for field_path in [
                format!("{resolved_list_path}.{field}"),
                format!("{resolved_list_path}.{value_target}"),
            ] {
                if let Some(ValueRef::Field(field_id)) = index.resolve(&field_path) {
                    refs.push(ValueRef::Field(field_id));
                } else {
                    unresolved.insert(field_path);
                    count += 1;
                }
            }
            count += collect_update_value_expression_refs(
                index, source, target, indexed, expected, refs, unresolved,
            );
            if let Some(fallback) = fallback {
                count += collect_update_value_expression_refs(
                    index, source, target, indexed, fallback, refs, unresolved,
                );
            }
            count
        }
    }
}

fn collect_number_operand_ref(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    operand: &str,
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
) -> usize {
    if plan_number_constant(operand).is_some() {
        return 0;
    }
    resolve_update_path(index, source, target, indexed, operand, refs, unresolved)
}

fn collect_bytes_scalar_arg_ref(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    arg: &BytesScalarArg,
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
) -> usize {
    match arg {
        BytesScalarArg::Static(_) => 0,
        BytesScalarArg::Path(path) => {
            resolve_update_path(index, source, target, indexed, path, refs, unresolved)
        }
    }
}

fn ordered_update_expression_inputs(
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    source: &str,
    target: &str,
    indexed: bool,
    expression: &UpdateExpression,
) -> Vec<ValueRef> {
    match expression {
        UpdateExpression::TextToNumber { path } => {
            resolve_update_value_ref(index, source, target, indexed, path)
                .into_iter()
                .collect()
        }
        UpdateExpression::BytesLength { path } | UpdateExpression::BytesIsEmpty { path } => {
            resolve_update_value_ref(index, source, target, indexed, path)
                .into_iter()
                .collect()
        }
        UpdateExpression::ListGet {
            path,
            index: list_index,
        } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let Some(index_value) = i64::try_from(*list_index).ok() else {
                return Vec::new();
            };
            let Some(index_constant_id) = push_integer_plan_constant(constants, index_value) else {
                return Vec::new();
            };
            vec![input, ValueRef::Constant(index_constant_id)]
        }
        UpdateExpression::BytesConcat { left, right } => [left, right]
            .into_iter()
            .filter_map(|path| resolve_update_value_ref(index, source, target, indexed, path))
            .collect(),
        UpdateExpression::BytesFind { haystack, needle } => [haystack, needle]
            .into_iter()
            .filter_map(|path| resolve_update_value_ref(index, source, target, indexed, path))
            .collect(),
        UpdateExpression::BytesStartsWith { path, prefix } => [path, prefix]
            .into_iter()
            .filter_map(|path| resolve_update_value_ref(index, source, target, indexed, path))
            .collect(),
        UpdateExpression::BytesEndsWith { path, suffix } => [path, suffix]
            .into_iter()
            .filter_map(|path| resolve_update_value_ref(index, source, target, indexed, path))
            .collect(),
        UpdateExpression::BytesSet {
            path,
            index: byte_index,
            value,
        } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let Some(index_value) = i64::try_from(*byte_index).ok() else {
                return Vec::new();
            };
            let Some(index_constant_id) = push_integer_plan_constant(constants, index_value) else {
                return Vec::new();
            };
            let value_constant_id =
                push_plan_constant(constants, PlanConstantValue::Byte { value: *value });
            vec![
                input,
                ValueRef::Constant(index_constant_id),
                ValueRef::Constant(value_constant_id),
            ]
        }
        UpdateExpression::BytesSlice {
            path,
            offset,
            byte_count,
        } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let Some(offset_ref) =
                bytes_scalar_arg_value_ref(index, constants, source, target, indexed, offset)
            else {
                return Vec::new();
            };
            let Some(byte_count_ref) =
                bytes_scalar_arg_value_ref(index, constants, source, target, indexed, byte_count)
            else {
                return Vec::new();
            };
            vec![input, offset_ref, byte_count_ref]
        }
        UpdateExpression::BytesTake { path, byte_count }
        | UpdateExpression::BytesDrop { path, byte_count } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let Some(byte_count_ref) =
                bytes_scalar_arg_value_ref(index, constants, source, target, indexed, byte_count)
            else {
                return Vec::new();
            };
            vec![input, byte_count_ref]
        }
        UpdateExpression::BytesZeros { byte_count } => {
            let Some(byte_count_value) = i64::try_from(*byte_count).ok() else {
                return Vec::new();
            };
            let Some(byte_count_constant_id) =
                push_integer_plan_constant(constants, byte_count_value)
            else {
                return Vec::new();
            };
            vec![ValueRef::Constant(byte_count_constant_id)]
        }
        UpdateExpression::FileReadBytes { path } => {
            let Some(path_ref) = (match path {
                FileBytesPath::StaticText(path) => {
                    let path_constant_id = push_plan_constant(
                        constants,
                        PlanConstantValue::Text {
                            value: path.clone(),
                        },
                    );
                    Some(ValueRef::Constant(path_constant_id))
                }
                FileBytesPath::StatePath(path) => {
                    resolve_update_value_ref(index, source, target, indexed, path)
                }
            }) else {
                return Vec::new();
            };
            vec![path_ref]
        }
        UpdateExpression::FileWriteBytes { bytes_path, path } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, bytes_path)
            else {
                return Vec::new();
            };
            let Some(path_ref) = (match path {
                FileBytesPath::StaticText(path) => {
                    let path_constant_id = push_plan_constant(
                        constants,
                        PlanConstantValue::Text {
                            value: path.clone(),
                        },
                    );
                    Some(ValueRef::Constant(path_constant_id))
                }
                FileBytesPath::StatePath(path) => {
                    resolve_update_value_ref(index, source, target, indexed, path)
                }
            }) else {
                return Vec::new();
            };
            vec![input, path_ref]
        }
        UpdateExpression::BytesToHex { path } | UpdateExpression::BytesToBase64 { path } => {
            resolve_update_value_ref(index, source, target, indexed, path)
                .into_iter()
                .collect()
        }
        UpdateExpression::BytesFromHex { path } | UpdateExpression::BytesFromBase64 { path } => {
            resolve_update_value_ref(index, source, target, indexed, path)
                .into_iter()
                .collect()
        }
        UpdateExpression::BytesReadUnsigned {
            path,
            offset,
            byte_count,
            endian,
        }
        | UpdateExpression::BytesReadSigned {
            path,
            offset,
            byte_count,
            endian,
        } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let Some(offset_value) = i64::try_from(*offset).ok() else {
                return Vec::new();
            };
            let Some(byte_count_value) = i64::try_from(*byte_count).ok() else {
                return Vec::new();
            };
            let Some(offset_constant_id) = push_integer_plan_constant(constants, offset_value)
            else {
                return Vec::new();
            };
            let Some(byte_count_constant_id) =
                push_integer_plan_constant(constants, byte_count_value)
            else {
                return Vec::new();
            };
            let endian_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: endian.clone(),
                },
            );
            vec![
                input,
                ValueRef::Constant(offset_constant_id),
                ValueRef::Constant(byte_count_constant_id),
                ValueRef::Constant(endian_constant_id),
            ]
        }
        UpdateExpression::BytesWriteUnsigned {
            path,
            offset,
            byte_count,
            endian,
            value,
        }
        | UpdateExpression::BytesWriteSigned {
            path,
            offset,
            byte_count,
            endian,
            value,
        } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let Some(offset_value) = i64::try_from(*offset).ok() else {
                return Vec::new();
            };
            let Some(byte_count_value) = i64::try_from(*byte_count).ok() else {
                return Vec::new();
            };
            let Some(offset_constant_id) = push_integer_plan_constant(constants, offset_value)
            else {
                return Vec::new();
            };
            let Some(byte_count_constant_id) =
                push_integer_plan_constant(constants, byte_count_value)
            else {
                return Vec::new();
            };
            let endian_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: endian.clone(),
                },
            );
            let Some(value_constant_id) = push_integer_plan_constant(constants, *value) else {
                return Vec::new();
            };
            vec![
                input,
                ValueRef::Constant(offset_constant_id),
                ValueRef::Constant(byte_count_constant_id),
                ValueRef::Constant(endian_constant_id),
                ValueRef::Constant(value_constant_id),
            ]
        }
        UpdateExpression::TextToBytes { path, encoding }
        | UpdateExpression::BytesToText { path, encoding } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let encoding_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: encoding.clone(),
                },
            );
            vec![input, ValueRef::Constant(encoding_constant_id)]
        }
        UpdateExpression::NumberInfix { left, op, right } => {
            let Some(left_ref) =
                number_operand_value_ref(index, constants, source, target, indexed, left)
            else {
                return Vec::new();
            };
            let op_constant_id =
                push_plan_constant(constants, PlanConstantValue::Text { value: op.clone() });
            let Some(right_ref) =
                number_operand_value_ref(index, constants, source, target, indexed, right)
            else {
                return Vec::new();
            };
            vec![left_ref, ValueRef::Constant(op_constant_id), right_ref]
        }
        UpdateExpression::ProjectTime {
            pointer_x,
            pointer_width,
            viewport_start,
            viewport_end,
            fallback,
        } => {
            let Some(pointer_x_ref) =
                number_operand_value_ref(index, constants, source, target, indexed, pointer_x)
            else {
                return Vec::new();
            };
            let Some(pointer_width_ref) =
                number_operand_value_ref(index, constants, source, target, indexed, pointer_width)
            else {
                return Vec::new();
            };
            let Some(viewport_start_ref) =
                number_operand_value_ref(index, constants, source, target, indexed, viewport_start)
            else {
                return Vec::new();
            };
            let Some(viewport_end_ref) =
                number_operand_value_ref(index, constants, source, target, indexed, viewport_end)
            else {
                return Vec::new();
            };
            let Some(fallback_ref) =
                number_operand_value_ref(index, constants, source, target, indexed, fallback)
            else {
                return Vec::new();
            };
            vec![
                pointer_x_ref,
                pointer_width_ref,
                viewport_start_ref,
                viewport_end_ref,
                fallback_ref,
            ]
        }
        UpdateExpression::TextTrimOrPrevious { path, previous } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let Some(previous) = resolve_update_value_ref(index, source, target, indexed, previous)
            else {
                return Vec::new();
            };
            vec![input, previous]
        }
        UpdateExpression::MatchInfixConst {
            left,
            op,
            right,
            arms,
        } => {
            let Some(left_ref) =
                infix_operand_value_ref(index, constants, source, target, indexed, left)
            else {
                return Vec::new();
            };
            let op_constant_id =
                push_plan_constant(constants, PlanConstantValue::Text { value: op.clone() });
            let Some(right_ref) =
                infix_operand_value_ref(index, constants, source, target, indexed, right)
            else {
                return Vec::new();
            };
            let mut refs = vec![left_ref, ValueRef::Constant(op_constant_id), right_ref];
            for arm in arms {
                let pattern_constant_id = push_plan_constant(
                    constants,
                    PlanConstantValue::Text {
                        value: arm.pattern.clone(),
                    },
                );
                let Some(mut output_refs) = ordered_update_value_expression_inputs(
                    index,
                    constants,
                    source,
                    target,
                    indexed,
                    &arm.output,
                ) else {
                    return Vec::new();
                };
                refs.push(ValueRef::Constant(pattern_constant_id));
                refs.append(&mut output_refs);
            }
            refs
        }
        UpdateExpression::PrefixPayloadConcat {
            prefix,
            payload_path,
            separator,
        } => {
            let Some(input) =
                resolve_update_value_ref(index, source, target, indexed, payload_path)
            else {
                return Vec::new();
            };
            let prefix_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: prefix.clone(),
                },
            );
            let separator_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: separator.clone(),
                },
            );
            vec![
                ValueRef::Constant(prefix_constant_id),
                input,
                ValueRef::Constant(separator_constant_id),
            ]
        }
        UpdateExpression::PrefixRootConcat {
            prefix,
            path,
            separator,
        } => {
            let Some(input) = resolve_update_value_ref(index, source, target, indexed, path) else {
                return Vec::new();
            };
            let prefix_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: prefix.clone(),
                },
            );
            let separator_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: separator.clone(),
                },
            );
            vec![
                ValueRef::Constant(prefix_constant_id),
                input,
                ValueRef::Constant(separator_constant_id),
            ]
        }
        UpdateExpression::MatchConst { input, arms } => {
            let Some(input_ref) = resolve_update_value_ref(index, source, target, indexed, input)
            else {
                return Vec::new();
            };
            let Some(target_type) = index.state_value_type(target) else {
                return Vec::new();
            };
            let mut refs = vec![input_ref];
            for arm in arms {
                let pattern_constant_id = push_plan_constant(
                    constants,
                    PlanConstantValue::Text {
                        value: arm.pattern.clone(),
                    },
                );
                let Some(output_constant) =
                    match_const_output_constant_value(&arm.output, target_type)
                else {
                    return Vec::new();
                };
                let output_constant_id = push_plan_constant(constants, output_constant);
                refs.push(ValueRef::Constant(pattern_constant_id));
                refs.push(ValueRef::Constant(output_constant_id));
            }
            refs
        }
        UpdateExpression::MatchValueConst { input, arms }
        | UpdateExpression::MatchTextIsEmptyConst { input, arms } => {
            let Some(input_ref) = resolve_update_value_ref(index, source, target, indexed, input)
            else {
                return Vec::new();
            };
            let mut refs = vec![input_ref];
            let patterns = match &expression {
                UpdateExpression::MatchTextIsEmptyConst { .. } => {
                    vec!["True".to_owned(), "False".to_owned(), "__".to_owned()]
                }
                _ => arms.iter().map(|arm| arm.pattern.clone()).collect(),
            };
            for pattern in patterns {
                let Some(arm) = arms.iter().find(|arm| arm.pattern == pattern) else {
                    continue;
                };
                let pattern_constant_id = push_plan_constant(
                    constants,
                    PlanConstantValue::Text {
                        value: pattern.clone(),
                    },
                );
                let Some(mut output_refs) = ordered_update_value_expression_inputs(
                    index,
                    constants,
                    source,
                    target,
                    indexed,
                    &arm.output,
                ) else {
                    continue;
                };
                refs.push(ValueRef::Constant(pattern_constant_id));
                refs.append(&mut output_refs);
            }
            refs
        }
        UpdateExpression::ListFindValue {
            list,
            field,
            expected,
            target: value_target,
            fallback,
        } => {
            let list_paths = scoped_resolution_candidates(target, list);
            let Some((resolved_list_path, list_ref @ ValueRef::List(_))) =
                list_paths.iter().find_map(|list_path| {
                    index.resolve(list_path).and_then(|value_ref| {
                        matches!(value_ref, ValueRef::List(_))
                            .then_some((list_path.as_str(), value_ref))
                    })
                })
            else {
                return Vec::new();
            };
            let Some(field_ref @ ValueRef::Field(_)) =
                index.resolve(&format!("{resolved_list_path}.{field}"))
            else {
                return Vec::new();
            };
            let Some(expected_ref) = update_value_expression_value_ref(
                index, constants, source, target, indexed, expected,
            ) else {
                return Vec::new();
            };
            let Some(target_ref @ ValueRef::Field(_)) =
                index.resolve(&format!("{resolved_list_path}.{value_target}"))
            else {
                return Vec::new();
            };
            let mut refs = vec![list_ref, field_ref, expected_ref, target_ref];
            if let Some(fallback) = fallback {
                let Some(fallback_ref) = update_value_expression_value_ref(
                    index, constants, source, target, indexed, fallback,
                ) else {
                    return Vec::new();
                };
                refs.push(fallback_ref);
            }
            refs
        }
        _ => Vec::new(),
    }
}

fn number_operand_value_ref(
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    source: &str,
    target: &str,
    indexed: bool,
    operand: &str,
) -> Option<ValueRef> {
    if let Some(value) = plan_number_constant(operand) {
        let constant_id = push_plan_constant(constants, value);
        return Some(ValueRef::Constant(constant_id));
    }
    resolve_update_value_ref(index, source, target, indexed, operand)
}

fn infix_operand_value_ref(
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    source: &str,
    target: &str,
    indexed: bool,
    expression: &UpdateValueExpression,
) -> Option<ValueRef> {
    match expression {
        UpdateValueExpression::Const { value } => {
            let value = match value.as_str() {
                "True" => PlanConstantValue::Bool { value: true },
                "False" => PlanConstantValue::Bool { value: false },
                _ => plan_number_constant(value).unwrap_or_else(|| PlanConstantValue::Text {
                    value: value.clone(),
                }),
            };
            Some(ValueRef::Constant(push_plan_constant(constants, value)))
        }
        UpdateValueExpression::ReadPath { path } => {
            resolve_update_value_ref(index, source, target, indexed, path)
        }
        UpdateValueExpression::MatchConst { .. }
        | UpdateValueExpression::MatchTextIsEmptyConst { .. }
        | UpdateValueExpression::NumberInfix { .. }
        | UpdateValueExpression::MatchInfixConst { .. } => None,
    }
}

fn update_value_expression_value_ref(
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    source: &str,
    target: &str,
    indexed: bool,
    expression: &UpdateValueExpression,
) -> Option<ValueRef> {
    match expression {
        UpdateValueExpression::Const { value } => {
            let constant_value = index
                .state_value_type(target)
                .and_then(|target_type| update_constant_value(value, target_type))
                .unwrap_or_else(|| PlanConstantValue::Text {
                    value: value.clone(),
                });
            let constant_id = push_plan_constant(constants, constant_value);
            Some(ValueRef::Constant(constant_id))
        }
        UpdateValueExpression::ReadPath { path } => {
            resolve_update_value_ref(index, source, target, indexed, path)
        }
        UpdateValueExpression::MatchConst { .. }
        | UpdateValueExpression::MatchTextIsEmptyConst { .. }
        | UpdateValueExpression::NumberInfix { .. }
        | UpdateValueExpression::MatchInfixConst { .. } => None,
    }
}

fn ordered_update_value_expression_inputs(
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    source: &str,
    target: &str,
    indexed: bool,
    expression: &UpdateValueExpression,
) -> Option<Vec<ValueRef>> {
    match expression {
        UpdateValueExpression::Const { .. } | UpdateValueExpression::ReadPath { .. } => {
            let tag_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: "ref".to_owned(),
                },
            );
            let value_ref = update_value_expression_value_ref(
                index, constants, source, target, indexed, expression,
            )?;
            Some(vec![ValueRef::Constant(tag_constant_id), value_ref])
        }
        UpdateValueExpression::MatchConst { input, arms } => {
            let tag_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: "match_const".to_owned(),
                },
            );
            let input_ref = resolve_update_value_ref(index, source, target, indexed, input)?;
            let arm_count = i64::try_from(arms.len()).ok()?;
            let arm_count_constant_id =
                push_plan_constant(constants, plan_integer_constant(arm_count)?);
            let mut refs = vec![
                ValueRef::Constant(tag_constant_id),
                input_ref,
                ValueRef::Constant(arm_count_constant_id),
            ];
            for arm in arms {
                let pattern_constant_id = push_plan_constant(
                    constants,
                    PlanConstantValue::Text {
                        value: arm.pattern.clone(),
                    },
                );
                let mut output_refs = ordered_update_value_expression_inputs(
                    index,
                    constants,
                    source,
                    target,
                    indexed,
                    &arm.output,
                )?;
                refs.push(ValueRef::Constant(pattern_constant_id));
                refs.append(&mut output_refs);
            }
            Some(refs)
        }
        UpdateValueExpression::MatchTextIsEmptyConst { input, arms } => {
            let tag_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: "match_text_is_empty_const".to_owned(),
                },
            );
            let input_ref = resolve_update_value_ref(index, source, target, indexed, input)?;
            let arm_count = i64::try_from(arms.len()).ok()?;
            let arm_count_constant_id =
                push_plan_constant(constants, plan_integer_constant(arm_count)?);
            let mut refs = vec![
                ValueRef::Constant(tag_constant_id),
                input_ref,
                ValueRef::Constant(arm_count_constant_id),
            ];
            for arm in arms {
                let pattern_constant_id = push_plan_constant(
                    constants,
                    PlanConstantValue::Text {
                        value: arm.pattern.clone(),
                    },
                );
                let mut output_refs = ordered_update_value_expression_inputs(
                    index,
                    constants,
                    source,
                    target,
                    indexed,
                    &arm.output,
                )?;
                refs.push(ValueRef::Constant(pattern_constant_id));
                refs.append(&mut output_refs);
            }
            Some(refs)
        }
        UpdateValueExpression::NumberInfix { left, op, right } => {
            let tag_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: "number_infix".to_owned(),
                },
            );
            let left_ref =
                number_operand_value_ref(index, constants, source, target, indexed, left)?;
            let op_constant_id =
                push_plan_constant(constants, PlanConstantValue::Text { value: op.clone() });
            let right_ref =
                number_operand_value_ref(index, constants, source, target, indexed, right)?;
            Some(vec![
                ValueRef::Constant(tag_constant_id),
                left_ref,
                ValueRef::Constant(op_constant_id),
                right_ref,
            ])
        }
        UpdateValueExpression::MatchInfixConst {
            left,
            op,
            right,
            arms,
        } => {
            let tag_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: "match_infix_const".to_owned(),
                },
            );
            let left_ref =
                number_operand_value_ref(index, constants, source, target, indexed, left)?;
            let op_constant_id =
                push_plan_constant(constants, PlanConstantValue::Text { value: op.clone() });
            let right_ref =
                number_operand_value_ref(index, constants, source, target, indexed, right)?;
            let arm_count = i64::try_from(arms.len()).ok()?;
            let arm_count_constant_id =
                push_plan_constant(constants, plan_integer_constant(arm_count)?);
            let mut refs = vec![
                ValueRef::Constant(tag_constant_id),
                left_ref,
                ValueRef::Constant(op_constant_id),
                right_ref,
                ValueRef::Constant(arm_count_constant_id),
            ];
            for arm in arms {
                let pattern_constant_id = push_plan_constant(
                    constants,
                    PlanConstantValue::Text {
                        value: arm.pattern.clone(),
                    },
                );
                let mut output_refs = ordered_update_value_expression_inputs(
                    index,
                    constants,
                    source,
                    target,
                    indexed,
                    &arm.output,
                )?;
                refs.push(ValueRef::Constant(pattern_constant_id));
                refs.append(&mut output_refs);
            }
            Some(refs)
        }
    }
}

fn resolve_update_value_ref(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    path: &str,
) -> Option<ValueRef> {
    if let Some(field) = index.source_field_payload_alias(source, path)
        && let Some(ValueRef::Source(source_id)) = index.resolve(source)
    {
        return Some(ValueRef::SourcePayload { source_id, field });
    }
    if let Some(field) = source_row_lookup_payload_field_from_path(index, source, path)
        && let Some(ValueRef::Source(source_id)) = index.resolve(source)
    {
        return Some(ValueRef::SourcePayload { source_id, field });
    }
    if let Some(value_ref) = index.resolve(path) {
        return Some(value_ref);
    }
    if let Some(alias_ref) = resolve_row_alias(index, target, indexed, path) {
        return Some(alias_ref);
    }
    if let Some(field) = source_payload_field_from_path(source, path, true)
        && index.source_has_payload_field(source, &field)
        && let Some(ValueRef::Source(source_id)) = index.resolve(source)
    {
        return Some(ValueRef::SourcePayload { source_id, field });
    }
    None
}

fn source_guard_for_update_guard(
    index: &ValueIndex,
    source: &str,
    guard: Option<&UpdateGuard>,
    refs: &mut Vec<ValueRef>,
    unresolved_refs: &mut BTreeSet<String>,
    unresolved: &mut usize,
) -> Option<PlanSourceGuard> {
    let guard = guard?;
    let Some(ValueRef::Source(source_id)) = index.resolve(source) else {
        unresolved_refs.insert(source.to_owned());
        *unresolved += 1;
        return None;
    };
    match guard {
        UpdateGuard::SourcePayloadOneOf { field, values } => {
            let field = source_payload_field_from_ir(field);
            refs.push(ValueRef::SourcePayload {
                source_id,
                field: field.clone(),
            });
            Some(PlanSourceGuard::SourcePayloadOneOf {
                source_id,
                field,
                values: values.clone(),
            })
        }
    }
}

fn collect_update_value_expression_refs(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    expression: &UpdateValueExpression,
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
) -> usize {
    match expression {
        UpdateValueExpression::Const { .. } => 0,
        UpdateValueExpression::ReadPath { path } => {
            resolve_update_path(index, source, target, indexed, path, refs, unresolved)
        }
        UpdateValueExpression::MatchConst { input, arms } => {
            let mut count =
                resolve_update_path(index, source, target, indexed, input, refs, unresolved);
            for arm in arms {
                count += collect_update_value_expression_refs(
                    index,
                    source,
                    target,
                    indexed,
                    &arm.output,
                    refs,
                    unresolved,
                );
            }
            count
        }
        UpdateValueExpression::MatchTextIsEmptyConst { input, arms } => {
            let mut count =
                resolve_update_path(index, source, target, indexed, input, refs, unresolved);
            for arm in arms {
                count += collect_update_value_expression_refs(
                    index,
                    source,
                    target,
                    indexed,
                    &arm.output,
                    refs,
                    unresolved,
                );
            }
            count
        }
        UpdateValueExpression::NumberInfix { left, right, .. } => {
            collect_number_operand_ref(index, source, target, indexed, left, refs, unresolved)
                + collect_number_operand_ref(
                    index, source, target, indexed, right, refs, unresolved,
                )
        }
        UpdateValueExpression::MatchInfixConst {
            left, right, arms, ..
        } => {
            let mut count =
                collect_number_operand_ref(index, source, target, indexed, left, refs, unresolved)
                    + collect_number_operand_ref(
                        index, source, target, indexed, right, refs, unresolved,
                    );
            for arm in arms {
                count += collect_update_value_expression_refs(
                    index,
                    source,
                    target,
                    indexed,
                    &arm.output,
                    refs,
                    unresolved,
                );
            }
            count
        }
    }
}

fn resolve_update_path(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    path: &str,
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
) -> usize {
    if let Some(field) = index.source_field_payload_alias(source, path)
        && let Some(ValueRef::Source(source_id)) = index.resolve(source)
    {
        refs.push(ValueRef::SourcePayload { source_id, field });
        return 0;
    }
    if let Some(field) = source_row_lookup_payload_field_from_path(index, source, path)
        && let Some(ValueRef::Source(source_id)) = index.resolve(source)
    {
        refs.push(ValueRef::SourcePayload { source_id, field });
        return 0;
    }
    if let Some(value_ref) = index.resolve(path) {
        refs.push(value_ref);
        return 0;
    }
    if let Some(alias_ref) = resolve_row_alias(index, target, indexed, path) {
        refs.push(alias_ref);
        return 0;
    }
    if source_payload_field_from_path(source, path, true)
        .is_some_and(|field| index.source_has_payload_field(source, &field))
    {
        return resolve_source_payload_path(index, source, path, refs, unresolved, true);
    }
    resolve_path(index, path, refs, unresolved)
}

fn source_row_lookup_payload_field_from_path(
    index: &ValueIndex,
    source: &str,
    path: &str,
) -> Option<SourcePayloadField> {
    let row_lookup_field = index.source_row_lookup_field(source)?;
    let matches_row_lookup = path == row_lookup_field
        || path
            .rsplit_once('.')
            .is_some_and(|(scope, field)| field == row_lookup_field && source.starts_with(scope));
    matches_row_lookup
        .then_some(SourcePayloadField::Address)
        .filter(|field| index.source_has_payload_field(source, field))
}

fn source_field_payload_aliases_from_program(
    program: &TypedProgram,
    source_payload_fields: &BTreeMap<String, BTreeSet<SourcePayloadField>>,
    source_row_lookup_fields: &BTreeMap<String, String>,
) -> BTreeMap<(String, String), SourcePayloadField> {
    let mut aliases = BTreeMap::new();
    for derived in &program.derived_values {
        if derived.kind != DerivedValueKind::SourceEventTransform {
            continue;
        }
        for source in &derived.sources {
            if let Some(field) = source_event_transform_row_lookup_payload_alias(
                program,
                derived,
                source,
                source_payload_fields,
                source_row_lookup_fields,
            ) {
                aliases.insert((source.clone(), derived.path.clone()), field);
            }
        }
    }

    let pure_latest_refs = program
        .derived_values
        .iter()
        .filter(|derived| derived.kind == DerivedValueKind::Pure)
        .filter_map(|derived| {
            let refs = pure_latest_reference_paths(program, derived);
            (!refs.is_empty()).then(|| (derived.path.clone(), refs))
        })
        .collect::<Vec<_>>();

    let mut changed = true;
    while changed {
        changed = false;
        for (target, refs) in &pure_latest_refs {
            let source_aliases = aliases
                .iter()
                .filter(|((_source, path), _field)| {
                    refs.iter()
                        .any(|reference| reference.as_str() == path.as_str())
                })
                .map(|((source, _path), field)| (source.clone(), field.clone()))
                .collect::<Vec<_>>();
            for (source, field) in source_aliases {
                if aliases.insert((source, target.clone()), field).is_none() {
                    changed = true;
                }
            }
        }
    }

    aliases
}

fn source_event_transform_row_lookup_payload_alias(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    source: &str,
    source_payload_fields: &BTreeMap<String, BTreeSet<SourcePayloadField>>,
    source_row_lookup_fields: &BTreeMap<String, String>,
) -> Option<SourcePayloadField> {
    let super::CompilerDerivedTextExpression::SourceRootText { path } =
        super::compiler_source_event_transform_text_expression(
            derived,
            source,
            &program.expressions,
            &program.functions,
        )
    else {
        return None;
    };
    source_row_lookup_payload_field_from_path_maps(
        source_payload_fields,
        source_row_lookup_fields,
        source,
        &path,
    )
}

fn source_row_lookup_payload_field_from_path_maps(
    source_payload_fields: &BTreeMap<String, BTreeSet<SourcePayloadField>>,
    source_row_lookup_fields: &BTreeMap<String, String>,
    source: &str,
    path: &str,
) -> Option<SourcePayloadField> {
    let row_lookup_field = source_row_lookup_fields.get(source)?;
    let matches_row_lookup = path == row_lookup_field
        || path
            .rsplit_once('.')
            .is_some_and(|(scope, field)| field == row_lookup_field && source.starts_with(scope));
    let field = matches_row_lookup.then_some(SourcePayloadField::Address)?;
    source_payload_fields
        .get(source)
        .is_some_and(|fields| fields.contains(&field))
        .then_some(field)
}

fn pure_latest_reference_paths(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
) -> Vec<String> {
    let exprs = super::compiler_statement_ast_exprs(&derived.statement, &program.expressions);
    if !exprs
        .iter()
        .any(|expr| matches!(expr.kind, AstExprKind::Latest))
    {
        return Vec::new();
    }
    let mut refs = exprs
        .iter()
        .filter_map(|expr| expression_path_string(program, expr.id))
        .map(|path| canonical_sibling_path(&derived.path, &path))
        .collect::<Vec<_>>();
    refs.sort();
    refs.dedup();
    refs
}

fn resolve_row_alias(
    index: &ValueIndex,
    target: &str,
    indexed: bool,
    path: &str,
) -> Option<ValueRef> {
    if !indexed || path.is_empty() || path.contains('.') {
        return None;
    }
    let (scope, _) = target.rsplit_once('.')?;
    index.resolve(&format!("{scope}.{path}"))
}

fn resolve_source_payload_path(
    index: &ValueIndex,
    source: &str,
    path: &str,
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
    allow_bare_field: bool,
) -> usize {
    let Some(field) = source_payload_field_from_path(source, path, allow_bare_field) else {
        return resolve_path(index, path, refs, unresolved);
    };
    if !index.source_has_payload_field(source, &field) {
        return resolve_path(index, path, refs, unresolved);
    }
    let Some(ValueRef::Source(source_id)) = index.resolve(source) else {
        unresolved.insert(source.to_owned());
        return 1;
    };
    refs.push(ValueRef::SourcePayload { source_id, field });
    0
}

fn source_payload_field_for_expression(
    index: &ValueIndex,
    source: &str,
    expression: &UpdateExpression,
) -> Option<SourcePayloadField> {
    let field = match expression {
        UpdateExpression::SourcePayload { path } => {
            source_payload_field_from_path(source, path, true)
        }
        UpdateExpression::ReadPath { path } => source_payload_field_from_path(source, path, true),
        UpdateExpression::TextToNumber { path } => {
            source_payload_field_from_path(source, path, true)
        }
        UpdateExpression::PrefixPayloadConcat { payload_path, .. } => {
            source_payload_field_from_path(source, payload_path, true)
        }
        UpdateExpression::TextTrimOrPrevious { path, .. } => {
            source_payload_field_from_path(source, path, true)
        }
        UpdateExpression::BytesLength { path }
        | UpdateExpression::BytesIsEmpty { path }
        | UpdateExpression::BytesToHex { path }
        | UpdateExpression::BytesToBase64 { path }
        | UpdateExpression::BytesToText { path, .. } => {
            source_payload_field_from_path(source, path, true)
        }
        _ => None,
    }?;
    index
        .source_has_payload_field(source, &field)
        .then_some(field)
}

fn source_payload_field_from_path(
    source: &str,
    path: &str,
    allow_bare_field: bool,
) -> Option<SourcePayloadField> {
    if allow_bare_field && !path.is_empty() && !path.contains('.') {
        return source_payload_field_from_suffix(path);
    }
    source_event_ref_variants(source)
        .into_iter()
        .find_map(|variant| {
            let suffix = source_payload_suffix_from_variant(path, &variant)?;
            source_payload_field_from_suffix(suffix)
        })
}

fn source_payload_field_from_suffix(suffix: &str) -> Option<SourcePayloadField> {
    match suffix {
        "text" | "change.text" | "event.change.text" | "events.change.text" => {
            Some(SourcePayloadField::Text)
        }
        "bytes" | "change.bytes" | "event.change.bytes" | "events.change.bytes" => {
            Some(SourcePayloadField::Bytes)
        }
        "key" | "key_down.key" | "event.key_down.key" | "events.key_down.key" => {
            Some(SourcePayloadField::Key)
        }
        "address" | "event.address" | "events.address" => Some(SourcePayloadField::Address),
        _ if !suffix.is_empty() && !suffix.contains('.') => {
            Some(SourcePayloadField::Named(suffix.to_owned()))
        }
        _ if suffix.starts_with("event.") && !suffix["event.".len()..].contains('.') => Some(
            SourcePayloadField::Named(suffix["event.".len()..].to_owned()),
        ),
        _ if suffix.starts_with("events.") && !suffix["events.".len()..].contains('.') => Some(
            SourcePayloadField::Named(suffix["events.".len()..].to_owned()),
        ),
        _ => None,
    }
}

fn source_payload_suffix_from_variant<'a>(path: &'a str, variant: &str) -> Option<&'a str> {
    if let Some(suffix) = source_suffix_after_variant(path, variant) {
        return Some(suffix);
    }
    let (base, event) = variant.rsplit_once('.')?;
    for event_prefix in [
        format!("{base}.event.{event}"),
        format!("{base}.events.{event}"),
    ] {
        if let Some(suffix) = source_suffix_after_variant(path, &event_prefix) {
            return Some(suffix);
        }
    }
    None
}

fn source_suffix_after_variant<'a>(path: &'a str, variant: &str) -> Option<&'a str> {
    if path == variant {
        return Some("");
    }
    if let Some(suffix) = path
        .strip_prefix(variant)
        .and_then(|suffix| suffix.strip_prefix('.'))
    {
        return Some(suffix);
    }
    let dotted_variant = format!(".{variant}");
    let start = path.find(&dotted_variant)?;
    let suffix = &path[start + dotted_variant.len()..];
    if suffix.is_empty() {
        return Some("");
    }
    suffix.strip_prefix('.')
}

fn source_event_ref_variants(source: &str) -> Vec<String> {
    let mut variants = vec![source.to_owned()];
    if let Some((_, suffix)) = source.split_once('.') {
        variants.push(suffix.to_owned());
        variants.push(format!("item.{suffix}"));
    }
    variants
}

fn bytes_scalar_arg_value_ref(
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    source: &str,
    target: &str,
    indexed: bool,
    arg: &BytesScalarArg,
) -> Option<ValueRef> {
    match arg {
        BytesScalarArg::Static(value) => {
            let value = i64::try_from(*value).ok()?;
            Some(ValueRef::Constant(push_plan_constant(
                constants,
                plan_integer_constant(value)?,
            )))
        }
        BytesScalarArg::Path(path) => {
            resolve_update_value_ref(index, source, target, indexed, path)
        }
    }
}

fn update_expression_kind_for_branch(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    expression: &UpdateExpression,
) -> PlanExpressionKind {
    if matches!(expression, UpdateExpression::ReadPath { .. })
        && source_payload_field_for_branch(index, source, target, indexed, expression).is_some()
    {
        return PlanExpressionKind::SourcePayload;
    }
    update_expression_kind(expression)
}

fn source_payload_field_for_branch(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    expression: &UpdateExpression,
) -> Option<SourcePayloadField> {
    source_payload_field_for_expression(index, source, expression).or_else(|| {
        let UpdateExpression::ReadPath { path } = expression else {
            return None;
        };
        match resolve_update_value_ref(index, source, target, indexed, path) {
            Some(ValueRef::SourcePayload { field, .. }) => Some(field),
            _ => None,
        }
    })
}

fn update_expression_kind(expression: &UpdateExpression) -> PlanExpressionKind {
    match expression {
        UpdateExpression::SourcePayload { .. } => PlanExpressionKind::SourcePayload,
        UpdateExpression::Const { .. } => PlanExpressionKind::Const,
        UpdateExpression::NumberInfix { .. } => PlanExpressionKind::NumberInfix,
        UpdateExpression::ProjectTime { .. } => PlanExpressionKind::ProjectTime,
        UpdateExpression::PreviousValue { .. } => PlanExpressionKind::PreviousValue,
        UpdateExpression::ReadPath { .. } => PlanExpressionKind::ReadPath,
        UpdateExpression::TextTrimOrPrevious { .. } => PlanExpressionKind::TextTrimOrPrevious,
        UpdateExpression::PrefixPayloadConcat { .. } => PlanExpressionKind::PrefixPayloadConcat,
        UpdateExpression::PrefixRootConcat { .. } => PlanExpressionKind::PrefixRootConcat,
        UpdateExpression::BoolNot { .. } => PlanExpressionKind::BoolNot,
        UpdateExpression::TextToNumber { .. } => PlanExpressionKind::TextToNumber,
        UpdateExpression::BytesLength { .. } => PlanExpressionKind::BytesLength,
        UpdateExpression::BytesIsEmpty { .. } => PlanExpressionKind::BytesIsEmpty,
        UpdateExpression::BytesGet { .. } => PlanExpressionKind::BytesGet,
        UpdateExpression::ListGet { .. } => PlanExpressionKind::ListGet,
        UpdateExpression::BytesSet { .. } => PlanExpressionKind::BytesSet,
        UpdateExpression::BytesSlice { .. } => PlanExpressionKind::BytesSlice,
        UpdateExpression::BytesTake { .. } => PlanExpressionKind::BytesTake,
        UpdateExpression::BytesDrop { .. } => PlanExpressionKind::BytesDrop,
        UpdateExpression::BytesZeros { .. } => PlanExpressionKind::BytesZeros,
        UpdateExpression::BytesToHex { .. } => PlanExpressionKind::BytesToHex,
        UpdateExpression::BytesFromHex { .. } => PlanExpressionKind::BytesFromHex,
        UpdateExpression::BytesToBase64 { .. } => PlanExpressionKind::BytesToBase64,
        UpdateExpression::BytesFromBase64 { .. } => PlanExpressionKind::BytesFromBase64,
        UpdateExpression::BytesReadUnsigned { .. } => PlanExpressionKind::BytesReadUnsigned,
        UpdateExpression::BytesReadSigned { .. } => PlanExpressionKind::BytesReadSigned,
        UpdateExpression::BytesWriteUnsigned { .. } => PlanExpressionKind::BytesWriteUnsigned,
        UpdateExpression::BytesWriteSigned { .. } => PlanExpressionKind::BytesWriteSigned,
        UpdateExpression::FileReadBytes { .. } => PlanExpressionKind::FileReadBytes,
        UpdateExpression::FileWriteBytes { .. } => PlanExpressionKind::FileWriteBytes,
        UpdateExpression::TextToBytes { .. } => PlanExpressionKind::TextToBytes,
        UpdateExpression::BytesToText { .. } => PlanExpressionKind::BytesToText,
        UpdateExpression::BytesConcat { .. } => PlanExpressionKind::BytesConcat,
        UpdateExpression::BytesEqual { .. } => PlanExpressionKind::BytesEqual,
        UpdateExpression::BytesFind { .. } => PlanExpressionKind::BytesFind,
        UpdateExpression::BytesStartsWith { .. } => PlanExpressionKind::BytesStartsWith,
        UpdateExpression::BytesEndsWith { .. } => PlanExpressionKind::BytesEndsWith,
        UpdateExpression::MatchConst { .. } => PlanExpressionKind::MatchConst,
        UpdateExpression::MatchValueConst { .. } => PlanExpressionKind::MatchValueConst,
        UpdateExpression::MatchTextIsEmptyConst { .. } => PlanExpressionKind::MatchTextIsEmptyConst,
        UpdateExpression::MatchInfixConst { .. } => PlanExpressionKind::MatchInfixConst,
        UpdateExpression::ListFindValue { .. } => PlanExpressionKind::ListFindValue,
        UpdateExpression::Unknown { .. } => PlanExpressionKind::Unknown,
    }
}

fn unique_value_refs(value_refs: Vec<ValueRef>) -> Vec<ValueRef> {
    value_refs
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn delta_routes(program: &TypedProgram) -> Vec<DeltaRoute> {
    let mut routes = Vec::new();
    for state in &program.state_cells {
        routes.push(DeltaRoute {
            id: PlanDeltaId(routes.len()),
            output: ValueRef::State(plan_state_id(state.id)),
        });
    }
    for list in &program.lists {
        routes.push(DeltaRoute {
            id: PlanDeltaId(routes.len()),
            output: ValueRef::List(plan_list_id(list.id)),
        });
    }
    for derived in &program.derived_values {
        routes.push(DeltaRoute {
            id: PlanDeltaId(routes.len()),
            output: derived_output_ref(program, derived),
        });
    }
    routes
}

fn derived_output_ref(program: &TypedProgram, derived: &boon_ir::DerivedValue) -> ValueRef {
    if let Some(field) = program
        .semantic_index
        .fields
        .iter()
        .find(|field| field.path == derived.path)
    {
        return ValueRef::Field(plan_field_id(field.id));
    }
    ValueRef::Field(plan_field_id(derived.id))
}

struct ValueIndex {
    by_path: BTreeMap<String, ValueRef>,
    source_payload_fields: BTreeMap<String, BTreeSet<SourcePayloadField>>,
    source_row_lookup_fields: BTreeMap<String, String>,
    source_field_payload_aliases: BTreeMap<(String, String), SourcePayloadField>,
    state_value_types: BTreeMap<String, PlanValueType>,
    field_value_types: BTreeMap<FieldId, PlanValueType>,
}

impl ValueIndex {
    fn new(
        program: &TypedProgram,
        root_field_types: &RootInitialFieldTypeMap,
        row_field_types: &RowInitialFieldTypeMap,
    ) -> Self {
        let mut by_path = BTreeMap::new();
        let mut source_payload_fields = BTreeMap::new();
        let mut source_row_lookup_fields = BTreeMap::new();
        let mut state_value_types = BTreeMap::new();
        let mut field_value_types = BTreeMap::new();
        let expression_types = expression_value_type_lookup(program);
        let synthetic_field_ids = synthetic_initial_list_field_ids(program);
        for source in &program.sources {
            by_path.insert(
                source.path.clone(),
                ValueRef::Source(plan_source_id(source.id)),
            );
            source_payload_fields.insert(
                source.path.clone(),
                source
                    .payload_schema
                    .fields
                    .iter()
                    .map(source_payload_field_from_ir)
                    .collect(),
            );
            if let Some(row_lookup_field) = source.payload_schema.row_lookup_field_name() {
                source_row_lookup_fields.insert(source.path.clone(), row_lookup_field.to_owned());
            }
        }
        for state in &program.state_cells {
            by_path.insert(state.path.clone(), ValueRef::State(plan_state_id(state.id)));
            state_value_types.insert(
                state.path.clone(),
                migration_storage_default(program, state).map_or_else(
                    || {
                        state_initial_value_type(
                            program,
                            state,
                            root_field_types,
                            row_field_types,
                            &expression_types,
                        )
                    },
                    |default| default.value_type,
                ),
            );
        }
        for list in &program.lists {
            by_path.insert(list.name.clone(), ValueRef::List(plan_list_id(list.id)));
            if let Some((_, local_name)) = list.name.rsplit_once('.') {
                by_path
                    .entry(local_name.to_owned())
                    .or_insert(ValueRef::List(plan_list_id(list.id)));
            }
            if let ListInitializer::RecordLiteral { rows } = &list.initializer {
                for row in rows {
                    for field in &row.fields {
                        if let Some(field_id) = storage_input_field_id(
                            program,
                            &list.name,
                            &field.name,
                            &synthetic_field_ids,
                        ) {
                            by_path
                                .entry(format!("{}.{}", list.name, field.name))
                                .or_insert(ValueRef::Field(field_id));
                            if let Some((_, local_name)) = list.name.rsplit_once('.') {
                                by_path
                                    .entry(format!("{local_name}.{}", field.name))
                                    .or_insert(ValueRef::Field(field_id));
                            }
                            let value_type = plan_value_type_from_initial_with_row_fields(
                                &field.value,
                                plan_scope_id(list.row_scope_id),
                                row_field_types,
                            );
                            insert_field_value_type(&mut field_value_types, field_id, value_type);
                        }
                    }
                }
            }
        }
        for derived in &program.derived_values {
            let output_ref = derived_output_ref(program, derived);
            if let ValueRef::Field(field_id) = &output_ref
                && let Some(value_type) =
                    derived_value_output_type(program, derived, &expression_types)
            {
                insert_field_value_type_if_absent(&mut field_value_types, *field_id, value_type);
            }
            by_path.insert(derived.path.clone(), output_ref);
        }
        for field in &program.semantic_index.fields {
            by_path
                .entry(field.path.clone())
                .or_insert(ValueRef::Field(plan_field_id(field.id)));
        }
        let source_field_payload_aliases = source_field_payload_aliases_from_program(
            program,
            &source_payload_fields,
            &source_row_lookup_fields,
        );
        Self {
            by_path,
            source_payload_fields,
            source_row_lookup_fields,
            source_field_payload_aliases,
            state_value_types,
            field_value_types,
        }
    }

    fn resolve(&self, path: &str) -> Option<ValueRef> {
        self.by_path.get(path).cloned()
    }

    fn source_has_payload_field(&self, source: &str, field: &SourcePayloadField) -> bool {
        self.source_payload_fields
            .get(source)
            .is_some_and(|fields| fields.contains(field))
    }

    fn source_row_lookup_field(&self, source: &str) -> Option<&str> {
        self.source_row_lookup_fields
            .get(source)
            .map(String::as_str)
    }

    fn source_field_payload_alias(&self, source: &str, path: &str) -> Option<SourcePayloadField> {
        self.source_field_payload_aliases
            .get(&(source.to_owned(), path.to_owned()))
            .cloned()
    }

    fn state_value_type(&self, path: &str) -> Option<&PlanValueType> {
        self.state_value_types.get(path)
    }

    fn field_value_type(&self, field_id: FieldId) -> Option<&PlanValueType> {
        self.field_value_types.get(&field_id)
    }
}

fn insert_field_value_type(
    field_value_types: &mut BTreeMap<FieldId, PlanValueType>,
    field_id: FieldId,
    value_type: PlanValueType,
) {
    if !plan_value_type_is_concrete(value_type) {
        return;
    }
    field_value_types
        .entry(field_id)
        .and_modify(|existing| {
            if *existing != value_type {
                *existing = PlanValueType::Unknown;
            }
        })
        .or_insert(value_type);
}

fn insert_field_value_type_if_absent(
    field_value_types: &mut BTreeMap<FieldId, PlanValueType>,
    field_id: FieldId,
    value_type: PlanValueType,
) {
    if !plan_value_type_is_concrete(value_type) {
        return;
    }
    field_value_types.entry(field_id).or_insert(value_type);
}
