use crate::{
    KernelCallTarget, KernelCheckedSnapshot, KernelDeclarationReference, KernelExternalTarget,
    KernelLexicalBindingTarget, KernelOwnerId, KernelProjectInput, KernelScopeReference,
    KernelStatementChildReference, KernelStatementReference, KernelValueReference,
};
use boon_checked::{
    CheckedCallId, CheckedDeclaration, CheckedDeclarationKind, CheckedExprId, CheckedListId,
    CheckedResourceBinding, CheckedScope, CheckedScopeKind, CheckedSourceId, CheckedSpan,
    CheckedStateId, CheckedStatement, CheckedStatementId, CheckedStatementKind, CheckedValueUse,
    ContextFormalId, DeclId, FlowMode, FlowType, LexicalScopeId, ObjectShape, Type, TypeVar,
    Variant,
};
use std::collections::BTreeSet;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelCheckedLinkTotals {
    /// Includes the single project-root scope at row zero.
    pub scopes: u32,
    pub expressions: u32,
    pub statements: u32,
    pub declarations: u32,
    pub type_variables: u32,
    pub calls: u32,
    pub context_formals: u32,
    pub sources: u32,
    pub states: u32,
    pub lists: u32,
    pub resolved_references: u64,
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
            totals,
        };
        layout.validate_references(snapshot)?;
        Ok(layout)
    }

    pub fn definitions(&self) -> &[KernelCheckedDefinitionLayout] {
        &self.definitions
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
    /// declaration namespace. Stable ABI declarations are appended by the
    /// compiler facade after these rows; they never participate in kernel
    /// definition solving or local declaration relocation.
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
            self.totals
                .declarations
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
        if declarations.len() + 1 != self.totals.declarations as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked declaration materializer produced {} rows for a namespace ending at {}",
                declarations.len(),
                self.totals.declarations,
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
        let provider_facts = facts(&unit, &provider_key, "provider");
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
