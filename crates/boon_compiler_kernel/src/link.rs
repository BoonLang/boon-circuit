use crate::{
    KernelCallTarget, KernelCheckedSnapshot, KernelDeclarationReference, KernelExternalTarget,
    KernelLexicalBindingTarget, KernelOwnerId, KernelProjectInput, KernelStatementChildReference,
    KernelStatementReference, KernelValueReference,
};
use boon_checked::{
    CheckedCallId, CheckedExprId, CheckedListId, CheckedSourceId, CheckedStateId,
    CheckedStatementId, ContextFormalId, DeclId,
};
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
    pub expressions: KernelCheckedRowRange,
    pub statements: KernelCheckedRowRange,
    pub declarations: KernelCheckedRowRange,
    pub calls: KernelCheckedRowRange,
    pub sources: KernelCheckedRowRange,
    pub states: KernelCheckedRowRange,
    pub lists: KernelCheckedRowRange,
    pub root_statement: CheckedStatementId,
    pub public_declaration: DeclId,
    pub result_expression: CheckedExprId,
    pub context_formal: Option<ContextFormalId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelCheckedLinkTotals {
    pub expressions: u32,
    pub statements: u32,
    pub declarations: u32,
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
        let mut definitions = Vec::with_capacity(snapshot.definitions.len());
        let mut public_declaration_authorities = Vec::with_capacity(snapshot.definitions.len());
        for (index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(index).map_err(|_| {
                KernelCheckedLinkError::new("kernel checked linker definition count exceeds u32")
            })?);
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
                expressions,
                statements,
                declarations,
                calls,
                sources,
                states,
                lists,
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
                    crate::KernelExecutionShapeArtifact::MatchArm { bindings, .. } => {
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
        KernelStatementInput, KernelStatementKind,
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
            expression_payloads: vec![crate::KernelExpressionSemanticPayload::None]
                .into_boxed_slice(),
            call_syntax: Box::new([]),
            execution_shapes: Box::new([]),
            statements: vec![KernelStatementInput {
                id: KernelStatementId(0),
                kind: KernelStatementKind::Field { name: name.into() },
                value: Some(KernelExpressionId(0)),
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
        assert_eq!(layout.totals().statements, 2);
        assert_eq!(layout.totals().declarations, 2);
        assert!(layout.totals().resolved_references >= 4);
        assert_eq!(layout.definitions()[0].result_expression, CheckedExprId(0));
        assert_eq!(layout.definitions()[1].result_expression, CheckedExprId(1));
        assert_eq!(layout.definitions()[0].public_declaration, DeclId(0));
        assert_eq!(layout.definitions()[1].public_declaration, DeclId(1));
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
            DeclId(0),
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

        let mut delegated = (*snapshot).clone();
        delegated.definitions[1].linkage.public_declaration =
            Some(KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)));
        let delegated_layout = KernelCheckedLinkLayout::new(&project, &delegated)
            .expect("a nested definition may share its enclosing public declaration");
        assert_eq!(
            delegated_layout.definitions()[1].public_declaration,
            DeclId(0)
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
