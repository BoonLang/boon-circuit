//! Allocation-free borrowed access to checked call topology.
//!
//! Runtime compilation reads the packed kernel authority directly. Rich rows
//! remain available to legacy/editor paths and as a debug parity oracle, but
//! neither representation is copied into another checked-call DTO here.

use boon_checked::{
    CheckedCall, CheckedCallContext, CheckedCallEntry, CheckedCallId, CheckedCallableSignature,
    CheckedContextBinding, CheckedContextTypeSubstitution, CheckedDeclaration,
    CheckedEvaluationScope, CheckedExprId, CheckedParameter, CheckedParameterKind,
    CheckedProgramFields, CheckedSpan, ContextFormalId, DeclId, FlowMode, FlowType, LexicalScopeId,
    Type, TypeVar, Variant,
};
use boon_compiler_kernel::{
    KernelPackedFlowRef, KernelPackedTypeRef, KernelSemanticCallContextRef,
    KernelSemanticCallEntryRef, KernelSemanticCallRef, KernelSemanticInputV1,
    KernelSemanticTypeMaterializer,
};

#[derive(Clone, Copy)]
enum CallSource<'a> {
    Rich(&'a [CheckedCall]),
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
        parameter: &'a CheckedParameter,
        value: CheckedExprId,
        from_pipe: bool,
        evaluation_scope: CheckedEvaluationScope,
    },
    FreshOut {
        parameter: &'a CheckedParameter,
        output: DeclId,
        scope: LexicalScopeId,
    },
    ForwardOut {
        parameter: &'a CheckedParameter,
        target: DeclId,
        target_name: &'a str,
    },
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

/// The single rich-type ownership bridge for one semantic elaboration.
///
/// Packed topology remains borrowed. Only values that the current durable
/// semantic schema must own are projected, once, and then moved into the final
/// `SemanticCall` rows after OUT/contextual construction has borrowed them.
pub(crate) struct CallTypeCatalog {
    rows: Vec<Option<CallTypeFacts>>,
    remaining: usize,
}

pub(crate) struct CallCatalog<'a> {
    program: &'a CheckedProgramFields,
    source: CallSource<'a>,
    rich_call_by_id: Box<[Option<usize>]>,
    callable_by_declaration: Box<[Option<usize>]>,
    declaration_by_id: Box<[Option<usize>]>,
    context_formal_by_id: Box<[Option<usize>]>,
}

pub(crate) struct CallIter<'catalog, 'program> {
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
        scheme: &'a Type,
    },
    Empty,
}

pub(crate) struct CallContextSubstitutionIter<'a> {
    source: CallContextSubstitutionSource<'a>,
    next: usize,
}

impl<'a> CallCatalog<'a> {
    pub(crate) fn new(
        program: &'a CheckedProgramFields,
        kernel_input: Option<&'a KernelSemanticInputV1>,
    ) -> Result<Self, String> {
        let source = kernel_input.map_or(CallSource::Rich(&program.calls), CallSource::Packed);
        let catalog = Self {
            program,
            source,
            rich_call_by_id: dense_index(
                &program.calls,
                |call| call.id.0 as usize,
                "checked call",
            )?,
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
            context_formal_by_id: dense_index(
                &program.context_formals,
                |formal| formal.id.0 as usize,
                "context formal",
            )?,
        };
        catalog.validate()?;
        #[cfg(debug_assertions)]
        if let CallSource::Packed(input) = catalog.source
            && !program.calls.is_empty()
        {
            catalog.validate_rich_parity(input)?;
        }
        Ok(catalog)
    }

    #[cfg(test)]
    pub(crate) fn rich(program: &'a CheckedProgramFields) -> Result<Self, String> {
        Self::new(program, None)
    }

    pub(crate) fn len(&self) -> usize {
        match self.source {
            CallSource::Rich(calls) => calls.len(),
            CallSource::Packed(input) => input.call_count(),
        }
    }

    pub(crate) fn calls(&self) -> CallIter<'_, 'a> {
        CallIter {
            catalog: self,
            next: 0,
        }
    }

    pub(crate) fn type_materializer(&self) -> CallTypeMaterializer<'a> {
        match self.source {
            CallSource::Rich(_) => CallTypeMaterializer::Rich,
            CallSource::Packed(input) => {
                CallTypeMaterializer::Packed(input.compatibility_type_materializer())
            }
        }
    }

    pub(crate) fn get(&self, id: CheckedCallId) -> Option<CallRef<'a>> {
        match self.source {
            CallSource::Rich(calls) => self
                .rich_call_by_id
                .get(id.0 as usize)
                .copied()
                .flatten()
                .and_then(|index| calls.get(index))
                .filter(|call| call.id == id)
                .map(CallRef::Rich),
            CallSource::Packed(input) => input.call(id).map(CallRef::Packed),
        }
    }

    pub(crate) fn callable(&self, id: DeclId) -> Option<&'a CheckedCallableSignature> {
        self.callable_index(id)
            .and_then(|index| self.program.callables.get(index))
            .filter(|callable| callable.decl_id == id)
    }

    pub(crate) fn callable_index(&self, id: DeclId) -> Option<usize> {
        self.callable_by_declaration
            .get(id.0 as usize)
            .copied()
            .flatten()
    }

    pub(crate) fn declaration(&self, id: DeclId) -> Option<&'a CheckedDeclaration> {
        self.declaration_by_id
            .get(id.0 as usize)
            .copied()
            .flatten()
            .and_then(|index| self.program.declarations.get(index))
            .filter(|declaration| declaration.id == id)
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
            CallRef::Packed(call) => self
                .callable(call.callable())
                .and_then(|callable| callable.context_formal)
                .and_then(|formal| {
                    self.context_formal_by_id
                        .get(formal.0 as usize)
                        .copied()
                        .flatten()
                        .and_then(|index| self.program.context_formals.get(index))
                        .filter(|definition| {
                            definition.id == formal && definition.callable == call.callable()
                        })
                        .map(|definition| CallContextSubstitutionSource::Packed {
                            call,
                            formal,
                            scheme: &definition.scheme.flow_type.ty,
                        })
                })
                .unwrap_or(CallContextSubstitutionSource::Empty),
        };
        CallContextSubstitutionIter { source, next: 0 }
    }

    fn validate(&self) -> Result<(), String> {
        for (index, call) in self.calls().enumerate() {
            if matches!(self.source, CallSource::Packed(_)) {
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
            if self
                .program
                .expressions
                .get(expression.0 as usize)
                .is_none_or(|candidate| candidate.id != expression)
            {
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
                && self
                    .program
                    .expressions
                    .get(value.0 as usize)
                    .is_none_or(|candidate| candidate.id != value)
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
                    .parameters
                    .iter()
                    .find(|parameter| parameter.decl_id == formal)
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
                let parameter = callable.parameters.get(parameter_ordinal).ok_or_else(|| {
                    format!(
                        "packed checked call {} entry {ordinal} references missing parameter ordinal {parameter_ordinal}",
                        call.id().0,
                    )
                })?;
                if parameter.ordinal != parameter_ordinal {
                    return Err(format!(
                        "packed checked call {} entry {ordinal} resolves parameter ordinal {parameter_ordinal} to stale ordinal {}",
                        call.id().0,
                        parameter.ordinal,
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
        parameter: &'a CheckedParameter,
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
                if parameter.evaluation_scope != *evaluation_scope {
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
                if declaration.name != *target_name {
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
        parameter: &'a CheckedParameter,
    ) -> Result<CallEntryRef<'a>, String> {
        match entry {
            KernelSemanticCallEntryRef::Input {
                value, from_pipe, ..
            } => {
                validate_parameter(
                    call,
                    ordinal,
                    parameter,
                    &parameter.name,
                    CheckedParameterKind::Value,
                )?;
                Ok(CallEntryRef::Input {
                    parameter,
                    value,
                    from_pipe,
                    evaluation_scope: parameter.evaluation_scope,
                })
            }
            KernelSemanticCallEntryRef::FreshOut { output, scope, .. } => {
                validate_parameter(
                    call,
                    ordinal,
                    parameter,
                    &parameter.name,
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
                    &parameter.name,
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
                    target_name: &declaration.name,
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
        if context.signature >= callable.contexts.len() {
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
        self.program
            .scopes
            .get(scope.0 as usize)
            .filter(|candidate| candidate.id == scope)
            .map(|_| ())
            .ok_or_else(|| {
                format!(
                    "checked call {} references missing scope {}",
                    call.0, scope.0,
                )
            })
    }

    #[cfg(debug_assertions)]
    fn validate_rich_parity(&self, input: &KernelSemanticInputV1) -> Result<(), String> {
        if self.program.calls.len() != input.call_count() {
            return Err(format!(
                "rich checked call count {} differs from packed count {}",
                self.program.calls.len(),
                input.call_count(),
            ));
        }
        let mut materializer = input.compatibility_type_materializer();
        for rich in &self.program.calls {
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
            if callable.intrinsic != rich.intrinsic || callable.role != rich.role {
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
                let rich_entry = self.entry_at(CallRef::Rich(rich), ordinal)?;
                let packed_entry = self.entry_at(CallRef::Packed(packed), ordinal)?;
                if !same_entry(rich_entry, packed_entry) {
                    return Err(format!(
                        "rich checked call {} entry {ordinal} differs from packed topology",
                        rich.id.0,
                    ));
                }
            }
            for ordinal in 0..rich.contexts.len() {
                if self.context_at(CallRef::Rich(rich), ordinal)?
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
        let call = match self.catalog.source {
            CallSource::Rich(calls) => calls.get(self.next).map(CallRef::Rich),
            CallSource::Packed(input) => {
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
                    if type_contains_variable(scheme, variable) {
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

impl<'a> CallEntryRef<'a> {
    pub(crate) fn parameter(self) -> &'a CheckedParameter {
        match self {
            Self::Input { parameter, .. }
            | Self::FreshOut { parameter, .. }
            | Self::ForwardOut { parameter, .. } => parameter,
        }
    }

    pub(crate) fn formal(self) -> DeclId {
        self.parameter().decl_id
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

    pub(crate) fn finish(self) -> Result<(), String> {
        if self.remaining == 0 {
            Ok(())
        } else {
            Err(format!(
                "semantic call construction left {} type-fact rows unconsumed",
                self.remaining,
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
    parameter: &CheckedParameter,
    name: &str,
    kind: CheckedParameterKind,
) -> Result<(), String> {
    if parameter.name != name || parameter.kind != kind {
        return Err(format!(
            "checked call {} entry {entry} differs from formal {} `{}`",
            call.0, parameter.decl_id.0, parameter.name,
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

#[cfg(debug_assertions)]
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
            left_parameter.decl_id == right_parameter.decl_id
                && left_parameter.name == right_parameter.name
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
            left_parameter.decl_id == right_parameter.decl_id
                && left_parameter.name == right_parameter.name
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
            left_parameter.decl_id == right_parameter.decl_id
                && left_parameter.name == right_parameter.name
                && left_target == right_target
                && left_name == right_name
        }
        _ => false,
    }
}

fn type_contains_variable(ty: &Type, variable: TypeVar) -> bool {
    match ty {
        Type::Var(candidate) => *candidate == variable,
        Type::List(item) | Type::Set(item) => type_contains_variable(item, variable),
        Type::Map { key, value } => {
            type_contains_variable(key, variable) || type_contains_variable(value, variable)
        }
        Type::Union(members) => members
            .iter()
            .any(|member| type_contains_variable(member, variable)),
        Type::Function { args, result } => {
            args.iter()
                .any(|argument| type_contains_variable(argument, variable))
                || type_contains_variable(&result.ty, variable)
        }
        Type::Object(shape) => shape
            .fields
            .values()
            .any(|field| type_contains_variable(field, variable)),
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields
                .fields
                .values()
                .any(|field| type_contains_variable(field, variable)),
        }),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => false,
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
}
