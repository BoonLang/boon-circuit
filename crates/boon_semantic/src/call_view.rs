//! Allocation-free borrowed access to checked call topology.
//!
//! Runtime compilation reads the packed kernel authority directly. Rich rows
//! remain available to legacy/editor paths and as a debug parity oracle, but
//! neither representation is copied into another checked-call DTO here.

use boon_checked::{
    CheckedCall, CheckedCallContext, CheckedCallContextKind, CheckedCallEntry, CheckedCallId,
    CheckedCallableKind, CheckedCallableSignature, CheckedContextBinding,
    CheckedContextTypeSubstitution, CheckedContextualOperation, CheckedDeclaration,
    CheckedDeclarationKind, CheckedEffectSummary, CheckedEvaluationScope, CheckedExprId,
    CheckedExternalDeclarationIdentityV1, CheckedIntrinsicV1, CheckedParameter,
    CheckedParameterDefault, CheckedParameterKind, CheckedParameterRequirement,
    CheckedProgramFields, CheckedSpan, CheckedStatementId, ContextFormalId, DeclId, FlowMode,
    FlowType, LexicalScopeId, ProgramRole, Type, TypeVar,
};
use boon_compiler_kernel::{
    KernelCallableSchemeRef, KernelPackedFlowRef, KernelPackedTypeRef,
    KernelSemanticCallContextRef, KernelSemanticCallEntryRef, KernelSemanticCallRef,
    KernelSemanticCallableContextIter, KernelSemanticCallableContextRef,
    KernelSemanticCallableParameterIter, KernelSemanticCallableParameterRef,
    KernelSemanticContextFormalRef, KernelSemanticInputV1, KernelSemanticParameterRequirementRef,
    KernelSemanticResolvedDeclarationRef, KernelSemanticTypeMaterializer,
};

enum CallCatalogSource<'a> {
    Rich {
        program: &'a CheckedProgramFields,
        call_by_id: Box<[Option<usize>]>,
        callable_by_declaration: Box<[Option<usize>]>,
        declaration_by_id: Box<[Option<usize>]>,
    },
    Packed(&'a KernelSemanticInputV1),
}

#[derive(Clone, Copy)]
pub(crate) enum CallRef<'a> {
    Rich(&'a CheckedCall),
    Packed(KernelSemanticCallRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum CallEntryRef<'a> {
    Input {
        parameter: CallableParameterRef<'a>,
        value: CheckedExprId,
        from_pipe: bool,
        evaluation_scope: CheckedEvaluationScope,
    },
    FreshOut {
        parameter: CallableParameterRef<'a>,
        output: DeclId,
        scope: LexicalScopeId,
    },
    ForwardOut {
        parameter: CallableParameterRef<'a>,
        target: DeclId,
        target_name: &'a str,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum CallableRef<'a> {
    Rich(&'a CheckedCallableSignature),
    Packed(KernelCallableSchemeRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum CallableParameterRef<'a> {
    Rich(&'a CheckedParameter),
    Packed(KernelSemanticCallableParameterRef<'a>),
}

pub(crate) enum CallableParameterIter<'a> {
    Rich(std::slice::Iter<'a, CheckedParameter>),
    Packed(KernelSemanticCallableParameterIter<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum CallableContextRef<'a> {
    Rich(&'a boon_checked::CheckedCallableContext),
    Packed(KernelSemanticCallableContextRef<'a>),
}

pub(crate) enum CallableContextIter<'a> {
    Rich(std::slice::Iter<'a, boon_checked::CheckedCallableContext>),
    Packed(KernelSemanticCallableContextIter<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum ContextFormalRef<'a> {
    Rich(&'a boon_checked::CheckedContextFormal),
    Packed(KernelSemanticContextFormalRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum DeclarationRef<'a> {
    Rich(&'a CheckedDeclaration),
    Packed(KernelSemanticResolvedDeclarationRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum ParameterRequirementRef<'a> {
    Rich(&'a CheckedParameterRequirement),
    Packed(KernelSemanticParameterRequirementRef<'a>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CallContextRef {
    pub declaration: DeclId,
    pub signature: usize,
    pub scope: LexicalScopeId,
}

#[derive(Clone, Copy)]
pub(crate) enum CallTypeRef<'a> {
    Rich(&'a Type),
    Packed(KernelPackedTypeRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum CallFlowRef<'a> {
    Rich(&'a FlowType),
    Packed(KernelPackedFlowRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) struct CallSubstitutionRef<'a> {
    pub variable: TypeVar,
    pub value: CallTypeRef<'a>,
}

#[derive(Clone, Copy)]
pub(crate) struct CallContextSubstitutionRef<'a> {
    pub formal: ContextFormalId,
    pub variable: TypeVar,
    pub value: CallTypeRef<'a>,
}

/// One phase-scoped projector for the rich types that a durable semantic row
/// must still own. Topology consumers stay on borrowed packed views; this
/// projector is never constructed per call or per type.
pub(crate) enum CallTypeMaterializer<'a> {
    Rich,
    Packed(KernelSemanticTypeMaterializer<'a>),
}

pub(crate) struct CallTypeFacts {
    pub(crate) result: FlowType,
    pub(crate) type_substitutions: Vec<boon_checked::CheckedTypeSubstitution>,
    pub(crate) contextual_substitutions: Vec<CheckedContextTypeSubstitution>,
}

pub(crate) struct CallableTypeFacts {
    pub(crate) result: FlowType,
    pub(crate) parameters: Box<[FlowType]>,
    pub(crate) contexts: Box<[FlowType]>,
    pub(crate) context_formal: Option<FlowType>,
}

/// The single rich-type ownership bridge for one semantic elaboration.
///
/// Packed topology remains borrowed. Only values that the current durable
/// semantic schema must own are projected, once, and then moved into the final
/// `SemanticCall` rows after OUT/contextual construction has borrowed them.
pub(crate) struct CallTypeCatalog {
    rows: Vec<Option<CallTypeFacts>>,
    callables: Vec<Option<CallableTypeFacts>>,
    remaining: usize,
    remaining_callables: usize,
}

pub(crate) struct CallCatalog<'a> {
    source: CallCatalogSource<'a>,
}

pub(crate) struct CallIter<'catalog, 'program> {
    catalog: &'catalog CallCatalog<'program>,
    next: usize,
}

pub(crate) struct CallableIter<'catalog, 'program> {
    catalog: &'catalog CallCatalog<'program>,
    next: usize,
}

pub(crate) struct CallEntryIter<'catalog, 'program> {
    catalog: &'catalog CallCatalog<'program>,
    call: CallRef<'program>,
    next: usize,
}

pub(crate) struct CallContextIter<'catalog, 'program> {
    catalog: &'catalog CallCatalog<'program>,
    call: CallRef<'program>,
    next: usize,
}

#[derive(Clone, Copy)]
enum CallContextSubstitutionSource<'a> {
    Rich(&'a [CheckedContextTypeSubstitution]),
    Packed {
        call: KernelSemanticCallRef<'a>,
        formal: ContextFormalId,
        scheme: KernelPackedTypeRef<'a>,
    },
    Empty,
}

pub(crate) struct CallContextSubstitutionIter<'a> {
    source: CallContextSubstitutionSource<'a>,
    next: usize,
}

impl<'a> CallCatalog<'a> {
    pub(crate) fn rich(program: &'a CheckedProgramFields) -> Result<Self, String> {
        let catalog = Self {
            source: CallCatalogSource::Rich {
                program,
                call_by_id: dense_index(&program.calls, |call| call.id.0 as usize, "checked call")?,
                callable_by_declaration: dense_index(
                    &program.callables,
                    |callable| callable.decl_id.0 as usize,
                    "callable declaration",
                )?,
                declaration_by_id: dense_index(
                    &program.declarations,
                    |declaration| declaration.id.0 as usize,
                    "declaration",
                )?,
            },
        };
        catalog.validate()?;
        Ok(catalog)
    }

    pub(crate) fn packed(input: &'a KernelSemanticInputV1) -> Result<Self, String> {
        let catalog = Self {
            source: CallCatalogSource::Packed(input),
        };
        catalog.validate()?;
        Ok(catalog)
    }

    pub(crate) fn len(&self) -> usize {
        match &self.source {
            CallCatalogSource::Rich { program, .. } => program.calls.len(),
            CallCatalogSource::Packed(input) => input.call_count(),
        }
    }

    pub(crate) fn calls(&self) -> CallIter<'_, 'a> {
        CallIter {
            catalog: self,
            next: 0,
        }
    }

    pub(crate) fn type_materializer(&self) -> CallTypeMaterializer<'a> {
        match &self.source {
            CallCatalogSource::Rich { .. } => CallTypeMaterializer::Rich,
            CallCatalogSource::Packed(input) => {
                CallTypeMaterializer::Packed(input.compatibility_type_materializer())
            }
        }
    }

    pub(crate) fn get(&self, id: CheckedCallId) -> Option<CallRef<'a>> {
        match &self.source {
            CallCatalogSource::Rich {
                program,
                call_by_id,
                ..
            } => call_by_id
                .get(id.0 as usize)
                .copied()
                .flatten()
                .and_then(|index| program.calls.get(index))
                .filter(|call| call.id == id)
                .map(CallRef::Rich),
            CallCatalogSource::Packed(input) => input.call(id).map(CallRef::Packed),
        }
    }

    pub(crate) fn callable(&self, id: DeclId) -> Option<CallableRef<'a>> {
        match &self.source {
            CallCatalogSource::Rich { program, .. } => self
                .callable_index(id)
                .and_then(|index| program.callables.get(index))
                .filter(|callable| callable.decl_id == id)
                .map(CallableRef::Rich),
            CallCatalogSource::Packed(input) => input.callable(id).map(CallableRef::Packed),
        }
    }

    pub(crate) fn context_formal(&self, callable: DeclId) -> Option<ContextFormalRef<'a>> {
        match &self.source {
            CallCatalogSource::Rich { program, .. } => {
                let formal = self.callable(callable)?.context_formal_id()?;
                program
                    .context_formals
                    .iter()
                    .find(|row| row.id == formal && row.callable == callable)
                    .map(ContextFormalRef::Rich)
            }
            CallCatalogSource::Packed(input) => input
                .callable(callable)?
                .context_formal()
                .map(ContextFormalRef::Packed),
        }
    }

    pub(crate) fn callable_count(&self) -> usize {
        match &self.source {
            CallCatalogSource::Rich { program, .. } => program.callables.len(),
            CallCatalogSource::Packed(input) => input.callable_count(),
        }
    }

    pub(crate) fn callables(&self) -> CallableIter<'_, 'a> {
        CallableIter {
            catalog: self,
            next: 0,
        }
    }

    pub(crate) fn callable_at(&self, index: usize) -> Option<CallableRef<'a>> {
        match &self.source {
            CallCatalogSource::Rich { program, .. } => {
                program.callables.get(index).map(CallableRef::Rich)
            }
            CallCatalogSource::Packed(input) => input.callable_at(index).map(CallableRef::Packed),
        }
    }

    pub(crate) fn callable_index(&self, id: DeclId) -> Option<usize> {
        match &self.source {
            CallCatalogSource::Rich {
                callable_by_declaration,
                ..
            } => callable_by_declaration
                .get(id.0 as usize)
                .copied()
                .flatten(),
            CallCatalogSource::Packed(input) => input.callable_index(id),
        }
    }

    pub(crate) fn declaration(&self, id: DeclId) -> Option<DeclarationRef<'a>> {
        match &self.source {
            CallCatalogSource::Rich {
                program,
                declaration_by_id,
                ..
            } => declaration_by_id
                .get(id.0 as usize)
                .copied()
                .flatten()
                .and_then(|index| program.declarations.get(index))
                .filter(|declaration| declaration.id == id)
                .map(DeclarationRef::Rich),
            CallCatalogSource::Packed(input) => input.declaration(id).map(DeclarationRef::Packed),
        }
    }

    pub(crate) fn entries(&self, call: CallRef<'a>) -> CallEntryIter<'_, 'a> {
        CallEntryIter {
            catalog: self,
            call,
            next: 0,
        }
    }

    pub(crate) fn contexts(&self, call: CallRef<'a>) -> CallContextIter<'_, 'a> {
        CallContextIter {
            catalog: self,
            call,
            next: 0,
        }
    }

    pub(crate) fn contextual_substitutions(
        &self,
        call: CallRef<'a>,
    ) -> CallContextSubstitutionIter<'a> {
        let source = match call {
            CallRef::Rich(call) => {
                CallContextSubstitutionSource::Rich(&call.contextual_substitutions)
            }
            CallRef::Packed(call) => match &self.source {
                CallCatalogSource::Packed(input) => input
                    .callable(call.callable())
                    .and_then(KernelCallableSchemeRef::context_formal)
                    .map(|formal| CallContextSubstitutionSource::Packed {
                        call,
                        formal: formal.id(),
                        scheme: formal.flow().ty(),
                    })
                    .unwrap_or(CallContextSubstitutionSource::Empty),
                CallCatalogSource::Rich { .. } => CallContextSubstitutionSource::Empty,
            },
        };
        CallContextSubstitutionIter { source, next: 0 }
    }

    fn validate(&self) -> Result<(), String> {
        let mut context_formal_count = 0usize;
        let mut previous_context_formal = None;
        for (index, callable) in self.callables().enumerate() {
            let declaration = callable.declaration();
            if self.callable_index(declaration) != Some(index) {
                return Err(format!(
                    "checked callable {} has a stale catalog index at {index}",
                    declaration.0,
                ));
            }
            for (ordinal, parameter) in callable.parameters().enumerate() {
                if parameter.ordinal() != ordinal {
                    return Err(format!(
                        "checked callable {} parameter {} is not canonical at ordinal {ordinal}",
                        declaration.0,
                        parameter.declaration().0,
                    ));
                }
            }
            let Some(formal_id) = callable.context_formal_id() else {
                continue;
            };
            if previous_context_formal.is_some_and(|previous| previous >= formal_id) {
                return Err(format!(
                    "checked callable {} context formal {} is not strictly ordered",
                    declaration.0, formal_id.0,
                ));
            }
            previous_context_formal = Some(formal_id);
            context_formal_count += 1;
            let formal = self.context_formal(declaration).ok_or_else(|| {
                format!(
                    "checked callable {} references missing context formal {}",
                    declaration.0, formal_id.0,
                )
            })?;
            if formal.id() != formal_id || formal.callable() != declaration {
                return Err(format!(
                    "checked callable {} context formal {} has inconsistent ownership",
                    declaration.0, formal_id.0,
                ));
            }
            if callable.kind() != CheckedCallableKind::User {
                return Err(format!(
                    "non-user checked callable {} owns context formal {}",
                    declaration.0, formal_id.0,
                ));
            }
        }
        let expected_context_formal_count = match &self.source {
            CallCatalogSource::Rich { program, .. } => program.context_formals.len(),
            CallCatalogSource::Packed(input) => input.entity_counts().context_formals,
        };
        if context_formal_count != expected_context_formal_count {
            return Err(format!(
                "checked callable catalog exposes {context_formal_count} context formals for {expected_context_formal_count} checked rows",
            ));
        }
        for (index, call) in self.calls().enumerate() {
            if matches!(&self.source, CallCatalogSource::Packed(_)) {
                let expected = CheckedCallId(
                    u32::try_from(index)
                        .map_err(|_| "packed checked call count exceeds u32".to_owned())?,
                );
                if call.id() != expected {
                    return Err(format!(
                        "packed checked call {} is not dense at index {index}",
                        call.id().0,
                    ));
                }
            }
            let expression = call.expression();
            if !self.has_expression(expression) {
                return Err(format!(
                    "checked call {} references missing expression {}",
                    call.id().0,
                    expression.0,
                ));
            }
            self.callable(call.callable()).ok_or_else(|| {
                format!(
                    "checked call {} references missing callable {}",
                    call.id().0,
                    call.callable().0,
                )
            })?;
            if let Some(owner) = call.owner_callable()
                && self.callable(owner).is_none()
            {
                return Err(format!(
                    "checked call {} references missing owner callable {}",
                    call.id().0,
                    owner.0,
                ));
            }
            for ordinal in 0..call.entry_count() {
                self.entry_at(call, ordinal)?;
            }
            for ordinal in 0..call.context_count() {
                self.context_at(call, ordinal)?;
            }
            if let CheckedContextBinding::Explicit { value, .. } = call.context_binding()
                && !self.has_expression(value)
            {
                return Err(format!(
                    "checked call {} PASS binding references missing expression {}",
                    call.id().0,
                    value.0,
                ));
            }
        }
        Ok(())
    }

    fn has_expression(&self, expression: CheckedExprId) -> bool {
        match &self.source {
            CallCatalogSource::Rich { program, .. } => program
                .expressions
                .get(expression.0 as usize)
                .is_some_and(|candidate| candidate.id == expression),
            CallCatalogSource::Packed(input) => input.expression(expression).is_some(),
        }
    }

    fn has_scope(&self, scope: LexicalScopeId) -> bool {
        match &self.source {
            CallCatalogSource::Rich { program, .. } => program
                .scopes
                .get(scope.0 as usize)
                .is_some_and(|candidate| candidate.id == scope),
            CallCatalogSource::Packed(input) => input.scope(scope).is_some(),
        }
    }

    fn entry_at(&self, call: CallRef<'a>, ordinal: usize) -> Result<CallEntryRef<'a>, String> {
        let callable = self.callable(call.callable()).ok_or_else(|| {
            format!(
                "checked call {} references missing callable {}",
                call.id().0,
                call.callable().0,
            )
        })?;
        match call {
            CallRef::Rich(call) => {
                let entry = call
                    .entries
                    .get(ordinal)
                    .ok_or_else(|| format!("checked call {} has no entry {ordinal}", call.id.0))?;
                let formal = rich_entry_formal(entry);
                let parameter = callable
                    .parameters()
                    .find(|parameter| parameter.declaration() == formal)
                    .ok_or_else(|| {
                        format!(
                            "checked call {} entry {ordinal} references missing formal {}",
                            call.id.0, formal.0,
                        )
                    })?;
                self.project_rich_entry(call.id, ordinal, entry, parameter)
            }
            CallRef::Packed(call) => {
                let entry = call.entry(ordinal).ok_or_else(|| {
                    format!("packed checked call {} has no entry {ordinal}", call.id().0)
                })?;
                let parameter_ordinal = packed_entry_parameter_ordinal(entry) as usize;
                let parameter = callable.parameter(parameter_ordinal).ok_or_else(|| {
                    format!(
                        "packed checked call {} entry {ordinal} references missing parameter ordinal {parameter_ordinal}",
                        call.id().0,
                    )
                })?;
                if parameter.ordinal() != parameter_ordinal {
                    return Err(format!(
                        "packed checked call {} entry {ordinal} resolves parameter ordinal {parameter_ordinal} to stale ordinal {}",
                        call.id().0,
                        parameter.ordinal(),
                    ));
                }
                self.project_packed_entry(call.id(), ordinal, entry, parameter)
            }
        }
    }

    fn project_rich_entry(
        &self,
        call: CheckedCallId,
        ordinal: usize,
        entry: &'a CheckedCallEntry,
        parameter: CallableParameterRef<'a>,
    ) -> Result<CallEntryRef<'a>, String> {
        match entry {
            CheckedCallEntry::Input {
                name,
                value,
                from_pipe,
                evaluation_scope,
                ..
            } => {
                validate_parameter(call, ordinal, parameter, name, CheckedParameterKind::Value)?;
                if parameter.evaluation_scope() != *evaluation_scope {
                    return Err(format!(
                        "checked call {} input {ordinal} has stale evaluation scope",
                        call.0,
                    ));
                }
                Ok(CallEntryRef::Input {
                    parameter,
                    value: *value,
                    from_pipe: *from_pipe,
                    evaluation_scope: *evaluation_scope,
                })
            }
            CheckedCallEntry::FreshOut {
                name,
                output,
                scope_id,
                ..
            } => {
                validate_parameter(call, ordinal, parameter, name, CheckedParameterKind::Out)?;
                self.declaration(*output).ok_or_else(|| {
                    format!(
                        "checked call {} FreshOut {ordinal} references missing declaration {}",
                        call.0, output.0,
                    )
                })?;
                self.require_scope(call, *scope_id)?;
                Ok(CallEntryRef::FreshOut {
                    parameter,
                    output: *output,
                    scope: *scope_id,
                })
            }
            CheckedCallEntry::ForwardOut {
                name,
                target,
                target_name,
                ..
            } => {
                validate_parameter(call, ordinal, parameter, name, CheckedParameterKind::Out)?;
                let declaration = self.declaration(*target).ok_or_else(|| {
                    format!(
                        "checked call {} ForwardOut {ordinal} references missing declaration {}",
                        call.0, target.0,
                    )
                })?;
                if declaration.name() != target_name {
                    return Err(format!(
                        "checked call {} ForwardOut {ordinal} has stale target name `{target_name}`",
                        call.0,
                    ));
                }
                Ok(CallEntryRef::ForwardOut {
                    parameter,
                    target: *target,
                    target_name,
                })
            }
        }
    }

    fn project_packed_entry(
        &self,
        call: CheckedCallId,
        ordinal: usize,
        entry: KernelSemanticCallEntryRef,
        parameter: CallableParameterRef<'a>,
    ) -> Result<CallEntryRef<'a>, String> {
        match entry {
            KernelSemanticCallEntryRef::Input {
                value, from_pipe, ..
            } => {
                validate_parameter(
                    call,
                    ordinal,
                    parameter,
                    parameter.name(),
                    CheckedParameterKind::Value,
                )?;
                Ok(CallEntryRef::Input {
                    parameter,
                    value,
                    from_pipe,
                    evaluation_scope: parameter.evaluation_scope(),
                })
            }
            KernelSemanticCallEntryRef::FreshOut { output, scope, .. } => {
                validate_parameter(
                    call,
                    ordinal,
                    parameter,
                    parameter.name(),
                    CheckedParameterKind::Out,
                )?;
                self.declaration(output).ok_or_else(|| {
                    format!(
                        "packed checked call {} FreshOut {ordinal} references missing declaration {}",
                        call.0, output.0,
                    )
                })?;
                self.require_scope(call, scope)?;
                Ok(CallEntryRef::FreshOut {
                    parameter,
                    output,
                    scope,
                })
            }
            KernelSemanticCallEntryRef::ForwardOut { target, .. } => {
                validate_parameter(
                    call,
                    ordinal,
                    parameter,
                    parameter.name(),
                    CheckedParameterKind::Out,
                )?;
                let declaration = self.declaration(target).ok_or_else(|| {
                    format!(
                        "packed checked call {} ForwardOut {ordinal} references missing declaration {}",
                        call.0, target.0,
                    )
                })?;
                Ok(CallEntryRef::ForwardOut {
                    parameter,
                    target,
                    target_name: declaration.name(),
                })
            }
        }
    }

    fn context_at(&self, call: CallRef<'a>, ordinal: usize) -> Result<CallContextRef, String> {
        let callable = self.callable(call.callable()).ok_or_else(|| {
            format!(
                "checked call {} references missing callable {}",
                call.id().0,
                call.callable().0,
            )
        })?;
        let context = match call {
            CallRef::Rich(call) => {
                let context = call.contexts.get(ordinal).ok_or_else(|| {
                    format!("checked call {} has no context {ordinal}", call.id.0)
                })?;
                rich_context_ref(context)
            }
            CallRef::Packed(call) => {
                call.context(ordinal)
                    .map(packed_context_ref)
                    .ok_or_else(|| {
                        format!(
                            "packed checked call {} has no context {ordinal}",
                            call.id().0
                        )
                    })?
            }
        };
        if context.signature >= callable.context_count() {
            return Err(format!(
                "checked call {} context {ordinal} references missing signature context {}",
                call.id().0,
                context.signature,
            ));
        }
        self.declaration(context.declaration).ok_or_else(|| {
            format!(
                "checked call {} context {ordinal} references missing declaration {}",
                call.id().0,
                context.declaration.0,
            )
        })?;
        self.require_scope(call.id(), context.scope)?;
        Ok(context)
    }

    fn require_scope(&self, call: CheckedCallId, scope: LexicalScopeId) -> Result<(), String> {
        self.has_scope(scope).then_some(()).ok_or_else(|| {
            format!(
                "checked call {} references missing scope {}",
                call.0, scope.0,
            )
        })
    }

    #[cfg(any(test, feature = "test-packed-call-oracle"))]
    pub(crate) fn has_complete_rich_parity_input(&self, program: &CheckedProgramFields) -> bool {
        let CallCatalogSource::Packed(input) = &self.source else {
            return false;
        };
        let counts = input.entity_counts();
        program.calls.len() == input.call_count()
            && program.callables.len() == input.callable_count()
            && program.declarations.len() == counts.declarations
            && program.context_formals.len() == counts.context_formals
    }

    #[cfg(any(test, feature = "test-packed-call-oracle"))]
    pub(crate) fn validate_rich_parity(
        &self,
        program: &'a CheckedProgramFields,
    ) -> Result<(), String> {
        let CallCatalogSource::Packed(input) = &self.source else {
            return Err("packed call parity requires a packed catalog".to_owned());
        };
        let rich_catalog = Self::rich(program)?;
        let counts = input.entity_counts();
        if program.callables.len() != input.callable_count()
            || program.callables.len() != counts.callables
        {
            return Err(format!(
                "rich checked callable count {} differs from packed counts {}/{}",
                program.callables.len(),
                input.callable_count(),
                counts.callables,
            ));
        }
        if program.declarations.len() != counts.declarations {
            return Err(format!(
                "rich checked declaration count {} differs from packed count {}",
                program.declarations.len(),
                counts.declarations,
            ));
        }
        if program.context_formals.len() != counts.context_formals {
            return Err(format!(
                "rich checked context-formal count {} differs from packed count {}",
                program.context_formals.len(),
                counts.context_formals,
            ));
        }
        if self.declaration(DeclId(0)).is_some() {
            return Err("packed declaration zero must remain the absent sentinel".to_owned());
        }
        let mut materializer = input.compatibility_type_materializer();
        let mut packed_context_formal_count = 0usize;
        for (index, expected) in program.callables.iter().enumerate() {
            let actual = input.callable_at(index).ok_or_else(|| {
                format!("packed callable authority omits rich callable index {index}")
            })?;
            if actual.index() != index
                || input.callable(expected.decl_id).map(|row| row.identity())
                    != Some(actual.identity())
                || input.callable_index(expected.decl_id) != Some(index)
                || actual.declaration() != expected.decl_id
                || actual.scope() != expected.scope_id
                || actual.kind() != expected.kind
                || actual.name() != expected.name
                || actual.intrinsic() != expected.intrinsic
                || actual.external_identity() != expected.external_identity
                || actual.parameter_count() != expected.parameters.len()
                || actual.context_count() != expected.contexts.len()
                || actual.role() != expected.role
                || actual.effect() != expected.effect
                || actual.body() != expected.body
                || actual.result_expression() != expected.result_expression
                || actual.contextual_operation() != expected.contextual_operation
            {
                return Err(format!(
                    "rich checked callable {} differs from packed scalar topology",
                    expected.decl_id.0,
                ));
            }
            if materializer
                .materialize_flow(actual.result())
                .map_err(|error| error.to_string())?
                != expected.result
            {
                return Err(format!(
                    "rich checked callable {} result differs from packed authority",
                    expected.decl_id.0,
                ));
            }
            for (ordinal, (expected_parameter, actual_parameter)) in expected
                .parameters
                .iter()
                .zip(actual.parameters())
                .enumerate()
            {
                if actual_parameter.declaration() != expected_parameter.decl_id
                    || actual_parameter.ordinal() != ordinal
                    || actual_parameter.ordinal() != expected_parameter.ordinal
                    || actual_parameter.name() != expected_parameter.name
                    || actual_parameter.kind() != expected_parameter.kind
                    || !ParameterRequirementRef::Packed(actual_parameter.requirement())
                        .matches_checked(&expected_parameter.requirement)
                    || actual_parameter.evaluation_scope() != expected_parameter.evaluation_scope
                    || actual_parameter.start() != expected_parameter.start
                    || actual_parameter.end() != expected_parameter.end
                    || materializer
                        .materialize_flow(actual_parameter.flow())
                        .map_err(|error| error.to_string())?
                        != expected_parameter.flow_type
                {
                    return Err(format!(
                        "rich checked callable {} parameter {ordinal} differs from packed authority",
                        expected.decl_id.0,
                    ));
                }
            }
            for (ordinal, (expected_context, actual_context)) in
                expected.contexts.iter().zip(actual.contexts()).enumerate()
            {
                if actual_context.name() != expected_context.name
                    || actual_context.kind() != expected_context.kind
                    || actual_context.provider() != expected_context.provider
                    || materializer
                        .materialize_flow(actual_context.flow())
                        .map_err(|error| error.to_string())?
                        != expected_context.flow_type
                {
                    return Err(format!(
                        "rich checked callable {} context {ordinal} differs from packed authority",
                        expected.decl_id.0,
                    ));
                }
            }
            match (expected.context_formal, actual.context_formal()) {
                (None, None) => {}
                (Some(expected_id), Some(actual_formal)) => {
                    packed_context_formal_count += 1;
                    let expected_formal = program
                        .context_formals
                        .iter()
                        .find(|formal| formal.id == expected_id)
                        .ok_or_else(|| {
                            format!(
                                "rich checked callable {} references missing context formal {}",
                                expected.decl_id.0, expected_id.0,
                            )
                        })?;
                    let expected_formal = ContextFormalRef::Rich(expected_formal);
                    let actual_formal = ContextFormalRef::Packed(actual_formal);
                    let CallFlowRef::Rich(expected_flow) = expected_formal.flow() else {
                        unreachable!("rich context formal exposes a rich flow")
                    };
                    if actual_formal.id() != expected_formal.id()
                        || actual_formal.callable() != expected_formal.callable()
                        || materializer
                            .materialize_flow(match actual_formal.flow() {
                                CallFlowRef::Packed(flow) => flow,
                                CallFlowRef::Rich(_) => {
                                    unreachable!("packed callable exposes a packed context formal")
                                }
                            })
                            .map_err(|error| error.to_string())?
                            != *expected_flow
                    {
                        return Err(format!(
                            "rich checked callable {} context formal differs from packed authority",
                            expected.decl_id.0,
                        ));
                    }
                }
                _ => {
                    return Err(format!(
                        "rich checked callable {} context-formal presence differs from packed authority",
                        expected.decl_id.0,
                    ));
                }
            }
        }
        if packed_context_formal_count != counts.context_formals {
            return Err(format!(
                "packed callable catalog exposes {packed_context_formal_count} context formals for {} sealed rows",
                counts.context_formals,
            ));
        }
        for expected in &program.declarations {
            let actual = self.declaration(expected.id).ok_or_else(|| {
                format!(
                    "packed declaration authority omits rich declaration {}",
                    expected.id.0,
                )
            })?;
            if actual.id() != expected.id
                || actual.scope() != expected.scope_id
                || actual.name() != expected.name
                || actual.kind() != expected.kind
                || actual.value() != expected.value
                || actual.body_scope() != expected.body_scope
                || actual.span() != Some(expected.span)
            {
                return Err(format!(
                    "rich checked declaration {} differs from packed topology",
                    expected.id.0,
                ));
            }
        }
        if program.calls.len() != input.call_count() {
            return Err(format!(
                "rich checked call count {} differs from packed count {}",
                program.calls.len(),
                input.call_count(),
            ));
        }
        for rich in &program.calls {
            let packed = input.call(rich.id).ok_or_else(|| {
                format!(
                    "packed call authority omits rich checked call {}",
                    rich.id.0
                )
            })?;
            if packed.expression() != rich.expression
                || packed.callable() != rich.callable
                || packed.owner_callable() != rich.owner_callable
                || packed.function() != rich.function
                || packed.context_binding() != rich.context_binding
                || packed.syntax_discriminated_result() != rich.syntax_discriminated_result
                || packed.span() != rich.span
            {
                return Err(format!(
                    "rich checked call {} differs from packed scalar topology",
                    rich.id.0,
                ));
            }
            let callable = self.callable(rich.callable).ok_or_else(|| {
                format!("rich checked call {} has no target signature", rich.id.0)
            })?;
            if callable.intrinsic() != rich.intrinsic || callable.role() != rich.role {
                return Err(format!(
                    "rich checked call {} differs from its target signature",
                    rich.id.0,
                ));
            }
            if rich.entries.len() != packed.entry_count()
                || rich.contexts.len() != packed.context_count()
            {
                return Err(format!(
                    "rich checked call {} differs from packed entry/context counts",
                    rich.id.0,
                ));
            }
            for ordinal in 0..rich.entries.len() {
                let rich_entry = rich_catalog.entry_at(CallRef::Rich(rich), ordinal)?;
                let packed_entry = self.entry_at(CallRef::Packed(packed), ordinal)?;
                if !same_entry(rich_entry, packed_entry) {
                    return Err(format!(
                        "rich checked call {} entry {ordinal} differs from packed topology",
                        rich.id.0,
                    ));
                }
            }
            for ordinal in 0..rich.contexts.len() {
                if rich_catalog.context_at(CallRef::Rich(rich), ordinal)?
                    != self.context_at(CallRef::Packed(packed), ordinal)?
                {
                    return Err(format!(
                        "rich checked call {} context {ordinal} differs from packed topology",
                        rich.id.0,
                    ));
                }
            }
            if materializer
                .materialize_flow(packed.result())
                .map_err(|error| error.to_string())?
                != rich.result
            {
                return Err(format!(
                    "rich checked call {} result differs from packed authority",
                    rich.id.0,
                ));
            }
            let facts = input
                .call_type_facts(rich.id)
                .ok_or_else(|| format!("packed call {} has no type facts", rich.id.0))?;
            if facts.substitution_count() != rich.type_substitutions.len() {
                return Err(format!(
                    "rich checked call {} substitution count differs from packed authority",
                    rich.id.0,
                ));
            }
            for (ordinal, expected) in rich.type_substitutions.iter().enumerate() {
                let actual = facts.substitution(ordinal).ok_or_else(|| {
                    format!("packed call {} omits substitution {ordinal}", rich.id.0)
                })?;
                if actual.parameter().linked_variable() != expected.variable
                    || materializer
                        .materialize_type(actual.value())
                        .map_err(|error| error.to_string())?
                        != expected.value
                {
                    return Err(format!(
                        "rich checked call {} substitution {ordinal} differs from packed authority",
                        rich.id.0,
                    ));
                }
            }
            let mut contextual = self.contextual_substitutions(CallRef::Packed(packed));
            for expected in &rich.contextual_substitutions {
                let actual = contextual.next().ok_or_else(|| {
                    format!("packed call {} omits a contextual substitution", rich.id.0)
                })?;
                let CallTypeRef::Packed(actual_value) = actual.value else {
                    unreachable!("packed contextual substitution exposes a packed value")
                };
                if actual.formal != expected.formal
                    || actual.variable != expected.variable
                    || materializer
                        .materialize_type(actual_value)
                        .map_err(|error| error.to_string())?
                        != expected.value
                {
                    return Err(format!(
                        "rich checked call {} contextual substitution differs from packed authority",
                        rich.id.0,
                    ));
                }
            }
            if contextual.next().is_some() {
                return Err(format!(
                    "packed call {} has extra contextual substitutions",
                    rich.id.0,
                ));
            }
        }
        Ok(())
    }
}

impl<'program> Iterator for CallIter<'_, 'program> {
    type Item = CallRef<'program>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.catalog.len() {
            return None;
        }
        let call = match &self.catalog.source {
            CallCatalogSource::Rich { program, .. } => {
                program.calls.get(self.next).map(CallRef::Rich)
            }
            CallCatalogSource::Packed(input) => {
                let id = CheckedCallId(
                    u32::try_from(self.next)
                        .expect("sealed packed call iterator index remains within u32"),
                );
                Some(CallRef::Packed(
                    input
                        .call(id)
                        .expect("sealed packed call iterator remains dense"),
                ))
            }
        }?;
        self.next += 1;
        Some(call)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.catalog.len().saturating_sub(self.next);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CallIter<'_, '_> {}

impl<'program> Iterator for CallableIter<'_, 'program> {
    type Item = CallableRef<'program>;

    fn next(&mut self) -> Option<Self::Item> {
        let callable = self.catalog.callable_at(self.next)?;
        self.next += 1;
        Some(callable)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.catalog.callable_count().saturating_sub(self.next);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CallableIter<'_, '_> {}

impl<'program> Iterator for CallEntryIter<'_, 'program> {
    type Item = CallEntryRef<'program>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.call.entry_count() {
            return None;
        }
        let ordinal = self.next;
        self.next += 1;
        Some(
            self.catalog
                .entry_at(self.call, ordinal)
                .expect("validated checked call entry remains exact"),
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.call.entry_count().saturating_sub(self.next);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CallEntryIter<'_, '_> {}

impl<'program> Iterator for CallContextIter<'_, 'program> {
    type Item = CallContextRef;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.call.context_count() {
            return None;
        }
        let ordinal = self.next;
        self.next += 1;
        Some(
            self.catalog
                .context_at(self.call, ordinal)
                .expect("validated checked call context remains exact"),
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.call.context_count().saturating_sub(self.next);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CallContextIter<'_, '_> {}

impl<'a> Iterator for CallContextSubstitutionIter<'a> {
    type Item = CallContextSubstitutionRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.source {
            CallContextSubstitutionSource::Rich(rows) => {
                let row = rows.get(self.next)?;
                self.next += 1;
                Some(CallContextSubstitutionRef {
                    formal: row.formal,
                    variable: row.variable,
                    value: CallTypeRef::Rich(&row.value),
                })
            }
            CallContextSubstitutionSource::Packed {
                call,
                formal,
                scheme,
            } => {
                let substitutions = call_type_facts(call)?;
                while self.next < substitutions.substitution_count() {
                    let ordinal = self.next;
                    self.next += 1;
                    let row = substitutions.substitution(ordinal)?;
                    let variable = row.parameter().linked_variable();
                    if scheme.contains_parameter(row.parameter()) {
                        return Some(CallContextSubstitutionRef {
                            formal,
                            variable,
                            value: CallTypeRef::Packed(row.value()),
                        });
                    }
                }
                None
            }
            CallContextSubstitutionSource::Empty => None,
        }
    }
}

impl<'a> CallRef<'a> {
    pub(crate) fn id(self) -> CheckedCallId {
        match self {
            Self::Rich(call) => call.id,
            Self::Packed(call) => call.id(),
        }
    }

    pub(crate) fn expression(self) -> CheckedExprId {
        match self {
            Self::Rich(call) => call.expression,
            Self::Packed(call) => call.expression(),
        }
    }

    pub(crate) fn callable(self) -> DeclId {
        match self {
            Self::Rich(call) => call.callable,
            Self::Packed(call) => call.callable(),
        }
    }

    pub(crate) fn owner_callable(self) -> Option<DeclId> {
        match self {
            Self::Rich(call) => call.owner_callable,
            Self::Packed(call) => call.owner_callable(),
        }
    }

    pub(crate) fn function(self) -> &'a str {
        match self {
            Self::Rich(call) => &call.function,
            Self::Packed(call) => call.function(),
        }
    }

    pub(crate) fn entry_count(self) -> usize {
        match self {
            Self::Rich(call) => call.entries.len(),
            Self::Packed(call) => call.entry_count(),
        }
    }

    pub(crate) fn context_count(self) -> usize {
        match self {
            Self::Rich(call) => call.contexts.len(),
            Self::Packed(call) => call.context_count(),
        }
    }

    pub(crate) fn context_binding(self) -> CheckedContextBinding {
        match self {
            Self::Rich(call) => call.context_binding,
            Self::Packed(call) => call.context_binding(),
        }
    }

    pub(crate) fn result(self) -> CallFlowRef<'a> {
        match self {
            Self::Rich(call) => CallFlowRef::Rich(&call.result),
            Self::Packed(call) => CallFlowRef::Packed(call.result()),
        }
    }

    pub(crate) fn type_substitution_count(self) -> usize {
        match self {
            Self::Rich(call) => call.type_substitutions.len(),
            Self::Packed(call) => {
                call_type_facts(call).map_or(0, |facts| facts.substitution_count())
            }
        }
    }

    pub(crate) fn type_substitution(self, ordinal: usize) -> Option<CallSubstitutionRef<'a>> {
        match self {
            Self::Rich(call) => {
                call.type_substitutions
                    .get(ordinal)
                    .map(|row| CallSubstitutionRef {
                        variable: row.variable,
                        value: CallTypeRef::Rich(&row.value),
                    })
            }
            Self::Packed(call) => {
                call_type_facts(call)?
                    .substitution(ordinal)
                    .map(|row| CallSubstitutionRef {
                        variable: row.parameter().linked_variable(),
                        value: CallTypeRef::Packed(row.value()),
                    })
            }
        }
    }

    pub(crate) fn syntax_discriminated_result(self) -> bool {
        match self {
            Self::Rich(call) => call.syntax_discriminated_result,
            Self::Packed(call) => call.syntax_discriminated_result(),
        }
    }

    pub(crate) fn span(self) -> CheckedSpan {
        match self {
            Self::Rich(call) => call.span,
            Self::Packed(call) => call.span(),
        }
    }
}

impl<'a> CallableRef<'a> {
    pub(crate) fn declaration(self) -> DeclId {
        match self {
            Self::Rich(row) => row.decl_id,
            Self::Packed(row) => row.declaration(),
        }
    }

    pub(crate) fn scope(self) -> LexicalScopeId {
        match self {
            Self::Rich(row) => row.scope_id,
            Self::Packed(row) => row.scope(),
        }
    }

    pub(crate) fn kind(self) -> CheckedCallableKind {
        match self {
            Self::Rich(row) => row.kind,
            Self::Packed(row) => row.kind(),
        }
    }

    pub(crate) fn name(self) -> &'a str {
        match self {
            Self::Rich(row) => &row.name,
            Self::Packed(row) => row.name(),
        }
    }

    pub(crate) fn intrinsic(self) -> Option<CheckedIntrinsicV1> {
        match self {
            Self::Rich(row) => row.intrinsic,
            Self::Packed(row) => row.intrinsic(),
        }
    }

    pub(crate) fn external_identity(self) -> Option<CheckedExternalDeclarationIdentityV1> {
        match self {
            Self::Rich(row) => row.external_identity,
            Self::Packed(row) => row.external_identity(),
        }
    }

    pub(crate) fn parameter_count(self) -> usize {
        match self {
            Self::Rich(row) => row.parameters.len(),
            Self::Packed(row) => row.parameter_count(),
        }
    }

    pub(crate) fn parameters(self) -> CallableParameterIter<'a> {
        match self {
            Self::Rich(row) => CallableParameterIter::Rich(row.parameters.iter()),
            Self::Packed(row) => CallableParameterIter::Packed(row.parameters()),
        }
    }

    pub(crate) fn parameter(self, ordinal: usize) -> Option<CallableParameterRef<'a>> {
        match self {
            Self::Rich(row) => row
                .parameters
                .iter()
                .find(|parameter| parameter.ordinal == ordinal)
                .map(CallableParameterRef::Rich),
            Self::Packed(row) => row.parameter(ordinal).map(CallableParameterRef::Packed),
        }
    }

    pub(crate) fn context_count(self) -> usize {
        match self {
            Self::Rich(row) => row.contexts.len(),
            Self::Packed(row) => row.context_count(),
        }
    }

    pub(crate) fn contexts(self) -> CallableContextIter<'a> {
        match self {
            Self::Rich(row) => CallableContextIter::Rich(row.contexts.iter()),
            Self::Packed(row) => CallableContextIter::Packed(row.contexts()),
        }
    }

    pub(crate) fn context_formal_id(self) -> Option<ContextFormalId> {
        match self {
            Self::Rich(row) => row.context_formal,
            Self::Packed(row) => row.context_formal().map(|formal| formal.id()),
        }
    }

    pub(crate) fn result(self) -> CallFlowRef<'a> {
        match self {
            Self::Rich(row) => CallFlowRef::Rich(&row.result),
            Self::Packed(row) => CallFlowRef::Packed(row.result()),
        }
    }

    pub(crate) fn role(self) -> ProgramRole {
        match self {
            Self::Rich(row) => row.role,
            Self::Packed(row) => row.role(),
        }
    }

    pub(crate) fn effect(self) -> CheckedEffectSummary {
        match self {
            Self::Rich(row) => row.effect,
            Self::Packed(row) => row.effect(),
        }
    }

    pub(crate) fn body(self) -> Option<CheckedStatementId> {
        match self {
            Self::Rich(row) => row.body,
            Self::Packed(row) => row.body(),
        }
    }

    pub(crate) fn result_expression(self) -> Option<CheckedExprId> {
        match self {
            Self::Rich(row) => row.result_expression,
            Self::Packed(row) => row.result_expression(),
        }
    }

    pub(crate) fn contextual_operation(self) -> Option<CheckedContextualOperation> {
        match self {
            Self::Rich(row) => row.contextual_operation,
            Self::Packed(row) => row.contextual_operation(),
        }
    }

    pub(crate) fn requires_pass(self) -> bool {
        self.context_formal_id().is_some()
    }
}

impl<'a> Iterator for CallableParameterIter<'a> {
    type Item = CallableParameterRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Rich(rows) => rows.next().map(CallableParameterRef::Rich),
            Self::Packed(rows) => rows.next().map(CallableParameterRef::Packed),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Rich(rows) => rows.size_hint(),
            Self::Packed(rows) => rows.size_hint(),
        }
    }
}

impl ExactSizeIterator for CallableParameterIter<'_> {}

impl<'a> CallableParameterRef<'a> {
    pub(crate) fn declaration(self) -> DeclId {
        match self {
            Self::Rich(row) => row.decl_id,
            Self::Packed(row) => row.declaration(),
        }
    }

    pub(crate) fn name(self) -> &'a str {
        match self {
            Self::Rich(row) => &row.name,
            Self::Packed(row) => row.name(),
        }
    }

    pub(crate) fn kind(self) -> CheckedParameterKind {
        match self {
            Self::Rich(row) => row.kind,
            Self::Packed(row) => row.kind(),
        }
    }

    pub(crate) fn ordinal(self) -> usize {
        match self {
            Self::Rich(row) => row.ordinal,
            Self::Packed(row) => row.ordinal(),
        }
    }

    pub(crate) fn flow(self) -> CallFlowRef<'a> {
        match self {
            Self::Rich(row) => CallFlowRef::Rich(&row.flow_type),
            Self::Packed(row) => CallFlowRef::Packed(row.flow()),
        }
    }

    pub(crate) fn requirement(self) -> ParameterRequirementRef<'a> {
        match self {
            Self::Rich(row) => ParameterRequirementRef::Rich(&row.requirement),
            Self::Packed(row) => ParameterRequirementRef::Packed(row.requirement()),
        }
    }

    pub(crate) fn evaluation_scope(self) -> CheckedEvaluationScope {
        match self {
            Self::Rich(row) => row.evaluation_scope,
            Self::Packed(row) => row.evaluation_scope(),
        }
    }

    pub(crate) fn start(self) -> usize {
        match self {
            Self::Rich(row) => row.start,
            Self::Packed(row) => row.start(),
        }
    }

    pub(crate) fn end(self) -> usize {
        match self {
            Self::Rich(row) => row.end,
            Self::Packed(row) => row.end(),
        }
    }
}

impl<'a> Iterator for CallableContextIter<'a> {
    type Item = CallableContextRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Rich(rows) => rows.next().map(CallableContextRef::Rich),
            Self::Packed(rows) => rows.next().map(CallableContextRef::Packed),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Rich(rows) => rows.size_hint(),
            Self::Packed(rows) => rows.size_hint(),
        }
    }
}

impl ExactSizeIterator for CallableContextIter<'_> {}

impl<'a> CallableContextRef<'a> {
    pub(crate) fn name(self) -> &'a str {
        match self {
            Self::Rich(row) => &row.name,
            Self::Packed(row) => row.name(),
        }
    }

    pub(crate) fn kind(self) -> CheckedCallContextKind {
        match self {
            Self::Rich(row) => row.kind,
            Self::Packed(row) => row.kind(),
        }
    }

    pub(crate) fn provider(self) -> DeclId {
        match self {
            Self::Rich(row) => row.provider,
            Self::Packed(row) => row.provider(),
        }
    }

    pub(crate) fn flow(self) -> CallFlowRef<'a> {
        match self {
            Self::Rich(row) => CallFlowRef::Rich(&row.flow_type),
            Self::Packed(row) => CallFlowRef::Packed(row.flow()),
        }
    }
}

impl<'a> ContextFormalRef<'a> {
    pub(crate) fn id(self) -> ContextFormalId {
        match self {
            Self::Rich(row) => row.id,
            Self::Packed(row) => row.id(),
        }
    }

    pub(crate) fn callable(self) -> DeclId {
        match self {
            Self::Rich(row) => row.callable,
            Self::Packed(row) => row.callable(),
        }
    }

    pub(crate) fn flow(self) -> CallFlowRef<'a> {
        match self {
            Self::Rich(row) => CallFlowRef::Rich(&row.scheme.flow_type),
            Self::Packed(row) => CallFlowRef::Packed(row.flow()),
        }
    }
}

impl<'a> DeclarationRef<'a> {
    #[cfg(any(test, feature = "test-packed-call-oracle"))]
    pub(crate) fn id(self) -> DeclId {
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

    pub(crate) fn name(self) -> &'a str {
        match self {
            Self::Rich(row) => &row.name,
            Self::Packed(row) => row.name(),
        }
    }

    pub(crate) fn kind(self) -> CheckedDeclarationKind {
        match self {
            Self::Rich(row) => row.kind,
            Self::Packed(row) => row.kind(),
        }
    }

    pub(crate) fn value(self) -> Option<CheckedExprId> {
        match self {
            Self::Rich(row) => row.value,
            Self::Packed(row) => row.value(),
        }
    }

    pub(crate) fn body_scope(self) -> Option<LexicalScopeId> {
        match self {
            Self::Rich(row) => row.body_scope,
            Self::Packed(row) => row.body_scope(),
        }
    }

    #[cfg(any(test, feature = "test-packed-call-oracle"))]
    pub(crate) fn span(self) -> Option<CheckedSpan> {
        match self {
            Self::Rich(row) => Some(row.span),
            Self::Packed(row) => row.span(),
        }
    }
}

impl ParameterRequirementRef<'_> {
    pub(crate) fn is_required(self) -> bool {
        match self {
            Self::Rich(row) => matches!(row, CheckedParameterRequirement::Required),
            Self::Packed(KernelSemanticParameterRequirementRef::Required) => true,
            Self::Packed(
                KernelSemanticParameterRequirementRef::CallableProfile(_)
                | KernelSemanticParameterRequirementRef::Tag(_)
                | KernelSemanticParameterRequirementRef::ExactInteger(_)
                | KernelSemanticParameterRequirementRef::Text(_),
            ) => false,
        }
    }

    pub(crate) fn to_owned(self) -> CheckedParameterRequirement {
        match self {
            Self::Rich(row) => row.clone(),
            Self::Packed(KernelSemanticParameterRequirementRef::Required) => {
                CheckedParameterRequirement::Required
            }
            Self::Packed(KernelSemanticParameterRequirementRef::CallableProfile(profile)) => {
                CheckedParameterRequirement::Optional {
                    default: CheckedParameterDefault::CallableProfile {
                        profile: profile.to_owned(),
                    },
                }
            }
            Self::Packed(KernelSemanticParameterRequirementRef::Tag(name)) => {
                CheckedParameterRequirement::Optional {
                    default: CheckedParameterDefault::Tag {
                        name: name.to_owned(),
                    },
                }
            }
            Self::Packed(KernelSemanticParameterRequirementRef::ExactInteger(value)) => {
                CheckedParameterRequirement::Optional {
                    default: CheckedParameterDefault::ExactInteger { value },
                }
            }
            Self::Packed(KernelSemanticParameterRequirementRef::Text(value)) => {
                CheckedParameterRequirement::Optional {
                    default: CheckedParameterDefault::Text {
                        value: value.to_owned(),
                    },
                }
            }
        }
    }

    pub(crate) fn matches_checked(self, expected: &CheckedParameterRequirement) -> bool {
        match (self, expected) {
            (Self::Rich(actual), expected) => actual == expected,
            (
                Self::Packed(KernelSemanticParameterRequirementRef::Required),
                CheckedParameterRequirement::Required,
            ) => true,
            (
                Self::Packed(KernelSemanticParameterRequirementRef::CallableProfile(actual)),
                CheckedParameterRequirement::Optional {
                    default: CheckedParameterDefault::CallableProfile { profile },
                },
            ) => actual == profile,
            (
                Self::Packed(KernelSemanticParameterRequirementRef::Tag(actual)),
                CheckedParameterRequirement::Optional {
                    default: CheckedParameterDefault::Tag { name },
                },
            ) => actual == name,
            (
                Self::Packed(KernelSemanticParameterRequirementRef::ExactInteger(actual)),
                CheckedParameterRequirement::Optional {
                    default: CheckedParameterDefault::ExactInteger { value },
                },
            ) => actual == *value,
            (
                Self::Packed(KernelSemanticParameterRequirementRef::Text(actual)),
                CheckedParameterRequirement::Optional {
                    default: CheckedParameterDefault::Text { value },
                },
            ) => actual == value,
            _ => false,
        }
    }
}

impl<'a> CallEntryRef<'a> {
    pub(crate) fn parameter(self) -> CallableParameterRef<'a> {
        match self {
            Self::Input { parameter, .. }
            | Self::FreshOut { parameter, .. }
            | Self::ForwardOut { parameter, .. } => parameter,
        }
    }

    pub(crate) fn formal(self) -> DeclId {
        self.parameter().declaration()
    }

    pub(crate) fn input(self) -> Option<(CheckedExprId, bool, CheckedEvaluationScope)> {
        match self {
            Self::Input {
                value,
                from_pipe,
                evaluation_scope,
                ..
            } => Some((value, from_pipe, evaluation_scope)),
            Self::FreshOut { .. } | Self::ForwardOut { .. } => None,
        }
    }

    pub(crate) fn output(self) -> Option<DeclId> {
        match self {
            Self::FreshOut { output, .. } => Some(output),
            Self::ForwardOut { target, .. } => Some(target),
            Self::Input { .. } => None,
        }
    }
}

impl<'a> CallFlowRef<'a> {
    pub(crate) fn mode(self) -> FlowMode {
        match self {
            Self::Rich(flow) => flow.mode,
            Self::Packed(flow) => flow.mode(),
        }
    }

    pub(crate) fn ty(self) -> CallTypeRef<'a> {
        match self {
            Self::Rich(flow) => CallTypeRef::Rich(&flow.ty),
            Self::Packed(flow) => CallTypeRef::Packed(flow.ty()),
        }
    }
}

impl CallTypeMaterializer<'_> {
    pub(crate) fn materialize_type(&mut self, ty: CallTypeRef<'_>) -> Result<Type, String> {
        match (self, ty) {
            (Self::Rich, CallTypeRef::Rich(ty)) => Ok(ty.clone()),
            (Self::Packed(materializer), CallTypeRef::Packed(ty)) => materializer
                .materialize_type(ty)
                .map_err(|error| error.to_string()),
            (Self::Rich, CallTypeRef::Packed(_)) => {
                Err("rich call projector received a packed type".to_owned())
            }
            (Self::Packed(_), CallTypeRef::Rich(_)) => {
                Err("packed call projector received a rich type".to_owned())
            }
        }
    }

    pub(crate) fn materialize_flow(&mut self, flow: CallFlowRef<'_>) -> Result<FlowType, String> {
        Ok(FlowType {
            mode: flow.mode(),
            ty: self.materialize_type(flow.ty())?,
        })
    }
}

impl CallTypeCatalog {
    pub(crate) fn new(calls: &CallCatalog<'_>) -> Result<Self, String> {
        let mut materializer = calls.type_materializer();
        let mut callable_types = Vec::with_capacity(calls.callable_count());
        for callable in calls.callables() {
            let parameters = callable
                .parameters()
                .map(|parameter| materializer.materialize_flow(parameter.flow()))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice();
            let contexts = callable
                .contexts()
                .map(|context| materializer.materialize_flow(context.flow()))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice();
            callable_types.push(CallableTypeFacts {
                result: materializer.materialize_flow(callable.result())?,
                parameters,
                contexts,
                context_formal: calls
                    .context_formal(callable.declaration())
                    .map(|formal| materializer.materialize_flow(formal.flow()))
                    .transpose()?,
            });
        }
        let mut rows = Vec::new();
        for call in calls.calls() {
            let index = call.id().0 as usize;
            if rows.len() <= index {
                rows.resize_with(index.saturating_add(1), || None);
            }
            let mut type_substitutions = Vec::with_capacity(call.type_substitution_count());
            for ordinal in 0..call.type_substitution_count() {
                let substitution = call.type_substitution(ordinal).ok_or_else(|| {
                    format!(
                        "checked call {} omits type substitution {ordinal}",
                        call.id().0,
                    )
                })?;
                type_substitutions.push(boon_checked::CheckedTypeSubstitution {
                    variable: substitution.variable,
                    value: materializer.materialize_type(substitution.value)?,
                });
            }
            let contextual_substitutions = calls
                .contextual_substitutions(call)
                .map(|substitution| {
                    let value = type_substitutions
                        .iter()
                        .find(|materialized| materialized.variable == substitution.variable)
                        .map(|materialized| materialized.value.clone())
                        .ok_or_else(|| {
                            format!(
                                "checked call {} contextual variable {} has no type substitution",
                                call.id().0,
                                substitution.variable.0,
                            )
                        })?;
                    Ok(CheckedContextTypeSubstitution {
                        formal: substitution.formal,
                        variable: substitution.variable,
                        value,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let facts = CallTypeFacts {
                result: materializer.materialize_flow(call.result())?,
                type_substitutions,
                contextual_substitutions,
            };
            if rows[index].replace(facts).is_some() {
                return Err(format!(
                    "checked call {} has duplicate type facts",
                    call.id().0,
                ));
            }
        }
        Ok(Self {
            rows,
            remaining_callables: callable_types.len(),
            callables: callable_types.into_iter().map(Some).collect(),
            remaining: calls.len(),
        })
    }

    pub(crate) fn get(&self, call: CheckedCallId) -> Option<&CallTypeFacts> {
        self.rows.get(call.0 as usize)?.as_ref()
    }

    pub(crate) fn take(&mut self, call: CheckedCallId) -> Option<CallTypeFacts> {
        let facts = self.rows.get_mut(call.0 as usize)?.take();
        if facts.is_some() {
            self.remaining -= 1;
        }
        facts
    }

    pub(crate) fn callable(
        &self,
        calls: &CallCatalog<'_>,
        callable: DeclId,
    ) -> Option<&CallableTypeFacts> {
        self.callables
            .get(calls.callable_index(callable)?)?
            .as_ref()
    }

    pub(crate) fn parameter(
        &self,
        calls: &CallCatalog<'_>,
        callable: DeclId,
        ordinal: usize,
    ) -> Option<&FlowType> {
        self.callable(calls, callable)?.parameters.get(ordinal)
    }

    pub(crate) fn take_context_formal(
        &mut self,
        calls: &CallCatalog<'_>,
        callable: DeclId,
    ) -> Option<FlowType> {
        self.callables
            .get_mut(calls.callable_index(callable)?)?
            .as_mut()?
            .context_formal
            .take()
    }

    pub(crate) fn take_callable(
        &mut self,
        calls: &CallCatalog<'_>,
        callable: DeclId,
    ) -> Option<CallableTypeFacts> {
        let facts = self
            .callables
            .get_mut(calls.callable_index(callable)?)?
            .take();
        if facts.is_some() {
            self.remaining_callables -= 1;
        }
        facts
    }

    pub(crate) fn finish(self) -> Result<(), String> {
        if self.remaining == 0 && self.remaining_callables == 0 {
            Ok(())
        } else {
            Err(format!(
                "semantic construction left {} call and {} callable type-fact rows unconsumed",
                self.remaining, self.remaining_callables,
            ))
        }
    }
}

fn call_type_facts(
    call: KernelSemanticCallRef<'_>,
) -> Option<boon_compiler_kernel::KernelDefinitionCallTypeFactsRef<'_>> {
    Some(call.type_facts())
}

fn dense_index<T>(
    rows: &[T],
    mut key: impl FnMut(&T) -> usize,
    label: &str,
) -> Result<Box<[Option<usize>]>, String> {
    let mut index = Vec::new();
    for (ordinal, row) in rows.iter().enumerate() {
        let key = key(row);
        if index.len() <= key {
            index.resize(key.saturating_add(1), None);
        }
        if index[key].replace(ordinal).is_some() {
            return Err(format!("checked program repeats {label} {key}"));
        }
    }
    Ok(index.into_boxed_slice())
}

fn validate_parameter(
    call: CheckedCallId,
    entry: usize,
    parameter: CallableParameterRef<'_>,
    name: &str,
    kind: CheckedParameterKind,
) -> Result<(), String> {
    if parameter.name() != name || parameter.kind() != kind {
        return Err(format!(
            "checked call {} entry {entry} differs from formal {} `{}`",
            call.0,
            parameter.declaration().0,
            parameter.name(),
        ));
    }
    Ok(())
}

fn rich_entry_formal(entry: &CheckedCallEntry) -> DeclId {
    match entry {
        CheckedCallEntry::Input { formal, .. }
        | CheckedCallEntry::FreshOut { formal, .. }
        | CheckedCallEntry::ForwardOut { formal, .. } => *formal,
    }
}

fn packed_entry_parameter_ordinal(entry: KernelSemanticCallEntryRef) -> u32 {
    match entry {
        KernelSemanticCallEntryRef::Input {
            parameter_ordinal, ..
        }
        | KernelSemanticCallEntryRef::FreshOut {
            parameter_ordinal, ..
        }
        | KernelSemanticCallEntryRef::ForwardOut {
            parameter_ordinal, ..
        } => parameter_ordinal,
    }
}

fn rich_context_ref(context: &CheckedCallContext) -> CallContextRef {
    CallContextRef {
        declaration: context.declaration,
        signature: context.signature,
        scope: context.scope_id,
    }
}

fn packed_context_ref(context: KernelSemanticCallContextRef) -> CallContextRef {
    CallContextRef {
        declaration: context.declaration,
        signature: context.signature_ordinal as usize,
        scope: context.scope,
    }
}

#[cfg(any(test, feature = "test-packed-call-oracle"))]
fn same_entry(left: CallEntryRef<'_>, right: CallEntryRef<'_>) -> bool {
    match (left, right) {
        (
            CallEntryRef::Input {
                parameter: left_parameter,
                value: left_value,
                from_pipe: left_pipe,
                evaluation_scope: left_scope,
            },
            CallEntryRef::Input {
                parameter: right_parameter,
                value: right_value,
                from_pipe: right_pipe,
                evaluation_scope: right_scope,
            },
        ) => {
            left_parameter.declaration() == right_parameter.declaration()
                && left_parameter.name() == right_parameter.name()
                && left_value == right_value
                && left_pipe == right_pipe
                && left_scope == right_scope
        }
        (
            CallEntryRef::FreshOut {
                parameter: left_parameter,
                output: left_output,
                scope: left_scope,
            },
            CallEntryRef::FreshOut {
                parameter: right_parameter,
                output: right_output,
                scope: right_scope,
            },
        ) => {
            left_parameter.declaration() == right_parameter.declaration()
                && left_parameter.name() == right_parameter.name()
                && left_output == right_output
                && left_scope == right_scope
        }
        (
            CallEntryRef::ForwardOut {
                parameter: left_parameter,
                target: left_target,
                target_name: left_name,
            },
            CallEntryRef::ForwardOut {
                parameter: right_parameter,
                target: right_target,
                target_name: right_name,
            },
        ) => {
            left_parameter.declaration() == right_parameter.declaration()
                && left_parameter.name() == right_parameter.name()
                && left_target == right_target
                && left_name == right_name
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_fields(source: &str) -> CheckedProgramFields {
        let parsed = boon_parser::parse_source("call-view.bn", source).unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "diagnostics: {:#?}",
            checked.report.diagnostics,
        );
        checked.program.unwrap().into_parts().0
    }

    #[test]
    fn rich_catalog_indexes_sparse_ids_and_borrows_entries() {
        let program = checked_fields("value: List/get(list: [1], position: 1)");
        let catalog = CallCatalog::rich(&program).unwrap();
        assert_eq!(catalog.calls().len(), program.calls.len());
        for call in catalog.calls() {
            assert_eq!(catalog.get(call.id()).map(CallRef::id), Some(call.id()));
            assert_eq!(catalog.entries(call).len(), call.entry_count());
            assert!(catalog.callable(call.callable()).is_some());
        }
    }

    #[test]
    fn rich_catalog_accepts_sparse_ids_but_rejects_duplicates_and_missing_targets() {
        let mut program = checked_fields("value: List/get(list: [1], position: 1)");
        program.calls[0].id = CheckedCallId(7);
        let catalog = CallCatalog::rich(&program).unwrap();
        assert!(catalog.get(CheckedCallId(7)).is_some());

        let mut program = checked_fields(
            "first: List/get(list: [1], position: 1)\nsecond: List/get(list: [2], position: 1)",
        );
        program.calls[1].id = program.calls[0].id;
        assert!(CallCatalog::rich(&program).is_err());

        let mut program = checked_fields("value: List/get(list: [1], position: 1)");
        program.calls[0].callable = DeclId(u32::MAX);
        assert!(CallCatalog::rich(&program).is_err());
    }

    #[test]
    fn rich_catalog_rejects_missing_duplicate_and_foreign_context_formals() {
        let source = "FUNCTION view() {\n    PASSED.store.value\n}\n";

        let mut missing = checked_fields(source);
        missing.context_formals.clear();
        assert!(CallCatalog::rich(&missing).is_err());

        let mut duplicate = checked_fields(source);
        duplicate
            .context_formals
            .push(duplicate.context_formals[0].clone());
        assert!(CallCatalog::rich(&duplicate).is_err());

        let mut foreign = checked_fields(source);
        foreign.context_formals[0].callable = DeclId(u32::MAX);
        assert!(CallCatalog::rich(&foreign).is_err());
    }
}
