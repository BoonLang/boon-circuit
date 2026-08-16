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
    if fields.source_bundle_digest_v1 != project.source_bundle_digest_v1() {
        return Err(CheckedMetadataError::new(
            "checked rows and parser snapshot have different source bundle digests",
        ));
    }
    let syntax = crate::TypecheckSyntaxProgram::UnitNative(project.clone());
    let expr_type_table = ExprTypeTable {
        entries: fields
            .expressions
            .iter()
            .map(|expression| ExprTypeEntry {
                expr_id: expression.id.0 as usize,
                flow_type: expression.flow_type.clone(),
            })
            .collect(),
    };
    let unknown_type_count = expr_type_table
        .entries
        .iter()
        .filter(|entry| matches!(entry.flow_type.ty, Type::Unknown))
        .count();
    let mut unresolved = BTreeSet::new();
    for entry in &expr_type_table.entries {
        crate::collect_type_vars(&entry.flow_type.ty, &mut unresolved);
    }
    let source_payload_shape_table = crate::checked_source_payload_shape_table(fields);
    let function_type_table = owner_function_type_table(&fields.callables);
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
