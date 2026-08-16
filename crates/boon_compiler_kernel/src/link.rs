use crate::{
    KernelAbiContextualOperation, KernelAbiInput, KernelCallArgumentKind, KernelCallInputRole,
    KernelCallTarget, KernelCheckedSnapshot, KernelDeclarationReference, KernelExternalTarget,
    KernelLexicalBindingTarget, KernelOwnerId, KernelProjectInput, KernelScopeReference,
    KernelStatementChildReference, KernelStatementReference, KernelTypeParameterId,
    KernelValueReference, derive_kernel_call_type_substitutions,
};
use boon_checked::{
    CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1, CheckedBlockBinding, CheckedCall,
    CheckedCallEntry, CheckedCallId, CheckedCallResultPath, CheckedCallableContext,
    CheckedCallableKind, CheckedCallableSignature, CheckedContextBinding, CheckedContextFormal,
    CheckedContextScheme, CheckedContextTypeSubstitution, CheckedContextualOperation,
    CheckedDeclaration, CheckedDeclarationKind, CheckedDefinitionExecutionNodeV1,
    CheckedDefinitionExecutionTemplateV1, CheckedDefinitionSelectorV1, CheckedEffectSummary,
    CheckedEvaluationScope, CheckedExprId, CheckedExpression, CheckedExpressionKind, CheckedList,
    CheckedListId, CheckedMatchPattern, CheckedParameter, CheckedParameterKind,
    CheckedParameterRequirement, CheckedPassedAccess, CheckedPatternBinding, CheckedRecordField,
    CheckedResourceBinding, CheckedResourceProjectionRequirement,
    CheckedRuntimeFlowTermProjectionV1, CheckedScope, CheckedScopeKind, CheckedSemanticPath,
    CheckedSource, CheckedSourceId, CheckedSourceRead, CheckedSpan, CheckedState, CheckedStateId,
    CheckedStatement, CheckedStatementId, CheckedStatementKind, CheckedTextSegment,
    CheckedTypeSubstitution, CheckedValueUse, ContextFormalId, DeclId, FlowMode, FlowType,
    LexicalScopeId, ObjectShape, ProgramRole, SemanticOccurrence, SemanticOccurrenceKind, Type,
    TypeVar, Variant,
};
use boon_syntax::StableOccurrenceKey;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelCheckedRowRange {
    pub start: u32,
    pub len: u32,
}

impl KernelCheckedRowRange {
    fn resolve(self, local: u32, label: &str) -> Result<u32, KernelCheckedLinkError> {
        if local >= self.len {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked linker references {label} row {local} outside local range 0..{}",
                self.len,
            )));
        }
        self.start.checked_add(local).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel checked linker {label} row overflows the global u32 namespace"
            ))
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedDefinitionLayout {
    pub owner: KernelOwnerId,
    pub scopes: KernelCheckedRowRange,
    pub expressions: KernelCheckedRowRange,
    pub statements: KernelCheckedRowRange,
    pub declarations: KernelCheckedRowRange,
    /// Definition-local alpha ordinals relocated into the global checked
    /// `TypeVar` namespace. Later expression/call rows reuse this same range.
    pub type_variables: KernelCheckedRowRange,
    pub calls: KernelCheckedRowRange,
    pub sources: KernelCheckedRowRange,
    pub states: KernelCheckedRowRange,
    pub lists: KernelCheckedRowRange,
    pub containing_scope: LexicalScopeId,
    pub root_statement: CheckedStatementId,
    pub public_declaration: DeclId,
    pub result_expression: CheckedExprId,
    pub context_formal: Option<ContextFormalId>,
}

/// Final checked namespace owned by one referenced stable ABI callable.
///
/// The kernel allocates these rows after all definition-owned declarations,
/// keeping parser-owned IDs and compiler/library ABI IDs in disjoint dense
/// ranges. Only callables actually referenced by this checked snapshot are
/// materialized.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedAbiCallableLayout {
    pub name: Box<str>,
    pub declaration: DeclId,
    pub parameters: Box<[DeclId]>,
    pub type_variables: KernelCheckedRowRange,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelCheckedLinkTotals {
    /// Includes the single project-root scope at row zero.
    pub scopes: u32,
    pub expressions: u32,
    pub statements: u32,
    pub declarations: u32,
    pub type_variables: u32,
    pub calls: u32,
    pub user_callables: u32,
    pub abi_callables: u32,
    pub callables: u32,
    pub context_formals: u32,
    pub sources: u32,
    pub states: u32,
    pub lists: u32,
    pub resolved_references: u64,
}

/// Complete dense checked rows materialized from one kernel snapshot.
///
/// This is the single projection seam between definition-local kernel
/// artifacts and the existing checked model. Source-unit coordinate rebasing
/// and project-level semantic indexes remain orchestration concerns; callers
/// must not re-run per-row linker methods independently.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedRows {
    pub scopes: Box<[CheckedScope]>,
    pub declarations: Box<[CheckedDeclaration]>,
    pub expressions: Box<[CheckedExpression]>,
    pub statements: Box<[CheckedStatement]>,
    pub callables: Box<[CheckedCallableSignature]>,
    pub context_formals: Box<[CheckedContextFormal]>,
    pub calls: Box<[CheckedCall]>,
    /// Parser-issued structural identity for each call row at the same dense
    /// ordinal. Checked-image sealing consumes this relocation directly and
    /// never interprets compact checked expression IDs as parser slots.
    pub call_occurrences: Box<[StableOccurrenceKey]>,
    pub call_result_paths: Box<[CheckedCallResultPath]>,
    pub pattern_bindings: Box<[CheckedPatternBinding]>,
    pub resource_projection_requirements: Box<[CheckedResourceProjectionRequirement]>,
    pub sources: Box<[CheckedSource]>,
    pub states: Box<[CheckedState]>,
    pub lists: Box<[CheckedList]>,
    pub definition_execution_templates: Box<[CheckedDefinitionExecutionTemplateV1]>,
    pub runtime_flow_terms: CheckedRuntimeFlowTermProjectionV1,
    pub occurrences: Box<[SemanticOccurrence]>,
    occurrence_ranges: Box<[KernelCheckedRowRange]>,
}

impl KernelCheckedRows {
    /// Rebase every source-bearing row owned by one definition from source-unit
    /// coordinates into the project-wide checked coordinate system.
    ///
    /// Keeping this operation beside the row materializer prevents the compiler
    /// facade from maintaining one relocation loop per table and guarantees new
    /// source-bearing tables participate in the same pass.
    pub fn rebase_definition_spans(
        &mut self,
        layout: &KernelCheckedLinkLayout,
        owner: KernelOwnerId,
        start_line: usize,
        start_byte: usize,
    ) -> Result<(), KernelCheckedLinkError> {
        let definition = layout.definition(owner)?;
        for row in checked_range(definition.scopes)? {
            let scope = self.scopes.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked scope linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut scope.span,
                start_line,
                start_byte,
                &format!("kernel checked scope row {row}"),
            )?;
        }
        for declaration_id in checked_range(definition.declarations)? {
            let row = declaration_id.checked_sub(1).ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked declaration row uses reserved identity zero",
                )
            })?;
            let declaration = self.declarations.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked declaration linker references missing row {declaration_id}"
                ))
            })?;
            rebase_checked_span(
                &mut declaration.span,
                start_line,
                start_byte,
                &format!("kernel checked declaration row {declaration_id}"),
            )?;
        }
        for row in checked_range(definition.expressions)? {
            let expression = self.expressions.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked expression linker references missing row {row}"
                ))
            })?;
            rebase_checked_expression_spans(expression, start_line, start_byte)?;
        }
        for row in checked_range(definition.statements)? {
            let statement = self.statements.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked statement linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut statement.span,
                start_line,
                start_byte,
                &format!("kernel checked statement row {row}"),
            )?;
        }
        for row in checked_range(definition.sources)? {
            let source = self.sources.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked SOURCE linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut source.span,
                start_line,
                start_byte,
                &format!("kernel checked SOURCE row {row}"),
            )?;
        }
        for row in checked_range(definition.states)? {
            let state = self.states.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked state linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut state.span,
                start_line,
                start_byte,
                &format!("kernel checked state row {row}"),
            )?;
        }
        for row in checked_range(definition.lists)? {
            let list = self.lists.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked LIST linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut list.span,
                start_line,
                start_byte,
                &format!("kernel checked LIST row {row}"),
            )?;
        }
        for row in checked_range(definition.calls)? {
            let call = self.calls.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked call linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut call.span,
                start_line,
                start_byte,
                &format!("kernel checked call row {row}"),
            )?;
            if let CheckedContextBinding::Explicit { span, .. } = &mut call.context_binding {
                rebase_checked_span(
                    span,
                    start_line,
                    start_byte,
                    &format!("kernel checked call row {row} PASS"),
                )?;
            }
        }
        let occurrence_range = *self
            .occurrence_ranges
            .get(owner.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked occurrence ranges omit definition {}",
                    owner.0,
                ))
            })?;
        for row in checked_range(occurrence_range)? {
            let occurrence = self.occurrences.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked occurrence linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut occurrence.span,
                start_line,
                start_byte,
                &format!("kernel checked occurrence row {row}"),
            )?;
        }
        if let Some(callable) = self
            .callables
            .iter_mut()
            .find(|callable| callable.decl_id == definition.public_declaration)
        {
            for parameter in &mut callable.parameters {
                parameter.start = start_byte.checked_add(parameter.start).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked callable {} parameter start overflowed",
                        callable.name,
                    ))
                })?;
                parameter.end = start_byte.checked_add(parameter.end).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked callable {} parameter end overflowed",
                        callable.name,
                    ))
                })?;
            }
        }
        Ok(())
    }
}

/// One prefix-sum relocation plan for a complete kernel checked snapshot.
///
/// Every definition stays definition-local through solving. This layout is the
/// only place where those IDs become the dense global IDs consumed by
/// `boon_checked`. It also validates every cross-definition reference before
/// any rich checked row is allocated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedLinkLayout {
    definitions: Box<[KernelCheckedDefinitionLayout]>,
    abi_callables: Box<[KernelCheckedAbiCallableLayout]>,
    abi_callable_by_name: BTreeMap<Box<str>, usize>,
    definition_declarations_end: u32,
    totals: KernelCheckedLinkTotals,
}

impl KernelCheckedLinkLayout {
    pub fn new(
        project: &KernelProjectInput,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Self, KernelCheckedLinkError> {
        if project.definition_count() != snapshot.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked linker received {} project definitions and {} artifacts",
                project.definition_count(),
                snapshot.definitions.len(),
            )));
        }
        let mut totals = KernelCheckedLinkTotals::default();
        totals.scopes = 1;
        // DeclId(0) is the language-wide absent/external-identity sentinel.
        // Direct checked rows therefore begin at one even though every
        // definition keeps zero-based local declaration IDs.
        totals.declarations = 1;
        let mut definitions = Vec::with_capacity(snapshot.definitions.len());
        let mut public_declaration_authorities = Vec::with_capacity(snapshot.definitions.len());
        for (index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(index).map_err(|_| {
                KernelCheckedLinkError::new("kernel checked linker definition count exceeds u32")
            })?);
            let scopes = take_range(
                &mut totals.scopes,
                definition.presentation.scopes.len(),
                "scope",
            )?;
            let expressions = take_range(
                &mut totals.expressions,
                definition.expressions.len(),
                "expression",
            )?;
            let statements = take_range(
                &mut totals.statements,
                definition.statements.len(),
                "statement",
            )?;
            let declarations = take_range(
                &mut totals.declarations,
                definition.declarations.len(),
                "declaration",
            )?;
            let type_variable_ordinals = definition_type_variables(definition);
            if type_variable_ordinals
                .iter()
                .enumerate()
                .any(|(expected, variable)| variable.0 as usize != expected)
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has a non-dense local type-variable namespace {:?}",
                    owner.0, type_variable_ordinals,
                )));
            }
            let type_variables = take_range(
                &mut totals.type_variables,
                type_variable_ordinals.len(),
                "type variable",
            )?;
            let calls = take_range(&mut totals.calls, definition.calls.len(), "call")?;
            let sources = take_range(&mut totals.sources, definition.sources.len(), "source")?;
            let states = take_range(&mut totals.states, definition.states.len(), "state")?;
            let lists = take_range(&mut totals.lists, definition.lists.len(), "list")?;
            let linkage = definition.linkage;
            let root_statement = linkage.root_statement.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} omits its direct-linker root statement",
                    owner.0,
                ))
            })?;
            if matches!(
                definition.statements.get(root_statement.0 as usize),
                Some(crate::KernelStatementArtifact {
                    kind: crate::KernelStatementKind::Function { .. },
                    ..
                })
            ) {
                totals.user_callables = totals.user_callables.checked_add(1).ok_or_else(|| {
                    KernelCheckedLinkError::new(
                        "kernel checked linker callable namespace exceeds u32",
                    )
                })?;
            }
            let public_declaration = linkage.public_declaration.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} omits its direct-linker public declaration",
                    owner.0,
                ))
            })?;
            public_declaration_authorities.push(public_declaration);
            let result_expression = linkage.result_expression.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} omits its direct-linker result expression",
                    owner.0,
                ))
            })?;
            let context_formal = linkage
                .context_formal_ordinal
                .map(|ordinal| {
                    if ordinal as usize >= definition.formals.len() {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} context formal ordinal {ordinal} is outside its formal table",
                            owner.0,
                        )));
                    }
                    let id = ContextFormalId(totals.context_formals);
                    totals.context_formals = totals.context_formals.checked_add(1).ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "kernel checked linker context-formal namespace exceeds u32",
                        )
                    })?;
                    Ok(id)
                })
                .transpose()?;
            definitions.push(KernelCheckedDefinitionLayout {
                owner,
                scopes,
                expressions,
                statements,
                declarations,
                type_variables,
                calls,
                sources,
                states,
                lists,
                containing_scope: LexicalScopeId(0),
                root_statement: CheckedStatementId(
                    statements.resolve(root_statement.0, "root statement")?,
                ),
                // Resolved after every definition range exists because nested
                // resource definitions may delegate this authority across
                // more than one owner boundary.
                public_declaration: DeclId(0),
                result_expression: CheckedExprId(
                    expressions.resolve(result_expression.0, "result expression")?,
                ),
                context_formal,
            });
        }
        totals.callables = totals.user_callables;
        let definition_declarations_end = totals.declarations;
        let referenced_abi_callables = referenced_abi_callable_names(snapshot)?;
        let mut abi_callables = Vec::with_capacity(referenced_abi_callables.len());
        let mut abi_callable_by_name = BTreeMap::new();
        for name in referenced_abi_callables {
            let callable = project.abi().callable(&name).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked linker references ABI callable `{name}` absent from its immutable project ABI"
                ))
            })?;
            let declaration = DeclId(totals.declarations);
            totals.declarations = totals.declarations.checked_add(1).ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked linker declaration namespace exceeds u32",
                )
            })?;
            let parameter_range = take_range(
                &mut totals.declarations,
                callable.parameters.len(),
                "ABI parameter declaration",
            )?;
            let parameters = (0..parameter_range.len)
                .map(|ordinal| DeclId(parameter_range.start + ordinal))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let type_variable_ordinals = abi_callable_type_variables(callable);
            if type_variable_ordinals
                .iter()
                .enumerate()
                .any(|(expected, variable)| variable.0 as usize != expected)
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel ABI callable `{name}` has a non-dense local type-variable namespace {:?}",
                    type_variable_ordinals,
                )));
            }
            let type_variables = take_range(
                &mut totals.type_variables,
                type_variable_ordinals.len(),
                "ABI type variable",
            )?;
            let index = abi_callables.len();
            if abi_callable_by_name
                .insert(name.clone().into_boxed_str(), index)
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked linker repeats referenced ABI callable `{name}`"
                )));
            }
            abi_callables.push(KernelCheckedAbiCallableLayout {
                name: name.into_boxed_str(),
                declaration,
                parameters,
                type_variables,
            });
            totals.abi_callables = totals.abi_callables.checked_add(1).ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked linker ABI callable namespace exceeds u32",
                )
            })?;
            totals.callables = totals.callables.checked_add(1).ok_or_else(|| {
                KernelCheckedLinkError::new("kernel checked linker callable namespace exceeds u32")
            })?;
        }
        for (index, definition) in snapshot.definitions.iter().enumerate() {
            definitions[index].containing_scope = match definition.presentation.containing_scope {
                KernelScopeReference::ProjectRoot => LexicalScopeId(0),
                KernelScopeReference::Owner { owner, scope } => LexicalScopeId(
                    definitions
                        .get(owner.0 as usize)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {index} containing scope references missing owner {}",
                                owner.0,
                            ))
                        })?
                        .scopes
                        .resolve(scope.0, "containing scope")?,
                ),
                KernelScopeReference::Containing | KernelScopeReference::Local(_) => {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {index} has an unresolved containing-scope authority"
                    )));
                }
            };
        }
        let mut resolved_public_declarations = vec![None; definitions.len()];
        let mut resolving_public_declarations = vec![false; definitions.len()];
        for definition in 0..definitions.len() {
            let public_declaration = resolve_public_declaration(
                definition,
                &definitions,
                &public_declaration_authorities,
                &mut resolved_public_declarations,
                &mut resolving_public_declarations,
            )?;
            definitions[definition].public_declaration = public_declaration;
        }
        let mut layout = Self {
            definitions: definitions.into_boxed_slice(),
            abi_callables: abi_callables.into_boxed_slice(),
            abi_callable_by_name,
            definition_declarations_end,
            totals,
        };
        layout.validate_references(snapshot)?;
        Ok(layout)
    }

    /// Materialize every currently kernel-owned checked row through this one
    /// relocation plan. ABI rows are appended before calls are linked so call
    /// targets resolve in the same namespace as user definitions.
    pub fn materialize_rows(
        &self,
        project: &KernelProjectInput,
        snapshot: &KernelCheckedSnapshot,
        role: ProgramRole,
    ) -> Result<KernelCheckedRows, KernelCheckedLinkError> {
        let scopes = self.materialize_scopes(snapshot)?;
        let mut declarations = self.materialize_declarations(snapshot)?.into_vec();
        let expressions = self.materialize_expressions(snapshot)?;
        let runtime_flow_terms = self.materialize_runtime_flow_terms(snapshot)?;
        #[cfg(test)]
        {
            let replay =
                CheckedRuntimeFlowTermProjectionV1::derive_from_checked_expressions(&expressions)
                    .map_err(KernelCheckedLinkError::new)?;
            if runtime_flow_terms != replay {
                return Err(KernelCheckedLinkError::new(
                    "kernel direct runtime flow-term handoff differs from rich checked replay",
                ));
            }
        }
        let statements = self.materialize_statements(snapshot)?;
        let sources = self.materialize_sources(snapshot)?;
        let states = self.materialize_states(snapshot)?;
        let lists = self.materialize_lists(snapshot)?;
        let (user_callables, context_formals) = self.materialize_user_callables(snapshot, role)?;
        let mut callables = user_callables.into_vec();
        let (abi_callables, abi_declarations) = self.materialize_abi_callables(project.abi())?;
        callables.extend(abi_callables);
        declarations.extend(abi_declarations);
        let (calls, call_occurrences) =
            self.materialize_calls(project, snapshot, &callables, &declarations)?;
        let call_result_paths =
            self.materialize_call_result_paths(&declarations, &callables, &expressions, &calls)?;
        let pattern_bindings = self.materialize_pattern_bindings(snapshot)?;
        let resource_projection_requirements = checked_resource_projection_requirements(
            &declarations,
            &callables,
            &calls,
            &expressions,
            &sources,
        );
        let (occurrences, occurrence_ranges) =
            self.materialize_occurrences(snapshot, &declarations, &expressions, &calls)?;
        let definition_execution_templates = self.materialize_definition_execution_templates(
            snapshot,
            &scopes,
            &declarations,
            &statements,
            &calls,
        )?;
        Ok(KernelCheckedRows {
            scopes,
            declarations: declarations.into_boxed_slice(),
            expressions,
            statements,
            callables: callables.into_boxed_slice(),
            context_formals,
            calls,
            call_occurrences,
            call_result_paths,
            pattern_bindings,
            resource_projection_requirements,
            sources,
            states,
            lists,
            definition_execution_templates,
            runtime_flow_terms,
            occurrences,
            occurrence_ranges,
        })
    }

    fn materialize_runtime_flow_terms(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<CheckedRuntimeFlowTermProjectionV1, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "runtime flow-term handoff")?;
        let mut digests = vec![None; self.totals.expressions as usize];
        for (owner_index, (definition, layout)) in snapshot
            .definitions
            .iter()
            .zip(self.definitions.iter())
            .enumerate()
        {
            if definition.flow_terms().expressions.len() != definition.expressions.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner_index} has {} expression term roots for {} expressions",
                    definition.flow_terms().expressions.len(),
                    definition.expressions.len()
                )));
            }
            for (local, flow) in definition.flow_terms().expressions.iter().enumerate() {
                let local = u32::try_from(local).map_err(|_| {
                    KernelCheckedLinkError::new(
                        "kernel definition expression term count exceeds u32",
                    )
                })?;
                let global = layout
                    .expressions
                    .resolve(local, "runtime flow-term expression")?
                    as usize;
                let slot = digests.get_mut(global).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel runtime flow-term expression {global} exceeds its dense table"
                    ))
                })?;
                if slot.replace(flow.runtime_erased_digest).is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel runtime flow-term expression {global} is published twice"
                    )));
                }
            }
        }
        let digests = digests
            .into_iter()
            .enumerate()
            .map(|(expression, digest)| {
                digest.ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel runtime flow-term handoff omits expression {expression}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CheckedRuntimeFlowTermProjectionV1::from_runtime_flow_digests(digests))
    }

    /// Publish one dependency-first execution template directly from the
    /// definition artifacts and this layout's final checked relocations.
    ///
    /// This is intentionally part of the same linker pass that creates the
    /// final checked authority rows. Compact expression/shape artifacts supply
    /// the graph, while already-linked declarations, statements, and calls
    /// supply the few execution identities whose semantics live at those row
    /// boundaries. No downstream pass reconstructs a whole-program graph from
    /// the completed rich checked image.
    pub fn materialize_definition_execution_templates(
        &self,
        snapshot: &KernelCheckedSnapshot,
        linked_scopes: &[CheckedScope],
        linked_declarations: &[CheckedDeclaration],
        linked_statements: &[CheckedStatement],
        linked_calls: &[CheckedCall],
    ) -> Result<Box<[CheckedDefinitionExecutionTemplateV1]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "definition execution template")?;

        let mut call_by_expression = BTreeMap::new();
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "definition execution template call")?;
            for (ordinal, call) in definition.calls.iter().enumerate() {
                let ordinal = u32::try_from(ordinal).map_err(|_| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} call count exceeds u32",
                        owner.0
                    ))
                })?;
                if call_by_expression
                    .insert((owner, call.expression), ordinal)
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats a call for expression {}",
                        owner.0, call.expression.0
                    )));
                }
            }
        }
        let linked_call_by_id = linked_calls
            .iter()
            .map(|call| (call.id, call))
            .collect::<BTreeMap<_, _>>();
        if linked_call_by_id.len() != linked_calls.len() {
            return Err(KernelCheckedLinkError::new(
                "kernel definition templates received duplicate linked call IDs",
            ));
        }
        let read_provider_callables = definition_template_read_provider_callables(
            snapshot,
            self,
            linked_scopes,
            linked_declarations,
        )?;
        let mut call_dependencies = BTreeMap::<
            (KernelOwnerId, crate::KernelExpressionId),
            Vec<(KernelOwnerId, crate::KernelExpressionId)>,
        >::new();
        for (key @ (owner, _), ordinal) in &call_by_expression {
            let call_id = self.call(*owner, *ordinal)?;
            let call = linked_call_by_id.get(&call_id).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition template references missing linked call {}",
                    call_id.0,
                ))
            })?;
            let consumed = call
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    CheckedCallEntry::Input { value, .. } => Some(*value),
                    CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => None,
                })
                .chain(call.context_binding.explicit().map(|(value, _)| value))
                .collect::<BTreeSet<_>>();
            let artifact = snapshot.definitions[owner.0 as usize]
                .calls
                .get(*ordinal as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} template references missing call ordinal {}",
                        owner.0, ordinal,
                    ))
                })?;
            let mut dependencies = Vec::new();
            let mut matched = BTreeSet::new();
            for input in &artifact.inputs {
                let dependency = definition_template_value(snapshot, *owner, input.value)?;
                let linked =
                    self.expression(dependency.0, KernelValueReference::Local(dependency.1))?;
                if consumed.contains(&linked) {
                    matched.insert(linked);
                    dependencies.push(dependency);
                }
            }
            if matched != consumed {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} template call {} cannot match consumed values {:?} to compact inputs {:?}",
                    owner.0, call_id.0, consumed, matched,
                )));
            }
            call_dependencies.insert(*key, dependencies);
        }
        let statement_child_dependencies =
            definition_template_statement_child_dependencies(snapshot, self, linked_statements)?;

        let mut templates = Vec::new();
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "definition execution template")?;
            let Some(root_statement) = definition.linkage.root_statement else {
                continue;
            };
            let Some(statement) = definition
                .statements
                .get(root_statement.0 as usize)
                .filter(|statement| statement.id == root_statement)
            else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} template references missing root statement {}",
                    owner.0, root_statement.0
                )));
            };
            if !matches!(statement.kind, crate::KernelStatementKind::Function { .. }) {
                continue;
            }
            let result = definition.linkage.result_expression.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} has no result expression",
                    owner.0
                ))
            })?;
            let root = (owner, result);
            let callable = self.definition(owner)?.public_declaration;
            let mut state = BTreeMap::<(KernelOwnerId, crate::KernelExpressionId), u8>::new();
            let mut dependencies = BTreeMap::<
                (KernelOwnerId, crate::KernelExpressionId),
                Vec<(KernelOwnerId, crate::KernelExpressionId)>,
            >::new();
            let mut nodes = Vec::new();
            let mut calls = Vec::new();
            let mut pending = vec![(root, false)];
            while let Some((key @ (node_owner, expression_id), exiting)) = pending.pop() {
                let node_definition =
                    snapshot
                        .definitions
                        .get(node_owner.0 as usize)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition template references missing owner {}",
                                node_owner.0
                            ))
                        })?;
                let expression = node_definition
                    .expressions
                    .get(expression_id.0 as usize)
                    .filter(|expression| expression.id == expression_id)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition template references missing expression {}:{}",
                            node_owner.0, expression_id.0
                        ))
                    })?;
                if exiting {
                    if state.get(&key).copied() == Some(2) {
                        continue;
                    }
                    state.insert(key, 2);
                    let local_dependencies = dependencies.remove(&key).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition template lost dependencies for expression {}:{}",
                            node_owner.0, expression_id.0
                        ))
                    })?;
                    let checked_dependencies = local_dependencies
                        .iter()
                        .map(|(dependency_owner, dependency)| {
                            self.expression(
                                *dependency_owner,
                                KernelValueReference::Local(*dependency),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let call = call_by_expression
                        .get(&key)
                        .copied()
                        .map(|ordinal| self.call(node_owner, ordinal))
                        .transpose()?;
                    if let Some(call) = call {
                        calls.push(call);
                    }
                    let selector =
                        definition_template_selector(snapshot, node_owner, expression, self)?;
                    nodes.push(CheckedDefinitionExecutionNodeV1 {
                        expression: self
                            .expression(node_owner, KernelValueReference::Local(expression_id))?,
                        dependencies: checked_dependencies,
                        call,
                        selector,
                    });
                    continue;
                }
                match state.get(&key).copied().unwrap_or(0) {
                    2 | 1 => continue,
                    _ => {
                        state.insert(key, 1);
                    }
                }
                let mut relocated_dependencies = definition_template_dependencies(
                    snapshot,
                    &statement_child_dependencies,
                    &call_dependencies,
                    &read_provider_callables,
                    callable,
                    node_owner,
                    expression,
                )?
                .into_iter()
                .map(|dependency @ (dependency_owner, expression)| {
                    self.expression(dependency_owner, KernelValueReference::Local(expression))
                        .map(|relocated| (relocated, dependency))
                })
                .collect::<Result<Vec<_>, _>>()?;
                relocated_dependencies.sort_unstable_by_key(|(relocated, _)| relocated.0);
                relocated_dependencies.dedup_by_key(|(relocated, _)| *relocated);
                let node_dependencies = relocated_dependencies
                    .into_iter()
                    .map(|(_, dependency)| dependency)
                    .collect::<Vec<_>>();
                dependencies.insert(key, node_dependencies.clone());
                pending.push((key, true));
                pending.extend(
                    node_dependencies
                        .into_iter()
                        .rev()
                        .map(|dependency| (dependency, false)),
                );
            }
            calls.sort_unstable_by_key(|call| call.0);
            calls.dedup();
            templates.push(CheckedDefinitionExecutionTemplateV1 {
                schema: CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1.to_owned(),
                callable,
                result: self.expression(owner, KernelValueReference::Local(result))?,
                nodes,
                calls,
            });
        }
        templates.sort_unstable_by_key(|template| template.callable.0);
        Ok(templates.into_boxed_slice())
    }

    /// Link pattern-binding declaration authority directly from the exact
    /// match-arm execution shape retained by each definition artifact.
    ///
    /// The selector is intentionally not rediscovered from a surrounding WHEN
    /// expression: static arm pruning can remove that structural edge while
    /// the binding still owns its authored selector occurrence.
    pub fn materialize_pattern_bindings(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedPatternBinding]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "pattern binding")?;
        let mut bindings = Vec::new();
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "pattern binding")?;
            for shape in &definition.execution_shapes {
                let crate::KernelExecutionShapeArtifact::MatchArm {
                    expression,
                    selector,
                    bindings: arm_bindings,
                } = shape
                else {
                    continue;
                };
                let arm = definition
                    .expressions
                    .get(expression.0 as usize)
                    .filter(|arm| arm.id == *expression)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} match-arm shape references missing expression {}",
                            owner.0, expression.0,
                        ))
                    })?;
                let crate::KernelOwnerNodeKind::MatchArm { pattern } = &arm.kind else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression {} has a match-arm shape but kind {:?}",
                        owner.0, expression.0, arm.kind,
                    )));
                };
                for (ordinal, binding) in arm_bindings.iter().enumerate() {
                    let declaration = definition
                        .declarations
                        .get(binding.0 as usize)
                        .filter(|declaration| declaration.id == *binding)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} match arm {} references missing binding declaration {}",
                                owner.0, expression.0, binding.0,
                            ))
                        })?;
                    if !matches!(
                        declaration.origin,
                        crate::KernelDeclarationOrigin::PatternBinding {
                            arm: declaration_arm,
                            ordinal: declaration_ordinal,
                        } if declaration_arm == *expression
                            && declaration_ordinal as usize == ordinal
                    ) || declaration.kind != crate::KernelDeclarationKind::PatternBinding
                    {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} match arm {} binding {} has inconsistent declaration authority",
                            owner.0, expression.0, binding.0,
                        )));
                    }
                    let projection = match pattern {
                        crate::KernelPattern::Tag { fields, .. }
                            if fields
                                .iter()
                                .any(|field| field.as_ref() == declaration.name.as_ref()) =>
                        {
                            vec![declaration.name.to_string()]
                        }
                        _ => Vec::new(),
                    };
                    bindings.push(CheckedPatternBinding {
                        declaration: self
                            .declaration(owner, KernelDeclarationReference::Local(*binding))?,
                        selector: self.expression(owner, *selector)?,
                        projection,
                    });
                }
            }
        }
        bindings.sort_unstable_by_key(|binding| binding.declaration);
        Ok(bindings.into_boxed_slice())
    }

    /// Derive each call's stable storage path from the already-linked checked
    /// expression graph. This is a single linear-table postpass; it performs no
    /// type inference and does not reopen source-shaped owner products.
    pub fn materialize_call_result_paths(
        &self,
        declarations: &[CheckedDeclaration],
        callables: &[CheckedCallableSignature],
        expressions: &[CheckedExpression],
        calls: &[CheckedCall],
    ) -> Result<Box<[CheckedCallResultPath]>, KernelCheckedLinkError> {
        let declaration_values = declarations
            .iter()
            .filter_map(|declaration| declaration.value.map(|value| (declaration.id, value)))
            .collect::<BTreeMap<_, _>>();
        let callable_results = callables
            .iter()
            .filter_map(|callable| {
                callable
                    .result_expression
                    .map(|expression| (callable.decl_id, expression))
            })
            .collect::<BTreeMap<_, _>>();
        let calls_by_id = calls
            .iter()
            .map(|call| (call.id, call))
            .collect::<BTreeMap<_, _>>();
        if calls_by_id.len() != calls.len() {
            return Err(KernelCheckedLinkError::new(
                "kernel checked call-result path materializer received duplicate call IDs",
            ));
        }
        let mut paths = Vec::new();
        for call in calls {
            let expression = expressions
                .get(call.expression.0 as usize)
                .filter(|expression| expression.id == call.expression)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked call {} references missing expression {}",
                        call.id.0, call.expression.0,
                    ))
                })?;
            let Some(anchor) = expression.declaration else {
                continue;
            };
            let Some(root) = declaration_values
                .get(&anchor)
                .copied()
                .or_else(|| callable_results.get(&anchor).copied())
            else {
                continue;
            };
            let Some(projection) =
                checked_projection_to_expression(expressions, &calls_by_id, root, call.expression)
            else {
                continue;
            };
            paths.push(CheckedCallResultPath {
                call: call.id,
                path: CheckedSemanticPath { anchor, projection },
            });
        }
        paths.sort_unstable_by_key(|path| path.call);
        Ok(paths.into_boxed_slice())
    }

    /// Emit the complete semantic occurrence inventory while source-local
    /// declaration/call/expression spans are still available. Rows remain
    /// grouped by definition so project coordinate rebasing is one bounded
    /// range update per source owner.
    pub fn materialize_occurrences(
        &self,
        snapshot: &KernelCheckedSnapshot,
        declarations: &[CheckedDeclaration],
        expressions: &[CheckedExpression],
        calls: &[CheckedCall],
    ) -> Result<(Box<[SemanticOccurrence]>, Box<[KernelCheckedRowRange]>), KernelCheckedLinkError>
    {
        self.validate_snapshot_definition_count(snapshot, "occurrence")?;
        let declaration_by_id = declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let mut occurrences = Vec::new();
        let mut ranges = Vec::with_capacity(snapshot.definitions.len());
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "occurrence")?;
            let linked = self.definition(owner)?;
            let range_start = u32::try_from(occurrences.len()).map_err(|_| {
                KernelCheckedLinkError::new("kernel checked occurrence count exceeds u32")
            })?;
            let mut declared = BTreeSet::new();

            // Authored declarations precede call-generated occurrences. Inline
            // record fields are lexical projection anchors, not authored-name
            // occurrences in the public checked index. Fresh OUT and
            // call-context declarations are emitted at their exact call
            // position below, matching their source authority.
            for declaration in &definition.declarations {
                if matches!(
                    declaration.origin,
                    crate::KernelDeclarationOrigin::RecordField { .. }
                        | crate::KernelDeclarationOrigin::CallbackBinding { .. }
                        | crate::KernelDeclarationOrigin::CallContext { .. }
                ) {
                    continue;
                }
                let target =
                    self.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
                let row = declaration_by_id.get(&target).copied().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} occurrence references missing declaration {}",
                        owner.0, target.0,
                    ))
                })?;
                if !declared.insert(target) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats declaration occurrence {}",
                        owner.0, target.0,
                    )));
                }
                occurrences.push(SemanticOccurrence {
                    target,
                    kind: SemanticOccurrenceKind::Declaration,
                    span: row.span,
                });
            }

            let mut syntax_by_expression = BTreeMap::new();
            for syntax in &definition.call_syntax {
                if syntax_by_expression
                    .insert(syntax.expression, syntax)
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats authored call expression {}",
                        owner.0, syntax.expression.0,
                    )));
                }
            }
            for (ordinal, artifact_call) in definition.calls.iter().enumerate() {
                let row = linked.calls.start as usize + ordinal;
                let call = calls
                    .get(row)
                    .filter(|call| call.id.0 as usize == row)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} occurrence references missing call row {row}",
                            owner.0,
                        ))
                    })?;
                let syntax = syntax_by_expression
                    .get(&artifact_call.expression)
                    .copied()
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} call expression {} has no authored occurrence surface",
                            owner.0, artifact_call.expression.0,
                        ))
                    })?;
                for entry in &call.entries {
                    match entry {
                        CheckedCallEntry::FreshOut { output, .. } => {
                            let declaration = declaration_by_id.get(output).copied().ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition {} FreshOut occurrence references missing declaration {}",
                                    owner.0, output.0,
                                ))
                            })?;
                            if !declared.insert(*output) {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} repeats FreshOut occurrence {}",
                                    owner.0, output.0,
                                )));
                            }
                            occurrences.push(SemanticOccurrence {
                                target: *output,
                                kind: SemanticOccurrenceKind::FreshOut,
                                span: declaration.span,
                            });
                        }
                        CheckedCallEntry::ForwardOut { name, target, .. } => {
                            let mut arguments = syntax.arguments.iter().filter(|argument| {
                                argument.kind == KernelCallArgumentKind::Named
                                    && argument.name.as_ref() == name
                            });
                            let argument = arguments.next().ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition {} ForwardOut `{name}` has no authored argument occurrence",
                                    owner.0,
                                ))
                            })?;
                            if arguments.next().is_some() {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} ForwardOut `{name}` has multiple authored argument occurrences",
                                    owner.0,
                                )));
                            }
                            occurrences.push(SemanticOccurrence {
                                target: *target,
                                kind: SemanticOccurrenceKind::ForwardOut,
                                span: checked_span(argument.span),
                            });
                        }
                        CheckedCallEntry::Input { .. } => {}
                    }
                }
                for context in &call.contexts {
                    let declaration = declaration_by_id
                        .get(&context.declaration)
                        .copied()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} call-context occurrence references missing declaration {}",
                                owner.0, context.declaration.0,
                            ))
                        })?;
                    if !declared.insert(context.declaration) {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} repeats call-context declaration occurrence {}",
                            owner.0, context.declaration.0,
                        )));
                    }
                    occurrences.push(SemanticOccurrence {
                        target: context.declaration,
                        kind: SemanticOccurrenceKind::Declaration,
                        span: declaration.span,
                    });
                }
                occurrences.push(SemanticOccurrence {
                    target: call.callable,
                    kind: SemanticOccurrenceKind::Call,
                    span: call.span,
                });
                if let CheckedContextBinding::Explicit { span, .. } = call.context_binding {
                    occurrences.push(SemanticOccurrence {
                        target: call.callable,
                        kind: SemanticOccurrenceKind::Pass,
                        span,
                    });
                }
            }
            let expected_declaration_occurrences = definition
                .declarations
                .iter()
                .filter(|declaration| {
                    !matches!(
                        declaration.origin,
                        crate::KernelDeclarationOrigin::RecordField { .. }
                    )
                })
                .count();
            if declared.len() != expected_declaration_occurrences {
                let missing = definition
                    .declarations
                    .iter()
                    .filter_map(|declaration| {
                        if matches!(
                            declaration.origin,
                            crate::KernelDeclarationOrigin::RecordField { .. }
                        ) {
                            return None;
                        }
                        let target = self
                            .declaration(owner, KernelDeclarationReference::Local(declaration.id))
                            .ok()?;
                        (!declared.contains(&target)).then_some(target.0)
                    })
                    .collect::<Vec<_>>();
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has declarations without exact occurrences: {missing:?}",
                    owner.0,
                )));
            }

            for row in checked_range(linked.expressions)? {
                let expression = expressions.get(row).filter(|expression| {
                    expression.id == CheckedExprId(u32::try_from(row).unwrap_or(u32::MAX))
                });
                let Some(expression) = expression else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} occurrence references missing expression row {row}",
                        owner.0,
                    )));
                };
                let target = match expression.kind {
                    CheckedExpressionKind::Read { target, .. }
                    | CheckedExpressionKind::Drain { target, .. } => target,
                    _ => continue,
                };
                occurrences.push(SemanticOccurrence {
                    target,
                    kind: SemanticOccurrenceKind::Read,
                    span: expression.span,
                });
            }
            let range_end = u32::try_from(occurrences.len()).map_err(|_| {
                KernelCheckedLinkError::new("kernel checked occurrence count exceeds u32")
            })?;
            ranges.push(KernelCheckedRowRange {
                start: range_start,
                len: range_end - range_start,
            });
        }
        Ok((occurrences.into_boxed_slice(), ranges.into_boxed_slice()))
    }

    pub fn definitions(&self) -> &[KernelCheckedDefinitionLayout] {
        &self.definitions
    }

    pub fn abi_callables(&self) -> &[KernelCheckedAbiCallableLayout] {
        &self.abi_callables
    }

    pub fn abi_callable(
        &self,
        name: &str,
    ) -> Result<&KernelCheckedAbiCallableLayout, KernelCheckedLinkError> {
        self.abi_callable_by_name
            .get(name)
            .and_then(|index| self.abi_callables.get(*index))
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked linker references unallocated ABI callable `{name}`"
                ))
            })
    }

    pub const fn totals(&self) -> KernelCheckedLinkTotals {
        self.totals
    }

    pub fn definition(
        &self,
        owner: KernelOwnerId,
    ) -> Result<&KernelCheckedDefinitionLayout, KernelCheckedLinkError> {
        self.definitions.get(owner.0 as usize).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel checked linker references missing definition {}",
                owner.0,
            ))
        })
    }

    pub fn expression(
        &self,
        owner: KernelOwnerId,
        value: KernelValueReference,
    ) -> Result<CheckedExprId, KernelCheckedLinkError> {
        match value {
            KernelValueReference::Local(expression) => Ok(CheckedExprId(
                self.definition(owner)?
                    .expressions
                    .resolve(expression.0, "expression")?,
            )),
            KernelValueReference::External(external) => {
                let target = self.definition(external.owner)?;
                match external.target {
                    KernelExternalTarget::Expression(expression) => Ok(CheckedExprId(
                        target
                            .expressions
                            .resolve(expression.0, "external expression")?,
                    )),
                    KernelExternalTarget::Result => Ok(target.result_expression),
                }
            }
        }
    }

    pub fn scope(
        &self,
        owner: KernelOwnerId,
        scope: KernelScopeReference,
    ) -> Result<LexicalScopeId, KernelCheckedLinkError> {
        match scope {
            KernelScopeReference::ProjectRoot => Ok(LexicalScopeId(0)),
            KernelScopeReference::Containing => Ok(self.definition(owner)?.containing_scope),
            KernelScopeReference::Local(scope) => Ok(LexicalScopeId(
                self.definition(owner)?.scopes.resolve(scope.0, "scope")?,
            )),
            KernelScopeReference::Owner {
                owner: provider,
                scope,
            } => Ok(LexicalScopeId(
                self.definition(provider)?
                    .scopes
                    .resolve(scope.0, "imported scope")?,
            )),
        }
    }

    fn lexical_declaration_for_scope(
        &self,
        snapshot: &KernelCheckedSnapshot,
        owner: KernelOwnerId,
        scope: KernelScopeReference,
    ) -> Result<Option<DeclId>, KernelCheckedLinkError> {
        let mut owner = owner;
        let mut scope = scope;
        let mut remaining = snapshot
            .definitions
            .iter()
            .map(|definition| definition.presentation.scopes.len())
            .sum::<usize>()
            .saturating_add(snapshot.definitions.len())
            .saturating_add(1);
        loop {
            if remaining == 0 {
                return Err(KernelCheckedLinkError::new(
                    "kernel checked presentation lexical scopes contain a cycle",
                ));
            }
            remaining -= 1;
            match scope {
                KernelScopeReference::ProjectRoot => return Ok(None),
                KernelScopeReference::Containing => {
                    let definition = snapshot.definitions.get(owner.0 as usize).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel lexical declaration lookup references missing definition {}",
                            owner.0,
                        ))
                    })?;
                    scope = definition.presentation.containing_scope;
                }
                KernelScopeReference::Owner {
                    owner: provider,
                    scope: provider_scope,
                } => {
                    owner = provider;
                    scope = KernelScopeReference::Local(provider_scope);
                }
                KernelScopeReference::Local(local) => {
                    let definition = snapshot.definitions.get(owner.0 as usize).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel lexical declaration lookup references missing definition {}",
                            owner.0,
                        ))
                    })?;
                    let row = definition
                        .presentation
                        .scopes
                        .get(local.0 as usize)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} lexical declaration lookup references missing scope {}",
                                owner.0, local.0,
                            ))
                        })?;
                    if let Some(declaration) = row.owner {
                        return self.declaration(owner, declaration).map(Some);
                    }
                    scope = row.parent;
                }
            }
        }
    }

    /// Consume compact definition presentation into the final checked lexical
    /// scope namespace without reopening parser arenas or owner-shard rows.
    pub fn materialize_scopes(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedScope]>, KernelCheckedLinkError> {
        if snapshot.definitions.len() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked scope materializer has {} definitions for a {}-definition layout",
                snapshot.definitions.len(),
                self.definitions.len(),
            )));
        }
        let mut scopes = Vec::with_capacity(self.totals.scopes as usize);
        scopes.push(CheckedScope {
            id: LexicalScopeId(0),
            parent: None,
            owner: None,
            kind: CheckedScopeKind::Root,
            span: CheckedSpan::default(),
        });
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked scope materializer definition count exceeds u32",
                )
            })?);
            let layout = self.definition(owner)?;
            for scope in &definition.presentation.scopes {
                let id = LexicalScopeId(layout.scopes.resolve(scope.id.0, "scope row")?);
                if id.0 as usize != scopes.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked scope materializer expected row {} but linked {}",
                        scopes.len(),
                        id.0,
                    )));
                }
                scopes.push(CheckedScope {
                    id,
                    parent: Some(self.scope(owner, scope.parent)?),
                    owner: scope
                        .owner
                        .map(|declaration| self.declaration(owner, declaration))
                        .transpose()?,
                    kind: match scope.kind {
                        crate::KernelScopeKind::Function => CheckedScopeKind::Function,
                        crate::KernelScopeKind::Block => CheckedScopeKind::Block,
                        crate::KernelScopeKind::Record => CheckedScopeKind::Record,
                        crate::KernelScopeKind::RepeatedOutput => CheckedScopeKind::RepeatedOutput,
                        crate::KernelScopeKind::CallContext => CheckedScopeKind::CallContext,
                    },
                    span: CheckedSpan {
                        line: scope.span.line,
                        start: scope.span.start,
                        end: scope.span.end,
                    },
                });
            }
        }
        if scopes.len() != self.totals.scopes as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked scope materializer produced {} rows for a {}-row layout",
                scopes.len(),
                self.totals.scopes,
            )));
        }
        Ok(scopes.into_boxed_slice())
    }

    /// Emit definition-owned declaration rows in the final nonzero checked
    /// declaration namespace. Stable ABI declarations occupy the following
    /// layout-owned range and are emitted by [`Self::materialize_abi_callables`].
    pub fn materialize_declarations(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedDeclaration]>, KernelCheckedLinkError> {
        if snapshot.definitions.len() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked declaration materializer has {} definitions for a {}-definition layout",
                snapshot.definitions.len(),
                self.definitions.len(),
            )));
        }
        let mut declarations = Vec::with_capacity(
            self.definition_declarations_end
                .checked_sub(1)
                .expect("the checked declaration namespace reserves row zero") as usize,
        );
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked declaration materializer definition count exceeds u32",
                )
            })?);
            if definition.declarations.len() != definition.presentation.declarations.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has {} declaration artifacts but {} declaration presentations",
                    owner.0,
                    definition.declarations.len(),
                    definition.presentation.declarations.len(),
                )));
            }
            for (declaration, presentation) in definition
                .declarations
                .iter()
                .zip(definition.presentation.declarations.iter())
            {
                if declaration.id != presentation.declaration {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} declaration artifact {} has presentation {}",
                        owner.0, declaration.id.0, presentation.declaration.0,
                    )));
                }
                let id =
                    self.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
                let expected = u32::try_from(declarations.len() + 1).map_err(|_| {
                    KernelCheckedLinkError::new(
                        "kernel checked declaration materializer row count exceeds u32",
                    )
                })?;
                if id != DeclId(expected) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked declaration materializer expected row {expected} but linked {}",
                        id.0,
                    )));
                }
                declarations.push(CheckedDeclaration {
                    id,
                    scope_id: self.scope(owner, presentation.scope)?,
                    name: declaration.name.to_string(),
                    kind: checked_declaration_kind(declaration.kind),
                    flow_type: declaration_flow_type(self, snapshot, owner, declaration)?,
                    value: declaration
                        .value
                        .map(|value| self.expression(owner, value))
                        .transpose()?,
                    body_scope: presentation
                        .body_scope
                        .map(|scope| self.scope(owner, KernelScopeReference::Local(scope)))
                        .transpose()?,
                    span: CheckedSpan {
                        line: presentation.span.line,
                        start: presentation.span.start,
                        end: presentation.span.end,
                    },
                });
            }
        }
        if declarations.len() + 1 != self.definition_declarations_end as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked declaration materializer produced {} rows for a namespace ending at {}",
                declarations.len(),
                self.definition_declarations_end,
            )));
        }
        Ok(declarations.into_boxed_slice())
    }

    /// Emit definition statements directly into the final checked namespace.
    ///
    /// Resource ownership is already explicit in the solved SOURCE, HOLD, and
    /// LIST artifacts. Build one dense reverse table from those authorities
    /// instead of rediscovering resources by walking expression trees or
    /// replaying the legacy owner assembler.
    pub fn materialize_statements(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedStatement]>, KernelCheckedLinkError> {
        if snapshot.definitions.len() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked statement materializer has {} definitions for a {}-definition layout",
                snapshot.definitions.len(),
                self.definitions.len(),
            )));
        }

        let mut resources =
            vec![Vec::<CheckedResourceBinding>::new(); self.totals.statements as usize];
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked statement materializer definition count exceeds u32",
                )
            })?);
            for source in &definition.sources {
                push_statement_resource(
                    &mut resources,
                    self.statement(owner, source.statement)?,
                    CheckedResourceBinding::Source {
                        source: self.source(owner, source.id.0)?,
                    },
                )?;
            }
        }
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked statement materializer definition count exceeds u32",
                )
            })?);
            for state in &definition.states {
                push_statement_resource(
                    &mut resources,
                    self.statement(owner, state.statement)?,
                    CheckedResourceBinding::State {
                        state: self.state(owner, state.id.0)?,
                    },
                )?;
            }
        }
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked statement materializer definition count exceeds u32",
                )
            })?);
            for list in &definition.lists {
                push_statement_resource(
                    &mut resources,
                    self.statement(owner, list.statement)?,
                    CheckedResourceBinding::ListAuthority {
                        list: self.list(owner, list.id.0)?,
                    },
                )?;
            }
        }

        let mut statements = Vec::with_capacity(self.totals.statements as usize);
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked statement materializer definition count exceeds u32",
                )
            })?);
            if definition.statements.len() != definition.presentation.statements.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has {} statement artifacts but {} statement presentations",
                    owner.0,
                    definition.statements.len(),
                    definition.presentation.statements.len(),
                )));
            }
            for (statement, presentation) in definition
                .statements
                .iter()
                .zip(definition.presentation.statements.iter())
            {
                if statement.id != presentation.statement {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} statement artifact {} has presentation {}",
                        owner.0, statement.id.0, presentation.statement.0,
                    )));
                }
                let id = self.statement(owner, KernelStatementReference::Local(statement.id))?;
                if id.0 as usize != statements.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked statement materializer expected row {} but linked {}",
                        statements.len(),
                        id.0,
                    )));
                }
                let declaration = statement_declaration_authority(definition, statement)?
                    .map(|declaration| self.declaration(owner, declaration))
                    .transpose()?;
                statements.push(CheckedStatement {
                    id,
                    scope_id: self.scope(owner, presentation.scope)?,
                    kind: checked_statement_kind(&statement.kind, declaration)?,
                    resources: std::mem::take(&mut resources[id.0 as usize]),
                    value: statement
                        .value
                        .map(|value| self.expression(owner, value))
                        .transpose()?,
                    value_use: match statement.value_use {
                        crate::KernelStatementValueUse::RuntimeValue => {
                            CheckedValueUse::RuntimeValue
                        }
                        crate::KernelStatementValueUse::RenderSlot => CheckedValueUse::RenderSlot,
                    },
                    children: statement
                        .children
                        .iter()
                        .map(|child| match child {
                            KernelStatementChildReference::Local(child) => {
                                self.statement(owner, KernelStatementReference::Local(*child))
                            }
                            KernelStatementChildReference::Owner(child) => {
                                self.statement(owner, KernelStatementReference::OwnerPublic(*child))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    span: CheckedSpan {
                        line: presentation.span.line,
                        start: presentation.span.start,
                        end: presentation.span.end,
                    },
                });
            }
        }
        if statements.len() != self.totals.statements as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked statement materializer produced {} rows for a {}-row layout",
                statements.len(),
                self.totals.statements,
            )));
        }
        if resources.iter().any(|resources| !resources.is_empty()) {
            return Err(KernelCheckedLinkError::new(
                "kernel checked statement materializer left resource bindings unattached",
            ));
        }
        Ok(statements.into_boxed_slice())
    }

    /// Emit every definition expression directly from immutable kernel facts.
    ///
    /// This is the central compatibility-assembler deletion seam: expression
    /// kind, structural children, lexical authority, type, effect, scope, and
    /// source coordinates are all consumed from compact rows produced during
    /// graph construction/solve. No parser or legacy owner DTO is reopened.
    pub fn materialize_expressions(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedExpression]>, KernelCheckedLinkError> {
        if snapshot.definitions.len() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked expression materializer has {} definitions for a {}-definition layout",
                snapshot.definitions.len(),
                self.definitions.len(),
            )));
        }

        let mut source_paths = BTreeMap::<DeclId, Vec<(Vec<String>, CheckedSourceId)>>::new();
        let mut declaration_metadata =
            BTreeMap::<DeclId, (KernelOwnerId, KernelScopeReference, Box<str>)>::new();
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked expression materializer definition count exceeds u32",
                )
            })?);
            for (declaration, presentation) in definition
                .declarations
                .iter()
                .zip(definition.presentation.declarations.iter())
            {
                let linked =
                    self.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
                if declaration_metadata
                    .insert(
                        linked,
                        (owner, presentation.scope, declaration.name.clone()),
                    )
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked expression materializer repeats declaration metadata for {}",
                        linked.0,
                    )));
                }
            }
        }
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked expression materializer definition count exceeds u32",
                )
            })?);
            for source in &definition.sources {
                let source_id = self.source(owner, source.id.0)?;
                let source_declaration = self.declaration(owner, source.declaration)?;
                let source_projection = source
                    .path
                    .projection
                    .iter()
                    .map(|field| field.to_string())
                    .collect::<Vec<_>>();
                let exact_anchor = self.declaration(owner, source.path.anchor)?;
                source_paths
                    .entry(exact_anchor)
                    .or_default()
                    .push((source_projection.clone(), source_id));

                // Checked lexical reads name a nested SOURCE by its exact
                // declaration, even when the authored path starts at an
                // enclosing record declaration. Derive every ancestor alias
                // once from compact declaration/scope presentation so reads
                // can be canonicalized without reopening parser or owner DTOs.
                let Some((_, mut scope, source_name)) =
                    declaration_metadata.get(&source_declaration).cloned()
                else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel SOURCE declaration {} has no presentation metadata",
                        source_declaration.0,
                    )));
                };
                let mut scope_owner = owner;
                let mut alias_projection =
                    Vec::with_capacity(source_projection.len().saturating_add(4));
                alias_projection.push(source_name.to_string());
                alias_projection.extend(source_projection);
                let mut seen_anchors = BTreeSet::new();
                while let Some(anchor) =
                    self.lexical_declaration_for_scope(snapshot, scope_owner, scope)?
                {
                    if !seen_anchors.insert(anchor) || anchor == source_declaration {
                        break;
                    }
                    source_paths
                        .entry(anchor)
                        .or_default()
                        .push((alias_projection.clone(), source_id));
                    let Some((owner, parent_scope, name)) =
                        declaration_metadata.get(&anchor).cloned()
                    else {
                        break;
                    };
                    alias_projection.insert(0, name.to_string());
                    scope_owner = owner;
                    scope = parent_scope;
                }
            }
        }
        for paths in source_paths.values_mut() {
            paths.sort_by_key(|(path, source)| (std::cmp::Reverse(path.len()), *source));
        }

        let mut expressions = Vec::with_capacity(self.totals.expressions as usize);
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel checked expression materializer definition count exceeds u32",
                )
            })?);
            let local_len = definition.expressions.len();
            if definition.presentation.expressions.len() != local_len
                || definition.expression_payloads.len() != local_len
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} expression artifacts, presentation, and payload tables differ: {} / {} / {}",
                    owner.0,
                    local_len,
                    definition.presentation.expressions.len(),
                    definition.expression_payloads.len(),
                )));
            }
            let mut shapes = vec![None; local_len];
            for shape in &definition.execution_shapes {
                let slot = shapes
                    .get_mut(shape.expression().0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} execution shape references missing expression {}",
                            owner.0,
                            shape.expression().0,
                        ))
                    })?;
                if slot.replace(shape).is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats execution shape for expression {}",
                        owner.0,
                        shape.expression().0,
                    )));
                }
            }
            let mut lexical = vec![None; local_len];
            for binding in &definition.lexical_bindings {
                let slot = lexical
                    .get_mut(binding.expression.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} lexical binding references missing expression {}",
                            owner.0, binding.expression.0,
                        ))
                    })?;
                if slot.replace(binding).is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats lexical binding for expression {}",
                        owner.0, binding.expression.0,
                    )));
                }
            }
            let mut calls = vec![None; local_len];
            for (ordinal, call) in definition.calls.iter().enumerate() {
                let slot = calls.get_mut(call.expression.0 as usize).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} call references missing expression {}",
                        owner.0, call.expression.0,
                    ))
                })?;
                if slot
                    .replace(u32::try_from(ordinal).map_err(|_| {
                        KernelCheckedLinkError::new("kernel call ordinal exceeds u32")
                    })?)
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats call for expression {}",
                        owner.0, call.expression.0,
                    )));
                }
            }

            for (((expression, presentation), payload), local_ordinal) in definition
                .expressions
                .iter()
                .zip(definition.presentation.expressions.iter())
                .zip(definition.expression_payloads.iter())
                .zip(0..)
            {
                if expression.id != presentation.expression
                    || expression.id.0 as usize != local_ordinal
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression rows are not dense at ordinal {local_ordinal}",
                        owner.0,
                    )));
                }
                let id = self.expression(owner, KernelValueReference::Local(expression.id))?;
                if id.0 as usize != expressions.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked expression materializer expected row {} but linked {}",
                        expressions.len(),
                        id.0,
                    )));
                }
                let declaration = match presentation.declaration {
                    Some(declaration) => Some(self.declaration(owner, declaration)?),
                    None => self.lexical_declaration_for_scope(
                        snapshot,
                        owner,
                        presentation.declaration_scope.unwrap_or(presentation.scope),
                    )?,
                };
                let kind = checked_expression_kind(
                    self,
                    owner,
                    definition,
                    expression,
                    presentation.span.line,
                    declaration,
                    payload,
                    shapes[local_ordinal],
                    lexical[local_ordinal],
                    calls[local_ordinal],
                    &source_paths,
                )?;
                expressions.push(CheckedExpression {
                    id,
                    scope_id: self.scope(owner, presentation.scope)?,
                    declaration,
                    flow_type: self.relocate_flow_type(owner, &expression.flow_type)?,
                    flush_type: expression
                        .flush_type
                        .as_ref()
                        .map(|ty| relocate_type(self.definition(owner)?.type_variables, ty))
                        .transpose()?,
                    effect: CheckedEffectSummary {
                        reads_state: expression.effect.reads_state,
                        writes_state: expression.effect.writes_state,
                        emits_source: expression.effect.emits_source,
                        invokes_host: expression.effect.invokes_host,
                    },
                    kind,
                    span: CheckedSpan {
                        line: presentation.span.line,
                        start: presentation.span.start,
                        end: presentation.span.end,
                    },
                });
            }
        }
        if expressions.len() != self.totals.expressions as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked expression materializer produced {} rows for a {}-row layout",
                expressions.len(),
                self.totals.expressions,
            )));
        }
        Ok(expressions.into_boxed_slice())
    }

    /// Emit authored user-callable signatures and their sparse inherited
    /// PASSED schemes directly from the same definition artifacts that own
    /// declaration and expression rows.
    ///
    /// Builtin/external ABI signatures are intentionally not accepted here;
    /// the compiler facade appends those stable lower-level contracts after
    /// this definition-owned table. Keeping the two authorities separate
    /// prevents parser or legacy owner metadata from leaking into the kernel.
    pub fn materialize_user_callables(
        &self,
        snapshot: &KernelCheckedSnapshot,
        role: ProgramRole,
    ) -> Result<
        (Box<[CheckedCallableSignature]>, Box<[CheckedContextFormal]>),
        KernelCheckedLinkError,
    > {
        self.validate_snapshot_definition_count(snapshot, "user callable")?;
        let mut callables = Vec::with_capacity(self.totals.user_callables as usize);
        let mut context_formals = Vec::with_capacity(self.totals.context_formals as usize);
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "user callable")?;
            let root_statement = definition.linkage.root_statement.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} has no root statement while linking callables",
                    owner.0,
                ))
            })?;
            let Some(root) = definition.statements.get(root_statement.0 as usize) else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} root statement {} is missing",
                    owner.0, root_statement.0,
                )));
            };
            let crate::KernelStatementKind::Function { name, parameters } = &root.kind else {
                if definition.linkage.context_formal_ordinal.is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel non-callable definition {} owns a context formal",
                        owner.0,
                    )));
                }
                continue;
            };
            let KernelDeclarationReference::Local(public_declaration) =
                definition.linkage.public_declaration.ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} has no public declaration",
                        owner.0,
                    ))
                })?
            else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} delegates its public declaration",
                    owner.0,
                )));
            };
            let public_declaration_row = definition
                .declarations
                .get(public_declaration.0 as usize)
                .filter(|declaration| {
                    declaration.id == public_declaration
                        && declaration.kind == crate::KernelDeclarationKind::Function
                })
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} has no exact function declaration {}",
                        owner.0, public_declaration.0,
                    ))
                })?;
            let public_presentation = declaration_presentation(definition, public_declaration)?;
            let body_scope = public_presentation.body_scope.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} has no body scope",
                    owner.0,
                ))
            })?;
            let root_presentation = statement_presentation(definition, root_statement)?;
            if root_presentation.body_scope != Some(body_scope) {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} function declaration and statement disagree on body scope",
                    owner.0,
                )));
            }
            if definition.formals.len()
                != parameters.len()
                    + usize::from(definition.linkage.context_formal_ordinal.is_some())
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} has {} parameters, {:?} context formal, and {} solved formals",
                    owner.0,
                    parameters.len(),
                    definition.linkage.context_formal_ordinal,
                    definition.formals.len(),
                )));
            }
            let mut checked_parameters = Vec::with_capacity(parameters.len());
            for parameter in parameters.iter() {
                if parameter.ordinal as usize >= parameters.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} parameter ordinal {} is outside 0..{}",
                        owner.0,
                        parameter.ordinal,
                        parameters.len(),
                    )));
                }
                let mut declarations = definition.declarations.iter().filter(|declaration| {
                    matches!(
                        declaration.origin,
                        crate::KernelDeclarationOrigin::Parameter { statement, ordinal }
                            if statement == root_statement && ordinal == parameter.ordinal
                    )
                });
                let declaration = declarations.next().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} parameter {} has no declaration",
                        owner.0, parameter.ordinal,
                    ))
                })?;
                if declarations.next().is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} parameter {} has multiple declarations",
                        owner.0, parameter.ordinal,
                    )));
                }
                let expected_kind = match parameter.kind {
                    crate::KernelParameterKind::Value => {
                        crate::KernelDeclarationKind::ValueParameter
                    }
                    crate::KernelParameterKind::Out => crate::KernelDeclarationKind::OutParameter,
                };
                if declaration.kind != expected_kind || declaration.name != parameter.name {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} parameter {} declaration disagrees with its authored header",
                        owner.0, parameter.ordinal,
                    )));
                }
                let presentation = declaration_presentation(definition, declaration.id)?;
                let evaluation_scope = match parameter.evaluation_scope {
                    crate::KernelParameterEvaluationScope::Parent => CheckedEvaluationScope::Parent,
                    crate::KernelParameterEvaluationScope::Output { parameter_ordinal } => {
                        let output = definition
                            .declarations
                            .iter()
                            .find(|candidate| {
                                matches!(
                                    candidate.origin,
                                    crate::KernelDeclarationOrigin::Parameter { statement, ordinal }
                                        if statement == root_statement && ordinal == parameter_ordinal
                                )
                            })
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel callable definition {} parameter {} targets missing output parameter {}",
                                    owner.0, parameter.ordinal, parameter_ordinal,
                                ))
                            })?;
                        if output.kind != crate::KernelDeclarationKind::OutParameter {
                            return Err(KernelCheckedLinkError::new(format!(
                                "kernel callable definition {} parameter {} targets non-OUT parameter {}",
                                owner.0, parameter.ordinal, parameter_ordinal,
                            )));
                        }
                        CheckedEvaluationScope::Output {
                            formal: self
                                .declaration(owner, KernelDeclarationReference::Local(output.id))?,
                        }
                    }
                };
                checked_parameters.push(CheckedParameter {
                    decl_id: self
                        .declaration(owner, KernelDeclarationReference::Local(declaration.id))?,
                    name: parameter.name.to_string(),
                    kind: match parameter.kind {
                        crate::KernelParameterKind::Value => CheckedParameterKind::Value,
                        crate::KernelParameterKind::Out => CheckedParameterKind::Out,
                    },
                    ordinal: parameter.ordinal as usize,
                    flow_type: self.relocate_flow_type(
                        owner,
                        &definition.formals[parameter.ordinal as usize],
                    )?,
                    requirement: CheckedParameterRequirement::Required,
                    evaluation_scope,
                    start: presentation.span.start,
                    end: presentation.span.end,
                });
            }
            checked_parameters.sort_unstable_by_key(|parameter| parameter.ordinal);

            let context_formal = definition
                .linkage
                .context_formal_ordinal
                .map(|ordinal| {
                    let id = self.definition(owner)?.context_formal.ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel callable definition {} has no linked context formal",
                            owner.0,
                        ))
                    })?;
                    if ordinal as usize != parameters.len() {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel callable definition {} context ordinal {} does not follow {} authored parameters",
                            owner.0,
                            ordinal,
                            parameters.len(),
                        )));
                    }
                    let flow_type = self.relocate_flow_type(
                        owner,
                        &definition.formals[ordinal as usize],
                    )?;
                    let projections = boon_checked::context_scheme_projections(&flow_type.ty);
                    context_formals.push(CheckedContextFormal {
                        id,
                        callable: self.declaration(
                            owner,
                            KernelDeclarationReference::Local(public_declaration),
                        )?,
                        scheme: CheckedContextScheme {
                            flow_type,
                            projections,
                        },
                    });
                    Ok(id)
                })
                .transpose()?;
            if public_declaration_row.name != *name {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} declaration name {:?} differs from function header {:?}",
                    owner.0, public_declaration_row.name, name,
                )));
            }
            let mut effect = CheckedEffectSummary::default();
            for expression in &definition.expressions {
                effect.reads_state |= expression.effect.reads_state;
                effect.writes_state |= expression.effect.writes_state;
                effect.emits_source |= expression.effect.emits_source;
                effect.invokes_host |= expression.effect.invokes_host;
            }
            callables.push(CheckedCallableSignature {
                decl_id: self
                    .declaration(owner, KernelDeclarationReference::Local(public_declaration))?,
                scope_id: self.scope(owner, KernelScopeReference::Local(body_scope))?,
                kind: CheckedCallableKind::User,
                name: name.to_string(),
                intrinsic: None,
                external_identity: None,
                parameters: checked_parameters,
                contexts: Vec::new(),
                context_formal,
                result: self.relocate_flow_type(owner, &definition.result)?,
                role,
                effect,
                body: Some(self.statement(owner, KernelStatementReference::Local(root_statement))?),
                result_expression: Some(self.expression(
                    owner,
                    KernelValueReference::Local(definition.linkage.result_expression.ok_or_else(
                        || {
                            KernelCheckedLinkError::new(format!(
                                "kernel callable definition {} has no result expression",
                                owner.0,
                            ))
                        },
                    )?),
                )?),
                contextual_operation: None,
            });
        }
        self.validate_materialized_count(
            "user callable",
            callables.len(),
            self.totals.user_callables,
        )?;
        self.validate_materialized_count(
            "context formal",
            context_formals.len(),
            self.totals.context_formals,
        )?;
        Ok((
            callables.into_boxed_slice(),
            context_formals.into_boxed_slice(),
        ))
    }

    /// Materialize every referenced builtin/external callable from the
    /// immutable kernel ABI, including its declarations and relocated local
    /// type-variable namespace.
    ///
    /// This is deliberately demand-shaped: an unused compiler/library
    /// contract does not bloat the checked image. The call occurrence and its
    /// ABI signature nevertheless share this one layout authority.
    pub fn materialize_abi_callables(
        &self,
        abi: &KernelAbiInput,
    ) -> Result<(Box<[CheckedCallableSignature]>, Box<[CheckedDeclaration]>), KernelCheckedLinkError>
    {
        let mut callables = Vec::with_capacity(self.totals.abi_callables as usize);
        let mut declarations = Vec::new();
        for layout in &self.abi_callables {
            let callable = abi.callable(&layout.name).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked ABI materializer cannot find `{}` in its immutable ABI",
                    layout.name,
                ))
            })?;
            if callable.parameters.len() != layout.parameters.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked ABI callable `{}` has {} parameters in its layout and {} in its contract",
                    layout.name,
                    layout.parameters.len(),
                    callable.parameters.len(),
                )));
            }
            let parameters = callable
                .parameters
                .iter()
                .zip(layout.parameters.iter().copied())
                .map(|(parameter, decl_id)| {
                    let evaluation_scope = match parameter.evaluation_scope {
                        crate::KernelParameterEvaluationScope::Parent => {
                            CheckedEvaluationScope::Parent
                        }
                        crate::KernelParameterEvaluationScope::Output { parameter_ordinal } => {
                            let formal = layout
                                .parameters
                                .get(parameter_ordinal as usize)
                                .copied()
                                .ok_or_else(|| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel checked ABI callable `{}` parameter `{}` targets missing OUT ordinal {parameter_ordinal}",
                                        layout.name, parameter.name,
                                    ))
                                })?;
                            CheckedEvaluationScope::Output { formal }
                        }
                    };
                    Ok(CheckedParameter {
                        decl_id,
                        name: parameter.name.to_string(),
                        kind: parameter.kind,
                        ordinal: parameter.ordinal as usize,
                        flow_type: relocate_abi_flow_type(layout, &parameter.flow_type)?,
                        requirement: parameter.requirement.clone(),
                        evaluation_scope,
                        start: 0,
                        end: 0,
                    })
                })
                .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
            let contexts = callable
                .contexts
                .iter()
                .map(|context| {
                    let provider = layout
                        .parameters
                        .get(context.provider_parameter_ordinal as usize)
                        .copied()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel checked ABI callable `{}` context `{}` targets missing parameter ordinal {}",
                                layout.name, context.name, context.provider_parameter_ordinal,
                            ))
                        })?;
                    Ok(CheckedCallableContext {
                        name: context.name.to_string(),
                        kind: context.kind,
                        provider,
                        flow_type: relocate_abi_flow_type(layout, &context.flow_type)?,
                    })
                })
                .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
            let result = relocate_abi_flow_type(layout, &callable.result)?;
            callables.push(CheckedCallableSignature {
                decl_id: layout.declaration,
                scope_id: LexicalScopeId(0),
                kind: match callable.kind {
                    crate::KernelCallableKind::Builtin => CheckedCallableKind::Builtin,
                    crate::KernelCallableKind::External => CheckedCallableKind::External,
                    crate::KernelCallableKind::User => {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel immutable ABI unexpectedly contains user callable `{}`",
                            layout.name,
                        )));
                    }
                },
                name: layout.name.to_string(),
                intrinsic: callable.intrinsic,
                external_identity: callable.external_identity,
                parameters: parameters.clone(),
                contexts,
                context_formal: None,
                result: result.clone(),
                role: callable.role,
                effect: callable.effect,
                body: None,
                result_expression: None,
                contextual_operation: callable
                    .contextual_operation
                    .map(|operation| checked_abi_contextual_operation(layout, operation))
                    .transpose()?,
            });
            declarations.push(CheckedDeclaration {
                id: layout.declaration,
                scope_id: LexicalScopeId(0),
                name: layout.name.to_string(),
                kind: match callable.kind {
                    crate::KernelCallableKind::Builtin => CheckedDeclarationKind::Builtin,
                    crate::KernelCallableKind::External => CheckedDeclarationKind::External,
                    crate::KernelCallableKind::User => unreachable!("validated above"),
                },
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Function {
                        args: parameters
                            .iter()
                            .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                            .map(|parameter| parameter.flow_type.ty.clone())
                            .collect(),
                        result: Box::new(result),
                    },
                },
                value: None,
                body_scope: None,
                span: CheckedSpan::default(),
            });
            declarations.extend(parameters.into_iter().map(|parameter| CheckedDeclaration {
                id: parameter.decl_id,
                scope_id: LexicalScopeId(0),
                name: parameter.name,
                kind: match parameter.kind {
                    CheckedParameterKind::Value => CheckedDeclarationKind::ValueParameter,
                    CheckedParameterKind::Out => CheckedDeclarationKind::OutParameter,
                },
                flow_type: parameter.flow_type,
                value: None,
                body_scope: None,
                span: CheckedSpan::default(),
            }));
        }
        self.validate_materialized_count(
            "ABI callable",
            callables.len(),
            self.totals.abi_callables,
        )?;
        let expected_declarations = self
            .totals
            .declarations
            .checked_sub(self.definition_declarations_end)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked ABI declaration range precedes definition declarations",
                )
            })?;
        self.validate_materialized_count(
            "ABI declaration",
            declarations.len(),
            expected_declarations,
        )?;
        Ok((
            callables.into_boxed_slice(),
            declarations.into_boxed_slice(),
        ))
    }

    /// Link every solved call occurrence directly from its definition
    /// artifact and the callable table produced by this same layout.
    ///
    /// No signature matching or type solving happens here. The kernel already
    /// retained the matched input edges, generated OUT/context declarations,
    /// stable type-parameter substitutions, and solved result. This pass only
    /// relocates those facts into final checked IDs.
    pub fn materialize_calls(
        &self,
        project: &KernelProjectInput,
        snapshot: &KernelCheckedSnapshot,
        callables: &[CheckedCallableSignature],
        declarations: &[CheckedDeclaration],
    ) -> Result<(Box<[CheckedCall]>, Box<[StableOccurrenceKey]>), KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "call")?;
        let callable_by_declaration = callables
            .iter()
            .map(|callable| (callable.decl_id, callable))
            .collect::<BTreeMap<_, _>>();
        if callable_by_declaration.len() != callables.len() {
            return Err(KernelCheckedLinkError::new(
                "kernel checked call materializer received duplicate callable declarations",
            ));
        }
        let declaration_by_id = declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let mut calls = Vec::with_capacity(self.totals.calls as usize);
        let mut call_occurrences = Vec::with_capacity(self.totals.calls as usize);
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "call")?;
            let local = self.definition(owner)?;
            let mut syntax_by_expression = BTreeMap::new();
            for syntax in &definition.call_syntax {
                if syntax_by_expression
                    .insert(syntax.expression, syntax)
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats call syntax expression {}",
                        owner.0, syntax.expression.0,
                    )));
                }
            }
            let owner_callable = definition
                .linkage
                .root_statement
                .and_then(|root| definition.statements.get(root.0 as usize))
                .filter(|root| matches!(root.kind, crate::KernelStatementKind::Function { .. }))
                .map(|_| local.public_declaration);
            for (ordinal, call) in definition.calls.iter().enumerate() {
                let syntax = syntax_by_expression
                    .get(&call.expression)
                    .copied()
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} call expression {} has no authored syntax row",
                            owner.0, call.expression.0,
                        ))
                    })?;
                let callable_id = match call.target {
                    KernelCallTarget::User { target, .. } => {
                        self.definition(target)?.public_declaration
                    }
                    KernelCallTarget::RenderConstructor { .. }
                    | KernelCallTarget::PureBuiltin { .. }
                    | KernelCallTarget::FixedAbi
                    | KernelCallTarget::HostEffect { .. }
                    | KernelCallTarget::FieldProjection { .. } => {
                        self.abi_callable(&syntax.function)?.declaration
                    }
                };
                let target = callable_by_declaration.get(&callable_id).copied().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} call `{}` targets declaration {} without a materialized signature",
                        owner.0, syntax.function, callable_id.0,
                    ))
                })?;
                if target.name != syntax.function.as_ref() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} call syntax names `{}` but its target is `{}`",
                        owner.0, syntax.function, target.name,
                    )));
                }
                let parameter_for_input = |input: &crate::KernelCallInputArtifact| {
                    let parameter = match &input.role {
                        KernelCallInputRole::Formal { ordinal } => target
                            .parameters
                            .get(*ordinal as usize)
                            .filter(|parameter| parameter.ordinal == *ordinal as usize),
                        KernelCallInputRole::Abi { name } => target
                            .parameters
                            .iter()
                            .find(|parameter| parameter.name == name.as_ref())
                            .or_else(|| {
                                (name.as_ref() == "$pipe").then(|| {
                                    target.parameters.iter().find(|parameter| {
                                        parameter.kind == CheckedParameterKind::Value
                                    })
                                })?
                            }),
                    };
                    parameter.ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} call `{}` has an input without a target parameter: {:?}",
                            owner.0, syntax.function, input.role,
                        ))
                    })
                };
                let mut entries = Vec::with_capacity(call.inputs.len());
                for input in &call.inputs {
                    if let (
                        KernelCallTarget::User { target, .. },
                        KernelCallInputRole::Formal { ordinal },
                    ) = (&call.target, &input.role)
                        && snapshot
                            .definitions
                            .get(target.0 as usize)
                            .is_some_and(|definition| {
                                definition.linkage.context_formal_ordinal == Some(*ordinal)
                            })
                    {
                        // PASSED is a context binding, not a normal checked
                        // call entry. It remains in the solver input table so
                        // the occurrence substitution can retain correlation.
                        continue;
                    }
                    let parameter = parameter_for_input(input)?;
                    let from_pipe = syntax.pipe_input == Some(input.value);
                    let argument = (!from_pipe)
                        .then(|| {
                            syntax.arguments.iter().find(|argument| {
                                argument.name.as_ref() == parameter.name
                                    && argument.value == input.value
                            })
                        })
                        .flatten()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} call `{}` input `{}` has no exact authored argument",
                                owner.0, syntax.function, parameter.name,
                            ))
                        });
                    match parameter.kind {
                        CheckedParameterKind::Value => {
                            if let Ok(argument) = argument.as_ref()
                                && argument.kind != KernelCallArgumentKind::Named
                            {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} call `{}` binds value input `{}` as a bare OUT",
                                    owner.0, syntax.function, parameter.name,
                                )));
                            }
                            entries.push(CheckedCallEntry::Input {
                                formal: parameter.decl_id,
                                name: parameter.name.clone(),
                                value: self.expression(owner, input.value)?,
                                from_pipe,
                                evaluation_scope: parameter.evaluation_scope,
                            });
                        }
                        CheckedParameterKind::Out => {
                            if from_pipe {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} call `{}` pipes into OUT parameter `{}`",
                                    owner.0, syntax.function, parameter.name,
                                )));
                            }
                            let argument = argument?;
                            match argument.kind {
                                KernelCallArgumentKind::BareBinding => {
                                    let declaration = exact_local_declaration_by_origin(
                                        definition,
                                        crate::KernelDeclarationOrigin::CallbackBinding {
                                            call: call.expression,
                                            ordinal: parameter.ordinal as u32,
                                        },
                                        "FreshOut",
                                    )?;
                                    let presentation =
                                        declaration_presentation(definition, declaration.id)?;
                                    let scope = presentation.body_scope.ok_or_else(|| {
                                        KernelCheckedLinkError::new(format!(
                                            "kernel definition {} FreshOut `{}` has no output scope",
                                            owner.0, parameter.name,
                                        ))
                                    })?;
                                    entries.push(CheckedCallEntry::FreshOut {
                                        formal: parameter.decl_id,
                                        name: parameter.name.clone(),
                                        output: self.declaration(
                                            owner,
                                            KernelDeclarationReference::Local(declaration.id),
                                        )?,
                                        scope_id: self
                                            .scope(owner, KernelScopeReference::Local(scope))?,
                                    });
                                }
                                KernelCallArgumentKind::Named => {
                                    let KernelValueReference::Local(expression) = input.value
                                    else {
                                        return Err(KernelCheckedLinkError::new(format!(
                                            "kernel definition {} call `{}` forwards OUT `{}` through a non-local occurrence",
                                            owner.0, syntax.function, parameter.name,
                                        )));
                                    };
                                    let binding = definition
                                        .lexical_bindings
                                        .iter()
                                        .find(|binding| {
                                            binding.expression == expression
                                                && binding.projection.is_empty()
                                        })
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel definition {} call `{}` forwarded OUT `{}` has no exact lexical target",
                                                owner.0, syntax.function, parameter.name,
                                            ))
                                        })?;
                                    let KernelLexicalBindingTarget::Declaration(target_reference) =
                                        binding.target
                                    else {
                                        return Err(KernelCheckedLinkError::new(format!(
                                            "kernel definition {} call `{}` forwarded OUT `{}` targets a non-declaration",
                                            owner.0, syntax.function, parameter.name,
                                        )));
                                    };
                                    let target_declaration =
                                        self.declaration(owner, target_reference)?;
                                    let target_name = declaration_by_id
                                        .get(&target_declaration)
                                        .map(|declaration| declaration.name.clone())
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel definition {} call `{}` forwarded OUT target {} has no declaration row",
                                                owner.0, syntax.function, target_declaration.0,
                                            ))
                                        })?;
                                    entries.push(CheckedCallEntry::ForwardOut {
                                        formal: parameter.decl_id,
                                        name: parameter.name.clone(),
                                        target: target_declaration,
                                        target_name,
                                    });
                                }
                            }
                        }
                    }
                }

                let mut contexts = Vec::with_capacity(target.contexts.len());
                for (context_ordinal, context) in target.contexts.iter().enumerate() {
                    let context_ordinal = u32::try_from(context_ordinal).map_err(|_| {
                        KernelCheckedLinkError::new(
                            "kernel checked call context ordinal exceeds u32",
                        )
                    })?;
                    let declaration = exact_local_declaration_by_origin(
                        definition,
                        crate::KernelDeclarationOrigin::CallContext {
                            call: call.expression,
                            ordinal: context_ordinal,
                        },
                        "call context",
                    )?;
                    if declaration.name.as_ref() != context.name {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} call `{}` context {} is named `{}` instead of `{}`",
                            owner.0,
                            syntax.function,
                            context_ordinal,
                            declaration.name,
                            context.name,
                        )));
                    }
                    let presentation = declaration_presentation(definition, declaration.id)?;
                    contexts.push(boon_checked::CheckedCallContext {
                        declaration: self.declaration(
                            owner,
                            KernelDeclarationReference::Local(declaration.id),
                        )?,
                        signature: context_ordinal as usize,
                        scope_id: self.scope(owner, presentation.scope)?,
                    });
                }

                let context_binding = if let Some(pass) = syntax.pass {
                    CheckedContextBinding::Explicit {
                        value: self.expression(owner, pass.value)?,
                        span: checked_span(pass.span),
                    }
                } else if let KernelCallTarget::User {
                    inherited_formal: Some(inherited),
                    ..
                } = call.target
                {
                    if definition.linkage.context_formal_ordinal != Some(inherited.caller_ordinal) {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} call `{}` inherits caller formal {} without matching linkage",
                            owner.0, syntax.function, inherited.caller_ordinal,
                        )));
                    }
                    CheckedContextBinding::Inherited {
                        formal: local.context_formal.ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} call `{}` inherits a missing context formal",
                                owner.0, syntax.function,
                            ))
                        })?,
                    }
                } else {
                    CheckedContextBinding::None
                };

                let (raw_substitutions, target_variables, target_type_variables, context) =
                    match call.target {
                        KernelCallTarget::User { target, .. } => {
                            let target_definition =
                                snapshot.definitions.get(target.0 as usize).ok_or_else(|| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel call `{}` references missing target definition {}",
                                        syntax.function, target.0,
                                    ))
                                })?;
                            let variables = callable_type_parameter_variables(
                                &target_definition.formals,
                                &target_definition.result,
                            );
                            let context = target_definition
                                .linkage
                                .context_formal_ordinal
                                .map(|ordinal| {
                                    let flow = target_definition
                                        .formals
                                        .get(ordinal as usize)
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel callable `{}` context ordinal {ordinal} is missing",
                                                syntax.function,
                                            ))
                                        })?;
                                    let formal = self
                                        .definition(target)?
                                        .context_formal
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel callable `{}` context has no checked formal",
                                                syntax.function,
                                            ))
                                        })?;
                                    Ok::<_, KernelCheckedLinkError>((
                                        formal,
                                        type_variables_in_flow(flow),
                                    ))
                                })
                                .transpose()?;
                            (
                                call.type_substitutions.as_ref().to_vec(),
                                variables,
                                self.definition(target)?.type_variables,
                                context,
                            )
                        }
                        KernelCallTarget::RenderConstructor { .. }
                        | KernelCallTarget::PureBuiltin { .. }
                        | KernelCallTarget::FixedAbi
                        | KernelCallTarget::HostEffect { .. }
                        | KernelCallTarget::FieldProjection { .. } => {
                            let contract =
                                project.abi().callable(&syntax.function).ok_or_else(|| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel call `{}` has no immutable ABI contract",
                                        syntax.function,
                                    ))
                                })?;
                            let actuals = call
                                .inputs
                                .iter()
                                .map(|input| {
                                    let parameter = parameter_for_input(input)?;
                                    let (_, actual) =
                                        value_flow_authority(snapshot, owner, input.value)?;
                                    Ok((parameter.ordinal as u32, actual.ty))
                                })
                                .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
                            let formals = contract
                                .parameters
                                .iter()
                                .map(|parameter| parameter.flow_type.clone())
                                .collect::<Vec<_>>();
                            let actual_result = matches!(
                                &call.target,
                                KernelCallTarget::PureBuiltin {
                                    kind: crate::KernelPureBuiltinKind::ListAppend
                                        | crate::KernelPureBuiltinKind::MapUpsert
                                        | crate::KernelPureBuiltinKind::SetAdd,
                                }
                            )
                            .then(|| {
                                value_flow_authority(
                                    snapshot,
                                    owner,
                                    KernelValueReference::Local(call.expression),
                                )
                                .map(|(_, result)| result)
                            })
                            .transpose()?;
                            let substitutions = derive_kernel_call_type_substitutions(
                                &formals,
                                &contract.result,
                                &actuals,
                                actual_result.as_ref().map(|result| &result.ty),
                            )
                            .into_vec();
                            (
                                substitutions,
                                callable_type_parameter_variables(&formals, &contract.result),
                                self.abi_callable(&syntax.function)?.type_variables,
                                None,
                            )
                        }
                    };
                let mut type_substitutions = Vec::with_capacity(raw_substitutions.len());
                let mut contextual_substitutions = Vec::new();
                for substitution in raw_substitutions {
                    let raw_variable = target_variables
                        .get(substitution.variable.0 as usize)
                        .copied()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel call `{}` substitution parameter {} is outside its target scheme",
                                syntax.function, substitution.variable.0,
                            ))
                        })?;
                    let variable = TypeVar(
                        target_type_variables
                            .resolve(raw_variable.0, "call target type variable")?,
                    );
                    let value = relocate_type(local.type_variables, &substitution.value)?;
                    type_substitutions.push(CheckedTypeSubstitution {
                        variable,
                        value: value.clone(),
                    });
                    if let Some((formal, context_variables)) = &context
                        && context_variables.contains(&raw_variable)
                    {
                        contextual_substitutions.push(CheckedContextTypeSubstitution {
                            formal: *formal,
                            variable,
                            value,
                        });
                    }
                }
                let result = self.relocate_flow_type(owner, &call.result)?;
                let syntax_discriminated_result = call.syntax_discriminated_result;
                let presentation = expression_presentation(definition, call.expression)?;
                let id = self.call(
                    owner,
                    u32::try_from(ordinal).map_err(|_| {
                        KernelCheckedLinkError::new("kernel checked call ordinal exceeds u32")
                    })?,
                )?;
                if id.0 as usize != calls.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked call materializer expected row {} but linked {}",
                        calls.len(),
                        id.0,
                    )));
                }
                calls.push(CheckedCall {
                    id,
                    expression: self
                        .expression(owner, KernelValueReference::Local(call.expression))?,
                    callable: callable_id,
                    owner_callable,
                    function: syntax.function.to_string(),
                    intrinsic: target.intrinsic,
                    entries,
                    contexts,
                    context_binding,
                    contextual_substitutions,
                    type_substitutions,
                    syntax_discriminated_result,
                    result,
                    role: target.role,
                    span: checked_span(presentation.span),
                });
                call_occurrences.push(syntax.occurrence.clone());
            }
        }
        self.validate_materialized_count("call", calls.len(), self.totals.calls)?;
        self.validate_materialized_count(
            "call occurrence",
            call_occurrences.len(),
            self.totals.calls,
        )?;
        Ok((
            calls.into_boxed_slice(),
            call_occurrences.into_boxed_slice(),
        ))
    }

    /// Emit every SOURCE resource directly from its solved definition row.
    ///
    /// The expression presentation owns the source coordinate and lexical
    /// scope. Resource identity, declaration authority, semantic path, and
    /// payload contract are already explicit in the immutable artifact, so
    /// this pass performs only dense relocation.
    pub fn materialize_sources(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedSource]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "SOURCE")?;
        let mut sources = Vec::with_capacity(self.totals.sources as usize);
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "SOURCE")?;
            for source in &definition.sources {
                let id = self.source(owner, source.id.0)?;
                if id.0 as usize != sources.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked SOURCE materializer expected row {} but linked {}",
                        sources.len(),
                        id.0,
                    )));
                }
                let presentation = expression_presentation(definition, source.expression)?;
                sources.push(CheckedSource {
                    id,
                    declaration: self.declaration(owner, source.declaration)?,
                    statement: self.statement(owner, source.statement)?,
                    expression: self
                        .expression(owner, KernelValueReference::Local(source.expression))?,
                    owner_scope: self.scope(owner, presentation.scope)?,
                    path: self.semantic_path(owner, &source.path)?,
                    interval_ms: source.interval_ms,
                    payload_type: relocate_type(
                        self.definition(owner)?.type_variables,
                        &source.payload_type,
                    )?,
                    span: checked_span(presentation.span),
                });
            }
        }
        self.validate_materialized_count("SOURCE", sources.len(), self.totals.sources)?;
        Ok(sources.into_boxed_slice())
    }

    /// Emit every persistent state row without rediscovering HOLD/LATEST
    /// structure from checked expressions.
    pub fn materialize_states(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedState]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "state")?;
        let mut states = Vec::with_capacity(self.totals.states as usize);
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "state")?;
            for state in &definition.states {
                let id = self.state(owner, state.id.0)?;
                if id.0 as usize != states.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked state materializer expected row {} but linked {}",
                        states.len(),
                        id.0,
                    )));
                }
                let (owner_scope, span) =
                    if state.kind == boon_checked::CheckedStateKind::StatementHold {
                        let (statement_owner, statement) =
                            self.local_statement_reference(snapshot, owner, state.statement)?;
                        let presentation = statement_presentation(
                            &snapshot.definitions[statement_owner.0 as usize],
                            statement,
                        )?;
                        (
                            self.scope(statement_owner, presentation.scope)?,
                            checked_span(presentation.span),
                        )
                    } else {
                        let presentation = expression_presentation(definition, state.expression)?;
                        (
                            self.scope(owner, presentation.scope)?,
                            checked_span(presentation.span),
                        )
                    };
                states.push(CheckedState {
                    id,
                    binding_declaration: self.declaration(owner, state.binding_declaration)?,
                    declaration: self.declaration(owner, state.declaration)?,
                    statement: self.statement(owner, state.statement)?,
                    expression: self
                        .expression(owner, KernelValueReference::Local(state.expression))?,
                    initial: self.expression(owner, state.initial)?,
                    owner_scope,
                    path: self.semantic_path(owner, &state.path)?,
                    kind: state.kind,
                    flow_type: self.relocate_flow_type(owner, &state.flow_type)?,
                    span,
                });
            }
        }
        self.validate_materialized_count("state", states.len(), self.totals.states)?;
        Ok(states.into_boxed_slice())
    }

    /// Emit every persistent LIST authority from the kernel's single solved
    /// resource table.
    pub fn materialize_lists(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedList]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "LIST")?;
        let mut lists = Vec::with_capacity(self.totals.lists as usize);
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "LIST")?;
            for list in &definition.lists {
                let id = self.list(owner, list.id.0)?;
                if id.0 as usize != lists.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked LIST materializer expected row {} but linked {}",
                        lists.len(),
                        id.0,
                    )));
                }
                let presentation = expression_presentation(definition, list.producer)?;
                lists.push(CheckedList {
                    id,
                    declaration: self.declaration(owner, list.declaration)?,
                    statement: self.statement(owner, list.statement)?,
                    producer: self.expression(owner, KernelValueReference::Local(list.producer))?,
                    owner_scope: self.scope(owner, presentation.scope)?,
                    path: self.semantic_path(owner, &list.path)?,
                    item_type: relocate_type(
                        self.definition(owner)?.type_variables,
                        &list.item_type,
                    )?,
                    capacity: list.capacity,
                    key_policy: list.key_policy,
                    span: checked_span(presentation.span),
                });
            }
        }
        self.validate_materialized_count("LIST", lists.len(), self.totals.lists)?;
        Ok(lists.into_boxed_slice())
    }

    fn validate_snapshot_definition_count(
        &self,
        snapshot: &KernelCheckedSnapshot,
        label: &str,
    ) -> Result<(), KernelCheckedLinkError> {
        if snapshot.definitions.len() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked {label} materializer has {} definitions for a {}-definition layout",
                snapshot.definitions.len(),
                self.definitions.len(),
            )));
        }
        Ok(())
    }

    fn validate_materialized_count(
        &self,
        label: &str,
        actual: usize,
        expected: u32,
    ) -> Result<(), KernelCheckedLinkError> {
        if actual != expected as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked {label} materializer produced {actual} rows for a {expected}-row layout",
            )));
        }
        Ok(())
    }

    fn semantic_path(
        &self,
        owner: KernelOwnerId,
        path: &crate::KernelSemanticPath,
    ) -> Result<CheckedSemanticPath, KernelCheckedLinkError> {
        Ok(CheckedSemanticPath {
            anchor: self.declaration(owner, path.anchor)?,
            projection: path
                .projection
                .iter()
                .map(|field| field.to_string())
                .collect(),
        })
    }

    fn local_statement_reference(
        &self,
        snapshot: &KernelCheckedSnapshot,
        owner: KernelOwnerId,
        statement: KernelStatementReference,
    ) -> Result<(KernelOwnerId, crate::KernelStatementId), KernelCheckedLinkError> {
        match statement {
            KernelStatementReference::Local(statement) => Ok((owner, statement)),
            KernelStatementReference::OwnerPublic(owner) => {
                let statement = snapshot
                    .definitions
                    .get(owner.0 as usize)
                    .and_then(|definition| definition.linkage.root_statement)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel state references definition {} without a root statement",
                            owner.0,
                        ))
                    })?;
                Ok((owner, statement))
            }
        }
    }

    pub fn declaration(
        &self,
        owner: KernelOwnerId,
        declaration: KernelDeclarationReference,
    ) -> Result<DeclId, KernelCheckedLinkError> {
        match declaration {
            KernelDeclarationReference::Local(declaration) => Ok(DeclId(
                self.definition(owner)?
                    .declarations
                    .resolve(declaration.0, "declaration")?,
            )),
            KernelDeclarationReference::OwnerPublic(owner) => {
                Ok(self.definition(owner)?.public_declaration)
            }
            KernelDeclarationReference::OwnerDeclaration { owner, declaration } => Ok(DeclId(
                self.definition(owner)?
                    .declarations
                    .resolve(declaration.0, "external declaration")?,
            )),
        }
    }

    /// Relocate one definition-local alpha-normalized flow into the global
    /// checked type-variable namespace. This same mapping is reused by every
    /// direct row family, so declarations and their later expression/call
    /// consumers cannot accidentally diverge.
    pub fn relocate_flow_type(
        &self,
        owner: KernelOwnerId,
        flow_type: &FlowType,
    ) -> Result<FlowType, KernelCheckedLinkError> {
        Ok(FlowType {
            mode: flow_type.mode,
            ty: relocate_type(self.definition(owner)?.type_variables, &flow_type.ty)?,
        })
    }

    pub fn statement(
        &self,
        owner: KernelOwnerId,
        statement: KernelStatementReference,
    ) -> Result<CheckedStatementId, KernelCheckedLinkError> {
        match statement {
            KernelStatementReference::Local(statement) => Ok(CheckedStatementId(
                self.definition(owner)?
                    .statements
                    .resolve(statement.0, "statement")?,
            )),
            KernelStatementReference::OwnerPublic(owner) => {
                Ok(self.definition(owner)?.root_statement)
            }
        }
    }

    pub fn call(
        &self,
        owner: KernelOwnerId,
        ordinal: u32,
    ) -> Result<CheckedCallId, KernelCheckedLinkError> {
        Ok(CheckedCallId(
            self.definition(owner)?.calls.resolve(ordinal, "call")?,
        ))
    }

    pub fn source(
        &self,
        owner: KernelOwnerId,
        ordinal: u32,
    ) -> Result<CheckedSourceId, KernelCheckedLinkError> {
        Ok(CheckedSourceId(
            self.definition(owner)?.sources.resolve(ordinal, "source")?,
        ))
    }

    pub fn state(
        &self,
        owner: KernelOwnerId,
        ordinal: u32,
    ) -> Result<CheckedStateId, KernelCheckedLinkError> {
        Ok(CheckedStateId(
            self.definition(owner)?.states.resolve(ordinal, "state")?,
        ))
    }

    pub fn list(
        &self,
        owner: KernelOwnerId,
        ordinal: u32,
    ) -> Result<CheckedListId, KernelCheckedLinkError> {
        Ok(CheckedListId(
            self.definition(owner)?.lists.resolve(ordinal, "list")?,
        ))
    }

    fn validate_references(
        &mut self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<(), KernelCheckedLinkError> {
        let mut resolved = 0_u64;
        for (index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(index).expect("definition index fits u32"));
            let local = self.definition(owner)?.clone();
            let _ = self.scope(owner, definition.presentation.containing_scope)?;
            for scope in &definition.presentation.scopes {
                let _ = local.scopes.resolve(scope.id.0, "scope presentation")?;
                let _ = self.scope(owner, scope.parent)?;
                if let Some(declaration) = scope.owner {
                    let _ = self.declaration(owner, declaration)?;
                    resolved = resolved.saturating_add(1);
                }
                resolved = resolved.saturating_add(1);
            }
            for expression in &definition.presentation.expressions {
                let _ = local
                    .expressions
                    .resolve(expression.expression.0, "expression presentation")?;
                let _ = self.scope(owner, expression.scope)?;
                if let Some(declaration) = expression.declaration {
                    let _ = self.declaration(owner, declaration)?;
                    resolved = resolved.saturating_add(1);
                }
                resolved = resolved.saturating_add(1);
            }
            for statement in &definition.presentation.statements {
                let _ = local
                    .statements
                    .resolve(statement.statement.0, "statement presentation")?;
                let _ = self.scope(owner, statement.scope)?;
                if let Some(body) = statement.body_scope {
                    let _ = local.scopes.resolve(body.0, "statement body scope")?;
                    resolved = resolved.saturating_add(1);
                }
                resolved = resolved.saturating_add(1);
            }
            for declaration in &definition.presentation.declarations {
                let _ = local
                    .declarations
                    .resolve(declaration.declaration.0, "declaration presentation")?;
                let _ = self.scope(owner, declaration.scope)?;
                if let Some(body) = declaration.body_scope {
                    let _ = local.scopes.resolve(body.0, "declaration body scope")?;
                    resolved = resolved.saturating_add(1);
                }
                resolved = resolved.saturating_add(1);
            }
            for expression in &definition.expressions {
                let _ = local
                    .expressions
                    .resolve(expression.id.0, "expression artifact")?;
                for input in &expression.inputs {
                    let _ = self.expression(owner, input.value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for statement in &definition.statements {
                let _ = local
                    .statements
                    .resolve(statement.id.0, "statement artifact")?;
                if let Some(value) = statement.value {
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
                for child in &statement.children {
                    match child {
                        KernelStatementChildReference::Local(child) => {
                            let _ = local.statements.resolve(child.0, "statement child")?;
                        }
                        KernelStatementChildReference::Owner(child) => {
                            let _ = self.definition(*child)?.root_statement;
                        }
                    }
                    resolved = resolved.saturating_add(1);
                }
            }
            for declaration in &definition.declarations {
                let _ = local
                    .declarations
                    .resolve(declaration.id.0, "declaration artifact")?;
                if let Some(value) = declaration.value {
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for binding in &definition.lexical_bindings {
                let _ = local
                    .expressions
                    .resolve(binding.expression.0, "lexical occurrence")?;
                match binding.target {
                    KernelLexicalBindingTarget::Declaration(declaration) => {
                        let _ = self.declaration(owner, declaration)?;
                    }
                    KernelLexicalBindingTarget::ContextFormal { ordinal } => {
                        if definition.linkage.context_formal_ordinal != Some(ordinal)
                            || local.context_formal.is_none()
                        {
                            return Err(KernelCheckedLinkError::new(format!(
                                "kernel definition {} lexical context ordinal {ordinal} has no exact context-formal anchor",
                                owner.0,
                            )));
                        }
                    }
                    KernelLexicalBindingTarget::Value { provider } => {
                        let _ = self.expression(owner, provider)?;
                    }
                    KernelLexicalBindingTarget::RuntimeContext => {}
                }
                resolved = resolved.saturating_add(1);
            }
            for (ordinal, call) in definition.calls.iter().enumerate() {
                let _ = self.call(
                    owner,
                    u32::try_from(ordinal).map_err(|_| {
                        KernelCheckedLinkError::new("kernel call ordinal exceeds u32")
                    })?,
                )?;
                let _ = local
                    .expressions
                    .resolve(call.expression.0, "call expression")?;
                if let KernelCallTarget::User { target, .. } = call.target {
                    let _ = self.definition(target)?.public_declaration;
                    resolved = resolved.saturating_add(1);
                }
                for input in &call.inputs {
                    let _ = self.expression(owner, input.value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for call in &definition.call_syntax {
                let _ = local
                    .expressions
                    .resolve(call.expression.0, "authored call expression")?;
                if let Some(value) = call.pipe_input {
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
                for argument in &call.arguments {
                    let _ = self.expression(owner, argument.value)?;
                    resolved = resolved.saturating_add(1);
                }
                if let Some(pass) = call.pass {
                    let _ = self.expression(owner, pass.value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for shape in &definition.execution_shapes {
                let _ = local
                    .expressions
                    .resolve(shape.expression().0, "execution-shape expression")?;
                match shape {
                    crate::KernelExecutionShapeArtifact::Conditional { .. } => {}
                    crate::KernelExecutionShapeArtifact::Record { fields, .. } => {
                        for field in fields {
                            if let Some(declaration) = field.declaration {
                                let _ = self.declaration(owner, declaration)?;
                                resolved = resolved.saturating_add(1);
                            }
                            let _ = self.expression(owner, field.value)?;
                            resolved = resolved.saturating_add(1);
                        }
                    }
                    crate::KernelExecutionShapeArtifact::Block {
                        bindings, result, ..
                    } => {
                        for binding in bindings {
                            let _ = self.declaration(owner, binding.declaration)?;
                            let _ = self.expression(owner, binding.value)?;
                            resolved = resolved.saturating_add(2);
                        }
                        if let Some(result) = result {
                            let _ = self.expression(owner, *result)?;
                            resolved = resolved.saturating_add(1);
                        }
                    }
                    crate::KernelExecutionShapeArtifact::MatchArm {
                        selector, bindings, ..
                    } => {
                        let _ = self.expression(owner, *selector)?;
                        resolved = resolved.saturating_add(1);
                        for binding in bindings {
                            let _ = local
                                .declarations
                                .resolve(binding.0, "match binding declaration")?;
                            resolved = resolved.saturating_add(1);
                        }
                    }
                }
            }
            for source in &definition.sources {
                let _ = self.source(owner, source.id.0)?;
                let _ = self.declaration(owner, source.declaration)?;
                let _ = self.statement(owner, source.statement)?;
                let _ = local
                    .expressions
                    .resolve(source.expression.0, "source expression")?;
                let _ = self.declaration(owner, source.path.anchor)?;
                resolved = resolved.saturating_add(4);
            }
            for state in &definition.states {
                let _ = self.state(owner, state.id.0)?;
                let _ = self.declaration(owner, state.binding_declaration)?;
                let _ = self.declaration(owner, state.declaration)?;
                let _ = self.statement(owner, state.statement)?;
                let _ = local
                    .expressions
                    .resolve(state.expression.0, "state expression")?;
                let _ = self.expression(owner, state.initial)?;
                let _ = self.declaration(owner, state.path.anchor)?;
                resolved = resolved.saturating_add(6);
            }
            for list in &definition.lists {
                let _ = self.list(owner, list.id.0)?;
                let _ = self.declaration(owner, list.declaration)?;
                let _ = self.statement(owner, list.statement)?;
                let _ = local
                    .expressions
                    .resolve(list.producer.0, "list producer")?;
                let _ = self.declaration(owner, list.path.anchor)?;
                resolved = resolved.saturating_add(4);
            }
            for effect in &definition.effects {
                let _ = local
                    .expressions
                    .resolve(effect.expression.0, "host-effect expression")?;
                resolved = resolved.saturating_add(1);
            }
        }
        self.totals.resolved_references = resolved;
        Ok(())
    }
}

fn checked_owner_id(index: usize, label: &str) -> Result<KernelOwnerId, KernelCheckedLinkError> {
    Ok(KernelOwnerId(u32::try_from(index).map_err(|_| {
        KernelCheckedLinkError::new(format!(
            "kernel checked {label} materializer definition count exceeds u32",
        ))
    })?))
}

fn definition_template_value(
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    value: KernelValueReference,
) -> Result<(KernelOwnerId, crate::KernelExpressionId), KernelCheckedLinkError> {
    match value {
        KernelValueReference::Local(expression) => Ok((owner, expression)),
        KernelValueReference::External(external) => match external.target {
            KernelExternalTarget::Expression(expression) => Ok((external.owner, expression)),
            KernelExternalTarget::Result => {
                let definition = snapshot
                    .definitions
                    .get(external.owner.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition template references missing external owner {}",
                            external.owner.0,
                        ))
                    })?;
                let expression = definition.linkage.result_expression.ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition template references owner {} without a result expression",
                        external.owner.0,
                    ))
                })?;
                Ok((external.owner, expression))
            }
        },
    }
}

fn definition_template_dependencies(
    snapshot: &KernelCheckedSnapshot,
    statement_child_dependencies: &BTreeMap<
        (KernelOwnerId, crate::KernelExpressionId),
        Vec<(KernelOwnerId, crate::KernelExpressionId)>,
    >,
    call_dependencies: &BTreeMap<
        (KernelOwnerId, crate::KernelExpressionId),
        Vec<(KernelOwnerId, crate::KernelExpressionId)>,
    >,
    read_provider_callables: &BTreeMap<(KernelOwnerId, crate::KernelExpressionId), Option<DeclId>>,
    callable: DeclId,
    owner: KernelOwnerId,
    expression: &crate::KernelExpressionArtifact,
) -> Result<Vec<(KernelOwnerId, crate::KernelExpressionId)>, KernelCheckedLinkError> {
    let definition = snapshot.definitions.get(owner.0 as usize).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition template references missing owner {}",
            owner.0,
        ))
    })?;
    let payload = definition
        .expression_payloads
        .get(expression.id.0 as usize)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel definition {} template expression {} has no semantic payload",
                owner.0, expression.id.0,
            ))
        })?;
    // A non-empty authored delimiter is structurally interpreted as a record
    // by the type engine, but remains a delimiter in the checked expression
    // graph. Its field values are owned by child statements, not expression
    // dependencies. Preserve that checked execution boundary here.
    if matches!(payload, crate::KernelExpressionSemanticPayload::Delimiter) {
        return Ok(Vec::new());
    }
    if let Some(dependencies) = call_dependencies.get(&(owner, expression.id)) {
        return Ok(dependencies.clone());
    }
    let mut dependencies = Vec::new();
    for input in &expression.inputs {
        if matches!(
            input.role,
            crate::KernelOwnerEdgeRole::CallOutArgument { .. }
                | crate::KernelOwnerEdgeRole::HoldUpdate
        ) {
            continue;
        }
        if matches!(input.role, crate::KernelOwnerEdgeRole::ReadProvider) {
            let read_callable = read_provider_callables
                .get(&(owner, expression.id))
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} template read {} has no lexical callable authority",
                        owner.0, expression.id.0,
                    ))
                })?;
            if *read_callable != Some(callable) {
                continue;
            }
        }
        dependencies.push(definition_template_value(snapshot, owner, input.value)?);
    }
    if matches!(
        expression.kind,
        crate::KernelOwnerNodeKind::Hold | crate::KernelOwnerNodeKind::MatchArm { .. }
    ) {
        dependencies.extend(
            statement_child_dependencies
                .get(&(owner, expression.id))
                .into_iter()
                .flatten()
                .copied(),
        );
    }
    if matches!(expression.kind, crate::KernelOwnerNodeKind::Block) {
        let mut shapes = definition.execution_shapes.iter().filter(|shape| {
            matches!(
                shape,
                crate::KernelExecutionShapeArtifact::Block {
                    expression: candidate,
                    ..
                } if *candidate == expression.id
            )
        });
        let Some(crate::KernelExecutionShapeArtifact::Block {
            bindings, result, ..
        }) = shapes.next()
        else {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} BLOCK expression {} has no execution shape",
                owner.0, expression.id.0,
            )));
        };
        if shapes.next().is_some() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} repeats a BLOCK shape for expression {}",
                owner.0, expression.id.0,
            )));
        }
        dependencies.extend(
            bindings
                .iter()
                .map(|binding| definition_template_value(snapshot, owner, binding.value))
                .collect::<Result<Vec<_>, _>>()?,
        );
        if let Some(result) = result {
            dependencies.push(definition_template_value(snapshot, owner, *result)?);
        }
    }
    Ok(dependencies)
}

fn definition_template_read_provider_callables(
    snapshot: &KernelCheckedSnapshot,
    layout: &KernelCheckedLinkLayout,
    linked_scopes: &[CheckedScope],
    linked_declarations: &[CheckedDeclaration],
) -> Result<
    BTreeMap<(KernelOwnerId, crate::KernelExpressionId), Option<DeclId>>,
    KernelCheckedLinkError,
> {
    let scopes = linked_scopes
        .iter()
        .map(|scope| (scope.id, scope))
        .collect::<BTreeMap<_, _>>();
    if scopes.len() != linked_scopes.len() {
        return Err(KernelCheckedLinkError::new(
            "kernel definition templates received duplicate linked scope IDs",
        ));
    }
    let declarations = linked_declarations
        .iter()
        .map(|declaration| (declaration.id, declaration))
        .collect::<BTreeMap<_, _>>();
    if declarations.len() != linked_declarations.len() {
        return Err(KernelCheckedLinkError::new(
            "kernel definition templates received duplicate linked declaration IDs",
        ));
    }
    let mut callable_by_scope = BTreeMap::new();
    for scope in linked_scopes {
        let mut current = scope.id;
        let mut visited = BTreeSet::new();
        let callable = loop {
            if !visited.insert(current) {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition template scope {} contains a parent cycle",
                    scope.id.0,
                )));
            }
            let definition = scopes.get(&current).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition template references missing linked scope {}",
                    current.0,
                ))
            })?;
            if definition.kind == CheckedScopeKind::Function {
                break definition.owner;
            }
            let Some(parent) = definition.parent else {
                break None;
            };
            current = parent;
        };
        callable_by_scope.insert(scope.id, callable);
    }

    let mut result = BTreeMap::new();
    for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
        let owner = checked_owner_id(owner_index, "definition template lexical read")?;
        for binding in &definition.lexical_bindings {
            if !definition
                .expressions
                .get(binding.expression.0 as usize)
                .filter(|expression| expression.id == binding.expression)
                .is_some_and(|expression| {
                    expression
                        .inputs
                        .iter()
                        .any(|input| matches!(input.role, crate::KernelOwnerEdgeRole::ReadProvider))
                })
            {
                continue;
            }
            let callable = match binding.target {
                KernelLexicalBindingTarget::Declaration(reference) => {
                    let declaration = layout.declaration(owner, reference)?;
                    let declaration = declarations.get(&declaration).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} template read {} targets missing declaration {}",
                            owner.0, binding.expression.0, declaration.0,
                        ))
                    })?;
                    if declaration.value.is_none() {
                        None
                    } else {
                        callable_by_scope
                            .get(&declaration.scope_id)
                            .copied()
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition template declaration {} references missing scope {}",
                                    declaration.id.0, declaration.scope_id.0,
                                ))
                            })?
                    }
                }
                KernelLexicalBindingTarget::ContextFormal { .. }
                | KernelLexicalBindingTarget::Value { .. }
                | KernelLexicalBindingTarget::RuntimeContext => None,
            };
            if result
                .insert((owner, binding.expression), callable)
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} repeats lexical callable authority for expression {}",
                    owner.0, binding.expression.0,
                )));
            }
        }
    }
    Ok(result)
}

fn definition_template_statement_child_dependencies(
    snapshot: &KernelCheckedSnapshot,
    layout: &KernelCheckedLinkLayout,
    linked_statements: &[CheckedStatement],
) -> Result<
    BTreeMap<
        (KernelOwnerId, crate::KernelExpressionId),
        Vec<(KernelOwnerId, crate::KernelExpressionId)>,
    >,
    KernelCheckedLinkError,
> {
    let mut expression_keys = BTreeMap::new();
    for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
        let owner = checked_owner_id(owner_index, "definition template expression reverse map")?;
        for expression in &definition.expressions {
            let linked = layout.expression(owner, KernelValueReference::Local(expression.id))?;
            if expression_keys
                .insert(linked, (owner, expression.id))
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition templates repeat linked expression {}",
                    linked.0,
                )));
            }
        }
    }
    let statements = linked_statements
        .iter()
        .map(|statement| (statement.id, statement))
        .collect::<BTreeMap<_, _>>();
    if statements.len() != linked_statements.len() {
        return Err(KernelCheckedLinkError::new(
            "kernel definition templates received duplicate linked statement IDs",
        ));
    }
    let mut statements_by_value = BTreeMap::<CheckedExprId, Vec<CheckedStatementId>>::new();
    for statement in linked_statements {
        if let Some(value) = statement.value {
            statements_by_value
                .entry(value)
                .or_default()
                .push(statement.id);
        }
    }

    let mut result = BTreeMap::new();
    for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
        let owner = checked_owner_id(owner_index, "definition template statement dependency")?;
        for expression in definition.expressions.iter().filter(|expression| {
            matches!(
                expression.kind,
                crate::KernelOwnerNodeKind::Hold | crate::KernelOwnerNodeKind::MatchArm { .. }
            )
        }) {
            let linked = layout.expression(owner, KernelValueReference::Local(expression.id))?;
            let values = linked_definition_template_statement_child_values(
                &statements,
                &statements_by_value,
                linked,
            )?;
            let dependencies = values
                .into_iter()
                .map(|value| {
                    expression_keys.get(&value).copied().ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition template statement dependency references missing expression {}",
                            value.0,
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            result.insert((owner, expression.id), dependencies);
        }
    }
    Ok(result)
}

fn linked_definition_template_statement_child_values(
    statements: &BTreeMap<CheckedStatementId, &CheckedStatement>,
    statements_by_value: &BTreeMap<CheckedExprId, Vec<CheckedStatementId>>,
    expression: CheckedExprId,
) -> Result<Vec<CheckedExprId>, KernelCheckedLinkError> {
    let Some(root) = statements_by_value
        .get(&expression)
        .into_iter()
        .flatten()
        .filter_map(|statement| statements.get(statement).copied())
        .find(|statement| statement.value == Some(expression) && !statement.children.is_empty())
    else {
        return Ok(Vec::new());
    };
    let mut pending = root.children.iter().rev().copied().collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut values = Vec::new();
    while let Some(statement) = pending.pop() {
        if !visited.insert(statement) {
            continue;
        }
        let definition = statements.get(&statement).copied().ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel definition template references missing linked statement {}",
                statement.0,
            ))
        })?;
        match definition.value {
            Some(value) if value == expression => {
                pending.extend(definition.children.iter().rev().copied());
            }
            Some(value) => values.push(value),
            None => pending.extend(definition.children.iter().rev().copied()),
        }
    }
    Ok(values)
}

fn definition_template_selector(
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    expression: &crate::KernelExpressionArtifact,
    layout: &KernelCheckedLinkLayout,
) -> Result<Option<CheckedDefinitionSelectorV1>, KernelCheckedLinkError> {
    let definition = snapshot.definitions.get(owner.0 as usize).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition template references missing owner {}",
            owner.0,
        ))
    })?;
    let mut matching_shapes = definition.execution_shapes.iter().filter(|shape| {
        matches!(
            shape,
            crate::KernelExecutionShapeArtifact::Conditional {
                expression: candidate,
                ..
            } if *candidate == expression.id
        )
    });
    let Some(shape) = matching_shapes.next() else {
        return Ok(None);
    };
    if matching_shapes.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} repeats a conditional shape for expression {}",
            owner.0, expression.id.0,
        )));
    }
    let crate::KernelExecutionShapeArtifact::Conditional { kind, .. } = shape else {
        unreachable!()
    };
    if *kind == crate::KernelConditionalKind::While {
        return Ok(None);
    }

    let mut selector_inputs = expression
        .inputs
        .iter()
        .filter(|input| matches!(input.role, crate::KernelOwnerEdgeRole::WhenInput));
    let selector = selector_inputs.next().ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition {} WHEN expression {} has no selector input",
            owner.0, expression.id.0,
        ))
    })?;
    if selector_inputs.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} WHEN expression {} has multiple selector inputs",
            owner.0, expression.id.0,
        )));
    }
    let (selector_owner, selector_expression) =
        definition_template_value(snapshot, owner, selector.value)?;
    let input = layout.expression(
        selector_owner,
        KernelValueReference::Local(selector_expression),
    )?;
    let arms = expression
        .inputs
        .iter()
        .filter(|input| matches!(input.role, crate::KernelOwnerEdgeRole::WhenArm))
        .map(|arm| {
            let (arm_owner, arm_expression) =
                definition_template_value(snapshot, owner, arm.value)?;
            layout.expression(arm_owner, KernelValueReference::Local(arm_expression))
        })
        .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
    Ok(Some(CheckedDefinitionSelectorV1 { input, arms }))
}

fn expression_presentation(
    definition: &crate::DefinitionArtifact,
    expression: crate::KernelExpressionId,
) -> Result<&crate::KernelExpressionPresentation, KernelCheckedLinkError> {
    definition
        .presentation
        .expressions
        .get(expression.0 as usize)
        .filter(|presentation| presentation.expression == expression)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel resource expression {} has no exact presentation row",
                expression.0,
            ))
        })
}

fn statement_presentation(
    definition: &crate::DefinitionArtifact,
    statement: crate::KernelStatementId,
) -> Result<&crate::KernelStatementPresentation, KernelCheckedLinkError> {
    definition
        .presentation
        .statements
        .get(statement.0 as usize)
        .filter(|presentation| presentation.statement == statement)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel resource statement {} has no exact presentation row",
                statement.0,
            ))
        })
}

fn declaration_presentation(
    definition: &crate::DefinitionArtifact,
    declaration: crate::KernelDeclarationId,
) -> Result<&crate::KernelDeclarationPresentation, KernelCheckedLinkError> {
    definition
        .presentation
        .declarations
        .get(declaration.0 as usize)
        .filter(|presentation| presentation.declaration == declaration)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel declaration {} has no exact presentation row",
                declaration.0,
            ))
        })
}

fn exact_local_declaration_by_origin<'a>(
    definition: &'a crate::DefinitionArtifact,
    origin: crate::KernelDeclarationOrigin,
    label: &str,
) -> Result<&'a crate::KernelDeclarationArtifact, KernelCheckedLinkError> {
    let mut declarations = definition
        .declarations
        .iter()
        .filter(|declaration| declaration.origin == origin);
    let declaration = declarations.next().ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel checked linker cannot find {label} declaration for {origin:?}"
        ))
    })?;
    if declarations.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel checked linker found multiple {label} declarations for {origin:?}"
        )));
    }
    Ok(declaration)
}

fn checked_range(
    range: KernelCheckedRowRange,
) -> Result<std::ops::Range<usize>, KernelCheckedLinkError> {
    let start = range.start as usize;
    let end = range
        .start
        .checked_add(range.len)
        .ok_or_else(|| KernelCheckedLinkError::new("kernel checked row range overflowed"))?
        as usize;
    Ok(start..end)
}

fn rebase_checked_span(
    span: &mut CheckedSpan,
    start_line: usize,
    start_byte: usize,
    label: &str,
) -> Result<(), KernelCheckedLinkError> {
    span.line =
        start_line
            .checked_add(span.line.checked_sub(1).ok_or_else(|| {
                KernelCheckedLinkError::new(format!("{label} has no source line"))
            })?)
            .ok_or_else(|| KernelCheckedLinkError::new(format!("{label} line overflowed")))?;
    span.start = start_byte
        .checked_add(span.start)
        .ok_or_else(|| KernelCheckedLinkError::new(format!("{label} start overflowed")))?;
    span.end = start_byte
        .checked_add(span.end)
        .ok_or_else(|| KernelCheckedLinkError::new(format!("{label} end overflowed")))?;
    Ok(())
}

fn rebase_checked_expression_spans(
    expression: &mut CheckedExpression,
    start_line: usize,
    start_byte: usize,
) -> Result<(), KernelCheckedLinkError> {
    rebase_checked_span(
        &mut expression.span,
        start_line,
        start_byte,
        &format!("kernel checked expression row {}", expression.id.0),
    )?;
    let structural_fields: &mut [_] = match &mut expression.kind {
        CheckedExpressionKind::TaggedObject { fields, .. }
        | CheckedExpressionKind::Object { fields } => fields.as_mut_slice(),
        _ => &mut [],
    };
    for (ordinal, field) in structural_fields.iter_mut().enumerate() {
        rebase_checked_span(
            &mut field.span,
            start_line,
            start_byte,
            &format!(
                "kernel checked expression row {} field {ordinal}",
                expression.id.0,
            ),
        )?;
    }
    if let CheckedExpressionKind::Block { bindings, .. } = &mut expression.kind {
        for (ordinal, binding) in bindings.iter_mut().enumerate() {
            rebase_checked_span(
                &mut binding.span,
                start_line,
                start_byte,
                &format!(
                    "kernel checked expression row {} BLOCK binding {ordinal}",
                    expression.id.0,
                ),
            )?;
        }
    }
    Ok(())
}

fn checked_span(span: crate::KernelSourceSpan) -> CheckedSpan {
    CheckedSpan {
        line: span.line,
        start: span.start,
        end: span.end,
    }
}

fn checked_declaration_kind(kind: crate::KernelDeclarationKind) -> CheckedDeclarationKind {
    match kind {
        crate::KernelDeclarationKind::Function => CheckedDeclarationKind::Function,
        crate::KernelDeclarationKind::ValueParameter => CheckedDeclarationKind::ValueParameter,
        crate::KernelDeclarationKind::OutParameter => CheckedDeclarationKind::OutParameter,
        crate::KernelDeclarationKind::Field => CheckedDeclarationKind::Field,
        crate::KernelDeclarationKind::Source => CheckedDeclarationKind::Source,
        crate::KernelDeclarationKind::Hold => CheckedDeclarationKind::Hold,
        crate::KernelDeclarationKind::List => CheckedDeclarationKind::List,
        crate::KernelDeclarationKind::PatternBinding => CheckedDeclarationKind::PatternBinding,
        crate::KernelDeclarationKind::FreshOut => CheckedDeclarationKind::FreshOut,
        crate::KernelDeclarationKind::ElementState => CheckedDeclarationKind::ElementState,
    }
}

fn statement_declaration_authority(
    definition: &crate::DefinitionArtifact,
    statement: &crate::KernelStatementArtifact,
) -> Result<Option<KernelDeclarationReference>, KernelCheckedLinkError> {
    let mut declarations = definition.declarations.iter().filter_map(|declaration| {
        matches!(
            declaration.origin,
            crate::KernelDeclarationOrigin::Statement { statement: candidate }
                if candidate == statement.id
        )
        .then_some(KernelDeclarationReference::Local(declaration.id))
    });
    let declaration = declarations.next();
    if declarations.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel statement {} has more than one declaration authority",
            statement.id.0,
        )));
    }
    let owns_authored_declaration = matches!(
        &statement.kind,
        crate::KernelStatementKind::Function { .. }
            | crate::KernelStatementKind::Field { .. }
            | crate::KernelStatementKind::Source { field: Some(_), .. }
            | crate::KernelStatementKind::Hold { field: Some(_), .. }
            | crate::KernelStatementKind::List { field: Some(_), .. }
    );
    Ok(declaration.or_else(|| {
        (owns_authored_declaration && definition.linkage.root_statement == Some(statement.id))
            .then_some(definition.linkage.public_declaration)
            .flatten()
    }))
}

fn checked_statement_kind(
    kind: &crate::KernelStatementKind,
    declaration: Option<DeclId>,
) -> Result<CheckedStatementKind, KernelCheckedLinkError> {
    Ok(match kind {
        crate::KernelStatementKind::Function { .. } => CheckedStatementKind::Function {
            declaration: declaration.ok_or_else(|| {
                KernelCheckedLinkError::new("kernel function statement has no declaration")
            })?,
        },
        crate::KernelStatementKind::Field { .. } => CheckedStatementKind::Field {
            declaration: declaration.ok_or_else(|| {
                KernelCheckedLinkError::new("kernel field statement has no declaration")
            })?,
        },
        crate::KernelStatementKind::Source { event, .. } => CheckedStatementKind::Source {
            declaration,
            event: event.as_deref().map(str::to_owned),
        },
        crate::KernelStatementKind::Hold { name, .. } => CheckedStatementKind::Hold {
            declaration,
            name: name.as_deref().map(str::to_owned),
        },
        crate::KernelStatementKind::List { capacity, .. } => CheckedStatementKind::List {
            declaration,
            capacity: *capacity,
        },
        crate::KernelStatementKind::Block => CheckedStatementKind::Block,
        crate::KernelStatementKind::Spread => CheckedStatementKind::Spread,
        crate::KernelStatementKind::Expression => CheckedStatementKind::Expression,
    })
}

#[allow(clippy::too_many_arguments)]
fn checked_expression_kind(
    layout: &KernelCheckedLinkLayout,
    owner: KernelOwnerId,
    definition: &crate::DefinitionArtifact,
    expression: &crate::KernelExpressionArtifact,
    container_line: usize,
    container_declaration: Option<DeclId>,
    payload: &crate::KernelExpressionSemanticPayload,
    shape: Option<&crate::KernelExecutionShapeArtifact>,
    lexical: Option<&crate::KernelLexicalBindingArtifact>,
    call: Option<u32>,
    source_paths: &BTreeMap<DeclId, Vec<(Vec<String>, CheckedSourceId)>>,
) -> Result<CheckedExpressionKind, KernelCheckedLinkError> {
    if let Some(call) = call {
        return Ok(CheckedExpressionKind::Call {
            call: layout.call(owner, call)?,
        });
    }
    if let Some(binding) = lexical {
        let projection = binding
            .projection
            .iter()
            .map(|field| field.to_string())
            .collect::<Vec<_>>();
        return match &binding.target {
            KernelLexicalBindingTarget::Declaration(target) => {
                let target = layout.declaration(owner, *target)?;
                Ok(match binding.access {
                    crate::KernelLexicalAccess::Read => {
                        let (target, projection, source) =
                            canonical_checked_source_read(source_paths, target, projection);
                        CheckedExpressionKind::Read {
                            target,
                            source,
                            projection,
                        }
                    }
                    crate::KernelLexicalAccess::Drain => {
                        let (target, projection, _) =
                            canonical_checked_source_read(source_paths, target, projection);
                        CheckedExpressionKind::Drain { target, projection }
                    }
                })
            }
            KernelLexicalBindingTarget::ContextFormal { ordinal } => {
                let owner_layout = layout.definition(owner)?;
                if definition.linkage.context_formal_ordinal != Some(*ordinal) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression {} reads context ordinal {} but owns {:?}",
                        owner.0,
                        expression.id.0,
                        ordinal,
                        definition.linkage.context_formal_ordinal,
                    )));
                }
                Ok(CheckedExpressionKind::Passed {
                    formal: owner_layout.context_formal.ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} has no linked context formal",
                            owner.0,
                        ))
                    })?,
                    projection,
                    access: match binding.access {
                        crate::KernelLexicalAccess::Read => CheckedPassedAccess::Read,
                        crate::KernelLexicalAccess::Drain => CheckedPassedAccess::Drain,
                    },
                })
            }
            KernelLexicalBindingTarget::RuntimeContext => {
                if binding.access == crate::KernelLexicalAccess::Drain {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression {} drains an ABI runtime context",
                        owner.0, expression.id.0,
                    )));
                }
                Ok(CheckedExpressionKind::ExternalRead {
                    canonical_path: lexical_payload_path(payload).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} runtime-context expression {} has no lexical path",
                            owner.0, expression.id.0,
                        ))
                    })?,
                    external_identity: None,
                })
            }
            KernelLexicalBindingTarget::Value { provider } => {
                Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} expression {} has an unanchored lexical value provider {provider:?}",
                    owner.0, expression.id.0,
                )))
            }
        };
    }
    if matches!(payload, crate::KernelExpressionSemanticPayload::Delimiter) {
        return Ok(CheckedExpressionKind::Delimiter);
    }
    if let crate::KernelExpressionSemanticPayload::Invalid(tokens) = payload
        && matches!(
            expression.kind,
            crate::KernelOwnerNodeKind::Number
                | crate::KernelOwnerNodeKind::Byte
                | crate::KernelOwnerNodeKind::Bits(_)
        )
    {
        return Ok(CheckedExpressionKind::Invalid {
            tokens: tokens.iter().map(|token| token.to_string()).collect(),
        });
    }

    let inputs = |role: &crate::KernelOwnerEdgeRole| {
        expression
            .inputs
            .iter()
            .filter(|input| &input.role == role)
            .map(|input| layout.expression(owner, input.value))
            .collect::<Result<Vec<_>, _>>()
    };
    let one_input = |role: &crate::KernelOwnerEdgeRole, label: &str| {
        let values = inputs(role)?;
        let [value] = values.as_slice() else {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} expression {} has {} {label} inputs",
                owner.0,
                expression.id.0,
                values.len(),
            )));
        };
        Ok(*value)
    };

    Ok(match &expression.kind {
        crate::KernelOwnerNodeKind::Source(_) => CheckedExpressionKind::Source,
        crate::KernelOwnerNodeKind::Absent => CheckedExpressionKind::Absent,
        crate::KernelOwnerNodeKind::Text => CheckedExpressionKind::Text {
            value: match payload {
                crate::KernelExpressionSemanticPayload::Text(value) => value.to_string(),
                _ => {
                    return Err(expression_payload_error(owner, expression, "text literal"));
                }
            },
        },
        crate::KernelOwnerNodeKind::TextTemplate => {
            let dynamic = inputs(&crate::KernelOwnerEdgeRole::TextDynamic)?;
            let crate::KernelExpressionSemanticPayload::TextTemplate(segments) = payload else {
                return Err(expression_payload_error(owner, expression, "text template"));
            };
            CheckedExpressionKind::TextTemplate {
                segments: segments
                    .iter()
                    .map(|segment| match segment {
                        crate::KernelTextTemplateSegment::Static(value) => {
                            Ok(CheckedTextSegment::Static {
                                value: value.to_string(),
                            })
                        }
                        crate::KernelTextTemplateSegment::Dynamic(ordinal) => dynamic
                            .get(*ordinal as usize)
                            .copied()
                            .map(|value| CheckedTextSegment::Dynamic { value })
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition {} text template {} references missing dynamic segment {}",
                                    owner.0, expression.id.0, ordinal,
                                ))
                            }),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            }
        }
        crate::KernelOwnerNodeKind::Number => CheckedExpressionKind::Number {
            value: match payload {
                crate::KernelExpressionSemanticPayload::Number(value) => value.clone(),
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression,
                        "number literal",
                    ));
                }
            },
        },
        crate::KernelOwnerNodeKind::Byte => CheckedExpressionKind::BytesByte {
            value: match payload {
                crate::KernelExpressionSemanticPayload::Byte(value) => *value,
                _ => return Err(expression_payload_error(owner, expression, "byte literal")),
            },
        },
        crate::KernelOwnerNodeKind::Bits(_) => CheckedExpressionKind::Bits {
            value: match payload {
                crate::KernelExpressionSemanticPayload::Bits(value) => value.clone(),
                _ => return Err(expression_payload_error(owner, expression, "bits literal")),
            },
        },
        crate::KernelOwnerNodeKind::Tag(name) => CheckedExpressionKind::Tag {
            name: name.to_string(),
        },
        crate::KernelOwnerNodeKind::Record { tag } => {
            let Some(crate::KernelExecutionShapeArtifact::Record { fields, .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} record expression {} has no exact execution shape",
                    owner.0, expression.id.0,
                )));
            };
            let fields = checked_record_fields(
                layout,
                owner,
                container_line,
                container_declaration,
                fields,
            )?;
            match tag {
                Some(tag) => CheckedExpressionKind::TaggedObject {
                    tag: tag.to_string(),
                    fields,
                },
                None => CheckedExpressionKind::Object { fields },
            }
        }
        crate::KernelOwnerNodeKind::Block => {
            let Some(crate::KernelExecutionShapeArtifact::Block {
                bindings, result, ..
            }) = shape
            else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} BLOCK expression {} has no exact execution shape",
                    owner.0, expression.id.0,
                )));
            };
            CheckedExpressionKind::Block {
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        Ok(CheckedBlockBinding {
                            declaration: layout.declaration(owner, binding.declaration)?,
                            value: layout.expression(owner, binding.value)?,
                            span: checked_nested_span(container_line, binding.span),
                        })
                    })
                    .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?,
                result: result
                    .map(|result| layout.expression(owner, result))
                    .transpose()?,
            }
        }
        crate::KernelOwnerNodeKind::Collection { kind, capacity } => match kind {
            crate::KernelCollectionKind::List => CheckedExpressionKind::List {
                capacity: *capacity,
                items: inputs(&crate::KernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Bytes => CheckedExpressionKind::Bytes {
                fixed_size: *capacity,
                items: inputs(&crate::KernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Set => CheckedExpressionKind::Set {
                items: inputs(&crate::KernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Map => CheckedExpressionKind::Map {
                entries: inputs(&crate::KernelOwnerEdgeRole::MapEntry)?,
            },
        },
        crate::KernelOwnerNodeKind::MapEntry => CheckedExpressionKind::MapEntry {
            key: one_input(&crate::KernelOwnerEdgeRole::MapKey, "map-key")?,
            value: one_input(&crate::KernelOwnerEdgeRole::MapValue, "map-value")?,
        },
        crate::KernelOwnerNodeKind::Latest => CheckedExpressionKind::Latest {
            branches: inputs(&crate::KernelOwnerEdgeRole::LatestBranch)?,
        },
        crate::KernelOwnerNodeKind::When => {
            let Some(crate::KernelExecutionShapeArtifact::Conditional { kind, .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} conditional expression {} has no exact execution shape",
                    owner.0, expression.id.0,
                )));
            };
            let input = one_input(
                &crate::KernelOwnerEdgeRole::WhenInput,
                "conditional selector",
            )?;
            let arms = inputs(&crate::KernelOwnerEdgeRole::WhenArm)?;
            match kind {
                crate::KernelConditionalKind::When => CheckedExpressionKind::When { input, arms },
                crate::KernelConditionalKind::While => CheckedExpressionKind::While { input, arms },
            }
        }
        crate::KernelOwnerNodeKind::Then => CheckedExpressionKind::Then {
            input: one_input(&crate::KernelOwnerEdgeRole::ThenInput, "THEN input")?,
            output: inputs(&crate::KernelOwnerEdgeRole::ThenOutput)?
                .into_iter()
                .next(),
        },
        crate::KernelOwnerNodeKind::Infix { operation } => CheckedExpressionKind::Infix {
            left: one_input(&crate::KernelOwnerEdgeRole::InfixLeft, "infix-left")?,
            op: operation.to_string(),
            right: one_input(&crate::KernelOwnerEdgeRole::InfixRight, "infix-right")?,
        },
        crate::KernelOwnerNodeKind::Draining => CheckedExpressionKind::Draining {
            input: one_input(&crate::KernelOwnerEdgeRole::DrainingInput, "DRAINING")?,
        },
        crate::KernelOwnerNodeKind::Hold => CheckedExpressionKind::Hold {
            initial: one_input(&crate::KernelOwnerEdgeRole::HoldInitial, "HOLD initial")?,
            name: match payload {
                crate::KernelExpressionSemanticPayload::HoldName(name) => name.to_string(),
                _ => return Err(expression_payload_error(owner, expression, "HOLD name")),
            },
        },
        crate::KernelOwnerNodeKind::MatchArm { .. } => {
            let Some(crate::KernelExecutionShapeArtifact::MatchArm { bindings, .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} match arm {} has no exact execution shape",
                    owner.0, expression.id.0,
                )));
            };
            CheckedExpressionKind::MatchArm {
                pattern: checked_match_pattern(payload)
                    .ok_or_else(|| expression_payload_error(owner, expression, "match pattern"))?,
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        layout.declaration(owner, KernelDeclarationReference::Local(*binding))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                output: inputs(&crate::KernelOwnerEdgeRole::MatchOutput)?
                    .into_iter()
                    .next(),
            }
        }
        crate::KernelOwnerNodeKind::Arrow => CheckedExpressionKind::Invalid {
            tokens: vec!["unconsumed_arrow".to_owned()],
        },
        crate::KernelOwnerNodeKind::Flush => CheckedExpressionKind::Flush {
            payload: one_input(&crate::KernelOwnerEdgeRole::FlushPayload, "FLUSH payload")?,
        },
        crate::KernelOwnerNodeKind::Delimiter => CheckedExpressionKind::Delimiter,
        crate::KernelOwnerNodeKind::Unknown => CheckedExpressionKind::Invalid {
            tokens: match payload {
                crate::KernelExpressionSemanticPayload::Invalid(tokens) => {
                    tokens.iter().map(|token| token.to_string()).collect()
                }
                crate::KernelExpressionSemanticPayload::LexicalPath(path) => vec![
                    "unresolved_value".to_owned(),
                    path.iter()
                        .map(|part| part.as_ref())
                        .collect::<Vec<_>>()
                        .join("/"),
                ],
                _ => vec!["unknown_expression".to_owned()],
            },
        },
        crate::KernelOwnerNodeKind::Known(_)
        | crate::KernelOwnerNodeKind::FormalRead { .. }
        | crate::KernelOwnerNodeKind::ContextRead { .. }
        | crate::KernelOwnerNodeKind::LexicalRead { .. }
        | crate::KernelOwnerNodeKind::ValueRead { .. }
        | crate::KernelOwnerNodeKind::DerivedRead { .. }
        | crate::KernelOwnerNodeKind::PatternRead { .. }
        | crate::KernelOwnerNodeKind::CollectionItemRead
        | crate::KernelOwnerNodeKind::FreshOut => {
            let stable = definition
                .relocations
                .expressions
                .get(expression.id.0 as usize);
            let span = definition
                .presentation
                .expressions
                .get(expression.id.0 as usize)
                .map(|presentation| presentation.span);
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} read expression {} ({stable:?}, span {span:?}) has no lexical authority",
                owner.0, expression.id.0,
            )));
        }
        crate::KernelOwnerNodeKind::UserCall { .. }
        | crate::KernelOwnerNodeKind::FieldProjection { .. }
        | crate::KernelOwnerNodeKind::RenderConstructor { .. }
        | crate::KernelOwnerNodeKind::PureBuiltin { .. }
        | crate::KernelOwnerNodeKind::FixedAbiCall { .. }
        | crate::KernelOwnerNodeKind::HostEffect { .. } => {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} call expression {} has no call artifact",
                owner.0, expression.id.0,
            )));
        }
    })
}

fn checked_record_fields(
    layout: &KernelCheckedLinkLayout,
    owner: KernelOwnerId,
    container_line: usize,
    container_declaration: Option<DeclId>,
    fields: &[crate::KernelExecutionRecordFieldArtifact],
) -> Result<Vec<CheckedRecordField>, KernelCheckedLinkError> {
    fields
        .iter()
        .map(|field| {
            Ok(CheckedRecordField {
                declaration: field
                    .declaration
                    .map(|declaration| layout.declaration(owner, declaration))
                    .transpose()?
                    .or(container_declaration),
                name: field.name.to_string(),
                value: layout.expression(owner, field.value)?,
                spread: field.spread,
                span: checked_nested_span(container_line, field.span),
            })
        })
        .collect()
}

fn checked_nested_span(line: usize, span: crate::KernelSourceSpan) -> CheckedSpan {
    CheckedSpan {
        line,
        start: span.start,
        end: span.end,
    }
}

fn checked_match_pattern(
    payload: &crate::KernelExpressionSemanticPayload,
) -> Option<CheckedMatchPattern> {
    let crate::KernelExpressionSemanticPayload::MatchPattern(pattern) = payload else {
        return None;
    };
    Some(match pattern {
        crate::KernelMatchPatternPayload::Wildcard => CheckedMatchPattern::Wildcard,
        crate::KernelMatchPatternPayload::Number(value) => CheckedMatchPattern::Number {
            value: value.clone(),
        },
        crate::KernelMatchPatternPayload::Text(value) => CheckedMatchPattern::Text {
            value: value.to_string(),
        },
        crate::KernelMatchPatternPayload::Tag { name, fields } => CheckedMatchPattern::Tag {
            name: name.to_string(),
            fields: fields.iter().map(|field| field.to_string()).collect(),
        },
        crate::KernelMatchPatternPayload::Binding(name) => CheckedMatchPattern::Binding {
            name: name.to_string(),
        },
        crate::KernelMatchPatternPayload::Bits(value) => CheckedMatchPattern::Bits {
            value: value.clone(),
        },
        crate::KernelMatchPatternPayload::Invalid => return None,
    })
}

fn checked_projection_to_expression(
    expressions: &[CheckedExpression],
    calls: &BTreeMap<CheckedCallId, &CheckedCall>,
    root: CheckedExprId,
    target: CheckedExprId,
) -> Option<Vec<String>> {
    fn visit(
        expressions: &[CheckedExpression],
        calls: &BTreeMap<CheckedCallId, &CheckedCall>,
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
        let expression = expressions
            .get(current.0 as usize)
            .filter(|expression| expression.id == current)?;
        let direct =
            |child, visiting: &mut BTreeSet<_>| visit(expressions, calls, child, target, visiting);
        let result = match &expression.kind {
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields } => fields.iter().find_map(|field| {
                let mut projection = direct(field.value, visiting)?;
                projection.insert(0, field.name.clone());
                Some(projection)
            }),
            CheckedExpressionKind::Call { call } => calls
                .get(call)
                .into_iter()
                .flat_map(|call| &call.entries)
                .find_map(|entry| match entry {
                    CheckedCallEntry::Input { value, .. } => direct(*value, visiting),
                    CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => None,
                }),
            CheckedExpressionKind::Draining { input }
            | CheckedExpressionKind::Hold { initial: input, .. } => direct(*input, visiting),
            CheckedExpressionKind::Flush { payload } => direct(*payload, visiting),
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
            | CheckedExpressionKind::Bytes { items, .. }
            | CheckedExpressionKind::Set { items }
            | CheckedExpressionKind::Latest { branches: items } => {
                items.iter().find_map(|item| direct(*item, visiting))
            }
            CheckedExpressionKind::Map { entries } => {
                entries.iter().find_map(|entry| direct(*entry, visiting))
            }
            CheckedExpressionKind::MapEntry { key, value } => {
                direct(*key, visiting).or_else(|| direct(*value, visiting))
            }
            CheckedExpressionKind::TextTemplate { segments } => {
                segments.iter().find_map(|segment| match segment {
                    CheckedTextSegment::Static { .. } => None,
                    CheckedTextSegment::Dynamic { value } => direct(*value, visiting),
                })
            }
            CheckedExpressionKind::Read { .. }
            | CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::Bits { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Absent
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => None,
        };
        visiting.remove(&current);
        result
    }

    visit(expressions, calls, root, target, &mut BTreeSet::new())
}

fn checked_resource_projection_requirements(
    declarations: &[CheckedDeclaration],
    callables: &[CheckedCallableSignature],
    calls: &[CheckedCall],
    expressions: &[CheckedExpression],
    sources: &[CheckedSource],
) -> Box<[CheckedResourceProjectionRequirement]> {
    let mut resolver =
        CheckedSourceProvenanceResolver::new(declarations, callables, calls, expressions, sources);
    expressions
        .iter()
        .filter_map(|expression| {
            let (target, projection) = match &expression.kind {
                CheckedExpressionKind::Read {
                    target,
                    projection,
                    source: None,
                }
                | CheckedExpressionKind::Drain { target, projection } => (*target, projection),
                _ => return None,
            };
            if projection.is_empty() {
                return None;
            }
            let required_type = if checked_type_is_specific(&expression.flow_type.ty) {
                expression.flow_type.ty.clone()
            } else {
                projection.last().map_or(Type::Unknown, |field| {
                    checked_source_payload_field_type(field)
                })
            };
            Some(CheckedResourceProjectionRequirement {
                expression: expression.id,
                target,
                projection: projection.clone(),
                source_origins: resolver.sources_for_declaration(target, projection),
                required_type,
            })
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn checked_type_is_specific(ty: &Type) -> bool {
    match ty {
        Type::Absent | Type::UnresolvedShape { .. } | Type::Unknown | Type::Var(_) => false,
        Type::Object(shape) if shape.open && shape.fields.is_empty() => false,
        Type::List(item) if matches!(item.as_ref(), Type::Object(shape) if shape.open && shape.fields.is_empty()) => {
            false
        }
        _ => true,
    }
}

fn checked_source_payload_field_type(field: &str) -> Type {
    match field {
        "press" | "click" | "double_click" | "blur" | "change" | "key_down" => {
            Type::object(ObjectShape::new(BTreeMap::new(), false))
        }
        "bytes" => Type::Bytes(boon_checked::BytesType::Dynamic),
        _ => Type::Text,
    }
}

struct CheckedSourcePathIndex<'a> {
    by_anchor: BTreeMap<DeclId, Vec<&'a CheckedSource>>,
}

impl<'a> CheckedSourcePathIndex<'a> {
    fn new(sources: &'a [CheckedSource]) -> Self {
        let mut by_anchor = BTreeMap::<DeclId, Vec<&CheckedSource>>::new();
        for source in sources {
            by_anchor
                .entry(source.path.anchor)
                .or_default()
                .push(source);
        }
        for candidates in by_anchor.values_mut() {
            candidates.sort_by_key(|source| std::cmp::Reverse(source.path.projection.len()));
        }
        Self { by_anchor }
    }

    fn exact_read(&self, target: DeclId, projection: &[String]) -> Option<CheckedSourceRead> {
        let mut matches = self.by_anchor.get(&target)?.iter().filter_map(|source| {
            projection
                .strip_prefix(source.path.projection.as_slice())
                .map(|payload| (*source, payload))
        });
        let (source, payload) = matches.next()?;
        if matches.next().is_some_and(|(candidate, _)| {
            candidate.path.projection.len() == source.path.projection.len()
        }) {
            return None;
        }
        Some(CheckedSourceRead {
            source: source.id,
            payload_projection: canonical_checked_source_payload_projection(payload),
        })
    }
}

fn canonical_checked_source_payload_projection(projection: &[String]) -> Vec<String> {
    if projection.is_empty() {
        return Vec::new();
    }
    let suffix = projection.join(".");
    let suffix = suffix
        .strip_prefix("event.")
        .or_else(|| suffix.strip_prefix("events."))
        .unwrap_or(&suffix);
    match suffix {
        "change.text" => vec!["text".to_owned()],
        "change.bytes" => vec!["bytes".to_owned()],
        "key_down.key" => vec!["key".to_owned()],
        "press" | "click" | "double_click" | "blur" | "change" | "key_down" => {
            vec![suffix.to_owned()]
        }
        field if !field.contains('.') => vec![field.to_owned()],
        _ => projection
            .strip_prefix(&["event".to_owned()])
            .or_else(|| projection.strip_prefix(&["events".to_owned()]))
            .unwrap_or(projection)
            .to_vec(),
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CheckedSourceResolution {
    Declaration(DeclId, Vec<String>),
    Expression(CheckedExprId, Vec<String>),
    ListItem(CheckedExprId, Vec<String>),
    Output(DeclId, Vec<String>),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CheckedSourceResolutionNode {
    Declaration(DeclId),
    Expression(CheckedExprId),
    ListItem(CheckedExprId),
    Output(DeclId),
}

impl CheckedSourceResolution {
    const fn node(&self) -> CheckedSourceResolutionNode {
        match self {
            Self::Declaration(declaration, _) => {
                CheckedSourceResolutionNode::Declaration(*declaration)
            }
            Self::Expression(expression, _) => CheckedSourceResolutionNode::Expression(*expression),
            Self::ListItem(expression, _) => CheckedSourceResolutionNode::ListItem(*expression),
            Self::Output(declaration, _) => CheckedSourceResolutionNode::Output(*declaration),
        }
    }
}

struct CheckedSourceProvenanceResolver<'a> {
    declarations: BTreeMap<DeclId, &'a CheckedDeclaration>,
    callables: BTreeMap<DeclId, &'a CheckedCallableSignature>,
    calls: BTreeMap<CheckedCallId, &'a CheckedCall>,
    expressions: BTreeMap<CheckedExprId, &'a CheckedExpression>,
    source_paths: CheckedSourcePathIndex<'a>,
    source_expressions: BTreeMap<CheckedExprId, Vec<CheckedSourceId>>,
    actual_inputs_by_formal: BTreeMap<DeclId, Vec<CheckedExprId>>,
    contextual_lists_by_output: BTreeMap<DeclId, Vec<CheckedExprId>>,
    forwarded_outputs_by_formal: BTreeMap<DeclId, Vec<DeclId>>,
    declaration_cache: BTreeMap<(DeclId, Vec<String>), Vec<CheckedSourceRead>>,
}

impl<'a> CheckedSourceProvenanceResolver<'a> {
    fn new(
        declarations: &'a [CheckedDeclaration],
        callables: &'a [CheckedCallableSignature],
        calls: &'a [CheckedCall],
        expressions: &'a [CheckedExpression],
        sources: &'a [CheckedSource],
    ) -> Self {
        let callable_index = callables
            .iter()
            .map(|callable| (callable.decl_id, callable))
            .collect::<BTreeMap<_, _>>();
        let mut source_expressions = BTreeMap::<CheckedExprId, Vec<CheckedSourceId>>::new();
        for source in sources {
            source_expressions
                .entry(source.expression)
                .or_default()
                .push(source.id);
        }
        let mut actual_inputs_by_formal = BTreeMap::<DeclId, Vec<CheckedExprId>>::new();
        let mut contextual_lists_by_output = BTreeMap::<DeclId, Vec<CheckedExprId>>::new();
        let mut forwarded_outputs_by_formal = BTreeMap::<DeclId, Vec<DeclId>>::new();
        for call in calls {
            let mut indexed_input_formals = BTreeSet::new();
            for entry in &call.entries {
                match entry {
                    CheckedCallEntry::Input { formal, value, .. } => {
                        if indexed_input_formals.insert(*formal) {
                            actual_inputs_by_formal
                                .entry(*formal)
                                .or_default()
                                .push(*value);
                        }
                    }
                    CheckedCallEntry::FreshOut { formal, output, .. }
                    | CheckedCallEntry::ForwardOut {
                        formal,
                        target: output,
                        ..
                    } => {
                        forwarded_outputs_by_formal
                            .entry(*formal)
                            .or_default()
                            .push(*output);
                    }
                }
            }
            let Some(operation) = callable_index
                .get(&call.callable)
                .and_then(|callable| callable.contextual_operation)
            else {
                continue;
            };
            let (list_formal, row_formal, _) = checked_contextual_operation_formals(operation);
            let Some(list) = checked_call_formal_input(call, list_formal) else {
                continue;
            };
            for entry in &call.entries {
                let output = match entry {
                    CheckedCallEntry::FreshOut { formal, output, .. } if *formal == row_formal => {
                        Some(*output)
                    }
                    CheckedCallEntry::ForwardOut {
                        formal,
                        target: output,
                        ..
                    } if *formal == row_formal => Some(*output),
                    _ => None,
                };
                if let Some(output) = output {
                    contextual_lists_by_output
                        .entry(output)
                        .or_default()
                        .push(list);
                }
            }
        }
        Self {
            declarations: declarations
                .iter()
                .map(|declaration| (declaration.id, declaration))
                .collect(),
            callables: callable_index,
            calls: calls.iter().map(|call| (call.id, call)).collect(),
            expressions: expressions
                .iter()
                .map(|expression| (expression.id, expression))
                .collect(),
            source_paths: CheckedSourcePathIndex::new(sources),
            source_expressions,
            actual_inputs_by_formal,
            contextual_lists_by_output,
            forwarded_outputs_by_formal,
            declaration_cache: BTreeMap::new(),
        }
    }

    fn sources_for_declaration(
        &mut self,
        target: DeclId,
        projection: &[String],
    ) -> Vec<CheckedSourceRead> {
        let key = (target, projection.to_vec());
        if let Some(cached) = self.declaration_cache.get(&key) {
            return cached.clone();
        }
        let mut explored = BTreeSet::new();
        let mut active = BTreeSet::new();
        let resolved = self
            .declaration_sources(target, projection, &mut explored, &mut active)
            .into_iter()
            .collect::<Vec<_>>();
        self.declaration_cache.insert(key, resolved.clone());
        resolved
    }

    fn declaration_sources(
        &self,
        target: DeclId,
        projection: &[String],
        explored: &mut BTreeSet<CheckedSourceResolution>,
        active: &mut BTreeSet<CheckedSourceResolutionNode>,
    ) -> BTreeSet<CheckedSourceRead> {
        let key = CheckedSourceResolution::Declaration(target, projection.to_vec());
        let node = key.node();
        if active.contains(&node) || !explored.insert(key) {
            return BTreeSet::new();
        }
        active.insert(node);
        let mut resolved = BTreeSet::new();
        if let Some(source) = self.source_paths.exact_read(target, projection) {
            resolved.insert(source);
        }
        let Some(declaration) = self.declarations.get(&target).copied() else {
            active.remove(&node);
            return resolved;
        };
        if declaration.kind == CheckedDeclarationKind::ValueParameter {
            for actual in self
                .actual_inputs_by_formal
                .get(&target)
                .into_iter()
                .flatten()
            {
                resolved.extend(self.expression_sources(*actual, projection, explored, active));
            }
        }
        if matches!(
            declaration.kind,
            CheckedDeclarationKind::FreshOut | CheckedDeclarationKind::OutParameter
        ) {
            resolved.extend(self.output_sources(target, projection, explored, active));
        }
        if let Some(value) = declaration.value {
            resolved.extend(self.expression_sources(value, projection, explored, active));
        }
        if let Some(result) = self
            .callables
            .get(&target)
            .and_then(|callable| callable.result_expression)
        {
            resolved.extend(self.expression_sources(result, projection, explored, active));
        }
        active.remove(&node);
        resolved
    }

    fn expression_sources(
        &self,
        expression_id: CheckedExprId,
        projection: &[String],
        explored: &mut BTreeSet<CheckedSourceResolution>,
        active: &mut BTreeSet<CheckedSourceResolutionNode>,
    ) -> BTreeSet<CheckedSourceRead> {
        let key = CheckedSourceResolution::Expression(expression_id, projection.to_vec());
        let node = key.node();
        if active.contains(&node) || !explored.insert(key) {
            return BTreeSet::new();
        }
        active.insert(node);
        let mut resolved = BTreeSet::new();
        if let Some(sources) = self.source_expressions.get(&expression_id) {
            resolved.extend(sources.iter().map(|source| CheckedSourceRead {
                source: *source,
                payload_projection: canonical_checked_source_payload_projection(projection),
            }));
            active.remove(&node);
            return resolved;
        }
        let Some(expression) = self.expressions.get(&expression_id).copied() else {
            active.remove(&node);
            return resolved;
        };
        match &expression.kind {
            CheckedExpressionKind::Read {
                target,
                projection: read_projection,
                ..
            }
            | CheckedExpressionKind::Drain {
                target,
                projection: read_projection,
            } => {
                let mut combined = read_projection.clone();
                combined.extend_from_slice(projection);
                resolved.extend(self.declaration_sources(*target, &combined, explored, active));
            }
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields } => {
                if let Some((field, rest)) = projection.split_first() {
                    for candidate in fields.iter().filter(|candidate| candidate.name == *field) {
                        resolved.extend(self.expression_sources(
                            candidate.value,
                            rest,
                            explored,
                            active,
                        ));
                    }
                }
            }
            CheckedExpressionKind::Call { call } => {
                if let Some(call) = self.calls.get(call).copied()
                    && let Some(callable) = self.callables.get(&call.callable).copied()
                {
                    if callable.kind == CheckedCallableKind::User {
                        if let Some(result) = callable.result_expression {
                            resolved.extend(
                                self.expression_sources(result, projection, explored, active),
                            );
                        }
                    } else if (matches!(
                        callable.contextual_operation,
                        Some(CheckedContextualOperation::Find { .. })
                    ) || matches!(
                        call.function.as_str(),
                        "List/get" | "List/latest" | "List/find"
                    )) && let Some(list) = checked_call_input(call, "list")
                    {
                        resolved.extend(self.list_item_sources(list, projection, explored, active));
                    }
                }
            }
            CheckedExpressionKind::Draining { input }
            | CheckedExpressionKind::Hold { initial: input, .. } => {
                resolved.extend(self.expression_sources(*input, projection, explored, active));
            }
            CheckedExpressionKind::Flush { payload } => {
                resolved.extend(self.expression_sources(*payload, projection, explored, active));
            }
            CheckedExpressionKind::Latest { branches } => {
                for branch in branches {
                    resolved.extend(self.expression_sources(*branch, projection, explored, active));
                }
            }
            CheckedExpressionKind::When { arms, .. }
            | CheckedExpressionKind::While { arms, .. } => {
                for arm in arms {
                    resolved.extend(self.expression_sources(*arm, projection, explored, active));
                }
            }
            CheckedExpressionKind::Then { output, .. }
            | CheckedExpressionKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    resolved.extend(self.expression_sources(*output, projection, explored, active));
                }
            }
            CheckedExpressionKind::Block { result, .. } => {
                if let Some(result) = result {
                    resolved.extend(self.expression_sources(*result, projection, explored, active));
                }
            }
            CheckedExpressionKind::MapEntry { key, value } => {
                resolved.extend(self.expression_sources(*key, &[], explored, active));
                resolved.extend(self.expression_sources(*value, projection, explored, active));
            }
            CheckedExpressionKind::Map { entries } => {
                for entry in entries {
                    resolved.extend(self.expression_sources(*entry, projection, explored, active));
                }
            }
            CheckedExpressionKind::Set { items } => {
                for item in items {
                    resolved.extend(self.expression_sources(*item, projection, explored, active));
                }
            }
            CheckedExpressionKind::List { .. }
            | CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::TextTemplate { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::Bits { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Absent
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Infix { .. }
            | CheckedExpressionKind::Bytes { .. }
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => {}
        }
        active.remove(&node);
        resolved
    }

    fn list_item_sources(
        &self,
        expression_id: CheckedExprId,
        projection: &[String],
        explored: &mut BTreeSet<CheckedSourceResolution>,
        active: &mut BTreeSet<CheckedSourceResolutionNode>,
    ) -> BTreeSet<CheckedSourceRead> {
        let key = CheckedSourceResolution::ListItem(expression_id, projection.to_vec());
        let node = key.node();
        if active.contains(&node) || !explored.insert(key) {
            return BTreeSet::new();
        }
        active.insert(node);
        let mut resolved = BTreeSet::new();
        let Some(expression) = self.expressions.get(&expression_id).copied() else {
            active.remove(&node);
            return resolved;
        };
        match &expression.kind {
            CheckedExpressionKind::List { items, .. } => {
                for item in items {
                    resolved.extend(self.expression_sources(*item, projection, explored, active));
                }
            }
            CheckedExpressionKind::Read {
                target,
                projection: list_projection,
                ..
            } => {
                if list_projection.is_empty()
                    && let Some(declaration) = self.declarations.get(target).copied()
                {
                    if declaration.kind == CheckedDeclarationKind::ValueParameter {
                        for actual in self
                            .actual_inputs_by_formal
                            .get(target)
                            .into_iter()
                            .flatten()
                        {
                            resolved.extend(
                                self.list_item_sources(*actual, projection, explored, active),
                            );
                        }
                    }
                    if let Some(value) = declaration.value {
                        resolved
                            .extend(self.list_item_sources(value, projection, explored, active));
                    }
                }
            }
            CheckedExpressionKind::Call { call } => {
                if let Some(call) = self.calls.get(call).copied()
                    && let Some(callable) = self.callables.get(&call.callable).copied()
                {
                    if callable.kind == CheckedCallableKind::User {
                        if let Some(result) = callable.result_expression {
                            resolved.extend(
                                self.list_item_sources(result, projection, explored, active),
                            );
                        }
                    } else {
                        match callable.contextual_operation {
                            Some(CheckedContextualOperation::Map { body, .. }) => {
                                if let Some(body) = checked_call_formal_input(call, body) {
                                    resolved.extend(
                                        self.expression_sources(body, projection, explored, active),
                                    );
                                }
                            }
                            Some(
                                CheckedContextualOperation::Filter { list, .. }
                                | CheckedContextualOperation::Retain { list, .. }
                                | CheckedContextualOperation::Remove { list, .. }
                                | CheckedContextualOperation::SortBy { list, .. }
                                | CheckedContextualOperation::ThenBy { list, .. },
                            ) => {
                                if let Some(list) = checked_call_formal_input(call, list) {
                                    resolved.extend(
                                        self.list_item_sources(list, projection, explored, active),
                                    );
                                }
                            }
                            Some(
                                CheckedContextualOperation::Every { .. }
                                | CheckedContextualOperation::Any { .. }
                                | CheckedContextualOperation::Find { .. },
                            )
                            | None => {
                                if matches!(call.function.as_str(), "List/take" | "List/page")
                                    && let Some(list) = checked_call_input(call, "list")
                                {
                                    resolved.extend(
                                        self.list_item_sources(list, projection, explored, active),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            CheckedExpressionKind::Draining { input }
            | CheckedExpressionKind::Hold { initial: input, .. } => {
                resolved.extend(self.list_item_sources(*input, projection, explored, active));
            }
            CheckedExpressionKind::Latest { branches } => {
                for branch in branches {
                    resolved.extend(self.list_item_sources(*branch, projection, explored, active));
                }
            }
            CheckedExpressionKind::When { arms, .. }
            | CheckedExpressionKind::While { arms, .. } => {
                for arm in arms {
                    resolved.extend(self.list_item_sources(*arm, projection, explored, active));
                }
            }
            CheckedExpressionKind::Then { output, .. }
            | CheckedExpressionKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    resolved.extend(self.list_item_sources(*output, projection, explored, active));
                }
            }
            CheckedExpressionKind::Block { result, .. } => {
                if let Some(result) = result {
                    resolved.extend(self.list_item_sources(*result, projection, explored, active));
                }
            }
            CheckedExpressionKind::TaggedObject { .. }
            | CheckedExpressionKind::Object { .. }
            | CheckedExpressionKind::MapEntry { .. }
            | CheckedExpressionKind::Map { .. }
            | CheckedExpressionKind::Set { .. }
            | CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::TextTemplate { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::Bits { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Absent
            | CheckedExpressionKind::Flush { .. }
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Infix { .. }
            | CheckedExpressionKind::Bytes { .. }
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => {}
        }
        active.remove(&node);
        resolved
    }

    fn output_sources(
        &self,
        target: DeclId,
        projection: &[String],
        explored: &mut BTreeSet<CheckedSourceResolution>,
        active: &mut BTreeSet<CheckedSourceResolutionNode>,
    ) -> BTreeSet<CheckedSourceRead> {
        let key = CheckedSourceResolution::Output(target, projection.to_vec());
        let node = key.node();
        if active.contains(&node) || !explored.insert(key) {
            return BTreeSet::new();
        }
        active.insert(node);
        let mut resolved = BTreeSet::new();
        for list in self
            .contextual_lists_by_output
            .get(&target)
            .into_iter()
            .flatten()
        {
            resolved.extend(self.list_item_sources(*list, projection, explored, active));
        }
        for output in self
            .forwarded_outputs_by_formal
            .get(&target)
            .into_iter()
            .flatten()
        {
            resolved.extend(self.output_sources(*output, projection, explored, active));
        }
        active.remove(&node);
        resolved
    }
}

fn checked_call_formal_input(call: &CheckedCall, formal: DeclId) -> Option<CheckedExprId> {
    call.entries.iter().find_map(|entry| match entry {
        CheckedCallEntry::Input {
            formal: candidate,
            value,
            ..
        } if *candidate == formal => Some(*value),
        CheckedCallEntry::Input { .. }
        | CheckedCallEntry::FreshOut { .. }
        | CheckedCallEntry::ForwardOut { .. } => None,
    })
}

fn checked_call_input(call: &CheckedCall, name: &str) -> Option<CheckedExprId> {
    call.entries.iter().find_map(|entry| match entry {
        CheckedCallEntry::Input {
            name: candidate,
            value,
            ..
        } if candidate == name => Some(*value),
        CheckedCallEntry::Input { .. }
        | CheckedCallEntry::FreshOut { .. }
        | CheckedCallEntry::ForwardOut { .. } => None,
    })
}

const fn checked_contextual_operation_formals(
    operation: CheckedContextualOperation,
) -> (DeclId, DeclId, DeclId) {
    match operation {
        CheckedContextualOperation::Map { list, row, body }
        | CheckedContextualOperation::Filter {
            list,
            row,
            predicate: body,
        }
        | CheckedContextualOperation::Retain {
            list,
            row,
            predicate: body,
        }
        | CheckedContextualOperation::Remove {
            list,
            row,
            predicate: body,
        }
        | CheckedContextualOperation::Every {
            list,
            row,
            predicate: body,
        }
        | CheckedContextualOperation::Any {
            list,
            row,
            predicate: body,
        }
        | CheckedContextualOperation::Find {
            list,
            row,
            predicate: body,
        }
        | CheckedContextualOperation::SortBy {
            list,
            row,
            key: body,
            ..
        }
        | CheckedContextualOperation::ThenBy {
            list,
            row,
            key: body,
            ..
        } => (list, row, body),
    }
}

fn lexical_payload_path(payload: &crate::KernelExpressionSemanticPayload) -> Option<String> {
    let crate::KernelExpressionSemanticPayload::LexicalPath(path) = payload else {
        return None;
    };
    Some(
        path.iter()
            .map(|part| part.as_ref())
            .collect::<Vec<_>>()
            .join("."),
    )
}

fn canonical_checked_source_read(
    source_paths: &BTreeMap<DeclId, Vec<(Vec<String>, CheckedSourceId)>>,
    target: DeclId,
    projection: Vec<String>,
) -> (DeclId, Vec<String>, Option<CheckedSourceRead>) {
    let Some(candidates) = source_paths.get(&target) else {
        return (target, projection, None);
    };
    let mut matches = candidates.iter().filter_map(|(path, source)| {
        projection
            .strip_prefix(path.as_slice())
            .map(|payload| (*source, path.len(), payload))
    });
    let Some((source, path_len, payload)) = matches.next() else {
        return (target, projection, None);
    };
    if matches
        .next()
        .is_some_and(|(_, candidate_len, _)| candidate_len == path_len)
    {
        return (target, projection, None);
    }
    let payload_projection = payload
        .strip_prefix(&["event".to_owned()])
        .or_else(|| payload.strip_prefix(&["events".to_owned()]))
        .unwrap_or(payload)
        .to_vec();
    (
        target,
        projection,
        Some(CheckedSourceRead {
            source,
            payload_projection,
        }),
    )
}

fn expression_payload_error(
    owner: KernelOwnerId,
    expression: &crate::KernelExpressionArtifact,
    expected: &str,
) -> KernelCheckedLinkError {
    KernelCheckedLinkError::new(format!(
        "kernel definition {} expression {} has no exact {expected} payload",
        owner.0, expression.id.0,
    ))
}

fn push_statement_resource(
    resources: &mut [Vec<CheckedResourceBinding>],
    statement: CheckedStatementId,
    resource: CheckedResourceBinding,
) -> Result<(), KernelCheckedLinkError> {
    let bindings = resources.get_mut(statement.0 as usize).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel resource references missing checked statement {}",
            statement.0,
        ))
    })?;
    if bindings.contains(&resource) {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel checked statement {} repeats resource binding {resource:?}",
            statement.0,
        )));
    }
    bindings.push(resource);
    Ok(())
}

fn declaration_flow_type(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    declaration: &crate::KernelDeclarationArtifact,
) -> Result<FlowType, KernelCheckedLinkError> {
    let definition = snapshot.definitions.get(owner.0 as usize).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel checked declaration flow references missing definition {}",
            owner.0,
        ))
    })?;
    if let Some(flow_type) = &declaration.declared_flow_type {
        return layout.relocate_flow_type(owner, flow_type);
    }
    if declaration.kind == crate::KernelDeclarationKind::Function {
        if definition.linkage.public_declaration
            != Some(KernelDeclarationReference::Local(declaration.id))
        {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} function declaration {} is not its public authority",
                owner.0, declaration.id.0,
            )));
        }
        let mut arguments = definition
            .declarations
            .iter()
            .filter_map(|candidate| match candidate.origin {
                crate::KernelDeclarationOrigin::Parameter { ordinal, .. }
                    if candidate.kind == crate::KernelDeclarationKind::ValueParameter =>
                {
                    Some((ordinal, candidate))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        arguments.sort_unstable_by_key(|(ordinal, _)| *ordinal);
        let arguments = arguments
            .into_iter()
            .map(|(ordinal, _)| {
                definition
                    .formals
                    .get(ordinal as usize)
                    .map(|formal| formal.ty.clone())
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} function value parameter {ordinal} has no solved formal",
                            owner.0,
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        return layout.relocate_flow_type(
            owner,
            &FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Function {
                    args: arguments,
                    result: Box::new(definition.result.clone()),
                },
            },
        );
    }
    if definition.linkage.public_declaration
        == Some(KernelDeclarationReference::Local(declaration.id))
    {
        return layout.relocate_flow_type(owner, &definition.result);
    }
    match declaration.origin {
        crate::KernelDeclarationOrigin::Parameter { ordinal, .. } => definition
            .formals
            .get(ordinal as usize)
            .map(|formal| layout.relocate_flow_type(owner, formal))
            .transpose()?
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} parameter declaration {} has no solved formal {ordinal}",
                    owner.0, declaration.id.0,
                ))
            }),
        crate::KernelDeclarationOrigin::PatternBinding { arm, ordinal } => {
            pattern_binding_flow_type(
                layout,
                snapshot,
                owner,
                arm,
                ordinal,
                declaration.name.as_ref(),
            )
        }
        crate::KernelDeclarationOrigin::CallbackBinding { call, ordinal } => fresh_out_flow_type(
            layout,
            snapshot,
            owner,
            declaration.name.as_ref(),
            call,
            ordinal,
        ),
        crate::KernelDeclarationOrigin::CallContext { .. } => {
            Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} call-context declaration {} has no declared flow type",
                owner.0, declaration.id.0,
            )))
        }
        crate::KernelDeclarationOrigin::Statement { .. }
        | crate::KernelDeclarationOrigin::RecordField { .. } => {
            let value = declaration.value.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} declaration {} has no value authority",
                    owner.0, declaration.id.0,
                ))
            })?;
            relocated_value_flow_type(layout, snapshot, owner, value)
        }
    }
}

fn fresh_out_flow_type(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    declaration_name: &str,
    call: crate::KernelExpressionId,
    ordinal: u32,
) -> Result<FlowType, KernelCheckedLinkError> {
    let definition = snapshot
        .definitions
        .get(owner.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("FreshOut definition is missing"))?;
    let mut calls = definition
        .call_syntax
        .iter()
        .filter(|candidate| candidate.expression == call);
    let call_syntax = calls.next().ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition {} FreshOut formal {ordinal} has no authored call surface",
            owner.0
        ))
    })?;
    if calls.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} FreshOut formal {ordinal} has multiple authored call surfaces",
            owner.0
        )));
    }
    let mut providers = call_syntax.arguments.iter().filter_map(|argument| {
        (argument.kind == crate::KernelCallArgumentKind::BareBinding
            && argument.name.as_ref() == declaration_name)
            .then_some(argument.value)
    });
    let provider = providers.next().ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition {} FreshOut formal {ordinal} `{declaration_name}` has no bare-OUT provider",
            owner.0
        ))
    })?;
    if providers.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} FreshOut formal {ordinal} `{declaration_name}` has multiple bare-OUT providers",
            owner.0
        )));
    }
    let KernelValueReference::Local(provider) = provider else {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} FreshOut formal {ordinal} `{declaration_name}` has an external bare-OUT provider",
            owner.0,
        )));
    };
    let expression = definition
        .expressions
        .get(provider.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("FreshOut provider expression is missing"))?;
    if !matches!(expression.kind, crate::KernelOwnerNodeKind::FreshOut) {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} FreshOut formal {ordinal} `{declaration_name}` provider {} has kind {:?}",
            owner.0, provider.0, expression.kind,
        )));
    }
    layout.relocate_flow_type(owner, &expression.flow_type)
}

fn pattern_binding_flow_type(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    arm: crate::KernelExpressionId,
    ordinal: u32,
    declaration_name: &str,
) -> Result<FlowType, KernelCheckedLinkError> {
    let definition = snapshot
        .definitions
        .get(owner.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("pattern-binding definition is missing"))?;
    let arm_expression = definition
        .expressions
        .get(arm.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("pattern-binding match arm is missing"))?;
    let crate::KernelOwnerNodeKind::MatchArm { pattern } = &arm_expression.kind else {
        return Err(KernelCheckedLinkError::new(
            "pattern-binding declaration does not name a match arm",
        ));
    };
    let mut shapes = definition
        .execution_shapes
        .iter()
        .filter_map(|shape| match shape {
            crate::KernelExecutionShapeArtifact::MatchArm {
                expression,
                selector,
                ..
            } if *expression == arm => Some(*selector),
            _ => None,
        });
    let selector = shapes.next().ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition {} match arm {} has no selector authority",
            owner.0, arm.0,
        ))
    })?;
    if shapes.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} match arm {} has multiple selector authorities",
            owner.0, arm.0,
        )));
    }
    let (selector_owner, selector) = value_flow_authority(snapshot, owner, selector)?;
    let ty = match pattern {
        crate::KernelPattern::Binding { name } if name.as_ref() == declaration_name => {
            selector.ty.clone()
        }
        crate::KernelPattern::Tag { name, fields }
            if fields.get(ordinal as usize).map(Box::as_ref) == Some(declaration_name) =>
        {
            let Type::VariantSet(variants) = &selector.ty else {
                return Ok(FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Unknown,
                });
            };
            variants
                .iter()
                .find_map(|variant| match variant {
                    Variant::Tagged {
                        tag,
                        fields: payload,
                    } if tag == name.as_ref() => payload.fields.get(declaration_name).cloned(),
                    Variant::Tag(_) | Variant::Tagged { .. } => None,
                })
                .unwrap_or(Type::Unknown)
        }
        crate::KernelPattern::Wildcard
        | crate::KernelPattern::Number
        | crate::KernelPattern::Text
        | crate::KernelPattern::Bits { .. }
        | crate::KernelPattern::Tag { .. }
        | crate::KernelPattern::Binding { .. }
        | crate::KernelPattern::Invalid => Type::Unknown,
    };
    layout.relocate_flow_type(
        selector_owner,
        &FlowType {
            mode: FlowMode::Continuous,
            ty,
        },
    )
}

fn relocated_value_flow_type(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    value: KernelValueReference,
) -> Result<FlowType, KernelCheckedLinkError> {
    let (authority, flow_type) = value_flow_authority(snapshot, owner, value)?;
    layout.relocate_flow_type(authority, &flow_type)
}

fn value_flow_authority(
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    value: KernelValueReference,
) -> Result<(KernelOwnerId, FlowType), KernelCheckedLinkError> {
    match value {
        KernelValueReference::Local(expression) => snapshot
            .definitions
            .get(owner.0 as usize)
            .and_then(|definition| definition.expressions.get(expression.0 as usize))
            .map(|expression| (owner, expression.flow_type.clone()))
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} value references missing expression {}",
                    owner.0, expression.0,
                ))
            }),
        KernelValueReference::External(external) => {
            let definition = snapshot
                .definitions
                .get(external.owner.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel value references missing external definition {}",
                        external.owner.0,
                    ))
                })?;
            match external.target {
                KernelExternalTarget::Expression(expression) => definition
                    .expressions
                    .get(expression.0 as usize)
                    .map(|expression| (external.owner, expression.flow_type.clone()))
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel external definition {} has no expression {}",
                            external.owner.0, expression.0,
                        ))
                    }),
                KernelExternalTarget::Result => Ok((external.owner, definition.result.clone())),
            }
        }
    }
}

fn callable_type_parameter_variables(formals: &[FlowType], result: &FlowType) -> Vec<TypeVar> {
    let mut parameters = BTreeMap::new();
    for formal in formals {
        collect_callable_type_parameter_variables(&formal.ty, &mut parameters);
    }
    collect_callable_type_parameter_variables(&result.ty, &mut parameters);
    let mut variables = vec![TypeVar(u32::MAX); parameters.len()];
    for (variable, parameter) in parameters {
        variables[parameter.0 as usize] = variable;
    }
    variables
}

fn collect_callable_type_parameter_variables(
    ty: &Type,
    parameters: &mut BTreeMap<TypeVar, KernelTypeParameterId>,
) {
    match ty {
        Type::Var(variable) => {
            let next = KernelTypeParameterId(
                u32::try_from(parameters.len())
                    .expect("kernel checked callable type-parameter count exceeds u32"),
            );
            parameters.entry(*variable).or_insert(next);
        }
        Type::Object(shape) => {
            for field in shape.ordered_fields().into_iter().map(|(_, field)| field) {
                collect_callable_type_parameter_variables(field, parameters);
            }
        }
        Type::List(item) | Type::Set(item) => {
            collect_callable_type_parameter_variables(item, parameters);
        }
        Type::Map { key, value } => {
            collect_callable_type_parameter_variables(key, parameters);
            collect_callable_type_parameter_variables(value, parameters);
        }
        Type::Function { args, result } => {
            for argument in args {
                collect_callable_type_parameter_variables(argument, parameters);
            }
            collect_callable_type_parameter_variables(&result.ty, parameters);
        }
        Type::VariantSet(variants) => {
            for variant in variants {
                if let Variant::Tagged { fields, .. } = variant {
                    for field in fields.ordered_fields().into_iter().map(|(_, field)| field) {
                        collect_callable_type_parameter_variables(field, parameters);
                    }
                }
            }
        }
        Type::Union(members) => {
            for member in members {
                collect_callable_type_parameter_variables(member, parameters);
            }
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => {}
    }
}

fn type_variables_in_flow(flow: &FlowType) -> BTreeSet<TypeVar> {
    let mut variables = BTreeSet::new();
    collect_flow_type_variables(flow, &mut variables);
    variables
}

fn referenced_abi_callable_names(
    snapshot: &KernelCheckedSnapshot,
) -> Result<BTreeSet<String>, KernelCheckedLinkError> {
    let mut names = BTreeSet::new();
    for (owner, definition) in snapshot.definitions.iter().enumerate() {
        let mut syntax_by_expression = BTreeMap::new();
        for syntax in &definition.call_syntax {
            if syntax_by_expression
                .insert(syntax.expression, syntax.function.as_ref())
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner} repeats call syntax for expression {}",
                    syntax.expression.0,
                )));
            }
        }
        for call in &definition.calls {
            if matches!(call.target, KernelCallTarget::User { .. }) {
                continue;
            }
            let function = syntax_by_expression
                .get(&call.expression)
                .copied()
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {owner} ABI call expression {} has no authored call identity",
                        call.expression.0,
                    ))
                })?;
            if let KernelCallTarget::HostEffect { operation } = &call.target
                && operation.as_ref() != function
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner} host call expression {} names `{function}` but targets `{operation}`",
                    call.expression.0,
                )));
            }
            if let KernelCallTarget::FieldProjection { field } = &call.target
                && function != format!("Field/{field}")
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner} field projection call {} names `{function}` instead of `Field/{field}`",
                    call.expression.0,
                )));
            }
            names.insert(function.to_owned());
        }
    }
    Ok(names)
}

fn abi_callable_type_variables(callable: &crate::KernelCallableAbiInput) -> BTreeSet<TypeVar> {
    let mut variables = BTreeSet::new();
    for parameter in &callable.parameters {
        collect_flow_type_variables(&parameter.flow_type, &mut variables);
    }
    for context in &callable.contexts {
        collect_flow_type_variables(&context.flow_type, &mut variables);
    }
    collect_flow_type_variables(&callable.result, &mut variables);
    variables
}

fn relocate_abi_flow_type(
    layout: &KernelCheckedAbiCallableLayout,
    flow_type: &FlowType,
) -> Result<FlowType, KernelCheckedLinkError> {
    Ok(FlowType {
        mode: flow_type.mode,
        ty: relocate_type(layout.type_variables, &flow_type.ty)?,
    })
}

fn checked_abi_contextual_operation(
    layout: &KernelCheckedAbiCallableLayout,
    operation: KernelAbiContextualOperation,
) -> Result<CheckedContextualOperation, KernelCheckedLinkError> {
    let parameter = |ordinal: u32, role: &str| {
        layout
            .parameters
            .get(ordinal as usize)
            .copied()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel ABI callable `{}` contextual {role} references missing parameter ordinal {ordinal}",
                    layout.name,
                ))
            })
    };
    Ok(match operation {
        KernelAbiContextualOperation::Map { list, row, body } => CheckedContextualOperation::Map {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            body: parameter(body, "body")?,
        },
        KernelAbiContextualOperation::Filter {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Filter {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Retain {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Retain {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Remove {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Remove {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Every {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Every {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Any {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Any {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Find {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Find {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::SortBy {
            list,
            row,
            key,
            direction,
        } => CheckedContextualOperation::SortBy {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            key: parameter(key, "key")?,
            direction: parameter(direction, "direction")?,
        },
        KernelAbiContextualOperation::ThenBy {
            list,
            row,
            key,
            direction,
        } => CheckedContextualOperation::ThenBy {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            key: parameter(key, "key")?,
            direction: parameter(direction, "direction")?,
        },
    })
}

fn definition_type_variables(definition: &crate::DefinitionArtifact) -> BTreeSet<TypeVar> {
    let mut variables = BTreeSet::new();
    for formal in &definition.formals {
        collect_flow_type_variables(formal, &mut variables);
    }
    collect_flow_type_variables(&definition.result, &mut variables);
    for expression in &definition.expressions {
        collect_flow_type_variables(&expression.flow_type, &mut variables);
        match &expression.kind {
            crate::KernelOwnerNodeKind::Known(ty) | crate::KernelOwnerNodeKind::Source(ty) => {
                collect_type_variables(ty, &mut variables);
            }
            _ => {}
        }
    }
    for call in &definition.calls {
        collect_flow_type_variables(&call.result, &mut variables);
        for substitution in &call.type_substitutions {
            collect_type_variables(&substitution.value, &mut variables);
        }
    }
    for source in &definition.sources {
        collect_type_variables(&source.payload_type, &mut variables);
    }
    for state in &definition.states {
        collect_flow_type_variables(&state.flow_type, &mut variables);
    }
    for list in &definition.lists {
        collect_type_variables(&list.item_type, &mut variables);
    }
    for diagnostic in &definition.diagnostics {
        if let crate::KernelDiagnosticKind::CallInputType {
            actual, expected, ..
        } = &diagnostic.kind
        {
            collect_type_variables(actual, &mut variables);
            collect_type_variables(expected, &mut variables);
        }
    }
    variables
}

fn collect_flow_type_variables(flow_type: &FlowType, variables: &mut BTreeSet<TypeVar>) {
    collect_type_variables(&flow_type.ty, variables);
}

fn collect_type_variables(ty: &Type, variables: &mut BTreeSet<TypeVar>) {
    match ty {
        Type::Var(variable) => {
            variables.insert(*variable);
        }
        Type::VariantSet(variants) => {
            for variant in variants.iter() {
                if let Variant::Tagged { fields, .. } = variant {
                    for field in fields.fields.values() {
                        collect_type_variables(field, variables);
                    }
                }
            }
        }
        Type::Object(shape) => {
            for field in shape.fields.values() {
                collect_type_variables(field, variables);
            }
        }
        Type::List(item) | Type::Set(item) => collect_type_variables(item, variables),
        Type::Function { args, result } => {
            for argument in args {
                collect_type_variables(argument, variables);
            }
            collect_flow_type_variables(result, variables);
        }
        Type::Union(members) => {
            for member in members {
                collect_type_variables(member, variables);
            }
        }
        Type::Map { key, value } => {
            collect_type_variables(key, variables);
            collect_type_variables(value, variables);
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown
        | Type::Bits { .. } => {}
    }
}

fn relocate_type(
    type_variables: KernelCheckedRowRange,
    ty: &Type,
) -> Result<Type, KernelCheckedLinkError> {
    Ok(match ty {
        Type::Var(variable) => Type::Var(TypeVar(
            type_variables.resolve(variable.0, "type variable")?,
        )),
        Type::VariantSet(variants) => Type::VariantSet(
            variants
                .iter()
                .map(|variant| {
                    Ok(match variant {
                        Variant::Tag(tag) => Variant::Tag(tag.clone()),
                        Variant::Tagged { tag, fields } => Variant::Tagged {
                            tag: tag.clone(),
                            fields: relocate_object_shape(type_variables, fields)?.into(),
                        },
                    })
                })
                .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?
                .into(),
        ),
        Type::Object(shape) => Type::object(relocate_object_shape(type_variables, shape)?),
        Type::List(item) => Type::List(Type::shared(relocate_type(type_variables, item)?)),
        Type::Set(item) => Type::Set(Type::shared(relocate_type(type_variables, item)?)),
        Type::Function { args, result } => Type::Function {
            args: args
                .iter()
                .map(|argument| relocate_type(type_variables, argument))
                .collect::<Result<Vec<_>, _>>()?,
            result: Box::new(FlowType {
                mode: result.mode,
                ty: relocate_type(type_variables, &result.ty)?,
            }),
        },
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|member| relocate_type(type_variables, member))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Type::Map { key, value } => Type::Map {
            key: Box::new(relocate_type(type_variables, key)?),
            value: Box::new(relocate_type(type_variables, value)?),
        },
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown
        | Type::Bits { .. } => ty.clone(),
    })
}

fn relocate_object_shape(
    type_variables: KernelCheckedRowRange,
    shape: &ObjectShape,
) -> Result<ObjectShape, KernelCheckedLinkError> {
    Ok(ObjectShape {
        fields: shape
            .fields
            .iter()
            .map(|(name, ty)| Ok((name.clone(), relocate_type(type_variables, ty)?)))
            .collect::<Result<_, KernelCheckedLinkError>>()?,
        field_order: shape.field_order.clone(),
        open: shape.open,
    })
}

fn resolve_public_declaration(
    definition: usize,
    definitions: &[KernelCheckedDefinitionLayout],
    authorities: &[KernelDeclarationReference],
    resolved: &mut [Option<DeclId>],
    resolving: &mut [bool],
) -> Result<DeclId, KernelCheckedLinkError> {
    if let Some(declaration) = resolved.get(definition).copied().flatten() {
        return Ok(declaration);
    }
    let layout = definitions.get(definition).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel checked linker references missing public definition {definition}"
        ))
    })?;
    let authority = authorities.get(definition).copied().ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel checked linker omits public authority for definition {definition}"
        ))
    })?;
    let is_resolving = resolving.get_mut(definition).ok_or_else(|| {
        KernelCheckedLinkError::new("kernel checked linker public-resolution table is malformed")
    })?;
    if *is_resolving {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel checked linker public declaration authorities contain a cycle at definition {definition}"
        )));
    }
    *is_resolving = true;
    let result = match authority {
        KernelDeclarationReference::Local(declaration) => Ok(DeclId(
            layout
                .declarations
                .resolve(declaration.0, "public declaration")?,
        )),
        KernelDeclarationReference::OwnerPublic(owner) => resolve_public_declaration(
            owner.0 as usize,
            definitions,
            authorities,
            resolved,
            resolving,
        ),
        KernelDeclarationReference::OwnerDeclaration { owner, declaration } => Ok(DeclId(
            definitions
                .get(owner.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked linker references missing public declaration owner {}",
                        owner.0
                    ))
                })?
                .declarations
                .resolve(declaration.0, "delegated public declaration")?,
        )),
    };
    resolving[definition] = false;
    let declaration = result?;
    resolved[definition] = Some(declaration);
    Ok(declaration)
}

fn take_range(
    next: &mut u32,
    len: usize,
    label: &str,
) -> Result<KernelCheckedRowRange, KernelCheckedLinkError> {
    let len = u32::try_from(len).map_err(|_| {
        KernelCheckedLinkError::new(format!("kernel checked linker {label} count exceeds u32"))
    })?;
    let range = KernelCheckedRowRange { start: *next, len };
    *next = next.checked_add(len).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel checked linker {label} namespace exceeds u32"
        ))
    })?;
    Ok(range)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedLinkError {
    message: Box<str>,
}

impl KernelCheckedLinkError {
    fn new(message: impl Into<Box<str>>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for KernelCheckedLinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for KernelCheckedLinkError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CheckDemand, KernelCheckProduct, KernelDeclarationId, KernelDeclarationInput,
        KernelDeclarationKind, KernelDeclarationOrigin, KernelDefinitionFactsInput,
        KernelDefinitionLinkage, KernelDefinitionRelocations, KernelExpressionId,
        KernelExpressionInputArtifact, KernelExpressionRelocation, KernelExternalExpression,
        KernelLexicalAccess, KernelLexicalBindingInput, KernelLexicalBindingTargetInput,
        KernelOwnerEdgeRole, KernelOwnerInputEdge, KernelOwnerNode, KernelOwnerNodeKind,
        KernelOwnerProgramInput, KernelProjectProgramInput, KernelSession, KernelStatementId,
        KernelStatementInput, KernelStatementKind, KernelStatementValueUse,
    };
    use boon_checked::FlowMode;
    use boon_syntax::{
        SourceUnitId, StableCheckOwnerKey, StableExpressionKey, StableItemRoute,
        StableItemRouteSegment, StableOwnerKey, StableStatementKey, StableStatementRoute,
        UnitItemKind,
    };

    fn owner_key(unit: &SourceUnitId, name: &str) -> StableCheckOwnerKey {
        StableCheckOwnerKey::Item(StableOwnerKey {
            source_unit_id: unit.clone(),
            item_route: StableItemRoute::__parser_from_segments(vec![StableItemRouteSegment {
                kind: UnitItemKind::Field,
                names: vec![name.to_owned()],
                matching_sibling_ordinal: 0,
            }]),
        })
    }

    fn facts(
        unit: &SourceUnitId,
        owner: &StableCheckOwnerKey,
        name: &str,
    ) -> KernelDefinitionFactsInput {
        let StableCheckOwnerKey::Item(owner) = owner else {
            unreachable!()
        };
        KernelDefinitionFactsInput {
            linkage: KernelDefinitionLinkage {
                root_statement: Some(KernelStatementId(0)),
                public_declaration: Some(KernelDeclarationReference::Local(KernelDeclarationId(0))),
                result_expression: Some(KernelExpressionId(0)),
                context_formal_ordinal: None,
            },
            relocations: KernelDefinitionRelocations {
                expressions: vec![KernelExpressionRelocation::Authored(StableExpressionKey {
                    source_unit_id: unit.clone(),
                    route_digest_v1: [name.as_bytes()[0]; 32],
                })]
                .into_boxed_slice(),
                statements: vec![StableStatementKey {
                    source_unit_id: unit.clone(),
                    route: StableStatementRoute {
                        owner: Some(owner.item_route.clone()),
                        statement_route: Vec::new(),
                    },
                }]
                .into_boxed_slice(),
            },
            presentation: crate::KernelDefinitionPresentation {
                containing_scope: KernelScopeReference::ProjectRoot,
                scopes: Box::new([]),
                expressions: vec![crate::KernelExpressionPresentation {
                    expression: KernelExpressionId(0),
                    scope: KernelScopeReference::Containing,
                    declaration: Some(KernelDeclarationReference::Local(KernelDeclarationId(0))),
                    declaration_scope: None,
                    span: crate::KernelSourceSpan {
                        line: 1,
                        start: 0,
                        end: 1,
                    },
                }]
                .into_boxed_slice(),
                statements: vec![crate::KernelStatementPresentation {
                    statement: KernelStatementId(0),
                    scope: KernelScopeReference::Containing,
                    body_scope: None,
                    span: crate::KernelSourceSpan {
                        line: 1,
                        start: 0,
                        end: 1,
                    },
                }]
                .into_boxed_slice(),
                declarations: vec![crate::KernelDeclarationPresentation {
                    declaration: KernelDeclarationId(0),
                    scope: KernelScopeReference::Containing,
                    body_scope: None,
                    span: crate::KernelSourceSpan {
                        line: 1,
                        start: 0,
                        end: 1,
                    },
                }]
                .into_boxed_slice(),
            },
            expression_payloads: vec![crate::KernelExpressionSemanticPayload::None]
                .into_boxed_slice(),
            call_syntax: Box::new([]),
            execution_shapes: Box::new([]),
            statements: vec![KernelStatementInput {
                id: KernelStatementId(0),
                kind: KernelStatementKind::Field { name: name.into() },
                value: Some(KernelExpressionId(0)),
                value_use: KernelStatementValueUse::RuntimeValue,
                children: Box::new([]),
            }]
            .into_boxed_slice(),
            declarations: vec![KernelDeclarationInput {
                id: KernelDeclarationId(0),
                origin: KernelDeclarationOrigin::Statement {
                    statement: KernelStatementId(0),
                },
                name: name.into(),
                kind: KernelDeclarationKind::Field,
                value: Some(KernelExpressionId(0)),
                declared_flow_type: None,
            }]
            .into_boxed_slice(),
            lexical_bindings: Box::new([]),
            sources: Box::new([]),
            states: Box::new([]),
            lists: Box::new([]),
            diagnostics: Box::new([]),
            diagnostic_values: Box::new([]),
        }
    }

    #[test]
    fn one_prefix_layout_globalizes_cross_definition_authorities_once() {
        let unit = SourceUnitId::from_path("app/RUN.bn").unwrap();
        let provider_key = owner_key(&unit, "provider");
        let consumer_key = owner_key(&unit, "consumer");
        let provider = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Number,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let consumer = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::ValueRead {
                    fields: Box::new([]),
                    mode_narrowing: None,
                },
                inputs: vec![KernelOwnerInputEdge {
                    role: KernelOwnerEdgeRole::ReadProvider,
                    expression: KernelExpressionId(1),
                }]
                .into_boxed_slice(),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: vec![KernelExternalExpression {
                owner: KernelOwnerId(0),
                target: KernelExternalTarget::Result,
            }]
            .into_boxed_slice(),
            result: KernelExpressionId(0),
        };
        let mut provider_facts = facts(&unit, &provider_key, "provider");
        provider_facts.expression_payloads[0] =
            crate::KernelExpressionSemanticPayload::Number(boon_data::ExactNumber::from_u64(1));
        let mut consumer_facts = facts(&unit, &consumer_key, "consumer");
        consumer_facts.statements[0].children =
            vec![KernelStatementChildReference::Owner(KernelOwnerId(0))].into_boxed_slice();
        consumer_facts.lexical_bindings = vec![KernelLexicalBindingInput {
            expression: KernelExpressionId(0),
            target: KernelLexicalBindingTargetInput::Declaration(
                KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)),
            ),
            projection: Box::new([]),
            access: KernelLexicalAccess::Read,
        }]
        .into_boxed_slice();
        let project = KernelProjectInput::new(
            KernelProjectProgramInput {
                owners: vec![provider, consumer].into_boxed_slice(),
            },
            vec![provider_facts, consumer_facts].into_boxed_slice(),
            vec![provider_key, consumer_key].into_boxed_slice(),
        )
        .unwrap();
        let mut session = KernelSession::new(project.clone());
        let checked = session.check(CheckDemand::CheckedImage).unwrap();
        let KernelCheckProduct::CheckedImage(snapshot) = checked.product else {
            unreachable!()
        };
        let layout = KernelCheckedLinkLayout::new(&project, &snapshot).unwrap();
        assert_eq!(layout.totals().expressions, 2);
        assert_eq!(layout.totals().scopes, 1);
        assert_eq!(layout.totals().statements, 2);
        assert_eq!(layout.totals().declarations, 3);
        assert!(layout.totals().resolved_references >= 4);
        assert_eq!(layout.definitions()[0].result_expression, CheckedExprId(0));
        assert_eq!(layout.definitions()[1].result_expression, CheckedExprId(1));
        assert_eq!(layout.definitions()[0].public_declaration, DeclId(1));
        assert_eq!(layout.definitions()[1].public_declaration, DeclId(2));
        assert_eq!(
            layout.definitions()[0].root_statement,
            CheckedStatementId(0)
        );
        assert_eq!(
            layout.definitions()[1].root_statement,
            CheckedStatementId(1)
        );
        let rows = layout
            .materialize_rows(&project, &snapshot, ProgramRole::Client)
            .expect("one linker call must materialize the complete checked-row surface");
        assert_eq!(rows.scopes.len(), 1);
        assert_eq!(rows.declarations.len(), 2);
        assert_eq!(rows.expressions.len(), 2);
        assert_eq!(rows.statements.len(), 2);
        assert!(rows.callables.is_empty());
        assert!(rows.context_formals.is_empty());
        assert!(rows.calls.is_empty());
        assert!(rows.call_occurrences.is_empty());
        assert!(rows.call_result_paths.is_empty());
        assert!(rows.pattern_bindings.is_empty());
        assert!(rows.resource_projection_requirements.is_empty());
        assert_eq!(rows.occurrences.len(), 3);
        assert_eq!(
            rows.occurrences
                .iter()
                .filter(|occurrence| occurrence.kind == SemanticOccurrenceKind::Declaration)
                .count(),
            2,
        );
        assert_eq!(
            rows.occurrences
                .iter()
                .filter(|occurrence| occurrence.kind == SemanticOccurrenceKind::Read)
                .count(),
            1,
        );
        assert!(rows.sources.is_empty());
        assert!(rows.states.is_empty());
        assert!(rows.lists.is_empty());

        let imported = snapshot.definitions[1].expressions[0].inputs[0].value;
        assert_eq!(
            layout.expression(KernelOwnerId(1), imported).unwrap(),
            CheckedExprId(0)
        );
        assert_eq!(
            layout
                .declaration(
                    KernelOwnerId(1),
                    KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)),
                )
                .unwrap(),
            DeclId(1),
        );
        assert_eq!(
            layout
                .statement(
                    KernelOwnerId(1),
                    KernelStatementReference::OwnerPublic(KernelOwnerId(0)),
                )
                .unwrap(),
            CheckedStatementId(0),
        );

        let mut scoped = (*snapshot).clone();
        scoped.definitions[0].presentation.scopes = vec![crate::KernelScopePresentation {
            id: crate::KernelScopeId(0),
            parent: KernelScopeReference::Containing,
            owner: Some(KernelDeclarationReference::Local(KernelDeclarationId(0))),
            kind: crate::KernelScopeKind::Block,
            origin: crate::KernelScopeOrigin::StatementBody {
                statement: KernelStatementId(0),
            },
            span: crate::KernelSourceSpan {
                line: 1,
                start: 0,
                end: 1,
            },
        }]
        .into_boxed_slice();
        scoped.definitions[1].presentation.containing_scope = KernelScopeReference::Owner {
            owner: KernelOwnerId(0),
            scope: crate::KernelScopeId(0),
        };
        let scoped_layout = KernelCheckedLinkLayout::new(&project, &scoped)
            .expect("a nested owner must inherit its enclosing compact scope");
        assert_eq!(scoped_layout.totals().scopes, 2);
        assert_eq!(
            scoped_layout.definitions()[1].containing_scope,
            LexicalScopeId(1)
        );
        assert_eq!(
            scoped_layout
                .scope(KernelOwnerId(1), KernelScopeReference::Containing)
                .unwrap(),
            LexicalScopeId(1)
        );
        let materialized_scopes = scoped_layout
            .materialize_scopes(&scoped)
            .expect("compact scopes materialize directly into checked rows");
        assert_eq!(materialized_scopes.len(), 2);
        assert_eq!(materialized_scopes[0].kind, CheckedScopeKind::Root);
        assert_eq!(materialized_scopes[0].parent, None);
        assert_eq!(materialized_scopes[1].id, LexicalScopeId(1));
        assert_eq!(materialized_scopes[1].parent, Some(LexicalScopeId(0)));
        assert_eq!(materialized_scopes[1].owner, Some(DeclId(1)));
        assert_eq!(materialized_scopes[1].kind, CheckedScopeKind::Block);
        let materialized_declarations = scoped_layout
            .materialize_declarations(&scoped)
            .expect("solved declarations materialize directly into checked rows");
        assert_eq!(materialized_declarations.len(), 2);
        assert_eq!(materialized_declarations[0].id, DeclId(1));
        assert_eq!(materialized_declarations[0].name, "provider");
        assert_eq!(materialized_declarations[0].scope_id, LexicalScopeId(0));
        assert_eq!(materialized_declarations[0].body_scope, None);
        assert_eq!(materialized_declarations[0].flow_type.ty, Type::Number);
        assert_eq!(materialized_declarations[1].id, DeclId(2));
        assert_eq!(materialized_declarations[1].name, "consumer");
        assert_eq!(materialized_declarations[1].flow_type.ty, Type::Number);
        scoped.definitions[1].statements[0].value_use = KernelStatementValueUse::RenderSlot;
        let materialized_statements = scoped_layout
            .materialize_statements(&scoped)
            .expect("definition statements materialize without an owner-shard assembler");
        assert_eq!(materialized_statements.len(), 2);
        assert_eq!(materialized_statements[0].id, CheckedStatementId(0));
        assert_eq!(materialized_statements[0].scope_id, LexicalScopeId(0));
        assert_eq!(
            materialized_statements[0].kind,
            CheckedStatementKind::Field {
                declaration: DeclId(1)
            }
        );
        assert_eq!(materialized_statements[0].value, Some(CheckedExprId(0)));
        assert_eq!(
            materialized_statements[0].value_use,
            CheckedValueUse::RuntimeValue
        );
        assert!(materialized_statements[0].resources.is_empty());
        assert!(materialized_statements[0].children.is_empty());
        assert_eq!(materialized_statements[1].id, CheckedStatementId(1));
        assert_eq!(
            materialized_statements[1].kind,
            CheckedStatementKind::Field {
                declaration: DeclId(2)
            }
        );
        assert_eq!(materialized_statements[1].value, Some(CheckedExprId(1)));
        assert_eq!(
            materialized_statements[1].value_use,
            CheckedValueUse::RenderSlot
        );
        assert_eq!(materialized_statements[1].children, [CheckedStatementId(0)]);

        let mut generic = (*snapshot).clone();
        for definition in &mut generic.definitions {
            definition.result.ty = Type::Var(TypeVar(0));
            definition.expressions[0].kind = KernelOwnerNodeKind::Known(Type::Var(TypeVar(0)));
            definition.expressions[0].flow_type.ty = Type::Var(TypeVar(0));
        }
        let generic_layout = KernelCheckedLinkLayout::new(&project, &generic)
            .expect("definition-local alpha ordinals must globalize once");
        assert_eq!(generic_layout.totals().type_variables, 2);
        assert_eq!(
            generic_layout.definitions()[0].type_variables,
            KernelCheckedRowRange { start: 0, len: 1 }
        );
        assert_eq!(
            generic_layout.definitions()[1].type_variables,
            KernelCheckedRowRange { start: 1, len: 1 }
        );
        let generic_declarations = generic_layout
            .materialize_declarations(&generic)
            .expect("direct declarations must use disjoint global type variables");
        assert_eq!(generic_declarations[0].flow_type.ty, Type::Var(TypeVar(0)));
        assert_eq!(generic_declarations[1].flow_type.ty, Type::Var(TypeVar(1)));

        let mut sparse_variables = generic.clone();
        sparse_variables.definitions[0].result.ty = Type::Var(TypeVar(1));
        sparse_variables.definitions[0].expressions[0].kind =
            KernelOwnerNodeKind::Known(Type::Var(TypeVar(1)));
        sparse_variables.definitions[0].expressions[0].flow_type.ty = Type::Var(TypeVar(1));
        let error = KernelCheckedLinkLayout::new(&project, &sparse_variables)
            .expect_err("a non-dense definition-local alpha namespace must fail closed");
        assert!(error.to_string().contains("non-dense local type-variable"));

        let mut missing_scope = scoped.clone();
        missing_scope.definitions[1].presentation.containing_scope = KernelScopeReference::Owner {
            owner: KernelOwnerId(0),
            scope: crate::KernelScopeId(99),
        };
        let error = KernelCheckedLinkLayout::new(&project, &missing_scope)
            .expect_err("a missing enclosing scope must fail before row materialization");
        assert!(error.to_string().contains("containing scope"));

        let mut delegated = (*snapshot).clone();
        delegated.definitions[1].linkage.public_declaration =
            Some(KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)));
        let delegated_layout = KernelCheckedLinkLayout::new(&project, &delegated)
            .expect("a nested definition may share its enclosing public declaration");
        assert_eq!(
            delegated_layout.definitions()[1].public_declaration,
            DeclId(1)
        );

        delegated.definitions[0].linkage.public_declaration =
            Some(KernelDeclarationReference::OwnerPublic(KernelOwnerId(1)));
        let error = KernelCheckedLinkLayout::new(&project, &delegated)
            .expect_err("public declaration authority cycles must fail closed");
        assert!(error.to_string().contains("contain a cycle"));

        let mut invalid = (*snapshot).clone();
        invalid.definitions[1].expressions[0].inputs[0] = KernelExpressionInputArtifact {
            role: KernelOwnerEdgeRole::ReadProvider,
            value: KernelValueReference::External(KernelExternalExpression {
                owner: KernelOwnerId(99),
                target: KernelExternalTarget::Result,
            }),
        };
        let error = KernelCheckedLinkLayout::new(&project, &invalid)
            .expect_err("an unlinked external owner must fail before row allocation");
        assert!(error.to_string().contains("missing definition 99"));
    }
}
