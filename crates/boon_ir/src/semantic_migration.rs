use super::{
    ContextualMaterialization, DerivedListStorageIds, ErasedBinding, ErasedBindingTarget,
    ErasedFieldDef, ErasedReadTarget, ErasedScopeIndex, ExecutableCallableKind, ExecutableExprId,
    ExecutableExpressionKind, ExecutableProgram, ExecutableStatement, ExecutableStatementId,
    ExprId, FieldId, ListId, ListMemory, ScopeId, SemanticMemoryId, StateCell, StateId,
    executable_expression_children, is_output_registry_value_path,
    reachable_executable_expression_ids,
};
use boon_typecheck::{
    BytesType, CheckedDeclaration, CheckedExprId, CheckedExpression, CheckedExpressionKind,
    CheckedProgram, CheckedProgramLoweringMetadata, DeclId, Type, Variant,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticMemoryKind {
    RootScalar,
    IndexedField,
    ListOwner,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticMemoryIdentity {
    pub canonical_module: String,
    pub owner_path: String,
    pub semantic_path: String,
    pub kind: SemanticMemoryKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemory {
    pub id: SemanticMemoryId,
    pub identity: SemanticMemoryIdentity,
    pub data_type: SemanticDataType,
    pub leaves: Vec<SemanticMemoryLeaf>,
    pub status: SemanticMemoryStatus,
    pub runtime_backing: SemanticMemoryRuntimeBacking,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemoryLeaf {
    pub semantic_path: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMemoryStatus {
    Active,
    Draining { marker_expr_id: ExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMemoryRuntimeBacking {
    RootState {
        state_id: StateId,
        field_id: Option<FieldId>,
    },
    IndexedState {
        state_id: StateId,
        field_id: Option<FieldId>,
        scope_id: ScopeId,
        list_id: Option<ListId>,
    },
    List {
        list_id: ListId,
        row_scope_id: Option<ScopeId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDataType {
    Null,
    Bool,
    Number,
    Text,
    Bytes {
        fixed_len: Option<usize>,
    },
    Variant {
        variants: Vec<SemanticVariantType>,
    },
    Record {
        fields: Vec<SemanticTypeField>,
        open: bool,
    },
    List {
        item: Box<SemanticDataType>,
    },
    Unknown {
        reason: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticTypeField {
    pub name: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticVariantType {
    pub tag: String,
    pub fields: Vec<SemanticTypeField>,
    pub open: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationEdge {
    pub source_leaves: Vec<MigrationSourceLeaf>,
    pub destination: MigrationDestination,
    pub transfer_kind: MigrationTransferKind,
    pub transform: MigrationTransform,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationSourceLeaf {
    pub memory_id: SemanticMemoryId,
    pub semantic_path: String,
    pub data_type: SemanticDataType,
    pub drain_expr_id: ExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationDestination {
    pub memory_id: SemanticMemoryId,
    pub semantic_path: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationTransferKind {
    Scalar,
    List,
    IndexedField,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationTransform {
    Identity,
    PureExpression {
        expression_root: ExprId,
        pipeline: Vec<ExprId>,
    },
}

pub(super) fn lower_semantic_memory_and_migrations(
    checked: &CheckedProgram,
    executable: &ExecutableProgram,
    state_cells: &[StateCell],
    lists: &[ListMemory],
    materializations: &[ContextualMaterialization],
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    storage: &ErasedScopeIndex,
) -> Result<(Vec<SemanticMemory>, Vec<MigrationEdge>), String> {
    let mut memory = build_semantic_memory(
        checked,
        executable,
        state_cells,
        lists,
        derived_list_storage,
        storage,
    )?;
    let mut reachable = reachable_executable_expression_ids(executable, materializations)?;
    loop {
        let before = reachable.len();
        for expression in &executable.expressions {
            if let ExecutableExpressionKind::Draining { input } = expression.kind
                && reachable.contains(&input)
            {
                reachable.insert(expression.id);
            }
        }
        if reachable.len() == before {
            break;
        }
    }
    let has_markers = executable.expressions.iter().any(|expression| {
        reachable.contains(&expression.id)
            && matches!(
                expression.kind,
                ExecutableExpressionKind::Drain { .. } | ExecutableExpressionKind::Draining { .. }
            )
    });
    if !has_markers {
        return Ok((memory, Vec::new()));
    }

    validate_checked_marker_identity(checked, executable, &reachable)?;
    associate_draining_markers(
        executable,
        derived_list_storage,
        storage,
        &reachable,
        &mut memory,
    )?;
    let exact_drain_sources = exact_drain_sources(executable, storage, &reachable, &memory)?;
    let exact_drain_destinations =
        exact_drain_destinations(executable, materializations, storage, &reachable, &memory)?;
    let mut drains = collect_drains(
        checked,
        executable,
        materializations,
        &reachable,
        &memory,
        &exact_drain_sources,
        &exact_drain_destinations,
    )?;
    refine_identity_destination_types(&mut memory, &mut drains);
    validate_pairs_and_coverage(&memory, &drains)?;
    validate_no_ordinary_draining_reads(
        checked,
        executable,
        materializations,
        derived_list_storage,
        storage,
        &reachable,
        &memory,
    )?;
    let edges = lower_edges(checked, executable, materializations, &memory, &drains)?;
    validate_migration_cycles(&memory, &edges)?;
    validate_list_owner_changes(&memory, &edges)?;
    Ok((memory, edges))
}

fn build_semantic_memory(
    checked: &CheckedProgram,
    executable: &ExecutableProgram,
    state_cells: &[StateCell],
    lists: &[ListMemory],
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    storage: &ErasedScopeIndex,
) -> Result<Vec<SemanticMemory>, String> {
    let list_paths = lists
        .iter()
        .map(|list| {
            let path = derived_list_storage
                .values()
                .find(|candidate| candidate.list_id == list.id)
                .map(|candidate| candidate.path.clone())
                .unwrap_or_else(|| list.name.clone());
            (list.id, path)
        })
        .collect::<BTreeMap<_, _>>();
    let list_types = lists
        .iter()
        .map(|list| {
            let typed = derived_list_storage
                .values()
                .find(|storage| storage.list_id == list.id)
                .map(|storage| SemanticDataType::List {
                    item: Box::new(semantic_data_type(&storage.item_type)),
                });
            (
                list.id,
                typed.unwrap_or_else(|| {
                    semantic_type_for_list(executable, list, derived_list_storage)
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut memory = Vec::with_capacity(state_cells.len() + lists.len());

    for state in state_cells.iter().filter(|state| state.published) {
        let storage_bindings = storage
            .bindings
            .iter()
            .filter(|binding| match binding.target {
                ErasedBindingTarget::State {
                    runtime, published, ..
                } => runtime == state.id && published,
                _ => false,
            })
            .collect::<Vec<_>>();
        let [binding] = storage_bindings.as_slice() else {
            return Err(format!(
                "state `{}` has {} exact erased storage bindings",
                state.path,
                storage_bindings.len()
            ));
        };
        let ErasedBindingTarget::State {
            executable: executable_state_id,
            runtime,
            published: true,
            field: field_id,
            row: storage_row,
        } = binding.target
        else {
            unreachable!("published state binding was filtered above")
        };
        if runtime != state.id || state.executable_state_id != Some(executable_state_id) {
            return Err(format!(
                "state `{}` identity differs from erased state binding {}",
                state.path, binding.id
            ));
        }
        if storage_row.map(|row| row.scope) != state.scope_id {
            return Err(format!(
                "state `{}` runtime scope {:?} differs from erased storage scope {:?}",
                state.path,
                state.scope_id,
                storage_row.map(|row| row.scope)
            ));
        }
        let storage_list_id = storage_row.map(|row| row.list);
        let field = field_id
            .map(|field_id| {
                storage
                    .fields
                    .get(field_id.as_usize())
                    .filter(|candidate| candidate.id == field_id)
                    .ok_or_else(|| {
                        format!(
                            "state `{}` binding {} references missing erased field {field_id}",
                            state.path, binding.id
                        )
                    })
            })
            .transpose()?;
        if let Some(field) = field
            && (field.declaration != Some(binding.declaration)
                || field.static_owner != binding.static_owner)
        {
            return Err(format!(
                "state `{}` erased field {field_id:?} has declaration/owner identity inconsistent with binding {}",
                state.path, binding.id
            ));
        }
        let mut data_type = semantic_type_for_state(state, binding, field, executable)?;
        let (kind, owner_path, runtime_backing) = if let Some(scope_id) = state.scope_id {
            let list = storage_list_id.and_then(|list_id| {
                lists
                    .get(list_id.as_usize())
                    .filter(|list| list.id == list_id)
            });
            if let Some(indexed_type) = list
                .and_then(|list| list_paths.get(&list.id).zip(list_types.get(&list.id)))
                .and_then(|(list_path, list_type)| indexed_state_type(state, list_path, list_type))
                .cloned()
                && type_quality(&indexed_type) > type_quality(&data_type)
            {
                data_type = indexed_type;
            }
            let owner_path = list
                .and_then(|list| list_paths.get(&list.id))
                .cloned()
                .unwrap_or_else(|| parent_path(&state.path));
            (
                SemanticMemoryKind::IndexedField,
                owner_path,
                SemanticMemoryRuntimeBacking::IndexedState {
                    state_id: state.id,
                    field_id,
                    scope_id,
                    list_id: storage_list_id,
                },
            )
        } else {
            (
                SemanticMemoryKind::RootScalar,
                parent_path(&state.path),
                SemanticMemoryRuntimeBacking::RootState {
                    state_id: state.id,
                    field_id,
                },
            )
        };
        let semantic_path = state
            .semantic_path
            .clone()
            .unwrap_or_else(|| state.path.clone());
        let source_line = checked_declaration(checked, binding.declaration)
            .map(|declaration| declaration.span.line)
            .unwrap_or(state.source_line);
        let identity = SemanticMemoryIdentity {
            canonical_module: canonical_module_for_line(&checked.lowering_metadata, source_line),
            owner_path,
            semantic_path,
            kind,
        };
        memory.push(SemanticMemory {
            id: SemanticMemoryId(memory.len()),
            leaves: semantic_leaves(&identity.semantic_path, &data_type),
            identity,
            data_type,
            status: SemanticMemoryStatus::Active,
            runtime_backing,
        });
    }

    for list in lists {
        let semantic_path = list_paths
            .get(&list.id)
            .cloned()
            .unwrap_or_else(|| list.name.clone());
        if is_output_registry_value_path(&semantic_path) {
            continue;
        }
        let data_type =
            list_types
                .get(&list.id)
                .cloned()
                .unwrap_or_else(|| SemanticDataType::Unknown {
                    reason: format!(
                        "checked executable has no recursive list type for `{}`",
                        list.name
                    ),
                });
        let source_line = derived_list_storage
            .iter()
            .find(|(_, candidate)| candidate.list_id == list.id)
            .and_then(|(statement_id, _)| executable_statement(executable, *statement_id))
            .and_then(|statement| statement.declaration)
            .and_then(|declaration| checked_declaration(checked, declaration))
            .map(|declaration| declaration.span.line)
            .unwrap_or(list.source_line);
        let identity = SemanticMemoryIdentity {
            canonical_module: canonical_module_for_line(&checked.lowering_metadata, source_line),
            owner_path: parent_path(&semantic_path),
            semantic_path,
            kind: SemanticMemoryKind::ListOwner,
        };
        memory.push(SemanticMemory {
            id: SemanticMemoryId(memory.len()),
            leaves: vec![SemanticMemoryLeaf {
                semantic_path: identity.semantic_path.clone(),
                data_type: data_type.clone(),
            }],
            identity,
            data_type,
            status: SemanticMemoryStatus::Active,
            runtime_backing: SemanticMemoryRuntimeBacking::List {
                list_id: list.id,
                row_scope_id: list.row_scope_id,
            },
        });
    }
    Ok(memory)
}

fn indexed_state_type<'a>(
    state: &StateCell,
    list_path: &str,
    list_type: &'a SemanticDataType,
) -> Option<&'a SemanticDataType> {
    let SemanticDataType::List { item } = list_type else {
        return None;
    };
    let state_path = state.semantic_path.as_deref().unwrap_or(&state.path);
    let relative_path = state_path
        .strip_prefix(list_path)
        .and_then(|suffix| suffix.strip_prefix('.'))
        .or_else(|| state_path.rsplit_once('.').map(|(_, suffix)| suffix))
        .unwrap_or(state_path);
    let parts = relative_path
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    semantic_type_at_path(item, &parts)
}

fn semantic_type_for_state(
    state: &StateCell,
    binding: &ErasedBinding,
    field: Option<&ErasedFieldDef>,
    executable: &ExecutableProgram,
) -> Result<SemanticDataType, String> {
    let state_id = state.executable_state_id.ok_or_else(|| {
        format!(
            "published state `{}` has no exact executable state identity",
            state.path
        )
    })?;
    let definition = executable
        .states
        .get(state_id.as_usize())
        .filter(|candidate| candidate.id == state_id)
        .ok_or_else(|| {
            format!(
                "state `{}` references missing executable state {state_id}",
                state.path
            )
        })?;
    if definition.declaration != binding.declaration
        || definition.owner != binding.static_owner
        || definition.expression != binding.producer
    {
        return Err(format!(
            "state `{}` executable definition {state_id} disagrees with erased binding {}",
            state.path, binding.id
        ));
    }
    let initial = executable
        .expressions
        .get(definition.initial.as_usize())
        .filter(|candidate| candidate.id == definition.initial)
        .ok_or_else(|| {
            format!(
                "state `{}` executable definition {state_id} has missing initializer {}",
                state.path, definition.initial
            )
        })?;
    Ok(std::iter::once(semantic_data_type(&initial.flow_type.ty))
        .chain(std::iter::once(semantic_data_type(&binding.flow_type.ty)))
        .chain(field.map(|field| semantic_data_type(&field.flow_type.ty)))
        .into_iter()
        .max_by_key(type_quality)
        .unwrap_or_else(|| SemanticDataType::Unknown {
            reason: format!(
                "checked executable has no recursive type for state `{}`",
                state.path
            ),
        }))
}

fn semantic_type_for_list(
    executable: &ExecutableProgram,
    list: &ListMemory,
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
) -> SemanticDataType {
    derived_list_storage
        .iter()
        .filter(|(_, storage)| storage.list_id == list.id)
        .filter_map(|(statement_id, _)| {
            executable_statement(executable, *statement_id)
                .and_then(|statement| statement.flow_type.as_ref())
                .map(|flow_type| semantic_data_type(&flow_type.ty))
        })
        .filter(|data_type| matches!(data_type, SemanticDataType::List { .. }))
        .into_iter()
        .max_by_key(semantic_list_type_quality)
        .unwrap_or_else(|| SemanticDataType::Unknown {
            reason: format!(
                "checked executable has no recursive list type for `{}`",
                list.name
            ),
        })
}

fn semantic_list_type_quality(
    data_type: &SemanticDataType,
) -> (bool, bool, usize, (bool, usize, usize)) {
    let (record, closed, field_count) = match data_type {
        SemanticDataType::List { item } => match item.as_ref() {
            SemanticDataType::Record { fields, open } => (true, !open, fields.len()),
            _ => (false, false, 0),
        },
        _ => (false, false, 0),
    };
    (record, closed, field_count, type_quality(data_type))
}

pub(crate) fn semantic_data_type(value: &Type) -> SemanticDataType {
    match value {
        Type::Text => SemanticDataType::Text,
        Type::Number => SemanticDataType::Number,
        Type::Bytes(BytesType::Dynamic) => SemanticDataType::Bytes { fixed_len: None },
        Type::Bytes(BytesType::Fixed(fixed_len)) => SemanticDataType::Bytes {
            fixed_len: Some(*fixed_len),
        },
        Type::Skip => SemanticDataType::Null,
        Type::VariantSet(variants)
            if boon_typecheck::variants_use_boolean_runtime_representation(variants) =>
        {
            SemanticDataType::Bool
        }
        Type::VariantSet(variants) => {
            let mut variants = variants
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) => SemanticVariantType {
                        tag: tag.clone(),
                        fields: Vec::new(),
                        open: false,
                    },
                    Variant::Tagged { tag, fields } => SemanticVariantType {
                        tag: tag.clone(),
                        fields: semantic_type_fields(&fields.fields),
                        open: fields.open,
                    },
                })
                .collect::<Vec<_>>();
            variants.sort_by(|left, right| left.tag.cmp(&right.tag));
            SemanticDataType::Variant { variants }
        }
        Type::Object(shape) => SemanticDataType::Record {
            fields: semantic_type_fields(&shape.fields),
            open: shape.open,
        },
        Type::List(item) => SemanticDataType::List {
            item: Box::new(semantic_data_type(item)),
        },
        Type::Function { .. } => SemanticDataType::Unknown {
            reason: "function values are not semantic memory data".to_owned(),
        },
        Type::RenderContract => SemanticDataType::Unknown {
            reason: "render contracts are not semantic memory data".to_owned(),
        },
        Type::UnresolvedShape { reason } => SemanticDataType::Unknown {
            reason: reason.clone(),
        },
        Type::Var(var) => SemanticDataType::Unknown {
            reason: format!("unresolved type variable {}", var.0),
        },
        Type::Unknown => SemanticDataType::Unknown {
            reason: "unknown type".to_owned(),
        },
    }
}

fn semantic_type_fields(fields: &BTreeMap<String, Type>) -> Vec<SemanticTypeField> {
    fields
        .iter()
        .map(|(name, data_type)| SemanticTypeField {
            name: name.clone(),
            data_type: semantic_data_type(data_type),
        })
        .collect()
}

fn type_quality(data_type: &SemanticDataType) -> (bool, usize, usize) {
    let (unknown, open, nodes) = type_quality_counts(data_type);
    let concrete = match data_type {
        SemanticDataType::Record { fields, .. } => fields.len(),
        SemanticDataType::Variant { variants } => variants.len(),
        SemanticDataType::Unknown { .. } => 0,
        _ => 2,
    };
    (
        unknown == 0 && open == 0,
        usize::MAX - unknown - open,
        concrete + nodes,
    )
}

fn type_quality_counts(data_type: &SemanticDataType) -> (usize, usize, usize) {
    match data_type {
        SemanticDataType::Unknown { .. } => (1, 0, 1),
        SemanticDataType::Record { fields, open } => fields.iter().fold(
            (0, usize::from(*open), 1),
            |(unknown, open_count, nodes), field| {
                let child = type_quality_counts(&field.data_type);
                (unknown + child.0, open_count + child.1, nodes + child.2)
            },
        ),
        SemanticDataType::Variant { variants } => {
            variants
                .iter()
                .fold((0, 0, 1), |(unknown, open_count, nodes), variant| {
                    variant.fields.iter().fold(
                        (unknown, open_count + usize::from(variant.open), nodes + 1),
                        |(unknown, open_count, nodes), field| {
                            let child = type_quality_counts(&field.data_type);
                            (unknown + child.0, open_count + child.1, nodes + child.2)
                        },
                    )
                })
        }
        SemanticDataType::List { item } => {
            let child = type_quality_counts(item);
            (child.0, child.1, child.2 + 1)
        }
        SemanticDataType::Null
        | SemanticDataType::Bool
        | SemanticDataType::Number
        | SemanticDataType::Text
        | SemanticDataType::Bytes { .. } => (0, 0, 1),
    }
}

fn semantic_leaves(path: &str, data_type: &SemanticDataType) -> Vec<SemanticMemoryLeaf> {
    let mut leaves = Vec::new();
    collect_semantic_leaves(path, data_type, &mut leaves);
    if leaves.is_empty() {
        leaves.push(SemanticMemoryLeaf {
            semantic_path: path.to_owned(),
            data_type: data_type.clone(),
        });
    }
    leaves
}

fn collect_semantic_leaves(
    path: &str,
    data_type: &SemanticDataType,
    leaves: &mut Vec<SemanticMemoryLeaf>,
) {
    match data_type {
        SemanticDataType::Record { fields, .. } if !fields.is_empty() => {
            for field in fields {
                collect_semantic_leaves(
                    &format!("{path}.{}", field.name),
                    &field.data_type,
                    leaves,
                );
            }
        }
        _ => leaves.push(SemanticMemoryLeaf {
            semantic_path: path.to_owned(),
            data_type: data_type.clone(),
        }),
    }
}

fn checked_declaration(
    checked: &CheckedProgram,
    declaration: DeclId,
) -> Option<&CheckedDeclaration> {
    checked
        .declarations
        .get(declaration.0 as usize)
        .filter(|candidate| candidate.id == declaration)
        .or_else(|| {
            checked
                .declarations
                .iter()
                .find(|candidate| candidate.id == declaration)
        })
}

fn executable_statement(
    executable: &ExecutableProgram,
    statement: ExecutableStatementId,
) -> Option<&ExecutableStatement> {
    executable
        .statements
        .iter()
        .find(|candidate| candidate.id == statement)
}

fn checked_expression(
    checked: &CheckedProgram,
    expression: CheckedExprId,
) -> Option<&CheckedExpression> {
    checked
        .expressions
        .get(expression.0 as usize)
        .filter(|candidate| candidate.id == expression)
        .or_else(|| {
            checked
                .expressions
                .iter()
                .find(|candidate| candidate.id == expression)
        })
}

fn canonical_module_for_line(metadata: &CheckedProgramLoweringMetadata, line: usize) -> String {
    metadata
        .source_units
        .iter()
        .find(|unit| {
            line >= unit.start_line && line < unit.start_line.saturating_add(unit.line_count.max(1))
        })
        .and_then(|unit| unit.module.clone())
        .unwrap_or_else(|| "$root".to_owned())
}

fn parent_path(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(parent, _)| parent.to_owned())
        .filter(|parent| !parent.is_empty())
        .unwrap_or_else(|| "$root".to_owned())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedRegion {
    memory_id: SemanticMemoryId,
    region_path: String,
    leaf_indexes: Vec<usize>,
    data_type: SemanticDataType,
}

fn region_for_memory_path(memory: &SemanticMemory, path: &str) -> Option<ResolvedRegion> {
    let memory_path = &memory.identity.semantic_path;
    if path != memory_path && !path.starts_with(&format!("{memory_path}.")) {
        return None;
    }
    if memory.identity.kind == SemanticMemoryKind::ListOwner && path != memory_path {
        return None;
    }
    let relative = path
        .strip_prefix(memory_path)
        .unwrap_or_default()
        .trim_start_matches('.')
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let data_type = semantic_type_at_path(&memory.data_type, &relative)?.clone();
    let leaf_indexes = memory
        .leaves
        .iter()
        .enumerate()
        .filter_map(|(index, leaf)| {
            (leaf.semantic_path == path || leaf.semantic_path.starts_with(&format!("{path}.")))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    (!leaf_indexes.is_empty()).then(|| ResolvedRegion {
        memory_id: memory.id,
        region_path: path.to_owned(),
        leaf_indexes,
        data_type,
    })
}

fn semantic_type_at_path<'a>(
    data_type: &'a SemanticDataType,
    path: &[&str],
) -> Option<&'a SemanticDataType> {
    let Some((field, rest)) = path.split_first() else {
        return Some(data_type);
    };
    let SemanticDataType::Record { fields, .. } = data_type else {
        return None;
    };
    let data_type = fields
        .iter()
        .find(|candidate| candidate.name == *field)
        .map(|field| &field.data_type)?;
    semantic_type_at_path(data_type, rest)
}

fn validate_checked_marker_identity(
    checked: &CheckedProgram,
    executable: &ExecutableProgram,
    reachable: &BTreeSet<ExecutableExprId>,
) -> Result<(), String> {
    for expression in executable
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        let checked_expression = checked_expression(checked, expression.checked_expr_id)
            .ok_or_else(|| {
                format!(
                    "executable migration expression {} references missing checked expression {}",
                    expression.id, expression.checked_expr_id.0
                )
            })?;
        match (&expression.kind, &checked_expression.kind) {
            (
                ExecutableExpressionKind::Drain {
                    target, projection, ..
                },
                CheckedExpressionKind::Drain {
                    target: checked_target,
                    projection: checked_projection,
                },
            ) if target == checked_target && projection == checked_projection => {}
            (
                ExecutableExpressionKind::Draining { input },
                CheckedExpressionKind::Draining {
                    input: checked_input,
                },
            ) => {
                let concrete_input = executable
                    .expressions
                    .get(input.as_usize())
                    .filter(|candidate| candidate.id == *input)
                    .ok_or_else(|| {
                        format!(
                            "DRAINING expression {} references missing executable input {input}",
                            expression.id
                        )
                    })?;
                if concrete_input.checked_expr_id != *checked_input {
                    return Err(format!(
                        "DRAINING expression {} input identity differs between checked and executable graphs",
                        checked_expression.id.0
                    ));
                }
            }
            (ExecutableExpressionKind::Drain { .. }, _)
            | (ExecutableExpressionKind::Draining { .. }, _) => {
                return Err(format!(
                    "migration marker {} does not match checked expression {}",
                    expression.id, expression.checked_expr_id.0
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn associate_draining_markers(
    executable: &ExecutableProgram,
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    storage: &ErasedScopeIndex,
    reachable: &BTreeSet<ExecutableExprId>,
    memory: &mut [SemanticMemory],
) -> Result<(), String> {
    let mut markers = executable
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
        .filter_map(|expression| match expression.kind {
            ExecutableExpressionKind::Draining { input } => Some((
                expression.id,
                expression.checked_expr_id,
                expression.owner,
                input,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    markers.sort_by_key(|(_, checked, owner, _)| (*checked, *owner));
    let mut marker_owners =
        BTreeMap::<boon_typecheck::CheckedExprId, BTreeSet<SemanticMemoryId>>::new();
    for (marker, checked, owner, input) in markers {
        let bindings = storage
            .bindings
            .iter()
            .filter(|binding| binding.static_owner == owner)
            .filter(|binding| binding.producer == marker || binding.producer == input)
            .collect::<Vec<_>>();
        let mut owners = bindings
            .into_iter()
            .flat_map(|binding| semantic_memory_for_binding(memory, binding))
            .collect::<BTreeSet<_>>();
        owners.extend(memory.iter().filter_map(|candidate| {
            let SemanticMemoryRuntimeBacking::List { list_id, .. } = candidate.runtime_backing
            else {
                return None;
            };
            let is_exact_root = derived_list_storage
                .iter()
                .filter(|(_, list)| list.list_id == list_id)
                .filter_map(|(statement_id, _)| {
                    executable_statement(executable, *statement_id)
                        .and_then(|statement| statement.value)
                })
                .any(|root| root == marker);
            is_exact_root.then_some(candidate.id)
        }));
        if owners.is_empty() {
            return Err(format!(
                "DRAINING expression {} is not attached to an exact state or list authority owner",
                checked.0
            ));
        }
        marker_owners.entry(checked).or_default().extend(owners);
    }
    for (marker, owners) in marker_owners {
        for owner in owners {
            let candidate = &mut memory[owner.as_usize()];
            if let SemanticMemoryStatus::Draining { marker_expr_id } = candidate.status {
                return Err(format!(
                    "semantic authority `{}` has multiple DRAINING markers (expressions {} and {})",
                    candidate.identity.semantic_path, marker_expr_id, marker.0
                ));
            }
            candidate.status = SemanticMemoryStatus::Draining {
                marker_expr_id: ExprId(marker.0 as usize),
            };
        }
    }
    Ok(())
}

fn semantic_memory_for_binding(
    memory: &[SemanticMemory],
    binding: &ErasedBinding,
) -> Vec<SemanticMemoryId> {
    memory
        .iter()
        .filter_map(|candidate| {
            let matches = match (&binding.target, &candidate.runtime_backing) {
                (
                    ErasedBindingTarget::State { runtime, .. },
                    SemanticMemoryRuntimeBacking::RootState { state_id, .. }
                    | SemanticMemoryRuntimeBacking::IndexedState { state_id, .. },
                ) => runtime == state_id,
                (
                    ErasedBindingTarget::Value {
                        field: Some(field),
                        row: None,
                    },
                    SemanticMemoryRuntimeBacking::RootState {
                        field_id: Some(memory_field),
                        ..
                    },
                ) => field == memory_field,
                (
                    ErasedBindingTarget::Value {
                        field: Some(field),
                        row: Some(row),
                    },
                    SemanticMemoryRuntimeBacking::IndexedState {
                        field_id: Some(memory_field),
                        scope_id,
                        list_id,
                        ..
                    },
                ) => {
                    field == memory_field
                        && row.scope == *scope_id
                        && list_id.is_none_or(|list| list == row.list)
                }
                (
                    ErasedBindingTarget::Value {
                        field: None,
                        row: Some(row),
                    },
                    SemanticMemoryRuntimeBacking::List { list_id, .. },
                ) => row.list == *list_id,
                _ => false,
            };
            matches.then_some(candidate.id)
        })
        .collect()
}

#[derive(Clone, Debug)]
struct DrainUse {
    drain_expr_id: ExprId,
    executable_drain: ExecutableExprId,
    source: ResolvedRegion,
    destination: ResolvedRegion,
    expression_root: ExprId,
    executable_root: ExecutableExprId,
    pipeline: Vec<ExprId>,
    identity: bool,
}

fn refine_identity_destination_types(memory: &mut [SemanticMemory], drains: &mut [DrainUse]) {
    for drain in drains.iter_mut().filter(|drain| drain.identity) {
        let destination = &memory[drain.destination.memory_id.as_usize()];
        let destination_is_whole_memory =
            drain.destination.region_path == destination.identity.semantic_path;
        if !destination_is_whole_memory
            || !migration_type_needs_refinement(&destination.data_type)
            || !migration_type_is_closed(&drain.source.data_type)
        {
            continue;
        }

        let source_type = drain.source.data_type.clone();
        let destination = &mut memory[drain.destination.memory_id.as_usize()];
        destination.data_type = source_type.clone();
        destination.leaves = semantic_leaves(&destination.identity.semantic_path, &source_type);
        drain.destination.data_type = source_type;
        drain.destination.leaf_indexes = (0..destination.leaves.len()).collect();
    }
}

fn migration_type_needs_refinement(data_type: &SemanticDataType) -> bool {
    match data_type {
        SemanticDataType::Unknown { .. } => true,
        SemanticDataType::Record { fields, open } => {
            *open
                || fields
                    .iter()
                    .any(|field| migration_type_needs_refinement(&field.data_type))
        }
        SemanticDataType::Variant { variants } => variants.iter().any(|variant| {
            variant.open
                || variant
                    .fields
                    .iter()
                    .any(|field| migration_type_needs_refinement(&field.data_type))
        }),
        SemanticDataType::List { item } => migration_type_needs_refinement(item),
        SemanticDataType::Null
        | SemanticDataType::Bool
        | SemanticDataType::Number
        | SemanticDataType::Text
        | SemanticDataType::Bytes { .. } => false,
    }
}

fn migration_type_is_closed(data_type: &SemanticDataType) -> bool {
    !migration_type_needs_refinement(data_type)
}

fn collect_drains(
    checked: &CheckedProgram,
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    reachable: &BTreeSet<ExecutableExprId>,
    memory: &[SemanticMemory],
    exact_sources: &BTreeMap<ExecutableExprId, ResolvedRegion>,
    exact_destinations: &BTreeMap<ExecutableExprId, DrainDestination>,
) -> Result<Vec<DrainUse>, String> {
    let mut drains = Vec::new();
    let mut lowered = BTreeSet::new();
    for drain in executable
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
        .filter(|expression| matches!(expression.kind, ExecutableExpressionKind::Drain { .. }))
    {
        let checked_drain =
            checked_expression(checked, drain.checked_expr_id).ok_or_else(|| {
                format!(
                    "DRAIN executable expression {} references missing checked expression {}",
                    drain.id, drain.checked_expr_id.0
                )
            })?;
        let line = checked_drain.span.line;
        let source = exact_sources.get(&drain.id).cloned().ok_or_else(|| {
            format!(
                "DRAIN expression {} at line {line} has no exact erased authority binding",
                drain.checked_expr_id.0
            )
        })?;
        let destination = exact_destinations.get(&drain.id).ok_or_else(|| {
            format!(
                "DRAIN expression {} at line {line} has no exact erased destination authority",
                drain.checked_expr_id.0
            )
        })?;
        let destination_memory = memory
            .get(destination.memory_id.as_usize())
            .filter(|candidate| candidate.id == destination.memory_id)
            .ok_or_else(|| {
                format!(
                    "DRAIN expression {} references missing destination memory {}",
                    drain.checked_expr_id.0, destination.memory_id
                )
            })?;
        let destination_region = region_for_memory_path(
            destination_memory,
            &destination_memory.identity.semantic_path,
        )
        .ok_or_else(|| {
            format!(
                "DRAIN destination `{}` has no closed semantic leaves",
                destination_memory.identity.semantic_path
            )
        })?;
        let root = if destination_memory.identity.kind == SemanticMemoryKind::ListOwner {
            drain.id
        } else {
            destination.initializer
        };
        let root_expression = executable
            .expressions
            .get(root.as_usize())
            .filter(|candidate| candidate.id == root)
            .ok_or_else(|| {
                format!(
                    "DRAIN destination `{}` references missing initializer {root}",
                    destination_memory.identity.semantic_path
                )
            })?;
        if !migration_expression_reaches(executable, materializations, root, drain.id) {
            return Err(format!(
                "DRAIN at line {line} conflicts with destination authority `{}`; DRAIN must be part of its initializer",
                destination_memory.identity.semantic_path
            ));
        }
        let identity = root == drain.id;
        let key = (
            drain.checked_expr_id,
            source.memory_id,
            source.region_path.clone(),
            destination.memory_id,
            destination_region.region_path.clone(),
            root_expression.checked_expr_id,
        );
        if !lowered.insert(key) {
            continue;
        }
        drains.push(DrainUse {
            drain_expr_id: ExprId(drain.checked_expr_id.0 as usize),
            executable_drain: drain.id,
            source,
            destination: destination_region,
            expression_root: ExprId(root_expression.checked_expr_id.0 as usize),
            executable_root: root,
            pipeline: vec![ExprId(root_expression.checked_expr_id.0 as usize)],
            identity,
        });
    }
    Ok(drains)
}

fn exact_drain_sources(
    executable: &ExecutableProgram,
    storage: &ErasedScopeIndex,
    reachable: &BTreeSet<ExecutableExprId>,
    memory: &[SemanticMemory],
) -> Result<BTreeMap<ExecutableExprId, ResolvedRegion>, String> {
    let mut sources = BTreeMap::<ExecutableExprId, ResolvedRegion>::new();
    for expression in executable
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        let ExecutableExpressionKind::Drain { .. } = &expression.kind else {
            continue;
        };
        let read = storage
            .reads
            .iter()
            .find(|read| read.expression == expression.id)
            .ok_or_else(|| {
                format!(
                    "DRAIN expression {} has no exact erased read target",
                    expression.checked_expr_id.0
                )
            })?;
        let (binding, projection) = match &read.target {
            ErasedReadTarget::Binding {
                binding,
                projection,
            } => (*binding, projection.as_slice()),
            ErasedReadTarget::StateProjection {
                binding, fields, ..
            } => (*binding, fields.as_slice()),
            target => {
                return Err(format!(
                    "DRAIN expression {} has non-authority read target {target:?}",
                    expression.checked_expr_id.0
                ));
            }
        };
        let binding = storage
            .bindings
            .get(binding.as_usize())
            .filter(|candidate| candidate.id == binding)
            .ok_or_else(|| {
                format!(
                    "DRAIN expression {} references missing binding {binding}",
                    expression.checked_expr_id.0
                )
            })?;
        let owners = semantic_memory_for_binding(memory, binding);
        let [owner] = owners.as_slice() else {
            return Err(format!(
                "DRAIN expression {} binding {} (`{}`; producer {}; target {:?}) resolves to {} semantic memories",
                expression.checked_expr_id.0,
                binding.id,
                binding.diagnostic_path,
                binding.producer,
                binding.target,
                owners.len()
            ));
        };
        let memory = memory
            .get(owner.as_usize())
            .filter(|memory| memory.id == *owner)
            .ok_or_else(|| format!("DRAIN references missing semantic memory {owner}"))?;
        let path = if projection.is_empty() {
            memory.identity.semantic_path.clone()
        } else {
            format!("{}.{}", memory.identity.semantic_path, projection.join("."))
        };
        let region = region_for_memory_path(memory, &path).ok_or_else(|| {
            format!(
                "DRAIN expression {} projection `{}` is outside semantic memory `{}`",
                expression.checked_expr_id.0,
                projection.join("."),
                memory.identity.semantic_path
            )
        })?;
        if let Some(previous) = sources.insert(expression.id, region.clone())
            && previous != region
        {
            return Err(format!(
                "DRAIN executable expression {} has incompatible concrete authority bindings",
                expression.id
            ));
        }
    }
    Ok(sources)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DrainDestination {
    memory_id: SemanticMemoryId,
    initializer: ExecutableExprId,
}

fn exact_drain_destinations(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    storage: &ErasedScopeIndex,
    reachable: &BTreeSet<ExecutableExprId>,
    memory: &[SemanticMemory],
) -> Result<BTreeMap<ExecutableExprId, DrainDestination>, String> {
    let mut destinations = BTreeMap::<ExecutableExprId, DrainDestination>::new();
    for expression in executable
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        if !matches!(expression.kind, ExecutableExpressionKind::Drain { .. }) {
            continue;
        }
        let mut owners = storage
            .bindings
            .iter()
            .filter(|binding| {
                migration_expression_reaches(
                    executable,
                    materializations,
                    binding.producer,
                    expression.id,
                )
            })
            .filter_map(|binding| {
                let ErasedBindingTarget::State {
                    executable: state, ..
                } = binding.target
                else {
                    return None;
                };
                let definition = executable
                    .states
                    .get(state.as_usize())
                    .filter(|candidate| candidate.id == state)?;
                Some((binding, definition.initial))
            })
            .flat_map(|(binding, initializer)| {
                semantic_memory_for_binding(memory, binding)
                    .into_iter()
                    .map(move |memory_id| DrainDestination {
                        memory_id,
                        initializer,
                    })
            })
            .collect::<BTreeSet<_>>();
        for materialization in materializations.iter().filter(|materialization| {
            migration_expression_reaches(
                executable,
                materializations,
                materialization.source,
                expression.id,
            )
        }) {
            let Some(target_list_id) = materialization.target_list_id else {
                continue;
            };
            owners.extend(memory.iter().filter_map(|candidate| {
                matches!(
                    candidate.runtime_backing,
                    SemanticMemoryRuntimeBacking::List { list_id, .. }
                        if list_id == target_list_id
                )
                .then_some(DrainDestination {
                    memory_id: candidate.id,
                    initializer: expression.id,
                })
            }));
        }
        let owners = owners.into_iter().collect::<Vec<_>>();
        let [owner] = owners.as_slice() else {
            return Err(format!(
                "DRAIN expression {} (executable {}) is contained by {} exact destination authorities",
                expression.checked_expr_id.0,
                expression.id,
                owners.len()
            ));
        };
        if let Some(previous) = destinations.insert(expression.id, *owner)
            && previous != *owner
        {
            return Err(format!(
                "DRAIN executable expression {} has incompatible concrete destination owners",
                expression.id
            ));
        }
    }
    Ok(destinations)
}

fn migration_expression_reaches(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    root: ExecutableExprId,
    target: ExecutableExprId,
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        if expression_id == target {
            return true;
        }
        let Some(expression) = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
        else {
            continue;
        };
        if let ExecutableExpressionKind::Materialize { materialization } = expression.kind
            && let Some(materialization) = materializations
                .get(materialization)
                .filter(|candidate| candidate.id == materialization)
        {
            pending.extend(materialization.expression_roots());
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    false
}

fn validate_pairs_and_coverage(
    memory: &[SemanticMemory],
    drains: &[DrainUse],
) -> Result<(), String> {
    let mut coverage = BTreeMap::<SemanticMemoryId, BTreeSet<usize>>::new();
    let mut regions = BTreeMap::<SemanticMemoryId, Vec<String>>::new();
    for drain in drains {
        let source_memory = &memory[drain.source.memory_id.as_usize()];
        if !matches!(source_memory.status, SemanticMemoryStatus::Draining { .. }) {
            let draining = memory
                .iter()
                .filter(|candidate| {
                    matches!(candidate.status, SemanticMemoryStatus::Draining { .. })
                })
                .map(|candidate| candidate.identity.semantic_path.as_str())
                .collect::<Vec<_>>();
            return Err(format!(
                "DRAIN source `{}` is not marked DRAINING (missing pair); marked authorities={draining:?}",
                drain.source.region_path,
            ));
        }
        if drain.source.memory_id == drain.destination.memory_id {
            return Err(format!(
                "self drain is not allowed for semantic authority `{}`",
                source_memory.identity.semantic_path
            ));
        }
        let prior_regions = regions.entry(drain.source.memory_id).or_default();
        for prior in prior_regions.iter() {
            if prior == &drain.source.region_path {
                return Err(format!(
                    "semantic authority region `{}` is drained more than once",
                    drain.source.region_path
                ));
            }
            if path_contains(prior, &drain.source.region_path)
                || path_contains(&drain.source.region_path, prior)
            {
                return Err(format!(
                    "overlapping ancestor/descendant drains `{prior}` and `{}` are not allowed",
                    drain.source.region_path
                ));
            }
        }
        prior_regions.push(drain.source.region_path.clone());
        let covered = coverage.entry(drain.source.memory_id).or_default();
        for leaf in &drain.source.leaf_indexes {
            if !covered.insert(*leaf) {
                return Err(format!(
                    "semantic authority leaf `{}` is drained more than once",
                    source_memory.leaves[*leaf].semantic_path
                ));
            }
        }
    }

    for candidate in memory
        .iter()
        .filter(|memory| matches!(memory.status, SemanticMemoryStatus::Draining { .. }))
    {
        let covered = coverage.get(&candidate.id);
        if covered.is_none_or(BTreeSet::is_empty) {
            return Err(format!(
                "semantic authority `{}` is marked DRAINING but has no DRAIN destination (missing pair)",
                candidate.identity.semantic_path
            ));
        }
        let missing = candidate
            .leaves
            .iter()
            .enumerate()
            .filter_map(|(index, leaf)| {
                (!covered.is_some_and(|covered| covered.contains(&index)))
                    .then_some(leaf.semantic_path.as_str())
            })
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!(
                "DRAINING source region `{}` has partial coverage; missing authoritative leaves: {}",
                candidate.identity.semantic_path,
                missing.join(", ")
            ));
        }
    }
    Ok(())
}

fn path_contains(ancestor: &str, descendant: &str) -> bool {
    descendant.starts_with(&format!("{ancestor}."))
}

fn validate_no_ordinary_draining_reads(
    checked: &CheckedProgram,
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    storage: &ErasedScopeIndex,
    reachable: &BTreeSet<ExecutableExprId>,
    memory: &[SemanticMemory],
) -> Result<(), String> {
    let draining = memory
        .iter()
        .filter(|memory| matches!(memory.status, SemanticMemoryStatus::Draining { .. }))
        .map(|memory| memory.id)
        .collect::<BTreeSet<_>>();
    if draining.is_empty() {
        return Ok(());
    }
    let mut definitions = BTreeMap::new();
    for memory_id in &draining {
        let candidate = &memory[memory_id.as_usize()];
        let roots =
            semantic_memory_definition_roots(candidate, executable, derived_list_storage, storage)?;
        let mut expressions = BTreeSet::new();
        for root in roots {
            expressions.extend(executable_subtree(executable, materializations, root)?);
        }
        definitions.insert(*memory_id, expressions);
    }

    for read in storage
        .reads
        .iter()
        .filter(|read| reachable.contains(&read.expression))
    {
        let expression = executable
            .expressions
            .get(read.expression.as_usize())
            .filter(|candidate| candidate.id == read.expression)
            .ok_or_else(|| {
                format!(
                    "erased read references missing expression {}",
                    read.expression
                )
            })?;
        if matches!(expression.kind, ExecutableExpressionKind::Drain { .. }) {
            continue;
        }
        let (binding_id, projection) = match &read.target {
            ErasedReadTarget::Binding {
                binding,
                projection,
            } => (*binding, projection.as_slice()),
            ErasedReadTarget::StateProjection {
                binding, fields, ..
            } => (*binding, fields.as_slice()),
            _ => continue,
        };
        let binding = storage
            .bindings
            .get(binding_id.as_usize())
            .filter(|candidate| candidate.id == binding_id)
            .ok_or_else(|| {
                format!(
                    "ordinary migration read {} references missing binding {binding_id}",
                    read.expression
                )
            })?;
        for memory_id in semantic_memory_for_binding(memory, binding) {
            if !draining.contains(&memory_id)
                || definitions
                    .get(&memory_id)
                    .is_some_and(|expressions| expressions.contains(&read.expression))
            {
                continue;
            }
            let authority = &memory[memory_id.as_usize()];
            let region_path = if projection.is_empty() {
                authority.identity.semantic_path.clone()
            } else {
                format!(
                    "{}.{}",
                    authority.identity.semantic_path,
                    projection.join(".")
                )
            };
            let line = checked_expression(checked, expression.checked_expr_id)
                .map(|expression| expression.span.line)
                .unwrap_or_default();
            return Err(format!(
                "ordinary reference to DRAINING authority `{region_path}` at line {line} is not allowed; use DRAIN at the migration destination"
            ));
        }
    }
    Ok(())
}

fn semantic_memory_definition_roots(
    memory: &SemanticMemory,
    executable: &ExecutableProgram,
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    storage: &ErasedScopeIndex,
) -> Result<Vec<ExecutableExprId>, String> {
    match memory.runtime_backing {
        SemanticMemoryRuntimeBacking::RootState { state_id, .. }
        | SemanticMemoryRuntimeBacking::IndexedState { state_id, .. } => {
            let roots = storage
                .bindings
                .iter()
                .filter_map(|binding| match binding.target {
                    ErasedBindingTarget::State {
                        runtime,
                        published: true,
                        ..
                    } if runtime == state_id => Some(binding.producer),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if roots.len() != 1 {
                return Err(format!(
                    "DRAINING authority `{}` has {} exact state definition roots",
                    memory.identity.semantic_path,
                    roots.len()
                ));
            }
            Ok(roots)
        }
        SemanticMemoryRuntimeBacking::List { list_id, .. } => {
            let roots = derived_list_storage
                .iter()
                .filter(|(_, candidate)| candidate.list_id == list_id)
                .filter_map(|(statement_id, _)| {
                    executable_statement(executable, *statement_id)
                        .and_then(|statement| statement.value)
                })
                .collect::<Vec<_>>();
            if roots.len() != 1 {
                return Err(format!(
                    "DRAINING list authority `{}` has {} exact checked definition roots",
                    memory.identity.semantic_path,
                    roots.len()
                ));
            }
            Ok(roots)
        }
    }
}

fn executable_subtree(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    root: ExecutableExprId,
) -> Result<BTreeSet<ExecutableExprId>, String> {
    let mut expressions = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(expression_id) = pending.pop() {
        if !expressions.insert(expression_id) {
            continue;
        }
        let expression = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|candidate| candidate.id == expression_id)
            .ok_or_else(|| {
                format!("migration definition reaches missing expression {expression_id}")
            })?;
        if let ExecutableExpressionKind::Materialize { materialization } = expression.kind {
            let materialization = materializations
                .get(materialization)
                .filter(|candidate| candidate.id == materialization)
                .ok_or_else(|| {
                    format!(
                        "migration definition reaches missing materialization {materialization}"
                    )
                })?;
            pending.extend(materialization.expression_roots());
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    Ok(expressions)
}

fn lower_edges(
    checked: &CheckedProgram,
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    memory: &[SemanticMemory],
    drains: &[DrainUse],
) -> Result<Vec<MigrationEdge>, String> {
    let mut destination_roots = BTreeMap::<(SemanticMemoryId, String), ExprId>::new();
    let mut destination_regions = BTreeMap::<SemanticMemoryId, Vec<String>>::new();
    let mut grouped = BTreeMap::<(SemanticMemoryId, String, ExprId), Vec<&DrainUse>>::new();
    for drain in drains {
        let destination_key = (
            drain.destination.memory_id,
            drain.destination.region_path.clone(),
        );
        let regions = destination_regions
            .entry(drain.destination.memory_id)
            .or_default();
        if let Some(prior) = regions.iter().find(|prior| {
            prior.as_str() != drain.destination.region_path
                && (path_contains(prior, &drain.destination.region_path)
                    || path_contains(&drain.destination.region_path, prior))
        }) {
            return Err(format!(
                "conflicting destination authority regions `{prior}` and `{}` overlap",
                drain.destination.region_path
            ));
        }
        if !regions.contains(&drain.destination.region_path) {
            regions.push(drain.destination.region_path.clone());
        }
        if let Some(prior_root) =
            destination_roots.insert(destination_key.clone(), drain.expression_root)
            && prior_root != drain.expression_root
        {
            return Err(format!(
                "conflicting destination authority `{}` is initialized by migration roots {} and {}",
                destination_key.1, prior_root, drain.expression_root
            ));
        }
        grouped
            .entry((destination_key.0, destination_key.1, drain.expression_root))
            .or_default()
            .push(drain);
    }

    let mut edges = Vec::with_capacity(grouped.len());
    for ((destination_memory_id, destination_path, expression_root), uses) in grouped {
        let destination_region = &uses[0].destination;
        let list_transfer =
            memory[destination_memory_id.as_usize()].identity.kind == SemanticMemoryKind::ListOwner;
        if !list_transfer {
            ensure_closed_migration_type(
                &destination_region.data_type,
                &format!("destination `{destination_path}`"),
            )?;
        }
        let all_identity = uses.len() == 1 && uses[0].identity;
        let output_type = if all_identity {
            uses[0].source.data_type.clone()
        } else {
            let roots = uses
                .iter()
                .map(|drain| drain.executable_root)
                .collect::<BTreeSet<_>>();
            let data_types = roots
                .iter()
                .map(|root| {
                    executable
                        .expressions
                        .get(root.as_usize())
                        .filter(|candidate| candidate.id == *root)
                        .map(|expression| semantic_data_type(&expression.flow_type.ty))
                        .ok_or_else(|| {
                            format!("migration transform references missing expression {root}")
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let Some(data_type) = data_types.first().cloned() else {
                return Err(format!(
                    "migration transform expression {expression_root} has no concrete output type"
                ));
            };
            if data_types.iter().any(|candidate| candidate != &data_type) {
                return Err(format!(
                    "migration transform expression {expression_root} has inconsistent concrete output types"
                ));
            }
            ensure_closed_migration_type(
                &data_type,
                &format!("migration transform expression {expression_root}"),
            )?;
            let allowed_drains = uses
                .iter()
                .map(|drain| drain.executable_drain)
                .collect::<BTreeSet<_>>();
            for root in roots {
                validate_pure_transform(
                    checked,
                    executable,
                    materializations,
                    root,
                    &allowed_drains,
                )?;
            }
            data_type
        };
        if !list_transfer && !migration_type_assignable(&output_type, &destination_region.data_type)
        {
            return Err(format!(
                "migration transform type mismatch for destination `{destination_path}`: output {:?}, destination {:?}",
                output_type, destination_region.data_type
            ));
        }

        let mut source_leaves = Vec::new();
        for drain in &uses {
            if !list_transfer {
                ensure_closed_migration_type(
                    &drain.source.data_type,
                    &format!("source `{}`", drain.source.region_path),
                )?;
            }
            let source_memory = &memory[drain.source.memory_id.as_usize()];
            for leaf_index in &drain.source.leaf_indexes {
                let leaf = &source_memory.leaves[*leaf_index];
                source_leaves.push(MigrationSourceLeaf {
                    memory_id: source_memory.id,
                    semantic_path: leaf.semantic_path.clone(),
                    data_type: leaf.data_type.clone(),
                    drain_expr_id: drain.drain_expr_id,
                });
            }
        }
        source_leaves.sort_by(|left, right| {
            (left.memory_id, &left.semantic_path, left.drain_expr_id).cmp(&(
                right.memory_id,
                &right.semantic_path,
                right.drain_expr_id,
            ))
        });
        let destination_memory = &memory[destination_memory_id.as_usize()];
        let transfer_kind = match destination_memory.identity.kind {
            SemanticMemoryKind::RootScalar => MigrationTransferKind::Scalar,
            SemanticMemoryKind::IndexedField => MigrationTransferKind::IndexedField,
            SemanticMemoryKind::ListOwner => MigrationTransferKind::List,
        };
        edges.push(MigrationEdge {
            source_leaves,
            destination: MigrationDestination {
                memory_id: destination_memory_id,
                semantic_path: destination_path,
                data_type: destination_region.data_type.clone(),
            },
            transfer_kind,
            transform: if all_identity {
                MigrationTransform::Identity
            } else {
                MigrationTransform::PureExpression {
                    expression_root,
                    pipeline: uses[0].pipeline.clone(),
                }
            },
        });
    }
    edges.sort_by(|left, right| {
        (left.destination.memory_id, &left.destination.semantic_path).cmp(&(
            right.destination.memory_id,
            &right.destination.semantic_path,
        ))
    });
    Ok(edges)
}

fn migration_type_assignable(source: &SemanticDataType, target: &SemanticDataType) -> bool {
    match (source, target) {
        (
            SemanticDataType::Variant { variants: source },
            SemanticDataType::Variant { variants: target },
        ) => source.iter().all(|source_variant| {
            target.iter().any(|target_variant| {
                source_variant.tag == target_variant.tag
                    && source_variant.open == target_variant.open
                    && source_variant.fields.len() == target_variant.fields.len()
                    && source_variant.fields.iter().all(|source_field| {
                        target_variant.fields.iter().any(|target_field| {
                            source_field.name == target_field.name
                                && migration_type_assignable(
                                    &source_field.data_type,
                                    &target_field.data_type,
                                )
                        })
                    })
            })
        }),
        (
            SemanticDataType::Record {
                fields: source,
                open: source_open,
            },
            SemanticDataType::Record {
                fields: target,
                open: target_open,
            },
        ) => {
            (!source_open || *target_open)
                && target.iter().all(|target_field| {
                    source.iter().any(|source_field| {
                        source_field.name == target_field.name
                            && migration_type_assignable(
                                &source_field.data_type,
                                &target_field.data_type,
                            )
                    })
                })
                && (*target_open || source.len() == target.len())
        }
        (SemanticDataType::List { item: source }, SemanticDataType::List { item: target }) => {
            migration_type_assignable(source, target)
        }
        _ => source == target,
    }
}

fn ensure_closed_migration_type(data_type: &SemanticDataType, context: &str) -> Result<(), String> {
    match data_type {
        SemanticDataType::Unknown { reason } => {
            Err(format!("{context} has unknown migration type: {reason}"))
        }
        SemanticDataType::Record { fields, open } => {
            if *open {
                return Err(format!("{context} has an open record migration type"));
            }
            for field in fields {
                ensure_closed_migration_type(
                    &field.data_type,
                    &format!("{context}.{}", field.name),
                )?;
            }
            Ok(())
        }
        SemanticDataType::Variant { variants } => {
            for variant in variants {
                if variant.open {
                    return Err(format!(
                        "{context} variant `{}` has an open migration type",
                        variant.tag
                    ));
                }
                for field in &variant.fields {
                    ensure_closed_migration_type(
                        &field.data_type,
                        &format!("{context}.{}.{}", variant.tag, field.name),
                    )?;
                }
            }
            Ok(())
        }
        SemanticDataType::List { item } => ensure_closed_migration_type(item, context),
        SemanticDataType::Null
        | SemanticDataType::Bool
        | SemanticDataType::Number
        | SemanticDataType::Text
        | SemanticDataType::Bytes { .. } => Ok(()),
    }
}

fn validate_pure_transform(
    checked: &CheckedProgram,
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    root: ExecutableExprId,
    allowed_drains: &BTreeSet<ExecutableExprId>,
) -> Result<(), String> {
    ExecutablePurityChecker {
        checked,
        executable,
        materializations,
        allowed_drains,
        active: BTreeSet::new(),
    }
    .check(root, &BTreeSet::new())
}

struct ExecutablePurityChecker<'a> {
    checked: &'a CheckedProgram,
    executable: &'a ExecutableProgram,
    materializations: &'a [ContextualMaterialization],
    allowed_drains: &'a BTreeSet<ExecutableExprId>,
    active: BTreeSet<ExecutableExprId>,
}

impl ExecutablePurityChecker<'_> {
    fn check(
        &mut self,
        expression_id: ExecutableExprId,
        locals: &BTreeSet<DeclId>,
    ) -> Result<(), String> {
        if !self.active.insert(expression_id) {
            return Err(format!(
                "migration transform contains a recursive executable expression at {expression_id}"
            ));
        }
        let result = self.check_inner(expression_id, locals);
        self.active.remove(&expression_id);
        result
    }

    fn check_inner(
        &mut self,
        expression_id: ExecutableExprId,
        locals: &BTreeSet<DeclId>,
    ) -> Result<(), String> {
        let expression = self
            .executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|candidate| candidate.id == expression_id)
            .ok_or_else(|| {
                format!("migration transform references missing expression {expression_id}")
            })?;
        let checked_expression = checked_expression(self.checked, expression.checked_expr_id)
            .ok_or_else(|| {
                format!(
                    "migration transform expression {expression_id} references missing checked expression {}",
                    expression.checked_expr_id.0
                )
            })?;
        let line = checked_expression.span.line;
        if matches!(
            &checked_expression.kind,
            CheckedExpressionKind::Hold { .. }
                | CheckedExpressionKind::Latest { .. }
                | CheckedExpressionKind::While { .. }
                | CheckedExpressionKind::Then { .. }
                | CheckedExpressionKind::Draining { .. }
        ) {
            return Err(format!(
                "migration transform at line {line} uses a stateful or flow combinator"
            ));
        }
        if expression.effect.reads_state
            || expression.effect.writes_state
            || expression.effect.emits_source
            || expression.effect.invokes_host
        {
            return Err(format!(
                "migration transform at line {line} contains a stateful, source-emitting, or host effect"
            ));
        }

        match &expression.kind {
            ExecutableExpressionKind::Drain { .. }
                if self.allowed_drains.contains(&expression_id) =>
            {
                Ok(())
            }
            ExecutableExpressionKind::Drain { .. } => Err(format!(
                "migration transform at line {line} contains a DRAIN owned by another destination"
            )),
            ExecutableExpressionKind::CanonicalRead { path, .. } => Err(format!(
                "migration transform at line {line} reads `{path}` outside its DRAIN inputs"
            )),
            ExecutableExpressionKind::LocalRead { declaration, .. } => {
                if locals.contains(declaration) {
                    Ok(())
                } else {
                    Err(format!(
                        "migration transform at line {line} reads unbound local declaration {}",
                        declaration.0
                    ))
                }
            }
            ExecutableExpressionKind::ExternalRead { canonical_path } => Err(format!(
                "migration transform at line {line} reads external value `{canonical_path}`"
            )),
            ExecutableExpressionKind::ElementState { .. } => Err(format!(
                "migration transform at line {line} reads element state"
            )),
            ExecutableExpressionKind::Source { .. } => Err(format!(
                "migration transform at line {line} reads SOURCE data"
            )),
            ExecutableExpressionKind::Call {
                callable_kind: ExecutableCallableKind::External,
                name,
                ..
            } => Err(format!(
                "migration transform at line {line} calls external function `{name}`"
            )),
            ExecutableExpressionKind::Hold { .. }
            | ExecutableExpressionKind::Latest { .. }
            | ExecutableExpressionKind::Then { .. }
            | ExecutableExpressionKind::Draining { .. } => Err(format!(
                "migration transform at line {line} uses a stateful or flow combinator"
            )),
            ExecutableExpressionKind::Delimiter => Err(format!(
                "migration transform at line {line} contains an unbound pipeline delimiter"
            )),
            ExecutableExpressionKind::Block { bindings, result } => {
                let mut scoped = locals.clone();
                for binding in bindings {
                    self.check(binding.value, &scoped)?;
                    scoped.insert(binding.declaration);
                }
                self.check(*result, &scoped)
            }
            ExecutableExpressionKind::Materialize { materialization } => {
                let materialization = self
                    .materializations
                    .get(*materialization)
                    .filter(|candidate| candidate.id == *materialization)
                    .ok_or_else(|| {
                        format!(
                            "migration transform references missing materialization {materialization}"
                        )
                    })?;
                for root in materialization.expression_roots() {
                    self.check(root, locals)?;
                }
                Ok(())
            }
            ExecutableExpressionKind::FunctionParameter { .. } => Err(format!(
                "migration transform at line {line} reads an unbound function parameter"
            )),
            ExecutableExpressionKind::MaterializationLocal { .. }
            | ExecutableExpressionKind::Text(_)
            | ExecutableExpressionKind::Number(_)
            | ExecutableExpressionKind::BytesByte(_)
            | ExecutableExpressionKind::Bool(_)
            | ExecutableExpressionKind::Tag(_) => Ok(()),
            _ => {
                for child in executable_expression_children(&expression.kind) {
                    self.check(child, locals)?;
                }
                Ok(())
            }
        }
    }
}

fn validate_migration_cycles(
    memory: &[SemanticMemory],
    edges: &[MigrationEdge],
) -> Result<(), String> {
    let mut graph = BTreeMap::<SemanticMemoryId, BTreeSet<SemanticMemoryId>>::new();
    for edge in edges {
        for source in &edge.source_leaves {
            graph
                .entry(source.memory_id)
                .or_default()
                .insert(edge.destination.memory_id);
        }
    }
    let mut complete = BTreeSet::new();
    let mut active = Vec::new();
    for memory_id in graph.keys().copied().collect::<Vec<_>>() {
        if let Some(cycle) = migration_cycle_from(memory_id, &graph, &mut complete, &mut active) {
            return Err(format!(
                "migration graph cycle is not allowed: {}",
                cycle
                    .iter()
                    .map(|id| memory[id.as_usize()].identity.semantic_path.as_str())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ));
        }
    }
    Ok(())
}

fn migration_cycle_from(
    memory_id: SemanticMemoryId,
    graph: &BTreeMap<SemanticMemoryId, BTreeSet<SemanticMemoryId>>,
    complete: &mut BTreeSet<SemanticMemoryId>,
    active: &mut Vec<SemanticMemoryId>,
) -> Option<Vec<SemanticMemoryId>> {
    if let Some(start) = active.iter().position(|candidate| *candidate == memory_id) {
        let mut cycle = active[start..].to_vec();
        cycle.push(memory_id);
        return Some(cycle);
    }
    if complete.contains(&memory_id) {
        return None;
    }
    active.push(memory_id);
    for next in graph.get(&memory_id).into_iter().flatten().copied() {
        if let Some(cycle) = migration_cycle_from(next, graph, complete, active) {
            return Some(cycle);
        }
    }
    active.pop();
    complete.insert(memory_id);
    None
}

fn validate_list_owner_changes(
    memory: &[SemanticMemory],
    edges: &[MigrationEdge],
) -> Result<(), String> {
    for edge in edges {
        let destination = &memory[edge.destination.memory_id.as_usize()];
        match edge.transfer_kind {
            MigrationTransferKind::List => {
                let source_ids = edge
                    .source_leaves
                    .iter()
                    .map(|source| source.memory_id)
                    .collect::<BTreeSet<_>>();
                if source_ids.len() != 1 {
                    return Err(
                        "whole-list migration cannot merge or partition independent list owners"
                            .to_owned(),
                    );
                }
                let source = &memory[source_ids.iter().next().unwrap().as_usize()];
                if source.identity.kind != SemanticMemoryKind::ListOwner
                    || edge.source_leaves.len() != 1
                    || edge.source_leaves[0].semantic_path != source.identity.semantic_path
                    || edge.destination.semantic_path != destination.identity.semantic_path
                {
                    return Err(format!(
                        "list owner migration `{}` -> `{}` must transfer each whole list",
                        source.identity.semantic_path, destination.identity.semantic_path
                    ));
                }
                if !migration_type_preserves_source_shape(&source.data_type, &destination.data_type)
                {
                    return Err(format!(
                        "incompatible list owner change `{}` -> `{}` changes authoritative row type; rename the owner and row schema in separate versions",
                        source.identity.semantic_path, destination.identity.semantic_path
                    ));
                }
                let source_indexed = indexed_schema(memory, &source.identity.semantic_path);
                let destination_indexed =
                    indexed_schema(memory, &destination.identity.semantic_path);
                if source_indexed != destination_indexed {
                    return Err(format!(
                        "incompatible list owner change `{}` -> `{}` also changes indexed row authority; perform the row-field migration in another schema version",
                        source.identity.semantic_path, destination.identity.semantic_path
                    ));
                }
            }
            MigrationTransferKind::IndexedField => {
                for source_id in edge
                    .source_leaves
                    .iter()
                    .map(|source| source.memory_id)
                    .collect::<BTreeSet<_>>()
                {
                    let source = &memory[source_id.as_usize()];
                    if source.identity.kind != SemanticMemoryKind::IndexedField
                        || source.identity.owner_path != destination.identity.owner_path
                    {
                        return Err(format!(
                            "indexed-field migration `{}` -> `{}` must remain under the same stable list owner",
                            source.identity.semantic_path, destination.identity.semantic_path
                        ));
                    }
                }
            }
            MigrationTransferKind::Scalar => {
                if edge.source_leaves.iter().any(|source| {
                    memory[source.memory_id.as_usize()].identity.kind
                        != SemanticMemoryKind::RootScalar
                }) {
                    return Err(format!(
                        "scalar migration destination `{}` cannot consume list or indexed authority",
                        destination.identity.semantic_path
                    ));
                }
            }
        }
    }
    Ok(())
}

fn migration_type_preserves_source_shape(
    source: &SemanticDataType,
    destination: &SemanticDataType,
) -> bool {
    match (source, destination) {
        (
            SemanticDataType::Record { fields: source, .. },
            SemanticDataType::Record {
                fields: destination,
                ..
            },
        ) => source.iter().all(|source_field| {
            destination.iter().any(|destination_field| {
                source_field.name == destination_field.name
                    && migration_type_preserves_source_shape(
                        &source_field.data_type,
                        &destination_field.data_type,
                    )
            })
        }),
        (
            SemanticDataType::Variant { variants: source },
            SemanticDataType::Variant {
                variants: destination,
            },
        ) => source.iter().all(|source_variant| {
            destination.iter().any(|destination_variant| {
                source_variant.tag == destination_variant.tag
                    && source_variant.fields.iter().all(|source_field| {
                        destination_variant.fields.iter().any(|destination_field| {
                            source_field.name == destination_field.name
                                && migration_type_preserves_source_shape(
                                    &source_field.data_type,
                                    &destination_field.data_type,
                                )
                        })
                    })
            })
        }),
        (SemanticDataType::List { item: source }, SemanticDataType::List { item: destination }) => {
            migration_type_preserves_source_shape(source, destination)
        }
        _ => source == destination,
    }
}

fn indexed_schema(
    memory: &[SemanticMemory],
    list_owner: &str,
) -> BTreeMap<String, SemanticDataType> {
    memory
        .iter()
        .filter(|memory| {
            memory.identity.kind == SemanticMemoryKind::IndexedField
                && memory.identity.owner_path == list_owner
        })
        .map(|memory| {
            (
                memory
                    .identity
                    .semantic_path
                    .rsplit_once('.')
                    .map(|(_, field)| field)
                    .unwrap_or(&memory.identity.semantic_path)
                    .to_owned(),
                memory.data_type.clone(),
            )
        })
        .collect()
}
