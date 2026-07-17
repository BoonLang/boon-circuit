use super::{
    ExprId, FieldDef, FieldId, ListId, ListMemory, RowScope, ScopeId, SemanticMemoryId, StateCell,
    StateId, is_output_registry_value_path, statement_expr_ids_recursive,
};
use boon_parser::{
    AstDrainPath, AstExpr, AstExprKind, AstStatement, AstStatementKind, ParsedProgram,
};
use boon_typecheck::{BytesType, Type, TypeCheckReport, Variant};
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
    program: &ParsedProgram,
    fields: &[FieldDef],
    row_scopes: &[RowScope],
    state_cells: &[StateCell],
    lists: &[ListMemory],
    typecheck_report: &TypeCheckReport,
) -> Result<(Vec<SemanticMemory>, Vec<MigrationEdge>), String> {
    let migration_fields = migration_field_defs(program, fields);
    let fields = migration_fields.as_slice();
    let mut memory = build_semantic_memory(
        program,
        fields,
        row_scopes,
        state_cells,
        lists,
        typecheck_report,
    );
    let has_markers = program.expressions.iter().any(|expr| {
        matches!(
            expr.kind,
            AstExprKind::Drain { .. } | AstExprKind::Draining { .. }
        )
    });
    if !has_markers {
        return Ok((memory, Vec::new()));
    }

    let parents = expression_parents(&program.expressions);
    associate_draining_markers(program, fields, &parents, &mut memory)?;
    let mut drains = {
        let authority = AuthorityIndex::new(&memory);
        collect_drains(
            program,
            fields,
            state_cells,
            lists,
            typecheck_report,
            &parents,
            &authority,
        )?
    };
    refine_identity_destination_types(&mut memory, &mut drains);
    validate_pairs_and_coverage(&memory, &drains)?;
    let authority = AuthorityIndex::new(&memory);
    validate_no_ordinary_draining_reads(program, fields, &memory, &authority)?;
    let edges = lower_edges(program, typecheck_report, &memory, &drains)?;
    validate_migration_cycles(&memory, &edges)?;
    validate_list_owner_changes(&memory, &edges)?;
    Ok((memory, edges))
}

fn build_semantic_memory(
    program: &ParsedProgram,
    fields: &[FieldDef],
    row_scopes: &[RowScope],
    state_cells: &[StateCell],
    lists: &[ListMemory],
    report: &TypeCheckReport,
) -> Vec<SemanticMemory> {
    let list_paths = lists
        .iter()
        .map(|list| (list.id, semantic_list_path(list, fields)))
        .collect::<BTreeMap<_, _>>();
    let list_types = lists
        .iter()
        .map(|list| {
            (
                list.id,
                semantic_type_for_list(program, list, fields, report),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut memory = Vec::with_capacity(state_cells.len() + lists.len());

    for state in state_cells {
        let field = fields.iter().find(|field| field.path == state.path);
        let field_id = field.and_then(|field| {
            fields
                .iter()
                .position(|candidate| candidate.path == field.path)
                .map(FieldId)
        });
        let mut data_type = semantic_type_for_state(state, field, report);
        let (kind, owner_path, runtime_backing) = if let Some(scope_id) = state.scope_id {
            let row_scope = row_scopes.iter().find(|scope| scope.id == scope_id);
            let list = row_scope.and_then(|scope| {
                lists
                    .iter()
                    .find(|list| list.row_scope_id == Some(scope_id) || list.name == scope.list)
            });
            if let Some(indexed_type) = row_scope
                .zip(list)
                .and_then(|(scope, list)| {
                    list_types
                        .get(&list.id)
                        .and_then(|list_type| indexed_state_type(state, scope, list_type))
                })
                .cloned()
                && type_quality(&indexed_type) > type_quality(&data_type)
            {
                data_type = indexed_type;
            }
            let owner_path = list
                .and_then(|list| list_paths.get(&list.id))
                .cloned()
                .or_else(|| row_scope.map(|scope| scope.list.clone()))
                .unwrap_or_else(|| parent_path(&state.path));
            (
                SemanticMemoryKind::IndexedField,
                owner_path,
                SemanticMemoryRuntimeBacking::IndexedState {
                    state_id: state.id,
                    field_id,
                    scope_id,
                    list_id: list.map(|list| list.id),
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
        let identity = SemanticMemoryIdentity {
            canonical_module: canonical_module_for_line(program, state.source_line),
            owner_path,
            semantic_path: state.path.clone(),
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
                        "typecheck report has no recursive list type for `{}`",
                        list.name
                    ),
                });
        let identity = SemanticMemoryIdentity {
            canonical_module: canonical_module_for_line(
                program,
                program
                    .list_memories
                    .iter()
                    .find(|candidate| candidate.name == list.name)
                    .map(|candidate| candidate.line)
                    .unwrap_or_default(),
            ),
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
    memory
}

fn indexed_state_type<'a>(
    state: &StateCell,
    row_scope: &RowScope,
    list_type: &'a SemanticDataType,
) -> Option<&'a SemanticDataType> {
    let SemanticDataType::List { item } = list_type else {
        return None;
    };
    let relative_path = state
        .path
        .strip_prefix(&format!("{}.", row_scope.row_scope))
        .unwrap_or_else(|| {
            state
                .path
                .split_once('.')
                .map(|(_, suffix)| suffix)
                .unwrap_or(state.path.as_str())
        });
    let parts = relative_path
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    semantic_type_at_path(item, &parts)
}

fn semantic_type_for_state(
    state: &StateCell,
    field: Option<&FieldDef>,
    report: &TypeCheckReport,
) -> SemanticDataType {
    let mut expr_ids = Vec::new();
    if let Some(field) = field {
        expr_ids.extend(field.ast_exprs.iter().filter_map(|expr| match &expr.kind {
            AstExprKind::Hold { .. } => Some(expr.id),
            AstExprKind::Pipe { op, .. } if op == "HOLD" => Some(expr.id),
            _ => None,
        }));
    }
    expr_ids.extend(state.initial_expr_id.map(|id| id.as_usize()));
    best_report_type(expr_ids, report).unwrap_or_else(|| SemanticDataType::Unknown {
        reason: format!(
            "typecheck report has no recursive type for state `{}`",
            state.path
        ),
    })
}

fn semantic_type_for_list(
    program: &ParsedProgram,
    list: &ListMemory,
    fields: &[FieldDef],
    report: &TypeCheckReport,
) -> SemanticDataType {
    let mut candidates = fields
        .iter()
        .filter(|field| field.path == list.name || field.path.ends_with(&format!(".{}", list.name)))
        .flat_map(|field| field.ast_exprs.iter().map(|expr| expr.id))
        .collect::<Vec<_>>();
    if let Some(statement) = semantic_list_statement(program, list) {
        candidates.extend(statement_expr_ids_recursive(
            statement,
            &program.expressions,
        ));
    }
    candidates.sort_unstable();
    candidates.dedup();
    let template_functions = program
        .row_scope_functions
        .iter()
        .filter(|scope| scope.list == list.name)
        .map(|scope| scope.function.as_str())
        .collect::<BTreeSet<_>>();
    let mut data_types = candidates
        .iter()
        .filter_map(|expr_id| checked_type_for_expr(report, *expr_id))
        .map(semantic_data_type)
        .filter(|data_type| matches!(data_type, SemanticDataType::List { .. }))
        .collect::<Vec<_>>();
    data_types.extend(
        report
            .list_map_bindings
            .iter()
            .filter(|binding| {
                (candidates.contains(&binding.map_expr_id)
                    || binding
                        .template_function
                        .as_deref()
                        .is_some_and(|function| template_functions.contains(function)))
                    && matches!(
                        binding.result_kind,
                        boon_typecheck::ListMapResultKind::RuntimeValue
                    )
            })
            .map(|binding| semantic_data_type(&binding.result_type))
            .filter(|data_type| matches!(data_type, SemanticDataType::List { .. })),
    );
    data_types.extend(candidates.iter().filter_map(|expr_id| {
        let AstExprKind::ListLiteral { items, .. } = &program.expressions.get(*expr_id)?.kind
        else {
            return None;
        };
        let mut item_types = items
            .iter()
            .filter_map(|item| checked_type_for_expr(report, *item))
            .map(semantic_data_type);
        let item_type = item_types.next()?;
        item_types
            .all(|candidate| candidate == item_type)
            .then(|| SemanticDataType::List {
                item: Box::new(item_type),
            })
    }));
    data_types
        .into_iter()
        .max_by_key(semantic_list_type_quality)
        .unwrap_or_else(|| SemanticDataType::Unknown {
            reason: format!(
                "typecheck report has no recursive list type for `{}`",
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

fn semantic_list_statement<'a>(
    program: &'a ParsedProgram,
    list: &ListMemory,
) -> Option<&'a AstStatement> {
    let line = program
        .list_memories
        .iter()
        .find(|candidate| candidate.name == list.name)
        .map(|candidate| candidate.line)?;
    fn find(statements: &[AstStatement], line: usize) -> Option<&AstStatement> {
        statements.iter().find_map(|statement| {
            let matches =
                statement.line == line && matches!(&statement.kind, AstStatementKind::List { .. });
            matches
                .then_some(statement)
                .or_else(|| find(&statement.children, line))
        })
    }
    find(&program.ast.statements, line)
}

fn best_report_type(
    expr_ids: impl IntoIterator<Item = usize>,
    report: &TypeCheckReport,
) -> Option<SemanticDataType> {
    best_report_type_matching(expr_ids, report, |_| true)
}

fn best_report_type_matching(
    expr_ids: impl IntoIterator<Item = usize>,
    report: &TypeCheckReport,
    predicate: impl Fn(&SemanticDataType) -> bool,
) -> Option<SemanticDataType> {
    expr_ids
        .into_iter()
        .filter_map(|expr_id| checked_type_for_expr(report, expr_id))
        .map(semantic_data_type)
        .filter(|data_type| predicate(data_type))
        .max_by_key(type_quality)
}

fn checked_type_for_expr(report: &TypeCheckReport, expr_id: usize) -> Option<&Type> {
    report
        .expr_type_table
        .entries
        .iter()
        .find(|entry| entry.expr_id == expr_id)
        .map(|entry| &entry.flow_type.ty)
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
        Type::VariantSet(variants) if variants_are_bool(variants) => SemanticDataType::Bool,
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

fn variants_are_bool(variants: &[Variant]) -> bool {
    variants.len() == 2
        && variants
            .iter()
            .filter_map(|variant| match variant {
                Variant::Tag(tag) => Some(tag.as_str()),
                Variant::Tagged { .. } => None,
            })
            .collect::<BTreeSet<_>>()
            == BTreeSet::from(["False", "True"])
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

fn semantic_list_path(list: &ListMemory, fields: &[FieldDef]) -> String {
    let mut candidates = fields
        .iter()
        .filter(|field| field.path == list.name || field.path.ends_with(&format!(".{}", list.name)))
        .map(|field| field.path.clone())
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| (path.matches('.').count(), path.clone()));
    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| list.name.clone())
}

fn canonical_module_for_line(program: &ParsedProgram, line: usize) -> String {
    program
        .files
        .iter()
        .find(|file| {
            let line_count = file.source.lines().count().max(1);
            line >= file.start_line && line < file.start_line + line_count
        })
        .and_then(|file| file.module.clone())
        .unwrap_or_else(|| "$root".to_owned())
}

fn parent_path(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(parent, _)| parent.to_owned())
        .filter(|parent| !parent.is_empty())
        .unwrap_or_else(|| "$root".to_owned())
}

#[derive(Clone, Debug)]
struct ResolvedRegion {
    memory_id: SemanticMemoryId,
    region_path: String,
    leaf_indexes: Vec<usize>,
    data_type: SemanticDataType,
}

struct AuthorityIndex<'a> {
    memory: &'a [SemanticMemory],
}

impl<'a> AuthorityIndex<'a> {
    fn new(memory: &'a [SemanticMemory]) -> Self {
        Self { memory }
    }

    fn resolve_drain_path(
        &self,
        path: &AstDrainPath,
        context: Option<&FieldDef>,
    ) -> Result<ResolvedRegion, String> {
        let display = drain_path_text(path);
        let candidates = drain_path_candidates(path, context);
        let mut resolved = candidates
            .iter()
            .flat_map(|candidate| self.resolve_canonical_path(candidate))
            .collect::<Vec<_>>();
        resolved.sort_by_key(|region| (region.memory_id, region.region_path.clone()));
        resolved.dedup_by(|left, right| {
            left.memory_id == right.memory_id && left.region_path == right.region_path
        });
        match resolved.as_slice() {
            [resolved] => Ok(resolved.clone()),
            [] => Err(format!(
                "DRAIN path `{display}` does not resolve to semantic authority; derived fields, sources, and ordinary values cannot be drained"
            )),
            _ => Err(format!(
                "DRAIN path `{display}` is ambiguous across semantic authority owners: {}",
                resolved
                    .iter()
                    .map(|region| {
                        self.memory[region.memory_id.as_usize()]
                            .identity
                            .semantic_path
                            .as_str()
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    fn resolve_reference_path(
        &self,
        parts: &[String],
        context: Option<&FieldDef>,
    ) -> Vec<ResolvedRegion> {
        let raw = parts.join(".");
        let mut candidates = vec![raw.clone()];
        if let Some(context) = context
            && !context.parent_path.is_empty()
        {
            candidates.insert(0, format!("{}.{}", context.parent_path, raw));
        }
        let mut resolved = candidates
            .iter()
            .flat_map(|candidate| self.resolve_canonical_path(candidate))
            .collect::<Vec<_>>();
        resolved.sort_by_key(|region| (region.memory_id, region.region_path.clone()));
        resolved.dedup_by(|left, right| {
            left.memory_id == right.memory_id && left.region_path == right.region_path
        });
        resolved
    }

    fn resolve_field_destination(&self, path: &str) -> Option<ResolvedRegion> {
        let mut resolved = self.resolve_canonical_path(path);
        resolved.sort_by_key(|region| {
            std::cmp::Reverse(
                self.memory[region.memory_id.as_usize()]
                    .identity
                    .semantic_path
                    .len(),
            )
        });
        resolved.into_iter().next()
    }

    fn resolve_canonical_path(&self, path: &str) -> Vec<ResolvedRegion> {
        self.memory
            .iter()
            .filter_map(|memory| region_for_memory_path(memory, path))
            .collect()
    }
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

fn drain_path_candidates(path: &AstDrainPath, context: Option<&FieldDef>) -> Vec<String> {
    let raw = match path {
        AstDrainPath::Binding { name } => name.clone(),
        AstDrainPath::Field { binding, fields } => std::iter::once(binding.as_str())
            .chain(fields.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("."),
        AstDrainPath::Passed { fields } => fields.join("."),
    };
    let mut candidates = Vec::new();
    if let Some(context) = context
        && !context.parent_path.is_empty()
        && !matches!(path, AstDrainPath::Passed { .. })
    {
        candidates.push(format!("{}.{}", context.parent_path, raw));
    }
    candidates.push(raw);
    candidates.sort();
    candidates.dedup();
    candidates
}

fn drain_path_text(path: &AstDrainPath) -> String {
    match path {
        AstDrainPath::Binding { name } => name.clone(),
        AstDrainPath::Field { binding, fields } => std::iter::once(binding.as_str())
            .chain(fields.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("."),
        AstDrainPath::Passed { fields } => format!("PASSED.{}", fields.join(".")),
    }
}

fn field_contexts_for_expr(expr_id: usize, fields: &[FieldDef]) -> Vec<&FieldDef> {
    let mut contexts = fields
        .iter()
        .filter(|field| field.ast_exprs.iter().any(|expr| expr.id == expr_id))
        .collect::<Vec<_>>();
    let expr_line = contexts.iter().find_map(|field| {
        field
            .ast_exprs
            .iter()
            .find(|expr| expr.id == expr_id)
            .map(|expr| expr.line)
    });
    if let Some(expr_line) = expr_line {
        let physical = contexts
            .iter()
            .copied()
            .filter(|field| field.ast_items.iter().any(|item| item.line == expr_line))
            .collect::<Vec<_>>();
        if !physical.is_empty() {
            contexts = physical;
        }
    }
    let max_depth = contexts
        .iter()
        .map(|field| field.path.matches('.').count())
        .max();
    if let Some(max_depth) = max_depth {
        contexts.retain(|field| field.path.matches('.').count() == max_depth);
    }
    contexts.sort_by(|left, right| left.path.cmp(&right.path));
    contexts.dedup_by(|left, right| left.path == right.path);
    contexts
}

fn associate_draining_markers(
    program: &ParsedProgram,
    fields: &[FieldDef],
    parents: &BTreeMap<usize, Vec<ParentLink>>,
    memory: &mut [SemanticMemory],
) -> Result<(), String> {
    let marker_owners = {
        let authority = AuthorityIndex::new(memory);
        let mut marker_owners = Vec::new();
        for marker in program
            .expressions
            .iter()
            .filter(|expr| matches!(expr.kind, AstExprKind::Draining { .. }))
        {
            let contexts = field_contexts_for_expr(marker.id, fields);
            if contexts.iter().any(|context| {
                !draining_marker_is_terminal(marker.id, context, parents, &program.expressions)
            }) {
                return Err(format!(
                    "DRAINING at line {} must be terminal in its semantic authority pipeline",
                    marker.line
                ));
            }
            let mut owners = contexts
                .into_iter()
                .filter_map(|context| authority.resolve_field_destination(&context.path))
                .filter(|region| {
                    authority.memory[region.memory_id.as_usize()]
                        .identity
                        .semantic_path
                        == region.region_path
                })
                .map(|region| region.memory_id)
                .collect::<Vec<_>>();
            owners.sort();
            owners.dedup();
            if owners.is_empty() {
                return Err(format!(
                    "DRAINING at line {} is not attached to a semantic authority owner",
                    marker.line
                ));
            }
            marker_owners.push((marker, owners));
        }
        marker_owners
    };
    for (marker, owners) in marker_owners {
        for owner in owners {
            let candidate = &mut memory[owner.as_usize()];
            if let SemanticMemoryStatus::Draining { marker_expr_id } = candidate.status {
                return Err(format!(
                    "semantic authority `{}` has multiple DRAINING markers (expressions {} and {})",
                    candidate.identity.semantic_path, marker_expr_id, marker.id
                ));
            }
            candidate.status = SemanticMemoryStatus::Draining {
                marker_expr_id: ExprId(marker.id),
            };
        }
    }
    Ok(())
}

fn draining_marker_is_terminal(
    marker_id: usize,
    context: &FieldDef,
    parents: &BTreeMap<usize, Vec<ParentLink>>,
    expressions: &[AstExpr],
) -> bool {
    let context_exprs = context
        .ast_exprs
        .iter()
        .map(|expr| expr.id)
        .collect::<BTreeSet<_>>();
    if parents
        .get(&marker_id)
        .into_iter()
        .flatten()
        .any(|parent| context_exprs.contains(&parent.parent))
    {
        return false;
    }
    let Some(pipeline) = statement_pipeline_expr_ids(&context.statement, expressions) else {
        return true;
    };
    pipeline.iter().rposition(|expr_id| {
        *expr_id == marker_id || expr_tree_contains(*expr_id, marker_id, expressions)
    }) == Some(pipeline.len() - 1)
}

#[derive(Clone, Debug)]
struct DrainUse {
    drain_expr_id: ExprId,
    source: ResolvedRegion,
    destination: ResolvedRegion,
    expression_root: ExprId,
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

#[derive(Clone, Debug)]
enum ParentRole {
    Plain,
    RecordField(String),
    ListItem,
    AuthorityWrapper,
    DrainingWrapper,
}

#[derive(Clone, Debug)]
struct ParentLink {
    parent: usize,
    role: ParentRole,
}

fn expression_parents(expressions: &[AstExpr]) -> BTreeMap<usize, Vec<ParentLink>> {
    let mut parents = BTreeMap::<usize, Vec<ParentLink>>::new();
    for expr in expressions {
        let mut add = |child, role| {
            parents.entry(child).or_default().push(ParentLink {
                parent: expr.id,
                role,
            });
        };
        match &expr.kind {
            AstExprKind::Call { args, .. } => {
                for arg in args {
                    add(arg.value, ParentRole::Plain);
                }
            }
            AstExprKind::Pipe { input, args, .. } => {
                add(*input, ParentRole::Plain);
                for arg in args {
                    add(arg.value, ParentRole::Plain);
                }
            }
            AstExprKind::Draining { input } => add(*input, ParentRole::DrainingWrapper),
            AstExprKind::Hold { initial, .. } => add(*initial, ParentRole::AuthorityWrapper),
            AstExprKind::When { input } => add(*input, ParentRole::AuthorityWrapper),
            AstExprKind::Then { input, output } => {
                add(*input, ParentRole::AuthorityWrapper);
                if let Some(output) = output {
                    add(*output, ParentRole::AuthorityWrapper);
                }
            }
            AstExprKind::Infix { left, right, .. } => {
                add(*left, ParentRole::Plain);
                add(*right, ParentRole::Plain);
            }
            AstExprKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    add(*output, ParentRole::AuthorityWrapper);
                }
            }
            AstExprKind::Object(fields)
            | AstExprKind::Record(fields)
            | AstExprKind::TaggedObject { fields, .. } => {
                for field in fields {
                    add(field.value, ParentRole::RecordField(field.name.clone()));
                }
            }
            AstExprKind::ListLiteral { items, .. } => {
                for item in items {
                    add(*item, ParentRole::ListItem);
                }
            }
            AstExprKind::BytesLiteral { items, .. } => {
                for item in items {
                    add(*item, ParentRole::Plain);
                }
            }
            AstExprKind::Identifier(_)
            | AstExprKind::Path(_)
            | AstExprKind::Drain { .. }
            | AstExprKind::StringLiteral(_)
            | AstExprKind::TextLiteral(_)
            | AstExprKind::Number(_)
            | AstExprKind::ByteLiteral { .. }
            | AstExprKind::Bool(_)
            | AstExprKind::Enum(_)
            | AstExprKind::Tag(_)
            | AstExprKind::Source
            | AstExprKind::Latest
            | AstExprKind::Delimiter
            | AstExprKind::Unknown(_) => {}
        }
    }
    parents
}

fn collect_drains(
    program: &ParsedProgram,
    fields: &[FieldDef],
    state_cells: &[StateCell],
    lists: &[ListMemory],
    report: &TypeCheckReport,
    parents: &BTreeMap<usize, Vec<ParentLink>>,
    authority: &AuthorityIndex<'_>,
) -> Result<Vec<DrainUse>, String> {
    let mut drains = Vec::new();
    for drain in program
        .expressions
        .iter()
        .filter(|expr| matches!(expr.kind, AstExprKind::Drain { .. }))
    {
        let AstExprKind::Drain { path } = &drain.kind else {
            unreachable!();
        };
        let contexts = field_contexts_for_expr(drain.id, fields);
        if contexts.is_empty() {
            return Err(format!(
                "DRAIN at line {} is not attached to a named destination field",
                drain.line
            ));
        }
        let mut lowered_for_destination = BTreeSet::new();
        for context in contexts {
            let source = authority.resolve_drain_path(path, Some(context))?;
            let Some(mut destination) = authority.resolve_field_destination(&context.path) else {
                continue;
            };
            if destination.region_path
                == authority.memory[destination.memory_id.as_usize()]
                    .identity
                    .semantic_path
                && let Some(suffix) =
                    record_destination_suffix(drain.id, context, parents, &program.expressions)
            {
                let nested = format!("{}.{}", destination.region_path, suffix);
                if let Some(region) = authority
                    .resolve_canonical_path(&nested)
                    .into_iter()
                    .find(|region| region.memory_id == destination.memory_id)
                {
                    destination = region;
                }
            }
            if !lowered_for_destination
                .insert((destination.memory_id, destination.region_path.clone()))
            {
                continue;
            }
            let (expression_root, pipeline) = if authority.memory[destination.memory_id.as_usize()]
                .identity
                .kind
                == SemanticMemoryKind::ListOwner
            {
                (drain.id, vec![drain.id])
            } else {
                migration_expression_root(drain.id, context, parents, &program.expressions)
            };
            validate_drain_initializes_destination(
                drain,
                context,
                destination.memory_id,
                state_cells,
                lists,
                &program.expressions,
            )?;
            let identity = expression_root == drain.id;
            if !identity && checked_type_for_expr(report, expression_root).is_none() {
                return Err(format!(
                    "migration transform rooted at expression {expression_root} has no typecheck entry"
                ));
            }
            drains.push(DrainUse {
                drain_expr_id: ExprId(drain.id),
                source,
                destination,
                expression_root: ExprId(expression_root),
                pipeline: pipeline.into_iter().map(ExprId).collect(),
                identity,
            });
        }
        if !drains
            .iter()
            .any(|candidate| candidate.drain_expr_id == ExprId(drain.id))
        {
            return Err(format!(
                "DRAIN at line {} does not initialize semantic authority; destinations must be scalar, indexed-field, or list memory",
                drain.line
            ));
        }
    }
    Ok(drains)
}

fn record_destination_suffix(
    expr_id: usize,
    context: &FieldDef,
    parents: &BTreeMap<usize, Vec<ParentLink>>,
    expressions: &[AstExpr],
) -> Option<String> {
    let context_exprs = context
        .ast_exprs
        .iter()
        .map(|expr| expr.id)
        .collect::<BTreeSet<_>>();
    let mut current = expr_id;
    let mut fields = Vec::new();
    let mut seen = BTreeSet::new();
    while seen.insert(current) {
        let link = parents
            .get(&current)
            .into_iter()
            .flat_map(|links| links.iter())
            .find(|link| context_exprs.contains(&link.parent))?;
        match &link.role {
            ParentRole::RecordField(field) => fields.push(field.clone()),
            ParentRole::ListItem | ParentRole::AuthorityWrapper | ParentRole::DrainingWrapper => {
                break;
            }
            ParentRole::Plain => {}
        }
        current = link.parent;
        if expressions.get(current).is_none() {
            break;
        }
    }
    fields.reverse();
    (!fields.is_empty()).then(|| fields.join("."))
}

fn migration_expression_root(
    drain_expr_id: usize,
    context: &FieldDef,
    parents: &BTreeMap<usize, Vec<ParentLink>>,
    expressions: &[AstExpr],
) -> (usize, Vec<usize>) {
    let context_exprs = context
        .ast_exprs
        .iter()
        .map(|expr| expr.id)
        .collect::<BTreeSet<_>>();
    let mut current = drain_expr_id;
    let mut seen = BTreeSet::new();
    while seen.insert(current) {
        let Some(link) = parents
            .get(&current)
            .into_iter()
            .flat_map(|links| links.iter())
            .find(|link| context_exprs.contains(&link.parent))
        else {
            break;
        };
        if !matches!(link.role, ParentRole::Plain) {
            break;
        }
        current = link.parent;
    }

    let pipeline = statement_pipeline_expr_ids(&context.statement, expressions)
        .unwrap_or_else(|| vec![current]);
    let mut relevant = Vec::new();
    let mut started = false;
    let mut root = current;
    for expr_id in pipeline {
        if !started {
            started = expr_tree_contains(expr_id, drain_expr_id, expressions)
                || expr_id == current
                || expr_tree_contains(expr_id, current, expressions);
            if !started {
                continue;
            }
        }
        let Some(expr) = expressions.get(expr_id) else {
            break;
        };
        if matches!(
            expr.kind,
            AstExprKind::Hold { .. } | AstExprKind::Draining { .. } | AstExprKind::Latest
        ) || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if matches!(op.as_str(), "HOLD" | "LATEST"))
        {
            break;
        }
        relevant.push(expr_id);
        root = expr_id;
    }
    if relevant.is_empty() {
        relevant.push(current);
    }
    (root, relevant)
}

fn validate_drain_initializes_destination(
    drain: &AstExpr,
    context: &FieldDef,
    destination_memory: SemanticMemoryId,
    state_cells: &[StateCell],
    lists: &[ListMemory],
    expressions: &[AstExpr],
) -> Result<(), String> {
    let is_state = state_cells.iter().any(|state| {
        matches!(state.id, StateId(id) if id == destination_memory.as_usize())
            || state.path == context.path
    });
    let is_list = lists.iter().any(|list| {
        list.name == context.path || context.path.ends_with(&format!(".{}", list.name))
    });
    if is_list {
        return Ok(());
    }
    if !is_state {
        // Nested fields of a record-valued root state are initialized with their owner.
        return Ok(());
    }
    let pipeline = statement_pipeline_expr_ids(&context.statement, expressions);
    if let Some(pipeline) = pipeline {
        let drain_position = pipeline.iter().position(|expr_id| {
            *expr_id == drain.id || expr_tree_contains(*expr_id, drain.id, expressions)
        });
        let authority_position = pipeline.iter().position(|expr_id| {
            expressions.get(*expr_id).is_some_and(|expr| {
                matches!(expr.kind, AstExprKind::Hold { .. } | AstExprKind::Latest)
                    || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "HOLD" || op == "LATEST")
            })
        });
        if let (Some(drain_position), Some(authority_position)) =
            (drain_position, authority_position)
            && drain_position <= authority_position
        {
            return Ok(());
        }
    }
    if context.ast_exprs.iter().any(|expr| {
        matches!(&expr.kind, AstExprKind::Hold { initial, .. } if expr_tree_contains(*initial, drain.id, expressions))
            || matches!(&expr.kind, AstExprKind::Pipe { input, op, .. } if op == "HOLD" && expr_tree_contains(*input, drain.id, expressions))
    }) {
        return Ok(());
    }
    Err(format!(
        "DRAIN at line {} conflicts with existing destination authority `{}`; DRAIN must be part of its initializer",
        drain.line, context.path
    ))
}

fn statement_pipeline_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<Vec<usize>> {
    let mut expr_ids = Vec::new();
    if let Some(expr_id) = statement.expr {
        expr_ids.push(expr_id);
        collect_pipeline_continuations(statement, expressions, &mut expr_ids);
    } else {
        expr_ids.extend(statement.children.iter().filter_map(|child| {
            matches!(
                child.kind,
                AstStatementKind::Expression
                    | AstStatementKind::Hold { .. }
                    | AstStatementKind::List { field: None, .. }
            )
            .then_some(child.expr)
            .flatten()
        }));
    }
    (expr_ids.len() > 1
        && expr_ids
            .iter()
            .skip(1)
            .all(|expr_id| expr_is_pipeline_continuation(*expr_id, expressions)))
    .then_some(expr_ids)
}

fn migration_field_defs(program: &ParsedProgram, fields: &[FieldDef]) -> Vec<FieldDef> {
    let mut output = fields.to_vec();
    let items = program.ast.semantic_parser_items().collect::<Vec<_>>();
    collect_structural_migration_fields(
        &program.ast.statements,
        &mut Vec::new(),
        program,
        &items,
        &mut output,
    );
    output
}

fn collect_structural_migration_fields(
    statements: &[AstStatement],
    scope: &mut Vec<String>,
    program: &ParsedProgram,
    items: &[&boon_parser::ParserItem],
    fields: &mut Vec<FieldDef>,
) {
    for statement in statements {
        let name = match &statement.kind {
            AstStatementKind::Field { name }
            | AstStatementKind::List {
                field: Some(name), ..
            }
            | AstStatementKind::Hold {
                field: Some(name), ..
            } if name != "document" && name != "scene" => Some(name.as_str()),
            _ => None,
        };
        if let Some(name) = name {
            let parent_path = scope.join(".");
            let path = if parent_path.is_empty() {
                name.to_owned()
            } else {
                format!("{parent_path}.{name}")
            };
            if !fields.iter().any(|field| field.path == path) {
                fields.push(FieldDef {
                    path: path.clone(),
                    local_name: name.to_owned(),
                    parent_path,
                    statement: statement.clone(),
                    ast_items: super::collect_statement_ast_items(statement, items),
                    ast_exprs: super::collect_statement_ast_exprs(statement, program),
                });
            }
            scope.push(name.to_owned());
            collect_structural_migration_fields(&statement.children, scope, program, items, fields);
            scope.pop();
            continue;
        }
        if !matches!(statement.kind, AstStatementKind::Function { .. }) {
            collect_structural_migration_fields(&statement.children, scope, program, items, fields);
        }
    }
}

fn collect_pipeline_continuations(
    statement: &AstStatement,
    expressions: &[AstExpr],
    expr_ids: &mut Vec<usize>,
) {
    for child in statement.children.iter().filter(|child| {
        matches!(
            child.kind,
            AstStatementKind::Expression
                | AstStatementKind::Hold { .. }
                | AstStatementKind::List { field: None, .. }
        ) && child
            .expr
            .is_some_and(|expr_id| expr_is_pipeline_continuation(expr_id, expressions))
    }) {
        if let Some(expr_id) = child.expr {
            expr_ids.push(expr_id);
        }
        collect_pipeline_continuations(child, expressions, expr_ids);
    }
}

fn expr_is_pipeline_continuation(expr_id: usize, expressions: &[AstExpr]) -> bool {
    let input = match expressions.get(expr_id).map(|expr| &expr.kind) {
        Some(AstExprKind::Pipe { input, .. })
        | Some(AstExprKind::Then { input, .. })
        | Some(AstExprKind::When { input })
        | Some(AstExprKind::Draining { input })
        | Some(AstExprKind::Hold { initial: input, .. }) => *input,
        _ => return false,
    };
    expr_chain_starts_with_placeholder(input, expressions)
}

fn expr_chain_starts_with_placeholder(expr_id: usize, expressions: &[AstExpr]) -> bool {
    match expressions.get(expr_id).map(|expr| &expr.kind) {
        Some(AstExprKind::Delimiter) => true,
        Some(AstExprKind::Unknown(tokens)) => !tokens.is_empty(),
        Some(AstExprKind::Pipe { input, .. })
        | Some(AstExprKind::Then { input, .. })
        | Some(AstExprKind::When { input })
        | Some(AstExprKind::Draining { input })
        | Some(AstExprKind::Hold { initial: input, .. }) => {
            expr_chain_starts_with_placeholder(*input, expressions)
        }
        _ => false,
    }
}

fn expr_tree_contains(root: usize, needle: usize, expressions: &[AstExpr]) -> bool {
    expr_tree_contains_seen(root, needle, expressions, &mut BTreeSet::new())
}

fn expr_tree_contains_seen(
    root: usize,
    needle: usize,
    expressions: &[AstExpr],
    seen: &mut BTreeSet<usize>,
) -> bool {
    if root == needle {
        return true;
    }
    if !seen.insert(root) {
        return false;
    }
    let Some(expr) = expressions.get(root) else {
        return false;
    };
    expr_children(expr)
        .into_iter()
        .any(|child| expr_tree_contains_seen(child, needle, expressions, seen))
}

fn expr_children(expr: &AstExpr) -> Vec<usize> {
    match &expr.kind {
        AstExprKind::Call { args, .. } => args.iter().map(|arg| arg.value).collect(),
        AstExprKind::Pipe { input, args, .. } => std::iter::once(*input)
            .chain(args.iter().map(|arg| arg.value))
            .collect(),
        AstExprKind::Draining { input }
        | AstExprKind::When { input }
        | AstExprKind::Hold { initial: input, .. } => vec![*input],
        AstExprKind::Then { input, output } => std::iter::once(*input)
            .chain(output.iter().copied())
            .collect(),
        AstExprKind::Infix { left, right, .. } => vec![*left, *right],
        AstExprKind::MatchArm { output, .. } => output.iter().copied().collect(),
        AstExprKind::Object(fields)
        | AstExprKind::Record(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            fields.iter().map(|field| field.value).collect()
        }
        AstExprKind::ListLiteral { items, .. } | AstExprKind::BytesLiteral { items, .. } => {
            items.clone()
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => Vec::new(),
    }
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
            return Err(format!(
                "DRAIN source `{}` is not marked DRAINING (missing pair)",
                drain.source.region_path
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
    program: &ParsedProgram,
    fields: &[FieldDef],
    memory: &[SemanticMemory],
    authority: &AuthorityIndex<'_>,
) -> Result<(), String> {
    let draining = memory
        .iter()
        .filter(|memory| matches!(memory.status, SemanticMemoryStatus::Draining { .. }))
        .map(|memory| memory.id)
        .collect::<BTreeSet<_>>();
    if draining.is_empty() {
        return Ok(());
    }
    let definitions = draining
        .iter()
        .map(|memory_id| {
            let memory = &memory[memory_id.as_usize()];
            let exprs = fields
                .iter()
                .filter(|field| {
                    field.path == memory.identity.semantic_path
                        || (memory.identity.kind == SemanticMemoryKind::ListOwner
                            && field
                                .path
                                .ends_with(&format!(".{}", memory.identity.semantic_path)))
                })
                .flat_map(|field| field.ast_exprs.iter().map(|expr| expr.id))
                .collect::<BTreeSet<_>>();
            (*memory_id, exprs)
        })
        .collect::<BTreeMap<_, _>>();

    for expr in &program.expressions {
        let parts = match &expr.kind {
            AstExprKind::Identifier(name) => vec![name.clone()],
            AstExprKind::Path(parts) => parts.clone(),
            _ => continue,
        };
        for context in field_contexts_for_expr(expr.id, fields)
            .into_iter()
            .map(Some)
            .chain(std::iter::once(None))
        {
            for resolved in authority.resolve_reference_path(&parts, context) {
                if !draining.contains(&resolved.memory_id)
                    || definitions
                        .get(&resolved.memory_id)
                        .is_some_and(|exprs| exprs.contains(&expr.id))
                {
                    continue;
                }
                return Err(format!(
                    "ordinary reference to DRAINING authority `{}` at line {} is not allowed; use DRAIN at the migration destination",
                    resolved.region_path, expr.line
                ));
            }
        }
    }
    Ok(())
}

fn lower_edges(
    program: &ParsedProgram,
    report: &TypeCheckReport,
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

    let functions = function_statements(&program.ast.statements);
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
            let data_type = checked_type_for_expr(report, expression_root.as_usize())
                .map(semantic_data_type)
                .ok_or_else(|| {
                    format!(
                        "migration transform expression {} has unknown type",
                        expression_root
                    )
                })?;
            ensure_closed_migration_type(
                &data_type,
                &format!("migration transform expression {expression_root}"),
            )?;
            let allowed_drains = uses
                .iter()
                .map(|drain| drain.drain_expr_id.as_usize())
                .collect::<BTreeSet<_>>();
            validate_pure_transform(program, &uses[0].pipeline, &allowed_drains, &functions)?;
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

fn function_statements(statements: &[AstStatement]) -> BTreeMap<String, &AstStatement> {
    let mut functions = BTreeMap::new();
    collect_function_statements(statements, &mut functions);
    functions
}

fn collect_function_statements<'a>(
    statements: &'a [AstStatement],
    functions: &mut BTreeMap<String, &'a AstStatement>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &statement.kind {
            functions.insert(name.clone(), statement);
        }
        collect_function_statements(&statement.children, functions);
    }
}

fn validate_pure_transform(
    program: &ParsedProgram,
    pipeline: &[ExprId],
    allowed_drains: &BTreeSet<usize>,
    functions: &BTreeMap<String, &AstStatement>,
) -> Result<(), String> {
    let mut checker = PurityChecker {
        program,
        allowed_drains,
        functions,
        active_functions: Vec::new(),
    };
    for expr_id in pipeline {
        checker.check_expr(expr_id.as_usize(), &BTreeSet::new(), true)?;
    }
    Ok(())
}

struct PurityChecker<'a> {
    program: &'a ParsedProgram,
    allowed_drains: &'a BTreeSet<usize>,
    functions: &'a BTreeMap<String, &'a AstStatement>,
    active_functions: Vec<String>,
}

impl PurityChecker<'_> {
    fn check_expr(
        &mut self,
        expr_id: usize,
        params: &BTreeSet<String>,
        allow_placeholder: bool,
    ) -> Result<(), String> {
        let expr = self.program.expressions.get(expr_id).ok_or_else(|| {
            format!("migration transform references missing expression {expr_id}")
        })?;
        match &expr.kind {
            AstExprKind::Drain { .. } if self.allowed_drains.contains(&expr.id) => Ok(()),
            AstExprKind::Drain { .. } => Err(format!(
                "migration transform at line {} contains a DRAIN owned by another destination",
                expr.line
            )),
            AstExprKind::Identifier(name) if params.contains(name) => Ok(()),
            AstExprKind::Path(parts) if parts.first().is_some_and(|root| params.contains(root)) => {
                Ok(())
            }
            AstExprKind::Identifier(name) => Err(format!(
                "migration transform at line {} reads `{name}` outside its DRAIN inputs",
                expr.line
            )),
            AstExprKind::Path(parts) => Err(format!(
                "migration transform at line {} reads `{}` outside its DRAIN inputs",
                expr.line,
                parts.join(".")
            )),
            AstExprKind::Source => Err(format!(
                "migration transform at line {} reads SOURCE data",
                expr.line
            )),
            AstExprKind::Call { function, args } => {
                for arg in args {
                    self.check_expr(arg.value, params, false)?;
                }
                self.check_call(function, expr.line)
            }
            AstExprKind::Pipe { input, op, args } => {
                self.check_expr(*input, params, true)?;
                let mut local_params = params.clone();
                let binding_arg = matches!(op.as_str(), "List/map" | "List/retain")
                    .then(|| args.first())
                    .flatten()
                    .filter(|arg| arg.name.is_none())
                    .and_then(|arg| self.program.expressions.get(arg.value))
                    .and_then(|expr| match &expr.kind {
                        AstExprKind::Identifier(name) => Some((expr.id, name.clone())),
                        _ => None,
                    });
                if let Some((_, binding)) = &binding_arg {
                    local_params.insert(binding.clone());
                }
                for arg in args {
                    if binding_arg
                        .as_ref()
                        .is_some_and(|(expr_id, _)| *expr_id == arg.value)
                    {
                        continue;
                    }
                    self.check_expr(arg.value, &local_params, false)?;
                }
                self.check_call(op, expr.line)
            }
            AstExprKind::Infix { left, right, .. } => {
                self.check_expr(*left, params, false)?;
                self.check_expr(*right, params, false)
            }
            AstExprKind::Object(fields)
            | AstExprKind::Record(fields)
            | AstExprKind::TaggedObject { fields, .. } => {
                for field in fields {
                    self.check_expr(field.value, params, false)?;
                }
                Ok(())
            }
            AstExprKind::ListLiteral { items, .. } | AstExprKind::BytesLiteral { items, .. } => {
                for item in items {
                    self.check_expr(*item, params, false)?;
                }
                Ok(())
            }
            AstExprKind::Delimiter if allow_placeholder => Ok(()),
            AstExprKind::When { input } => self.check_expr(*input, params, true),
            AstExprKind::MatchArm { output, .. } => match output {
                Some(output) => self.check_expr(*output, params, false),
                None => Ok(()),
            },
            AstExprKind::StringLiteral(_)
            | AstExprKind::TextLiteral(_)
            | AstExprKind::Number(_)
            | AstExprKind::ByteLiteral { .. }
            | AstExprKind::Bool(_)
            | AstExprKind::Enum(_)
            | AstExprKind::Tag(_) => Ok(()),
            AstExprKind::Hold { .. }
            | AstExprKind::Latest
            | AstExprKind::Then { .. }
            | AstExprKind::Draining { .. } => Err(format!(
                "migration transform at line {} uses a stateful or flow combinator",
                expr.line
            )),
            AstExprKind::Delimiter | AstExprKind::Unknown(_) => Err(format!(
                "migration transform at line {} contains an unknown expression",
                expr.line
            )),
        }
    }

    fn check_call(&mut self, function: &str, line: usize) -> Result<(), String> {
        if stateful_or_effectful_call(function) {
            return Err(format!(
                "migration transform at line {line} calls stateful, nondeterministic, or host operation `{function}`"
            ));
        }
        if pure_builtin(function) {
            return Ok(());
        }
        let Some(statement) = self.functions.get(function).copied() else {
            return Err(format!(
                "migration transform at line {line} calls unknown function `{function}`"
            ));
        };
        if self
            .active_functions
            .iter()
            .any(|active| active == function)
        {
            return Err(format!(
                "migration transform purity cannot prove recursive function `{function}` deterministic"
            ));
        }
        let AstStatementKind::Function { args, .. } = &statement.kind else {
            unreachable!();
        };
        let params = args.iter().cloned().collect::<BTreeSet<_>>();
        self.active_functions.push(function.to_owned());
        let result = self.check_function_body(statement, &params);
        self.active_functions.pop();
        result
    }

    fn check_function_body(
        &mut self,
        statement: &AstStatement,
        params: &BTreeSet<String>,
    ) -> Result<(), String> {
        for child in &statement.children {
            if matches!(child.kind, AstStatementKind::Function { .. }) {
                continue;
            }
            if let Some(expr_id) = child.expr {
                self.check_expr(expr_id, params, true)?;
            }
            self.check_function_body(child, params)?;
        }
        Ok(())
    }
}

fn stateful_or_effectful_call(function: &str) -> bool {
    matches!(
        function,
        "HOLD"
            | "LATEST"
            | "WHEN"
            | "THEN"
            | "WHILE"
            | "Bool/toggle"
            | "List/latest"
            | "List/append"
            | "List/remove"
            | "Timer/interval"
            | "Ulid/generate"
            | "File/read_bytes"
            | "File/read_text"
            | "File/write_bytes"
            | "Router/route"
            | "Router/go_to"
    ) || function.starts_with("Time/")
        || function.starts_with("Timer/")
        || function.starts_with("Random/")
        || function.starts_with("Ulid/")
        || function.starts_with("File/")
        || function.starts_with("Router/")
        || function.starts_with("Host/")
        || function.starts_with("Document/")
        || function.starts_with("Element/")
        || function.starts_with("Scene/")
        || function.starts_with("Widget/")
}

fn pure_builtin(function: &str) -> bool {
    matches!(
        function,
        "Text/empty"
            | "Text/space"
            | "Text/trim"
            | "Text/to_uppercase"
            | "Text/concat"
            | "Text/time_range_label"
            | "Text/substring"
            | "Text/is_empty"
            | "Text/is_not_empty"
            | "Text/starts_with"
            | "Text/contains"
            | "Text/all_chars_in"
            | "Text/find"
            | "Text/length"
            | "Text/to_number"
            | "Text/to_bytes"
            | "Number/add"
            | "Number/subtract"
            | "Number/min"
            | "Number/max"
            | "Number/bit_width"
            | "Number/interpolate"
            | "Number/project_width"
            | "Number/project_offset"
            | "Number/project_time"
            | "Number/to_text"
            | "Number/to_codepoint_text"
            | "Number/to_ascii_text"
            | "Bool/not"
            | "Bool/and"
            | "Bytes/length"
            | "Bytes/is_empty"
            | "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/concat"
            | "Bytes/equal"
            | "Bytes/find"
            | "Bytes/starts_with"
            | "Bytes/ends_with"
            | "Bytes/to_text"
            | "Bytes/to_hex"
            | "Bytes/to_base64"
            | "Bytes/from_hex"
            | "Bytes/from_base64"
            | "Bytes/zeros"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
            | "List/map"
            | "List/retain"
            | "List/range"
            | "List/chunk"
            | "List/filter_text_contains"
            | "List/filter_field_equal"
            | "List/filter_field_not_equal"
            | "List/move_field_first"
            | "List/move_field_last"
            | "List/find"
            | "List/find_value"
            | "List/get"
            | "List/count"
            | "List/length"
            | "List/sum"
            | "List/every"
            | "List/any"
            | "List/is_not_empty"
            | "List/join_field"
            | "Error/new"
            | "Error/text"
    ) || function.starts_with("Field/")
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
