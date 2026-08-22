//! Allocation-free borrowed access to the checked program's base rows.
//!
//! The wrappers intentionally expose only data that a future packed semantic
//! input can provide without reconstructing `CheckedProgramFields`. Calls and
//! definition execution templates already have their own dual-source catalogs.
//! Rich nested statement/expression variants remain accessible only through an
//! explicitly rich-only method until the packed backend defines their views.

#![allow(dead_code)]

use boon_checked::{
    CheckedCallableKind, CheckedCallableSignature, CheckedContextFormal, CheckedDeclaration,
    CheckedDeclarationKind, CheckedEffectSummary, CheckedExprId, CheckedExpression,
    CheckedExpressionKind, CheckedList, CheckedListId, CheckedListKeyPolicy, CheckedProgramFields,
    CheckedScope, CheckedScopeKind, CheckedSemanticPath, CheckedSource, CheckedSourceId,
    CheckedSpan, CheckedState, CheckedStateId, CheckedStateKind, CheckedStatement,
    CheckedStatementId, CheckedStatementKind, CheckedTypeView, CheckedValueUse, ContextFormalId,
    DeclId, FlowMode, FlowType, LexicalScopeId, ProgramRole, Type,
};
use boon_contract::SourceBundleDigestV1;

#[derive(Clone, Copy)]
enum CheckedProgramSource<'a> {
    Rich(&'a CheckedProgramFields),
    // Future: Packed(&'a boon_compiler_kernel::KernelSemanticInputV1).
}

/// One immutable, allocation-free checked-image view for semantic lowering.
#[derive(Clone, Copy)]
pub(crate) struct CheckedProgramView<'a> {
    source: CheckedProgramSource<'a>,
}

macro_rules! borrowed_row {
    ($wrapper:ident, $source:ident, $rich:ty) => {
        #[derive(Clone, Copy)]
        pub(crate) struct $wrapper<'a> {
            source: $source<'a>,
        }

        #[derive(Clone, Copy)]
        enum $source<'a> {
            Rich(&'a $rich),
            // Add the packed row variant with the packed program backend.
        }

        impl<'a> $wrapper<'a> {
            fn rich(row: &'a $rich) -> Self {
                Self {
                    source: $source::Rich(row),
                }
            }
        }
    };
}

borrowed_row!(ScopeRef, ScopeSource, CheckedScope);
borrowed_row!(DeclarationRef, DeclarationSource, CheckedDeclaration);
borrowed_row!(StatementRef, StatementSource, CheckedStatement);
borrowed_row!(ExpressionRef, ExpressionSource, CheckedExpression);
borrowed_row!(CallableRef, CallableSource, CheckedCallableSignature);
borrowed_row!(ContextFormalRef, ContextFormalSource, CheckedContextFormal);
borrowed_row!(SourceRef, SourceSource, CheckedSource);
borrowed_row!(StateRef, StateSource, CheckedState);
borrowed_row!(ListRef, ListSource, CheckedList);

#[derive(Clone, Copy)]
enum TypeSource<'a> {
    Rich(&'a Type),
    // Future: store-qualified packed type reference.
}

/// Borrowed recursive-type inspection; never materializes or clones `Type`.
#[derive(Clone, Copy)]
pub(crate) struct TypeRef<'a> {
    source: TypeSource<'a>,
}

impl<'a> TypeRef<'a> {
    fn rich(ty: &'a Type) -> Self {
        Self {
            source: TypeSource::Rich(ty),
        }
    }
}

impl CheckedTypeView for TypeRef<'_> {
    fn list_item(self) -> Option<Self> {
        match self.source {
            TypeSource::Rich(ty) => ty.list_item().map(TypeRef::rich),
        }
    }

    fn is_text(self) -> bool {
        match self.source {
            TypeSource::Rich(ty) => ty.is_text(),
        }
    }

    fn is_number(self) -> bool {
        match self.source {
            TypeSource::Rich(ty) => ty.is_number(),
        }
    }

    fn is_render_contract(self) -> bool {
        match self.source {
            TypeSource::Rich(ty) => ty.is_render_contract(),
        }
    }

    fn object_field(self, name: &str) -> Option<Self> {
        match self.source {
            TypeSource::Rich(ty) => ty.object_field(name).map(TypeRef::rich),
        }
    }

    fn all_variants_are_bare_tags(self, predicate: impl FnMut(&str) -> bool) -> bool {
        match self.source {
            TypeSource::Rich(ty) => ty.all_variants_are_bare_tags(predicate),
        }
    }
}

#[derive(Clone, Copy)]
enum FlowSource<'a> {
    Rich(&'a FlowType),
    // Future: store-qualified packed flow reference.
}

#[derive(Clone, Copy)]
pub(crate) struct FlowRef<'a> {
    source: FlowSource<'a>,
}

impl<'a> FlowRef<'a> {
    fn rich(flow: &'a FlowType) -> Self {
        Self {
            source: FlowSource::Rich(flow),
        }
    }

    pub(crate) fn mode(self) -> FlowMode {
        match self.source {
            FlowSource::Rich(flow) => flow.mode,
        }
    }

    pub(crate) fn ty(self) -> TypeRef<'a> {
        match self.source {
            FlowSource::Rich(flow) => TypeRef::rich(&flow.ty),
        }
    }
}

#[derive(Clone, Copy)]
enum StringPathSource<'a> {
    Rich(&'a [String]),
    // Future: SymbolId range plus its text-catalog authority.
}

/// Borrowed authored path. Segments are returned as `&str`.
#[derive(Clone, Copy)]
pub(crate) struct StringPathRef<'a> {
    source: StringPathSource<'a>,
}

impl<'a> StringPathRef<'a> {
    fn rich(path: &'a [String]) -> Self {
        Self {
            source: StringPathSource::Rich(path),
        }
    }

    pub(crate) fn len(self) -> usize {
        match self.source {
            StringPathSource::Rich(path) => path.len(),
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub(crate) fn get(self, index: usize) -> Option<&'a str> {
        match self.source {
            StringPathSource::Rich(path) => path.get(index).map(String::as_str),
        }
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = &'a str> + 'a {
        (0..self.len()).filter_map(move |index| self.get(index))
    }
}

#[derive(Clone, Copy)]
enum SemanticPathSource<'a> {
    Rich(&'a CheckedSemanticPath),
    // Future: packed anchor plus SymbolId projection range.
}

#[derive(Clone, Copy)]
pub(crate) struct SemanticPathRef<'a> {
    source: SemanticPathSource<'a>,
}

impl<'a> SemanticPathRef<'a> {
    fn rich(path: &'a CheckedSemanticPath) -> Self {
        Self {
            source: SemanticPathSource::Rich(path),
        }
    }

    pub(crate) fn anchor(self) -> DeclId {
        match self.source {
            SemanticPathSource::Rich(path) => path.anchor,
        }
    }

    pub(crate) fn projection(self) -> StringPathRef<'a> {
        match self.source {
            SemanticPathSource::Rich(path) => StringPathRef::rich(&path.projection),
        }
    }
}

macro_rules! rich_copy_accessors {
    ($wrapper:ident, $source:ident; $($method:ident -> $ty:ty = $field:ident),+ $(,)?) => {
        impl $wrapper<'_> {
            $(
                pub(crate) fn $method(self) -> $ty {
                    match self.source {
                        $source::Rich(row) => row.$field,
                    }
                }
            )+
        }
    };
}

rich_copy_accessors!(ScopeRef, ScopeSource;
    id -> LexicalScopeId = id,
    parent -> Option<LexicalScopeId> = parent,
    owner -> Option<DeclId> = owner,
    kind -> CheckedScopeKind = kind,
    span -> CheckedSpan = span,
);

rich_copy_accessors!(DeclarationRef, DeclarationSource;
    id -> DeclId = id,
    scope -> LexicalScopeId = scope_id,
    kind -> CheckedDeclarationKind = kind,
    value -> Option<CheckedExprId> = value,
    body_scope -> Option<LexicalScopeId> = body_scope,
    span -> CheckedSpan = span,
);

impl<'a> DeclarationRef<'a> {
    pub(crate) fn name(self) -> &'a str {
        match self.source {
            DeclarationSource::Rich(row) => &row.name,
        }
    }

    pub(crate) fn flow(self) -> FlowRef<'a> {
        match self.source {
            DeclarationSource::Rich(row) => FlowRef::rich(&row.flow_type),
        }
    }
}

rich_copy_accessors!(StatementRef, StatementSource;
    id -> CheckedStatementId = id,
    scope -> LexicalScopeId = scope_id,
    value -> Option<CheckedExprId> = value,
    value_use -> CheckedValueUse = value_use,
    span -> CheckedSpan = span,
);

impl<'a> StatementRef<'a> {
    /// Transitional rich-only access. Do not use from packed-capable code.
    pub(crate) fn rich_kind(self) -> &'a CheckedStatementKind {
        match self.source {
            StatementSource::Rich(row) => &row.kind,
        }
    }
}

rich_copy_accessors!(ExpressionRef, ExpressionSource;
    id -> CheckedExprId = id,
    scope -> LexicalScopeId = scope_id,
    declaration -> Option<DeclId> = declaration,
    effect -> CheckedEffectSummary = effect,
    span -> CheckedSpan = span,
);

impl<'a> ExpressionRef<'a> {
    pub(crate) fn flow(self) -> FlowRef<'a> {
        match self.source {
            ExpressionSource::Rich(row) => FlowRef::rich(&row.flow_type),
        }
    }

    pub(crate) fn flush_type(self) -> Option<TypeRef<'a>> {
        match self.source {
            ExpressionSource::Rich(row) => row.flush_type.as_ref().map(TypeRef::rich),
        }
    }

    /// Transitional rich-only access. Do not use from packed-capable code.
    pub(crate) fn rich_kind(self) -> &'a CheckedExpressionKind {
        match self.source {
            ExpressionSource::Rich(row) => &row.kind,
        }
    }
}

rich_copy_accessors!(CallableRef, CallableSource;
    declaration -> DeclId = decl_id,
    scope -> LexicalScopeId = scope_id,
    kind -> CheckedCallableKind = kind,
    context_formal -> Option<ContextFormalId> = context_formal,
    role -> ProgramRole = role,
    effect -> CheckedEffectSummary = effect,
    body -> Option<CheckedStatementId> = body,
    result_expression -> Option<CheckedExprId> = result_expression,
);

impl<'a> CallableRef<'a> {
    pub(crate) fn name(self) -> &'a str {
        match self.source {
            CallableSource::Rich(row) => &row.name,
        }
    }

    pub(crate) fn result(self) -> FlowRef<'a> {
        match self.source {
            CallableSource::Rich(row) => FlowRef::rich(&row.result),
        }
    }
}

rich_copy_accessors!(ContextFormalRef, ContextFormalSource;
    id -> ContextFormalId = id,
    callable -> DeclId = callable,
);

impl<'a> ContextFormalRef<'a> {
    pub(crate) fn flow(self) -> FlowRef<'a> {
        match self.source {
            ContextFormalSource::Rich(row) => FlowRef::rich(&row.scheme.flow_type),
        }
    }

    pub(crate) fn projection_count(self) -> usize {
        match self.source {
            ContextFormalSource::Rich(row) => row.scheme.projections.len(),
        }
    }

    pub(crate) fn projection(self, index: usize) -> Option<StringPathRef<'a>> {
        match self.source {
            ContextFormalSource::Rich(row) => row
                .scheme
                .projections
                .get(index)
                .map(|path| StringPathRef::rich(path)),
        }
    }
}

macro_rules! resource_row {
    ($wrapper:ident, $source:ident, $id_ty:ty, $type_method:ident, $type_field:ident) => {
        impl<'a> $wrapper<'a> {
            pub(crate) fn id(self) -> $id_ty {
                match self.source {
                    $source::Rich(row) => row.id,
                }
            }

            pub(crate) fn declaration(self) -> DeclId {
                match self.source {
                    $source::Rich(row) => row.declaration,
                }
            }

            pub(crate) fn statement(self) -> CheckedStatementId {
                match self.source {
                    $source::Rich(row) => row.statement,
                }
            }

            pub(crate) fn owner_scope(self) -> LexicalScopeId {
                match self.source {
                    $source::Rich(row) => row.owner_scope,
                }
            }

            pub(crate) fn path(self) -> SemanticPathRef<'a> {
                match self.source {
                    $source::Rich(row) => SemanticPathRef::rich(&row.path),
                }
            }

            pub(crate) fn $type_method(self) -> TypeRef<'a> {
                match self.source {
                    $source::Rich(row) => TypeRef::rich(&row.$type_field),
                }
            }

            pub(crate) fn span(self) -> CheckedSpan {
                match self.source {
                    $source::Rich(row) => row.span,
                }
            }
        }
    };
}

resource_row!(
    SourceRef,
    SourceSource,
    CheckedSourceId,
    payload_type,
    payload_type
);
resource_row!(ListRef, ListSource, CheckedListId, item_type, item_type);

impl SourceRef<'_> {
    pub(crate) fn expression(self) -> CheckedExprId {
        match self.source {
            SourceSource::Rich(row) => row.expression,
        }
    }

    pub(crate) fn interval_ms(self) -> Option<u64> {
        match self.source {
            SourceSource::Rich(row) => row.interval_ms,
        }
    }
}

impl<'a> StateRef<'a> {
    pub(crate) fn id(self) -> CheckedStateId {
        match self.source {
            StateSource::Rich(row) => row.id,
        }
    }

    pub(crate) fn binding_declaration(self) -> DeclId {
        match self.source {
            StateSource::Rich(row) => row.binding_declaration,
        }
    }

    pub(crate) fn declaration(self) -> DeclId {
        match self.source {
            StateSource::Rich(row) => row.declaration,
        }
    }

    pub(crate) fn statement(self) -> CheckedStatementId {
        match self.source {
            StateSource::Rich(row) => row.statement,
        }
    }

    pub(crate) fn expression(self) -> CheckedExprId {
        match self.source {
            StateSource::Rich(row) => row.expression,
        }
    }

    pub(crate) fn initial(self) -> CheckedExprId {
        match self.source {
            StateSource::Rich(row) => row.initial,
        }
    }

    pub(crate) fn owner_scope(self) -> LexicalScopeId {
        match self.source {
            StateSource::Rich(row) => row.owner_scope,
        }
    }

    pub(crate) fn path(self) -> SemanticPathRef<'a> {
        match self.source {
            StateSource::Rich(row) => SemanticPathRef::rich(&row.path),
        }
    }

    pub(crate) fn kind(self) -> CheckedStateKind {
        match self.source {
            StateSource::Rich(row) => row.kind,
        }
    }

    pub(crate) fn flow(self) -> FlowRef<'a> {
        match self.source {
            StateSource::Rich(row) => FlowRef::rich(&row.flow_type),
        }
    }

    pub(crate) fn span(self) -> CheckedSpan {
        match self.source {
            StateSource::Rich(row) => row.span,
        }
    }
}

impl ListRef<'_> {
    pub(crate) fn producer(self) -> CheckedExprId {
        match self.source {
            ListSource::Rich(row) => row.producer,
        }
    }

    pub(crate) fn capacity(self) -> Option<usize> {
        match self.source {
            ListSource::Rich(row) => row.capacity,
        }
    }

    pub(crate) fn key_policy(self) -> CheckedListKeyPolicy {
        match self.source {
            ListSource::Rich(row) => row.key_policy,
        }
    }
}

macro_rules! program_rows {
    ($count:ident, $at:ident, $iter:ident, $field:ident, $row:ident) => {
        pub(crate) fn $count(self) -> usize {
            match self.source {
                CheckedProgramSource::Rich(program) => program.$field.len(),
            }
        }

        pub(crate) fn $at(self, index: usize) -> Option<$row<'a>> {
            match self.source {
                CheckedProgramSource::Rich(program) => program.$field.get(index).map($row::rich),
            }
        }

        pub(crate) fn $iter(self) -> impl Iterator<Item = $row<'a>> + 'a {
            (0..self.$count()).filter_map(move |index| self.$at(index))
        }
    };
}

impl<'a> CheckedProgramView<'a> {
    pub(crate) fn rich(program: &'a CheckedProgramFields) -> Self {
        Self {
            source: CheckedProgramSource::Rich(program),
        }
    }

    pub(crate) fn source_bundle_digest(self) -> SourceBundleDigestV1 {
        match self.source {
            CheckedProgramSource::Rich(program) => program.source_bundle_digest_v1,
        }
    }

    pub(crate) fn role(self) -> ProgramRole {
        match self.source {
            CheckedProgramSource::Rich(program) => program.role,
        }
    }

    pub(crate) fn root_scope(self) -> LexicalScopeId {
        match self.source {
            CheckedProgramSource::Rich(program) => program.root_scope,
        }
    }

    program_rows!(scope_count, scope_at, scopes, scopes, ScopeRef);
    program_rows!(
        declaration_count,
        declaration_at,
        declarations,
        declarations,
        DeclarationRef
    );
    program_rows!(
        statement_count,
        statement_at,
        statements,
        statements,
        StatementRef
    );
    program_rows!(
        expression_count,
        expression_at,
        expressions,
        expressions,
        ExpressionRef
    );
    program_rows!(
        callable_count,
        callable_at,
        callables,
        callables,
        CallableRef
    );
    program_rows!(
        context_formal_count,
        context_formal_at,
        context_formals,
        context_formals,
        ContextFormalRef
    );
    program_rows!(source_count, source_at, sources, sources, SourceRef);
    program_rows!(state_count, state_at, states, states, StateRef);
    program_rows!(list_count, list_at, lists, lists, ListRef);

    pub(crate) fn scope(self, id: LexicalScopeId) -> Option<ScopeRef<'a>> {
        self.scope_at(id.0 as usize).filter(|row| row.id() == id)
    }

    pub(crate) fn declaration(self, id: DeclId) -> Option<DeclarationRef<'a>> {
        self.declaration_at(id.0 as usize)
            .filter(|row| row.id() == id)
    }

    pub(crate) fn statement(self, id: CheckedStatementId) -> Option<StatementRef<'a>> {
        self.statement_at(id.0 as usize)
            .filter(|row| row.id() == id)
    }

    pub(crate) fn expression(self, id: CheckedExprId) -> Option<ExpressionRef<'a>> {
        self.expression_at(id.0 as usize)
            .filter(|row| row.id() == id)
    }

    pub(crate) fn callable(self, declaration: DeclId) -> Option<CallableRef<'a>> {
        self.callables()
            .find(|row| row.declaration() == declaration)
    }

    pub(crate) fn context_formal(self, id: ContextFormalId) -> Option<ContextFormalRef<'a>> {
        self.context_formals().find(|row| row.id() == id)
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
