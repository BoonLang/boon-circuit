//! Allocation-free borrowed access to checked base rows.
//!
//! Runtime lowering borrows the kernel's packed authority. Rich checked rows
//! remain an explicit editor/oracle source. Every wrapper keeps the owning
//! authority in its variant, so no bare text or type ID can escape and no
//! compatibility row is reconstructed merely to inspect topology.

#![allow(dead_code)]

use boon_checked::{
    CheckedEffectSummary, CheckedExprId, CheckedExpression, CheckedList, CheckedListId,
    CheckedProgramFields, CheckedScope, CheckedScopeKind, CheckedSource, CheckedSourceId,
    CheckedState, CheckedStateId, CheckedStatement, CheckedStatementId, CheckedStatementKind,
    DeclId, LexicalScopeId, OutputRootTypeEntry, ProgramRole,
};
use boon_compiler_kernel::{
    KernelSemanticExpressionRef, KernelSemanticInputV1, KernelSemanticListRef,
    KernelSemanticScopeRef, KernelSemanticSourceRef, KernelSemanticStateRef,
    KernelSemanticStatementChildIter, KernelSemanticStatementKindRef, KernelSemanticStatementRef,
};
use boon_contract::SourceBundleDigestV1;

#[derive(Clone, Copy)]
enum CheckedProgramSource<'a> {
    Rich(&'a CheckedProgramFields),
    Packed(&'a KernelSemanticInputV1),
}

/// One immutable checked-image view for semantic construction.
///
/// The view itself owns nothing. It may be copied freely during one semantic
/// phase, but no value borrowing it is stored in the durable semantic image.
#[derive(Clone, Copy)]
pub(crate) struct CheckedProgramView<'a> {
    source: CheckedProgramSource<'a>,
}

#[derive(Clone, Copy)]
pub(crate) enum ScopeRef<'a> {
    Rich(&'a CheckedScope),
    Packed(KernelSemanticScopeRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum StatementRef<'a> {
    Rich(&'a CheckedStatement),
    Packed(KernelSemanticStatementRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum ExpressionRef<'a> {
    Rich(&'a CheckedExpression),
    Packed(KernelSemanticExpressionRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum SourceRef<'a> {
    Rich(&'a CheckedSource),
    Packed(KernelSemanticSourceRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum StateRef<'a> {
    Rich(&'a CheckedState),
    Packed(KernelSemanticStateRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum ListRef<'a> {
    Rich(&'a CheckedList),
    Packed(KernelSemanticListRef<'a>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StatementKindRef {
    Function,
    Field,
    Source,
    Hold,
    List,
    Block,
    Spread,
    Expression,
}

pub(crate) enum StatementChildIter<'a> {
    Rich(std::slice::Iter<'a, CheckedStatementId>),
    Packed(KernelSemanticStatementChildIter<'a>),
}

impl Iterator for StatementChildIter<'_> {
    type Item = CheckedStatementId;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Rich(children) => children.next().copied(),
            Self::Packed(children) => children.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Rich(children) => children.size_hint(),
            Self::Packed(children) => children.size_hint(),
        }
    }
}

impl ExactSizeIterator for StatementChildIter<'_> {}

impl ScopeRef<'_> {
    pub(crate) fn id(self) -> LexicalScopeId {
        match self {
            Self::Rich(row) => row.id,
            Self::Packed(row) => row.id(),
        }
    }

    pub(crate) fn parent(self) -> Option<LexicalScopeId> {
        match self {
            Self::Rich(row) => row.parent,
            Self::Packed(row) => row.parent(),
        }
    }

    pub(crate) fn owner(self) -> Option<DeclId> {
        match self {
            Self::Rich(row) => row.owner,
            Self::Packed(row) => row.owner(),
        }
    }

    pub(crate) fn kind(self) -> CheckedScopeKind {
        match self {
            Self::Rich(row) => row.kind,
            Self::Packed(row) => row.kind(),
        }
    }
}

impl<'a> StatementRef<'a> {
    pub(crate) fn id(self) -> CheckedStatementId {
        match self {
            Self::Rich(row) => row.id,
            Self::Packed(row) => row.id(),
        }
    }

    pub(crate) fn scope(self) -> LexicalScopeId {
        match self {
            Self::Rich(row) => row.scope_id,
            Self::Packed(row) => row.scope(),
        }
    }

    pub(crate) fn declaration(self) -> Option<DeclId> {
        match self {
            Self::Rich(row) => match row.kind {
                CheckedStatementKind::Function { declaration }
                | CheckedStatementKind::Field { declaration } => Some(declaration),
                CheckedStatementKind::Source { declaration, .. }
                | CheckedStatementKind::Hold { declaration, .. }
                | CheckedStatementKind::List { declaration, .. } => declaration,
                CheckedStatementKind::Block
                | CheckedStatementKind::Spread
                | CheckedStatementKind::Expression => None,
            },
            Self::Packed(row) => row.declaration(),
        }
    }

    pub(crate) fn kind(self) -> StatementKindRef {
        match self {
            Self::Rich(row) => match row.kind {
                CheckedStatementKind::Function { .. } => StatementKindRef::Function,
                CheckedStatementKind::Field { .. } => StatementKindRef::Field,
                CheckedStatementKind::Source { .. } => StatementKindRef::Source,
                CheckedStatementKind::Hold { .. } => StatementKindRef::Hold,
                CheckedStatementKind::List { .. } => StatementKindRef::List,
                CheckedStatementKind::Block => StatementKindRef::Block,
                CheckedStatementKind::Spread => StatementKindRef::Spread,
                CheckedStatementKind::Expression => StatementKindRef::Expression,
            },
            Self::Packed(row) => match row.kind() {
                KernelSemanticStatementKindRef::Function { .. } => StatementKindRef::Function,
                KernelSemanticStatementKindRef::Field { .. } => StatementKindRef::Field,
                KernelSemanticStatementKindRef::Source { .. } => StatementKindRef::Source,
                KernelSemanticStatementKindRef::Hold { .. } => StatementKindRef::Hold,
                KernelSemanticStatementKindRef::List { .. } => StatementKindRef::List,
                KernelSemanticStatementKindRef::Block => StatementKindRef::Block,
                KernelSemanticStatementKindRef::Spread => StatementKindRef::Spread,
                KernelSemanticStatementKindRef::Expression => StatementKindRef::Expression,
            },
        }
    }

    pub(crate) fn value(self) -> Option<CheckedExprId> {
        match self {
            Self::Rich(row) => row.value,
            Self::Packed(row) => row.value(),
        }
    }

    pub(crate) fn children(self) -> StatementChildIter<'a> {
        match self {
            Self::Rich(row) => StatementChildIter::Rich(row.children.iter()),
            Self::Packed(row) => StatementChildIter::Packed(row.children()),
        }
    }
}

impl ExpressionRef<'_> {
    pub(crate) fn id(self) -> CheckedExprId {
        match self {
            Self::Rich(row) => row.id,
            Self::Packed(row) => row.id(),
        }
    }

    pub(crate) fn effect(self) -> CheckedEffectSummary {
        match self {
            Self::Rich(row) => row.effect,
            Self::Packed(row) => {
                let effect = row.effect();
                CheckedEffectSummary {
                    reads_state: effect.reads_state,
                    writes_state: effect.writes_state,
                    emits_source: effect.emits_source,
                    invokes_host: effect.invokes_host,
                }
            }
        }
    }
}

impl SourceRef<'_> {
    pub(crate) fn id(self) -> CheckedSourceId {
        match self {
            Self::Rich(row) => row.id,
            Self::Packed(row) => row.id(),
        }
    }

    pub(crate) fn declaration(self) -> DeclId {
        match self {
            Self::Rich(row) => row.declaration,
            Self::Packed(row) => row.declaration(),
        }
    }

    pub(crate) fn statement(self) -> CheckedStatementId {
        match self {
            Self::Rich(row) => row.statement,
            Self::Packed(row) => row.statement(),
        }
    }

    pub(crate) fn expression(self) -> CheckedExprId {
        match self {
            Self::Rich(row) => row.expression,
            Self::Packed(row) => row.expression(),
        }
    }

    pub(crate) fn owner_scope(self) -> LexicalScopeId {
        match self {
            Self::Rich(row) => row.owner_scope,
            Self::Packed(row) => row.owner_scope(),
        }
    }
}

impl StateRef<'_> {
    pub(crate) fn id(self) -> CheckedStateId {
        match self {
            Self::Rich(row) => row.id,
            Self::Packed(row) => row.id(),
        }
    }

    pub(crate) fn declaration(self) -> DeclId {
        match self {
            Self::Rich(row) => row.declaration,
            Self::Packed(row) => row.declaration(),
        }
    }

    pub(crate) fn statement(self) -> CheckedStatementId {
        match self {
            Self::Rich(row) => row.statement,
            Self::Packed(row) => row.statement(),
        }
    }

    pub(crate) fn expression(self) -> CheckedExprId {
        match self {
            Self::Rich(row) => row.expression,
            Self::Packed(row) => row.expression(),
        }
    }

    pub(crate) fn initial(self) -> CheckedExprId {
        match self {
            Self::Rich(row) => row.initial,
            Self::Packed(row) => row.initial(),
        }
    }

    pub(crate) fn owner_scope(self) -> LexicalScopeId {
        match self {
            Self::Rich(row) => row.owner_scope,
            Self::Packed(row) => row.owner_scope(),
        }
    }
}

impl ListRef<'_> {
    pub(crate) fn id(self) -> CheckedListId {
        match self {
            Self::Rich(row) => row.id,
            Self::Packed(row) => row.id(),
        }
    }

    pub(crate) fn declaration(self) -> DeclId {
        match self {
            Self::Rich(row) => row.declaration,
            Self::Packed(row) => row.declaration(),
        }
    }

    pub(crate) fn statement(self) -> CheckedStatementId {
        match self {
            Self::Rich(row) => row.statement,
            Self::Packed(row) => row.statement(),
        }
    }

    pub(crate) fn producer(self) -> CheckedExprId {
        match self {
            Self::Rich(row) => row.producer,
            Self::Packed(row) => row.producer(),
        }
    }

    pub(crate) fn owner_scope(self) -> LexicalScopeId {
        match self {
            Self::Rich(row) => row.owner_scope,
            Self::Packed(row) => row.owner_scope(),
        }
    }
}

macro_rules! checked_rows {
    (
        $count:ident,
        $at:ident,
        $iter:ident,
        $rich:ident,
        $packed_count:ident,
        $packed_get:ident,
        $id:ident,
        $row:ident
    ) => {
        pub(crate) fn $count(self) -> usize {
            match self.source {
                CheckedProgramSource::Rich(program) => program.$rich.len(),
                CheckedProgramSource::Packed(input) => input.entity_counts().$packed_count,
            }
        }

        pub(crate) fn $at(self, index: usize) -> Option<$row<'a>> {
            match self.source {
                CheckedProgramSource::Rich(program) => {
                    let row = program.$rich.get(index)?;
                    (row.id.0 as usize == index).then_some($row::Rich(row))
                }
                CheckedProgramSource::Packed(input) => input
                    .$packed_get($id(u32::try_from(index).ok()?))
                    .map($row::Packed),
            }
        }

        pub(crate) fn $iter(self) -> impl ExactSizeIterator<Item = $row<'a>> + 'a {
            (0..self.$count()).map(move |index| {
                self.$at(index)
                    .expect("sealed checked base-row authority remains dense")
            })
        }
    };
}

impl<'a> CheckedProgramView<'a> {
    pub(crate) fn rich(program: &'a CheckedProgramFields) -> Self {
        Self {
            source: CheckedProgramSource::Rich(program),
        }
    }

    pub(crate) fn packed(input: &'a KernelSemanticInputV1) -> Self {
        Self {
            source: CheckedProgramSource::Packed(input),
        }
    }

    pub(crate) fn source_bundle_digest(self) -> SourceBundleDigestV1 {
        match self.source {
            CheckedProgramSource::Rich(program) => program.source_bundle_digest_v1,
            CheckedProgramSource::Packed(input) => input.source_bundle_digest_v1(),
        }
    }

    pub(crate) fn role(self) -> ProgramRole {
        match self.source {
            CheckedProgramSource::Rich(program) => program.role,
            CheckedProgramSource::Packed(input) => input.role(),
        }
    }

    pub(crate) fn root_scope(self) -> LexicalScopeId {
        match self.source {
            CheckedProgramSource::Rich(program) => program.root_scope,
            CheckedProgramSource::Packed(input) => input.project_root_scope().id(),
        }
    }

    /// Rich editor/oracle metadata remains independent of the packed
    /// statement reconstruction. RuntimePacked deliberately has no duplicate
    /// output-root table, so differential builds compare the two authorities.
    pub(crate) fn rich_output_root_types(self) -> Option<&'a [OutputRootTypeEntry]> {
        match self.source {
            CheckedProgramSource::Rich(program) => {
                Some(&program.lowering_metadata.output_root_types)
            }
            CheckedProgramSource::Packed(_) => None,
        }
    }

    checked_rows!(
        scope_count,
        scope_at,
        scopes,
        scopes,
        scopes,
        scope,
        LexicalScopeId,
        ScopeRef
    );
    checked_rows!(
        statement_count,
        statement_at,
        statements,
        statements,
        statements,
        statement,
        CheckedStatementId,
        StatementRef
    );
    checked_rows!(
        expression_count,
        expression_at,
        expressions,
        expressions,
        expressions,
        expression,
        CheckedExprId,
        ExpressionRef
    );
    checked_rows!(
        source_count,
        source_at,
        sources,
        sources,
        sources,
        source,
        CheckedSourceId,
        SourceRef
    );
    checked_rows!(
        state_count,
        state_at,
        states,
        states,
        states,
        state,
        CheckedStateId,
        StateRef
    );
    checked_rows!(
        list_count,
        list_at,
        lists,
        lists,
        lists,
        list,
        CheckedListId,
        ListRef
    );

    pub(crate) fn scope(self, id: LexicalScopeId) -> Option<ScopeRef<'a>> {
        self.scope_at(id.0 as usize).filter(|row| row.id() == id)
    }

    pub(crate) fn statement(self, id: CheckedStatementId) -> Option<StatementRef<'a>> {
        self.statement_at(id.0 as usize)
            .filter(|row| row.id() == id)
    }

    pub(crate) fn expression(self, id: CheckedExprId) -> Option<ExpressionRef<'a>> {
        self.expression_at(id.0 as usize)
            .filter(|row| row.id() == id)
    }

    pub(crate) fn source(self, id: CheckedSourceId) -> Option<SourceRef<'a>> {
        self.source_at(id.0 as usize).filter(|row| row.id() == id)
    }

    pub(crate) fn state(self, id: CheckedStateId) -> Option<StateRef<'a>> {
        self.state_at(id.0 as usize).filter(|row| row.id() == id)
    }

    pub(crate) fn list(self, id: CheckedListId) -> Option<ListRef<'a>> {
        self.list_at(id.0 as usize).filter(|row| row.id() == id)
    }
}
