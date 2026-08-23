//! Complete checked demand roots published before occurrence expansion.
//!
//! This product is intentionally private to semantic construction. It is the
//! one checked-to-semantic worklist seed: OUT consumes its program schedule
//! roots, contextual expansion consumes its retained definitions, and the
//! construction-owned image will consume the categorized obligations.

use crate::{
    call_view::CallCatalog,
    checked_view::{CheckedProgramView, StatementKindRef, StatementRef},
    out_net::ProducerRootSpec,
};
use boon_checked::{
    CheckedCallableKind, CheckedExprId, CheckedStatementId, DeclId, LexicalScopeId,
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
        program: CheckedProgramView<'_>,
        calls: &CallCatalog<'_>,
        producer_roots: &[ProducerRootSpec],
        retained_definitions: BTreeSet<DeclId>,
    ) -> Result<Self, String> {
        let mut scope_owners = Vec::with_capacity(program.scope_count());
        for scope in program.scopes() {
            let expected = u32::try_from(scope_owners.len())
                .map_err(|_| "verified intent scope count exceeds u32".to_owned())?;
            if scope.id().0 != expected {
                return Err(format!(
                    "verified intent expected dense scope {} but found {}",
                    expected,
                    scope.id().0,
                ));
            }
            scope_owners.push(function_owner_for_scope(program, scope.id())?);
        }

        let mut roots = Vec::new();
        let mut program_schedule_roots = Vec::new();
        let mut seen_program_roots = vec![false; program.expression_count()];
        for statement in program.statements() {
            let owner_callable = scope_owner(&scope_owners, statement.scope())?;
            let Some(expression) = statement.value() else {
                continue;
            };
            let checked_expression = program.expression(expression).ok_or_else(|| {
                format!(
                    "verified intent statement {} references missing expression {}",
                    statement.id().0,
                    expression.0
                )
            })?;
            let is_program_schedule_root = owner_callable.is_none()
                && statement.kind() != StatementKindRef::Function
                && (statement.scope() == program.root_scope()
                    || checked_expression.effect()
                        != boon_checked::CheckedEffectSummary::default()
                    || matches!(
                        statement.kind(),
                        StatementKindRef::Source | StatementKindRef::Hold | StatementKindRef::List
                    ));
            if is_program_schedule_root {
                push_program_schedule_root(
                    &mut program_schedule_roots,
                    &mut seen_program_roots,
                    expression,
                );
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::ProgramSchedule,
                    expression,
                    owner_callable: None,
                    declaration: statement.declaration(),
                    statement: Some(statement.id()),
                });
            }
            if checked_expression.effect() != boon_checked::CheckedEffectSummary::default() {
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::ConsequentialEffect,
                    expression,
                    owner_callable,
                    declaration: statement.declaration(),
                    statement: Some(statement.id()),
                });
            }
            if owner_callable.is_none()
                && statement.declaration().is_some_and(|declaration| {
                    calls.declaration(declaration).is_some_and(|declaration| {
                        matches!(declaration.name(), "document" | "scene")
                    })
                })
            {
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::RetainedVisualOutput,
                    expression,
                    owner_callable: None,
                    declaration: statement.declaration(),
                    statement: Some(statement.id()),
                });
            }
        }

        if let Some(outputs) = program.rich_output_root_types() {
            for output in outputs {
                let Some(expression) = output.value else {
                    continue;
                };
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::HostOutput,
                    expression,
                    owner_callable: None,
                    declaration: Some(output.declaration),
                    statement: Some(output.statement),
                });
            }
        } else {
            for output in packed_output_root_statements(program, calls)? {
                if let Some(expression) = output.value() {
                    roots.push(VerifiedIntentRootV1 {
                        kind: VerifiedIntentRootKindV1::HostOutput,
                        expression,
                        owner_callable: None,
                        declaration: output.declaration(),
                        statement: Some(output.id()),
                    });
                }
            }
        }
        for source in program.sources() {
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::SourceAuthority,
                expression: source.expression(),
                owner_callable: scope_owner(&scope_owners, source.owner_scope())?,
                declaration: Some(source.declaration()),
                statement: Some(source.statement()),
            });
        }
        for state in program.states() {
            let owner_callable = scope_owner(&scope_owners, state.owner_scope())?;
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::StateAuthority,
                expression: state.expression(),
                owner_callable,
                declaration: Some(state.declaration()),
                statement: Some(state.statement()),
            });
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::StateInitialValue,
                expression: state.initial(),
                owner_callable,
                declaration: Some(state.declaration()),
                statement: Some(state.statement()),
            });
        }
        for list in program.lists() {
            roots.push(VerifiedIntentRootV1 {
                kind: VerifiedIntentRootKindV1::ListAuthority,
                expression: list.producer(),
                owner_callable: scope_owner(&scope_owners, list.owner_scope())?,
                declaration: Some(list.declaration()),
                statement: Some(list.statement()),
            });
        }
        for call in calls.calls() {
            if calls
                .callable(call.callable())
                .is_some_and(|callable| callable.kind() == CheckedCallableKind::External)
            {
                roots.push(VerifiedIntentRootV1 {
                    kind: VerifiedIntentRootKindV1::ExternalCall,
                    expression: call.expression(),
                    owner_callable: call.owner_callable(),
                    declaration: Some(call.callable()),
                    statement: None,
                });
            }
        }
        for producer in producer_roots {
            let callable = calls.callable(producer.callable).ok_or_else(|| {
                format!(
                    "verified producer intent references missing callable {}",
                    producer.callable.0
                )
            })?;
            let expression = callable.result_expression().ok_or_else(|| {
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
                statement: callable.body(),
            });
        }

        roots.sort_unstable();
        roots.dedup();
        for root in &roots {
            if program.expression(root.expression).is_none() {
                return Err(format!(
                    "verified {:?} intent references missing expression {}",
                    root.kind, root.expression.0
                ));
            }
            if let Some(declaration) = root.declaration
                && calls.declaration(declaration).is_none()
            {
                return Err(format!(
                    "verified {:?} intent references missing declaration {}",
                    root.kind, declaration.0
                ));
            }
            if let Some(statement) = root.statement
                && program.statement(statement).is_none()
            {
                return Err(format!(
                    "verified {:?} intent references missing statement {}",
                    root.kind, statement.0
                ));
            }
        }
        if retained_definitions
            .iter()
            .any(|definition| calls.callable(*definition).is_none())
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

fn push_program_schedule_root(
    roots: &mut Vec<CheckedExprId>,
    seen: &mut [bool],
    expression: CheckedExprId,
) {
    let seen = seen
        .get_mut(expression.0 as usize)
        .expect("verified schedule root expression was resolved in the dense authority");
    if !*seen {
        *seen = true;
        roots.push(expression);
    }
}

fn packed_output_root_statements<'a>(
    program: CheckedProgramView<'a>,
    calls: &CallCatalog<'a>,
) -> Result<Vec<StatementRef<'a>>, String> {
    let container = program.statements().find(|statement| {
        statement.kind() == StatementKindRef::Field
            && statement.scope() == program.root_scope()
            && statement.declaration().is_some_and(|declaration| {
                calls
                    .declaration(declaration)
                    .is_some_and(|declaration| declaration.name() == "outputs")
            })
    });
    let Some(container) = container else {
        return Ok(Vec::new());
    };
    let mut roots = Vec::new();
    for child in container.children() {
        let statement = program.statement(child).ok_or_else(|| {
            format!(
                "verified output intent references missing statement {}",
                child.0
            )
        })?;
        if !matches!(
            statement.kind(),
            StatementKindRef::Field | StatementKindRef::List
        ) {
            continue;
        }
        let Some(declaration) = statement.declaration() else {
            continue;
        };
        let declaration = calls.declaration(declaration).ok_or_else(|| {
            format!(
                "verified output intent references missing declaration {}",
                declaration.0
            )
        })?;
        let duplicate = roots.iter().copied().any(|root: StatementRef<'a>| {
            root.declaration()
                .and_then(|declaration| calls.declaration(declaration))
                .is_some_and(|previous| previous.name() == declaration.name())
        });
        if duplicate {
            continue;
        }
        roots.push(statement);
    }
    Ok(roots)
}

fn scope_owner(
    scope_owners: &[Option<DeclId>],
    scope: LexicalScopeId,
) -> Result<Option<DeclId>, String> {
    scope_owners
        .get(scope.0 as usize)
        .copied()
        .ok_or_else(|| format!("verified intent references missing scope {}", scope.0))
}

fn function_owner_for_scope(
    program: CheckedProgramView<'_>,
    mut scope: LexicalScopeId,
) -> Result<Option<DeclId>, String> {
    for _ in 0..program.scope_count() {
        let checked = program
            .scope(scope)
            .ok_or_else(|| format!("verified intent references missing scope {}", scope.0))?;
        if checked.kind() == boon_checked::CheckedScopeKind::Function {
            return Ok(checked.owner());
        }
        let Some(parent) = checked.parent() else {
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
        let calls = CallCatalog::rich(&program).unwrap();
        let intent = VerifiedSemanticIntentV1::build(
            CheckedProgramView::rich(&program),
            &calls,
            &[],
            BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            intent.program_schedule_roots().len(),
            3,
            "schedule roots: {:#?}",
            intent.program_schedule_roots()
        );
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
