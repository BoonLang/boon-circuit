//! Packed derivation of checked list-order metadata.
//!
//! Order analysis borrows the permanent owner/fact/type columns directly. It
//! never constructs rich checked expressions, declarations, callables, or
//! source rows. Invocation state is a compact parent-linked frame arena; text
//! leaves borrow the existing text/literal authorities and paths retain
//! `SymbolId`s until the final checked output boundary.

use super::*;
use boon_checked::{
    CheckedCallOrderChain, CheckedOrderChain, CheckedOrderDirection, CheckedOrderKey,
    CheckedTypeSubstitution, apply_checked_type_substitutions_once,
};
use boon_contract::SymbolId;
use boon_data::{Bits, ExactNumber};
use std::cmp::Reverse;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedOrderDiagnostic {
    pub span: CheckedSpan,
    pub kind: KernelCheckedOrderDiagnosticKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelCheckedOrderDiagnosticKind {
    MissingPrecedingSort,
    UnsupportedKeyType { key_type: Type },
    ImpureKey,
    PartialKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedOrderDerivation {
    pub chains: Box<[CheckedCallOrderChain]>,
    pub diagnostics: Box<[KernelCheckedOrderDiagnostic]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedOrderFrameId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedOrderFrame {
    parent: Option<PackedOrderFrameId>,
    call: CheckedCallId,
    target_owner: KernelOwnerId,
    target_root_statement: crate::KernelStatementId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PackedOrderWalkKind {
    State,
    Semantic,
    Total,
    Pure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedOrderVisit {
    kind: PackedOrderWalkKind,
    expression: CheckedExprId,
    frame: Option<PackedOrderFrameId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedOrderLocalLocator {
    owner: KernelOwnerId,
    ordinal: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedOrderSourceRoute {
    anchor: DeclId,
    source: CheckedSourceId,
    expression: CheckedExprId,
    symbol_start: u32,
    symbol_len: u32,
}

#[derive(Clone, Copy)]
struct PackedOrderExpressionRef<'a> {
    owner: KernelOwnerId,
    local: crate::KernelExpressionId,
    definition: KernelDefinitionRef<'a>,
    node: &'a crate::PackedKernelOwnerNode,
    facts: crate::PackedDefinitionFactsRef<'a>,
    presentation: &'a crate::PackedExpressionPresentation,
    payload: crate::PackedExpressionPayload,
    shape: Option<&'a crate::PackedExecutionShape>,
    lexical: Option<&'a crate::PackedLexicalBinding>,
    call: Option<KernelCheckedPackedCallRef<'a>>,
}

impl<'a> PackedOrderExpressionRef<'a> {
    fn inputs(self) -> &'a [crate::PackedKernelOwnerInputEdge] {
        self.node.inputs(self.definition.input())
    }
}

#[derive(Clone, Copy)]
struct PackedOrderDeclarationRef<'a> {
    owner: KernelOwnerId,
    local: crate::KernelDeclarationId,
    definition: KernelDefinitionRef<'a>,
    row: &'a crate::PackedDeclaration,
    presentation: &'a crate::PackedDeclarationPresentation,
}

#[derive(Clone, Copy)]
struct PackedOrderParameterRef<'a> {
    name: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderState<'a> {
    Ordered(AnalyzedOrderChain<'a>),
    Unordered,
    Deferred,
    Invalid { call_path: Vec<CheckedCallId> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AnalyzedOrderChain<'a> {
    checked: CheckedOrderChain,
    semantic: Vec<PackedOrderSemanticKey<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackedOrderSemanticKey<'a> {
    key: Option<PackedOrderSemanticExpression<'a>>,
    direction: PackedOrderSemanticDirection<'a>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderSemanticDirection<'a> {
    Ascending,
    Descending,
    Dynamic(Option<PackedOrderSemanticExpression<'a>>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderSemanticFunction<'a> {
    Named(&'a str),
    Latest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderSemanticInputName<'a> {
    Named(&'a str),
    Index(u32),
    Passed,
}

/// Temporary structural value used only to compare order keys across branches.
/// Text and numeric leaves borrow packed storage; identifier paths retain
/// global `SymbolId`s rather than cloning strings.
#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderSemanticExpression<'a> {
    Row(Vec<SymbolId>),
    Capture {
        path: Vec<SymbolId>,
        projection: Vec<SymbolId>,
    },
    Project {
        input: Box<PackedOrderSemanticExpression<'a>>,
        fields: Vec<SymbolId>,
    },
    Text(&'a str),
    TextTemplate(Vec<PackedOrderSemanticTextSegment<'a>>),
    Number(&'a ExactNumber),
    Bits(&'a Bits),
    Tag(&'a str),
    Call {
        function: PackedOrderSemanticFunction<'a>,
        inputs: Vec<(
            PackedOrderSemanticInputName<'a>,
            PackedOrderSemanticExpression<'a>,
        )>,
    },
    Infix {
        operator: &'a str,
        left: Box<PackedOrderSemanticExpression<'a>>,
        right: Box<PackedOrderSemanticExpression<'a>>,
    },
    Select {
        input: Box<PackedOrderSemanticExpression<'a>>,
        outputs: Vec<PackedOrderSemanticExpression<'a>>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderSemanticTextSegment<'a> {
    Static(&'a str),
    Dynamic(PackedOrderSemanticExpression<'a>),
}

struct PackedOrderAnalyzer<'a> {
    input: &'a KernelSemanticInputConstructionV1,
    abi: &'a KernelAbiInput,
    layout: &'a KernelCheckedLinkLayout,
    snapshot: &'a KernelCheckedSnapshot,
    lexical_by_expression: Box<[Option<PackedOrderLocalLocator>]>,
    shape_by_expression: Box<[Option<PackedOrderLocalLocator>]>,
    call_by_expression: Box<[Option<CheckedCallId>]>,
    source_routes: Vec<PackedOrderSourceRoute>,
    source_route_symbols: Vec<SymbolId>,
    frames: Vec<PackedOrderFrame>,
    active: Vec<PackedOrderVisit>,
    type_cache: DefinitionTypeMaterializationCache,
    substitutions: Vec<Option<Box<[CheckedTypeSubstitution]>>>,
}

impl KernelCheckedRows {
    /// Derive order chains without reading any rich row in `self`.
    pub fn derive_packed_order_chains(
        &self,
        project: &KernelProjectInput,
        layout: &KernelCheckedLinkLayout,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<KernelCheckedOrderDerivation, KernelCheckedLinkError> {
        self.semantic_input
            .derive_packed_order_chains(project, layout, snapshot)
    }
}

impl KernelRuntimePackedLinkV1 {
    /// Derive order metadata directly from the packed runtime linker product.
    pub fn derive_packed_order_chains(
        &self,
        project: &KernelProjectInput,
        layout: &KernelCheckedLinkLayout,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<KernelCheckedOrderDerivation, KernelCheckedLinkError> {
        self.semantic_input
            .derive_packed_order_chains(project, layout, snapshot)
    }
}

impl KernelSemanticInputConstructionV1 {
    fn derive_packed_order_chains(
        &self,
        project: &KernelProjectInput,
        layout: &KernelCheckedLinkLayout,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<KernelCheckedOrderDerivation, KernelCheckedLinkError> {
        PackedOrderAnalyzer::new(self, project, layout, snapshot)?.derive()
    }
}

impl<'a> PackedOrderAnalyzer<'a> {
    fn new(
        input: &'a KernelSemanticInputConstructionV1,
        project: &'a KernelProjectInput,
        layout: &'a KernelCheckedLinkLayout,
        snapshot: &'a KernelCheckedSnapshot,
    ) -> Result<Self, KernelCheckedLinkError> {
        if !Arc::ptr_eq(&input.definition_code, &snapshot.definition_code)
            || !Arc::ptr_eq(&input.program, &snapshot.program)
            || !std::ptr::eq(project.program(), snapshot.program.as_ref())
        {
            return Err(KernelCheckedLinkError::new(
                "packed order derivation cannot combine foreign checked authorities",
            ));
        }
        if input.definition_count != snapshot.definition_count()
            || input.expression_count != layout.totals.expressions
            || input.call_count != layout.totals.calls
        {
            return Err(KernelCheckedLinkError::new(
                "packed order derivation received mismatched dense link ranges",
            ));
        }

        let expression_count = input.expression_count as usize;
        let mut lexical_by_expression = vec![None; expression_count];
        let mut shape_by_expression = vec![None; expression_count];
        let mut call_by_expression = vec![None; expression_count];
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            let facts = definition.runtime_facts();
            for (ordinal, binding) in facts.lexical_bindings().iter().enumerate() {
                let expression = input
                    .relocate_expression(crate::PackedExpressionRef::new(owner, binding.expression))
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "packed order lexical binding references missing expression {}:{}",
                            owner.0, binding.expression.0,
                        ))
                    })?;
                let slot = lexical_by_expression
                    .get_mut(expression.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "packed order lexical expression is outside its dense range",
                        )
                    })?;
                if slot
                    .replace(PackedOrderLocalLocator {
                        owner,
                        ordinal: u32::try_from(ordinal).map_err(|_| {
                            KernelCheckedLinkError::new(
                                "packed order lexical-binding count exceeds u32",
                            )
                        })?,
                    })
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed order repeats lexical binding for expression {}",
                        expression.0,
                    )));
                }
            }
            for (ordinal, shape) in facts.execution_shapes().iter().enumerate() {
                let expression = input
                    .relocate_expression(crate::PackedExpressionRef::new(owner, shape.expression()))
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "packed order execution shape references missing expression {}:{}",
                            owner.0,
                            shape.expression().0,
                        ))
                    })?;
                let slot = shape_by_expression
                    .get_mut(expression.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "packed order shaped expression is outside its dense range",
                        )
                    })?;
                if slot
                    .replace(PackedOrderLocalLocator {
                        owner,
                        ordinal: u32::try_from(ordinal).map_err(|_| {
                            KernelCheckedLinkError::new(
                                "packed order execution-shape count exceeds u32",
                            )
                        })?,
                    })
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed order repeats execution shape for expression {}",
                        expression.0,
                    )));
                }
            }
        }
        layout.for_each_packed_call(snapshot, |call| {
            let id = call.id()?;
            let expression = call.expression()?;
            let slot = call_by_expression
                .get_mut(expression.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed order call {} references missing expression {}",
                        id.0, expression.0,
                    ))
                })?;
            if slot.replace(id).is_some() {
                return Err(KernelCheckedLinkError::new(format!(
                    "packed order repeats call expression {}",
                    expression.0,
                )));
            }
            Ok(())
        })?;

        let mut analyzer = Self {
            input,
            abi: project.abi(),
            layout,
            snapshot,
            lexical_by_expression: lexical_by_expression.into_boxed_slice(),
            shape_by_expression: shape_by_expression.into_boxed_slice(),
            call_by_expression: call_by_expression.into_boxed_slice(),
            source_routes: Vec::new(),
            source_route_symbols: Vec::new(),
            frames: Vec::new(),
            active: Vec::new(),
            type_cache: snapshot.definition_code.materialization_cache(),
            substitutions: (0..input.call_count).map(|_| None).collect(),
        };
        analyzer.build_source_routes()?;
        analyzer.validate()?;
        Ok(analyzer)
    }

    fn validate(&self) -> Result<(), KernelCheckedLinkError> {
        self.layout.for_each_packed_call(self.snapshot, |call| {
            let id = call.id()?;
            let expression = call.expression()?;
            if self.expression(expression).is_none() {
                return Err(KernelCheckedLinkError::new(format!(
                    "packed order call {} references missing packed expression {}",
                    id.0, expression.0,
                )));
            }
            let code = call.code()?;
            let symbol = code.call_function(call.ordinal as usize).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "packed order call {} has no function symbol",
                    id.0,
                ))
            })?;
            self.snapshot
                .definition_code
                .symbol(symbol)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed order call {} function is outside its text authority",
                        id.0,
                    ))
                })?;
            let local_expression = code
                .call_expression(call.ordinal as usize)
                .expect("packed call expression was validated by the linked view");
            code.base_expression(local_expression.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed order call {} expression has no base result flow",
                        id.0,
                    ))
                })?;
            let span = code.call_span(call.ordinal as usize).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "packed order call {} has no source span",
                    id.0,
                ))
            })?;
            self.input.rebase_span(call.owner, span).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "packed order call {} was analyzed before span relocation",
                    id.0,
                ))
            })?;
            call.context_binding()?;
            for entry in call.entries()? {
                if self.parameter(call, entry.parameter_ordinal()).is_none() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed order call {} entry references missing exact parameter ordinal {}",
                        id.0,
                        entry.parameter_ordinal(),
                    )));
                }
            }
            let target = call.target()?;
            let facts = code
                .call_type_substitutions(call.ordinal as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed order call {} has no substitution span",
                        id.0,
                    ))
                })?;
            let mut seen = Vec::with_capacity(facts.len());
            for substitution in facts {
                let linked = self.linked_type_parameter(target, substitution.variable)?;
                if seen.contains(&linked) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed order call {} repeats target type parameter {}",
                        id.0, linked.0,
                    )));
                }
                seen.push(linked);
            }
            Ok(())
        })
    }

    fn derive(mut self) -> Result<KernelCheckedOrderDerivation, KernelCheckedLinkError> {
        let mut chains = Vec::new();
        let mut diagnostics = Vec::new();
        for ordinal in 0..self.input.call_count {
            let id = CheckedCallId(ordinal);
            let call = self.layout.packed_call(self.snapshot, id)?;
            match self.call_state(call, None) {
                PackedOrderState::Invalid { call_path } => {
                    diagnostics.push(KernelCheckedOrderDiagnostic {
                        span: self.order_chain_diagnostic_span(&call_path, self.call_span(call)),
                        kind: KernelCheckedOrderDiagnosticKind::MissingPrecedingSort,
                    });
                }
                PackedOrderState::Ordered(chain) => {
                    for key in &chain.checked.keys {
                        let span = self.order_chain_diagnostic_span(
                            &key.call_path,
                            self.expression_span(key.key)
                                .unwrap_or_else(|| self.call_span(call)),
                        );
                        if !type_is_deferred_order_key(&key.key_type)
                            && !type_is_orderable_key(&key.key_type)
                        {
                            diagnostics.push(KernelCheckedOrderDiagnostic {
                                span,
                                kind: KernelCheckedOrderDiagnosticKind::UnsupportedKeyType {
                                    key_type: key.key_type.clone(),
                                },
                            });
                        }
                        if !key.pure {
                            diagnostics.push(KernelCheckedOrderDiagnostic {
                                span,
                                kind: KernelCheckedOrderDiagnosticKind::ImpureKey,
                            });
                        }
                        if !key.total {
                            diagnostics.push(KernelCheckedOrderDiagnostic {
                                span,
                                kind: KernelCheckedOrderDiagnosticKind::PartialKey,
                            });
                        }
                    }
                    chains.push(CheckedCallOrderChain {
                        call: id,
                        chain: chain.checked,
                    });
                }
                PackedOrderState::Unordered | PackedOrderState::Deferred => {}
            }
        }
        debug_assert!(self.frames.is_empty());
        debug_assert!(self.active.is_empty());
        chains.sort_by_key(|entry| entry.call);
        Ok(KernelCheckedOrderDerivation {
            chains: chains.into_boxed_slice(),
            diagnostics: diagnostics.into_boxed_slice(),
        })
    }

    fn expression(&self, id: CheckedExprId) -> Option<PackedOrderExpressionRef<'a>> {
        let local = self.input.local_expression(id)?;
        let owner = local.owner();
        let expression = local.expression();
        let definition = self.snapshot.definition(owner)?;
        let facts = definition.runtime_facts();
        let node = definition.input().node(expression)?;
        let presentation = facts
            .expression_presentations()
            .get(expression.0 as usize)
            .filter(|row| row.expression == expression)?;
        let payload = *facts.expression_payloads().get(expression.0 as usize)?;
        let lexical = self
            .lexical_by_expression
            .get(id.0 as usize)
            .copied()
            .flatten()
            .and_then(|locator| {
                (locator.owner == owner)
                    .then(|| facts.lexical_bindings().get(locator.ordinal as usize))
                    .flatten()
            });
        let shape = self
            .shape_by_expression
            .get(id.0 as usize)
            .copied()
            .flatten()
            .and_then(|locator| {
                (locator.owner == owner)
                    .then(|| facts.execution_shapes().get(locator.ordinal as usize))
                    .flatten()
            });
        let call = self
            .call_by_expression
            .get(id.0 as usize)
            .copied()
            .flatten()
            .and_then(|call| self.call(call));
        Some(PackedOrderExpressionRef {
            owner,
            local: expression,
            definition,
            node,
            facts,
            presentation,
            payload,
            shape,
            lexical,
            call,
        })
    }

    fn declaration(&self, id: DeclId) -> Option<PackedOrderDeclarationRef<'a>> {
        let (owner, local) = self.input.local_declaration(id)?;
        let local = crate::KernelDeclarationId(local);
        let definition = self.snapshot.definition(owner)?;
        let facts = definition.runtime_facts();
        let row = facts
            .declarations()
            .get(local.0 as usize)
            .filter(|row| row.id == local)?;
        let presentation = facts
            .declaration_presentations()
            .get(local.0 as usize)
            .filter(|row| row.declaration == local)?;
        Some(PackedOrderDeclarationRef {
            owner,
            local,
            definition,
            row,
            presentation,
        })
    }

    fn call(&self, id: CheckedCallId) -> Option<KernelCheckedPackedCallRef<'a>> {
        self.layout.packed_call(self.snapshot, id).ok()
    }

    fn parameter(
        &self,
        call: KernelCheckedPackedCallRef<'a>,
        ordinal: u32,
    ) -> Option<PackedOrderParameterRef<'a>> {
        match call.target().ok()? {
            crate::KernelCallableSchemeId::User(owner) => {
                let definition = self.snapshot.definition(owner)?;
                let root = definition.linkage().root_statement?;
                let mut matches =
                    definition
                        .runtime_facts()
                        .declarations()
                        .iter()
                        .filter(|declaration| {
                            matches!(
                                declaration.origin,
                                crate::KernelDeclarationOrigin::Parameter {
                                    statement,
                                    ordinal: candidate,
                                } if statement == root && candidate == ordinal
                            )
                        });
                let declaration = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                Some(PackedOrderParameterRef {
                    name: definition.input().symbol(declaration.name)?,
                })
            }
            crate::KernelCallableSchemeId::Abi(callable) => {
                let parameter = self
                    .abi
                    .callable_by_id(callable)?
                    .parameters
                    .get(ordinal as usize)
                    .filter(|parameter| parameter.ordinal == ordinal)?;
                Some(PackedOrderParameterRef {
                    name: parameter.name.as_ref(),
                })
            }
        }
    }

    fn input(&self, call: KernelCheckedPackedCallRef<'a>, name: &str) -> Option<CheckedExprId> {
        call.entries().ok()?.iter().find_map(|entry| match entry {
            crate::PackedCallEntry::Input {
                parameter_ordinal,
                value,
                ..
            } if self
                .parameter(call, *parameter_ordinal)
                .is_some_and(|parameter| parameter.name == name) =>
            {
                self.input.relocate_value(call.owner, *value)
            }
            crate::PackedCallEntry::Input { .. }
            | crate::PackedCallEntry::FreshOut { .. }
            | crate::PackedCallEntry::ForwardOut { .. } => None,
        })
    }

    fn explicit_context_value(
        &self,
        call: KernelCheckedPackedCallRef<'a>,
    ) -> Option<CheckedExprId> {
        match call.context_binding().ok()? {
            crate::PackedCallContextBinding::Explicit { value, .. } => {
                self.input.relocate_value(call.owner, value)
            }
            crate::PackedCallContextBinding::None
            | crate::PackedCallContextBinding::Inherited { .. } => None,
        }
    }

    fn call_function(&self, call: KernelCheckedPackedCallRef<'a>) -> &'a str {
        let symbol = call
            .code()
            .expect("validated packed order call has code")
            .call_function(call.ordinal as usize)
            .expect("validated packed order call has a function symbol");
        self.snapshot
            .definition_code
            .symbol(symbol)
            .expect("validated packed order call function belongs to its text authority")
    }

    fn call_span(&self, call: KernelCheckedPackedCallRef<'a>) -> CheckedSpan {
        let span = call
            .code()
            .expect("validated packed order call has code")
            .call_span(call.ordinal as usize)
            .expect("validated packed order call has a span");
        self.input
            .rebase_span(call.owner, span)
            .expect("packed order derivation runs after span relocation")
    }

    fn expression_span(&self, expression: CheckedExprId) -> Option<CheckedSpan> {
        let expression = self.expression(expression)?;
        self.input
            .rebase_span(expression.owner, expression.presentation.span.materialize())
    }

    fn call_may_return_ordered_list(&self, call: KernelCheckedPackedCallRef<'a>) -> bool {
        let code = call.code().expect("validated packed order call has code");
        let expression = code
            .call_expression(call.ordinal as usize)
            .expect("validated packed order call has an expression");
        let result = code
            .base_expression(expression.0 as usize)
            .expect("validated packed order call has a base result");
        self.packed_type_may_be_ordered_list(result.term)
    }

    fn packed_type_may_be_ordered_list(&self, term: crate::TypeTermId) -> bool {
        match self
            .snapshot
            .definition_code
            .type_store()
            .as_arena()
            .term(term)
        {
            crate::TypeTerm::List(_)
            | crate::TypeTerm::Variable(_)
            | crate::TypeTerm::Unknown
            | crate::TypeTerm::UnresolvedShape(_) => true,
            crate::TypeTerm::Union(members) => members
                .iter()
                .copied()
                .any(|member| self.packed_type_may_be_ordered_list(member)),
            crate::TypeTerm::Text
            | crate::TypeTerm::Number
            | crate::TypeTerm::Bytes(_)
            | crate::TypeTerm::Absent
            | crate::TypeTerm::VariantSet(_)
            | crate::TypeTerm::Object { .. }
            | crate::TypeTerm::OpenObjectPlaceholder
            | crate::TypeTerm::RenderContract
            | crate::TypeTerm::Function { .. }
            | crate::TypeTerm::Map { .. }
            | crate::TypeTerm::Set(_)
            | crate::TypeTerm::Bits(_) => false,
        }
    }

    fn linked_type_parameter(
        &self,
        target: crate::KernelCallableSchemeId,
        parameter: crate::KernelTypeParameterId,
    ) -> Result<TypeVar, KernelCheckedLinkError> {
        match target {
            crate::KernelCallableSchemeId::User(owner) => {
                let definition = self.layout.definition(owner)?;
                let code = self
                    .snapshot
                    .definition_code
                    .definition(owner)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "packed order target definition {} has no callable scheme",
                            owner.0,
                        ))
                    })?;
                let parameter = code
                    .callable_type_parameters()
                    .get(parameter.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "packed order target definition {} has no type parameter {}",
                            owner.0, parameter.0,
                        ))
                    })?;
                Ok(TypeVar(definition.type_variables.resolve(
                    parameter.linked_local,
                    "packed order target type parameter",
                )?))
            }
            crate::KernelCallableSchemeId::Abi(callable) => {
                let definition = self.layout.abi_callable(callable)?;
                let scheme = self
                    .snapshot
                    .definition_code
                    .abi_callable_scheme(callable)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "packed order ABI target {} has no callable scheme",
                            callable.0,
                        ))
                    })?;
                let parameter = scheme
                    .type_parameters()
                    .get(parameter.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "packed order ABI target {} has no type parameter {}",
                            callable.0, parameter.0,
                        ))
                    })?;
                Ok(TypeVar(definition.type_variables.resolve(
                    parameter.linked_local,
                    "packed order ABI target type parameter",
                )?))
            }
        }
    }

    fn materialized_substitutions(
        &mut self,
        call: KernelCheckedPackedCallRef<'a>,
    ) -> &[CheckedTypeSubstitution] {
        let id = call.id().expect("validated packed order call has an ID");
        if self.substitutions[id.0 as usize].is_none() {
            let code = call.code().expect("validated packed order call has code");
            let alpha_start = self
                .layout
                .definition(call.owner)
                .expect("validated packed order call owner has a layout")
                .type_variables
                .start;
            let facts = {
                let mut materializer = code.linked_materializer(&mut self.type_cache, alpha_start);
                materializer
                    .materialize_call_facts(call.ordinal as usize)
                    .expect("validated packed order call has type facts")
            };
            let target = call
                .target()
                .expect("validated packed order call has a target scheme");
            let substitutions = facts
                .substitutions
                .into_vec()
                .into_iter()
                .map(|substitution| CheckedTypeSubstitution {
                    variable: self
                        .linked_type_parameter(target, substitution.variable)
                        .expect("validated packed order substitution has a linked target"),
                    value: substitution.value,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            self.substitutions[id.0 as usize] = Some(substitutions);
        }
        self.substitutions[id.0 as usize]
            .as_deref()
            .expect("packed order substitution cache was populated")
    }

    fn materialized_expression_type(&mut self, expression: CheckedExprId) -> Type {
        let Some(local) = self.input.local_expression(expression) else {
            return Type::Unknown;
        };
        let Some(code) = self.snapshot.definition_code.definition(local.owner()) else {
            return Type::Unknown;
        };
        let Ok(layout) = self.layout.definition(local.owner()) else {
            return Type::Unknown;
        };
        code.linked_materializer(&mut self.type_cache, layout.type_variables.start)
            .materialize_published_expression(local.expression().0 as usize)
            .map(|flow| flow.ty)
            .unwrap_or(Type::Unknown)
    }

    fn push_user_frame(
        &mut self,
        parent: Option<PackedOrderFrameId>,
        call: KernelCheckedPackedCallRef<'a>,
        target_owner: KernelOwnerId,
    ) -> PackedOrderFrameId {
        let target_root_statement = self
            .snapshot
            .definition(target_owner)
            .and_then(|definition| definition.linkage().root_statement)
            .expect("validated user-call target has a root statement");
        let id = PackedOrderFrameId(
            u32::try_from(self.frames.len()).expect("packed order frame depth exceeds u32"),
        );
        self.frames.push(PackedOrderFrame {
            parent,
            call: call.id().expect("validated packed call has an ID"),
            target_owner,
            target_root_statement,
        });
        id
    }

    fn pop_user_frame(&mut self, id: PackedOrderFrameId) {
        debug_assert_eq!(self.frames.len(), id.0 as usize + 1);
        self.frames.pop();
    }

    fn frame(&self, id: PackedOrderFrameId) -> PackedOrderFrame {
        *self
            .frames
            .get(id.0 as usize)
            .expect("active packed order frame ID remains in range")
    }

    fn frame_contains_owner(
        &self,
        mut frame: Option<PackedOrderFrameId>,
        owner: KernelOwnerId,
    ) -> bool {
        while let Some(id) = frame {
            let row = self.frame(id);
            if row.target_owner == owner {
                return true;
            }
            frame = row.parent;
        }
        false
    }

    fn frame_actual(
        &self,
        target: DeclId,
        mut frame: Option<PackedOrderFrameId>,
    ) -> Option<(CheckedExprId, Option<PackedOrderFrameId>)> {
        let declaration = self.declaration(target)?;
        let crate::KernelDeclarationOrigin::Parameter { statement, ordinal } =
            declaration.row.origin
        else {
            return None;
        };
        while let Some(id) = frame {
            let row = self.frame(id);
            if row.target_owner == declaration.owner && row.target_root_statement == statement {
                let call = self.call(row.call)?;
                let value = call.entries().ok()?.iter().find_map(|entry| match entry {
                    crate::PackedCallEntry::Input {
                        parameter_ordinal,
                        value,
                        ..
                    } if *parameter_ordinal == ordinal => {
                        self.input.relocate_value(call.owner, *value)
                    }
                    crate::PackedCallEntry::Input { .. }
                    | crate::PackedCallEntry::FreshOut { .. }
                    | crate::PackedCallEntry::ForwardOut { .. } => None,
                })?;
                return Some((value, row.parent));
            }
            frame = row.parent;
        }
        None
    }

    fn call_path(
        &self,
        mut frame: Option<PackedOrderFrameId>,
        terminal: CheckedCallId,
    ) -> Vec<CheckedCallId> {
        let mut path = Vec::new();
        while let Some(id) = frame {
            let row = self.frame(id);
            path.push(row.call);
            frame = row.parent;
        }
        path.reverse();
        path.push(terminal);
        path
    }

    fn begin_visit(
        &mut self,
        kind: PackedOrderWalkKind,
        expression: CheckedExprId,
        frame: Option<PackedOrderFrameId>,
    ) -> bool {
        let visit = PackedOrderVisit {
            kind,
            expression,
            frame,
        };
        if self.active.contains(&visit) {
            return false;
        }
        self.active.push(visit);
        true
    }

    fn end_visit(
        &mut self,
        kind: PackedOrderWalkKind,
        expression: CheckedExprId,
        frame: Option<PackedOrderFrameId>,
    ) {
        debug_assert_eq!(
            self.active.pop(),
            Some(PackedOrderVisit {
                kind,
                expression,
                frame,
            })
        );
    }

    fn linked_value(
        &self,
        expression: PackedOrderExpressionRef<'a>,
        encoded: crate::KernelExpressionId,
    ) -> Option<CheckedExprId> {
        let value = expression
            .definition
            .resolve_value(encoded, expression.local.0 as usize)
            .ok()?;
        self.input.relocate_value(expression.owner, value)
    }

    fn exact_input(
        &self,
        expression: PackedOrderExpressionRef<'a>,
        role: crate::PackedKernelOwnerEdgeRole,
    ) -> Option<CheckedExprId> {
        let mut inputs = expression
            .inputs()
            .iter()
            .filter(|input| input.role == role);
        let input = inputs.next()?;
        if inputs.next().is_some() {
            return None;
        }
        self.linked_value(expression, input.expression)
    }

    fn declaration_value(
        &self,
        declaration: PackedOrderDeclarationRef<'a>,
    ) -> Option<CheckedExprId> {
        let value = declaration.row.value?;
        let value = declaration
            .definition
            .resolve_value(value, declaration.local.0 as usize)
            .ok()?;
        self.input.relocate_value(declaration.owner, value)
    }

    fn path_symbols(&self, path: PathId) -> Option<Vec<SymbolId>> {
        Some(self.snapshot.program.path(path)?.iter().collect())
    }

    fn path_is_empty(&self, path: PathId) -> bool {
        self.snapshot
            .program
            .path(path)
            .is_some_and(|path| path.is_empty())
    }

    fn build_source_routes(&mut self) -> Result<(), KernelCheckedLinkError> {
        let snapshot = self.snapshot;
        let mut alias_projection = Vec::new();
        let mut seen_anchors = Vec::new();
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for source in definition.runtime_facts().sources() {
                let source_id = self.layout.source(owner, source.id.0)?;
                let source_declaration = self.layout.declaration(owner, source.declaration)?;
                let source_expression = self
                    .layout
                    .expression(owner, KernelValueReference::Local(source.expression))?;
                let source_projection = self.path_symbols(source.projection).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed order SOURCE {}:{} references a foreign projection",
                        owner.0, source.id.0,
                    ))
                })?;
                self.push_source_route(
                    source_declaration,
                    source_id,
                    source_expression,
                    &source_projection,
                )?;
                let source_row = self.declaration(source_declaration).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed order SOURCE declaration {} has no packed row",
                        source_declaration.0,
                    ))
                })?;
                alias_projection.clear();
                alias_projection.push(source_row.row.name);
                alias_projection.extend(source_projection.iter().copied());
                seen_anchors.clear();
                let mut scope_owner = source_row.owner;
                let mut scope = source_row.presentation.scope;
                while let Some(anchor) =
                    self.layout
                        .lexical_declaration_for_scope(snapshot, scope_owner, scope)?
                {
                    if seen_anchors.contains(&anchor) || anchor == source_declaration {
                        break;
                    }
                    seen_anchors.push(anchor);
                    self.push_source_route(
                        anchor,
                        source_id,
                        source_expression,
                        &alias_projection,
                    )?;
                    let Some(parent) = self.declaration(anchor) else {
                        break;
                    };
                    alias_projection.insert(0, parent.row.name);
                    scope_owner = parent.owner;
                    scope = parent.presentation.scope;
                }
            }
        }
        self.source_routes
            .sort_unstable_by_key(|route| (route.anchor, Reverse(route.symbol_len), route.source));
        Ok(())
    }

    fn push_source_route(
        &mut self,
        anchor: DeclId,
        source: CheckedSourceId,
        expression: CheckedExprId,
        symbols: &[SymbolId],
    ) -> Result<(), KernelCheckedLinkError> {
        let symbol_start = u32::try_from(self.source_route_symbols.len()).map_err(|_| {
            KernelCheckedLinkError::new("packed order SOURCE-route symbols exceed u32")
        })?;
        let symbol_len = u32::try_from(symbols.len()).map_err(|_| {
            KernelCheckedLinkError::new("packed order SOURCE-route path exceeds u32")
        })?;
        self.source_route_symbols.extend_from_slice(symbols);
        self.source_routes.push(PackedOrderSourceRoute {
            anchor,
            source,
            expression,
            symbol_start,
            symbol_len,
        });
        Ok(())
    }

    fn source_route_symbols(&self, route: PackedOrderSourceRoute) -> Option<&[SymbolId]> {
        let start = route.symbol_start as usize;
        let end = start.checked_add(route.symbol_len as usize)?;
        self.source_route_symbols.get(start..end)
    }

    fn source_expression_for_read(
        &self,
        target: DeclId,
        projection: PathId,
    ) -> Option<CheckedExprId> {
        let projection = self.snapshot.program.path(projection)?;
        let start = self
            .source_routes
            .partition_point(|route| route.anchor < target);
        let mut matched: Option<PackedOrderSourceRoute> = None;
        for route in self.source_routes[start..]
            .iter()
            .copied()
            .take_while(|route| route.anchor == target)
        {
            let symbols = self.source_route_symbols(route)?;
            if symbols.len() > projection.len()
                || !symbols
                    .iter()
                    .copied()
                    .zip(projection.iter())
                    .all(|(left, right)| left == right)
            {
                continue;
            }
            if let Some(previous) = matched {
                if previous.symbol_len == route.symbol_len {
                    return None;
                }
                break;
            }
            matched = Some(route);
        }
        matched.map(|route| route.expression)
    }

    fn read_provider(&self, target: DeclId, projection: PathId) -> Option<CheckedExprId> {
        self.declaration(target)
            .and_then(|declaration| self.declaration_value(declaration))
            .or_else(|| self.source_expression_for_read(target, projection))
    }

    fn pattern_binding(
        &self,
        declaration: DeclId,
    ) -> Option<KernelSemanticPatternBindingLocatorV1> {
        self.input
            .pattern_bindings
            .binary_search_by_key(&declaration, |binding| binding.declaration)
            .ok()
            .and_then(|index| self.input.pattern_bindings.get(index))
            .copied()
    }

    fn declaration_canonical_path(
        &self,
        declaration: PackedOrderDeclarationRef<'a>,
    ) -> Option<Vec<SymbolId>> {
        if !matches!(
            declaration.row.kind,
            crate::KernelDeclarationKind::Field
                | crate::KernelDeclarationKind::Source
                | crate::KernelDeclarationKind::Hold
                | crate::KernelDeclarationKind::List
        ) {
            return None;
        }
        let mut segments = vec![declaration.row.name];
        let mut owner = declaration.owner;
        let mut scope = declaration.presentation.scope;
        let mut remaining = (self.input.scope_count as usize)
            .saturating_add(self.input.definition_count)
            .saturating_add(1);
        loop {
            if remaining == 0 {
                return None;
            }
            remaining -= 1;
            match scope {
                KernelScopeReference::ProjectRoot => break,
                KernelScopeReference::Containing => {
                    let definition = self.snapshot.definition(owner)?;
                    scope = definition.runtime_facts().containing_scope();
                }
                KernelScopeReference::Owner {
                    owner: provider,
                    scope: provider_scope,
                } => {
                    owner = provider;
                    scope = KernelScopeReference::Local(provider_scope);
                }
                KernelScopeReference::Local(local) => {
                    let definition = self.snapshot.definition(owner)?;
                    let row = definition
                        .runtime_facts()
                        .scopes()
                        .get(local.0 as usize)
                        .filter(|row| row.id == local)?;
                    if row.kind == crate::KernelScopeKind::Function {
                        return None;
                    }
                    if let Some(anchor) = row.owner {
                        let anchor = self.layout.declaration(owner, anchor).ok()?;
                        if let Some(anchor) = self.declaration(anchor)
                            && matches!(
                                anchor.row.kind,
                                crate::KernelDeclarationKind::Field
                                    | crate::KernelDeclarationKind::Source
                                    | crate::KernelDeclarationKind::Hold
                                    | crate::KernelDeclarationKind::List
                            )
                        {
                            segments.push(anchor.row.name);
                        }
                    }
                    scope = row.parent;
                }
            }
        }
        segments.reverse();
        Some(segments)
    }

    fn expression_state(
        &mut self,
        expression: CheckedExprId,
        frame: Option<PackedOrderFrameId>,
    ) -> PackedOrderState<'a> {
        if !self.begin_visit(PackedOrderWalkKind::State, expression, frame) {
            return PackedOrderState::Deferred;
        }
        let result =
            self.expression(expression)
                .map_or(PackedOrderState::Unordered, |expression| {
                    if let Some(call) = expression.call {
                        return self.call_state(call, frame);
                    }
                    if let Some(binding) = expression.lexical
                        && binding.access == crate::KernelLexicalAccess::Read
                        && self.path_is_empty(binding.projection)
                        && let crate::KernelLexicalBindingTargetInput::Declaration(target) =
                            binding.target
                        && let Some(target) =
                            self.input.relocate_declaration(expression.owner, target)
                    {
                        if let Some((value, parent)) = self.frame_actual(target, frame) {
                            return self.expression_state(value, parent);
                        }
                        if let Some(value) = self.read_provider(target, binding.projection) {
                            return self.expression_state(value, frame);
                        }
                        return if self.declaration(target).is_some_and(|declaration| {
                            declaration.row.kind == crate::KernelDeclarationKind::ValueParameter
                        }) {
                            PackedOrderState::Deferred
                        } else {
                            PackedOrderState::Unordered
                        };
                    }
                    match expression.node.kind {
                        crate::PackedKernelOwnerNodeKind::Block => expression
                            .shape
                            .and_then(|shape| match shape {
                                crate::PackedExecutionShape::Block {
                                    result: Some(result),
                                    ..
                                } => self.linked_value(expression, *result),
                                _ => None,
                            })
                            .map_or(PackedOrderState::Unordered, |result| {
                                self.expression_state(result, frame)
                            }),
                        crate::PackedKernelOwnerNodeKind::Then => self
                            .exact_input(expression, crate::PackedKernelOwnerEdgeRole::ThenOutput)
                            .map_or(PackedOrderState::Unordered, |output| {
                                self.expression_state(output, frame)
                            }),
                        crate::PackedKernelOwnerNodeKind::MatchArm { .. } => self
                            .exact_input(expression, crate::PackedKernelOwnerEdgeRole::MatchOutput)
                            .map_or(PackedOrderState::Unordered, |output| {
                                self.expression_state(output, frame)
                            }),
                        crate::PackedKernelOwnerNodeKind::Latest => self.merge_branch_states(
                            expression,
                            crate::PackedKernelOwnerEdgeRole::LatestBranch,
                            frame,
                        ),
                        crate::PackedKernelOwnerNodeKind::When => self.merge_branch_states(
                            expression,
                            crate::PackedKernelOwnerEdgeRole::WhenArm,
                            frame,
                        ),
                        _ => PackedOrderState::Unordered,
                    }
                });
        self.end_visit(PackedOrderWalkKind::State, expression, frame);
        result
    }

    fn call_state(
        &mut self,
        call: KernelCheckedPackedCallRef<'a>,
        frame: Option<PackedOrderFrameId>,
    ) -> PackedOrderState<'a> {
        if !self.call_may_return_ordered_list(call) {
            return PackedOrderState::Unordered;
        }
        match self.call_function(call) {
            "List/sort_by" => {
                let Some(key) = self.input(call, "key") else {
                    return PackedOrderState::Unordered;
                };
                let (key, semantic) = self.order_key(call, key, frame);
                PackedOrderState::Ordered(AnalyzedOrderChain {
                    checked: CheckedOrderChain { keys: vec![key] },
                    semantic: vec![semantic],
                })
            }
            "List/then_by" => {
                let Some(list) = self.input(call, "list") else {
                    return PackedOrderState::Unordered;
                };
                match self.expression_state(list, frame) {
                    PackedOrderState::Ordered(mut chain) => {
                        let Some(key) = self.input(call, "key") else {
                            return PackedOrderState::Unordered;
                        };
                        let (key, semantic) = self.order_key(call, key, frame);
                        chain.checked.keys.push(key);
                        chain.semantic.push(semantic);
                        PackedOrderState::Ordered(chain)
                    }
                    PackedOrderState::Deferred => PackedOrderState::Deferred,
                    PackedOrderState::Invalid { call_path } => {
                        PackedOrderState::Invalid { call_path }
                    }
                    PackedOrderState::Unordered => PackedOrderState::Invalid {
                        call_path: self.call_path(
                            frame,
                            call.id().expect("validated packed order call has an ID"),
                        ),
                    },
                }
            }
            "List/filter" | "List/retain" | "List/remove" | "List/map" | "List/take"
            | "List/page" => self
                .input(call, "list")
                .map_or(PackedOrderState::Unordered, |list| {
                    self.expression_state(list, frame)
                }),
            _ => match call
                .target()
                .expect("validated packed order call has a target")
            {
                crate::KernelCallableSchemeId::Abi(_) => PackedOrderState::Unordered,
                crate::KernelCallableSchemeId::User(owner) => {
                    if self.frame_contains_owner(frame, owner) {
                        return PackedOrderState::Deferred;
                    }
                    let Some(result) = self
                        .input
                        .definition_relocation(owner)
                        .map(|relocation| relocation.result_expression)
                    else {
                        return PackedOrderState::Unordered;
                    };
                    let nested = self.push_user_frame(frame, call, owner);
                    let result = self.expression_state(result, Some(nested));
                    self.pop_user_frame(nested);
                    result
                }
            },
        }
    }

    fn order_key(
        &mut self,
        call: KernelCheckedPackedCallRef<'a>,
        key: CheckedExprId,
        frame: Option<PackedOrderFrameId>,
    ) -> (CheckedOrderKey, PackedOrderSemanticKey<'a>) {
        let mut key_type = self.materialized_expression_type(key);
        key_type =
            apply_checked_type_substitutions_once(&key_type, self.materialized_substitutions(call));
        let mut current = frame;
        while let Some(id) = current {
            let row = self.frame(id);
            if let Some(frame_call) = self.call(row.call) {
                key_type = apply_checked_type_substitutions_once(
                    &key_type,
                    self.materialized_substitutions(frame_call),
                );
            }
            current = row.parent;
        }
        let (direction, semantic_direction) = self.order_direction(call, frame);
        let id = call.id().expect("validated packed order call has an ID");
        let checked = CheckedOrderKey {
            call_path: self.call_path(frame, id),
            key,
            direction,
            key_type,
            pure: self.expression_is_pure(key, frame),
            total: self.expression_is_total(key, frame),
        };
        let semantic = PackedOrderSemanticKey {
            key: self.semantic_expression(key, frame),
            direction: semantic_direction,
        };
        (checked, semantic)
    }

    fn order_direction(
        &mut self,
        call: KernelCheckedPackedCallRef<'a>,
        frame: Option<PackedOrderFrameId>,
    ) -> (CheckedOrderDirection, PackedOrderSemanticDirection<'a>) {
        let Some(expression) = self.input(call, "direction") else {
            return (
                CheckedOrderDirection::Ascending,
                PackedOrderSemanticDirection::Ascending,
            );
        };
        let semantic = self.semantic_expression(expression, frame);
        match semantic {
            Some(PackedOrderSemanticExpression::Tag("Ascending")) => (
                CheckedOrderDirection::Ascending,
                PackedOrderSemanticDirection::Ascending,
            ),
            Some(PackedOrderSemanticExpression::Tag("Descending")) => (
                CheckedOrderDirection::Descending,
                PackedOrderSemanticDirection::Descending,
            ),
            semantic => (
                CheckedOrderDirection::Dynamic { expression },
                PackedOrderSemanticDirection::Dynamic(semantic),
            ),
        }
    }

    fn semantic_project(
        input: PackedOrderSemanticExpression<'a>,
        fields: Vec<SymbolId>,
    ) -> PackedOrderSemanticExpression<'a> {
        if fields.is_empty() {
            input
        } else {
            PackedOrderSemanticExpression::Project {
                input: Box::new(input),
                fields,
            }
        }
    }

    fn semantic_expression(
        &mut self,
        expression: CheckedExprId,
        frame: Option<PackedOrderFrameId>,
    ) -> Option<PackedOrderSemanticExpression<'a>> {
        if !self.begin_visit(PackedOrderWalkKind::Semantic, expression, frame) {
            return None;
        }
        let result = (|| {
            let expression = self.expression(expression)?;
            if let Some(binding) = expression.lexical {
                return match binding.target {
                    crate::KernelLexicalBindingTargetInput::Declaration(target)
                        if binding.access == crate::KernelLexicalAccess::Read =>
                    {
                        let target = self.input.relocate_declaration(expression.owner, target)?;
                        let projection = self.path_symbols(binding.projection)?;
                        if let Some((value, parent)) = self.frame_actual(target, frame) {
                            return self
                                .semantic_expression(value, parent)
                                .map(|value| Self::semantic_project(value, projection));
                        }
                        let declaration = self.declaration(target);
                        if declaration.is_some_and(|declaration| {
                            matches!(
                                declaration.row.kind,
                                crate::KernelDeclarationKind::OutParameter
                                    | crate::KernelDeclarationKind::FreshOut
                            )
                        }) {
                            return Some(PackedOrderSemanticExpression::Row(projection));
                        }
                        if let Some(binding) = self.pattern_binding(target) {
                            let mut fields = Vec::with_capacity(
                                projection
                                    .len()
                                    .saturating_add(usize::from(binding.projection.is_some())),
                            );
                            fields.extend(binding.projection);
                            fields.extend(projection);
                            return self
                                .semantic_expression(binding.selector, frame)
                                .map(|value| Self::semantic_project(value, fields));
                        }
                        let expanded = self
                            .read_provider(target, binding.projection)
                            .and_then(|value| self.semantic_expression(value, frame))
                            .map(|value| Self::semantic_project(value, projection.clone()));
                        expanded.or_else(|| {
                            declaration.and_then(|declaration| {
                                self.declaration_canonical_path(declaration).map(|path| {
                                    PackedOrderSemanticExpression::Capture { path, projection }
                                })
                            })
                        })
                    }
                    crate::KernelLexicalBindingTargetInput::RuntimeContext
                        if binding.access == crate::KernelLexicalAccess::Read =>
                    {
                        let crate::PackedExpressionPayload::LexicalPath(path) = expression.payload
                        else {
                            return None;
                        };
                        Some(PackedOrderSemanticExpression::Capture {
                            path: self.path_symbols(path)?,
                            projection: Vec::new(),
                        })
                    }
                    crate::KernelLexicalBindingTargetInput::Declaration(_)
                    | crate::KernelLexicalBindingTargetInput::ContextFormal { .. }
                    | crate::KernelLexicalBindingTargetInput::Value { .. }
                    | crate::KernelLexicalBindingTargetInput::RuntimeContext => None,
                };
            }
            if let Some(call) = expression.call {
                return match call
                    .target()
                    .expect("validated packed order call has a target")
                {
                    crate::KernelCallableSchemeId::User(owner) => {
                        if self.frame_contains_owner(frame, owner) {
                            return None;
                        }
                        let result = self.input.definition_relocation(owner)?.result_expression;
                        let nested = self.push_user_frame(frame, call, owner);
                        let result = self.semantic_expression(result, Some(nested));
                        self.pop_user_frame(nested);
                        result
                    }
                    crate::KernelCallableSchemeId::Abi(_) => {
                        let mut inputs = Vec::new();
                        for entry in call.entries().ok()? {
                            if let crate::PackedCallEntry::Input {
                                parameter_ordinal,
                                value,
                                ..
                            } = entry
                            {
                                let parameter = self.parameter(call, *parameter_ordinal)?;
                                let value = self.input.relocate_value(call.owner, *value)?;
                                inputs.push((
                                    PackedOrderSemanticInputName::Named(parameter.name),
                                    self.semantic_expression(value, frame)?,
                                ));
                            }
                        }
                        if let Some(value) = self.explicit_context_value(call) {
                            inputs.push((
                                PackedOrderSemanticInputName::Passed,
                                self.semantic_expression(value, frame)?,
                            ));
                        }
                        Some(PackedOrderSemanticExpression::Call {
                            function: PackedOrderSemanticFunction::Named(self.call_function(call)),
                            inputs,
                        })
                    }
                };
            }
            match expression.node.kind {
                crate::PackedKernelOwnerNodeKind::Text => {
                    let crate::PackedExpressionPayload::Text(value) = expression.payload else {
                        return None;
                    };
                    Some(PackedOrderSemanticExpression::Text(
                        expression.facts.literal_text(value),
                    ))
                }
                crate::PackedKernelOwnerNodeKind::TextTemplate => {
                    let segments = expression.facts.template_segments(expression.payload)?;
                    segments
                        .iter()
                        .map(|segment| match segment {
                            crate::PackedTextTemplateSegment::Static(value) => {
                                Some(PackedOrderSemanticTextSegment::Static(
                                    expression.facts.literal_text(*value),
                                ))
                            }
                            crate::PackedTextTemplateSegment::Dynamic(ordinal) => expression
                                .inputs()
                                .iter()
                                .filter(|input| {
                                    input.role == crate::PackedKernelOwnerEdgeRole::TextDynamic
                                })
                                .nth(*ordinal as usize)
                                .and_then(|input| self.linked_value(expression, input.expression))
                                .and_then(|value| self.semantic_expression(value, frame))
                                .map(PackedOrderSemanticTextSegment::Dynamic),
                        })
                        .collect::<Option<Vec<_>>>()
                        .map(PackedOrderSemanticExpression::TextTemplate)
                }
                crate::PackedKernelOwnerNodeKind::Number => {
                    let crate::PackedExpressionPayload::Number(value) = expression.payload else {
                        return None;
                    };
                    Some(PackedOrderSemanticExpression::Number(
                        expression.facts.number(value),
                    ))
                }
                crate::PackedKernelOwnerNodeKind::Bits(_) => {
                    let crate::PackedExpressionPayload::Bits(value) = expression.payload else {
                        return None;
                    };
                    Some(PackedOrderSemanticExpression::Bits(
                        expression.facts.bits(value),
                    ))
                }
                crate::PackedKernelOwnerNodeKind::Tag(name) => Some(
                    PackedOrderSemanticExpression::Tag(expression.definition.input().symbol(name)?),
                ),
                crate::PackedKernelOwnerNodeKind::Infix { operation } => self
                    .exact_input(expression, crate::PackedKernelOwnerEdgeRole::InfixLeft)
                    .and_then(|left| self.semantic_expression(left, frame))
                    .zip(
                        self.exact_input(expression, crate::PackedKernelOwnerEdgeRole::InfixRight)
                            .and_then(|right| self.semantic_expression(right, frame)),
                    )
                    .map(|(left, right)| PackedOrderSemanticExpression::Infix {
                        operator: expression
                            .definition
                            .input()
                            .symbol(operation)
                            .unwrap_or(""),
                        left: Box::new(left),
                        right: Box::new(right),
                    }),
                crate::PackedKernelOwnerNodeKind::Block => expression
                    .shape
                    .and_then(|shape| match shape {
                        crate::PackedExecutionShape::Block {
                            result: Some(result),
                            ..
                        } => self.linked_value(expression, *result),
                        _ => None,
                    })
                    .and_then(|result| self.semantic_expression(result, frame)),
                crate::PackedKernelOwnerNodeKind::Then => self
                    .exact_input(expression, crate::PackedKernelOwnerEdgeRole::ThenOutput)
                    .and_then(|output| self.semantic_expression(output, frame)),
                crate::PackedKernelOwnerNodeKind::MatchArm { .. } => self
                    .exact_input(expression, crate::PackedKernelOwnerEdgeRole::MatchOutput)
                    .and_then(|output| self.semantic_expression(output, frame)),
                crate::PackedKernelOwnerNodeKind::Latest => {
                    let mut inputs = Vec::new();
                    for (index, input) in expression
                        .inputs()
                        .iter()
                        .filter(|input| {
                            input.role == crate::PackedKernelOwnerEdgeRole::LatestBranch
                        })
                        .enumerate()
                    {
                        let value = self.linked_value(expression, input.expression)?;
                        inputs.push((
                            PackedOrderSemanticInputName::Index(u32::try_from(index).ok()?),
                            self.semantic_expression(value, frame)?,
                        ));
                    }
                    Some(PackedOrderSemanticExpression::Call {
                        function: PackedOrderSemanticFunction::Latest,
                        inputs,
                    })
                }
                crate::PackedKernelOwnerNodeKind::When => {
                    let input =
                        self.exact_input(expression, crate::PackedKernelOwnerEdgeRole::WhenInput)?;
                    let input = self.semantic_expression(input, frame)?;
                    let outputs = expression
                        .inputs()
                        .iter()
                        .filter(|edge| edge.role == crate::PackedKernelOwnerEdgeRole::WhenArm)
                        .map(|edge| {
                            self.linked_value(expression, edge.expression)
                                .and_then(|arm| self.semantic_expression(arm, frame))
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(PackedOrderSemanticExpression::Select {
                        input: Box::new(input),
                        outputs,
                    })
                }
                crate::PackedKernelOwnerNodeKind::Source(_)
                | crate::PackedKernelOwnerNodeKind::Absent
                | crate::PackedKernelOwnerNodeKind::Byte
                | crate::PackedKernelOwnerNodeKind::Record { .. }
                | crate::PackedKernelOwnerNodeKind::Collection { .. }
                | crate::PackedKernelOwnerNodeKind::MapEntry
                | crate::PackedKernelOwnerNodeKind::FormalRead { .. }
                | crate::PackedKernelOwnerNodeKind::ContextRead { .. }
                | crate::PackedKernelOwnerNodeKind::LexicalRead { .. }
                | crate::PackedKernelOwnerNodeKind::ValueRead { .. }
                | crate::PackedKernelOwnerNodeKind::DerivedRead { .. }
                | crate::PackedKernelOwnerNodeKind::PatternRead { .. }
                | crate::PackedKernelOwnerNodeKind::CollectionItemRead
                | crate::PackedKernelOwnerNodeKind::FreshOut
                | crate::PackedKernelOwnerNodeKind::UserCall { .. }
                | crate::PackedKernelOwnerNodeKind::RenderConstructor { .. }
                | crate::PackedKernelOwnerNodeKind::PureBuiltin { .. }
                | crate::PackedKernelOwnerNodeKind::FixedAbiCall { .. }
                | crate::PackedKernelOwnerNodeKind::HostEffect { .. }
                | crate::PackedKernelOwnerNodeKind::Draining
                | crate::PackedKernelOwnerNodeKind::Hold
                | crate::PackedKernelOwnerNodeKind::Arrow
                | crate::PackedKernelOwnerNodeKind::Delimiter
                | crate::PackedKernelOwnerNodeKind::Unknown
                | crate::PackedKernelOwnerNodeKind::Flush
                | crate::PackedKernelOwnerNodeKind::FieldProjection { .. }
                | crate::PackedKernelOwnerNodeKind::Known(_) => None,
            }
        })();
        self.end_visit(PackedOrderWalkKind::Semantic, expression, frame);
        result
    }

    fn expression_is_total(
        &mut self,
        expression: CheckedExprId,
        frame: Option<PackedOrderFrameId>,
    ) -> bool {
        if !self.begin_visit(PackedOrderWalkKind::Total, expression, frame) {
            return true;
        }
        let total = (|| {
            let expression = self.expression(expression)?;
            if let Some(binding) = expression.lexical {
                return Some(match binding.target {
                    crate::KernelLexicalBindingTargetInput::Declaration(target)
                        if binding.access == crate::KernelLexicalAccess::Read =>
                    {
                        let Some(target) =
                            self.input.relocate_declaration(expression.owner, target)
                        else {
                            return Some(false);
                        };
                        if let Some((value, parent)) = self.frame_actual(target, frame) {
                            self.expression_is_total(value, parent)
                        } else {
                            self.read_provider(target, binding.projection)
                                .is_none_or(|value| self.expression_is_total(value, frame))
                        }
                    }
                    crate::KernelLexicalBindingTargetInput::RuntimeContext
                        if binding.access == crate::KernelLexicalAccess::Read =>
                    {
                        true
                    }
                    crate::KernelLexicalBindingTargetInput::Declaration(_)
                    | crate::KernelLexicalBindingTargetInput::ContextFormal { .. }
                    | crate::KernelLexicalBindingTargetInput::Value { .. }
                    | crate::KernelLexicalBindingTargetInput::RuntimeContext => false,
                });
            }
            if let Some(call) = expression.call {
                return Some(
                    match call
                        .target()
                        .expect("validated packed order call has a target")
                    {
                        crate::KernelCallableSchemeId::User(owner) => {
                            if self.frame_contains_owner(frame, owner) {
                                // Totality is an error-capability analysis, not
                                // a termination proof. A recursive revisit is
                                // therefore the same coinductive success used
                                // by the generic active-visit guard above.
                                return Some(true);
                            }
                            let Some(result) = self
                                .input
                                .definition_relocation(owner)
                                .map(|relocation| relocation.result_expression)
                            else {
                                return Some(false);
                            };
                            let nested = self.push_user_frame(frame, call, owner);
                            let total = self.expression_is_total(result, Some(nested));
                            self.pop_user_frame(nested);
                            total
                        }
                        crate::KernelCallableSchemeId::Abi(_) => {
                            if order_key_call_is_error_capable(self.call_function(call)) {
                                false
                            } else {
                                let mut total = true;
                                for entry in
                                    call.entries().expect("validated packed call has entries")
                                {
                                    if let crate::PackedCallEntry::Input { value, .. } = entry
                                        && let Some(value) =
                                            self.input.relocate_value(call.owner, *value)
                                        && !self.expression_is_total(value, frame)
                                    {
                                        total = false;
                                        break;
                                    }
                                }
                                total
                                    && self
                                        .explicit_context_value(call)
                                        .is_none_or(|value| self.expression_is_total(value, frame))
                            }
                        }
                    },
                );
            }
            Some(match expression.node.kind {
                crate::PackedKernelOwnerNodeKind::Text => {
                    matches!(expression.payload, crate::PackedExpressionPayload::Text(_))
                }
                crate::PackedKernelOwnerNodeKind::Number => {
                    matches!(
                        expression.payload,
                        crate::PackedExpressionPayload::Number(_)
                    )
                }
                crate::PackedKernelOwnerNodeKind::Byte => {
                    matches!(expression.payload, crate::PackedExpressionPayload::Byte(_))
                }
                crate::PackedKernelOwnerNodeKind::Bits(_) => {
                    matches!(expression.payload, crate::PackedExpressionPayload::Bits(_))
                }
                crate::PackedKernelOwnerNodeKind::Tag(_) => true,
                crate::PackedKernelOwnerNodeKind::TextTemplate => expression
                    .inputs()
                    .iter()
                    .filter(|edge| edge.role == crate::PackedKernelOwnerEdgeRole::TextDynamic)
                    .all(|edge| {
                        self.linked_value(expression, edge.expression)
                            .is_some_and(|value| self.expression_is_total(value, frame))
                    }),
                crate::PackedKernelOwnerNodeKind::Infix { operation } => {
                    let operation = expression
                        .definition
                        .input()
                        .symbol(operation)
                        .unwrap_or("");
                    !matches!(operation, "+" | "-" | "*" | "/" | "%")
                        && self
                            .exact_input(expression, crate::PackedKernelOwnerEdgeRole::InfixLeft)
                            .is_some_and(|left| self.expression_is_total(left, frame))
                        && self
                            .exact_input(expression, crate::PackedKernelOwnerEdgeRole::InfixRight)
                            .is_some_and(|right| self.expression_is_total(right, frame))
                }
                crate::PackedKernelOwnerNodeKind::Block => expression
                    .shape
                    .and_then(|shape| match shape {
                        crate::PackedExecutionShape::Block {
                            result: Some(result),
                            ..
                        } => self.linked_value(expression, *result),
                        _ => None,
                    })
                    .is_some_and(|result| self.expression_is_total(result, frame)),
                crate::PackedKernelOwnerNodeKind::Then => self
                    .exact_input(expression, crate::PackedKernelOwnerEdgeRole::ThenOutput)
                    .is_some_and(|output| self.expression_is_total(output, frame)),
                crate::PackedKernelOwnerNodeKind::MatchArm { .. } => self
                    .exact_input(expression, crate::PackedKernelOwnerEdgeRole::MatchOutput)
                    .is_some_and(|output| self.expression_is_total(output, frame)),
                crate::PackedKernelOwnerNodeKind::Latest => expression
                    .inputs()
                    .iter()
                    .filter(|edge| edge.role == crate::PackedKernelOwnerEdgeRole::LatestBranch)
                    .all(|edge| {
                        self.linked_value(expression, edge.expression)
                            .is_some_and(|branch| self.expression_is_total(branch, frame))
                    }),
                crate::PackedKernelOwnerNodeKind::When => expression
                    .inputs()
                    .iter()
                    .filter(|edge| {
                        matches!(
                            edge.role,
                            crate::PackedKernelOwnerEdgeRole::WhenInput
                                | crate::PackedKernelOwnerEdgeRole::WhenArm
                        )
                    })
                    .all(|edge| {
                        self.linked_value(expression, edge.expression)
                            .is_some_and(|value| self.expression_is_total(value, frame))
                    }),
                crate::PackedKernelOwnerNodeKind::MapEntry => expression
                    .inputs()
                    .iter()
                    .filter(|edge| {
                        matches!(
                            edge.role,
                            crate::PackedKernelOwnerEdgeRole::MapKey
                                | crate::PackedKernelOwnerEdgeRole::MapValue
                        )
                    })
                    .all(|edge| {
                        self.linked_value(expression, edge.expression)
                            .is_some_and(|value| self.expression_is_total(value, frame))
                    }),
                crate::PackedKernelOwnerNodeKind::Collection {
                    kind: crate::KernelCollectionKind::Map,
                    ..
                } => expression
                    .inputs()
                    .iter()
                    .filter(|edge| edge.role == crate::PackedKernelOwnerEdgeRole::MapEntry)
                    .all(|edge| {
                        self.linked_value(expression, edge.expression)
                            .is_some_and(|value| self.expression_is_total(value, frame))
                    }),
                crate::PackedKernelOwnerNodeKind::Collection {
                    kind: crate::KernelCollectionKind::Set,
                    ..
                } => expression
                    .inputs()
                    .iter()
                    .filter(|edge| edge.role == crate::PackedKernelOwnerEdgeRole::CollectionItem)
                    .all(|edge| {
                        self.linked_value(expression, edge.expression)
                            .is_some_and(|value| self.expression_is_total(value, frame))
                    }),
                crate::PackedKernelOwnerNodeKind::Source(_)
                | crate::PackedKernelOwnerNodeKind::Absent
                | crate::PackedKernelOwnerNodeKind::Record { .. }
                | crate::PackedKernelOwnerNodeKind::Collection { .. }
                | crate::PackedKernelOwnerNodeKind::FormalRead { .. }
                | crate::PackedKernelOwnerNodeKind::ContextRead { .. }
                | crate::PackedKernelOwnerNodeKind::LexicalRead { .. }
                | crate::PackedKernelOwnerNodeKind::ValueRead { .. }
                | crate::PackedKernelOwnerNodeKind::DerivedRead { .. }
                | crate::PackedKernelOwnerNodeKind::PatternRead { .. }
                | crate::PackedKernelOwnerNodeKind::CollectionItemRead
                | crate::PackedKernelOwnerNodeKind::FreshOut
                | crate::PackedKernelOwnerNodeKind::UserCall { .. }
                | crate::PackedKernelOwnerNodeKind::RenderConstructor { .. }
                | crate::PackedKernelOwnerNodeKind::PureBuiltin { .. }
                | crate::PackedKernelOwnerNodeKind::FixedAbiCall { .. }
                | crate::PackedKernelOwnerNodeKind::HostEffect { .. }
                | crate::PackedKernelOwnerNodeKind::Draining
                | crate::PackedKernelOwnerNodeKind::Hold
                | crate::PackedKernelOwnerNodeKind::Arrow
                | crate::PackedKernelOwnerNodeKind::Delimiter
                | crate::PackedKernelOwnerNodeKind::Unknown
                | crate::PackedKernelOwnerNodeKind::Flush
                | crate::PackedKernelOwnerNodeKind::FieldProjection { .. }
                | crate::PackedKernelOwnerNodeKind::Known(_) => false,
            })
        })()
        .unwrap_or(false);
        self.end_visit(PackedOrderWalkKind::Total, expression, frame);
        total
    }

    fn merge_branch_states(
        &mut self,
        expression: PackedOrderExpressionRef<'a>,
        role: crate::PackedKernelOwnerEdgeRole,
        frame: Option<PackedOrderFrameId>,
    ) -> PackedOrderState<'a> {
        let branches = expression
            .inputs()
            .iter()
            .filter(|input| input.role == role);
        let mut state = None;
        for branch in branches {
            let Some(branch) = self.linked_value(expression, branch.expression) else {
                return PackedOrderState::Unordered;
            };
            let next = self.expression_state(branch, frame);
            state = Some(match (state, next) {
                (None, next) => next,
                (Some(PackedOrderState::Invalid { call_path }), _)
                | (_, PackedOrderState::Invalid { call_path }) => {
                    PackedOrderState::Invalid { call_path }
                }
                (Some(PackedOrderState::Ordered(left)), PackedOrderState::Ordered(right))
                    if left.semantic == right.semantic =>
                {
                    PackedOrderState::Ordered(left)
                }
                (Some(PackedOrderState::Deferred), _) | (_, PackedOrderState::Deferred) => {
                    PackedOrderState::Deferred
                }
                _ => PackedOrderState::Unordered,
            });
        }
        state.unwrap_or(PackedOrderState::Unordered)
    }

    fn expression_is_pure(
        &mut self,
        expression: CheckedExprId,
        frame: Option<PackedOrderFrameId>,
    ) -> bool {
        if !self.begin_visit(PackedOrderWalkKind::Pure, expression, frame) {
            return true;
        }
        let pure = (|| {
            let expression = self.expression(expression)?;
            let flow = expression
                .definition
                .code()
                .published_expression(expression.local.0 as usize)?;
            let effect = expression.definition.expression_effect(expression.local)?;
            if flow.mode != FlowMode::Continuous || effect != crate::KernelEffectSummary::default()
            {
                return Some(false);
            }
            if let Some(binding) = expression.lexical {
                if binding.access == crate::KernelLexicalAccess::Read
                    && self.path_is_empty(binding.projection)
                    && let crate::KernelLexicalBindingTargetInput::Declaration(target) =
                        binding.target
                    && let Some(target) = self.input.relocate_declaration(expression.owner, target)
                {
                    if let Some((value, parent)) = self.frame_actual(target, frame) {
                        return Some(self.expression_is_pure(value, parent));
                    }
                    return Some(
                        self.read_provider(target, binding.projection)
                            .is_none_or(|value| self.expression_is_pure(value, frame)),
                    );
                }
                return Some(true);
            }
            if let Some(call) = expression.call {
                for entry in call
                    .entries()
                    .expect("validated packed order call has entries")
                {
                    if let crate::PackedCallEntry::Input { value, .. } = entry {
                        let Some(value) = self.input.relocate_value(call.owner, *value) else {
                            return Some(false);
                        };
                        if !self.expression_is_pure(value, frame) {
                            return Some(false);
                        }
                    }
                }
                return Some(
                    self.explicit_context_value(call)
                        .is_none_or(|value| self.expression_is_pure(value, frame)),
                );
            }

            let all_role = |this: &mut Self, role: crate::PackedKernelOwnerEdgeRole| -> bool {
                expression
                    .inputs()
                    .iter()
                    .filter(|edge| edge.role == role)
                    .all(|edge| {
                        this.linked_value(expression, edge.expression)
                            .is_some_and(|value| this.expression_is_pure(value, frame))
                    })
            };
            Some(match expression.node.kind {
                crate::PackedKernelOwnerNodeKind::TextTemplate => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::TextDynamic)
                }
                crate::PackedKernelOwnerNodeKind::Record { .. } => {
                    let Some(shape) = expression.shape else {
                        return Some(false);
                    };
                    let Some(fields) = expression.facts.execution_fields(shape) else {
                        return Some(false);
                    };
                    fields.iter().all(|field| {
                        self.linked_value(expression, field.value)
                            .is_some_and(|value| self.expression_is_pure(value, frame))
                    })
                }
                crate::PackedKernelOwnerNodeKind::Draining => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::DrainingInput)
                }
                crate::PackedKernelOwnerNodeKind::Hold => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::HoldInitial)
                }
                crate::PackedKernelOwnerNodeKind::Flush => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::FlushPayload)
                }
                crate::PackedKernelOwnerNodeKind::Latest => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::LatestBranch)
                }
                crate::PackedKernelOwnerNodeKind::When => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::WhenInput)
                        && all_role(self, crate::PackedKernelOwnerEdgeRole::WhenArm)
                }
                crate::PackedKernelOwnerNodeKind::Then => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::ThenInput)
                        && all_role(self, crate::PackedKernelOwnerEdgeRole::ThenOutput)
                }
                crate::PackedKernelOwnerNodeKind::Infix { .. } => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::InfixLeft)
                        && all_role(self, crate::PackedKernelOwnerEdgeRole::InfixRight)
                }
                crate::PackedKernelOwnerNodeKind::MatchArm { .. } => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::MatchOutput)
                }
                crate::PackedKernelOwnerNodeKind::Block => {
                    let Some(shape) = expression.shape else {
                        return Some(false);
                    };
                    let Some(bindings) = expression.facts.execution_bindings(shape) else {
                        return Some(false);
                    };
                    bindings.iter().all(|binding| {
                        self.linked_value(expression, binding.value)
                            .is_some_and(|value| self.expression_is_pure(value, frame))
                    }) && match shape {
                        crate::PackedExecutionShape::Block { result, .. } => {
                            result.is_none_or(|result| {
                                self.linked_value(expression, result)
                                    .is_some_and(|value| self.expression_is_pure(value, frame))
                            })
                        }
                        _ => false,
                    }
                }
                crate::PackedKernelOwnerNodeKind::Collection {
                    kind: crate::KernelCollectionKind::Map,
                    ..
                } => all_role(self, crate::PackedKernelOwnerEdgeRole::MapEntry),
                crate::PackedKernelOwnerNodeKind::Collection { .. } => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::CollectionItem)
                }
                crate::PackedKernelOwnerNodeKind::MapEntry => {
                    all_role(self, crate::PackedKernelOwnerEdgeRole::MapKey)
                        && all_role(self, crate::PackedKernelOwnerEdgeRole::MapValue)
                }
                crate::PackedKernelOwnerNodeKind::Source(_)
                | crate::PackedKernelOwnerNodeKind::Absent
                | crate::PackedKernelOwnerNodeKind::Text
                | crate::PackedKernelOwnerNodeKind::Number
                | crate::PackedKernelOwnerNodeKind::Byte
                | crate::PackedKernelOwnerNodeKind::Bits(_)
                | crate::PackedKernelOwnerNodeKind::Tag(_)
                | crate::PackedKernelOwnerNodeKind::FormalRead { .. }
                | crate::PackedKernelOwnerNodeKind::ContextRead { .. }
                | crate::PackedKernelOwnerNodeKind::LexicalRead { .. }
                | crate::PackedKernelOwnerNodeKind::ValueRead { .. }
                | crate::PackedKernelOwnerNodeKind::DerivedRead { .. }
                | crate::PackedKernelOwnerNodeKind::PatternRead { .. }
                | crate::PackedKernelOwnerNodeKind::CollectionItemRead
                | crate::PackedKernelOwnerNodeKind::FreshOut
                | crate::PackedKernelOwnerNodeKind::UserCall { .. }
                | crate::PackedKernelOwnerNodeKind::RenderConstructor { .. }
                | crate::PackedKernelOwnerNodeKind::PureBuiltin { .. }
                | crate::PackedKernelOwnerNodeKind::FixedAbiCall { .. }
                | crate::PackedKernelOwnerNodeKind::HostEffect { .. }
                | crate::PackedKernelOwnerNodeKind::Arrow
                | crate::PackedKernelOwnerNodeKind::Delimiter
                | crate::PackedKernelOwnerNodeKind::Unknown
                | crate::PackedKernelOwnerNodeKind::FieldProjection { .. }
                | crate::PackedKernelOwnerNodeKind::Known(_) => true,
            })
        })()
        .unwrap_or(false);
        self.end_visit(PackedOrderWalkKind::Pure, expression, frame);
        pure
    }

    fn order_chain_diagnostic_span(
        &self,
        call_path: &[CheckedCallId],
        fallback: CheckedSpan,
    ) -> CheckedSpan {
        call_path
            .first()
            .and_then(|call| self.call(*call))
            .map_or(fallback, |call| self.call_span(call))
    }
}

fn order_key_call_is_error_capable(function: &str) -> bool {
    matches!(
        function,
        "Text/to_number"
            | "Text/slice"
            | "List/get"
            | "List/latest"
            | "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/from_hex"
            | "Bytes/from_base64"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
            | "Bytes/to_bits"
    )
}

fn type_is_orderable_key(ty: &Type) -> bool {
    matches!(ty, Type::Number | Type::Text)
        || matches!(
            ty,
            Type::VariantSet(variants)
                if !variants.is_empty()
                    && variants.iter().all(|variant| matches!(variant, Variant::Tag(_)))
        )
}

fn type_is_deferred_order_key(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Var(_) | Type::Unknown | Type::UnresolvedShape { .. }
    )
}
