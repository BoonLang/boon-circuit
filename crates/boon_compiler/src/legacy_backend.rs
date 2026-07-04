use boon_ir::{
    self as ir, BytesScalarArg, DerivedValueKind, FileBytesPath, InitialValue,
    ListAppendFieldValue, ListInitializer, ListOperationKind, ListPredicate, ListProjectionKind,
    TypedProgram, UpdateExpression, UpdateGuard, UpdateValueExpression,
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

fn ir_scope_id(value: Option<ScopeId>) -> Option<ir::ScopeId> {
    value.map(|value| ir::ScopeId(value.0))
}

fn source_payload_schema_from_ir(value: &ir::SourcePayloadSchema) -> SourcePayloadSchema {
    SourcePayloadSchema {
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
        address_lookup_field: value
            .address_lookup_field
            .clone()
            .or_else(|| value.row_lookup_field.clone()),
    }
}

fn source_payload_descriptor_from_ir(
    value: &ir::SourcePayloadDescriptor,
) -> SourcePayloadDescriptor {
    SourcePayloadDescriptor {
        field: source_payload_field_from_ir(&value.field),
        value_type: source_payload_value_type_from_ir(value.value_type),
    }
}

fn source_payload_value_type_from_ir(value: ir::SourcePayloadValueType) -> SourcePayloadValueType {
    match value {
        ir::SourcePayloadValueType::Bytes => SourcePayloadValueType::Bytes,
        ir::SourcePayloadValueType::Text => SourcePayloadValueType::Text,
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

fn initial_value_kind_from_ir(value: &InitialValue) -> InitialValueKind {
    match value {
        InitialValue::Text { .. } => InitialValueKind::Text,
        InitialValue::Number { .. } => InitialValueKind::Number,
        InitialValue::Byte { .. } => InitialValueKind::Byte,
        InitialValue::Bool { .. } => InitialValueKind::Bool,
        InitialValue::Bytes { .. } => InitialValueKind::Bytes,
        InitialValue::Enum { .. } => InitialValueKind::Enum,
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

pub fn compile_typed_program(
    program: &TypedProgram,
    target_profile: TargetProfile,
) -> Result<MachinePlan, PlanError> {
    let row_initial_field_types = row_initial_field_value_types(program);
    let synthetic_initial_field_ids = synthetic_initial_list_field_ids(program);
    let index = ValueIndex::new(program, &row_initial_field_types);
    let mut next_op = 0usize;
    let mut unresolved_refs = BTreeSet::new();

    let source_routes = program
        .sources
        .iter()
        .enumerate()
        .map(|(route_id, source)| SourceRoute {
            id: PlanSourceRouteId(route_id),
            source_id: plan_source_id(source.id),
            path: source.path.clone(),
            scoped: source.scoped,
            scope_id: plan_scope_id(source.scope_id),
            payload_schema: source_payload_schema_from_ir(&source.payload_schema),
        })
        .collect::<Vec<_>>();

    let mut constants = Vec::new();
    let initial_constant_ids = program
        .state_cells
        .iter()
        .map(|state| {
            initial_constant_value(&state.initial_value)
                .map(|value| push_plan_constant(&mut constants, value))
        })
        .collect::<Vec<_>>();

    let scalar_slots = program
        .state_cells
        .iter()
        .enumerate()
        .map(|(slot_id, state)| ScalarStorageSlot {
            id: PlanStorageId(slot_id),
            state_id: plan_state_id(state.id),
            value_type: plan_value_type_from_initial_with_row_fields(
                &state.initial_value,
                plan_scope_id(state.scope_id),
                &row_initial_field_types,
            ),
            scope_id: plan_scope_id(state.scope_id),
            indexed: state.indexed,
            initial_value_kind: initial_value_kind_from_ir(&state.initial_value),
            initial_constant_id: initial_constant_ids[slot_id],
            initial_row_field_path: initial_row_field_path(&state.initial_value),
        })
        .collect::<Vec<_>>();

    let list_slot_offset = scalar_slots.len();
    let list_slots = program
        .lists
        .iter()
        .enumerate()
        .map(|(slot_index, list)| ListStorageSlot {
            id: PlanStorageId(list_slot_offset + slot_index),
            list_id: plan_list_id(list.id),
            scope_id: plan_scope_id(list.row_scope_id),
            row_field_ids: list_row_field_ids(program, list, &synthetic_initial_field_ids),
            capacity: list.capacity,
            hidden_key_type: list.hidden_key_type.clone(),
            has_generation: list.has_generation,
            initializer_kind: list_initializer_kind_from_ir(&list.initializer),
            range: plan_range_initializer(&list.initializer),
            initial_rows: plan_initial_list_rows(
                program,
                list,
                &list.initializer,
                &synthetic_initial_field_ids,
            ),
        })
        .collect::<Vec<_>>();
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
        .enumerate()
        .map(|(state_index, state)| {
            op(
                &mut next_op,
                PlanOpKind::StateInitialize {
                    initial_value_kind: initial_value_kind_from_ir(&state.initial_value),
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

    let update_ops = program
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
            op(
                &mut next_op,
                PlanOpKind::UpdateBranch {
                    expression_kind: update_expression_kind_for_branch(
                        &index,
                        &branch.source,
                        &branch.expression,
                    ),
                    ordered_inputs: ordered_update_expression_inputs(
                        &index,
                        &mut constants,
                        &branch.source,
                        &branch.target,
                        branch.indexed,
                        &branch.expression,
                    ),
                    source_payload_field: source_payload_field_for_expression(
                        &index,
                        &branch.source,
                        &branch.expression,
                    ),
                    update_constant_id: update_constant_id_for_expression(
                        &index,
                        &mut constants,
                        &branch.target,
                        &branch.expression,
                    ),
                    source_guard,
                },
                unique_value_refs(inputs),
                output,
                branch.indexed,
                unresolved,
            )
        })
        .collect::<Vec<_>>();

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
                                let value_ref = index.resolve(path);
                                if let Some(value_ref) = value_ref.clone() {
                                    inputs.push(value_ref.clone());
                                } else {
                                    unresolved += resolve_path(
                                        &index,
                                        path,
                                        &mut inputs,
                                        &mut unresolved_refs,
                                    );
                                }
                                append_fields.push(PlanListAppendField {
                                    name: field.name.clone(),
                                    field_id: row_field_id_for_list_field(
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
                                    field_id: row_field_id_for_list_field(
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
                                    field_id: row_field_id_for_list_field(
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

    let list_projection_ops = program
        .list_projections
        .iter()
        .map(|projection| {
            let mut inputs = Vec::new();
            let mut unresolved = 0usize;
            let source_list = match index.resolve(&projection.list) {
                Some(ValueRef::List(list_id)) => {
                    inputs.push(ValueRef::List(list_id));
                    Some(list_id)
                }
                Some(value_ref) => {
                    inputs.push(value_ref);
                    None
                }
                None => {
                    unresolved += 1;
                    unresolved_refs.insert(projection.list.clone());
                    None
                }
            };
            let output = index.resolve(&projection.target);
            if output.is_none() {
                unresolved += 1;
                unresolved_refs.insert(projection.target.clone());
            }
            let projection_plan = match (&projection.kind, source_list) {
                (ListProjectionKind::Find { field, value }, Some(source_list)) => {
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
                    ListProjectionKind::Chunk {
                        size: Some(size),
                        item_field,
                        label_field,
                    },
                    Some(source_list),
                ) => Some(PlanListProjection::Chunk {
                    source_list,
                    size: *size,
                    item_field: item_field.clone(),
                    label_field: label_field.clone(),
                }),
                (ListProjectionKind::Chunk { size: None, .. }, _) => {
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
    let typed_lowering_executable =
        unresolved_executable_ref_count == 0 && unknown_plan_op_count == 0;
    let cpu_plan_executor_unsupported_op_count =
        cpu_plan_executor_unsupported_op_count(&regions, &list_slots, &scalar_slots, &constants);
    let cpu_plan_executor_complete =
        typed_lowering_executable && cpu_plan_executor_unsupported_op_count == 0;

    Ok(MachinePlan {
        version: PlanVersion::default(),
        target_profile,
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
            update_branch_count: program.update_branches.len(),
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
            source_route_count: program.sources.len(),
            scalar_storage_count: program.state_cells.len(),
            list_storage_count: program.lists.len(),
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
                    |((list_name, field_name), field_id)| DebugEntry {
                        id: format!("field:{}", field_id.0),
                        label: format!("{list_name}.{field_name}"),
                    },
                ))
                .collect(),
            unresolved_executable_refs: unresolved_refs.into_iter().collect(),
        },
        regions,
    })
}

fn initial_constant_value(value: &InitialValue) -> Option<PlanConstantValue> {
    match value {
        InitialValue::Text { value } => Some(PlanConstantValue::Text {
            value: value.clone(),
        }),
        InitialValue::Number { value } => Some(PlanConstantValue::Number { value: *value }),
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
        InitialValue::RootInitialField { .. }
        | InitialValue::RowInitialField { .. }
        | InitialValue::Unknown { .. } => None,
    }
}

fn initial_row_field_path(value: &InitialValue) -> Option<String> {
    match value {
        InitialValue::RowInitialField { path } => Some(path.clone()),
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
        let Some(expr_id) = direct_statement_value_expr_id(&derived.statement) else {
            continue;
        };
        let Some(value_type) = inferred_expression_value_type(program, expr_id, &expr_value_types)
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
    )
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
    statement.expr.or_else(|| {
        statement
            .children
            .iter()
            .find_map(direct_statement_value_expr_id)
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
                        field_id: row_field_id_for_list_field(
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
                if list.row_scope_id.is_some() {
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
            ListInitializer::Empty | ListInitializer::Unknown { .. } => {}
        }
    }
    ids
}

fn append_constant_id(constants: &mut Vec<PlanConstant>, value: &str) -> PlanConstantId {
    push_plan_constant(constants, append_constant_value(value))
}

fn append_constant_value(value: &str) -> PlanConstantValue {
    match value {
        "True" => PlanConstantValue::Bool { value: true },
        "False" => PlanConstantValue::Bool { value: false },
        _ => value
            .parse::<i64>()
            .map(|value| PlanConstantValue::Number { value })
            .unwrap_or_else(|_| PlanConstantValue::Text {
                value: value.to_owned(),
            }),
    }
}

fn derived_expression_for_value(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    _unresolved_refs: &mut BTreeSet<String>,
) -> Option<PlanDerivedExpression> {
    source_key_text_trim_non_empty_expression(program, derived, index, inputs)
        .or_else(|| source_event_transform_expression(program, derived, index, constants, inputs))
        .or_else(|| bool_not_derived_expression(program, derived, index, inputs))
        .or_else(|| number_compare_const_derived_expression(program, derived, index, inputs))
        .or_else(|| root_bool_derived_expression(program, derived, index, inputs))
        .or_else(|| row_expression_for_value(program, derived, index, constants, inputs))
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
        let value = if let Some(value) = super::compiler_source_then_field_value(&exprs, source) {
            row_expression_from_compiler_field_value(&mut local_constants, &mut local_inputs, value)
        } else {
            source_event_transform_text_arm_expression(
                program,
                derived,
                index,
                &mut local_inputs,
                source,
            )?
        };
        if !local_inputs.contains(&ValueRef::Source(source_id)) {
            local_inputs.push(ValueRef::Source(source_id));
        }
        arm_values.push((source_id, value));
    }
    if arm_values.is_empty() {
        return None;
    }
    let default = source_event_transform_default_expression(
        program,
        derived,
        index,
        &mut local_constants,
        &mut local_inputs,
        &mut env,
        &expr_value_types,
    )
    .unwrap_or_else(|| {
        let value = if arm_values
            .iter()
            .all(|(_, value)| plan_row_expression_is_bool_constant(&local_constants, value))
        {
            PlanConstantValue::Bool { value: false }
        } else {
            PlanConstantValue::Text {
                value: String::new(),
            }
        };
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
    {
        input = source_payload_backing_row_state(
            program,
            index,
            source,
            *payload_source_id,
            field,
            derived.indexed,
        )?;
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
) -> Option<PlanRowExpression> {
    for child in &derived.statement.children {
        if source_event_transform_statement_mentions_source(program, child, &derived.sources) {
            continue;
        }
        if let Some(value) = lower_row_statement_value(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            child,
        )
        .and_then(lowered_scalar)
        {
            return Some(value);
        }
    }
    None
}

fn source_event_transform_statement_mentions_source(
    program: &TypedProgram,
    statement: &AstStatement,
    sources: &[String],
) -> bool {
    let exprs = super::compiler_statement_ast_exprs(statement, &program.expressions);
    exprs.iter().any(|expr| {
        let path = match &expr.kind {
            AstExprKind::Identifier(value) => value.clone(),
            AstExprKind::Path(parts) => parts.join("."),
            _ => return false,
        };
        sources.iter().any(|source| {
            source_event_ref_variants(source)
                .iter()
                .any(|variant| source_suffix_after_variant(&path, variant).is_some())
        })
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
    let left_path = expression_path_string(program, *left)?;
    let right_expr = expr_by_id(program, *right)?;
    let AstExprKind::Number(right_value) = &right_expr.kind else {
        return None;
    };
    let right = right_value.parse::<i64>().ok()?;
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
            let right_expr = expr_by_id(program, *right)?;
            let AstExprKind::Number(right_value) = &right_expr.kind else {
                return None;
            };
            let right = right_value.parse::<i64>().ok()?;
            let canonical_path = canonical_sibling_path(derived_path, &left_path);
            let left = index.resolve(&canonical_path)?;
            inputs.push(left.clone());
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
    if derived.kind != DerivedValueKind::Pure {
        return None;
    }
    if !derived.indexed && !row_statement_calls_router_route(program, &derived.statement) {
        return None;
    }
    let mut local_constants = constants.clone();
    let mut local_inputs = inputs.clone();
    let mut env = BTreeMap::new();
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

fn row_statement_calls_router_route(program: &TypedProgram, statement: &AstStatement) -> bool {
    super::compiler_statement_ast_exprs(statement, &program.expressions)
        .iter()
        .any(|expr| match &expr.kind {
            AstExprKind::Pipe { op, .. } => op == "Router/route",
            AstExprKind::Call { function, .. } => function == "Router/route",
            _ => false,
        })
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
    match &expr.kind {
        AstExprKind::Delimiter => env.get(ROW_PREVIOUS_BINDING).cloned(),
        AstExprKind::Identifier(name) => env.get(name).cloned().or_else(|| {
            row_field_expression(program, derived, index, inputs, name).map(LoweredRowValue::Scalar)
        }),
        AstExprKind::Path(parts) if parts.len() == 1 => {
            let name = parts.first()?;
            env.get(name).cloned().or_else(|| {
                row_field_expression(program, derived, index, inputs, name)
                    .map(LoweredRowValue::Scalar)
            })
        }
        AstExprKind::Path(parts) if parts.len() == 2 => {
            if let Some(value) = env.get(&parts[0]).cloned() {
                return match value {
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
                    LoweredRowValue::Scalar(object) => {
                        Some(LoweredRowValue::Scalar(PlanRowExpression::ObjectField {
                            object: Box::new(object),
                            field: parts[1].clone(),
                        }))
                    }
                };
            }
            let path = parts.join(".");
            row_field_expression(program, derived, index, inputs, &path)
                .map(LoweredRowValue::Scalar)
        }
        AstExprKind::Number(value) => {
            let value = value.parse::<i64>().ok()?;
            Some(LoweredRowValue::Scalar(row_constant_expression(
                constants,
                inputs,
                PlanConstantValue::Number { value },
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
            let left_value_type = lowered_row_value_type(index, &left).or_else(|| {
                inferred_expression_value_type(program, left_expr_id, expr_value_types)
            });
            let right_value_type = lowered_row_value_type(index, &right).or_else(|| {
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
            if op == "%" || op == "/" || op == "-" || op == "*" =>
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

fn lowered_scalar(value: LoweredRowValue) -> Option<PlanRowExpression> {
    match value {
        LoweredRowValue::Scalar(expression) => Some(expression),
        LoweredRowValue::ListRow { .. } | LoweredRowValue::ListFindRow { .. } => None,
    }
}

fn lowered_row_value_type(index: &ValueIndex, value: &LoweredRowValue) -> Option<PlanValueType> {
    match value {
        LoweredRowValue::Scalar(expression) => row_expression_value_type(index, expression),
        LoweredRowValue::ListRow { .. } | LoweredRowValue::ListFindRow { .. } => None,
    }
}

fn row_expression_value_type(
    index: &ValueIndex,
    expression: &PlanRowExpression,
) -> Option<PlanValueType> {
    match expression {
        PlanRowExpression::Field { input } => match input {
            ValueRef::Field(field_id) => index.field_value_type(*field_id).copied(),
            _ => None,
        },
        PlanRowExpression::Constant { .. } => None,
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
            _ => None,
        },
        PlanRowExpression::Select { arms, .. } => {
            let mut arm_types = arms
                .iter()
                .filter_map(|arm| row_expression_value_type(index, &arm.value));
            let first = arm_types.next()?;
            arm_types.all(|arm_type| arm_type == first).then_some(first)
        }
        PlanRowExpression::ListGetField { field, .. }
        | PlanRowExpression::ListFindValue { target: field, .. } => {
            index.field_value_type(*field).copied()
        }
        PlanRowExpression::ListRef { .. }
        | PlanRowExpression::ListRange { .. }
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
                value: i64::from(*value),
            },
        ));
    }
    if let AstExprKind::Infix { left, op, right } = &expr.kind {
        if matches!(op.as_str(), "+" | "-" | "*" | "/" | "%") {
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
        AstExprKind::Pipe { input, op, args: _ } if op == "WHILE" => *input,
        AstExprKind::When { input } => *input,
        _ => return None,
    };
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
        let arm_value = if let Some(output) = output {
            if row_expr_is_block_marker(program, *output) && !child.children.is_empty() {
                lower_row_function_body(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    child,
                    &mut arm_env,
                    expr_value_types,
                )?
            } else {
                lower_row_expr(
                    program,
                    derived,
                    index,
                    constants,
                    inputs,
                    &mut arm_env,
                    expr_value_types,
                    *output,
                )?
            }
        } else {
            lower_row_function_body(
                program,
                derived,
                index,
                constants,
                inputs,
                child,
                &mut arm_env,
                expr_value_types,
            )?
        };
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

fn row_expr_is_block_marker(program: &TypedProgram, expr_id: usize) -> bool {
    matches!(
        expr_by_id(program, expr_id).map(|expr| &expr.kind),
        Some(AstExprKind::Identifier(name)) if name == "BLOCK"
    )
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
            .parse::<i64>()
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
            .or_else(|| Some((PlanRowSelectPattern::Text { value: label }, None))),
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
    if !(first == '_' || first.is_ascii_alphabetic()) {
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
            | "Text/substring"
    )
}

fn row_list_builtin(function: &str) -> bool {
    matches!(
        function,
        "List/find" | "List/find_value" | "List/range" | "List/map" | "List/sum"
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
            let list_id = lower_row_list_ref(program, index, inputs, list_expr)?;
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
            let input_expr =
                piped_input.or_else(|| positional_arg(args, 0).map(|arg| arg.value))?;
            let binding_arg_index = if piped_input.is_some() { 0 } else { 1 };
            let binding = positional_arg(args, binding_arg_index)
                .and_then(|arg| row_raw_symbol(program, arg.value))?;
            let new_expr = named_arg(args, "new")?.value;
            let input = lower_row_list_expression(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                input_expr,
            )?;
            let mut map_env = env.clone();
            map_env.insert(
                binding.clone(),
                LoweredRowValue::Scalar(PlanRowExpression::ListMapItem {
                    binding: binding.clone(),
                }),
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
        "List/sum" => {
            let input_expr =
                piped_input.or_else(|| first_positional_arg(args).map(|arg| arg.value))?;
            let input = lower_row_list_expression(
                program,
                derived,
                index,
                constants,
                inputs,
                env,
                expr_value_types,
                input_expr,
            )?;
            Some(LoweredRowValue::Scalar(PlanRowExpression::ListSum {
                input: Box::new(input),
            }))
        }
        _ => None,
    }
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
    if let Some(list_id) = lower_row_list_ref(program, index, inputs, expr_id) {
        return Some(PlanRowExpression::ListRef { list_id });
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

fn lower_row_list_ref(
    program: &TypedProgram,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    expr_id: usize,
) -> Option<ListId> {
    let list_path = expression_path_string(program, expr_id)?;
    let ValueRef::List(list_id) = index.resolve(&list_path)? else {
        return None;
    };
    inputs.push(ValueRef::List(list_id));
    Some(list_id)
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
                .is_some_and(|name| names.iter().any(|candidate| *candidate == name))
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
    )
}

fn row_builtin_arg_expects_symbol(function: &str, arg_name: Option<&str>) -> bool {
    matches!(
        (function, arg_name),
        (_, Some("encoding"))
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
    })?;
    let input = lower_row_expr(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
        input_expr,
    )?;
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
    for child in &body.children {
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
    output
}

fn row_field_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    path: &str,
) -> Option<PlanRowExpression> {
    let canonical = canonical_sibling_path(&derived.path, path);
    let value_ref = index.resolve(&canonical).or_else(|| {
        synthetic_range_row_field_ref(program, plan_scope_id(derived.scope_id), path)
    })?;
    inputs.push(value_ref.clone());
    Some(PlanRowExpression::Field { input: value_ref })
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
            PlanConstantValue::Number { value }
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
        PlanValueType::Number => value
            .parse::<i64>()
            .ok()
            .map(|value| PlanConstantValue::Number { value }),
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
        PlanValueType::RootInitialField
        | PlanValueType::RowInitialField
        | PlanValueType::Unknown => match value {
            "True" => Some(PlanConstantValue::Bool { value: true }),
            "False" => Some(PlanConstantValue::Bool { value: false }),
            _ => value
                .parse::<i64>()
                .ok()
                .map(|value| PlanConstantValue::Number { value })
                .or_else(|| {
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
            resolve_update_path(index, source, target, indexed, left, refs, unresolved)
                + resolve_update_path(index, source, target, indexed, right, refs, unresolved)
        }
        UpdateExpression::MatchNumberInfixConst {
            left, right, arms, ..
        } => {
            let mut count =
                resolve_update_path(index, source, target, indexed, left, refs, unresolved)
                    + resolve_update_path(index, source, target, indexed, right, refs, unresolved);
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
            let mut count =
                resolve_update_path(index, source, target, indexed, list, refs, unresolved);
            count += collect_update_value_expression_refs(
                index, source, target, indexed, expected, refs, unresolved,
            );
            let _ = (field, value_target);
            if let Some(fallback) = fallback {
                count += collect_update_value_expression_refs(
                    index, source, target, indexed, fallback, refs, unresolved,
                );
            }
            count
        }
    }
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
            let index_constant_id =
                push_plan_constant(constants, PlanConstantValue::Number { value: index_value });
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
            let byte_count_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Number {
                    value: byte_count_value,
                },
            );
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
            let offset_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Number {
                    value: offset_value,
                },
            );
            let byte_count_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Number {
                    value: byte_count_value,
                },
            );
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
            let offset_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Number {
                    value: offset_value,
                },
            );
            let byte_count_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Number {
                    value: byte_count_value,
                },
            );
            let endian_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: endian.clone(),
                },
            );
            let value_constant_id =
                push_plan_constant(constants, PlanConstantValue::Number { value: *value });
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
        UpdateExpression::MatchTextIsEmptyConst { input, arms } => {
            let Some(input_ref) = resolve_update_value_ref(index, source, target, indexed, input)
            else {
                return Vec::new();
            };
            let mut refs = vec![input_ref];
            for pattern in ["True", "False", "__"] {
                let Some(arm) = arms.iter().find(|arm| arm.pattern == pattern) else {
                    continue;
                };
                let Some(output_ref) = update_value_expression_value_ref(
                    index,
                    constants,
                    source,
                    target,
                    indexed,
                    &arm.output,
                ) else {
                    continue;
                };
                refs.push(output_ref);
            }
            refs
        }
        _ => Vec::new(),
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
            let constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: value.clone(),
                },
            );
            Some(ValueRef::Constant(constant_id))
        }
        UpdateValueExpression::ReadPath { path } => {
            resolve_update_value_ref(index, source, target, indexed, path)
        }
        UpdateValueExpression::MatchConst { .. }
        | UpdateValueExpression::NumberInfix { .. }
        | UpdateValueExpression::MatchNumberInfixConst { .. } => None,
    }
}

fn resolve_update_value_ref(
    index: &ValueIndex,
    source: &str,
    target: &str,
    indexed: bool,
    path: &str,
) -> Option<ValueRef> {
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
    let Some(guard) = guard else {
        return None;
    };
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
        UpdateValueExpression::MatchConst { input, .. } => {
            resolve_update_path(index, source, target, indexed, input, refs, unresolved)
        }
        UpdateValueExpression::NumberInfix { left, right, .. }
        | UpdateValueExpression::MatchNumberInfixConst { left, right, .. } => {
            resolve_update_path(index, source, target, indexed, left, refs, unresolved)
                + resolve_update_path(index, source, target, indexed, right, refs, unresolved)
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
        UpdateExpression::PrefixPayloadConcat { payload_path, .. } => {
            source_payload_field_from_path(source, payload_path, true)
        }
        UpdateExpression::TextTrimOrPrevious { path, .. } => {
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
        return Some(source_payload_field_from_suffix(path)?);
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
                PlanConstantValue::Number { value },
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
    expression: &UpdateExpression,
) -> PlanExpressionKind {
    if matches!(expression, UpdateExpression::ReadPath { .. })
        && source_payload_field_for_expression(index, source, expression).is_some()
    {
        return PlanExpressionKind::SourcePayload;
    }
    update_expression_kind(expression)
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
        UpdateExpression::BytesLength { .. } => PlanExpressionKind::BytesLength,
        UpdateExpression::BytesIsEmpty { .. } => PlanExpressionKind::BytesIsEmpty,
        UpdateExpression::BytesGet { .. } => PlanExpressionKind::BytesGet,
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
        UpdateExpression::MatchNumberInfixConst { .. } => PlanExpressionKind::MatchNumberInfixConst,
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
            output: ValueRef::Field(plan_field_id(derived.id)),
        });
    }
    routes
}

fn derived_output_ref(program: &TypedProgram, derived: &boon_ir::DerivedValue) -> ValueRef {
    if derived.indexed {
        if let Some(field) = program
            .semantic_index
            .fields
            .iter()
            .find(|field| field.path == derived.path)
        {
            return ValueRef::Field(plan_field_id(field.id));
        }
    }
    ValueRef::Field(plan_field_id(derived.id))
}

struct ValueIndex {
    by_path: BTreeMap<String, ValueRef>,
    source_payload_fields: BTreeMap<String, BTreeSet<SourcePayloadField>>,
    state_value_types: BTreeMap<String, PlanValueType>,
    field_value_types: BTreeMap<FieldId, PlanValueType>,
}

impl ValueIndex {
    fn new(program: &TypedProgram, row_field_types: &RowInitialFieldTypeMap) -> Self {
        let mut by_path = BTreeMap::new();
        let mut source_payload_fields = BTreeMap::new();
        let mut state_value_types = BTreeMap::new();
        let mut field_value_types = BTreeMap::new();
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
        }
        for state in &program.state_cells {
            by_path.insert(state.path.clone(), ValueRef::State(plan_state_id(state.id)));
            state_value_types.insert(
                state.path.clone(),
                plan_value_type_from_initial_with_row_fields(
                    &state.initial_value,
                    plan_scope_id(state.scope_id),
                    row_field_types,
                ),
            );
        }
        for list in &program.lists {
            by_path.insert(list.name.clone(), ValueRef::List(plan_list_id(list.id)));
            if let ListInitializer::RecordLiteral { rows } = &list.initializer {
                for row in rows {
                    for field in &row.fields {
                        if let Some(field_id) = row_field_id_for_list_field(
                            program,
                            &list.name,
                            &field.name,
                            &synthetic_field_ids,
                        ) {
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
            if let ValueRef::Field(field_id) = &output_ref {
                if let Some(expr_id) = direct_statement_value_expr_id(&derived.statement) {
                    let expr_value_types = expression_value_type_lookup(program);
                    if let Some(value_type) =
                        inferred_expression_value_type(program, expr_id, &expr_value_types)
                    {
                        insert_field_value_type_if_absent(
                            &mut field_value_types,
                            *field_id,
                            value_type,
                        );
                    }
                }
            }
            by_path.insert(derived.path.clone(), output_ref);
        }
        for field in &program.semantic_index.fields {
            by_path
                .entry(field.path.clone())
                .or_insert(ValueRef::Field(plan_field_id(field.id)));
        }
        Self {
            by_path,
            source_payload_fields,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_cells_project_for_plan_test() -> boon_parser::ParsedProgram {
        boon_parser::parse_project(
            "examples/cells.bn",
            [
                (
                    "examples/cells/defaults.bn".to_owned(),
                    include_str!("../../../examples/cells/defaults.bn").to_owned(),
                ),
                (
                    "examples/cells/formula.bn".to_owned(),
                    include_str!("../../../examples/cells/formula.bn").to_owned(),
                ),
                (
                    "examples/cells/cell.bn".to_owned(),
                    include_str!("../../../examples/cells/cell.bn").to_owned(),
                ),
                (
                    "examples/cells/model.bn".to_owned(),
                    include_str!("../../../examples/cells/model.bn").to_owned(),
                ),
                (
                    "examples/cells/columns.bn".to_owned(),
                    include_str!("../../../examples/cells/columns.bn").to_owned(),
                ),
                (
                    "examples/cells/store.bn".to_owned(),
                    include_str!("../../../examples/cells/store.bn").to_owned(),
                ),
                (
                    "examples/cells/view.bn".to_owned(),
                    include_str!("../../../examples/cells/view.bn").to_owned(),
                ),
                (
                    "examples/cells.bn".to_owned(),
                    include_str!("../../../examples/cells.bn").to_owned(),
                ),
            ],
        )
        .expect("checked-in Cells project should parse")
    }

    #[test]
    fn plan_hash_is_stable_for_same_plan() {
        let plan = MachinePlan {
            version: PlanVersion::default(),
            target_profile: TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: StorageLayout {
                scalar_slots: Vec::new(),
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: Vec::new(),
            dirty_plan: DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: DeltaPlan { deltas: Vec::new() },
            capability_summary: CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 0,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: Vec::new(),
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        assert_eq!(plan_sha256(&plan).unwrap(), plan_sha256(&plan).unwrap());
        assert_eq!(verify_plan(&plan).unwrap().status, "pass");
    }

    #[test]
    fn bytes_literal_lowers_to_executable_typed_storage_and_constant_payload() {
        let parsed = boon_parser::parse_source(
            "bytes-plan-literal.bn",
            r#"
source: SOURCE
payload:
    BYTES[4] { 16u01, 16u02, 16u03, 16u04 } |> HOLD payload { LATEST {} }
"#,
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.executable,
            "minimal BYTES[4] plan should be executable: {:#?}",
            plan.capability_summary
        );
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);
        assert_eq!(plan.capability_summary.unresolved_executable_ref_count, 0);

        let slot = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| matches!(slot.initial_value_kind, InitialValueKind::Bytes))
            .expect("BYTES scalar slot should lower");
        assert_eq!(slot.value_type, PlanValueType::Bytes { fixed_len: Some(4) });
        let constant_id = slot
            .initial_constant_id
            .expect("BYTES scalar slot should reference a typed constant");
        let constant = plan
            .constants
            .iter()
            .find(|constant| constant.id == constant_id)
            .expect("referenced BYTES constant should exist");
        let PlanConstantValue::Bytes {
            byte_len,
            sha256,
            inline_bytes,
        } = &constant.value
        else {
            panic!("BYTES scalar should reference a BYTES constant");
        };
        assert_eq!(*byte_len, 4);
        assert_eq!(
            sha256,
            "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a"
        );
        assert_eq!(inline_bytes.as_deref(), Some(&[1, 2, 3, 4][..]));
    }

    #[test]
    fn repeated_plan_constants_are_interned_by_value() {
        let parsed = boon_parser::parse_source(
            "bytes-plan-constant-interning.bn",
            r#"
source: SOURCE
payload_a:
    BYTES[2] { 16u01, 16u02 } |> HOLD payload_a { LATEST {} }
payload_b:
    BYTES[2] { 16u01, 16u02 } |> HOLD payload_b { LATEST {} }
measure_a:
    16u00 |> HOLD first_a {
        LATEST {
            source |> THEN { payload_a |> Bytes/get(index: 0) }
        }
    }
measure_b:
    16u00 |> HOLD first_b {
        LATEST {
            source |> THEN { payload_b |> Bytes/get(index: 0) }
        }
    }
"#,
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");

        let payload_a_id = StateId(debug_entry_id(
            &plan.debug_map.state_slots,
            "state",
            "payload_a",
        ));
        let payload_b_id = StateId(debug_entry_id(
            &plan.debug_map.state_slots,
            "state",
            "payload_b",
        ));
        let payload_a_constant_id = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == payload_a_id)
            .and_then(|slot| slot.initial_constant_id)
            .expect("payload_a should have an initial constant");
        let payload_b_constant_id = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == payload_b_id)
            .and_then(|slot| slot.initial_constant_id)
            .expect("payload_b should have an initial constant");
        assert_eq!(
            payload_a_constant_id, payload_b_constant_id,
            "identical BYTES initial values should share one PlanConstantId"
        );

        let bytes_constants = plan
            .constants
            .iter()
            .filter(|constant| {
                matches!(
                    &constant.value,
                    PlanConstantValue::Bytes {
                        byte_len: 2,
                        inline_bytes: Some(bytes),
                        ..
                    } if bytes == &[1, 2]
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            bytes_constants.len(),
            1,
            "duplicate BYTES constants should not be repeated in the MachinePlan constant pool"
        );

        let zero_constant_ids = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .filter_map(|op| match &op.kind {
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesGet,
                    update_constant_id: Some(constant_id),
                    ..
                } if plan_constant_by_id(&plan.constants, *constant_id).is_some_and(
                    |constant| matches!(constant.value, PlanConstantValue::Number { value: 0 }),
                ) =>
                {
                    Some(*constant_id)
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            zero_constant_ids.len(),
            1,
            "repeated Bytes/get(index: 0) operands should reuse the same number constant"
        );

        let mut tampered = plan.clone();
        let duplicate = tampered.constants[bytes_constants[0].id.0].value.clone();
        let duplicate_id = PlanConstantId(tampered.constants.len());
        tampered.constants.push(PlanConstant {
            id: duplicate_id,
            value: duplicate,
        });
        let tampered_verification = verify_plan(&tampered).unwrap();
        assert_eq!(tampered_verification.status, "fail");
        assert!(
            tampered_verification
                .checks
                .iter()
                .any(|check| check.id == "plan-constants-deduplicated" && !check.pass),
            "duplicate constants must fail the MachinePlan verifier: {tampered_verification:#?}"
        );
    }

    #[test]
    fn bytes_length_update_lowers_to_typed_executable_plan_op() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_length_plan_ops.bn",
            include_str!("../../../examples/bytes_length_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Bytes/length root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
        let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
        let byte_len_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.byte_len");
        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == byte_len_state_id)
            })
            .expect("store.measure should lower to a byte_len update branch");
        assert_eq!(op.unresolved_executable_ref_count, 0);
        assert!(
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::State(id) if id.0 == payload_state_id)),
            "Bytes/length input must be a typed state ref to top-level payload, not a string path: {op:#?}"
        );
        assert!(matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesLength,
                source_payload_field: None,
                update_constant_id: None,
                ..
            }
        ));

        let mut tampered = plan.clone();
        let tampered_op = tampered
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesLength,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/length update op");
        tampered_op.inputs = vec![
            ValueRef::Source(SourceId(source_id)),
            ValueRef::State(StateId(byte_len_state_id)),
        ];
        let tampered_verification = verify_plan(&tampered).unwrap();
        assert_eq!(tampered_verification.status, "fail");
        assert!(
            tampered_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/length with a non-BYTES input must not satisfy CPU executor support: {tampered_verification:#?}"
        );
    }

    #[test]
    fn bytes_is_empty_update_lowers_to_typed_executable_plan_op() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_is_empty_plan_ops.bn",
            include_str!("../../../examples/bytes_is_empty_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Bytes/is_empty root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
        let empty_payload_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "empty_payload");
        let filled_payload_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "filled_payload");
        let empty_target_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.empty_is_empty");
        let filled_target_state_id = debug_entry_id(
            &plan.debug_map.state_slots,
            "state",
            "store.filled_is_empty",
        );

        for (target_state_id, payload_state_id, target_label) in [
            (
                empty_target_state_id,
                empty_payload_state_id,
                "store.empty_is_empty",
            ),
            (
                filled_target_state_id,
                filled_payload_state_id,
                "store.filled_is_empty",
            ),
        ] {
            let op = plan
                .regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                        && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| {
                    panic!("store.measure should lower to a bytes_is_empty update for {target_label}")
                });
            assert_eq!(op.unresolved_executable_ref_count, 0);
            assert!(
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::State(id) if id.0 == payload_state_id)),
                "Bytes/is_empty input must be a typed BYTES state ref, not a string path: {op:#?}"
            );
            assert!(matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesIsEmpty,
                    source_payload_field: None,
                    update_constant_id: None,
                    ..
                }
            ));
        }

        let mut tampered_input = plan.clone();
        let tampered_op = tampered_input
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesIsEmpty,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/is_empty update op");
        tampered_op.inputs = vec![
            ValueRef::Source(SourceId(source_id)),
            ValueRef::State(StateId(empty_target_state_id)),
        ];
        let tampered_input_verification = verify_plan(&tampered_input).unwrap();
        assert_eq!(tampered_input_verification.status, "fail");
        assert!(
            tampered_input_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/is_empty with a non-BYTES input must not satisfy CPU executor support: {tampered_input_verification:#?}"
        );

        let mut tampered_output = plan.clone();
        let tampered_op = tampered_output
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesIsEmpty,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/is_empty update op");
        tampered_op.output = Some(ValueRef::State(StateId(empty_payload_state_id)));
        let tampered_output_verification = verify_plan(&tampered_output).unwrap();
        assert_eq!(tampered_output_verification.status, "fail");
        assert!(
            tampered_output_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/is_empty with a non-BOOL output must not satisfy CPU executor support: {tampered_output_verification:#?}"
        );
    }

    #[test]
    fn bytes_get_update_lowers_to_typed_executable_plan_op() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_get_plan_ops.bn",
            include_str!("../../../examples/bytes_get_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Bytes/get root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
        let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
        let target_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.selected_byte");
        let target_slot = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id.0 == target_state_id)
            .expect("selected_byte storage slot should lower");
        assert_eq!(target_slot.value_type, PlanValueType::Byte);

        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
            })
            .expect("store.measure should lower to a bytes_get update branch");
        assert_eq!(op.unresolved_executable_ref_count, 0);
        assert!(
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::State(id) if id.0 == payload_state_id)),
            "Bytes/get input must be a typed BYTES state ref, not a string path: {op:#?}"
        );
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesGet,
            source_payload_field: None,
            update_constant_id: Some(index_constant_id),
            ..
        } = &op.kind
        else {
            panic!("Bytes/get op should carry a typed index constant: {op:#?}");
        };
        assert_eq!(
            plan.constants
                .iter()
                .find(|constant| constant.id == *index_constant_id)
                .map(|constant| &constant.value),
            Some(&PlanConstantValue::Number { value: 2 })
        );

        let mut tampered_input = plan.clone();
        let tampered_op = tampered_input
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesGet,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/get update op");
        tampered_op.inputs = vec![
            ValueRef::Source(SourceId(source_id)),
            ValueRef::State(StateId(target_state_id)),
        ];
        let tampered_input_verification = verify_plan(&tampered_input).unwrap();
        assert_eq!(tampered_input_verification.status, "fail");
        assert!(
            tampered_input_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/get with a non-BYTES input must not satisfy CPU executor support: {tampered_input_verification:#?}"
        );

        let mut tampered_output = plan.clone();
        let tampered_op = tampered_output
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesGet,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/get update op");
        tampered_op.output = Some(ValueRef::State(StateId(payload_state_id)));
        let tampered_output_verification = verify_plan(&tampered_output).unwrap();
        assert_eq!(tampered_output_verification.status, "fail");
        assert!(
            tampered_output_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/get with a non-BYTE output must not satisfy CPU executor support: {tampered_output_verification:#?}"
        );

        let mut tampered_index = plan.clone();
        let tampered_op = tampered_index
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesGet,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/get update op");
        let PlanOpKind::UpdateBranch {
            update_constant_id, ..
        } = &mut tampered_op.kind
        else {
            unreachable!()
        };
        *update_constant_id = None;
        let tampered_index_verification = verify_plan(&tampered_index).unwrap();
        assert_eq!(tampered_index_verification.status, "fail");
        assert!(
            tampered_index_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_index_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/get without a typed index constant must fail verifier support: {tampered_index_verification:#?}"
        );
    }

    #[test]
    fn bytes_set_update_lowers_to_typed_executable_plan_op() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_set_plan_ops.bn",
            include_str!("../../../examples/bytes_set_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Bytes/set root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.patch");
        let input_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "left_payload");
        let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.patched");
        let target_slot = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id.0 == target_state_id)
            .expect("patched storage slot should lower");
        assert_eq!(
            target_slot.value_type,
            PlanValueType::Bytes { fixed_len: Some(4) }
        );

        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
            })
            .expect("store.patch should lower to a bytes_set update branch");
        assert_eq!(op.unresolved_executable_ref_count, 0);
        assert!(
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::State(id) if id.0 == input_state_id)),
            "Bytes/set input must be a typed BYTES state ref, not a string path: {op:#?}"
        );
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesSet,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } = &op.kind
        else {
            panic!("Bytes/set op should carry typed ordered operands: {op:#?}");
        };
        let [
            ValueRef::State(ordered_input),
            ValueRef::Constant(index_constant_id),
            ValueRef::Constant(value_constant_id),
        ] = ordered_inputs.as_slice()
        else {
            panic!("Bytes/set ordered operands should be state/index/value: {ordered_inputs:#?}");
        };
        assert_eq!(ordered_input.0, input_state_id);
        assert_eq!(
            plan.constants
                .iter()
                .find(|constant| constant.id == *index_constant_id)
                .map(|constant| &constant.value),
            Some(&PlanConstantValue::Number { value: 2 })
        );
        assert_eq!(
            plan.constants
                .iter()
                .find(|constant| constant.id == *value_constant_id)
                .map(|constant| &constant.value),
            Some(&PlanConstantValue::Byte { value: 0xaa })
        );

        let mut tampered_missing_value = plan.clone();
        let tampered_op = tampered_missing_value
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesSet,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/set update op");
        let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &mut tampered_op.kind else {
            unreachable!()
        };
        ordered_inputs.pop();
        let tampered_missing_value_verification = verify_plan(&tampered_missing_value).unwrap();
        assert_eq!(tampered_missing_value_verification.status, "fail");

        let mut tampered_value_type = plan.clone();
        let value_constant = tampered_value_type
            .constants
            .iter_mut()
            .find(|constant| constant.id == *value_constant_id)
            .expect("value constant should exist");
        value_constant.value = PlanConstantValue::Number { value: 170 };
        let tampered_value_type_verification = verify_plan(&tampered_value_type).unwrap();
        assert_eq!(tampered_value_type_verification.status, "fail");

        let mut tampered_oob_index = plan.clone();
        let index_constant = tampered_oob_index
            .constants
            .iter_mut()
            .find(|constant| constant.id == *index_constant_id)
            .expect("index constant should exist");
        index_constant.value = PlanConstantValue::Number { value: 4 };
        let tampered_oob_index_verification = verify_plan(&tampered_oob_index).unwrap();
        assert_eq!(tampered_oob_index_verification.status, "fail");
    }

    #[test]
    fn text_bytes_conversion_updates_lower_to_typed_executable_plan_ops() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_text_conversion_plan_ops.bn",
            include_str!("../../../examples/bytes_text_conversion_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "text/BYTES conversion fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let encode_source_id =
            debug_entry_id(&plan.debug_map.source_routes, "source", "store.encode");
        let decode_source_id =
            debug_entry_id(&plan.debug_map.source_routes, "source", "store.decode");
        let text_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "text_payload");
        let encoded_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.encoded");
        let decoded_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.decoded");

        let encode_op =
            plan.regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs.iter().any(
                        |input| matches!(input, ValueRef::Source(id) if id.0 == encode_source_id),
                    ) && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == encoded_state_id)
                })
                .expect("store.encode should lower to a Text/to_bytes update branch");
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::TextToBytes,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } = &encode_op.kind
        else {
            panic!("Text/to_bytes op should carry typed ordered operands: {encode_op:#?}");
        };
        let [
            ValueRef::State(ordered_text),
            ValueRef::Constant(encode_encoding_id),
        ] = ordered_inputs.as_slice()
        else {
            panic!("Text/to_bytes ordered operands should be state/encoding: {ordered_inputs:#?}");
        };
        assert_eq!(ordered_text.0, text_state_id);
        assert_eq!(
            plan.constants
                .iter()
                .find(|constant| constant.id == *encode_encoding_id)
                .map(|constant| &constant.value),
            Some(&PlanConstantValue::Text {
                value: "Utf8".to_owned()
            })
        );

        let decode_op =
            plan.regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs.iter().any(
                        |input| matches!(input, ValueRef::Source(id) if id.0 == decode_source_id),
                    ) && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == decoded_state_id)
                })
                .expect("store.decode should lower to a Bytes/to_text update branch");
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesToText,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } = &decode_op.kind
        else {
            panic!("Bytes/to_text op should carry typed ordered operands: {decode_op:#?}");
        };
        let [
            ValueRef::State(ordered_bytes),
            ValueRef::Constant(decode_encoding_id),
        ] = ordered_inputs.as_slice()
        else {
            panic!("Bytes/to_text ordered operands should be state/encoding: {ordered_inputs:#?}");
        };
        assert_eq!(ordered_bytes.0, encoded_state_id);
        assert_eq!(
            plan.constants
                .iter()
                .find(|constant| constant.id == *decode_encoding_id)
                .map(|constant| &constant.value),
            Some(&PlanConstantValue::Text {
                value: "Utf8".to_owned()
            })
        );

        let mut tampered_encoding = plan.clone();
        let constant = tampered_encoding
            .constants
            .iter_mut()
            .find(|constant| constant.id == *decode_encoding_id)
            .expect("decode encoding constant should exist");
        constant.value = PlanConstantValue::Text {
            value: "Utf16".to_owned(),
        };
        let tampered_verification = verify_plan(&tampered_encoding).unwrap();
        assert_eq!(tampered_verification.status, "fail");
        assert!(
            tampered_verification.checks.iter().any(|check| check.id
                == "constant-refs-resolve-and-match-storage-types"
                && !check.pass)
                || tampered_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "unsupported conversion constants must fail verification: {tampered_verification:#?}"
        );
    }

    #[test]
    fn ascii_text_bytes_conversion_lowers_to_typed_executable_plan_ops() {
        let source = r#"
text_payload:
    TEXT { A1+2 } |> HOLD text_payload { LATEST {} }

store: [
    encode: SOURCE
    decode: SOURCE
    encoded:
        BYTES {} |> HOLD encoded {
            LATEST {
                store.encode |> THEN { text_payload |> Text/to_bytes(encoding: Ascii) }
            }
        }
    decoded:
        TEXT {} |> HOLD decoded {
            LATEST {
                store.decode |> THEN { store.encoded |> Bytes/to_text(encoding: Ascii) }
            }
        }
]

document: Document/new(root: Element/label(element: [], label: store.decoded))
"#;
        let parsed = boon_parser::parse_source("ascii-conversion.bn", source).unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let ascii_constant_ids = plan
            .constants
            .iter()
            .filter_map(|constant| {
                matches!(
                    &constant.value,
                    PlanConstantValue::Text { value } if value == "Ascii"
                )
                .then_some(constant.id)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ascii_constant_ids.len(),
            1,
            "Text/to_bytes and Bytes/to_text should share one interned Ascii constant"
        );
        let conversion_encoding_ids = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .filter_map(|op| match &op.kind {
                PlanOpKind::UpdateBranch {
                    expression_kind:
                        PlanExpressionKind::TextToBytes | PlanExpressionKind::BytesToText,
                    ordered_inputs,
                    ..
                } => match ordered_inputs.last() {
                    Some(ValueRef::Constant(constant_id)) => Some(*constant_id),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            conversion_encoding_ids.len(),
            2,
            "fixture should contain one Text/to_bytes and one Bytes/to_text encoding operand"
        );
        assert!(
            conversion_encoding_ids
                .iter()
                .all(|constant_id| *constant_id == ascii_constant_ids[0]),
            "both conversion ops should reference the interned Ascii constant: {conversion_encoding_ids:?}"
        );
    }

    #[test]
    fn bytes_equal_update_lowers_to_typed_executable_plan_op() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_equal_plan_ops.bn",
            include_str!("../../../examples/bytes_equal_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Bytes/equal root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
        let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
        let same_payload_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "same_payload");
        let different_payload_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "different_payload");
        let same_target_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.same");
        let different_target_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.different");

        for (target_state_id, right_state_id, target_label) in [
            (same_target_state_id, same_payload_state_id, "store.same"),
            (
                different_target_state_id,
                different_payload_state_id,
                "store.different",
            ),
        ] {
            let target_slot = plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.state_id.0 == target_state_id)
                .unwrap_or_else(|| panic!("{target_label} storage slot should lower"));
            assert_eq!(target_slot.value_type, PlanValueType::Bool);

            let op = plan
                .regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                        && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| {
                    panic!("store.measure should lower to a bytes_equal update for {target_label}")
                });
            assert_eq!(op.unresolved_executable_ref_count, 0);
            assert!(
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::State(id) if id.0 == payload_state_id)),
                "Bytes/equal left input must be a typed BYTES state ref: {op:#?}"
            );
            assert!(
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::State(id) if id.0 == right_state_id)),
                "Bytes/equal right input must be a typed BYTES state ref: {op:#?}"
            );
            assert!(matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesEqual,
                    source_payload_field: None,
                    update_constant_id: None,
                    ..
                }
            ));
        }

        let mut tampered_input = plan.clone();
        let tampered_op = tampered_input
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesEqual,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/equal update op");
        tampered_op.inputs = vec![
            ValueRef::Source(SourceId(source_id)),
            ValueRef::State(StateId(same_target_state_id)),
            ValueRef::State(StateId(same_payload_state_id)),
        ];
        let tampered_input_verification = verify_plan(&tampered_input).unwrap();
        assert_eq!(tampered_input_verification.status, "fail");
        assert!(
            tampered_input_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/equal with a non-BYTES input must not satisfy CPU executor support: {tampered_input_verification:#?}"
        );

        let mut tampered_output = plan.clone();
        let tampered_op = tampered_output
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesEqual,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/equal update op");
        tampered_op.output = Some(ValueRef::State(StateId(payload_state_id)));
        let tampered_output_verification = verify_plan(&tampered_output).unwrap();
        assert_eq!(tampered_output_verification.status, "fail");
        assert!(
            tampered_output_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_output_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/equal with a non-BOOL output must fail verifier support: {tampered_output_verification:#?}"
        );

        let mut tampered_constant = plan.clone();
        let tampered_op = tampered_constant
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesEqual,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/equal update op");
        let PlanOpKind::UpdateBranch {
            update_constant_id, ..
        } = &mut tampered_op.kind
        else {
            unreachable!()
        };
        *update_constant_id = Some(PlanConstantId(0));
        let tampered_constant_verification = verify_plan(&tampered_constant).unwrap();
        assert_eq!(tampered_constant_verification.status, "fail");
        assert!(
            tampered_constant_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_constant_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/equal with an update constant must fail verifier support: {tampered_constant_verification:#?}"
        );
    }

    #[test]
    fn bytes_search_updates_lower_to_ordered_typed_executable_plan_ops() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_search_plan_ops.bn",
            include_str!("../../../examples/bytes_search_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Bytes/search fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
        let joined_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.joined");
        let found_needle_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "found_needle");
        let missing_needle_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "missing_needle");
        let empty_needle_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "empty_needle");
        let prefix_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "prefix");
        let wrong_prefix_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "wrong_prefix");
        let suffix_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "suffix");
        let wrong_suffix_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "wrong_suffix");

        let expected = [
            (
                "store.found_index",
                PlanExpressionKind::BytesFind,
                found_needle_state_id,
                PlanValueType::Number,
            ),
            (
                "store.missing_index",
                PlanExpressionKind::BytesFind,
                missing_needle_state_id,
                PlanValueType::Number,
            ),
            (
                "store.empty_index",
                PlanExpressionKind::BytesFind,
                empty_needle_state_id,
                PlanValueType::Number,
            ),
            (
                "store.starts",
                PlanExpressionKind::BytesStartsWith,
                prefix_state_id,
                PlanValueType::Bool,
            ),
            (
                "store.not_starts",
                PlanExpressionKind::BytesStartsWith,
                wrong_prefix_state_id,
                PlanValueType::Bool,
            ),
            (
                "store.ends",
                PlanExpressionKind::BytesEndsWith,
                suffix_state_id,
                PlanValueType::Bool,
            ),
            (
                "store.not_ends",
                PlanExpressionKind::BytesEndsWith,
                wrong_suffix_state_id,
                PlanValueType::Bool,
            ),
        ];

        for (target_label, expression_kind, second_state_id, output_type) in expected {
            let target_state_id =
                debug_entry_id(&plan.debug_map.state_slots, "state", target_label);
            let target_slot = plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.state_id.0 == target_state_id)
                .unwrap_or_else(|| panic!("{target_label} storage slot should lower"));
            assert_eq!(target_slot.value_type, output_type);

            let op = plan
                .regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                        && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| {
                    panic!("store.measure should lower to bytes search update for {target_label}")
                });
            assert_eq!(op.unresolved_executable_ref_count, 0);
            assert!(matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: actual_kind,
                    source_payload_field: None,
                    update_constant_id: None,
                    ordered_inputs,
                    ..
                } if *actual_kind == expression_kind
                    && ordered_inputs == &vec![
                        ValueRef::State(StateId(joined_state_id)),
                        ValueRef::State(StateId(second_state_id)),
                    ]
            ));
        }

        let mut tampered_missing_order = plan.clone();
        let tampered_op = tampered_missing_order
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesFind,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/find update op");
        let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &mut tampered_op.kind else {
            unreachable!()
        };
        ordered_inputs.clear();
        let tampered_missing_order_verification = verify_plan(&tampered_missing_order).unwrap();
        assert_eq!(tampered_missing_order_verification.status, "fail");
        assert!(
            tampered_missing_order_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_missing_order_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/find without ordered inputs must fail verifier support: {tampered_missing_order_verification:#?}"
        );

        let mut tampered_output = plan.clone();
        let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "left_payload");
        let tampered_op = tampered_output
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesStartsWith,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/starts_with update op");
        tampered_op.output = Some(ValueRef::State(StateId(payload_state_id)));
        let tampered_output_verification = verify_plan(&tampered_output).unwrap();
        assert_eq!(tampered_output_verification.status, "fail");
        assert!(
            tampered_output_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_output_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/starts_with with non-BOOL output must fail verifier support: {tampered_output_verification:#?}"
        );

        let mut tampered_constant = plan.clone();
        let tampered_op = tampered_constant
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesEndsWith,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/ends_with update op");
        let PlanOpKind::UpdateBranch {
            update_constant_id, ..
        } = &mut tampered_op.kind
        else {
            unreachable!()
        };
        *update_constant_id = Some(PlanConstantId(0));
        let tampered_constant_verification = verify_plan(&tampered_constant).unwrap();
        assert_eq!(tampered_constant_verification.status, "fail");
        assert!(
            tampered_constant_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_constant_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/ends_with with an update constant must fail verifier support: {tampered_constant_verification:#?}"
        );
    }

    #[test]
    fn bytes_encoding_updates_lower_to_ordered_typed_executable_plan_ops() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_encoding_plan_ops.bn",
            include_str!("../../../examples/bytes_encoding_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(plan.capability_summary.cpu_plan_executor_complete);
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.decode");
        let zeros_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.zeros");
        let hex_input_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "hex_input");
        let base64_input_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "base64_input");
        let decoded_hex_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.decoded_hex");
        let decoded_base64_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.decoded_base64");

        let op_for = |target_state_id: usize| {
            plan.regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                        && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| panic!("missing decode op for state {target_state_id}"))
        };

        assert!(matches!(
            &op_for(zeros_state_id).kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesZeros,
                source_payload_field: None,
                update_constant_id: None,
                ordered_inputs,
                ..
            } if matches!(ordered_inputs.as_slice(), [ValueRef::Constant(_)])
        ));
        assert!(matches!(
            &op_for(decoded_hex_state_id).kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesFromHex,
                source_payload_field: None,
                update_constant_id: None,
                ordered_inputs,
                ..
            } if ordered_inputs == &vec![ValueRef::State(StateId(hex_input_state_id))]
        ));
        assert!(matches!(
            &op_for(decoded_base64_state_id).kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesFromBase64,
                source_payload_field: None,
                update_constant_id: None,
                ordered_inputs,
                ..
            } if ordered_inputs == &vec![ValueRef::State(StateId(base64_input_state_id))]
        ));

        let encode_source_id =
            debug_entry_id(&plan.debug_map.source_routes, "source", "store.encode");
        let joined_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.joined");
        for (target, expression_kind) in [
            ("store.hex", PlanExpressionKind::BytesToHex),
            ("store.base64", PlanExpressionKind::BytesToBase64),
        ] {
            let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", target);
            let op = plan
                .regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs.iter().any(
                        |input| matches!(input, ValueRef::Source(id) if id.0 == encode_source_id),
                    ) && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| panic!("missing encode op for {target}"));
            assert!(matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: actual_kind,
                    source_payload_field: None,
                    update_constant_id: None,
                    ordered_inputs,
                    ..
                } if *actual_kind == expression_kind
                    && ordered_inputs == &vec![ValueRef::State(StateId(joined_state_id))]
            ));
        }
    }

    #[test]
    fn bytes_set_conversion_bank_updates_lower_to_typed_executable_plan_ops() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_set_conversion_bank_plan_ops.bn",
            include_str!("../../../examples/bytes_set_conversion_bank_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(plan.capability_summary.cpu_plan_executor_complete);
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.runtime_ast_dependency_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let constant_value = |constant_id: PlanConstantId| {
            plan.constants
                .iter()
                .find(|constant| constant.id == constant_id)
                .map(|constant| &constant.value)
                .unwrap_or_else(|| panic!("missing plan constant {constant_id:?}"))
        };
        let patch_source_id =
            debug_entry_id(&plan.debug_map.source_routes, "source", "store.patch");
        let inspect_source_id =
            debug_entry_id(&plan.debug_map.source_routes, "source", "store.inspect");
        let left_payload_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "left_payload");
        let patched_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.patched");

        let op_for = |source_id: usize, target: &str| {
            let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", target);
            plan.regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                        && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| panic!("missing update op for {target}"))
        };

        let patch_op = op_for(patch_source_id, "store.patched");
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesSet,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } = &patch_op.kind
        else {
            panic!("Bytes/set op should carry typed ordered operands: {patch_op:#?}");
        };
        let [
            ValueRef::State(input_bytes),
            ValueRef::Constant(index_id),
            ValueRef::Constant(value_id),
        ] = ordered_inputs.as_slice()
        else {
            panic!("Bytes/set ordered operands should be state/index/value: {ordered_inputs:#?}");
        };
        assert_eq!(input_bytes.0, left_payload_state_id);
        assert_eq!(
            constant_value(*index_id),
            &PlanConstantValue::Number { value: 1 }
        );
        assert_eq!(
            constant_value(*value_id),
            &PlanConstantValue::Byte { value: 0x5A }
        );

        let text_op = op_for(inspect_source_id, "store.text");
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesToText,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } = &text_op.kind
        else {
            panic!("Bytes/to_text op should carry typed ordered operands: {text_op:#?}");
        };
        let [ValueRef::State(text_input), ValueRef::Constant(encoding_id)] =
            ordered_inputs.as_slice()
        else {
            panic!(
                "Bytes/to_text ordered operands should be patched state plus encoding: {ordered_inputs:#?}"
            );
        };
        assert_eq!(text_input.0, patched_state_id);
        assert_eq!(
            constant_value(*encoding_id),
            &PlanConstantValue::Text {
                value: "Utf8".to_owned()
            }
        );

        for (target, expression_kind) in [
            ("store.hex", PlanExpressionKind::BytesToHex),
            ("store.base64", PlanExpressionKind::BytesToBase64),
        ] {
            let op = op_for(inspect_source_id, target);
            assert!(matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: actual_kind,
                    source_payload_field: None,
                    update_constant_id: None,
                    ordered_inputs,
                    ..
                } if *actual_kind == expression_kind
                    && ordered_inputs == &vec![ValueRef::State(StateId(patched_state_id))]
            ));
        }
    }

    #[test]
    fn bytes_numeric_updates_lower_to_ordered_typed_executable_plan_ops() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_numeric_plan_ops.bn",
            include_str!("../../../examples/bytes_numeric_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(plan.capability_summary.cpu_plan_executor_complete);
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.runtime_ast_dependency_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let constant_value = |constant_id: PlanConstantId| {
            plan.constants
                .iter()
                .find(|constant| constant.id == constant_id)
                .map(|constant| &constant.value)
                .unwrap_or_else(|| panic!("missing plan constant {constant_id:?}"))
        };
        let number_constant = |value_ref: &ValueRef| {
            let ValueRef::Constant(constant_id) = value_ref else {
                panic!("expected numeric constant ref, got {value_ref:?}");
            };
            match constant_value(*constant_id) {
                PlanConstantValue::Number { value } => *value,
                other => panic!("expected numeric constant, got {other:?}"),
            }
        };
        let text_constant = |value_ref: &ValueRef| {
            let ValueRef::Constant(constant_id) = value_ref else {
                panic!("expected text constant ref, got {value_ref:?}");
            };
            match constant_value(*constant_id) {
                PlanConstantValue::Text { value } => value.as_str(),
                other => panic!("expected text constant, got {other:?}"),
            }
        };
        let op_for = |source_label: &str, target_label: &str| {
            let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", source_label);
            let target_state_id =
                debug_entry_id(&plan.debug_map.state_slots, "state", target_label);
            plan.regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                        && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| panic!("missing numeric op for {source_label} -> {target_label}"))
        };
        let assert_numeric_branch =
            |source_label: &str,
             target_label: &str,
             input_label: &str,
             expected_kind: PlanExpressionKind,
             expected_offset: i64,
             expected_byte_count: i64,
             expected_endian: &str,
             expected_value: Option<i64>| {
                let input_state_id =
                    debug_entry_id(&plan.debug_map.state_slots, "state", input_label);
                let op = op_for(source_label, target_label);
                let PlanOpKind::UpdateBranch {
                    expression_kind,
                    source_payload_field,
                    update_constant_id,
                    ordered_inputs,
                    ..
                } = &op.kind
                else {
                    panic!("expected update branch for {target_label}: {op:#?}");
                };
                assert_eq!(*expression_kind, expected_kind);
                assert_eq!(*source_payload_field, None);
                assert_eq!(*update_constant_id, None);
                match expected_value {
                    Some(value) => {
                        assert_eq!(ordered_inputs.len(), 5);
                        assert_eq!(ordered_inputs[0], ValueRef::State(StateId(input_state_id)));
                        assert_eq!(number_constant(&ordered_inputs[1]), expected_offset);
                        assert_eq!(number_constant(&ordered_inputs[2]), expected_byte_count);
                        assert_eq!(text_constant(&ordered_inputs[3]), expected_endian);
                        assert_eq!(number_constant(&ordered_inputs[4]), value);
                    }
                    None => {
                        assert_eq!(ordered_inputs.len(), 4);
                        assert_eq!(ordered_inputs[0], ValueRef::State(StateId(input_state_id)));
                        assert_eq!(number_constant(&ordered_inputs[1]), expected_offset);
                        assert_eq!(number_constant(&ordered_inputs[2]), expected_byte_count);
                        assert_eq!(text_constant(&ordered_inputs[3]), expected_endian);
                    }
                }
            };

        assert_numeric_branch(
            "store.measure",
            "store.read_u16_le",
            "payload",
            PlanExpressionKind::BytesReadUnsigned,
            0,
            2,
            "Little",
            None,
        );
        assert_numeric_branch(
            "store.measure",
            "store.read_u16_be",
            "payload",
            PlanExpressionKind::BytesReadUnsigned,
            0,
            2,
            "Big",
            None,
        );
        assert_numeric_branch(
            "store.measure",
            "store.read_i16_be",
            "payload",
            PlanExpressionKind::BytesReadSigned,
            2,
            2,
            "Big",
            None,
        );
        assert_numeric_branch(
            "store.measure",
            "store.read_i8",
            "payload",
            PlanExpressionKind::BytesReadSigned,
            5,
            1,
            "Little",
            None,
        );
        assert_numeric_branch(
            "store.write",
            "store.written_unsigned",
            "payload",
            PlanExpressionKind::BytesWriteUnsigned,
            6,
            2,
            "Big",
            Some(4660),
        );
        assert_numeric_branch(
            "store.write",
            "store.written_signed",
            "payload",
            PlanExpressionKind::BytesWriteSigned,
            4,
            2,
            "Little",
            Some(-129),
        );
        assert_numeric_branch(
            "store.inspect",
            "store.written_unsigned_read",
            "store.written_unsigned",
            PlanExpressionKind::BytesReadUnsigned,
            6,
            2,
            "Big",
            None,
        );
        assert_numeric_branch(
            "store.inspect",
            "store.written_signed_read",
            "store.written_signed",
            PlanExpressionKind::BytesReadSigned,
            4,
            2,
            "Little",
            None,
        );

        let write_unsigned_op = op_for("store.write", "store.written_unsigned");
        let endian_constant_id = match &write_unsigned_op.kind {
            PlanOpKind::UpdateBranch { ordered_inputs, .. } => match ordered_inputs.get(3) {
                Some(ValueRef::Constant(constant_id)) => *constant_id,
                other => panic!("missing endian constant for numeric write: {other:?}"),
            },
            other => panic!("expected update branch for numeric write: {other:?}"),
        };
        let mut tampered = plan.clone();
        let constant = tampered
            .constants
            .iter_mut()
            .find(|constant| constant.id == endian_constant_id)
            .expect("tampered plan should contain numeric endian constant");
        constant.value = PlanConstantValue::Text {
            value: "Middle".to_owned(),
        };
        let tampered_verification = verify_plan(&tampered).unwrap();
        assert_eq!(tampered_verification.status, "fail");
        assert!(
            tampered_verification.checks.iter().any(|check| check.id
                == "constant-refs-resolve-and-match-storage-types"
                && !check.pass)
                || tampered_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "unsupported endian constant must fail plan verification: {tampered_verification:#?}"
        );
    }

    #[test]
    fn bytes_numeric_plan_verifier_rejects_invalid_operands_and_output_lengths() {
        let plan = bytes_numeric_fixture_plan();

        let read_u16_le_op_id = update_op_id_for(&plan, "store.measure", "store.read_u16_le");
        let read_byte_count_constant_id = ordered_constant_id(&plan, read_u16_le_op_id, 2);
        let mut invalid_byte_count = plan.clone();
        set_number_constant(&mut invalid_byte_count, read_byte_count_constant_id, 3);
        assert_numeric_plan_rejected(&invalid_byte_count, "unsupported numeric byte_count");

        let read_offset_constant_id = ordered_constant_id(&plan, read_u16_le_op_id, 1);
        let mut out_of_bounds = plan.clone();
        set_number_constant(&mut out_of_bounds, read_offset_constant_id, 7);
        assert_numeric_plan_rejected(&out_of_bounds, "fixed input range out of bounds");

        let write_unsigned_op_id = update_op_id_for(&plan, "store.write", "store.written_unsigned");
        let write_unsigned_value_constant_id = ordered_constant_id(&plan, write_unsigned_op_id, 4);
        let mut unsigned_overflow = plan.clone();
        set_number_constant(
            &mut unsigned_overflow,
            write_unsigned_value_constant_id,
            65_536,
        );
        assert_numeric_plan_rejected(&unsigned_overflow, "unsigned numeric write overflow");

        let write_signed_op_id = update_op_id_for(&plan, "store.write", "store.written_signed");
        let write_signed_value_constant_id = ordered_constant_id(&plan, write_signed_op_id, 4);
        let mut signed_overflow = plan.clone();
        set_number_constant(&mut signed_overflow, write_signed_value_constant_id, 32_768);
        assert_numeric_plan_rejected(&signed_overflow, "signed numeric write overflow");

        let write_unsigned_output_state_id = match &op_by_id(&plan, write_unsigned_op_id).output {
            Some(ValueRef::State(state_id)) => *state_id,
            other => panic!("numeric write should target a state, got {other:?}"),
        };
        let mut fixed_length_mismatch = plan.clone();
        let slot = fixed_length_mismatch
            .storage_layout
            .scalar_slots
            .iter_mut()
            .find(|slot| slot.state_id == write_unsigned_output_state_id)
            .expect("numeric write output state should have a storage slot");
        slot.value_type = PlanValueType::Bytes { fixed_len: Some(7) };
        assert_numeric_plan_rejected(
            &fixed_length_mismatch,
            "numeric write output fixed length mismatch",
        );
    }

    #[test]
    fn bytes_concat_update_lowers_to_ordered_typed_executable_plan_op() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_concat_plan_ops.bn",
            include_str!("../../../examples/bytes_concat_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Bytes/concat root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
        let left_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "left_payload");
        let right_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "right_payload");
        let joined_pipe_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.joined_pipe");
        let joined_call_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.joined_call");

        for (target_state_id, target_label) in [
            (joined_pipe_state_id, "store.joined_pipe"),
            (joined_call_state_id, "store.joined_call"),
        ] {
            let target_slot = plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.state_id.0 == target_state_id)
                .unwrap_or_else(|| panic!("{target_label} storage slot should lower"));
            assert_eq!(
                target_slot.value_type,
                PlanValueType::Bytes { fixed_len: Some(5) }
            );

            let op = plan
                .regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                        && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| {
                    panic!("store.measure should lower to a bytes_concat update for {target_label}")
                });
            assert_eq!(op.unresolved_executable_ref_count, 0);
            assert!(matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesConcat,
                    source_payload_field: None,
                    update_constant_id: None,
                    ordered_inputs,
                    ..
                } if ordered_inputs == &vec![
                    ValueRef::State(StateId(left_state_id)),
                    ValueRef::State(StateId(right_state_id)),
                ]
            ));
        }

        let mut tampered_missing_order = plan.clone();
        let tampered_op = tampered_missing_order
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BytesConcat,
                        ..
                    }
                )
            })
            .expect("fixture should contain a Bytes/concat update op");
        let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &mut tampered_op.kind else {
            unreachable!()
        };
        ordered_inputs.clear();
        let tampered_missing_order_verification = verify_plan(&tampered_missing_order).unwrap();
        assert_eq!(tampered_missing_order_verification.status, "fail");
        assert!(
            tampered_missing_order_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_missing_order_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/concat without ordered operands must fail verifier support: {tampered_missing_order_verification:#?}"
        );

        let mut tampered_fixed_len = plan.clone();
        let target_slot = tampered_fixed_len
            .storage_layout
            .scalar_slots
            .iter_mut()
            .find(|slot| slot.state_id.0 == joined_pipe_state_id)
            .expect("joined_pipe storage slot should lower");
        target_slot.value_type = PlanValueType::Bytes { fixed_len: Some(4) };
        let tampered_fixed_len_verification = verify_plan(&tampered_fixed_len).unwrap();
        assert_eq!(tampered_fixed_len_verification.status, "fail");
        assert!(
            tampered_fixed_len_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_fixed_len_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/concat fixed output length mismatch must fail verifier support: {tampered_fixed_len_verification:#?}"
        );
    }

    #[test]
    fn bytes_slice_take_drop_updates_lower_to_typed_executable_plan_ops() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_slice_take_drop_plan_ops.bn",
            include_str!("../../../examples/bytes_slice_take_drop_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Bytes/slice/take/drop fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.split");
        let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
        let sliced_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.sliced");
        let taken_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.taken");
        let dropped_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.dropped");

        let op_for = |target_state_id: usize| {
            plan.regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                        && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| {
                    panic!("store.split should lower update for state {target_state_id}")
                })
        };

        let sliced_op = op_for(sliced_state_id);
        assert!(matches!(
            &sliced_op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesSlice,
                source_payload_field: None,
                update_constant_id: None,
                ordered_inputs,
                ..
            } if matches!(
                ordered_inputs.as_slice(),
                [
                    ValueRef::State(state_id),
                    ValueRef::Constant(_),
                    ValueRef::Constant(_)
                ] if state_id.0 == payload_state_id
            )
        ));
        let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &sliced_op.kind else {
            unreachable!()
        };
        let [
            _,
            ValueRef::Constant(offset_id),
            ValueRef::Constant(slice_count_id),
        ] = ordered_inputs.as_slice()
        else {
            panic!("slice op should carry ordered constants: {sliced_op:#?}");
        };
        assert_eq!(
            plan.constants
                .iter()
                .find(|constant| constant.id == *offset_id)
                .map(|constant| &constant.value),
            Some(&PlanConstantValue::Number { value: 1 })
        );
        assert_eq!(
            plan.constants
                .iter()
                .find(|constant| constant.id == *slice_count_id)
                .map(|constant| &constant.value),
            Some(&PlanConstantValue::Number { value: 3 })
        );

        let taken_op = op_for(taken_state_id);
        assert!(matches!(
            &taken_op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesTake,
                ordered_inputs,
                ..
            } if matches!(
                ordered_inputs.as_slice(),
                [ValueRef::State(state_id), ValueRef::Constant(_)] if state_id.0 == payload_state_id
            )
        ));
        let dropped_op = op_for(dropped_state_id);
        assert!(matches!(
            &dropped_op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesDrop,
                ordered_inputs,
                ..
            } if matches!(
                ordered_inputs.as_slice(),
                [ValueRef::State(state_id), ValueRef::Constant(_)] if state_id.0 == payload_state_id
            )
        ));

        let mut tampered_fixed_len = plan.clone();
        let target_slot = tampered_fixed_len
            .storage_layout
            .scalar_slots
            .iter_mut()
            .find(|slot| slot.state_id.0 == sliced_state_id)
            .expect("sliced storage slot should lower");
        target_slot.value_type = PlanValueType::Bytes { fixed_len: Some(2) };
        let tampered_fixed_len_verification = verify_plan(&tampered_fixed_len).unwrap();
        assert_eq!(tampered_fixed_len_verification.status, "fail");
        assert!(
            tampered_fixed_len_verification
                .checks
                .iter()
                .any(
                    |check| check.id == "constant-refs-resolve-and-match-storage-types"
                        && !check.pass
                )
                || tampered_fixed_len_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "Bytes/slice fixed output length mismatch must fail verifier support: {tampered_fixed_len_verification:#?}"
        );
    }

    #[test]
    fn bytes_dynamic_slice_count_lowers_to_typed_number_state_operand() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_dynamic_slice_plan_ops.bn",
            include_str!("../../../examples/bytes_dynamic_slice_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "dynamic Bytes/slice fixture should be executable by the CPU PlanExecutor: {:#?}",
            plan.capability_summary
        );
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.split");
        let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
        let count_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.slice_count");
        let output_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.dynamic_sliced");
        let output_slot = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id.0 == output_state_id)
            .expect("dynamic_sliced storage slot should lower");
        assert_eq!(
            output_slot.value_type,
            PlanValueType::Bytes { fixed_len: None }
        );

        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == output_state_id)
            })
            .expect("store.split should lower dynamic Bytes/slice update");
        let PlanOpKind::UpdateBranch {
            expression_kind,
            source_payload_field,
            update_constant_id,
            ordered_inputs,
            ..
        } = &op.kind
        else {
            panic!("dynamic slice should lower as an update branch: {op:#?}");
        };
        assert_eq!(*expression_kind, PlanExpressionKind::BytesSlice);
        assert_eq!(*source_payload_field, None);
        assert_eq!(*update_constant_id, None);
        assert_eq!(ordered_inputs.len(), 3);
        assert_eq!(
            ordered_inputs[0],
            ValueRef::State(StateId(payload_state_id))
        );
        assert!(matches!(ordered_inputs[1], ValueRef::Constant(_)));
        assert_eq!(ordered_inputs[2], ValueRef::State(StateId(count_state_id)));
        assert!(
            op.inputs
                .contains(&ValueRef::State(StateId(count_state_id)))
        );

        let mut tampered = plan.clone();
        let count_slot = tampered
            .storage_layout
            .scalar_slots
            .iter_mut()
            .find(|slot| slot.state_id.0 == count_state_id)
            .expect("count storage slot should lower");
        count_slot.value_type = PlanValueType::Text;
        let tampered_verification = verify_plan(&tampered).unwrap();
        assert_eq!(tampered_verification.status, "fail");
        assert!(
            tampered_verification.checks.iter().any(|check| check.id
                == "constant-refs-resolve-and-match-storage-types"
                && !check.pass)
                || tampered_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "dynamic Bytes/slice count must be a NUMBER state: {tampered_verification:#?}"
        );
    }

    #[test]
    fn verifier_rejects_tampered_inline_bytes_payload() {
        let parsed = boon_parser::parse_source(
            "bytes-plan-literal.bn",
            r#"
source: SOURCE
payload:
    BYTES[4] { 16u01, 16u02, 16u03, 16u04 } |> HOLD payload { LATEST {} }
"#,
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let PlanConstantValue::Bytes {
            inline_bytes: Some(bytes),
            ..
        } = &mut plan.constants[0].value
        else {
            panic!("fixture should produce an inline BYTES constant");
        };
        bytes[0] = 9;

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "byte-constants-match-hashes" && !check.pass),
            "tampered inline bytes should fail verifier: {verification:#?}"
        );
    }

    #[test]
    fn verifier_rejects_tampered_cpu_executor_support_shapes() {
        let parsed = boon_parser::parse_source(
            "examples/root_scalar_plan_ops.bn",
            include_str!("../../../examples/root_scalar_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "root scalar fixture should be executable before tampering: {:#?}",
            plan.capability_summary
        );

        let mut missing_payload_ref = plan.clone();
        let payload_read_op = missing_payload_ref
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::SourcePayload,
                        ..
                    }
                ) && op
                    .inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::SourcePayload { .. }))
            })
            .expect("fixture should contain a SourcePayload update branch");
        payload_read_op
            .inputs
            .retain(|input| !matches!(input, ValueRef::SourcePayload { .. }));
        let missing_payload_ref_verification = verify_plan(&missing_payload_ref).unwrap();
        assert_eq!(missing_payload_ref_verification.status, "fail");
        assert!(
            missing_payload_ref_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "SourcePayload update without its typed payload ref must not satisfy CPU executor support: {missing_payload_ref_verification:#?}"
        );

        let text_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.source_text");
        let mut wrong_bool_input = plan.clone();
        let bool_not_op = wrong_bool_input
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::BoolNot,
                        ..
                    }
                )
            })
            .expect("fixture should contain a BoolNot update branch");
        bool_not_op.inputs = bool_not_op
            .inputs
            .iter()
            .map(|input| match input {
                ValueRef::State(_) => ValueRef::State(StateId(text_state_id)),
                other => other.clone(),
            })
            .collect();
        let wrong_bool_input_verification = verify_plan(&wrong_bool_input).unwrap();
        assert_eq!(wrong_bool_input_verification.status, "fail");
        assert!(
            wrong_bool_input_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "BoolNot update with a non-bool input must not satisfy CPU executor support: {wrong_bool_input_verification:#?}"
        );
    }

    #[test]
    fn source_payload_update_lowers_to_typed_payload_ref() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let source_id = debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "store.sources.new_todo_input.change",
        );
        let state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.new_todo_text");
        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
            })
            .expect("new todo text route should lower to one update op");
        assert_eq!(op.unresolved_executable_ref_count, 0);
        assert!(
            op.inputs.iter().any(|input| matches!(
                input,
                ValueRef::SourcePayload {
                    source_id: input_source_id,
                    field: SourcePayloadField::Text
                } if input_source_id.0 == source_id
            )),
            "source payload should be a typed executable operand: {op:#?}"
        );
        assert!(matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::SourcePayload,
                source_payload_field: Some(SourcePayloadField::Text),
                ..
            }
        ));
    }

    #[test]
    fn verify_plan_rejects_tampered_source_payload_field_after_lowering() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_source_payload_plan_ops.bn",
            include_str!("../../../examples/bytes_source_payload_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        assert_eq!(verify_plan(&plan).unwrap().status, "pass");

        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.receive");
        let state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.received");
        let op = plan
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
            })
            .expect("BYTES source payload route should lower to one update op");
        assert!(
            op.inputs.iter().any(|input| matches!(
                input,
                ValueRef::SourcePayload {
                    source_id: input_source_id,
                    field: SourcePayloadField::Bytes
                } if input_source_id.0 == source_id
            )),
            "source payload should be a typed BYTES executable operand: {op:#?}"
        );
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::SourcePayload,
            source_payload_field,
            ..
        } = &mut op.kind
        else {
            panic!("BYTES source payload route should be a source-payload update branch");
        };
        assert_eq!(*source_payload_field, Some(SourcePayloadField::Bytes));

        *source_payload_field = Some(SourcePayloadField::Text);

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification.checks.iter().any(|check| check.id
                == "constant-refs-resolve-and-match-storage-types"
                && !check.pass),
            "tampered source_payload_field must fail storage type verification: {verification:#?}"
        );
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "tampered source_payload_field must fail derived executor support counts: {verification:#?}"
        );
    }

    #[test]
    fn verify_plan_rejects_tampered_bytes_source_payload_descriptor_type() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_source_payload_plan_ops.bn",
            include_str!("../../../examples/bytes_source_payload_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        assert_eq!(verify_plan(&plan).unwrap().status, "pass");

        let route = plan
            .source_routes
            .iter_mut()
            .find(|route| route.path == "store.receive")
            .expect("fixture should contain store.receive source route");
        let descriptor = route
            .payload_schema
            .typed_fields
            .iter_mut()
            .find(|descriptor| descriptor.field == SourcePayloadField::Bytes)
            .expect("BYTES source route should declare a typed Bytes payload descriptor");
        assert_eq!(descriptor.value_type, SourcePayloadValueType::Bytes);
        descriptor.value_type = SourcePayloadValueType::Text;

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification.checks.iter().any(|check| check.id
                == "constant-refs-resolve-and-match-storage-types"
                && !check.pass),
            "tampered Bytes payload descriptor must fail storage/type verification: {verification:#?}"
        );
    }

    #[test]
    fn verify_plan_rejects_tampered_text_source_payload_field_after_lowering() {
        let parsed = boon_parser::parse_source(
            "examples/root_scalar_plan_ops.bn",
            include_str!("../../../examples/root_scalar_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        assert_eq!(verify_plan(&plan).unwrap().status, "pass");

        let source_id = debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "store.input.change",
        );
        let state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.copied");
        let op = plan
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
            })
            .expect("TEXT source payload route should lower to one update op");
        assert!(
            op.inputs.iter().any(|input| matches!(
                input,
                ValueRef::SourcePayload {
                    source_id: input_source_id,
                    field: SourcePayloadField::Text
                } if input_source_id.0 == source_id
            )),
            "source payload should be a typed TEXT executable operand: {op:#?}"
        );
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::SourcePayload,
            source_payload_field,
            ..
        } = &mut op.kind
        else {
            panic!("TEXT source payload route should be a source-payload update branch");
        };
        assert_eq!(*source_payload_field, Some(SourcePayloadField::Text));

        *source_payload_field = Some(SourcePayloadField::Key);

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification.checks.iter().any(|check| check.id
                == "constant-refs-resolve-and-match-storage-types"
                && !check.pass),
            "TEXT-to-key tamper must fail typed payload declaration verification: {verification:#?}"
        );
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "tampered source_payload_field must fail typed payload operand/executor support counts: {verification:#?}"
        );
    }

    #[test]
    fn verify_plan_accepts_bytes_source_payload_guards() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_source_payload_plan_ops.bn",
            include_str!("../../../examples/bytes_source_payload_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.receive");
        let state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.received");
        let op = plan
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
            })
            .expect("BYTES source payload route should lower to one update op");
        let PlanOpKind::UpdateBranch { source_guard, .. } = &mut op.kind else {
            panic!("BYTES source payload route should be an update branch");
        };
        *source_guard = Some(PlanSourceGuard::SourcePayloadOneOf {
            source_id: SourceId(source_id),
            field: SourcePayloadField::Bytes,
            values: vec!["01fe04".to_owned()],
        });

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && check.pass),
            "BYTES source payload guard should remain executable in the CPU PlanExecutor capability summary: {verification:#?}"
        );
    }

    #[test]
    fn const_update_lowers_to_typed_update_constant() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let source_id = debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "store.sources.filter_active.press",
        );
        let state_id = debug_entry_id(
            &plan.debug_map.state_slots,
            "state",
            "store.selected_filter",
        );
        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
            })
            .expect("filter active route should lower to one update op");
        assert_eq!(op.unresolved_executable_ref_count, 0);
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::Const,
            source_payload_field: None,
            update_constant_id: Some(update_constant_id),
            ..
        } = &op.kind
        else {
            panic!("filter active should lower as a typed Const update branch: {op:#?}");
        };
        let constant = plan
            .constants
            .iter()
            .find(|constant| constant.id == *update_constant_id)
            .expect("const update should reference a plan constant");
        assert_eq!(
            constant.value,
            PlanConstantValue::Enum {
                value: "Active".to_owned()
            }
        );
    }

    #[test]
    fn verifier_rejects_tampered_const_update_constant_ref() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let source_id = debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "store.sources.filter_active.press",
        );
        let state_id = debug_entry_id(
            &plan.debug_map.state_slots,
            "state",
            "store.selected_filter",
        );
        let op = plan
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
            })
            .expect("filter active route should lower to one update op");
        let PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::Const,
            update_constant_id,
            ..
        } = &mut op.kind
        else {
            panic!("filter active should lower as a typed Const update branch: {op:#?}");
        };
        *update_constant_id = Some(PlanConstantId(usize::MAX));

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification.checks.iter().any(|check| {
                check.id == "constant-refs-resolve-and-match-storage-types" && !check.pass
            }),
            "tampered const update ref should fail verifier: {verification:#?}"
        );
    }

    #[test]
    fn todomvc_row_aliases_lower_to_executable_plan_refs() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        assert!(
            plan.capability_summary.typed_lowering_executable,
            "TodoMVC should lower to a structurally typed MachinePlan: {:#?}",
            plan.capability_summary
        );
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "TodoMVC whole-plan CPU executor readiness should be true after retain/list-view execution support: {:#?}",
            plan.capability_summary
        );
        assert_eq!(plan.capability_summary.unresolved_executable_ref_count, 0);
        assert_eq!(plan.capability_summary.executable_string_path_count, 0);
        assert!(
            plan.debug_map.unresolved_executable_refs.is_empty(),
            "row aliases should be resolved through typed refs, got {:?}",
            plan.debug_map.unresolved_executable_refs
        );
    }

    #[test]
    fn todomvc_append_lowers_to_typed_trigger_fields_and_initial_rows() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let title_to_add_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.title_to_add",
        );
        let todos_id = debug_entry_id(&plan.debug_map.list_slots, "list", "todos");
        let append_op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::ListOperations)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                matches!(&op.output, Some(ValueRef::List(id)) if id.0 == todos_id)
                    && matches!(
                        &op.kind,
                        PlanOpKind::ListOperation {
                            operation_kind: PlanListOperationKind::Append,
                            ..
                        }
                    )
            })
            .expect("TodoMVC todos append should lower to one list op");
        let PlanOpKind::ListOperation {
            operation_kind: PlanListOperationKind::Append,
            append: Some(append),
            ..
        } = &append_op.kind
        else {
            panic!("append op should carry typed append details: {append_op:#?}");
        };
        assert_eq!(append.trigger, ValueRef::Field(FieldId(title_to_add_id)));
        assert!(append_op.inputs.contains(&append.trigger));
        assert_eq!(append.fields.len(), 1);
        assert_eq!(append.fields[0].name, "title");
        assert_eq!(append.fields[0].field_id, Some(FieldId(11)));
        assert_eq!(
            append.fields[0].value_ref,
            Some(ValueRef::Field(FieldId(title_to_add_id)))
        );
        assert_eq!(append.fields[0].constant_id, None);
        let list_slot = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id.0 == todos_id)
            .expect("todos list slot should exist");
        assert_eq!(list_slot.initial_rows.len(), 4);
        assert_eq!(
            list_slot.initial_rows[0].fields[0].field_id,
            Some(FieldId(11))
        );
        assert_eq!(
            list_slot.initial_rows[0].fields[1].field_id,
            Some(FieldId(13))
        );
        assert_eq!(
            list_slot.initial_rows[0].fields[0].value,
            PlanConstantValue::Text {
                value: "Read documentation".to_owned()
            }
        );
        assert_eq!(
            list_slot.initial_rows[1].fields[1].value,
            PlanConstantValue::Bool { value: true }
        );
        assert!(
            verify_plan(&plan)
                .unwrap()
                .checks
                .iter()
                .any(|check| check.id == "list-initial-row-fields-resolve" && check.pass),
            "initial row refs should verify"
        );
        assert!(
            verify_plan(&plan)
                .unwrap()
                .checks
                .iter()
                .any(|check| check.id == "list-append-refs-resolve" && check.pass),
            "append refs should verify"
        );
    }

    #[test]
    fn todomvc_remove_lowers_to_typed_source_and_predicate() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let todos_id = debug_entry_id(&plan.debug_map.list_slots, "list", "todos");
        let remove_source_id = debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "todo.sources.remove_todo_button.press",
        );
        let clear_source_id = debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "store.sources.clear_completed_button.press",
        );
        let completed_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "todo.completed");

        let remove_ops = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::ListOperations)
            .flat_map(|region| region.ops.iter())
            .filter(|op| {
                matches!(&op.output, Some(ValueRef::List(id)) if id.0 == todos_id)
                    && matches!(
                        &op.kind,
                        PlanOpKind::ListOperation {
                            operation_kind: PlanListOperationKind::Remove,
                            ..
                        }
                    )
            })
            .collect::<Vec<_>>();
        assert_eq!(remove_ops.len(), 2);

        let row_remove = remove_ops
            .iter()
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::ListOperation {
                        remove: Some(PlanListRemove {
                            source: ValueRef::Source(source_id),
                            predicate: PlanListRemovePredicate::AlwaysTrue,
                        }),
                        ..
                    } if source_id.0 == remove_source_id
                )
            })
            .expect("row remove source should lower to typed AlwaysTrue remove metadata");
        let PlanOpKind::ListOperation {
            remove: Some(row_remove_plan),
            ..
        } = &row_remove.kind
        else {
            panic!("row remove op should carry typed remove metadata");
        };
        assert!(row_remove.inputs.contains(&row_remove_plan.source));

        let clear_remove = remove_ops
            .iter()
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::ListOperation {
                        remove: Some(PlanListRemove {
                            source: ValueRef::Source(source_id),
                            predicate: PlanListRemovePredicate::RowFieldBool {
                                input: ValueRef::State(state_id),
                            },
                        }),
                        ..
                    } if source_id.0 == clear_source_id && state_id.0 == completed_state_id
                )
            })
            .expect("clear-completed should lower to typed row-field bool remove predicate");
        let PlanOpKind::ListOperation {
            remove: Some(clear_remove_plan),
            ..
        } = &clear_remove.kind
        else {
            panic!("clear-completed op should carry typed remove metadata");
        };
        assert!(clear_remove.inputs.contains(&clear_remove_plan.source));
        assert!(matches!(
            &clear_remove_plan.predicate,
            PlanListRemovePredicate::RowFieldBool {
                input: ValueRef::State(state_id),
            } if state_id.0 == completed_state_id
        ));
        assert!(
            clear_remove
                .inputs
                .contains(&ValueRef::State(StateId(completed_state_id)))
        );
        assert!(
            verify_plan(&plan)
                .unwrap()
                .checks
                .iter()
                .any(|check| check.id == "list-remove-refs-resolve" && check.pass),
            "remove refs should verify"
        );
    }

    #[test]
    fn todomvc_counts_and_has_completed_lower_to_typed_refs() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let todos_id = debug_entry_id(&plan.debug_map.list_slots, "list", "todos");
        let completed_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "todo.completed");
        let active_count_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.active_count",
        );
        let completed_count_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.completed_count",
        );
        let has_completed_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.has_completed",
        );

        let count_ops = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::ListOperations)
            .flat_map(|region| region.ops.iter())
            .filter(|op| {
                matches!(&op.output, Some(ValueRef::List(id)) if id.0 == todos_id)
                    && matches!(
                        &op.kind,
                        PlanOpKind::ListOperation {
                            operation_kind: PlanListOperationKind::Count,
                            ..
                        }
                    )
            })
            .collect::<Vec<_>>();
        assert_eq!(count_ops.len(), 2);
        assert!(count_ops.iter().any(|op| {
            matches!(
                &op.kind,
                PlanOpKind::ListOperation {
                    count: Some(PlanListCount {
                        target: ValueRef::Field(field_id),
                        predicate: PlanListRemovePredicate::RowFieldBoolNot {
                            input: ValueRef::State(state_id),
                        },
                    }),
                    ..
                } if field_id.0 == active_count_id && state_id.0 == completed_state_id
            )
        }));
        assert!(count_ops.iter().any(|op| {
            matches!(
                &op.kind,
                PlanOpKind::ListOperation {
                    count: Some(PlanListCount {
                        target: ValueRef::Field(field_id),
                        predicate: PlanListRemovePredicate::RowFieldBool {
                            input: ValueRef::State(state_id),
                        },
                    }),
                    ..
                } if field_id.0 == completed_count_id && state_id.0 == completed_state_id
            )
        }));

        let has_completed = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == has_completed_id)
            })
            .expect("store.has_completed derived op should lower");
        assert!(matches!(
            &has_completed.kind,
            PlanOpKind::DerivedValue {
                derived_kind: PlanDerivedKind::Pure,
                expression: Some(PlanDerivedExpression::NumberCompareConst {
                    left: ValueRef::Field(field_id),
                    op,
                    right: 0,
                }),
                ..
            } if field_id.0 == completed_count_id && op == ">"
        ));

        let checks = verify_plan(&plan).unwrap().checks;
        assert!(
            checks
                .iter()
                .any(|check| check.id == "list-count-refs-resolve" && check.pass),
            "count refs should verify"
        );
        assert!(
            checks
                .iter()
                .any(|check| check.id == "derived-expression-refs-resolve" && check.pass),
            "derived numeric compare refs should verify"
        );
    }

    #[test]
    fn todomvc_typed_remove_and_count_list_ops_are_cpu_supported() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let unsupported_list_ops = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::ListOperations)
            .flat_map(|region| region.ops.iter())
            .filter(|op| {
                !cpu_plan_executor_supports_whole_plan_op(
                    &plan.storage_layout.scalar_slots,
                    &plan.storage_layout.list_slots,
                    &plan.constants,
                    op,
                    &BTreeSet::new(),
                    &BTreeSet::new(),
                    &BTreeSet::new(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(unsupported_list_ops.len(), 0);
        assert!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count
                <= 2,
            "TodoMVC list remove/count/retain ops should no longer be counted unsupported once typed retain execution exists"
        );
    }

    #[test]
    fn todomvc_retain_list_view_carries_typed_selected_filter_metadata() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let todos_id = debug_entry_id(&plan.debug_map.list_slots, "list", "todos");
        let visible_todos_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.visible_todos",
        );
        let selected_filter_id = debug_entry_id(
            &plan.debug_map.state_slots,
            "state",
            "store.selected_filter",
        );
        let completed_id = debug_entry_id(&plan.debug_map.state_slots, "state", "todo.completed");
        let retain_op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::ListOperations)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                matches!(op.output, Some(ValueRef::List(list_id)) if list_id.0 == todos_id)
                    && matches!(
                        &op.kind,
                        PlanOpKind::ListOperation {
                            operation_kind: PlanListOperationKind::Retain,
                            retain: Some(PlanListRetain {
                                target: ValueRef::Field(field_id),
                                predicate:
                                    PlanListRemovePredicate::SelectedFilterVisibility {
                                        selector: ValueRef::State(selector_id),
                                        row_field: ValueRef::State(row_field_id),
                                    },
                            }),
                            ..
                        } if field_id.0 == visible_todos_id
                            && selector_id.0 == selected_filter_id
                            && row_field_id.0 == completed_id
                    )
            })
            .expect("TodoMVC visible_todos retain op should carry typed selected-filter metadata");

        assert!(
            retain_op
                .inputs
                .contains(&ValueRef::Field(FieldId(visible_todos_id)))
        );
        assert!(
            retain_op
                .inputs
                .contains(&ValueRef::State(StateId(selected_filter_id)))
        );
        assert!(
            retain_op
                .inputs
                .contains(&ValueRef::State(StateId(completed_id)))
        );
        assert!(
            verify_plan(&plan)
                .unwrap()
                .checks
                .iter()
                .any(|check| check.id == "list-retain-refs-resolve" && check.pass),
            "retain refs should verify"
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0,
            "typed retain metadata is preserved and the retain-backed ListView is CPU-supported"
        );
    }

    #[test]
    fn todomvc_root_number_compare_over_typed_count_is_cpu_supported() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let has_completed_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.has_completed",
        );
        let has_completed = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == has_completed_id)
            })
            .expect("store.has_completed derived op should lower");

        let supported_count_outputs = plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .filter_map(|op| match (&op.kind, op.output.clone()) {
                (
                    PlanOpKind::ListOperation {
                        operation_kind,
                        append,
                        remove,
                        retain,
                        count: Some(count),
                        ..
                    },
                    Some(ValueRef::List(_)),
                ) if cpu_plan_executor_supports_list_operation_op(
                    op,
                    *operation_kind,
                    append.as_ref(),
                    remove.as_ref(),
                    retain.as_ref(),
                    Some(count),
                    &plan.constants,
                ) =>
                {
                    match count.target {
                        ValueRef::Field(field_id) => Some(field_id),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();

        assert!(cpu_plan_executor_supports_whole_plan_op(
            &plan.storage_layout.scalar_slots,
            &plan.storage_layout.list_slots,
            &plan.constants,
            has_completed,
            &BTreeSet::new(),
            &supported_count_outputs,
            &BTreeSet::new(),
        ));
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0,
            "typed root numeric compare and typed root summary boolean expressions over supported List/count fields should reduce the TodoMVC unsupported count"
        );
    }

    #[test]
    fn todomvc_root_summary_booleans_lower_to_typed_boolean_expressions() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let has_todos_id =
            debug_entry_id(&plan.debug_map.derived_values, "field", "store.has_todos");
        let all_completed_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.all_completed",
        );
        let summary_ops = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .filter_map(|op| {
                let Some(ValueRef::Field(field_id)) = op.output else {
                    return None;
                };
                if field_id.0 != has_todos_id && field_id.0 != all_completed_id {
                    return None;
                }
                Some((field_id.0, op))
            })
            .collect::<BTreeMap<_, _>>();

        let has_todos = summary_ops
            .get(&has_todos_id)
            .expect("store.has_todos should lower");
        assert!(matches!(
            &has_todos.kind,
            PlanOpKind::DerivedValue {
                derived_kind: PlanDerivedKind::Pure,
                expression: Some(PlanDerivedExpression::BoolNotExpression { input }),
                ..
            } if matches!(input.as_ref(), PlanDerivedExpression::BoolAnd { .. })
        ));

        let all_completed = summary_ops
            .get(&all_completed_id)
            .expect("store.all_completed should lower");
        assert!(matches!(
            &all_completed.kind,
            PlanOpKind::DerivedValue {
                derived_kind: PlanDerivedKind::Pure,
                expression: Some(PlanDerivedExpression::BoolAnd { .. }),
                ..
            }
        ));

        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0,
            "typed root boolean expressions over supported List/count fields should reduce the TodoMVC unsupported count"
        );
    }

    #[test]
    fn todomvc_aggregate_derived_counts_are_cpu_supported_from_typed_list_counts() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let active_count_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.active_count",
        );
        let completed_count_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.completed_count",
        );
        let aggregate_ops = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .filter(|op| {
                matches!(
                    op.output,
                    Some(ValueRef::Field(field_id))
                        if field_id.0 == active_count_id || field_id.0 == completed_count_id
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(aggregate_ops.len(), 2);
        assert!(aggregate_ops.iter().all(|op| matches!(
            &op.kind,
            PlanOpKind::DerivedValue {
                derived_kind: PlanDerivedKind::Aggregate,
                expression: None,
                ..
            }
        )));
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0,
            "aggregate derived count fields backed by supported typed List/count ops should not remain unsupported"
        );
    }

    #[test]
    fn todomvc_guarded_root_const_update_is_cpu_supported() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let new_todo_text_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.new_todo_text");
        let guarded_clear = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                !op.indexed
                    && matches!(op.output, Some(ValueRef::State(state_id)) if state_id.0 == new_todo_text_id)
                    && matches!(
                        &op.kind,
                        PlanOpKind::UpdateBranch {
                            expression_kind: PlanExpressionKind::Const,
                            source_guard: Some(PlanSourceGuard::SourcePayloadOneOf {
                                field: SourcePayloadField::Key,
                                values,
                                ..
                            }),
                            ..
                        } if values == &vec!["Enter".to_owned()]
                    )
            })
            .expect("TodoMVC Enter clear branch should lower with typed source guard");

        assert!(cpu_plan_executor_supports_whole_plan_op(
            &plan.storage_layout.scalar_slots,
            &plan.storage_layout.list_slots,
            &plan.constants,
            guarded_clear,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        ));
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0,
            "root const update branches guarded by a typed source-payload one-of guard should be CPU-supported"
        );
    }

    #[test]
    fn todomvc_indexed_text_trim_or_previous_updates_are_cpu_supported() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let title_id = debug_entry_id(&plan.debug_map.state_slots, "state", "todo.title");
        let edit_text_id = debug_entry_id(&plan.debug_map.state_slots, "state", "todo.edit_text");
        let text_trim_updates = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .filter(|op| {
                op.indexed
                    && matches!(
                        &op.kind,
                        PlanOpKind::UpdateBranch {
                            expression_kind: PlanExpressionKind::TextTrimOrPrevious,
                            ..
                        }
                    )
                    && matches!(
                        op.output,
                        Some(ValueRef::State(state_id))
                            if state_id.0 == title_id || state_id.0 == edit_text_id
                    )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            text_trim_updates.len(),
            3,
            "TodoMVC should lower title/edit draft text-trim updates explicitly"
        );

        assert!(
            text_trim_updates.iter().any(|op| matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    source_payload_field: None,
                    source_guard: Some(PlanSourceGuard::SourcePayloadOneOf {
                        field: SourcePayloadField::Key,
                        values,
                        ..
                    }),
                    ..
                } if matches!(op.output, Some(ValueRef::State(state_id)) if state_id.0 == title_id)
                    && values == &vec!["Enter".to_owned()]
            )),
            "Enter commit should be a guarded indexed title trim branch"
        );
        assert!(
            text_trim_updates.iter().any(|op| matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    source_payload_field: Some(SourcePayloadField::Text),
                    ..
                } if matches!(op.output, Some(ValueRef::State(state_id)) if state_id.0 == edit_text_id)
            )),
            "edit draft change should use typed Text source payload plus text fallback state"
        );
        assert!(
            text_trim_updates
                .iter()
                .all(|op| cpu_plan_executor_supports_whole_plan_op(
                    &plan.storage_layout.scalar_slots,
                    &plan.storage_layout.list_slots,
                    &plan.constants,
                    op,
                    &BTreeSet::new(),
                    &BTreeSet::new(),
                    &BTreeSet::new(),
                )),
            "indexed text-trim branches are implemented in runtime/schema and should be counted CPU-supported"
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0,
            "only TodoMVC list retain/list-view support should remain unsupported after indexed text-trim support accounting"
        );
    }

    #[test]
    fn cells_unscoped_record_literal_initial_rows_get_typed_field_ids() {
        let parsed = parse_cells_project_for_plan_test();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let defaults_id =
            debug_entry_id(&plan.debug_map.list_slots, "list", "cells_default_values");
        let list_slot = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id.0 == defaults_id)
            .expect("Cells default values list slot should exist");
        assert_eq!(list_slot.initial_rows.len(), 5);
        let first_fields = &list_slot.initial_rows[0].fields;
        assert_eq!(first_fields.len(), 3);
        for field in first_fields {
            assert!(
                field.field_id.is_some(),
                "unscoped static list field `{}` should receive a typed synthetic field id",
                field.name
            );
        }
        let debug_labels = first_fields
            .iter()
            .map(|field| {
                let id = field.field_id.expect("field id checked above");
                plan.debug_map
                    .fields
                    .iter()
                    .find(|entry| entry.id == format!("field:{}", id.0))
                    .map(|entry| entry.label.clone())
                    .expect("synthetic field id should be debuggable")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            debug_labels,
            vec![
                "cells_default_values.address".to_owned(),
                "cells_default_values.field".to_owned(),
                "cells_default_values.value".to_owned()
            ]
        );
        assert!(
            verify_plan(&plan)
                .unwrap()
                .checks
                .iter()
                .any(|check| check.id == "list-initial-row-fields-resolve" && check.pass),
            "Cells static list initial row refs should verify"
        );
    }

    #[test]
    fn cells_range_list_preserves_typed_bounds() {
        let parsed = parse_cells_project_for_plan_test();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let cells_id = debug_entry_id(&plan.debug_map.list_slots, "list", "cells");
        let list_slot = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id.0 == cells_id)
            .expect("Cells range list slot should exist");
        assert_eq!(list_slot.initializer_kind, ListInitializerKind::Range);
        assert_eq!(
            list_slot.range,
            Some(PlanRangeInitializer { from: 0, to: 2599 })
        );
        let cells_index_id = debug_entry_id(&plan.debug_map.fields, "field", "cells.index");
        let address_id = debug_entry_id(&plan.debug_map.fields, "field", "cell.address");
        let address_op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .find(|op| matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == address_id))
            .expect("cell.address should have a derived op");
        assert!(
            address_op
                .inputs
                .contains(&ValueRef::Field(FieldId(cells_index_id))),
            "cell.address should depend on the typed synthetic range row index"
        );
        assert!(
            matches!(
                &address_op.kind,
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::RowExpression {
                        expression: PlanRowExpression::TextConcat { .. }
                    }),
                    ..
                }
            ),
            "cell.address should lower to an executable generic row expression"
        );
        assert!(
            verify_plan(&plan)
                .unwrap()
                .checks
                .iter()
                .any(|check| check.id == "list-range-bounds-resolve" && check.pass),
            "Cells range bounds should verify"
        );
    }

    #[test]
    fn cells_display_text_when_lowers_to_cpu_supported_row_select() {
        let parsed = parse_cells_project_for_plan_test();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let display_text_id = debug_entry_id(&plan.debug_map.fields, "field", "cell.display_text");
        let display_text = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                matches!(
                    op.output,
                    Some(ValueRef::Field(field_id)) if field_id.0 == display_text_id
                )
            })
            .expect("cell.display_text derived op should lower");

        assert!(
            matches!(
                &display_text.kind,
                PlanOpKind::DerivedValue {
                    derived_kind: PlanDerivedKind::Pure,
                    expression: Some(PlanDerivedExpression::RowExpression {
                        expression: PlanRowExpression::Select { .. }
                    }),
                    ..
                }
            ),
            "cell.display_text WHEN expression should lower as a generic row select"
        );
        assert!(
            cpu_plan_executor_supports_whole_plan_op(
                &plan.storage_layout.scalar_slots,
                &plan.storage_layout.list_slots,
                &plan.constants,
                display_text,
                &BTreeSet::new(),
                &BTreeSet::new(),
                &BTreeSet::new(),
            ),
            "cell.display_text row select should be executable by the generic CPU PlanExecutor"
        );
        assert!(
            plan.capability_summary.cpu_plan_executor_complete,
            "Cells MachinePlan should be CPU-complete once row WHEN expressions lower generically"
        );
        assert_eq!(
            plan.capability_summary
                .cpu_plan_executor_unsupported_op_count,
            0
        );
    }

    #[test]
    fn row_bytes_predicates_lower_to_typed_expressions() {
        fn count_kind(value: &serde_json::Value, kind: &str) -> usize {
            match value {
                serde_json::Value::Object(object) => {
                    usize::from(object.get("kind").and_then(|value| value.as_str()) == Some(kind))
                        + object
                            .values()
                            .map(|value| count_kind(value, kind))
                            .sum::<usize>()
                }
                serde_json::Value::Array(items) => {
                    items.iter().map(|value| count_kind(value, kind)).sum()
                }
                _ => 0,
            }
        }

        fn count_generic_builtin(value: &serde_json::Value, function: &str) -> usize {
            match value {
                serde_json::Value::Object(object) => {
                    usize::from(
                        object.get("kind").and_then(|value| value.as_str()) == Some("builtin_call")
                            && object.get("function").and_then(|value| value.as_str())
                                == Some(function),
                    ) + object
                        .values()
                        .map(|value| count_generic_builtin(value, function))
                        .sum::<usize>()
                }
                serde_json::Value::Array(items) => items
                    .iter()
                    .map(|value| count_generic_builtin(value, function))
                    .sum(),
                _ => 0,
            }
        }

        fn replace_first_row_bytes_builtin(
            expression: &mut PlanRowExpression,
            function: &str,
        ) -> bool {
            match expression {
                PlanRowExpression::BytesIsEmpty { input } if function == "Bytes/is_empty" => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesLength { input } if function == "Bytes/length" => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesGet { input, index } if function == "Bytes/get" => {
                    let input = (**input).clone();
                    let index = (**index).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("index".to_owned()),
                            value: index,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesTake { input, byte_count } if function == "Bytes/take" => {
                    let input = (**input).clone();
                    let byte_count = (**byte_count).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesDrop { input, byte_count } if function == "Bytes/drop" => {
                    let input = (**input).clone();
                    let byte_count = (**byte_count).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesZeros { byte_count } if function == "Bytes/zeros" => {
                    let byte_count = (**byte_count).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: None,
                        args: vec![PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } if function == "Bytes/read_unsigned" => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    let endian = (**endian).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                            PlanRowCallArg {
                                name: Some("endian".to_owned()),
                                value: endian,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } if function == "Bytes/read_signed" => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    let endian = (**endian).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                            PlanRowCallArg {
                                name: Some("endian".to_owned()),
                                value: endian,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } if function == "Bytes/set" => {
                    let input = (**input).clone();
                    let index = (**index).clone();
                    let value = (**value).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("index".to_owned()),
                                value: index,
                            },
                            PlanRowCallArg {
                                name: Some("value".to_owned()),
                                value,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } if function == "Bytes/write_unsigned" => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    let endian = (**endian).clone();
                    let value = (**value).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                            PlanRowCallArg {
                                name: Some("endian".to_owned()),
                                value: endian,
                            },
                            PlanRowCallArg {
                                name: Some("value".to_owned()),
                                value,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } if function == "Bytes/write_signed" => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    let endian = (**endian).clone();
                    let value = (**value).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                            PlanRowCallArg {
                                name: Some("endian".to_owned()),
                                value: endian,
                            },
                            PlanRowCallArg {
                                name: Some("value".to_owned()),
                                value,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesEndsWith { input, suffix }
                    if function == "Bytes/ends_with" =>
                {
                    let input = (**input).clone();
                    let suffix = (**suffix).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("suffix".to_owned()),
                            value: suffix,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesConcat { left, right } if function == "Bytes/concat" => {
                    let left = (**left).clone();
                    let right = (**right).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(left)),
                        args: vec![PlanRowCallArg {
                            name: Some("with".to_owned()),
                            value: right,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesToText { input, encoding }
                    if function == "Bytes/to_text" =>
                {
                    let input = (**input).clone();
                    let args = encoding
                        .as_deref()
                        .map(|encoding| PlanRowCallArg {
                            name: Some("encoding".to_owned()),
                            value: encoding.clone(),
                        })
                        .into_iter()
                        .collect();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args,
                    };
                    true
                }
                PlanRowExpression::BytesToHex { input } if function == "Bytes/to_hex" => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesToBase64 { input } if function == "Bytes/to_base64" => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesFromHex { input } if function == "Bytes/from_hex" => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesFromBase64 { input } if function == "Bytes/from_base64" => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesEqual { left, right } if function == "Bytes/equal" => {
                    let left = (**left).clone();
                    let right = (**right).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: function.to_owned(),
                        input: Some(Box::new(left)),
                        args: vec![PlanRowCallArg {
                            name: Some("with".to_owned()),
                            value: right,
                        }],
                    };
                    true
                }
                PlanRowExpression::Object { fields } => fields
                    .iter_mut()
                    .any(|field| replace_first_row_bytes_builtin(&mut field.value, function)),
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input }
                | PlanRowExpression::BytesIsEmpty { input }
                | PlanRowExpression::BytesLength { input } => {
                    replace_first_row_bytes_builtin(input, function)
                }
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(prefix, function)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(start, function)
                        || replace_first_row_bytes_builtin(length, function)
                }
                PlanRowExpression::TextToBytes { input, encoding } => {
                    replace_first_row_bytes_builtin(input, function)
                        || encoding.as_deref_mut().is_some_and(|encoding| {
                            replace_first_row_bytes_builtin(encoding, function)
                        })
                }
                PlanRowExpression::BytesToText { input, encoding } => {
                    replace_first_row_bytes_builtin(input, function)
                        || encoding.as_deref_mut().is_some_and(|encoding| {
                            replace_first_row_bytes_builtin(encoding, function)
                        })
                }
                PlanRowExpression::BytesToHex { input }
                | PlanRowExpression::BytesToBase64 { input }
                | PlanRowExpression::BytesFromHex { input }
                | PlanRowExpression::BytesFromBase64 { input } => {
                    replace_first_row_bytes_builtin(input, function)
                }
                PlanRowExpression::BytesGet { input, index } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(index, function)
                }
                PlanRowExpression::BytesSlice {
                    input,
                    offset,
                    byte_count,
                } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(offset, function)
                        || replace_first_row_bytes_builtin(byte_count, function)
                }
                PlanRowExpression::BytesTake { input, byte_count }
                | PlanRowExpression::BytesDrop { input, byte_count } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(byte_count, function)
                }
                PlanRowExpression::BytesZeros { byte_count } => {
                    replace_first_row_bytes_builtin(byte_count, function)
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                }
                | PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(offset, function)
                        || replace_first_row_bytes_builtin(byte_count, function)
                        || replace_first_row_bytes_builtin(endian, function)
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(index, function)
                        || replace_first_row_bytes_builtin(value, function)
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                }
                | PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(offset, function)
                        || replace_first_row_bytes_builtin(byte_count, function)
                        || replace_first_row_bytes_builtin(endian, function)
                        || replace_first_row_bytes_builtin(value, function)
                }
                PlanRowExpression::BytesFind { input, needle } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(needle, function)
                }
                PlanRowExpression::BytesStartsWith { input, prefix } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(prefix, function)
                }
                PlanRowExpression::BytesEndsWith { input, suffix } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(suffix, function)
                }
                PlanRowExpression::BytesConcat { left, right } => {
                    replace_first_row_bytes_builtin(left, function)
                        || replace_first_row_bytes_builtin(right, function)
                }
                PlanRowExpression::BytesEqual { left, right } => {
                    replace_first_row_bytes_builtin(left, function)
                        || replace_first_row_bytes_builtin(right, function)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    replace_first_row_bytes_builtin(left, function)
                        || replace_first_row_bytes_builtin(right, function)
                }
                PlanRowExpression::TextConcat { parts } => parts
                    .iter_mut()
                    .any(|part| replace_first_row_bytes_builtin(part, function)),
                PlanRowExpression::ListGetField { index, .. } => {
                    replace_first_row_bytes_builtin(index, function)
                }
                PlanRowExpression::ListFindValue {
                    value, fallback, ..
                } => {
                    replace_first_row_bytes_builtin(value, function)
                        || fallback.as_deref_mut().is_some_and(|fallback| {
                            replace_first_row_bytes_builtin(fallback, function)
                        })
                }
                PlanRowExpression::ListRange { from, to } => {
                    replace_first_row_bytes_builtin(from, function)
                        || replace_first_row_bytes_builtin(to, function)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    replace_first_row_bytes_builtin(input, function)
                        || replace_first_row_bytes_builtin(value, function)
                }
                PlanRowExpression::BuiltinCall { input, args, .. } => {
                    input
                        .as_deref_mut()
                        .is_some_and(|input| replace_first_row_bytes_builtin(input, function))
                        || args
                            .iter_mut()
                            .any(|arg| replace_first_row_bytes_builtin(&mut arg.value, function))
                }
                PlanRowExpression::Select { input, arms } => {
                    replace_first_row_bytes_builtin(input, function)
                        || arms
                            .iter_mut()
                            .any(|arm| replace_first_row_bytes_builtin(&mut arm.value, function))
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        let source = r#"
store: [
    trigger: SOURCE
    tick:
        0 |> HOLD tick {
            LATEST {
                trigger |> THEN { 1 }
            }
        }
    rows:
        LIST {
            [payload: BYTES[3] { 16u41, 16u42, 16u43 }]
            [payload: BYTES[0] {}]
        }
        |> List/map(row, new: row_summary(row: row))
]

FUNCTION row_summary(row) {
    [
        payload: row.payload
        suffix: BYTES[2] { 16u42, 16u43 }
        empty: row.payload |> Bytes/is_empty
        len: row.payload |> Bytes/length
        second: row.payload |> Bytes/get(index: 1)
        taken: row.payload |> Bytes/take(byte_count: 2)
        dropped: row.payload |> Bytes/drop(byte_count: 1)
        zeroed: Bytes/zeros(byte_count: 2)
        read_u16_be: row.payload |> Bytes/read_unsigned(offset: 0, byte_count: 2, endian: Big)
        read_i8: row.payload |> Bytes/read_signed(offset: 0, byte_count: 1, endian: Little)
        patched: row.payload |> Bytes/set(index: 1, value: 16uFF)
        write_u16_be: row.payload |> Bytes/write_unsigned(offset: 0, byte_count: 2, endian: Big, value: 258)
        write_i8: row.payload |> Bytes/write_signed(offset: 0, byte_count: 1, endian: Little, value: -1)
        ends: row.payload |> Bytes/ends_with(suffix: BYTES[2] { 16u42, 16u43 })
        same: row.payload |> Bytes/equal(with: row.payload)
        joined: row.payload |> Bytes/concat(with: BYTES[1] { 16u04 })
        text: row.payload |> Bytes/to_text(encoding: Utf8)
        hex: row.payload |> Bytes/to_hex
        base64: row.payload |> Bytes/to_base64
        decoded_hex: TEXT { 414243 } |> Bytes/from_hex
        decoded_base64: TEXT { QUJD } |> Bytes/from_base64
    ]
}

document: Document/new(root: Element/label(element: [], label: TEXT { row bytes }))
"#;

        let parsed = boon_parser::parse_source("row-bytes-predicates.bn", source).unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let plan_json = serde_json::to_value(&plan).unwrap();

        assert_eq!(count_kind(&plan_json, "bytes_is_empty"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_length"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_get"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_take"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_drop"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_zeros"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_read_unsigned"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_read_signed"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_set"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_write_unsigned"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_write_signed"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_ends_with"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_equal"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_concat"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_to_text"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_to_hex"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_to_base64"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_from_hex"), 1);
        assert_eq!(count_kind(&plan_json, "bytes_from_base64"), 1);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/is_empty"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/length"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/get"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/take"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/drop"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/zeros"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/read_unsigned"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/read_signed"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/set"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/write_unsigned"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/write_signed"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/ends_with"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/equal"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/concat"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/to_text"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/to_hex"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/to_base64"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/from_hex"), 0);
        assert_eq!(count_generic_builtin(&plan_json, "Bytes/from_base64"), 0);

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "pass");

        for function in [
            "Bytes/is_empty",
            "Bytes/length",
            "Bytes/get",
            "Bytes/take",
            "Bytes/drop",
            "Bytes/zeros",
            "Bytes/read_unsigned",
            "Bytes/read_signed",
            "Bytes/set",
            "Bytes/write_unsigned",
            "Bytes/write_signed",
            "Bytes/ends_with",
            "Bytes/equal",
            "Bytes/concat",
            "Bytes/to_text",
            "Bytes/to_hex",
            "Bytes/to_base64",
            "Bytes/from_hex",
            "Bytes/from_base64",
        ] {
            let mut tampered = plan.clone();
            let replaced = tampered
                .regions
                .iter_mut()
                .filter(|region| region.kind == RegionKind::DerivedEvaluation)
                .flat_map(|region| region.ops.iter_mut())
                .any(|op| match &mut op.kind {
                    PlanOpKind::DerivedValue {
                        expression: Some(PlanDerivedExpression::RowExpression { expression }),
                        ..
                    } => replace_first_row_bytes_builtin(expression, function),
                    _ => false,
                });
            assert!(
                replaced,
                "row fixture should contain a typed {function} expression"
            );
            let tampered_verification = verify_plan(&tampered).unwrap();
            assert_eq!(tampered_verification.status, "fail");
            assert!(
                tampered_verification
                    .checks
                    .iter()
                    .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
                "generic row {function} must not remain verifier-admissible: {tampered_verification:#?}"
            );
        }
    }

    #[test]
    fn cells_formula_byte_scan_offsets_lower_as_numeric_infix() {
        fn expression_contains_function(
            expression: &PlanRowExpression,
            function_name: &str,
        ) -> bool {
            match expression {
                PlanRowExpression::TextToBytes { input, encoding } => {
                    function_name == "Text/to_bytes"
                        || expression_contains_function(input, function_name)
                        || encoding.as_deref().is_some_and(|encoding| {
                            expression_contains_function(encoding, function_name)
                        })
                }
                PlanRowExpression::BytesToText { input, encoding } => {
                    function_name == "Bytes/to_text"
                        || expression_contains_function(input, function_name)
                        || encoding.as_deref().is_some_and(|encoding| {
                            expression_contains_function(encoding, function_name)
                        })
                }
                PlanRowExpression::BytesToHex { input } => {
                    function_name == "Bytes/to_hex"
                        || expression_contains_function(input, function_name)
                }
                PlanRowExpression::BytesToBase64 { input } => {
                    function_name == "Bytes/to_base64"
                        || expression_contains_function(input, function_name)
                }
                PlanRowExpression::BytesFromHex { input } => {
                    function_name == "Bytes/from_hex"
                        || expression_contains_function(input, function_name)
                }
                PlanRowExpression::BytesFromBase64 { input } => {
                    function_name == "Bytes/from_base64"
                        || expression_contains_function(input, function_name)
                }
                PlanRowExpression::BytesFind { input, needle } => {
                    function_name == "Bytes/find"
                        || expression_contains_function(input, function_name)
                        || expression_contains_function(needle, function_name)
                }
                PlanRowExpression::BytesStartsWith { input, prefix } => {
                    function_name == "Bytes/starts_with"
                        || expression_contains_function(input, function_name)
                        || expression_contains_function(prefix, function_name)
                }
                PlanRowExpression::BytesEndsWith { input, suffix } => {
                    function_name == "Bytes/ends_with"
                        || expression_contains_function(input, function_name)
                        || expression_contains_function(suffix, function_name)
                }
                PlanRowExpression::BytesIsEmpty { input } => {
                    function_name == "Bytes/is_empty"
                        || expression_contains_function(input, function_name)
                }
                PlanRowExpression::BytesConcat { left, right } => {
                    function_name == "Bytes/concat"
                        || expression_contains_function(left, function_name)
                        || expression_contains_function(right, function_name)
                }
                PlanRowExpression::BytesEqual { left, right } => {
                    function_name == "Bytes/equal"
                        || expression_contains_function(left, function_name)
                        || expression_contains_function(right, function_name)
                }
                PlanRowExpression::BuiltinCall {
                    function,
                    input,
                    args,
                } => {
                    function == function_name
                        || input
                            .as_deref()
                            .is_some_and(|input| expression_contains_function(input, function_name))
                        || args
                            .iter()
                            .any(|arg| expression_contains_function(&arg.value, function_name))
                }
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input } => {
                    expression_contains_function(input, function_name)
                }
                PlanRowExpression::Object { fields } => fields
                    .iter()
                    .any(|field| expression_contains_function(&field.value, function_name)),
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    expression_contains_function(input, function_name)
                        || expression_contains_function(prefix, function_name)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    expression_contains_function(input, function_name)
                        || expression_contains_function(start, function_name)
                        || expression_contains_function(length, function_name)
                }
                PlanRowExpression::BytesLength { input } => {
                    function_name == "Bytes/length"
                        || expression_contains_function(input, function_name)
                }
                PlanRowExpression::BytesGet { input, index } => {
                    function_name == "Bytes/get"
                        || expression_contains_function(input, function_name)
                        || expression_contains_function(index, function_name)
                }
                PlanRowExpression::BytesSlice {
                    input,
                    offset,
                    byte_count,
                } => {
                    expression_contains_function(input, function_name)
                        || expression_contains_function(offset, function_name)
                        || expression_contains_function(byte_count, function_name)
                }
                PlanRowExpression::BytesTake { input, byte_count }
                | PlanRowExpression::BytesDrop { input, byte_count } => {
                    expression_contains_function(input, function_name)
                        || expression_contains_function(byte_count, function_name)
                }
                PlanRowExpression::BytesZeros { byte_count } => {
                    expression_contains_function(byte_count, function_name)
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                }
                | PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    (matches!(
                        expression,
                        PlanRowExpression::BytesReadUnsigned { .. }
                            if function_name == "Bytes/read_unsigned"
                    ) || matches!(
                        expression,
                        PlanRowExpression::BytesReadSigned { .. }
                            if function_name == "Bytes/read_signed"
                    )) || expression_contains_function(input, function_name)
                        || expression_contains_function(offset, function_name)
                        || expression_contains_function(byte_count, function_name)
                        || expression_contains_function(endian, function_name)
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } => {
                    function_name == "Bytes/set"
                        || expression_contains_function(input, function_name)
                        || expression_contains_function(index, function_name)
                        || expression_contains_function(value, function_name)
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    function_name == "Bytes/write_unsigned"
                        || expression_contains_function(input, function_name)
                        || expression_contains_function(offset, function_name)
                        || expression_contains_function(byte_count, function_name)
                        || expression_contains_function(endian, function_name)
                        || expression_contains_function(value, function_name)
                }
                PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    function_name == "Bytes/write_signed"
                        || expression_contains_function(input, function_name)
                        || expression_contains_function(offset, function_name)
                        || expression_contains_function(byte_count, function_name)
                        || expression_contains_function(endian, function_name)
                        || expression_contains_function(value, function_name)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    expression_contains_function(left, function_name)
                        || expression_contains_function(right, function_name)
                }
                PlanRowExpression::TextConcat { parts } => parts
                    .iter()
                    .any(|part| expression_contains_function(part, function_name)),
                PlanRowExpression::ListGetField { index, .. } => {
                    expression_contains_function(index, function_name)
                }
                PlanRowExpression::ListFindValue {
                    value, fallback, ..
                } => {
                    expression_contains_function(value, function_name)
                        || fallback.as_deref().is_some_and(|fallback| {
                            expression_contains_function(fallback, function_name)
                        })
                }
                PlanRowExpression::ListRange { from, to } => {
                    expression_contains_function(from, function_name)
                        || expression_contains_function(to, function_name)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    expression_contains_function(input, function_name)
                        || expression_contains_function(value, function_name)
                }
                PlanRowExpression::Select { input, arms } => {
                    expression_contains_function(input, function_name)
                        || arms
                            .iter()
                            .any(|arm| expression_contains_function(&arm.value, function_name))
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        fn is_direct_bytes_find_result(expression: &PlanRowExpression) -> bool {
            match expression {
                PlanRowExpression::BytesFind { .. } => true,
                PlanRowExpression::BuiltinCall { function, .. } => function == "Bytes/find",
                _ => false,
            }
        }

        fn contains_direct_bytes_find_text_concat(expression: &PlanRowExpression) -> bool {
            match expression {
                PlanRowExpression::TextConcat { parts } => {
                    parts.iter().any(is_direct_bytes_find_result)
                }
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input } => {
                    contains_direct_bytes_find_text_concat(input)
                }
                PlanRowExpression::Object { fields } => fields
                    .iter()
                    .any(|field| contains_direct_bytes_find_text_concat(&field.value)),
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(prefix)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(start)
                        || contains_direct_bytes_find_text_concat(length)
                }
                PlanRowExpression::TextToBytes { input, encoding } => {
                    contains_direct_bytes_find_text_concat(input)
                        || encoding
                            .as_deref()
                            .is_some_and(contains_direct_bytes_find_text_concat)
                }
                PlanRowExpression::BytesToText { input, encoding } => {
                    contains_direct_bytes_find_text_concat(input)
                        || encoding
                            .as_deref()
                            .is_some_and(contains_direct_bytes_find_text_concat)
                }
                PlanRowExpression::BytesToHex { input }
                | PlanRowExpression::BytesToBase64 { input }
                | PlanRowExpression::BytesFromHex { input }
                | PlanRowExpression::BytesFromBase64 { input } => {
                    contains_direct_bytes_find_text_concat(input)
                }
                PlanRowExpression::BytesLength { input } => {
                    contains_direct_bytes_find_text_concat(input)
                }
                PlanRowExpression::BytesGet { input, index } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(index)
                }
                PlanRowExpression::BytesSlice {
                    input,
                    offset,
                    byte_count,
                } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(offset)
                        || contains_direct_bytes_find_text_concat(byte_count)
                }
                PlanRowExpression::BytesTake { input, byte_count }
                | PlanRowExpression::BytesDrop { input, byte_count } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(byte_count)
                }
                PlanRowExpression::BytesZeros { byte_count } => {
                    contains_direct_bytes_find_text_concat(byte_count)
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                }
                | PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(offset)
                        || contains_direct_bytes_find_text_concat(byte_count)
                        || contains_direct_bytes_find_text_concat(endian)
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(index)
                        || contains_direct_bytes_find_text_concat(value)
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                }
                | PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(offset)
                        || contains_direct_bytes_find_text_concat(byte_count)
                        || contains_direct_bytes_find_text_concat(endian)
                        || contains_direct_bytes_find_text_concat(value)
                }
                PlanRowExpression::BytesFind { input, needle } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(needle)
                }
                PlanRowExpression::BytesStartsWith { input, prefix } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(prefix)
                }
                PlanRowExpression::BytesEndsWith { input, suffix } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(suffix)
                }
                PlanRowExpression::BytesIsEmpty { input } => {
                    contains_direct_bytes_find_text_concat(input)
                }
                PlanRowExpression::BytesConcat { left, right } => {
                    contains_direct_bytes_find_text_concat(left)
                        || contains_direct_bytes_find_text_concat(right)
                }
                PlanRowExpression::BytesEqual { left, right } => {
                    contains_direct_bytes_find_text_concat(left)
                        || contains_direct_bytes_find_text_concat(right)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    contains_direct_bytes_find_text_concat(left)
                        || contains_direct_bytes_find_text_concat(right)
                }
                PlanRowExpression::ListGetField { index, .. } => {
                    contains_direct_bytes_find_text_concat(index)
                }
                PlanRowExpression::ListFindValue {
                    value, fallback, ..
                } => {
                    contains_direct_bytes_find_text_concat(value)
                        || fallback
                            .as_deref()
                            .is_some_and(contains_direct_bytes_find_text_concat)
                }
                PlanRowExpression::ListRange { from, to } => {
                    contains_direct_bytes_find_text_concat(from)
                        || contains_direct_bytes_find_text_concat(to)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    contains_direct_bytes_find_text_concat(input)
                        || contains_direct_bytes_find_text_concat(value)
                }
                PlanRowExpression::BuiltinCall { input, args, .. } => {
                    input
                        .as_deref()
                        .is_some_and(contains_direct_bytes_find_text_concat)
                        || args
                            .iter()
                            .any(|arg| contains_direct_bytes_find_text_concat(&arg.value))
                }
                PlanRowExpression::Select { input, arms } => {
                    contains_direct_bytes_find_text_concat(input)
                        || arms
                            .iter()
                            .any(|arm| contains_direct_bytes_find_text_concat(&arm.value))
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        fn contains_bytes_find_numeric_plus(expression: &PlanRowExpression) -> bool {
            match expression {
                PlanRowExpression::NumberInfix { op, left, right } if op == "+" => {
                    expression_contains_function(left, "Bytes/find")
                        || expression_contains_function(right, "Bytes/find")
                        || contains_bytes_find_numeric_plus(left)
                        || contains_bytes_find_numeric_plus(right)
                }
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input } => contains_bytes_find_numeric_plus(input),
                PlanRowExpression::Object { fields } => fields
                    .iter()
                    .any(|field| contains_bytes_find_numeric_plus(&field.value)),
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(prefix)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(start)
                        || contains_bytes_find_numeric_plus(length)
                }
                PlanRowExpression::TextToBytes { input, encoding } => {
                    contains_bytes_find_numeric_plus(input)
                        || encoding
                            .as_deref()
                            .is_some_and(contains_bytes_find_numeric_plus)
                }
                PlanRowExpression::BytesToText { input, encoding } => {
                    contains_bytes_find_numeric_plus(input)
                        || encoding
                            .as_deref()
                            .is_some_and(contains_bytes_find_numeric_plus)
                }
                PlanRowExpression::BytesToHex { input }
                | PlanRowExpression::BytesToBase64 { input }
                | PlanRowExpression::BytesFromHex { input }
                | PlanRowExpression::BytesFromBase64 { input } => {
                    contains_bytes_find_numeric_plus(input)
                }
                PlanRowExpression::BytesLength { input } => contains_bytes_find_numeric_plus(input),
                PlanRowExpression::BytesGet { input, index } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(index)
                }
                PlanRowExpression::BytesSlice {
                    input,
                    offset,
                    byte_count,
                } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(offset)
                        || contains_bytes_find_numeric_plus(byte_count)
                }
                PlanRowExpression::BytesTake { input, byte_count }
                | PlanRowExpression::BytesDrop { input, byte_count } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(byte_count)
                }
                PlanRowExpression::BytesZeros { byte_count } => {
                    contains_bytes_find_numeric_plus(byte_count)
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                }
                | PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(offset)
                        || contains_bytes_find_numeric_plus(byte_count)
                        || contains_bytes_find_numeric_plus(endian)
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(index)
                        || contains_bytes_find_numeric_plus(value)
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                }
                | PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(offset)
                        || contains_bytes_find_numeric_plus(byte_count)
                        || contains_bytes_find_numeric_plus(endian)
                        || contains_bytes_find_numeric_plus(value)
                }
                PlanRowExpression::BytesFind { input, needle } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(needle)
                }
                PlanRowExpression::BytesStartsWith { input, prefix } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(prefix)
                }
                PlanRowExpression::BytesEndsWith { input, suffix } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(suffix)
                }
                PlanRowExpression::BytesIsEmpty { input } => {
                    contains_bytes_find_numeric_plus(input)
                }
                PlanRowExpression::BytesConcat { left, right } => {
                    contains_bytes_find_numeric_plus(left)
                        || contains_bytes_find_numeric_plus(right)
                }
                PlanRowExpression::BytesEqual { left, right } => {
                    contains_bytes_find_numeric_plus(left)
                        || contains_bytes_find_numeric_plus(right)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    contains_bytes_find_numeric_plus(left)
                        || contains_bytes_find_numeric_plus(right)
                }
                PlanRowExpression::TextConcat { parts } => {
                    parts.iter().any(contains_bytes_find_numeric_plus)
                }
                PlanRowExpression::ListGetField { index, .. } => {
                    contains_bytes_find_numeric_plus(index)
                }
                PlanRowExpression::ListFindValue {
                    value, fallback, ..
                } => {
                    contains_bytes_find_numeric_plus(value)
                        || fallback
                            .as_deref()
                            .is_some_and(contains_bytes_find_numeric_plus)
                }
                PlanRowExpression::ListRange { from, to } => {
                    contains_bytes_find_numeric_plus(from) || contains_bytes_find_numeric_plus(to)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    contains_bytes_find_numeric_plus(input)
                        || contains_bytes_find_numeric_plus(value)
                }
                PlanRowExpression::BuiltinCall { input, args, .. } => {
                    input
                        .as_deref()
                        .is_some_and(contains_bytes_find_numeric_plus)
                        || args
                            .iter()
                            .any(|arg| contains_bytes_find_numeric_plus(&arg.value))
                }
                PlanRowExpression::Select { input, arms } => {
                    contains_bytes_find_numeric_plus(input)
                        || arms
                            .iter()
                            .any(|arm| contains_bytes_find_numeric_plus(&arm.value))
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        fn contains_typed_bytes_slice(expression: &PlanRowExpression) -> bool {
            match expression {
                PlanRowExpression::BytesSlice {
                    input, byte_count, ..
                } => {
                    matches!(byte_count.as_ref(), PlanRowExpression::NumberInfix { .. })
                        || contains_typed_bytes_slice(input)
                        || contains_typed_bytes_slice(byte_count)
                }
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input } => contains_typed_bytes_slice(input),
                PlanRowExpression::Object { fields } => fields
                    .iter()
                    .any(|field| contains_typed_bytes_slice(&field.value)),
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    contains_typed_bytes_slice(input) || contains_typed_bytes_slice(prefix)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    contains_typed_bytes_slice(input)
                        || contains_typed_bytes_slice(start)
                        || contains_typed_bytes_slice(length)
                }
                PlanRowExpression::TextToBytes { input, encoding } => {
                    contains_typed_bytes_slice(input)
                        || encoding.as_deref().is_some_and(contains_typed_bytes_slice)
                }
                PlanRowExpression::BytesToText { input, encoding } => {
                    contains_typed_bytes_slice(input)
                        || encoding.as_deref().is_some_and(contains_typed_bytes_slice)
                }
                PlanRowExpression::BytesToHex { input }
                | PlanRowExpression::BytesToBase64 { input }
                | PlanRowExpression::BytesFromHex { input }
                | PlanRowExpression::BytesFromBase64 { input } => contains_typed_bytes_slice(input),
                PlanRowExpression::BytesLength { input } => contains_typed_bytes_slice(input),
                PlanRowExpression::BytesGet { input, index } => {
                    contains_typed_bytes_slice(input) || contains_typed_bytes_slice(index)
                }
                PlanRowExpression::BytesTake { input, byte_count }
                | PlanRowExpression::BytesDrop { input, byte_count } => {
                    contains_typed_bytes_slice(input) || contains_typed_bytes_slice(byte_count)
                }
                PlanRowExpression::BytesZeros { byte_count } => {
                    contains_typed_bytes_slice(byte_count)
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                }
                | PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    contains_typed_bytes_slice(input)
                        || contains_typed_bytes_slice(offset)
                        || contains_typed_bytes_slice(byte_count)
                        || contains_typed_bytes_slice(endian)
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } => {
                    contains_typed_bytes_slice(input)
                        || contains_typed_bytes_slice(index)
                        || contains_typed_bytes_slice(value)
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                }
                | PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    contains_typed_bytes_slice(input)
                        || contains_typed_bytes_slice(offset)
                        || contains_typed_bytes_slice(byte_count)
                        || contains_typed_bytes_slice(endian)
                        || contains_typed_bytes_slice(value)
                }
                PlanRowExpression::BytesFind { input, needle } => {
                    contains_typed_bytes_slice(input) || contains_typed_bytes_slice(needle)
                }
                PlanRowExpression::BytesStartsWith { input, prefix } => {
                    contains_typed_bytes_slice(input) || contains_typed_bytes_slice(prefix)
                }
                PlanRowExpression::BytesEndsWith { input, suffix } => {
                    contains_typed_bytes_slice(input) || contains_typed_bytes_slice(suffix)
                }
                PlanRowExpression::BytesIsEmpty { input } => contains_typed_bytes_slice(input),
                PlanRowExpression::BytesConcat { left, right } => {
                    contains_typed_bytes_slice(left) || contains_typed_bytes_slice(right)
                }
                PlanRowExpression::BytesEqual { left, right } => {
                    contains_typed_bytes_slice(left) || contains_typed_bytes_slice(right)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    contains_typed_bytes_slice(left) || contains_typed_bytes_slice(right)
                }
                PlanRowExpression::TextConcat { parts } => {
                    parts.iter().any(contains_typed_bytes_slice)
                }
                PlanRowExpression::ListGetField { index, .. } => contains_typed_bytes_slice(index),
                PlanRowExpression::ListFindValue {
                    value, fallback, ..
                } => {
                    contains_typed_bytes_slice(value)
                        || fallback.as_deref().is_some_and(contains_typed_bytes_slice)
                }
                PlanRowExpression::ListRange { from, to } => {
                    contains_typed_bytes_slice(from) || contains_typed_bytes_slice(to)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    contains_typed_bytes_slice(input) || contains_typed_bytes_slice(value)
                }
                PlanRowExpression::BuiltinCall { input, args, .. } => {
                    input.as_deref().is_some_and(contains_typed_bytes_slice)
                        || args
                            .iter()
                            .any(|arg| contains_typed_bytes_slice(&arg.value))
                }
                PlanRowExpression::Select { input, arms } => {
                    contains_typed_bytes_slice(input)
                        || arms
                            .iter()
                            .any(|arm| contains_typed_bytes_slice(&arm.value))
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        fn contains_typed_byte_scanner_ops(expression: &PlanRowExpression) -> bool {
            match expression {
                PlanRowExpression::TextToBytes { .. }
                | PlanRowExpression::BytesToText { .. }
                | PlanRowExpression::BytesToHex { .. }
                | PlanRowExpression::BytesToBase64 { .. }
                | PlanRowExpression::BytesFromHex { .. }
                | PlanRowExpression::BytesFromBase64 { .. }
                | PlanRowExpression::BytesFind { .. }
                | PlanRowExpression::BytesStartsWith { .. }
                | PlanRowExpression::BytesEndsWith { .. }
                | PlanRowExpression::BytesIsEmpty { .. }
                | PlanRowExpression::BytesConcat { .. }
                | PlanRowExpression::BytesEqual { .. }
                | PlanRowExpression::BytesTake { .. }
                | PlanRowExpression::BytesDrop { .. }
                | PlanRowExpression::BytesZeros { .. }
                | PlanRowExpression::BytesReadUnsigned { .. }
                | PlanRowExpression::BytesReadSigned { .. }
                | PlanRowExpression::BytesSet { .. }
                | PlanRowExpression::BytesWriteUnsigned { .. }
                | PlanRowExpression::BytesWriteSigned { .. } => true,
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input } => contains_typed_byte_scanner_ops(input),
                PlanRowExpression::Object { fields } => fields
                    .iter()
                    .any(|field| contains_typed_byte_scanner_ops(&field.value)),
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    contains_typed_byte_scanner_ops(input)
                        || contains_typed_byte_scanner_ops(prefix)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    contains_typed_byte_scanner_ops(input)
                        || contains_typed_byte_scanner_ops(start)
                        || contains_typed_byte_scanner_ops(length)
                }
                PlanRowExpression::BytesSlice {
                    input,
                    offset,
                    byte_count,
                } => {
                    contains_typed_byte_scanner_ops(input)
                        || contains_typed_byte_scanner_ops(offset)
                        || contains_typed_byte_scanner_ops(byte_count)
                }
                PlanRowExpression::BytesLength { input } => contains_typed_byte_scanner_ops(input),
                PlanRowExpression::BytesGet { input, index } => {
                    contains_typed_byte_scanner_ops(input) || contains_typed_byte_scanner_ops(index)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    contains_typed_byte_scanner_ops(left) || contains_typed_byte_scanner_ops(right)
                }
                PlanRowExpression::TextConcat { parts } => {
                    parts.iter().any(contains_typed_byte_scanner_ops)
                }
                PlanRowExpression::ListGetField { index, .. } => {
                    contains_typed_byte_scanner_ops(index)
                }
                PlanRowExpression::ListFindValue {
                    value, fallback, ..
                } => {
                    contains_typed_byte_scanner_ops(value)
                        || fallback
                            .as_deref()
                            .is_some_and(contains_typed_byte_scanner_ops)
                }
                PlanRowExpression::ListRange { from, to } => {
                    contains_typed_byte_scanner_ops(from) || contains_typed_byte_scanner_ops(to)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    contains_typed_byte_scanner_ops(input) || contains_typed_byte_scanner_ops(value)
                }
                PlanRowExpression::BuiltinCall { input, args, .. } => {
                    input
                        .as_deref()
                        .is_some_and(contains_typed_byte_scanner_ops)
                        || args
                            .iter()
                            .any(|arg| contains_typed_byte_scanner_ops(&arg.value))
                }
                PlanRowExpression::Select { input, arms } => {
                    contains_typed_byte_scanner_ops(input)
                        || arms
                            .iter()
                            .any(|arm| contains_typed_byte_scanner_ops(&arm.value))
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        fn matches_generic_row_builtin_function(
            expression: &PlanRowExpression,
            function_name: &str,
        ) -> bool {
            match expression {
                PlanRowExpression::BuiltinCall {
                    function,
                    input,
                    args,
                } => {
                    function == function_name
                        || input.as_deref().is_some_and(|input| {
                            matches_generic_row_builtin_function(input, function_name)
                        })
                        || args.iter().any(|arg| {
                            matches_generic_row_builtin_function(&arg.value, function_name)
                        })
                }
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input } => {
                    matches_generic_row_builtin_function(input, function_name)
                }
                PlanRowExpression::Object { fields } => fields
                    .iter()
                    .any(|field| matches_generic_row_builtin_function(&field.value, function_name)),
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(prefix, function_name)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(start, function_name)
                        || matches_generic_row_builtin_function(length, function_name)
                }
                PlanRowExpression::TextToBytes { input, encoding } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || encoding.as_deref().is_some_and(|encoding| {
                            matches_generic_row_builtin_function(encoding, function_name)
                        })
                }
                PlanRowExpression::BytesToText { input, encoding } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || encoding.as_deref().is_some_and(|encoding| {
                            matches_generic_row_builtin_function(encoding, function_name)
                        })
                }
                PlanRowExpression::BytesToHex { input }
                | PlanRowExpression::BytesToBase64 { input }
                | PlanRowExpression::BytesFromHex { input }
                | PlanRowExpression::BytesFromBase64 { input } => {
                    matches_generic_row_builtin_function(input, function_name)
                }
                PlanRowExpression::BytesLength { input } => {
                    matches_generic_row_builtin_function(input, function_name)
                }
                PlanRowExpression::BytesGet { input, index } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(index, function_name)
                }
                PlanRowExpression::BytesSlice {
                    input,
                    offset,
                    byte_count,
                } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(offset, function_name)
                        || matches_generic_row_builtin_function(byte_count, function_name)
                }
                PlanRowExpression::BytesTake { input, byte_count }
                | PlanRowExpression::BytesDrop { input, byte_count } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(byte_count, function_name)
                }
                PlanRowExpression::BytesZeros { byte_count } => {
                    matches_generic_row_builtin_function(byte_count, function_name)
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                }
                | PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(offset, function_name)
                        || matches_generic_row_builtin_function(byte_count, function_name)
                        || matches_generic_row_builtin_function(endian, function_name)
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(index, function_name)
                        || matches_generic_row_builtin_function(value, function_name)
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                }
                | PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(offset, function_name)
                        || matches_generic_row_builtin_function(byte_count, function_name)
                        || matches_generic_row_builtin_function(endian, function_name)
                        || matches_generic_row_builtin_function(value, function_name)
                }
                PlanRowExpression::BytesFind { input, needle } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(needle, function_name)
                }
                PlanRowExpression::BytesStartsWith { input, prefix } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(prefix, function_name)
                }
                PlanRowExpression::BytesEndsWith { input, suffix } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(suffix, function_name)
                }
                PlanRowExpression::BytesIsEmpty { input } => {
                    matches_generic_row_builtin_function(input, function_name)
                }
                PlanRowExpression::BytesConcat { left, right } => {
                    matches_generic_row_builtin_function(left, function_name)
                        || matches_generic_row_builtin_function(right, function_name)
                }
                PlanRowExpression::BytesEqual { left, right } => {
                    matches_generic_row_builtin_function(left, function_name)
                        || matches_generic_row_builtin_function(right, function_name)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    matches_generic_row_builtin_function(left, function_name)
                        || matches_generic_row_builtin_function(right, function_name)
                }
                PlanRowExpression::TextConcat { parts } => parts
                    .iter()
                    .any(|part| matches_generic_row_builtin_function(part, function_name)),
                PlanRowExpression::ListGetField { index, .. } => {
                    matches_generic_row_builtin_function(index, function_name)
                }
                PlanRowExpression::ListFindValue {
                    value, fallback, ..
                } => {
                    matches_generic_row_builtin_function(value, function_name)
                        || fallback.as_deref().is_some_and(|fallback| {
                            matches_generic_row_builtin_function(fallback, function_name)
                        })
                }
                PlanRowExpression::ListRange { from, to } => {
                    matches_generic_row_builtin_function(from, function_name)
                        || matches_generic_row_builtin_function(to, function_name)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || matches_generic_row_builtin_function(value, function_name)
                }
                PlanRowExpression::Select { input, arms } => {
                    matches_generic_row_builtin_function(input, function_name)
                        || arms.iter().any(|arm| {
                            matches_generic_row_builtin_function(&arm.value, function_name)
                        })
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        fn replace_first_typed_byte_scanner_with_generic(
            expression: &mut PlanRowExpression,
        ) -> bool {
            match expression {
                PlanRowExpression::TextToBytes { input, encoding } => {
                    let input = (**input).clone();
                    let args = encoding
                        .as_deref()
                        .map(|encoding| PlanRowCallArg {
                            name: Some("encoding".to_owned()),
                            value: encoding.clone(),
                        })
                        .into_iter()
                        .collect();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Text/to_bytes".to_owned(),
                        input: Some(Box::new(input)),
                        args,
                    };
                    true
                }
                PlanRowExpression::BytesToText { input, encoding } => {
                    let input = (**input).clone();
                    let args = encoding
                        .as_deref()
                        .map(|encoding| PlanRowCallArg {
                            name: Some("encoding".to_owned()),
                            value: encoding.clone(),
                        })
                        .into_iter()
                        .collect();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/to_text".to_owned(),
                        input: Some(Box::new(input)),
                        args,
                    };
                    true
                }
                PlanRowExpression::BytesToHex { input } => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/to_hex".to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesToBase64 { input } => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/to_base64".to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesFromHex { input } => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/from_hex".to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesFromBase64 { input } => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/from_base64".to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesSlice {
                    input,
                    offset,
                    byte_count,
                } => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/slice".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesTake { input, byte_count } => {
                    let input = (**input).clone();
                    let byte_count = (**byte_count).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/take".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesDrop { input, byte_count } => {
                    let input = (**input).clone();
                    let byte_count = (**byte_count).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/drop".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesZeros { byte_count } => {
                    let byte_count = (**byte_count).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/zeros".to_owned(),
                        input: None,
                        args: vec![PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    let endian = (**endian).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/read_unsigned".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                            PlanRowCallArg {
                                name: Some("endian".to_owned()),
                                value: endian,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    let endian = (**endian).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/read_signed".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                            PlanRowCallArg {
                                name: Some("endian".to_owned()),
                                value: endian,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } => {
                    let input = (**input).clone();
                    let index = (**index).clone();
                    let value = (**value).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/set".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("index".to_owned()),
                                value: index,
                            },
                            PlanRowCallArg {
                                name: Some("value".to_owned()),
                                value,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    let endian = (**endian).clone();
                    let value = (**value).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/write_unsigned".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                            PlanRowCallArg {
                                name: Some("endian".to_owned()),
                                value: endian,
                            },
                            PlanRowCallArg {
                                name: Some("value".to_owned()),
                                value,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    let input = (**input).clone();
                    let offset = (**offset).clone();
                    let byte_count = (**byte_count).clone();
                    let endian = (**endian).clone();
                    let value = (**value).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/write_signed".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![
                            PlanRowCallArg {
                                name: Some("offset".to_owned()),
                                value: offset,
                            },
                            PlanRowCallArg {
                                name: Some("byte_count".to_owned()),
                                value: byte_count,
                            },
                            PlanRowCallArg {
                                name: Some("endian".to_owned()),
                                value: endian,
                            },
                            PlanRowCallArg {
                                name: Some("value".to_owned()),
                                value,
                            },
                        ],
                    };
                    true
                }
                PlanRowExpression::BytesFind { input, needle } => {
                    let input = (**input).clone();
                    let needle = (**needle).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/find".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("needle".to_owned()),
                            value: needle,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesStartsWith { input, prefix } => {
                    let input = (**input).clone();
                    let prefix = (**prefix).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/starts_with".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("prefix".to_owned()),
                            value: prefix,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesEndsWith { input, suffix } => {
                    let input = (**input).clone();
                    let suffix = (**suffix).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/ends_with".to_owned(),
                        input: Some(Box::new(input)),
                        args: vec![PlanRowCallArg {
                            name: Some("suffix".to_owned()),
                            value: suffix,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesIsEmpty { input } => {
                    let input = (**input).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/is_empty".to_owned(),
                        input: Some(Box::new(input)),
                        args: Vec::new(),
                    };
                    true
                }
                PlanRowExpression::BytesConcat { left, right } => {
                    let left = (**left).clone();
                    let right = (**right).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/concat".to_owned(),
                        input: Some(Box::new(left)),
                        args: vec![PlanRowCallArg {
                            name: Some("with".to_owned()),
                            value: right,
                        }],
                    };
                    true
                }
                PlanRowExpression::BytesEqual { left, right } => {
                    let left = (**left).clone();
                    let right = (**right).clone();
                    *expression = PlanRowExpression::BuiltinCall {
                        function: "Bytes/equal".to_owned(),
                        input: Some(Box::new(left)),
                        args: vec![PlanRowCallArg {
                            name: Some("with".to_owned()),
                            value: right,
                        }],
                    };
                    true
                }
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input } => {
                    replace_first_typed_byte_scanner_with_generic(input)
                }
                PlanRowExpression::Object { fields } => fields
                    .iter_mut()
                    .any(|field| replace_first_typed_byte_scanner_with_generic(&mut field.value)),
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    replace_first_typed_byte_scanner_with_generic(input)
                        || replace_first_typed_byte_scanner_with_generic(prefix)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    replace_first_typed_byte_scanner_with_generic(input)
                        || replace_first_typed_byte_scanner_with_generic(start)
                        || replace_first_typed_byte_scanner_with_generic(length)
                }
                PlanRowExpression::BytesLength { input } => {
                    replace_first_typed_byte_scanner_with_generic(input)
                }
                PlanRowExpression::BytesGet { input, index } => {
                    replace_first_typed_byte_scanner_with_generic(input)
                        || replace_first_typed_byte_scanner_with_generic(index)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    replace_first_typed_byte_scanner_with_generic(left)
                        || replace_first_typed_byte_scanner_with_generic(right)
                }
                PlanRowExpression::TextConcat { parts } => parts
                    .iter_mut()
                    .any(replace_first_typed_byte_scanner_with_generic),
                PlanRowExpression::ListGetField { index, .. } => {
                    replace_first_typed_byte_scanner_with_generic(index)
                }
                PlanRowExpression::ListFindValue {
                    value, fallback, ..
                } => {
                    replace_first_typed_byte_scanner_with_generic(value)
                        || fallback
                            .as_deref_mut()
                            .is_some_and(replace_first_typed_byte_scanner_with_generic)
                }
                PlanRowExpression::ListRange { from, to } => {
                    replace_first_typed_byte_scanner_with_generic(from)
                        || replace_first_typed_byte_scanner_with_generic(to)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    replace_first_typed_byte_scanner_with_generic(input)
                        || replace_first_typed_byte_scanner_with_generic(value)
                }
                PlanRowExpression::BuiltinCall { input, args, .. } => {
                    input
                        .as_deref_mut()
                        .is_some_and(replace_first_typed_byte_scanner_with_generic)
                        || args.iter_mut().any(|arg| {
                            replace_first_typed_byte_scanner_with_generic(&mut arg.value)
                        })
                }
                PlanRowExpression::Select { input, arms } => {
                    replace_first_typed_byte_scanner_with_generic(input)
                        || arms.iter_mut().any(|arm| {
                            replace_first_typed_byte_scanner_with_generic(&mut arm.value)
                        })
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        let parsed = parse_cells_project_for_plan_test();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let value_id = debug_entry_id(&plan.debug_map.fields, "field", "cell.value");
        let value_expression = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .find_map(|op| match &op.kind {
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::RowExpression { expression }),
                    ..
                } if matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == value_id) => {
                    Some(expression)
                }
                _ => None,
            })
            .expect("cell.value should lower to a row expression");

        assert!(
            contains_bytes_find_numeric_plus(value_expression),
            "Cells formula parser offsets such as `index + 1` should lower as numeric infix"
        );
        assert!(
            !contains_direct_bytes_find_text_concat(value_expression),
            "Bytes/find parser offsets must not lower through text concatenation"
        );
        assert!(
            contains_typed_bytes_slice(value_expression),
            "Cells formula byte scanning should lower Bytes/slice as a typed row expression with a dynamic numeric byte_count"
        );
        assert!(
            !expression_contains_function(value_expression, "Bytes/slice"),
            "Cells formula byte scanning must not leave Bytes/slice as a generic row builtin call"
        );
        assert!(
            contains_typed_byte_scanner_ops(value_expression),
            "Cells formula byte scanning should lower text-to-bytes and byte scanner calls as typed row expressions"
        );
        for function in ["Text/to_bytes", "Bytes/find", "Bytes/starts_with"] {
            assert!(
                !matches_generic_row_builtin_function(value_expression, function),
                "Cells formula byte scanning must not leave {function} as a generic row builtin call"
            );
        }

        let mut tampered = plan.clone();
        let replaced = tampered
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter_mut())
            .any(|op| match &mut op.kind {
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::RowExpression { expression }),
                    ..
                } if matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == value_id) => {
                    replace_first_typed_byte_scanner_with_generic(expression)
                }
                _ => false,
            });
        assert!(
            replaced,
            "Cells should contain a typed row byte scanner to tamper"
        );
        let tampered_verification = verify_plan(&tampered).unwrap();
        assert_eq!(tampered_verification.status, "fail");
        assert!(
            tampered_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
            "generic row byte scanner BuiltinCall must not remain verifier-admissible: {tampered_verification:#?}"
        );
    }

    #[test]
    fn cells_row_lookup_field_ids_must_belong_to_referenced_list() {
        fn tamper_first_row_lookup(expression: &mut PlanRowExpression, invalid: FieldId) -> bool {
            match expression {
                PlanRowExpression::ListFindValue { target, .. } => {
                    *target = invalid;
                    true
                }
                PlanRowExpression::ListGetField { field, .. } => {
                    *field = invalid;
                    true
                }
                PlanRowExpression::TextTrim { input }
                | PlanRowExpression::TextIsEmpty { input }
                | PlanRowExpression::TextLength { input }
                | PlanRowExpression::TextToNumber { input }
                | PlanRowExpression::ObjectField { object: input, .. }
                | PlanRowExpression::ListSum { input } => tamper_first_row_lookup(input, invalid),
                PlanRowExpression::Object { fields } => fields
                    .iter_mut()
                    .any(|field| tamper_first_row_lookup(&mut field.value, invalid)),
                PlanRowExpression::TextStartsWith { input, prefix } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(prefix, invalid)
                }
                PlanRowExpression::TextSubstring {
                    input,
                    start,
                    length,
                } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(start, invalid)
                        || tamper_first_row_lookup(length, invalid)
                }
                PlanRowExpression::TextToBytes { input, encoding } => {
                    tamper_first_row_lookup(input, invalid)
                        || encoding
                            .as_deref_mut()
                            .is_some_and(|encoding| tamper_first_row_lookup(encoding, invalid))
                }
                PlanRowExpression::BytesToText { input, encoding } => {
                    tamper_first_row_lookup(input, invalid)
                        || encoding
                            .as_deref_mut()
                            .is_some_and(|encoding| tamper_first_row_lookup(encoding, invalid))
                }
                PlanRowExpression::BytesToHex { input }
                | PlanRowExpression::BytesToBase64 { input }
                | PlanRowExpression::BytesFromHex { input }
                | PlanRowExpression::BytesFromBase64 { input } => {
                    tamper_first_row_lookup(input, invalid)
                }
                PlanRowExpression::BytesLength { input } => tamper_first_row_lookup(input, invalid),
                PlanRowExpression::BytesGet { input, index } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(index, invalid)
                }
                PlanRowExpression::BytesSlice {
                    input,
                    offset,
                    byte_count,
                } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(offset, invalid)
                        || tamper_first_row_lookup(byte_count, invalid)
                }
                PlanRowExpression::BytesTake { input, byte_count }
                | PlanRowExpression::BytesDrop { input, byte_count } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(byte_count, invalid)
                }
                PlanRowExpression::BytesZeros { byte_count } => {
                    tamper_first_row_lookup(byte_count, invalid)
                }
                PlanRowExpression::BytesReadUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                }
                | PlanRowExpression::BytesReadSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(offset, invalid)
                        || tamper_first_row_lookup(byte_count, invalid)
                        || tamper_first_row_lookup(endian, invalid)
                }
                PlanRowExpression::BytesSet {
                    input,
                    index,
                    value,
                } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(index, invalid)
                        || tamper_first_row_lookup(value, invalid)
                }
                PlanRowExpression::BytesWriteUnsigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                }
                | PlanRowExpression::BytesWriteSigned {
                    input,
                    offset,
                    byte_count,
                    endian,
                    value,
                } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(offset, invalid)
                        || tamper_first_row_lookup(byte_count, invalid)
                        || tamper_first_row_lookup(endian, invalid)
                        || tamper_first_row_lookup(value, invalid)
                }
                PlanRowExpression::BytesFind { input, needle } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(needle, invalid)
                }
                PlanRowExpression::BytesStartsWith { input, prefix } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(prefix, invalid)
                }
                PlanRowExpression::BytesEndsWith { input, suffix } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(suffix, invalid)
                }
                PlanRowExpression::BytesIsEmpty { input } => {
                    tamper_first_row_lookup(input, invalid)
                }
                PlanRowExpression::BytesConcat { left, right } => {
                    tamper_first_row_lookup(left, invalid)
                        || tamper_first_row_lookup(right, invalid)
                }
                PlanRowExpression::BytesEqual { left, right } => {
                    tamper_first_row_lookup(left, invalid)
                        || tamper_first_row_lookup(right, invalid)
                }
                PlanRowExpression::NumberInfix { left, right, .. } => {
                    tamper_first_row_lookup(left, invalid)
                        || tamper_first_row_lookup(right, invalid)
                }
                PlanRowExpression::TextConcat { parts } => parts
                    .iter_mut()
                    .any(|part| tamper_first_row_lookup(part, invalid)),
                PlanRowExpression::ListRange { from, to } => {
                    tamper_first_row_lookup(from, invalid) || tamper_first_row_lookup(to, invalid)
                }
                PlanRowExpression::ListMap { input, value, .. } => {
                    tamper_first_row_lookup(input, invalid)
                        || tamper_first_row_lookup(value, invalid)
                }
                PlanRowExpression::BuiltinCall { input, args, .. } => {
                    input
                        .as_deref_mut()
                        .is_some_and(|input| tamper_first_row_lookup(input, invalid))
                        || args
                            .iter_mut()
                            .any(|arg| tamper_first_row_lookup(&mut arg.value, invalid))
                }
                PlanRowExpression::Select { input, arms } => {
                    tamper_first_row_lookup(input, invalid)
                        || arms
                            .iter_mut()
                            .any(|arm| tamper_first_row_lookup(&mut arm.value, invalid))
                }
                PlanRowExpression::Field { .. }
                | PlanRowExpression::Constant { .. }
                | PlanRowExpression::ListRef { .. }
                | PlanRowExpression::ListMapItem { .. } => false,
            }
        }

        let parsed = parse_cells_project_for_plan_test();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let verification = verify_plan(&plan).unwrap();
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "row-expression-list-fields-resolve" && check.pass),
            "fresh Cells row lookup field ids should verify: {verification:#?}"
        );

        let invalid = FieldId(usize::MAX - 1);
        let tampered = plan
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter_mut())
            .any(|op| match &mut op.kind {
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::RowExpression { expression }),
                    ..
                } => tamper_first_row_lookup(expression, invalid),
                _ => false,
            });
        assert!(tampered, "Cells should contain a row lookup expression");

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "row-expression-list-fields-resolve" && !check.pass),
            "tampered row lookup field id should fail membership verification: {verification:#?}"
        );
    }

    #[test]
    fn cells_row_initial_fields_get_concrete_storage_types() {
        let parsed = parse_cells_project_for_plan_test();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        for label in ["cell.editing_text", "cell.formula_text"] {
            let state_id = StateId(debug_entry_id(&plan.debug_map.state_slots, "state", label));
            let storage_type =
                plan_value_type_for_state_slots(&plan.storage_layout.scalar_slots, state_id);
            assert_eq!(
                storage_type,
                Some(&PlanValueType::Text),
                "{label} should keep row-initial explainability but execute as TEXT"
            );
            let initial_kind = plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.state_id == state_id)
                .map(|slot| slot.initial_value_kind);
            assert_eq!(initial_kind, Some(InitialValueKind::RowInitialField));
        }

        let verification = verify_plan(&plan).unwrap();
        assert!(
            verification.checks.iter().any(|check| check.id
                == "constant-refs-resolve-and-match-storage-types"
                && check.pass),
            "Cells row-initial SourcePayload(Text) writes should verify with concrete storage types: {verification:#?}"
        );
        assert_eq!(verification.status, "pass");
    }

    #[test]
    fn indexed_bytes_row_initial_fields_get_concrete_storage_types() {
        let parsed = boon_parser::parse_source(
            "examples/bytes_indexed_source_payload_plan_ops.bn",
            include_str!("../../../examples/bytes_indexed_source_payload_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        let payload_id = StateId(debug_entry_id(
            &plan.debug_map.state_slots,
            "state",
            "row.payload",
        ));
        let payload_slot = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == payload_id)
            .expect("row.payload slot should exist");
        assert!(payload_slot.indexed);
        assert_eq!(
            payload_slot.value_type,
            PlanValueType::Bytes { fixed_len: Some(3) }
        );
        assert_eq!(
            payload_slot.initial_value_kind,
            InitialValueKind::RowInitialField
        );
        let payload_bank = plan
            .storage_layout
            .byte_banks
            .iter()
            .find(|bank| bank.state_id == payload_id)
            .expect("fixed indexed row.payload should declare a byte bank");
        assert_eq!(payload_bank.state_storage_id, payload_slot.id);
        assert!(payload_bank.indexed);
        assert_eq!(payload_bank.scope_id, payload_slot.scope_id);
        assert_eq!(payload_bank.fixed_len, 3);

        let rows_id = debug_entry_id(&plan.debug_map.list_slots, "list", "rows");
        let list_slot = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id.0 == rows_id)
            .expect("rows list slot should exist");
        let payload_field = list_slot.initial_rows[0]
            .fields
            .iter()
            .find(|field| field.name == "payload")
            .expect("payload initial field should exist");
        let PlanConstantValue::Bytes {
            byte_len,
            inline_bytes,
            ..
        } = &payload_field.value
        else {
            panic!("payload initial field should be a typed BYTES constant: {payload_field:#?}");
        };
        assert_eq!(*byte_len, 3);
        assert_eq!(inline_bytes.as_deref(), Some(&[0, 0, 0][..]));

        let receive_id = SourceId(debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "row.receive",
        ));
        let receive_update = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.indexed
                    && matches!(&op.output, Some(ValueRef::State(id)) if *id == payload_id)
                    && op.inputs.contains(&ValueRef::Source(receive_id))
            })
            .expect("row.receive should update row.payload through an indexed op");
        let PlanOpKind::UpdateBranch {
            expression_kind,
            source_payload_field,
            ..
        } = &receive_update.kind
        else {
            panic!("row.receive payload op should be an update branch: {receive_update:#?}");
        };
        assert_eq!(expression_kind, &PlanExpressionKind::SourcePayload);
        assert_eq!(source_payload_field, &Some(SourcePayloadField::Bytes));
        assert!(receive_update.inputs.contains(&ValueRef::SourcePayload {
            source_id: receive_id,
            field: SourcePayloadField::Bytes,
        }));

        let verification = verify_plan(&plan).unwrap();
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "byte-bank-slots-match-fixed-bytes" && check.pass)
        );
        assert_eq!(
            verification.status, "pass",
            "indexed row BYTES initial field plan should verify: {verification:#?}"
        );
    }

    #[test]
    fn fixed_bytes_scalars_declare_byte_banks_but_dynamic_bytes_do_not() {
        let fixed_parsed = boon_parser::parse_source(
            "examples/bytes_set_plan_ops.bn",
            include_str!("../../../examples/bytes_set_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let fixed_ir = boon_ir::lower(&fixed_parsed).unwrap();
        let fixed_plan = compile_typed_program(&fixed_ir, TargetProfile::SoftwareDefault).unwrap();

        let fixed_id = StateId(debug_entry_id(
            &fixed_plan.debug_map.state_slots,
            "state",
            "store.patched",
        ));
        let fixed_slot = fixed_plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == fixed_id)
            .expect("fixed BYTES slot should exist");
        assert_eq!(
            fixed_slot.value_type,
            PlanValueType::Bytes { fixed_len: Some(4) }
        );
        let fixed_bank = fixed_plan
            .storage_layout
            .byte_banks
            .iter()
            .find(|bank| bank.state_id == fixed_id)
            .expect("fixed BYTES slot should declare a byte bank");
        assert_eq!(fixed_bank.state_storage_id, fixed_slot.id);
        assert_eq!(fixed_bank.fixed_len, 4);
        assert_eq!(fixed_bank.capacity, Some(1));
        assert!(!fixed_bank.indexed);
        let fixed_verification = verify_plan(&fixed_plan).unwrap();
        assert!(
            fixed_verification
                .checks
                .iter()
                .any(|check| check.id == "byte-bank-slots-match-fixed-bytes" && check.pass)
        );
        assert_eq!(fixed_verification.status, "pass");

        let dynamic_parsed = boon_parser::parse_source(
            "examples/bytes_source_payload_plan_ops.bn",
            include_str!("../../../examples/bytes_source_payload_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let dynamic_ir = boon_ir::lower(&dynamic_parsed).unwrap();
        let dynamic_plan =
            compile_typed_program(&dynamic_ir, TargetProfile::SoftwareDefault).unwrap();
        let dynamic_id = StateId(debug_entry_id(
            &dynamic_plan.debug_map.state_slots,
            "state",
            "store.received",
        ));
        let dynamic_slot = dynamic_plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == dynamic_id)
            .expect("dynamic BYTES slot should exist");
        assert_eq!(
            dynamic_slot.value_type,
            PlanValueType::Bytes { fixed_len: None }
        );
        assert!(
            !dynamic_plan
                .storage_layout
                .byte_banks
                .iter()
                .any(|bank| bank.state_id == dynamic_id),
            "dynamic BYTES state should not declare a fixed-size byte bank"
        );

        let dynamic_verification = verify_plan(&dynamic_plan).unwrap();
        assert!(
            dynamic_verification
                .checks
                .iter()
                .any(|check| check.id == "byte-bank-slots-match-fixed-bytes" && check.pass)
        );
        assert_eq!(dynamic_verification.status, "pass");
    }

    #[test]
    fn todomvc_title_to_add_lowers_to_typed_source_key_trim_expression() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let title_to_add_id = debug_entry_id(
            &plan.debug_map.derived_values,
            "field",
            "store.title_to_add",
        );
        let key_down_source_id = debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "store.sources.new_todo_input.key_down",
        );
        let new_text_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "store.new_todo_text");
        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .find(|op| matches!(&op.output, Some(ValueRef::Field(id)) if id.0 == title_to_add_id))
            .expect("title_to_add derived op should exist");
        let PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::SourceEventTransform,
            expression:
                Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                    source_id,
                    key_field,
                    required_key,
                    state,
                    skip_empty,
                }),
            ..
        } = &op.kind
        else {
            panic!("title_to_add should lower to a typed source-key trim expression: {op:#?}");
        };
        assert_eq!(source_id.0, key_down_source_id);
        assert_eq!(key_field, &SourcePayloadField::Key);
        assert_eq!(required_key, "Enter");
        assert_eq!(state, &ValueRef::State(StateId(new_text_state_id)));
        assert!(*skip_empty);
        assert!(
            op.inputs
                .contains(&ValueRef::Source(SourceId(key_down_source_id)))
        );
        assert!(op.inputs.contains(&ValueRef::SourcePayload {
            source_id: SourceId(key_down_source_id),
            field: SourcePayloadField::Key,
        }));
        assert!(
            op.inputs
                .contains(&ValueRef::State(StateId(new_text_state_id)))
        );
        assert!(
            verify_plan(&plan)
                .unwrap()
                .checks
                .iter()
                .any(|check| check.id == "derived-expression-refs-resolve" && check.pass),
            "derived expression refs should verify"
        );
    }

    #[test]
    fn todomvc_row_bool_not_derived_values_lower_to_typed_inputs() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let not_editing_id = debug_entry_id(&plan.debug_map.fields, "field", "todo.not_editing");
        let not_completed_id =
            debug_entry_id(&plan.debug_map.fields, "field", "todo.not_completed");
        let editing_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "todo.editing");
        let completed_state_id =
            debug_entry_id(&plan.debug_map.state_slots, "state", "todo.completed");

        for (output_id, input_state_id) in [
            (not_editing_id, editing_state_id),
            (not_completed_id, completed_state_id),
        ] {
            let op = plan
                .regions
                .iter()
                .filter(|region| region.kind == RegionKind::DerivedEvaluation)
                .flat_map(|region| region.ops.iter())
                .find(|op| matches!(&op.output, Some(ValueRef::Field(id)) if id.0 == output_id))
                .expect("typed Bool/not derived op should exist");
            assert!(op.indexed);
            assert!(matches!(
                &op.kind,
                PlanOpKind::DerivedValue {
                    derived_kind: PlanDerivedKind::Pure,
                    expression: Some(PlanDerivedExpression::BoolNot {
                        input: ValueRef::State(state_id)
                    }),
                    ..
                } if state_id.0 == input_state_id
            ));
            assert!(
                op.inputs
                    .contains(&ValueRef::State(StateId(input_state_id)))
            );
        }

        assert!(
            verify_plan(&plan)
                .unwrap()
                .checks
                .iter()
                .any(|check| check.id == "derived-expression-refs-resolve" && check.pass),
            "Bool/not derived expression refs should verify"
        );
    }

    #[test]
    fn verifier_rejects_tampered_derived_expression_ref() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let op = plan
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::DerivedValue {
                        expression: Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }),
                        ..
                    }
                )
            })
            .expect("typed derived expression should exist");
        op.inputs
            .retain(|input| !matches!(input, ValueRef::State(StateId(0))));

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "derived-expression-refs-resolve" && !check.pass),
            "tampered derived expression refs should fail verifier: {verification:#?}"
        );
    }

    #[test]
    fn verifier_rejects_tampered_append_field_ref() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let append_op = plan
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::ListOperations)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::ListOperation {
                        operation_kind: PlanListOperationKind::Append,
                        ..
                    }
                )
            })
            .expect("append op should exist");
        let PlanOpKind::ListOperation {
            append: Some(append),
            ..
        } = &mut append_op.kind
        else {
            panic!("append op should carry append details");
        };
        append.fields[0].field_id = None;

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "list-append-refs-resolve" && !check.pass),
            "tampered append refs should fail verifier: {verification:#?}"
        );
    }

    #[test]
    fn verifier_rejects_tampered_remove_source_ref() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let remove_source_id = debug_entry_id(
            &plan.debug_map.source_routes,
            "source",
            "todo.sources.remove_todo_button.press",
        );
        let remove_op = plan
            .regions
            .iter_mut()
            .filter(|region| region.kind == RegionKind::ListOperations)
            .flat_map(|region| region.ops.iter_mut())
            .find(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::ListOperation {
                        operation_kind: PlanListOperationKind::Remove,
                        remove: Some(PlanListRemove {
                            source: ValueRef::Source(source_id),
                            ..
                        }),
                        ..
                    } if source_id.0 == remove_source_id
                )
            })
            .expect("row remove op should exist");
        let PlanOpKind::ListOperation {
            remove: Some(remove),
            ..
        } = &mut remove_op.kind
        else {
            panic!("row remove op should carry remove metadata");
        };
        remove.source = ValueRef::Source(SourceId(usize::MAX));

        let verification = verify_plan(&plan).unwrap();
        assert_eq!(verification.status, "fail");
        assert!(
            verification
                .checks
                .iter()
                .any(|check| check.id == "list-remove-refs-resolve" && !check.pass),
            "tampered remove refs should fail verifier: {verification:#?}"
        );
    }

    fn debug_entry_id(entries: &[DebugEntry], prefix: &str, label: &str) -> usize {
        entries
            .iter()
            .find(|entry| entry.label == label)
            .and_then(|entry| {
                entry
                    .id
                    .strip_prefix(prefix)
                    .and_then(|suffix| suffix.strip_prefix(':'))
                    .and_then(|suffix| suffix.parse::<usize>().ok())
            })
            .unwrap_or_else(|| panic!("missing debug entry `{prefix}:{label}`"))
    }

    fn bytes_numeric_fixture_plan() -> MachinePlan {
        let parsed = boon_parser::parse_source(
            "examples/bytes_numeric_plan_ops.bn",
            include_str!("../../../examples/bytes_numeric_plan_ops.bn").to_owned(),
        )
        .unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();
        let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        assert_eq!(verify_plan(&plan).unwrap().status, "pass");
        plan
    }

    fn update_op_id_for(plan: &MachinePlan, source_label: &str, target_label: &str) -> PlanOpId {
        let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", source_label);
        let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", target_label);
        plan.regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
            })
            .map(|op| op.id)
            .unwrap_or_else(|| panic!("missing update op for {source_label} -> {target_label}"))
    }

    fn op_by_id(plan: &MachinePlan, op_id: PlanOpId) -> &PlanOp {
        plan.regions
            .iter()
            .flat_map(|region| region.ops.iter())
            .find(|op| op.id == op_id)
            .unwrap_or_else(|| panic!("missing op {op_id:?}"))
    }

    fn ordered_constant_id(plan: &MachinePlan, op_id: PlanOpId, index: usize) -> PlanConstantId {
        let op = op_by_id(plan, op_id);
        let ordered_inputs = update_branch_ordered_inputs(op);
        match ordered_inputs.get(index) {
            Some(ValueRef::Constant(constant_id)) => *constant_id,
            other => panic!("expected ordered constant input {index} for {op_id:?}, got {other:?}"),
        }
    }

    fn set_number_constant(plan: &mut MachinePlan, constant_id: PlanConstantId, value: i64) {
        let constant = plan
            .constants
            .iter_mut()
            .find(|constant| constant.id == constant_id)
            .unwrap_or_else(|| panic!("missing constant {constant_id:?}"));
        constant.value = PlanConstantValue::Number { value };
    }

    fn assert_numeric_plan_rejected(plan: &MachinePlan, reason: &str) {
        let verification = verify_plan(plan).unwrap();
        assert_eq!(
            verification.status, "fail",
            "{reason} must reject the MachinePlan: {verification:#?}"
        );
        assert!(
            verification.checks.iter().any(|check| matches!(
                check.id.as_str(),
                "constant-refs-resolve-and-match-storage-types"
                    | "capability-summary-derived-counts"
            ) && !check.pass),
            "{reason} should fail a typed constant/capability verifier check: {verification:#?}"
        );
    }
}
