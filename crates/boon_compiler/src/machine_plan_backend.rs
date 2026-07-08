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
        ir::SourcePayloadValueType::Bool => SourcePayloadValueType::Bool,
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
    let root_initial_field_types = root_initial_field_value_types(program);
    let synthetic_initial_field_ids = synthetic_initial_list_field_ids(program);
    let index = ValueIndex::new(program, &root_initial_field_types, &row_initial_field_types);
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
            value_type: plan_value_type_from_initial_with_root_and_row_fields(
                &state.path,
                &state.initial_value,
                plan_scope_id(state.scope_id),
                &root_initial_field_types,
                &row_initial_field_types,
            ),
            scope_id: plan_scope_id(state.scope_id),
            indexed: state.indexed,
            initial_value_kind: initial_value_kind_from_ir(&state.initial_value),
            initial_constant_id: initial_constant_ids[slot_id],
            initial_root_field_path: initial_root_field_path(&state.initial_value),
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

fn root_initial_field_value_types(program: &TypedProgram) -> RootInitialFieldTypeMap {
    let mut root_field_types = RootInitialFieldTypeMap::new();
    let source_payload_types = source_payload_value_type_lookup(program);

    for state in &program.state_cells {
        if !matches!(state.initial_value, InitialValue::RootInitialField { .. }) {
            continue;
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
            payload_types.insert(
                (
                    source.path.clone(),
                    source_payload_field_from_ir(&descriptor.field),
                ),
                plan_value_type_from_source_payload_type(descriptor.value_type),
            );
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
        | UpdateExpression::BytesToText { .. } => Some(PlanValueType::Text),
        UpdateExpression::BoolNot { .. }
        | UpdateExpression::BytesIsEmpty { .. }
        | UpdateExpression::BytesEqual { .. }
        | UpdateExpression::BytesStartsWith { .. }
        | UpdateExpression::BytesEndsWith { .. } => Some(PlanValueType::Bool),
        UpdateExpression::NumberInfix { .. }
        | UpdateExpression::ProjectTime { .. }
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
        | UpdateExpression::FileReadBytes { .. }
        | UpdateExpression::FileWriteBytes { .. } => Some(PlanValueType::Bytes { fixed_len: None }),
        UpdateExpression::PreviousValue { .. }
        | UpdateExpression::MatchConst { .. }
        | UpdateExpression::MatchValueConst { .. }
        | UpdateExpression::MatchTextIsEmptyConst { .. }
        | UpdateExpression::MatchNumberInfixConst { .. }
        | UpdateExpression::ListFindValue { .. }
        | UpdateExpression::Unknown { .. } => None,
    }
}

fn plan_value_type_from_source_payload_type(
    value_type: ir::SourcePayloadValueType,
) -> PlanValueType {
    match value_type {
        ir::SourcePayloadValueType::Bytes => PlanValueType::Bytes { fixed_len: None },
        ir::SourcePayloadValueType::Bool => PlanValueType::Bool,
        ir::SourcePayloadValueType::Text => PlanValueType::Text,
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
        if let Some(backing_state) = source_payload_backing_row_state(
            program,
            index,
            source,
            *payload_source_id,
            field,
            derived.indexed,
        ) {
            input = backing_state;
        }
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
    source_event_transform_default_expression_in_statement(
        program,
        derived,
        index,
        constants,
        inputs,
        env,
        expr_value_types,
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
    statement: &AstStatement,
) -> Option<PlanRowExpression> {
    for child in &statement.children {
        if source_event_transform_statement_mentions_source(program, child, &derived.sources) {
            if child.children.is_empty() {
                continue;
            }
        } else if let Some(value) = lower_row_statement_value(
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
        if let Some(value) = source_event_transform_default_expression_in_statement(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            child,
        ) {
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
    if !matches!(
        derived.kind,
        DerivedValueKind::Pure | DerivedValueKind::ListView
    ) {
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
            "List/count" | "List/length" => Some(PlanValueType::Number),
            "List/join_field" => Some(PlanValueType::Text),
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
    let result = match &expr_by_id(program, expr_id)?.kind {
        AstExprKind::Pipe { op, args, .. } if row_list_builtin(op) => lower_row_list_builtin(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            op,
            None,
            args,
        ),
        AstExprKind::Pipe { op, args, .. } if row_text_builtin(op) => lower_row_text_builtin(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            op,
            None,
            args,
        ),
        AstExprKind::Pipe { op, args, .. } if row_generic_builtin(op) => lower_row_builtin_call(
            program,
            derived,
            index,
            constants,
            inputs,
            env,
            expr_value_types,
            op,
            None,
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
    let mut call_args = Vec::new();
    for child in &statement.children {
        let AstStatementKind::Field { name } = &child.kind else {
            return None;
        };
        let value = child.expr?;
        call_args.push(AstCallArg {
            name: Some(name.clone()),
            value,
            start: child.start,
            end: child.end,
        });
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
            .parse::<i64>()
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
        "List/retain" => {
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
                LoweredRowValue::Scalar(PlanRowExpression::ListMapItem {
                    binding: binding.clone(),
                }),
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
    let list_id = candidates
        .iter()
        .flatten()
        .find_map(|candidate| {
            program
                .lists
                .iter()
                .find(|list| {
                    list.name == *candidate
                        || list
                            .name
                            .rsplit_once('.')
                            .is_some_and(|(_, list_local)| list_local == *candidate)
                })
                .map(|list| plan_list_id(list.id))
        })
        .or_else(|| {
            candidates
                .into_iter()
                .flatten()
                .filter_map(|path| match index.resolve(path) {
                    Some(ValueRef::List(list_id)) => Some(list_id),
                    _ => None,
                })
                .next()
        })?;
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

fn row_field_expression(
    program: &TypedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    inputs: &mut Vec<ValueRef>,
    path: &str,
) -> Option<PlanRowExpression> {
    let mut candidates = scoped_resolution_candidates(&derived.path, path);
    if let Some((parent, _)) = derived.path.rsplit_once('.') {
        candidates.push(format!("{parent}.{path}"));
        if let Some((grandparent, _)) = parent.rsplit_once('.') {
            candidates.push(format!("{grandparent}.{path}"));
        }
    }
    candidates.sort();
    candidates.dedup();
    let value_ref = candidates
        .iter()
        .find_map(|candidate| index.resolve(candidate))
        .or_else(|| {
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

fn scoped_resolution_candidates(parent_path: &str, path: &str) -> Vec<String> {
    let mut candidates = vec![path.to_owned(), canonical_sibling_path(parent_path, path)];
    if let Some((_, local_name)) = path.rsplit_once('.') {
        candidates.push(local_name.to_owned());
    }
    candidates.sort();
    candidates.dedup();
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
            collect_number_operand_ref(index, source, target, indexed, left, refs, unresolved)
                + collect_number_operand_ref(
                    index, source, target, indexed, right, refs, unresolved,
                )
        }
        UpdateExpression::MatchNumberInfixConst {
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
    if operand.parse::<i64>().is_ok() {
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
        UpdateExpression::MatchNumberInfixConst {
            left,
            op,
            right,
            arms,
        } => {
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
                refs.push(ValueRef::Constant(pattern_constant_id));
                refs.push(output_ref);
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
    if let Ok(value) = operand.parse::<i64>() {
        let constant_id = push_plan_constant(constants, PlanConstantValue::Number { value });
        return Some(ValueRef::Constant(constant_id));
    }
    resolve_update_value_ref(index, source, target, indexed, operand)
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
        | UpdateValueExpression::NumberInfix { .. }
        | UpdateValueExpression::MatchNumberInfixConst { .. } => None,
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
                push_plan_constant(constants, PlanConstantValue::Number { value: arm_count });
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
        UpdateValueExpression::MatchNumberInfixConst {
            left,
            op,
            right,
            arms,
        } => {
            let tag_constant_id = push_plan_constant(
                constants,
                PlanConstantValue::Text {
                    value: "match_number_infix_const".to_owned(),
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
                push_plan_constant(constants, PlanConstantValue::Number { value: arm_count });
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
        UpdateValueExpression::NumberInfix { left, right, .. } => {
            collect_number_operand_ref(index, source, target, indexed, left, refs, unresolved)
                + collect_number_operand_ref(
                    index, source, target, indexed, right, refs, unresolved,
                )
        }
        UpdateValueExpression::MatchNumberInfixConst {
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
                .filter_map(|((source, path), field)| {
                    refs.iter()
                        .any(|reference| reference == path)
                        .then(|| (source.clone(), field.clone()))
                })
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
                plan_value_type_from_initial_with_root_and_row_fields(
                    &state.path,
                    &state.initial_value,
                    plan_scope_id(state.scope_id),
                    root_field_types,
                    row_field_types,
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
                        if let Some(field_id) = row_field_id_for_list_field(
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

#[cfg(test)]
#[path = "machine_plan_backend_tests.rs"]
mod tests;
