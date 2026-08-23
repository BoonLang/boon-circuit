use crate::definition_code::Span32;
use crate::{
    KernelCallArgumentKind, KernelConditionalKind, KernelDeclarationId, KernelDeclarationKind,
    KernelDeclarationOrigin, KernelDeclarationReference, KernelDefinitionFactsInput,
    KernelDefinitionLinkage, KernelExecutionShapeInput, KernelExpressionId,
    KernelExpressionRelocation, KernelExpressionSemanticPayload, KernelLexicalAccess,
    KernelLexicalBindingTargetInput, KernelListId, KernelMatchPatternPayload,
    KernelOwnerBuildError, KernelOwnerId, KernelParameterEvaluationScope, KernelParameterKind,
    KernelScopeId, KernelScopeKind, KernelScopeOrigin, KernelScopeReference, KernelSourceId,
    KernelSourceSpan, KernelStateId, KernelStatementChildReference, KernelStatementId,
    KernelStatementKind, KernelStatementReference, KernelStatementValueUse,
    KernelStructuralDeclarationInput, KernelTextTemplateSegment,
};
use boon_checked::{CheckedListKeyPolicy, CheckedStateKind};
use boon_contract::{PathId, ProjectTextSnapshot, SymbolId};
use boon_data::{Bits, ExactNumber};
use boon_syntax::{StableOccurrenceKey, StableStatementKey};
use std::sync::Arc;

pub(crate) fn pack_definition_facts_store(
    text: &ProjectTextSnapshot,
    facts: &[KernelDefinitionFactsInput],
) -> Result<Arc<PackedDefinitionFactsStore>, KernelOwnerBuildError> {
    let mut builder = PackedDefinitionFactsStoreBuilder::with_capacity(facts.len());
    for definition in facts {
        builder.push(text, definition)?;
    }
    Ok(Arc::new(builder.finish()))
}

/// Fixed-width source coordinates carried by the normal checked/semantic
/// path. Parser-sized `usize` values are validated once at the construction
/// boundary and never widen every retained row on 64-bit hosts.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct PackedSourceSpan {
    pub(crate) line: u32,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl PackedSourceSpan {
    fn pack(span: KernelSourceSpan, label: &str) -> Result<Self, KernelOwnerBuildError> {
        Ok(Self {
            line: checked_u32(span.line, label)?,
            start: checked_u32(span.start, label)?,
            end: checked_u32(span.end, label)?,
        })
    }

    pub(crate) const fn materialize(self) -> KernelSourceSpan {
        KernelSourceSpan {
            line: self.line as usize,
            start: self.start as usize,
            end: self.end as usize,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedScopePresentation {
    pub(crate) id: KernelScopeId,
    pub(crate) parent: KernelScopeReference,
    pub(crate) owner: Option<KernelDeclarationReference>,
    pub(crate) kind: KernelScopeKind,
    pub(crate) origin: KernelScopeOrigin,
    pub(crate) span: PackedSourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedExpressionPresentation {
    pub(crate) expression: KernelExpressionId,
    pub(crate) scope: KernelScopeReference,
    pub(crate) declaration: Option<KernelDeclarationReference>,
    pub(crate) declaration_scope: Option<KernelScopeReference>,
    pub(crate) span: PackedSourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedStatementPresentation {
    pub(crate) statement: KernelStatementId,
    pub(crate) scope: KernelScopeReference,
    pub(crate) body_scope: Option<KernelScopeId>,
    pub(crate) span: PackedSourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedDeclarationPresentation {
    pub(crate) declaration: KernelDeclarationId,
    pub(crate) scope: KernelScopeReference,
    pub(crate) body_scope: Option<KernelScopeId>,
    pub(crate) span: PackedSourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedStatementParameter {
    pub(crate) declaration: crate::KernelDeclarationId,
    pub(crate) name: SymbolId,
    pub(crate) kind: KernelParameterKind,
    pub(crate) ordinal: u32,
    pub(crate) evaluation_scope: KernelParameterEvaluationScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackedStatementKind {
    Function {
        name: SymbolId,
        parameters: Span32,
    },
    Field {
        name: SymbolId,
    },
    Source {
        field: Option<SymbolId>,
        event: Option<SymbolId>,
    },
    Hold {
        field: Option<SymbolId>,
        name: Option<SymbolId>,
    },
    List {
        field: Option<SymbolId>,
        capacity: Option<u32>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedStatement {
    pub(crate) id: KernelStatementId,
    pub(crate) kind: PackedStatementKind,
    pub(crate) value: Option<KernelExpressionId>,
    pub(crate) value_use: KernelStatementValueUse,
    children: Span32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedDeclaration {
    pub(crate) id: KernelDeclarationId,
    pub(crate) origin: KernelDeclarationOrigin,
    pub(crate) name: SymbolId,
    pub(crate) kind: KernelDeclarationKind,
    pub(crate) value: Option<KernelExpressionId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedLexicalBinding {
    pub(crate) expression: KernelExpressionId,
    pub(crate) target: KernelLexicalBindingTargetInput,
    pub(crate) projection: PathId,
    pub(crate) access: KernelLexicalAccess,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedSource {
    pub(crate) id: KernelSourceId,
    pub(crate) declaration: KernelDeclarationReference,
    pub(crate) statement: KernelStatementReference,
    pub(crate) expression: KernelExpressionId,
    pub(crate) projection: PathId,
    pub(crate) interval_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedState {
    pub(crate) id: KernelStateId,
    pub(crate) binding_declaration: KernelDeclarationReference,
    pub(crate) declaration: KernelDeclarationReference,
    pub(crate) statement: KernelStatementReference,
    pub(crate) expression: KernelExpressionId,
    pub(crate) initial: KernelExpressionId,
    pub(crate) projection: PathId,
    pub(crate) synthetic_path: bool,
    pub(crate) kind: CheckedStateKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedList {
    pub(crate) id: KernelListId,
    pub(crate) declaration: KernelDeclarationReference,
    pub(crate) statement: KernelStatementReference,
    pub(crate) producer: KernelExpressionId,
    pub(crate) projection: PathId,
    pub(crate) capacity: Option<u32>,
    pub(crate) key_policy: CheckedListKeyPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedCallArgument {
    pub(crate) ordinal: u32,
    pub(crate) kind: KernelCallArgumentKind,
    pub(crate) name: SymbolId,
    pub(crate) value: KernelExpressionId,
    pub(crate) span: PackedSourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedCallSyntax {
    pub(crate) expression: KernelExpressionId,
    pub(crate) authored_site_digest_v4: [u8; 32],
    pub(crate) function: SymbolId,
    pub(crate) pipe_input: Option<KernelExpressionId>,
    arguments: Span32,
    occurrence: u32,
    pub(crate) pass: Option<PackedCallPass>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedCallPass {
    pub(crate) value: KernelExpressionId,
    pub(crate) final_clause: bool,
    pub(crate) span: PackedSourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedExecutionRecordField {
    pub(crate) ordinal: u32,
    pub(crate) declaration: Option<KernelStructuralDeclarationInput>,
    pub(crate) name: SymbolId,
    pub(crate) value: KernelExpressionId,
    pub(crate) spread: bool,
    pub(crate) span: PackedSourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedExecutionBlockBinding {
    pub(crate) ordinal: u32,
    pub(crate) declaration: KernelStructuralDeclarationInput,
    pub(crate) value: KernelExpressionId,
    pub(crate) span: PackedSourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackedExecutionShape {
    Conditional {
        expression: KernelExpressionId,
        kind: KernelConditionalKind,
    },
    Record {
        expression: KernelExpressionId,
        fields: Span32,
    },
    Block {
        expression: KernelExpressionId,
        bindings: Span32,
        result: Option<KernelExpressionId>,
    },
    MatchArm {
        expression: KernelExpressionId,
        selector: KernelExpressionId,
        bindings: Span32,
    },
}

impl PackedExecutionShape {
    pub(crate) const fn expression(self) -> KernelExpressionId {
        match self {
            Self::Conditional { expression, .. }
            | Self::Record { expression, .. }
            | Self::Block { expression, .. }
            | Self::MatchArm { expression, .. } => expression,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackedTextTemplateSegment {
    Static(Span32),
    Dynamic(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackedMatchPatternPayload {
    Wildcard,
    Number(u32),
    Text(Span32),
    Tag { name: SymbolId, fields: PathId },
    Binding(SymbolId),
    Bits(u32),
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackedExpressionPayload {
    None,
    Text(Span32),
    TextTemplate(Span32),
    Number(u32),
    Byte(u8),
    Bits(u32),
    HoldName(SymbolId),
    MatchPattern(PackedMatchPatternPayload),
    Delimiter,
    LexicalPath(PathId),
    Invalid(Span32),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PackedDefinitionFacts {
    linkage: KernelDefinitionLinkage,
    containing_scope: KernelScopeReference,
    expression_relocations: Span32,
    statement_relocations: Span32,
    diagnostic_values: Span32,
    scopes: Span32,
    expression_presentations: Span32,
    statement_presentations: Span32,
    declaration_presentations: Span32,
    expression_payloads: Span32,
    call_syntax: Span32,
    execution_shapes: Span32,
    statements: Span32,
    declarations: Span32,
    lexical_bindings: Span32,
    sources: Span32,
    states: Span32,
    lists: Span32,
}

/// One non-owning view over the runtime facts of a definition. Every
/// variable-width family is a range into one project-wide immutable column;
/// names and authored projections are coordinates in the type store's text
/// authority.
#[derive(Clone, Copy)]
pub(crate) struct PackedDefinitionFactsRef<'a> {
    store: &'a PackedDefinitionFactsStore,
    facts: &'a PackedDefinitionFacts,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PackedDefinitionFactsStore {
    definitions: Box<[PackedDefinitionFacts]>,
    expression_relocations: Box<[KernelExpressionRelocation]>,
    statement_relocations: Box<[StableStatementKey]>,
    diagnostic_values: Box<[KernelExpressionId]>,
    scopes: Box<[PackedScopePresentation]>,
    expression_presentations: Box<[PackedExpressionPresentation]>,
    statement_presentations: Box<[PackedStatementPresentation]>,
    declaration_presentations: Box<[PackedDeclarationPresentation]>,
    expression_payloads: Box<[PackedExpressionPayload]>,
    template_segments: Box<[PackedTextTemplateSegment]>,
    calls: Box<[PackedCallSyntax]>,
    call_occurrences: Box<[StableOccurrenceKey]>,
    call_arguments: Box<[PackedCallArgument]>,
    execution_shapes: Box<[PackedExecutionShape]>,
    execution_fields: Box<[PackedExecutionRecordField]>,
    execution_bindings: Box<[PackedExecutionBlockBinding]>,
    execution_binding_declarations: Box<[KernelDeclarationId]>,
    statements: Box<[PackedStatement]>,
    statement_parameters: Box<[PackedStatementParameter]>,
    statement_children: Box<[KernelStatementChildReference]>,
    declarations: Box<[PackedDeclaration]>,
    lexical_bindings: Box<[PackedLexicalBinding]>,
    sources: Box<[PackedSource]>,
    states: Box<[PackedState]>,
    lists: Box<[PackedList]>,
    text_bytes: Box<[u8]>,
    text_spans: Box<[Span32]>,
    numbers: Box<[ExactNumber]>,
    bits: Box<[Bits]>,
}

#[derive(Debug, Default)]
pub(crate) struct PackedDefinitionFactsStoreBuilder {
    definitions: Vec<PackedDefinitionFacts>,
    expression_relocations: Vec<KernelExpressionRelocation>,
    statement_relocations: Vec<StableStatementKey>,
    diagnostic_values: Vec<KernelExpressionId>,
    scopes: Vec<PackedScopePresentation>,
    expression_presentations: Vec<PackedExpressionPresentation>,
    statement_presentations: Vec<PackedStatementPresentation>,
    declaration_presentations: Vec<PackedDeclarationPresentation>,
    expression_payloads: Vec<PackedExpressionPayload>,
    template_segments: Vec<PackedTextTemplateSegment>,
    calls: Vec<PackedCallSyntax>,
    call_occurrences: Vec<StableOccurrenceKey>,
    call_arguments: Vec<PackedCallArgument>,
    execution_shapes: Vec<PackedExecutionShape>,
    execution_fields: Vec<PackedExecutionRecordField>,
    execution_bindings: Vec<PackedExecutionBlockBinding>,
    execution_binding_declarations: Vec<KernelDeclarationId>,
    statements: Vec<PackedStatement>,
    statement_parameters: Vec<PackedStatementParameter>,
    statement_children: Vec<KernelStatementChildReference>,
    declarations: Vec<PackedDeclaration>,
    lexical_bindings: Vec<PackedLexicalBinding>,
    sources: Vec<PackedSource>,
    states: Vec<PackedState>,
    lists: Vec<PackedList>,
    text_bytes: Vec<u8>,
    text_spans: Vec<Span32>,
    numbers: Vec<ExactNumber>,
    bits: Vec<Bits>,
}

impl PackedDefinitionFactsStoreBuilder {
    pub(crate) fn with_capacity(definitions: usize) -> Self {
        Self {
            definitions: Vec::with_capacity(definitions),
            ..Self::default()
        }
    }

    pub(crate) fn definition_count(&self) -> usize {
        self.definitions.len()
    }

    pub(crate) fn push_empty(&mut self) {
        self.definitions.push(PackedDefinitionFacts::default());
    }

    pub(crate) fn push(
        &mut self,
        text: &ProjectTextSnapshot,
        facts: &KernelDefinitionFactsInput,
    ) -> Result<(), KernelOwnerBuildError> {
        let owner = self.definitions.len();
        let expression_relocations = append_span(
            &mut self.expression_relocations,
            facts.relocations.expressions.iter().cloned().map(Ok),
            "expression relocation",
        )?;
        let statement_relocations = append_span(
            &mut self.statement_relocations,
            facts.relocations.statements.iter().cloned().map(Ok),
            "statement relocation",
        )?;
        let diagnostic_values = append_span(
            &mut self.diagnostic_values,
            facts.diagnostic_values.iter().copied().map(Ok),
            "diagnostic value",
        )?;
        let scopes = append_span(
            &mut self.scopes,
            facts.presentation.scopes.iter().map(|scope| {
                Ok(PackedScopePresentation {
                    id: scope.id,
                    parent: scope.parent,
                    owner: scope.owner,
                    kind: scope.kind,
                    origin: scope.origin,
                    span: PackedSourceSpan::pack(scope.span, "scope span")?,
                })
            }),
            "scope",
        )?;
        let expression_presentations = append_span(
            &mut self.expression_presentations,
            facts.presentation.expressions.iter().map(|expression| {
                Ok(PackedExpressionPresentation {
                    expression: expression.expression,
                    scope: expression.scope,
                    declaration: expression.declaration,
                    declaration_scope: expression.declaration_scope,
                    span: PackedSourceSpan::pack(expression.span, "expression span")?,
                })
            }),
            "expression presentation",
        )?;
        let statement_presentations = append_span(
            &mut self.statement_presentations,
            facts.presentation.statements.iter().map(|statement| {
                Ok(PackedStatementPresentation {
                    statement: statement.statement,
                    scope: statement.scope,
                    body_scope: statement.body_scope,
                    span: PackedSourceSpan::pack(statement.span, "statement span")?,
                })
            }),
            "statement presentation",
        )?;
        let declaration_presentations = append_span(
            &mut self.declaration_presentations,
            facts.presentation.declarations.iter().map(|declaration| {
                Ok(PackedDeclarationPresentation {
                    declaration: declaration.declaration,
                    scope: declaration.scope,
                    body_scope: declaration.body_scope,
                    span: PackedSourceSpan::pack(declaration.span, "declaration span")?,
                })
            }),
            "declaration presentation",
        )?;

        let payload_start = self.expression_payloads.len();
        for payload in &facts.expression_payloads {
            let payload = self.pack_payload(text, payload)?;
            self.expression_payloads.push(payload);
        }
        let expression_payloads = packed_span(
            payload_start,
            self.expression_payloads.len(),
            "expression payload",
        )?;

        let call_start = self.calls.len();
        for call in &facts.call_syntax {
            let occurrence = checked_u32(self.call_occurrences.len(), "call occurrence count")?;
            self.call_occurrences.push(call.occurrence.clone());
            let arguments = append_span(
                &mut self.call_arguments,
                call.arguments.iter().map(|argument| {
                    Ok(PackedCallArgument {
                        ordinal: argument.ordinal,
                        kind: argument.kind,
                        name: lookup_symbol(text, &argument.name, owner, "call argument")?,
                        value: argument.value,
                        span: PackedSourceSpan::pack(argument.span, "call argument span")?,
                    })
                }),
                "call argument",
            )?;
            self.calls.push(PackedCallSyntax {
                expression: call.expression,
                authored_site_digest_v4: call.authored_site_digest_v4,
                function: lookup_symbol(text, &call.function, owner, "call function")?,
                pipe_input: call.pipe_input,
                arguments,
                occurrence,
                pass: call
                    .pass
                    .map(|pass| {
                        Ok(PackedCallPass {
                            value: pass.value,
                            final_clause: pass.final_clause,
                            span: PackedSourceSpan::pack(pass.span, "call pass span")?,
                        })
                    })
                    .transpose()?,
            });
        }
        let call_syntax = packed_span(call_start, self.calls.len(), "call syntax")?;

        let shape_start = self.execution_shapes.len();
        for shape in &facts.execution_shapes {
            let packed = match shape {
                KernelExecutionShapeInput::Conditional { expression, kind } => {
                    PackedExecutionShape::Conditional {
                        expression: *expression,
                        kind: *kind,
                    }
                }
                KernelExecutionShapeInput::Record { expression, fields } => {
                    let fields = append_span(
                        &mut self.execution_fields,
                        fields.iter().map(|field| {
                            Ok(PackedExecutionRecordField {
                                ordinal: field.ordinal,
                                declaration: field.declaration,
                                name: lookup_symbol(
                                    text,
                                    &field.name,
                                    owner,
                                    "execution record field",
                                )?,
                                value: field.value,
                                spread: field.spread,
                                span: PackedSourceSpan::pack(
                                    field.span,
                                    "execution record field span",
                                )?,
                            })
                        }),
                        "execution record field",
                    )?;
                    PackedExecutionShape::Record {
                        expression: *expression,
                        fields,
                    }
                }
                KernelExecutionShapeInput::Block {
                    expression,
                    bindings,
                    result,
                } => {
                    let bindings = append_span(
                        &mut self.execution_bindings,
                        bindings.iter().map(|binding| {
                            Ok(PackedExecutionBlockBinding {
                                ordinal: binding.ordinal,
                                declaration: binding.declaration,
                                value: binding.value,
                                span: PackedSourceSpan::pack(
                                    binding.span,
                                    "execution block binding span",
                                )?,
                            })
                        }),
                        "execution block binding",
                    )?;
                    PackedExecutionShape::Block {
                        expression: *expression,
                        bindings,
                        result: *result,
                    }
                }
                KernelExecutionShapeInput::MatchArm {
                    expression,
                    selector,
                    bindings,
                } => {
                    let bindings = append_span(
                        &mut self.execution_binding_declarations,
                        bindings.iter().copied().map(Ok),
                        "execution match binding",
                    )?;
                    PackedExecutionShape::MatchArm {
                        expression: *expression,
                        selector: *selector,
                        bindings,
                    }
                }
            };
            self.execution_shapes.push(packed);
        }
        let execution_shapes =
            packed_span(shape_start, self.execution_shapes.len(), "execution shape")?;

        let statement_start = self.statements.len();
        for statement in &facts.statements {
            let kind = match &statement.kind {
                KernelStatementKind::Function { name, parameters } => {
                    let mut parameter_declarations =
                        facts.declarations.iter().filter(|declaration| {
                            matches!(
                                declaration.origin,
                                crate::KernelDeclarationOrigin::Parameter {
                                    statement: owner_statement,
                                    ..
                                } if owner_statement == statement.id
                            )
                        });
                    let packed_parameters = append_span(
                        &mut self.statement_parameters,
                        parameters.iter().enumerate().map(|(index, parameter)| {
                            let ordinal = u32::try_from(index).map_err(|_| {
                                KernelOwnerBuildError::new(
                                    "statement parameter count exceeds u32",
                                )
                            })?;
                            if parameter.ordinal != ordinal {
                                return Err(KernelOwnerBuildError::new(format!(
                                    "function statement {} parameter index {index} has noncanonical ordinal {}",
                                    statement.id.0, parameter.ordinal,
                                )));
                            }
                            let declaration = parameter_declarations.next().ok_or_else(|| {
                                KernelOwnerBuildError::new(format!(
                                    "function statement {} parameter ordinal {ordinal} has no declaration",
                                    statement.id.0,
                                ))
                            })?;
                            if declaration.origin
                                != (crate::KernelDeclarationOrigin::Parameter {
                                    statement: statement.id,
                                    ordinal,
                                })
                            {
                                return Err(KernelOwnerBuildError::new(format!(
                                    "function statement {} parameter ordinal {ordinal} has a noncanonical declaration",
                                    statement.id.0
                                )));
                            }
                            let expected_kind = match parameter.kind {
                                crate::KernelParameterKind::Value => {
                                    crate::KernelDeclarationKind::ValueParameter
                                }
                                crate::KernelParameterKind::Out => {
                                    crate::KernelDeclarationKind::OutParameter
                                }
                            };
                            if declaration.kind != expected_kind
                                || declaration.name != parameter.name
                            {
                                return Err(KernelOwnerBuildError::new(format!(
                                    "function statement {} parameter ordinal {ordinal} differs from declaration {}",
                                    statement.id.0,
                                    declaration.id.0,
                                )));
                            }
                            Ok(PackedStatementParameter {
                                declaration: declaration.id,
                                name: lookup_symbol(
                                    text,
                                    &parameter.name,
                                    owner,
                                    "statement parameter",
                                )?,
                                kind: parameter.kind,
                                ordinal: parameter.ordinal,
                                evaluation_scope: parameter.evaluation_scope,
                            })
                        }),
                        "statement parameter",
                    )?;
                    if let Some(declaration) = parameter_declarations.next() {
                        return Err(KernelOwnerBuildError::new(format!(
                            "function statement {} has unexpected parameter declaration {}",
                            statement.id.0, declaration.id.0,
                        )));
                    }
                    PackedStatementKind::Function {
                        name: lookup_symbol(text, name, owner, "function statement")?,
                        parameters: packed_parameters,
                    }
                }
                KernelStatementKind::Field { name } => PackedStatementKind::Field {
                    name: lookup_symbol(text, name, owner, "field statement")?,
                },
                KernelStatementKind::Source { field, event } => PackedStatementKind::Source {
                    field: lookup_optional_symbol(text, field.as_deref(), owner, "SOURCE field")?,
                    event: lookup_optional_symbol(text, event.as_deref(), owner, "SOURCE event")?,
                },
                KernelStatementKind::Hold { field, name } => PackedStatementKind::Hold {
                    field: lookup_optional_symbol(text, field.as_deref(), owner, "HOLD field")?,
                    name: lookup_optional_symbol(text, name.as_deref(), owner, "HOLD name")?,
                },
                KernelStatementKind::List { field, capacity } => PackedStatementKind::List {
                    field: lookup_optional_symbol(text, field.as_deref(), owner, "LIST field")?,
                    capacity: capacity
                        .map(|capacity| checked_u32(capacity, "LIST capacity"))
                        .transpose()?,
                },
                KernelStatementKind::Block => PackedStatementKind::Block,
                KernelStatementKind::Spread => PackedStatementKind::Spread,
                KernelStatementKind::Expression => PackedStatementKind::Expression,
            };
            let children = append_span(
                &mut self.statement_children,
                statement.children.iter().copied().map(Ok),
                "statement child",
            )?;
            self.statements.push(PackedStatement {
                id: statement.id,
                kind,
                value: statement.value,
                value_use: statement.value_use,
                children,
            });
        }
        let statements = packed_span(statement_start, self.statements.len(), "statement")?;

        let declarations = append_span(
            &mut self.declarations,
            facts.declarations.iter().map(|declaration| {
                Ok(PackedDeclaration {
                    id: declaration.id,
                    origin: declaration.origin,
                    name: lookup_symbol(text, &declaration.name, owner, "declaration")?,
                    kind: declaration.kind,
                    value: declaration.value,
                })
            }),
            "declaration",
        )?;
        let lexical_bindings = append_span(
            &mut self.lexical_bindings,
            facts.lexical_bindings.iter().map(|binding| {
                Ok(PackedLexicalBinding {
                    expression: binding.expression,
                    target: binding.target,
                    projection: lookup_path(
                        text,
                        &binding.projection,
                        owner,
                        "lexical projection",
                    )?,
                    access: binding.access,
                })
            }),
            "lexical binding",
        )?;
        let sources = append_span(
            &mut self.sources,
            facts.sources.iter().map(|source| {
                Ok(PackedSource {
                    id: source.id,
                    declaration: source.declaration,
                    statement: source.statement,
                    expression: source.expression,
                    projection: lookup_path(text, &source.projection, owner, "SOURCE projection")?,
                    interval_ms: source.interval_ms,
                })
            }),
            "SOURCE",
        )?;
        let states = append_span(
            &mut self.states,
            facts.states.iter().map(|state| {
                Ok(PackedState {
                    id: state.id,
                    binding_declaration: state.binding_declaration,
                    declaration: state.declaration,
                    statement: state.statement,
                    expression: state.expression,
                    initial: state.initial,
                    projection: lookup_path(text, &state.projection, owner, "state projection")?,
                    synthetic_path: state.synthetic_path,
                    kind: state.kind,
                })
            }),
            "state",
        )?;
        let lists = append_span(
            &mut self.lists,
            facts.lists.iter().map(|list| {
                Ok(PackedList {
                    id: list.id,
                    declaration: list.declaration,
                    statement: list.statement,
                    producer: list.producer,
                    projection: lookup_path(text, &list.projection, owner, "LIST projection")?,
                    capacity: list
                        .capacity
                        .map(|capacity| checked_u32(capacity, "LIST capacity"))
                        .transpose()?,
                    key_policy: list.key_policy,
                })
            }),
            "LIST",
        )?;
        self.definitions.push(PackedDefinitionFacts {
            linkage: facts.linkage,
            containing_scope: facts.presentation.containing_scope,
            expression_relocations,
            statement_relocations,
            diagnostic_values,
            scopes,
            expression_presentations,
            statement_presentations,
            declaration_presentations,
            expression_payloads,
            call_syntax,
            execution_shapes,
            statements,
            declarations,
            lexical_bindings,
            sources,
            states,
            lists,
        });
        Ok(())
    }

    fn pack_payload(
        &mut self,
        text: &ProjectTextSnapshot,
        payload: &KernelExpressionSemanticPayload,
    ) -> Result<PackedExpressionPayload, KernelOwnerBuildError> {
        let owner = self.definitions.len();
        Ok(match payload {
            KernelExpressionSemanticPayload::None => PackedExpressionPayload::None,
            KernelExpressionSemanticPayload::Text(value) => {
                PackedExpressionPayload::Text(self.push_text(value)?)
            }
            KernelExpressionSemanticPayload::TextTemplate(segments) => {
                let start = self.template_segments.len();
                for segment in segments {
                    let segment = match segment {
                        KernelTextTemplateSegment::Static(value) => {
                            PackedTextTemplateSegment::Static(self.push_text(value)?)
                        }
                        KernelTextTemplateSegment::Dynamic(ordinal) => {
                            PackedTextTemplateSegment::Dynamic(*ordinal)
                        }
                    };
                    self.template_segments.push(segment);
                }
                PackedExpressionPayload::TextTemplate(packed_span(
                    start,
                    self.template_segments.len(),
                    "text template segment",
                )?)
            }
            KernelExpressionSemanticPayload::Number(value) => {
                let index = checked_u32(self.numbers.len(), "exact number count")?;
                self.numbers.push(value.clone());
                PackedExpressionPayload::Number(index)
            }
            KernelExpressionSemanticPayload::Byte(value) => PackedExpressionPayload::Byte(*value),
            KernelExpressionSemanticPayload::Bits(value) => {
                let index = checked_u32(self.bits.len(), "BITS literal count")?;
                self.bits.push(value.clone());
                PackedExpressionPayload::Bits(index)
            }
            KernelExpressionSemanticPayload::HoldName(name) => PackedExpressionPayload::HoldName(
                lookup_symbol(text, name, owner, "HOLD payload name")?,
            ),
            KernelExpressionSemanticPayload::MatchPattern(pattern) => {
                PackedExpressionPayload::MatchPattern(self.pack_match_pattern(text, pattern)?)
            }
            KernelExpressionSemanticPayload::Delimiter => PackedExpressionPayload::Delimiter,
            KernelExpressionSemanticPayload::LexicalPath(path) => {
                PackedExpressionPayload::LexicalPath(lookup_path(
                    text,
                    path,
                    owner,
                    "lexical payload path",
                )?)
            }
            KernelExpressionSemanticPayload::Invalid(tokens) => {
                let start = self.text_spans.len();
                for token in tokens {
                    let token = self.push_text(token)?;
                    self.text_spans.push(token);
                }
                PackedExpressionPayload::Invalid(packed_span(
                    start,
                    self.text_spans.len(),
                    "invalid token",
                )?)
            }
        })
    }

    fn pack_match_pattern(
        &mut self,
        text: &ProjectTextSnapshot,
        pattern: &KernelMatchPatternPayload,
    ) -> Result<PackedMatchPatternPayload, KernelOwnerBuildError> {
        let owner = self.definitions.len();
        Ok(match pattern {
            KernelMatchPatternPayload::Wildcard => PackedMatchPatternPayload::Wildcard,
            KernelMatchPatternPayload::Number(value) => {
                let index = checked_u32(self.numbers.len(), "pattern number count")?;
                self.numbers.push(value.clone());
                PackedMatchPatternPayload::Number(index)
            }
            KernelMatchPatternPayload::Text(value) => {
                PackedMatchPatternPayload::Text(self.push_text(value)?)
            }
            KernelMatchPatternPayload::Tag { name, fields } => PackedMatchPatternPayload::Tag {
                name: lookup_symbol(text, name, owner, "pattern tag")?,
                fields: lookup_path(text, fields, owner, "pattern fields")?,
            },
            KernelMatchPatternPayload::Binding(name) => PackedMatchPatternPayload::Binding(
                lookup_symbol(text, name, owner, "pattern binding")?,
            ),
            KernelMatchPatternPayload::Bits(value) => {
                let index = checked_u32(self.bits.len(), "pattern BITS count")?;
                self.bits.push(value.clone());
                PackedMatchPatternPayload::Bits(index)
            }
            KernelMatchPatternPayload::Invalid => PackedMatchPatternPayload::Invalid,
        })
    }

    fn push_text(&mut self, value: &str) -> Result<Span32, KernelOwnerBuildError> {
        let start = self.text_bytes.len();
        self.text_bytes.extend_from_slice(value.as_bytes());
        packed_span(start, self.text_bytes.len(), "literal text bytes")
    }

    pub(crate) fn finish(self) -> PackedDefinitionFactsStore {
        PackedDefinitionFactsStore {
            definitions: self.definitions.into_boxed_slice(),
            expression_relocations: self.expression_relocations.into_boxed_slice(),
            statement_relocations: self.statement_relocations.into_boxed_slice(),
            diagnostic_values: self.diagnostic_values.into_boxed_slice(),
            scopes: self.scopes.into_boxed_slice(),
            expression_presentations: self.expression_presentations.into_boxed_slice(),
            statement_presentations: self.statement_presentations.into_boxed_slice(),
            declaration_presentations: self.declaration_presentations.into_boxed_slice(),
            expression_payloads: self.expression_payloads.into_boxed_slice(),
            template_segments: self.template_segments.into_boxed_slice(),
            calls: self.calls.into_boxed_slice(),
            call_occurrences: self.call_occurrences.into_boxed_slice(),
            call_arguments: self.call_arguments.into_boxed_slice(),
            execution_shapes: self.execution_shapes.into_boxed_slice(),
            execution_fields: self.execution_fields.into_boxed_slice(),
            execution_bindings: self.execution_bindings.into_boxed_slice(),
            execution_binding_declarations: self.execution_binding_declarations.into_boxed_slice(),
            statements: self.statements.into_boxed_slice(),
            statement_parameters: self.statement_parameters.into_boxed_slice(),
            statement_children: self.statement_children.into_boxed_slice(),
            declarations: self.declarations.into_boxed_slice(),
            lexical_bindings: self.lexical_bindings.into_boxed_slice(),
            sources: self.sources.into_boxed_slice(),
            states: self.states.into_boxed_slice(),
            lists: self.lists.into_boxed_slice(),
            text_bytes: self.text_bytes.into_boxed_slice(),
            text_spans: self.text_spans.into_boxed_slice(),
            numbers: self.numbers.into_boxed_slice(),
            bits: self.bits.into_boxed_slice(),
        }
    }
}

impl PackedDefinitionFactsStore {
    pub(crate) fn definition_count(&self) -> usize {
        self.definitions.len()
    }

    pub(crate) fn definition(&self, owner: KernelOwnerId) -> Option<PackedDefinitionFactsRef<'_>> {
        self.definitions
            .get(owner.0 as usize)
            .map(|facts| PackedDefinitionFactsRef { store: self, facts })
    }

    pub(crate) fn text(&self, span: Span32) -> Option<&str> {
        std::str::from_utf8(span.get(&self.text_bytes)?).ok()
    }

    pub(crate) fn number(&self, index: u32) -> Option<&ExactNumber> {
        self.numbers.get(index as usize)
    }

    pub(crate) fn bits(&self, index: u32) -> Option<&Bits> {
        self.bits.get(index as usize)
    }
}

impl<'a> PackedDefinitionFactsRef<'a> {
    pub(crate) const fn linkage(self) -> KernelDefinitionLinkage {
        self.facts.linkage
    }

    pub(crate) const fn containing_scope(self) -> KernelScopeReference {
        self.facts.containing_scope
    }

    pub(crate) fn expression_relocations(self) -> &'a [KernelExpressionRelocation] {
        self.facts
            .expression_relocations
            .get(&self.store.expression_relocations)
            .expect("sealed expression-relocation span remains valid")
    }

    pub(crate) fn statement_relocations(self) -> &'a [StableStatementKey] {
        self.facts
            .statement_relocations
            .get(&self.store.statement_relocations)
            .expect("sealed statement-relocation span remains valid")
    }

    pub(crate) fn diagnostic_values(self) -> &'a [KernelExpressionId] {
        self.facts
            .diagnostic_values
            .get(&self.store.diagnostic_values)
            .expect("sealed diagnostic-value span remains valid")
    }

    pub(crate) fn scopes(self) -> &'a [PackedScopePresentation] {
        self.facts
            .scopes
            .get(&self.store.scopes)
            .expect("sealed scope span remains valid")
    }

    pub(crate) fn expression_presentations(self) -> &'a [PackedExpressionPresentation] {
        self.facts
            .expression_presentations
            .get(&self.store.expression_presentations)
            .expect("sealed expression-presentation span remains valid")
    }

    pub(crate) fn statement_presentations(self) -> &'a [PackedStatementPresentation] {
        self.facts
            .statement_presentations
            .get(&self.store.statement_presentations)
            .expect("sealed statement-presentation span remains valid")
    }

    pub(crate) fn declaration_presentations(self) -> &'a [PackedDeclarationPresentation] {
        self.facts
            .declaration_presentations
            .get(&self.store.declaration_presentations)
            .expect("sealed declaration-presentation span remains valid")
    }

    pub(crate) fn expression_payloads(self) -> &'a [PackedExpressionPayload] {
        self.facts
            .expression_payloads
            .get(&self.store.expression_payloads)
            .expect("sealed expression-payload span remains valid")
    }

    pub(crate) fn calls(self) -> &'a [PackedCallSyntax] {
        self.facts
            .call_syntax
            .get(&self.store.calls)
            .expect("sealed call-syntax span remains valid")
    }

    pub(crate) fn call_arguments(self, call: &PackedCallSyntax) -> &'a [PackedCallArgument] {
        call.arguments
            .get(&self.store.call_arguments)
            .expect("sealed call-argument span remains valid")
    }

    pub(crate) fn call_occurrence(self, call: &PackedCallSyntax) -> &'a StableOccurrenceKey {
        self.store
            .call_occurrences
            .get(call.occurrence as usize)
            .expect("sealed call occurrence index remains valid")
    }

    pub(crate) fn execution_shapes(self) -> &'a [PackedExecutionShape] {
        self.facts
            .execution_shapes
            .get(&self.store.execution_shapes)
            .expect("sealed execution-shape span remains valid")
    }

    pub(crate) fn execution_fields(
        self,
        shape: &PackedExecutionShape,
    ) -> Option<&'a [PackedExecutionRecordField]> {
        match shape {
            PackedExecutionShape::Record { fields, .. } => fields.get(&self.store.execution_fields),
            _ => None,
        }
    }

    pub(crate) fn execution_bindings(
        self,
        shape: &PackedExecutionShape,
    ) -> Option<&'a [PackedExecutionBlockBinding]> {
        match shape {
            PackedExecutionShape::Block { bindings, .. } => {
                bindings.get(&self.store.execution_bindings)
            }
            _ => None,
        }
    }

    pub(crate) fn execution_match_bindings(
        self,
        shape: &PackedExecutionShape,
    ) -> Option<&'a [KernelDeclarationId]> {
        match shape {
            PackedExecutionShape::MatchArm { bindings, .. } => {
                bindings.get(&self.store.execution_binding_declarations)
            }
            _ => None,
        }
    }

    pub(crate) fn statements(self) -> &'a [PackedStatement] {
        self.facts
            .statements
            .get(&self.store.statements)
            .expect("sealed statement span remains valid")
    }

    pub(crate) fn statement_parameters(
        self,
        statement: &PackedStatement,
    ) -> Option<&'a [PackedStatementParameter]> {
        match statement.kind {
            PackedStatementKind::Function { parameters, .. } => {
                parameters.get(&self.store.statement_parameters)
            }
            _ => None,
        }
    }

    pub(crate) fn statement_children(
        self,
        statement: &PackedStatement,
    ) -> &'a [KernelStatementChildReference] {
        statement
            .children
            .get(&self.store.statement_children)
            .expect("sealed statement-child span remains valid")
    }

    pub(crate) fn declarations(self) -> &'a [PackedDeclaration] {
        self.facts
            .declarations
            .get(&self.store.declarations)
            .expect("sealed declaration span remains valid")
    }

    pub(crate) fn lexical_bindings(self) -> &'a [PackedLexicalBinding] {
        self.facts
            .lexical_bindings
            .get(&self.store.lexical_bindings)
            .expect("sealed lexical-binding span remains valid")
    }

    pub(crate) fn sources(self) -> &'a [PackedSource] {
        self.facts
            .sources
            .get(&self.store.sources)
            .expect("sealed SOURCE span remains valid")
    }

    pub(crate) fn states(self) -> &'a [PackedState] {
        self.facts
            .states
            .get(&self.store.states)
            .expect("sealed state span remains valid")
    }

    pub(crate) fn lists(self) -> &'a [PackedList] {
        self.facts
            .lists
            .get(&self.store.lists)
            .expect("sealed LIST span remains valid")
    }

    pub(crate) fn template_segments(
        self,
        payload: PackedExpressionPayload,
    ) -> Option<&'a [PackedTextTemplateSegment]> {
        match payload {
            PackedExpressionPayload::TextTemplate(segments) => {
                segments.get(&self.store.template_segments)
            }
            _ => None,
        }
    }

    pub(crate) fn invalid_tokens(
        self,
        payload: PackedExpressionPayload,
    ) -> Option<impl ExactSizeIterator<Item = &'a str> + 'a> {
        let spans = match payload {
            PackedExpressionPayload::Invalid(spans) => spans.get(&self.store.text_spans)?,
            _ => return None,
        };
        Some(spans.iter().map(|span| {
            self.store
                .text(*span)
                .expect("sealed invalid-token bytes remain UTF-8")
        }))
    }

    pub(crate) fn literal_text(self, span: Span32) -> &'a str {
        self.store
            .text(span)
            .expect("sealed literal bytes remain UTF-8")
    }

    pub(crate) fn number(self, index: u32) -> &'a ExactNumber {
        self.store
            .number(index)
            .expect("sealed exact-number index remains valid")
    }

    pub(crate) fn bits(self, index: u32) -> &'a Bits {
        self.store
            .bits(index)
            .expect("sealed BITS index remains valid")
    }
}

fn checked_u32(value: usize, label: &str) -> Result<u32, KernelOwnerBuildError> {
    u32::try_from(value)
        .map_err(|_| KernelOwnerBuildError::new(format!("kernel packed {label} exceeds u32")))
}

fn packed_span(start: usize, end: usize, label: &str) -> Result<Span32, KernelOwnerBuildError> {
    Span32::from_bounds(start, end, label)
        .map_err(|error| KernelOwnerBuildError::new(error.to_string()))
}

fn append_span<T>(
    column: &mut Vec<T>,
    rows: impl IntoIterator<Item = Result<T, KernelOwnerBuildError>>,
    label: &str,
) -> Result<Span32, KernelOwnerBuildError> {
    let start = column.len();
    for row in rows {
        column.push(row?);
    }
    packed_span(start, column.len(), label)
}

fn lookup_symbol(
    text: &ProjectTextSnapshot,
    value: &str,
    owner: usize,
    label: &str,
) -> Result<SymbolId, KernelOwnerBuildError> {
    text.lookup_symbol(value).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel definition {owner} {label} `{value}` is absent from the project text authority"
        ))
    })
}

fn lookup_optional_symbol(
    text: &ProjectTextSnapshot,
    value: Option<&str>,
    owner: usize,
    label: &str,
) -> Result<Option<SymbolId>, KernelOwnerBuildError> {
    value
        .map(|value| lookup_symbol(text, value, owner, label))
        .transpose()
}

fn lookup_path(
    text: &ProjectTextSnapshot,
    value: &[Box<str>],
    owner: usize,
    label: &str,
) -> Result<PathId, KernelOwnerBuildError> {
    text.lookup_path(value.iter().map(Box::as_ref))
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "kernel definition {owner} {label} is absent from the project text authority"
            ))
        })
}
