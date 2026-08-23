//! Parser-to-checked lowering metadata reconstruction.
//!
//! This pass consumes completed checked rows and parser-issued identities. It
//! performs no inference and is intentionally available without the
//! test-gated legacy owner solver.

use boon_checked::*;
use boon_parser::ProjectSyntaxSnapshot;
use boon_syntax::{AstStatement, AstStatementKind};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedMetadataError {
    message: String,
}

impl CheckedMetadataError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CheckedMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CheckedMetadataError {}

fn owner_function_type_table(callables: &[CheckedCallableSignature]) -> FunctionTypeTable {
    let mut entries = callables
        .iter()
        .filter(|callable| callable.kind == CheckedCallableKind::User)
        .map(|callable| FunctionTypeEntry {
            callable: callable.decl_id,
            name: callable.name.clone(),
            parameters: callable
                .parameters
                .iter()
                .map(|parameter| FunctionTypeParameterEntry {
                    formal: parameter.decl_id,
                    ordinal: parameter.ordinal,
                    name: parameter.name.clone(),
                    flow_type: parameter.flow_type.clone(),
                })
                .collect(),
            result: callable.result.clone(),
            effect: callable.effect,
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.callable);
    FunctionTypeTable { entries }
}

fn runtime_function_type_table(
    syntax: &crate::TypecheckSyntaxProgram,
    fields: &CheckedProgramFields,
) -> Result<FunctionTypeTable, String> {
    fn collect_functions<'a>(statements: &'a [AstStatement], target: &mut Vec<&'a AstStatement>) {
        for statement in statements {
            if matches!(statement.kind, AstStatementKind::Function { .. }) {
                target.push(statement);
            }
            collect_functions(&statement.children, target);
        }
    }

    fn merge_effect(target: &mut CheckedEffectSummary, source: CheckedEffectSummary) {
        target.reads_state |= source.reads_state;
        target.writes_state |= source.writes_state;
        target.emits_source |= source.emits_source;
        target.invokes_host |= source.invokes_host;
    }

    let statements = fields
        .statements
        .iter()
        .map(|statement| (statement.id, statement))
        .collect::<BTreeMap<_, _>>();
    let declarations = fields
        .declarations
        .iter()
        .map(|declaration| (declaration.id, declaration))
        .collect::<BTreeMap<_, _>>();
    let scopes = fields
        .scopes
        .iter()
        .map(|scope| (scope.id, scope))
        .collect::<BTreeMap<_, _>>();
    let function_body_scopes = fields
        .declarations
        .iter()
        .filter(|declaration| declaration.kind == CheckedDeclarationKind::Function)
        .filter_map(|declaration| declaration.body_scope)
        .collect::<BTreeSet<_>>();
    let mut parameters = BTreeMap::new();
    for declaration in fields.declarations.iter().filter(|declaration| {
        function_body_scopes.contains(&declaration.scope_id)
            && matches!(
                declaration.kind,
                CheckedDeclarationKind::ValueParameter | CheckedDeclarationKind::OutParameter
            )
    }) {
        if parameters
            .insert(
                (declaration.scope_id, declaration.name.as_str()),
                declaration,
            )
            .is_some()
        {
            return Err(format!(
                "checked function scope {} repeats parameter `{}`",
                declaration.scope_id.0, declaration.name,
            ));
        }
    }

    let mut function_owner_by_scope = BTreeMap::new();
    for scope in &fields.scopes {
        let mut current = scope.id;
        let mut visited = BTreeSet::new();
        let owner = loop {
            if !visited.insert(current) {
                return Err(format!(
                    "checked lexical scope {} contains a parent cycle",
                    scope.id.0,
                ));
            }
            let current_scope = scopes
                .get(&current)
                .ok_or_else(|| format!("checked program has no lexical scope {}", current.0))?;
            if current_scope.kind == CheckedScopeKind::Function {
                break current_scope.owner;
            }
            let Some(parent) = current_scope.parent else {
                break None;
            };
            current = parent;
        };
        function_owner_by_scope.insert(scope.id, owner);
    }
    let mut effects = BTreeMap::<DeclId, CheckedEffectSummary>::new();
    for expression in &fields.expressions {
        if let Some(owner) = function_owner_by_scope
            .get(&expression.scope_id)
            .copied()
            .flatten()
        {
            merge_effect(effects.entry(owner).or_default(), expression.effect);
        }
    }

    let mut syntax_functions = Vec::new();
    for statements in syntax.root_statement_units() {
        collect_functions(statements, &mut syntax_functions);
    }
    let mut entries = Vec::with_capacity(syntax_functions.len());
    for statement in syntax_functions {
        let AstStatementKind::Function {
            name,
            parameters: syntax_parameters,
        } = &statement.kind
        else {
            unreachable!("function inventory contains only function statements")
        };
        let statement_id = syntax.checked_statement_id(statement.id);
        let checked_statement = statements.get(&statement_id).ok_or_else(|| {
            format!(
                "function `{name}` has no checked statement {}",
                statement_id.0,
            )
        })?;
        let CheckedStatementKind::Function { declaration } = checked_statement.kind else {
            return Err(format!(
                "function `{name}` checked statement {} has a non-function kind",
                statement_id.0,
            ));
        };
        let callable = declarations.get(&declaration).ok_or_else(|| {
            format!(
                "function `{name}` references missing declaration {}",
                declaration.0,
            )
        })?;
        if callable.kind != CheckedDeclarationKind::Function || callable.name != *name {
            return Err(format!(
                "function `{name}` disagrees with checked declaration {}",
                declaration.0,
            ));
        }
        let body_scope = callable.body_scope.ok_or_else(|| {
            format!(
                "function `{name}` declaration {} has no body scope",
                declaration.0,
            )
        })?;
        let mut checked_parameters = Vec::with_capacity(syntax_parameters.len());
        for parameter in syntax_parameters {
            let checked = parameters
                .get(&(body_scope, parameter.name.as_str()))
                .ok_or_else(|| {
                    format!(
                        "function `{name}` has no checked parameter `{}` in scope {}",
                        parameter.name, body_scope.0,
                    )
                })?;
            let expected_kind = match parameter.kind {
                boon_syntax::AstParameterKind::Value => CheckedDeclarationKind::ValueParameter,
                boon_syntax::AstParameterKind::Out => CheckedDeclarationKind::OutParameter,
            };
            if checked.kind != expected_kind {
                return Err(format!(
                    "function `{name}` parameter `{}` has the wrong checked kind",
                    parameter.name,
                ));
            }
            checked_parameters.push(FunctionTypeParameterEntry {
                formal: checked.id,
                ordinal: parameter.ordinal,
                name: checked.name.clone(),
                flow_type: checked.flow_type.clone(),
            });
        }
        let Type::Function { args, result } = &callable.flow_type.ty else {
            return Err(format!(
                "function `{name}` declaration {} has no function type",
                declaration.0,
            ));
        };
        let mut expected_args = args.iter();
        let value_args_match = checked_parameters
            .iter()
            .zip(syntax_parameters)
            .filter(|(_, parameter)| parameter.kind == boon_syntax::AstParameterKind::Value)
            .all(|(parameter, _)| {
                expected_args
                    .next()
                    .is_some_and(|expected| expected == &parameter.flow_type.ty)
            })
            && expected_args.next().is_none();
        if !value_args_match {
            return Err(format!(
                "function `{name}` parameter declarations disagree with its checked function type",
            ));
        }
        entries.push(FunctionTypeEntry {
            callable: declaration,
            name: callable.name.clone(),
            parameters: checked_parameters,
            result: result.as_ref().clone(),
            effect: effects.get(&declaration).copied().unwrap_or_default(),
        });
    }
    entries.sort_by_key(|entry| entry.callable);
    Ok(FunctionTypeTable { entries })
}

fn owner_expr_type_table(fields: &CheckedProgramFields) -> ExprTypeTable {
    ExprTypeTable {
        entries: fields
            .expressions
            .iter()
            .map(|expression| ExprTypeEntry {
                expr_id: expression.id.0 as usize,
                flow_type: expression.flow_type.clone(),
            })
            .collect(),
    }
}

pub(crate) fn checked_report_type_tables(
    syntax: &crate::TypecheckSyntaxProgram,
    fields: &CheckedProgramFields,
) -> Result<(ExprTypeTable, FunctionTypeTable), String> {
    Ok((
        owner_expr_type_table(fields),
        runtime_function_type_table(syntax, fields)?,
    ))
}

/// Materialize report-only rich type tables on explicit editor/error demand.
///
/// Rich/editor construction keeps expression and callable types in its checked
/// rows. Ordinary RuntimePacked construction omits callable rows entirely and
/// therefore never calls this compatibility helper; an editor demand projects
/// those rows from the retained packed kernel snapshot first. Building either
/// report table eagerly would clone recursive types solely so a successful
/// runtime request could immediately discard the duplicate projection.
pub fn populate_checked_report_type_tables(fields: &mut CheckedProgramFields) {
    if fields.lowering_metadata.expr_type_table.entries.is_empty() {
        fields.lowering_metadata.expr_type_table = owner_expr_type_table(fields);
    }
    if fields
        .lowering_metadata
        .function_type_table
        .entries
        .is_empty()
    {
        fields.lowering_metadata.function_type_table = owner_function_type_table(&fields.callables);
    }
}

fn owner_named_value_type_table(
    syntax: &crate::TypecheckSyntaxProgram,
    fields: &CheckedProgramFields,
) -> Result<NamedValueTypeTable, String> {
    let mut syntax_sites = BTreeMap::new();
    crate::collect_canonical_named_value_sites(
        syntax.statements(),
        &mut Vec::new(),
        &mut syntax_sites,
    );
    for sites in syntax_sites.values_mut() {
        for site in sites {
            *site = syntax.checked_statement_id(*site).0 as usize;
        }
    }
    let mut table = NamedValueTypeTable {
        checked_statement_sites: Vec::new(),
        entries: syntax_sites
            .keys()
            .cloned()
            .map(|path| NamedValueTypeEntry {
                path,
                origins: Vec::new(),
                flow_type: crate::unknown_flow_type(),
            })
            .collect(),
    };
    let lookup = crate::CheckedProgramLookup::new(fields);
    crate::refresh_named_value_types_from_checked_program(&mut table, &syntax_sites, &lookup)?;
    Ok(table)
}

fn syntax_statement_by_checked_id(
    syntax: &crate::TypecheckSyntaxProgram,
    id: CheckedStatementId,
) -> Option<&AstStatement> {
    fn find<'a>(
        statements: &'a [AstStatement],
        syntax: &crate::TypecheckSyntaxProgram,
        id: CheckedStatementId,
    ) -> Option<&'a AstStatement> {
        for statement in statements {
            if syntax.checked_statement_id(statement.id) == id {
                return Some(statement);
            }
            if let Some(found) = find(&statement.children, syntax, id) {
                return Some(found);
            }
        }
        None
    }

    syntax
        .root_statement_units()
        .find_map(|statements| find(statements, syntax, id))
}

fn owner_output_root_types(
    syntax: &crate::TypecheckSyntaxProgram,
    fields: &CheckedProgramFields,
) -> Result<Vec<OutputRootTypeEntry>, CheckedMetadataError> {
    let lookup = crate::CheckedProgramLookup::new(fields);
    let containers = syntax
        .statements()
        .iter()
        .filter(|statement| {
            matches!(&statement.kind, AstStatementKind::Field { name } if name == "outputs")
        })
        .collect::<Vec<_>>();
    let Some(container) = containers.first() else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    let mut names = BTreeSet::new();
    for source in &container.children {
        if matches!(
            source.kind,
            AstStatementKind::Hold { field: Some(_), .. }
                | AstStatementKind::Source { field: Some(_), .. }
        ) {
            continue;
        }
        let name = match &source.kind {
            AstStatementKind::Field { name }
            | AstStatementKind::List {
                field: Some(name), ..
            } => name,
            _ => continue,
        };
        if !names.insert(name.clone()) {
            continue;
        }
        let statement_id = syntax.checked_statement_id(source.id);
        let checked_statement = lookup.unique_statement(statement_id).ok_or_else(|| {
            CheckedMetadataError::new(format!(
                "output root `{}` has no exact checked statement",
                name
            ))
        })?;
        let declaration = match checked_statement.kind {
            CheckedStatementKind::Field { declaration }
            | CheckedStatementKind::List {
                declaration: Some(declaration),
                ..
            } => declaration,
            _ => {
                return Err(CheckedMetadataError::new(format!(
                    "output root `{}` has no exact checked declaration identity",
                    name
                )));
            }
        };
        let ty = checked_statement
            .value
            .and_then(|value| lookup.expressions.get(&value).copied())
            .map(|expression| {
                expression.flush_type.as_ref().map_or_else(
                    || expression.flow_type.ty.clone(),
                    |flush_type| crate::union_structural_type(&expression.flow_type.ty, flush_type),
                )
            })
            .unwrap_or(Type::Unknown);
        entries.push(OutputRootTypeEntry {
            name: name.clone(),
            declaration,
            statement: statement_id,
            value: checked_statement.value,
            ty,
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn owner_render_slot_table(
    syntax: &crate::TypecheckSyntaxProgram,
    fields: &CheckedProgramFields,
) -> Result<RenderSlotTable, CheckedMetadataError> {
    let registry = crate::RenderContractRegistry::default();
    let mut slots = Vec::new();
    for statement in fields
        .statements
        .iter()
        .filter(|statement| statement.value_use == CheckedValueUse::RenderSlot)
    {
        let source = syntax_statement_by_checked_id(syntax, statement.id).ok_or_else(|| {
            CheckedMetadataError::new("render slot has no exact syntax statement")
        })?;
        let slot_name = match &source.kind {
            AstStatementKind::Field { name }
            | AstStatementKind::Source {
                field: Some(name), ..
            }
            | AstStatementKind::List {
                field: Some(name), ..
            } => name.clone(),
            _ => "items".to_owned(),
        };
        let value_expr_id = statement.value.map(|value| value.0 as usize);
        let actual_type = statement
            .value
            .and_then(|value| fields.expressions.get(value.0 as usize))
            .map(|expression| expression.flow_type.ty.clone())
            .unwrap_or_else(|| {
                if matches!(slot_name.as_str(), "items" | "children") {
                    Type::List(Type::shared(crate::open_object_type()))
                } else {
                    crate::open_object_type()
                }
            });
        let mut diagnostics = Vec::new();
        if let Some(value) = statement.value
            && !registry.slot_accepts_type(&slot_name, &actual_type)
        {
            let expression = fields.expressions.get(value.0 as usize).ok_or_else(|| {
                CheckedMetadataError::new(
                    "render slot references a missing checked value expression",
                )
            })?;
            diagnostics.push(TypeDiagnostic {
                severity: DiagnosticSeverity::Error,
                line: expression.span.line,
                start: expression.span.start,
                end: expression.span.end,
                message: if crate::type_contains_absence(&actual_type) {
                    "`SKIP` cannot be used as a render value".to_owned()
                } else {
                    crate::render_slot_type_error(&slot_name, &actual_type)
                },
            });
        }
        slots.push(RenderSlot {
            slot_statement_id: statement.id.0 as usize,
            slot_name: slot_name.clone(),
            expected_contract: registry.slot_contract(&slot_name).to_owned(),
            value_expr_id,
            actual_type,
            diagnostics,
        });
    }
    slots.sort_by_key(|slot| slot.slot_statement_id);
    Ok(RenderSlotTable { slots })
}

fn owner_host_port_table(
    syntax: &crate::TypecheckSyntaxProgram,
    fields: &CheckedProgramFields,
    outputs: &[OutputRootTypeEntry],
    diagnostics: &[TypeDiagnostic],
) -> Result<HostPortTable, CheckedMetadataError> {
    let source_paths = fields
        .sources
        .iter()
        .filter_map(|source| fields.semantic_path(&source.path))
        .collect::<BTreeSet<_>>();
    let source_lookup = crate::SourcePayloadPathLookup::new(&source_paths);
    let (host_ports, _) = crate::host_port_table(syntax, &source_lookup);
    let table = crate::resolve_checked_host_port_table(&host_ports, fields, outputs);
    crate::validate_checked_host_port_source_payload_types(fields, &host_ports).map_err(
        |error| {
            CheckedMetadataError::new(format!(
                "checked host source payload differs from its parser-owned host contract: {error}"
            ))
        },
    )?;
    match table {
        Ok(table) => Ok(table),
        Err(error)
            if diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == error) =>
        {
            Ok(HostPortTable::default())
        }
        Err(error) => Err(CheckedMetadataError::new(format!(
            "checked host-port relocation unexpectedly failed: {error}"
        ))),
    }
}

/// Reconstruct every lowering table from parser-issued identities and a
/// completed checked-row graph.
///
/// This pass performs no inference and consumes no owner-solver diagnostic
/// facts. It is the shared transition seam for both compatibility assembly and
/// the dense kernel's final checked construction.
pub fn derive_project_checked_lowering_metadata(
    project: &ProjectSyntaxSnapshot,
    fields: &CheckedProgramFields,
    diagnostics: &[TypeDiagnostic],
) -> Result<CheckedProgramLoweringMetadata, CheckedMetadataError> {
    derive_project_checked_lowering_metadata_with_report_types(project, fields, diagnostics, true)
}

/// Derive the operational lowering metadata used by successful runtime
/// compilation without cloning the report-only expression and function type
/// tables. An editor request or a failing runtime check can materialize those
/// tables later through [`populate_checked_report_type_tables`].
pub fn derive_project_runtime_checked_lowering_metadata(
    project: &ProjectSyntaxSnapshot,
    fields: &CheckedProgramFields,
    diagnostics: &[TypeDiagnostic],
) -> Result<CheckedProgramLoweringMetadata, CheckedMetadataError> {
    derive_project_checked_lowering_metadata_with_report_types(project, fields, diagnostics, false)
}

fn derive_project_checked_lowering_metadata_with_report_types(
    project: &ProjectSyntaxSnapshot,
    fields: &CheckedProgramFields,
    diagnostics: &[TypeDiagnostic],
    retain_report_type_tables: bool,
) -> Result<CheckedProgramLoweringMetadata, CheckedMetadataError> {
    if fields.source_bundle_digest_v1 != project.source_bundle_digest_v1() {
        return Err(CheckedMetadataError::new(
            "checked rows and parser snapshot have different source bundle digests",
        ));
    }
    let syntax = crate::TypecheckSyntaxProgram::UnitNative(project.clone());
    let unknown_type_count = fields
        .expressions
        .iter()
        .filter(|expression| matches!(expression.flow_type.ty, Type::Unknown))
        .count();
    let mut unresolved = BTreeSet::new();
    for expression in &fields.expressions {
        crate::collect_type_vars(&expression.flow_type.ty, &mut unresolved);
    }
    let expr_type_table = retain_report_type_tables
        .then(|| owner_expr_type_table(fields))
        .unwrap_or_default();
    let source_payload_shape_table = crate::checked_source_payload_shape_table(fields);
    let function_type_table = retain_report_type_tables
        .then(|| owner_function_type_table(&fields.callables))
        .unwrap_or_default();
    let named_value_type_table =
        owner_named_value_type_table(&syntax, fields).map_err(CheckedMetadataError::new)?;
    let output_root_types = owner_output_root_types(&syntax, fields)?;
    let render_slot_table = owner_render_slot_table(&syntax, fields)?;
    let host_port_table = owner_host_port_table(&syntax, fields, &output_root_types, diagnostics)?;
    let lookup = crate::CheckedProgramLookup::new(fields);
    crate::validate_structural_lowering_metadata(
        fields,
        &lookup,
        &source_payload_shape_table,
        &function_type_table,
        &named_value_type_table,
        &output_root_types,
        &host_port_table,
        retain_report_type_tables,
    )
    .map_err(CheckedMetadataError::new)?;
    Ok(CheckedProgramLoweringMetadata {
        source_units: project
            .source_layouts()
            .iter()
            .map(|unit| CheckedSourceUnitMetadata {
                path: unit.path.clone(),
                module: unit.module.clone(),
                start_line: unit.start_line,
                line_count: unit.line_count,
            })
            .collect(),
        original_source_expression_count: project.expression_count(),
        source_payload_shape_table,
        host_port_table,
        output_root_types,
        expr_type_table,
        function_type_table,
        named_value_type_table,
        render_slot_table,
        checked_expression_count: fields.expressions.len(),
        dynamic_fallback_count: unknown_type_count + unresolved.len(),
        diagnostics: diagnostics.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_function_table_reconstruction_matches_exact_editor_rows() {
        let fixtures = [
            (
                "nested",
                r#"
container: BLOCK {
    FUNCTION nested(value) {
        value + 1
    }
}
"#,
            ),
            (
                "structured-out",
                r#"
FUNCTION wrapped(list, row: OUT, new) {
    list |> List/map(item: row, new: [value: new])
}
rows: LIST { [value: 1] }
result: rows |> wrapped(row, new: row.value + 1)
"#,
            ),
            (
                "host-effect",
                r#"
FUNCTION random_byte() {
    Random/bytes(byte_count: 1)
}
value: random_byte()
"#,
            ),
            (
                "event-flow",
                r#"
FUNCTION pulses(count) {
    count |> Stream/pulses()
}
value: pulses(count: 3)
"#,
            ),
        ];

        for (label, source) in fixtures {
            let project = boon_parser::parse_project_syntax(
                "app/RUN.bn",
                [("app/RUN.bn".to_owned(), source.to_owned())],
            )
            .unwrap_or_else(|error| panic!("{label} fixture must parse: {error}"));
            let output = crate::check_project_editor_program_profiled_with_external_types(
                &project,
                &ExternalTypeEnvironment::default(),
            )
            .0;
            assert!(
                !output.report.has_errors(),
                "{label} editor fixture diagnostics: {:#?}",
                output.report.diagnostics,
            );
            let fields = output
                .checked_program_fields()
                .unwrap_or_else(|| panic!("{label} editor fixture has no checked fields"));
            let syntax = crate::TypecheckSyntaxProgram::UnitNative(project);
            let reconstructed = runtime_function_type_table(&syntax, fields)
                .unwrap_or_else(|error| panic!("{label} reconstruction failed: {error}"));
            assert_eq!(
                reconstructed, fields.lowering_metadata.function_type_table,
                "{label} compact reconstruction differs from exact EditorRich rows",
            );
        }
    }
}
