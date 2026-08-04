//! Complete checked demand roots published before occurrence expansion.
//!
//! This product is intentionally private to semantic construction. It is the
//! one checked-to-semantic worklist seed: OUT consumes its program schedule
//! roots, contextual expansion consumes its retained definitions, and the
//! construction-owned image will consume the categorized obligations.

use crate::out_net::ProducerRootSpec;
use boon_checked::{
    CheckedCallableKind, CheckedExprId, CheckedProgramFields, CheckedStatementId,
    CheckedStatementKind, DeclId, LexicalScopeId,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum VerifiedIntentRootKindV1 {
    ProgramSchedule,
    RetainedVisualOutput,
    HostOutput,
    SourceAuthority,
    StateAuthority,
    StateInitialValue,
    ListAuthority,
    ConsequentialEffect,
    ExternalCall,
    ProducerFunction,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct VerifiedIntentRootV1 {
    pub kind: VerifiedIntentRootKindV1,
    pub expression: CheckedExprId,
    pub owner_callable: Option<DeclId>,
    pub declaration: Option<DeclId>,
    pub statement: Option<CheckedStatementId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedSemanticIntentV1 {
    roots: Vec<VerifiedIntentRootV1>,
    program_schedule_roots: Vec<CheckedExprId>,
    retained_definitions: BTreeSet<DeclId>,
}

impl VerifiedSemanticIntentV1 {
    pub(crate) fn build(
        program: &CheckedProgramFields,
        producer_roots: &[ProducerRootSpec],
        retained_definitions: BTreeSet<DeclId>,
    ) -> Result<Self, String> {
        let scope_owners = program
            .scopes
            .iter()
            .map(|scope| Ok((scope.id, function_owner_for_scope(program, scope.id)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let expressions = program
            .expressions
            .iter()
            .map(|expression| (expression.id, expression))
            .collect::<BTreeMap<_, _>>();
        let declarations = program
            .declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let statements = program
            .statements
            .iter()
            .map(|statement| (statement.id, statement))
            .collect::<BTreeMap<_, _>>();
        let callables = program
            .callables
            .iter()
            .map(|callable| (callable.decl_id, callable))
            .collect::<BTreeMap<_, _>>();

        let mut roots = Vec::new();
        let mut program_schedule_roots = Vec::new();
        let mut seen_program_roots = BTreeSet::new();
        for statement in &program.statements {
            let owner_callable = scope_owners.get(&statement.scope_id).copied().flatten();
            if owner_callable.is_none()
                && !matches!(statement.kind, CheckedStatementKind::Function { .. })
                && let Some(expression) = statement.value
            {
                if seen_program_roots.insert(expression) {
                    program_schedule_roots.push(expression);
                }
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::ProgramSchedule,
                    expression,
                    owner_callable: None,
                    declaration: statement_declaration(&statement.kind),
                    statement: Some(statement.id),
                });
            }

            let Some(expression) = statement.value else {
                continue;
            };
            let checked_expression = expressions.get(&expression).ok_or_else(|| {
                format!(
                    "verified intent statement {} references missing expression {}",
                    statement.id.0, expression.0
                )
            })?;
            if checked_expression.effect != boon_checked::CheckedEffectSummary::default() {
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::ConsequentialEffect,
                    expression,
                    owner_callable,
                    declaration: statement_declaration(&statement.kind),
                    statement: Some(statement.id),
                });
            }
            if owner_callable.is_none()
                && statement_declaration(&statement.kind).is_some_and(|declaration| {
                    declarations.get(&declaration).is_some_and(|declaration| {
                        matches!(declaration.name.as_str(), "document" | "scene")
                    })
                })
            {
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::RetainedVisualOutput,
                    expression,
                    owner_callable: None,
                    declaration: statement_declaration(&statement.kind),
                    statement: Some(statement.id),
                });
            }
        }

        for output in &program.lowering_metadata.output_root_types {
            if let Some(expression) = output.value {
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::HostOutput,
                    expression,
                    owner_callable: None,
                    declaration: Some(output.declaration),
                    statement: Some(output.statement),
                });
            }
        }
        for source in &program.sources {
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::SourceAuthority,
                expression: source.expression,
                owner_callable: scope_owners.get(&source.owner_scope).copied().flatten(),
                declaration: Some(source.declaration),
                statement: Some(source.statement),
            });
        }
        for state in &program.states {
            let owner_callable = scope_owners.get(&state.owner_scope).copied().flatten();
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::StateAuthority,
                expression: state.expression,
                owner_callable,
                declaration: Some(state.declaration),
                statement: Some(state.statement),
            });
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::StateInitialValue,
                expression: state.initial,
                owner_callable,
                declaration: Some(state.declaration),
                statement: Some(state.statement),
            });
        }
        for list in &program.lists {
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::ListAuthority,
                expression: list.producer,
                owner_callable: scope_owners.get(&list.owner_scope).copied().flatten(),
                declaration: Some(list.declaration),
                statement: Some(list.statement),
            });
        }
        for call in &program.calls {
            if callables
                .get(&call.callable)
                .is_some_and(|callable| callable.kind == CheckedCallableKind::External)
            {
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::ExternalCall,
                    expression: call.expression,
                    owner_callable: call.owner_callable,
                    declaration: Some(call.callable),
                    statement: None,
                });
            }
        }
        for producer in producer_roots {
            let callable = callables.get(&producer.callable).ok_or_else(|| {
                format!(
                    "verified producer intent references missing callable {}",
                    producer.callable.0
                )
            })?;
            let expression = callable.result_expression.ok_or_else(|| {
                format!(
                    "verified producer intent callable {} has no result expression",
                    producer.callable.0
                )
            })?;
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::ProducerFunction,
                expression,
                owner_callable: Some(producer.callable),
                declaration: Some(producer.result_declaration),
                statement: callable.body,
            });
        }

        roots.sort_unstable();
        roots.dedup();
        for root in &roots {
            if !expressions.contains_key(&root.expression) {
                return Err(format!(
                    "verified {:?} intent references missing expression {}",
                    root.kind, root.expression.0
                ));
            }
            if let Some(declaration) = root.declaration
                && !declarations.contains_key(&declaration)
            {
                return Err(format!(
                    "verified {:?} intent references missing declaration {}",
                    root.kind, declaration.0
                ));
            }
            if let Some(statement) = root.statement
                && !statements.contains_key(&statement)
            {
                return Err(format!(
                    "verified {:?} intent references missing statement {}",
                    root.kind, statement.0
                ));
            }
        }
        if retained_definitions
            .iter()
            .any(|definition| !callables.contains_key(definition))
        {
            return Err("verified retained intent references a missing callable".to_owned());
        }

        Ok(Self {
            roots,
            program_schedule_roots,
            retained_definitions,
        })
    }

    pub(crate) fn roots(&self) -> &[VerifiedIntentRootV1] {
        &self.roots
    }

    pub(crate) fn program_schedule_roots(&self) -> &[CheckedExprId] {
        &self.program_schedule_roots
    }

    pub(crate) fn retained_definitions(&self) -> &BTreeSet<DeclId> {
        &self.retained_definitions
    }

    pub(crate) fn trace(&self) {
        if std::env::var_os("BOON_SEMANTIC_TRACE").is_none() {
            return;
        }
        let mut counts = BTreeMap::<VerifiedIntentRootKindV1, usize>::new();
        for root in self.roots() {
            *counts.entry(root.kind).or_default() += 1;
        }
        eprintln!(
            "boon_semantic verified_intent roots={} program_schedule_roots={} retained_definitions={} roots_by_kind={counts:?}",
            self.roots.len(),
            self.program_schedule_roots.len(),
            self.retained_definitions.len(),
        );
    }
}

fn statement_declaration(kind: &CheckedStatementKind) -> Option<DeclId> {
    match kind {
        CheckedStatementKind::Function { declaration }
        | CheckedStatementKind::Field { declaration } => Some(*declaration),
        CheckedStatementKind::Source { declaration, .. }
        | CheckedStatementKind::Hold { declaration, .. }
        | CheckedStatementKind::List { declaration, .. } => *declaration,
        CheckedStatementKind::Block
        | CheckedStatementKind::Spread
        | CheckedStatementKind::Expression => None,
    }
}

fn function_owner_for_scope(
    program: &CheckedProgramFields,
    mut scope: LexicalScopeId,
) -> Result<Option<DeclId>, String> {
    let mut visited = BTreeSet::new();
    while visited.insert(scope) {
        let checked = program
            .scopes
            .iter()
            .find(|candidate| candidate.id == scope)
            .ok_or_else(|| format!("verified intent references missing scope {}", scope.0))?;
        if checked.kind == boon_checked::CheckedScopeKind::Function {
            return Ok(checked.owner);
        }
        let Some(parent) = checked.parent else {
            return Ok(None);
        };
        scope = parent;
    }
    Err(format!(
        "verified intent scope ancestry is cyclic at {}",
        scope.0
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_separates_schedule_resource_effect_and_output_roots() {
        let parsed = boon_parser::parse_source(
            "verified-intent.bn",
            r#"
store: [press: SOURCE]
outputs: [
    answer: 42
]
document: Document/new(root: [])
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (program, _) = checked.program.unwrap().into_parts();
        let intent = VerifiedSemanticIntentV1::build(&program, &[], BTreeSet::new()).unwrap();
        assert_eq!(intent.program_schedule_roots().len(), 4);
        let kinds = intent
            .roots()
            .iter()
            .map(|root| root.kind)
            .collect::<BTreeSet<_>>();
        assert!(kinds.contains(&VerifiedIntentRootKindV1::ProgramSchedule));
        assert!(kinds.contains(&VerifiedIntentRootKindV1::SourceAuthority));
        assert!(kinds.contains(&VerifiedIntentRootKindV1::HostOutput));
        assert!(kinds.contains(&VerifiedIntentRootKindV1::RetainedVisualOutput));
    }
}
