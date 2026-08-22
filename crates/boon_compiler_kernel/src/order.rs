//! Packed-call derivation of checked list-order metadata.
//!
//! Expression semantics still borrow the completed rich expression rows while
//! the checked-model migration is in progress. Call topology, substitutions,
//! result types, spans, and invocation frames come directly from permanent
//! definition-code columns; RuntimePacked compilation therefore never needs a
//! rich `CheckedCall` projection merely to derive order chains.

use super::*;
use boon_checked::{
    CheckedCallOrderChain, CheckedEffectSummary, CheckedOrderChain, CheckedOrderDirection,
    CheckedOrderKey, CheckedParameter, CheckedTypeSubstitution,
    apply_checked_type_substitutions_once,
};
use boon_data::{Bits, ExactNumber};
use std::collections::{BTreeMap, BTreeSet};

/// Presentation-independent order diagnostic emitted by the packed analyzer.
///
/// The compiler facade owns Boon-facing type labels. Keeping that formatting
/// above the dependency firewall lets this module retain only the exact type
/// already required by `CheckedOrderKey`.
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

#[derive(Clone, Debug)]
struct PackedOrderFrame {
    call: CheckedCallId,
    callable: DeclId,
    bindings: BTreeMap<DeclId, CheckedExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderState {
    Ordered(AnalyzedOrderChain),
    Unordered,
    Deferred,
    Invalid { call_path: Vec<CheckedCallId> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AnalyzedOrderChain {
    checked: CheckedOrderChain,
    semantic: Vec<PackedOrderSemanticKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackedOrderSemanticKey {
    key: Option<PackedOrderSemanticExpression>,
    direction: PackedOrderSemanticDirection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderSemanticDirection {
    Ascending,
    Descending,
    Dynamic(Option<PackedOrderSemanticExpression>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderSemanticExpression {
    Row(Vec<String>),
    Capture {
        path: String,
        projection: Vec<String>,
    },
    Project {
        input: Box<PackedOrderSemanticExpression>,
        fields: Vec<String>,
    },
    Text(String),
    TextTemplate(Vec<PackedOrderSemanticTextSegment>),
    Number(ExactNumber),
    Bits(Bits),
    Tag(String),
    Call {
        function: String,
        inputs: Vec<(String, PackedOrderSemanticExpression)>,
    },
    Infix {
        operator: String,
        left: Box<PackedOrderSemanticExpression>,
        right: Box<PackedOrderSemanticExpression>,
    },
    Select {
        input: Box<PackedOrderSemanticExpression>,
        outputs: Vec<PackedOrderSemanticExpression>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PackedOrderSemanticTextSegment {
    Static(String),
    Dynamic(PackedOrderSemanticExpression),
}

struct PackedOrderAnalyzer<'a> {
    rows: &'a KernelCheckedRows,
    layout: &'a KernelCheckedLinkLayout,
    snapshot: &'a KernelCheckedSnapshot,
    declarations: BTreeMap<DeclId, &'a CheckedDeclaration>,
    callables: BTreeMap<DeclId, &'a CheckedCallableSignature>,
    pattern_bindings: BTreeMap<DeclId, &'a CheckedPatternBinding>,
    active: BTreeSet<(CheckedExprId, Vec<CheckedCallId>)>,
    type_cache: DefinitionTypeMaterializationCache,
    substitutions: BTreeMap<CheckedCallId, Box<[CheckedTypeSubstitution]>>,
}

impl KernelCheckedRows {
    /// Derive list-order chains without reading `self.calls`.
    ///
    /// This method must run after every definition span relocation has been
    /// installed. It rejects a foreign layout/snapshot and validates the
    /// packed call-to-parameter seal before interpreting any expression.
    pub fn derive_packed_order_chains(
        &self,
        layout: &KernelCheckedLinkLayout,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<KernelCheckedOrderDerivation, KernelCheckedLinkError> {
        PackedOrderAnalyzer::new(self, layout, snapshot)?.derive()
    }
}

impl<'a> PackedOrderAnalyzer<'a> {
    fn new(
        rows: &'a KernelCheckedRows,
        layout: &'a KernelCheckedLinkLayout,
        snapshot: &'a KernelCheckedSnapshot,
    ) -> Result<Self, KernelCheckedLinkError> {
        if !Arc::ptr_eq(
            &rows.semantic_input.definition_code,
            &snapshot.definition_code,
        ) {
            return Err(KernelCheckedLinkError::new(
                "packed order derivation cannot combine a foreign checked snapshot",
            ));
        }
        let mut analyzer = Self {
            rows,
            layout,
            snapshot,
            declarations: rows
                .declarations
                .iter()
                .map(|declaration| (declaration.id, declaration))
                .collect(),
            callables: rows
                .callables
                .iter()
                .map(|callable| (callable.decl_id, callable))
                .collect(),
            pattern_bindings: rows
                .pattern_bindings
                .iter()
                .map(|binding| (binding.declaration, binding))
                .collect(),
            active: BTreeSet::new(),
            type_cache: snapshot.definition_code.materialization_cache(),
            substitutions: BTreeMap::new(),
        };
        analyzer.validate()?;
        Ok(analyzer)
    }

    fn validate(&mut self) -> Result<(), KernelCheckedLinkError> {
        self.layout.for_each_packed_call(self.snapshot, |call| {
            let id = call.id()?;
            let expression = call.expression()?;
            if self
                .rows
                .expressions
                .get(expression.0 as usize)
                .is_none_or(|row| row.id != expression)
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "packed order call {} references missing checked expression {}",
                    id.0, expression.0,
                )));
            }
            let callable = call.callable()?;
            let signature = self.callables.get(&callable).copied().ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "packed order call {} references missing callable {}",
                    id.0, callable.0,
                ))
            })?;
            let code = call.code()?;
            let symbol = code
                .call_function(call.ordinal as usize)
                .ok_or_else(|| {
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
            self.rows
                .semantic_input
                .rebase_span(call.owner, span)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed order call {} was analyzed before span relocation",
                        id.0,
                    ))
                })?;
            call.context_binding()?;
            for entry in call.entries()? {
                self.parameter(signature, entry.parameter_ordinal())
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "packed order call {} entry references missing exact parameter ordinal {} on callable {}",
                            id.0,
                            entry.parameter_ordinal(),
                            callable.0,
                        ))
                    })?;
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
            let mut seen = BTreeSet::new();
            for substitution in facts {
                let linked = self.linked_type_parameter(target, substitution.variable)?;
                if !seen.insert(linked) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed order call {} repeats target type parameter {}",
                        id.0, linked.0,
                    )));
                }
            }
            Ok(())
        })
    }

    fn derive(mut self) -> Result<KernelCheckedOrderDerivation, KernelCheckedLinkError> {
        let mut chains = Vec::new();
        let mut diagnostics = Vec::new();
        for ordinal in 0..self.rows.semantic_input.call_count {
            let id = CheckedCallId(ordinal);
            let call = self.layout.packed_call(self.snapshot, id)?;
            match self.call_state(call, &[]) {
                PackedOrderState::Invalid { call_path } => {
                    diagnostics.push(KernelCheckedOrderDiagnostic {
                        span: self.order_chain_diagnostic_span(&call_path, self.call_span(call)),
                        kind: KernelCheckedOrderDiagnosticKind::MissingPrecedingSort,
                    });
                }
                PackedOrderState::Ordered(chain) => {
                    chains.push(CheckedCallOrderChain {
                        call: id,
                        chain: chain.checked.clone(),
                    });
                    for key in chain.checked.keys {
                        let span = self.order_chain_diagnostic_span(
                            &key.call_path,
                            self.rows
                                .expressions
                                .get(key.key.0 as usize)
                                .map_or(self.call_span(call), |expression| expression.span),
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
                }
                PackedOrderState::Unordered | PackedOrderState::Deferred => {}
            }
        }
        chains.sort_by_key(|entry| entry.call);
        Ok(KernelCheckedOrderDerivation {
            chains: chains.into_boxed_slice(),
            diagnostics: diagnostics.into_boxed_slice(),
        })
    }

    fn call(&self, id: CheckedCallId) -> Option<KernelCheckedPackedCallRef<'a>> {
        self.layout.packed_call(self.snapshot, id).ok()
    }

    fn declaration(&self, id: DeclId) -> Option<&'a CheckedDeclaration> {
        self.declarations.get(&id).copied()
    }

    fn callable(&self, id: DeclId) -> Option<&'a CheckedCallableSignature> {
        self.callables.get(&id).copied()
    }

    fn pattern_binding(&self, id: DeclId) -> Option<&'a CheckedPatternBinding> {
        self.pattern_bindings.get(&id).copied()
    }

    fn signature(&self, call: KernelCheckedPackedCallRef<'a>) -> &'a CheckedCallableSignature {
        self.callable(
            call.callable()
                .expect("validated packed order call has a callable relocation"),
        )
        .expect("validated packed order call has a checked signature")
    }

    fn parameter<'signature>(
        &self,
        signature: &'signature CheckedCallableSignature,
        ordinal: u32,
    ) -> Option<&'signature CheckedParameter> {
        let ordinal = usize::try_from(ordinal).ok()?;
        let mut matches = signature
            .parameters
            .iter()
            .filter(|parameter| parameter.ordinal == ordinal);
        let parameter = matches.next()?;
        matches.next().is_none().then_some(parameter)
    }

    fn input(&self, call: KernelCheckedPackedCallRef<'a>, name: &str) -> Option<CheckedExprId> {
        let signature = self.signature(call);
        call.entries()
            .expect("validated packed order call has entries")
            .iter()
            .find_map(|entry| match entry {
                crate::PackedCallEntry::Input {
                    parameter_ordinal,
                    value,
                    ..
                } if self
                    .parameter(signature, *parameter_ordinal)
                    .is_some_and(|parameter| parameter.name == name) =>
                {
                    self.rows.semantic_input.relocate_value(call.owner, *value)
                }
                crate::PackedCallEntry::Input { .. }
                | crate::PackedCallEntry::FreshOut { .. }
                | crate::PackedCallEntry::ForwardOut { .. } => None,
            })
    }

    fn input_bindings(
        &self,
        call: KernelCheckedPackedCallRef<'a>,
    ) -> BTreeMap<DeclId, CheckedExprId> {
        let signature = self.signature(call);
        call.entries()
            .expect("validated packed order call has entries")
            .iter()
            .filter_map(|entry| match entry {
                crate::PackedCallEntry::Input {
                    parameter_ordinal,
                    value,
                    ..
                } => Some((
                    self.parameter(signature, *parameter_ordinal)
                        .expect("validated packed entry parameter exists")
                        .decl_id,
                    self.rows
                        .semantic_input
                        .relocate_value(call.owner, *value)
                        .expect("validated packed call input relocates"),
                )),
                crate::PackedCallEntry::FreshOut { .. }
                | crate::PackedCallEntry::ForwardOut { .. } => None,
            })
            .collect()
    }

    fn input_values(&self, call: KernelCheckedPackedCallRef<'a>) -> Vec<CheckedExprId> {
        call.entries()
            .expect("validated packed order call has entries")
            .iter()
            .filter_map(|entry| match entry {
                crate::PackedCallEntry::Input { value, .. } => {
                    self.rows.semantic_input.relocate_value(call.owner, *value)
                }
                crate::PackedCallEntry::FreshOut { .. }
                | crate::PackedCallEntry::ForwardOut { .. } => None,
            })
            .chain(self.explicit_context_value(call))
            .collect()
    }

    fn explicit_context_value(
        &self,
        call: KernelCheckedPackedCallRef<'a>,
    ) -> Option<CheckedExprId> {
        match call
            .context_binding()
            .expect("validated packed order call has a context binding")
        {
            crate::PackedCallContextBinding::Explicit { value, .. } => {
                self.rows.semantic_input.relocate_value(call.owner, value)
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
        self.rows
            .semantic_input
            .rebase_span(call.owner, span)
            .expect("packed order derivation runs after span relocation")
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
        if !self.substitutions.contains_key(&id) {
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
            self.substitutions.insert(id, substitutions);
        }
        self.substitutions
            .get(&id)
            .expect("packed order substitution cache was populated")
    }

    fn expression_state(
        &mut self,
        expression: CheckedExprId,
        frames: &[PackedOrderFrame],
    ) -> PackedOrderState {
        let frame_path = frames.iter().map(|frame| frame.call).collect::<Vec<_>>();
        if !self.active.insert((expression, frame_path.clone())) {
            return PackedOrderState::Deferred;
        }
        let result = self.rows.expressions.get(expression.0 as usize).map_or(
            PackedOrderState::Unordered,
            |expression| match &expression.kind {
                CheckedExpressionKind::Call { call } => self
                    .call(*call)
                    .map_or(PackedOrderState::Unordered, |call| {
                        self.call_state(call, frames)
                    }),
                CheckedExpressionKind::Read {
                    target,
                    projection,
                    source,
                } if projection.is_empty() => {
                    if let Some((frame_index, value)) =
                        frames.iter().enumerate().rev().find_map(|(index, frame)| {
                            frame
                                .bindings
                                .get(target)
                                .copied()
                                .map(|value| (index, value))
                        })
                    {
                        self.expression_state(value, &frames[..frame_index])
                    } else if let Some(value) = self
                        .declaration(*target)
                        .and_then(|declaration| declaration.value)
                        .or_else(|| {
                            source
                                .as_ref()
                                .and_then(|source| self.source_expression(source))
                        })
                    {
                        self.expression_state(value, frames)
                    } else if self.declaration(*target).is_some_and(|declaration| {
                        declaration.kind == CheckedDeclarationKind::ValueParameter
                    }) {
                        PackedOrderState::Deferred
                    } else {
                        PackedOrderState::Unordered
                    }
                }
                CheckedExpressionKind::Block {
                    result: Some(result),
                    ..
                }
                | CheckedExpressionKind::Then {
                    output: Some(result),
                    ..
                }
                | CheckedExpressionKind::MatchArm {
                    output: Some(result),
                    ..
                } => self.expression_state(*result, frames),
                CheckedExpressionKind::Latest { branches }
                | CheckedExpressionKind::When { arms: branches, .. }
                | CheckedExpressionKind::While { arms: branches, .. } => {
                    self.merge_branch_states(branches, frames)
                }
                _ => PackedOrderState::Unordered,
            },
        );
        self.active.remove(&(expression, frame_path));
        result
    }

    fn call_state(
        &mut self,
        call: KernelCheckedPackedCallRef<'a>,
        frames: &[PackedOrderFrame],
    ) -> PackedOrderState {
        if !self.call_may_return_ordered_list(call) {
            return PackedOrderState::Unordered;
        }
        match self.call_function(call) {
            "List/sort_by" => {
                let Some(key) = self.input(call, "key") else {
                    return PackedOrderState::Unordered;
                };
                let (key, semantic) = self.order_key(call, key, frames);
                PackedOrderState::Ordered(AnalyzedOrderChain {
                    checked: CheckedOrderChain { keys: vec![key] },
                    semantic: vec![semantic],
                })
            }
            "List/then_by" => {
                let Some(list) = self.input(call, "list") else {
                    return PackedOrderState::Unordered;
                };
                match self.expression_state(list, frames) {
                    PackedOrderState::Ordered(mut chain) => {
                        let Some(key) = self.input(call, "key") else {
                            return PackedOrderState::Unordered;
                        };
                        let (key, semantic) = self.order_key(call, key, frames);
                        chain.checked.keys.push(key);
                        chain.semantic.push(semantic);
                        PackedOrderState::Ordered(chain)
                    }
                    PackedOrderState::Deferred => PackedOrderState::Deferred,
                    PackedOrderState::Invalid { call_path } => {
                        PackedOrderState::Invalid { call_path }
                    }
                    PackedOrderState::Unordered => PackedOrderState::Invalid {
                        call_path: frames
                            .iter()
                            .map(|frame| frame.call)
                            .chain(std::iter::once(
                                call.id().expect("validated packed order call has an ID"),
                            ))
                            .collect(),
                    },
                }
            }
            "List/filter" | "List/retain" | "List/remove" | "List/map" | "List/take"
            | "List/page" => self
                .input(call, "list")
                .map_or(PackedOrderState::Unordered, |list| {
                    self.expression_state(list, frames)
                }),
            _ => {
                let signature = self.signature(call);
                if signature.kind != CheckedCallableKind::User {
                    return PackedOrderState::Unordered;
                }
                if frames
                    .iter()
                    .any(|frame| frame.callable == signature.decl_id)
                {
                    return PackedOrderState::Deferred;
                }
                let Some(result) = signature.result_expression else {
                    return PackedOrderState::Unordered;
                };
                let mut nested = frames.to_vec();
                nested.push(PackedOrderFrame {
                    call: call.id().expect("validated packed order call has an ID"),
                    callable: signature.decl_id,
                    bindings: self.input_bindings(call),
                });
                self.expression_state(result, &nested)
            }
        }
    }

    fn order_key(
        &mut self,
        call: KernelCheckedPackedCallRef<'a>,
        key: CheckedExprId,
        frames: &[PackedOrderFrame],
    ) -> (CheckedOrderKey, PackedOrderSemanticKey) {
        let mut key_type = self
            .rows
            .expressions
            .get(key.0 as usize)
            .map(|expression| expression.flow_type.ty.clone())
            .unwrap_or(Type::Unknown);
        key_type =
            apply_checked_type_substitutions_once(&key_type, self.materialized_substitutions(call));
        for frame in frames.iter().rev() {
            if let Some(frame_call) = self.call(frame.call) {
                key_type = apply_checked_type_substitutions_once(
                    &key_type,
                    self.materialized_substitutions(frame_call),
                );
            }
        }
        let (direction, semantic_direction) = self.order_direction(call, frames);
        let id = call.id().expect("validated packed order call has an ID");
        let checked = CheckedOrderKey {
            call_path: frames
                .iter()
                .map(|frame| frame.call)
                .chain(std::iter::once(id))
                .collect(),
            key,
            direction,
            key_type,
            pure: self.expression_is_pure(key, frames, &mut BTreeSet::new()),
            total: self.expression_is_total(key, frames, &mut BTreeSet::new()),
        };
        let semantic = PackedOrderSemanticKey {
            key: self.semantic_expression(key, frames, &mut BTreeSet::new()),
            direction: semantic_direction,
        };
        (checked, semantic)
    }

    fn order_direction(
        &self,
        call: KernelCheckedPackedCallRef<'a>,
        frames: &[PackedOrderFrame],
    ) -> (CheckedOrderDirection, PackedOrderSemanticDirection) {
        let Some(expression) = self.input(call, "direction") else {
            return (
                CheckedOrderDirection::Ascending,
                PackedOrderSemanticDirection::Ascending,
            );
        };
        let semantic = self.semantic_expression(expression, frames, &mut BTreeSet::new());
        match semantic {
            Some(PackedOrderSemanticExpression::Tag(value)) if value == "Ascending" => (
                CheckedOrderDirection::Ascending,
                PackedOrderSemanticDirection::Ascending,
            ),
            Some(PackedOrderSemanticExpression::Tag(value)) if value == "Descending" => (
                CheckedOrderDirection::Descending,
                PackedOrderSemanticDirection::Descending,
            ),
            semantic => (
                CheckedOrderDirection::Dynamic { expression },
                PackedOrderSemanticDirection::Dynamic(semantic),
            ),
        }
    }

    fn semantic_expression(
        &self,
        expression: CheckedExprId,
        frames: &[PackedOrderFrame],
        active: &mut BTreeSet<(CheckedExprId, Vec<CheckedCallId>)>,
    ) -> Option<PackedOrderSemanticExpression> {
        let frame_path = frames.iter().map(|frame| frame.call).collect::<Vec<_>>();
        if !active.insert((expression, frame_path.clone())) {
            return None;
        }
        let Some(expression_value) = self.rows.expressions.get(expression.0 as usize) else {
            active.remove(&(expression, frame_path));
            return None;
        };
        let project = |input: PackedOrderSemanticExpression, fields: &[String]| {
            if fields.is_empty() {
                input
            } else {
                PackedOrderSemanticExpression::Project {
                    input: Box::new(input),
                    fields: fields.to_vec(),
                }
            }
        };
        let result = (|| match &expression_value.kind {
            CheckedExpressionKind::Read {
                target,
                projection,
                source,
            } => {
                if let Some((frame_index, value)) =
                    frames.iter().enumerate().rev().find_map(|(index, frame)| {
                        frame
                            .bindings
                            .get(target)
                            .copied()
                            .map(|value| (index, value))
                    })
                {
                    self.semantic_expression(value, &frames[..frame_index], active)
                        .map(|value| project(value, projection))
                } else {
                    let declaration = self.declaration(*target);
                    if declaration.is_some_and(|declaration| {
                        matches!(
                            declaration.kind,
                            CheckedDeclarationKind::OutParameter | CheckedDeclarationKind::FreshOut
                        )
                    }) {
                        Some(PackedOrderSemanticExpression::Row(projection.clone()))
                    } else if let Some(binding) = self.pattern_binding(*target) {
                        let mut fields = binding.projection.clone();
                        fields.extend(projection.iter().cloned());
                        self.semantic_expression(binding.selector, frames, active)
                            .map(|value| project(value, &fields))
                    } else {
                        let declared_value = declaration
                            .and_then(|declaration| declaration.value)
                            .or_else(|| {
                                source
                                    .as_ref()
                                    .and_then(|source| self.source_expression(source))
                            });
                        let expanded = declared_value.and_then(|value| {
                            self.semantic_expression(value, frames, active)
                                .map(|value| project(value, projection))
                        });
                        expanded.or_else(|| {
                            declaration.and_then(|declaration| {
                                self.declaration_canonical_path(declaration).map(|path| {
                                    PackedOrderSemanticExpression::Capture {
                                        path,
                                        projection: projection.clone(),
                                    }
                                })
                            })
                        })
                    }
                }
            }
            CheckedExpressionKind::ExternalRead { canonical_path, .. } => {
                Some(PackedOrderSemanticExpression::Capture {
                    path: canonical_path.clone(),
                    projection: Vec::new(),
                })
            }
            CheckedExpressionKind::Text { value } => {
                Some(PackedOrderSemanticExpression::Text(value.clone()))
            }
            CheckedExpressionKind::TextTemplate { segments } => segments
                .iter()
                .map(|segment| match segment {
                    CheckedTextSegment::Static { value } => {
                        Some(PackedOrderSemanticTextSegment::Static(value.clone()))
                    }
                    CheckedTextSegment::Dynamic { value } => self
                        .semantic_expression(*value, frames, active)
                        .map(PackedOrderSemanticTextSegment::Dynamic),
                })
                .collect::<Option<Vec<_>>>()
                .map(PackedOrderSemanticExpression::TextTemplate),
            CheckedExpressionKind::Number { value } => {
                Some(PackedOrderSemanticExpression::Number(value.clone()))
            }
            CheckedExpressionKind::Bits { value } => {
                Some(PackedOrderSemanticExpression::Bits(value.clone()))
            }
            CheckedExpressionKind::Absent | CheckedExpressionKind::Flush { .. } => None,
            CheckedExpressionKind::Tag { name } => {
                Some(PackedOrderSemanticExpression::Tag(name.clone()))
            }
            CheckedExpressionKind::Call { call } => {
                let call = self.call(*call)?;
                let signature = self.signature(call);
                if signature.kind == CheckedCallableKind::User {
                    let result = signature.result_expression?;
                    let mut nested = frames.to_vec();
                    nested.push(PackedOrderFrame {
                        call: call.id().ok()?,
                        callable: signature.decl_id,
                        bindings: self.input_bindings(call),
                    });
                    self.semantic_expression(result, &nested, active)
                } else {
                    let mut inputs = Vec::new();
                    for entry in call.entries().ok()? {
                        if let crate::PackedCallEntry::Input {
                            parameter_ordinal,
                            value,
                            ..
                        } = entry
                        {
                            let parameter = self.parameter(signature, *parameter_ordinal)?;
                            let value = self
                                .rows
                                .semantic_input
                                .relocate_value(call.owner, *value)?;
                            inputs.push((
                                parameter.name.clone(),
                                self.semantic_expression(value, frames, active)?,
                            ));
                        }
                    }
                    if let Some(value) = self.explicit_context_value(call) {
                        inputs.push((
                            "PASS".to_owned(),
                            self.semantic_expression(value, frames, active)?,
                        ));
                    }
                    Some(PackedOrderSemanticExpression::Call {
                        function: self.call_function(call).to_owned(),
                        inputs,
                    })
                }
            }
            CheckedExpressionKind::Infix { left, op, right } => self
                .semantic_expression(*left, frames, active)
                .zip(self.semantic_expression(*right, frames, active))
                .map(|(left, right)| PackedOrderSemanticExpression::Infix {
                    operator: op.clone(),
                    left: Box::new(left),
                    right: Box::new(right),
                }),
            CheckedExpressionKind::Block {
                result: Some(result),
                ..
            }
            | CheckedExpressionKind::Then {
                output: Some(result),
                ..
            }
            | CheckedExpressionKind::MatchArm {
                output: Some(result),
                ..
            } => self.semantic_expression(*result, frames, active),
            CheckedExpressionKind::Latest { branches } => branches
                .iter()
                .enumerate()
                .map(|(index, branch)| {
                    self.semantic_expression(*branch, frames, active)
                        .map(|value| (index.to_string(), value))
                })
                .collect::<Option<Vec<_>>>()
                .map(|inputs| PackedOrderSemanticExpression::Call {
                    function: "LATEST".to_owned(),
                    inputs,
                }),
            CheckedExpressionKind::When { input, arms }
            | CheckedExpressionKind::While { input, arms } => self
                .semantic_expression(*input, frames, active)
                .zip(
                    arms.iter()
                        .map(|arm| self.semantic_expression(*arm, frames, active))
                        .collect::<Option<Vec<_>>>(),
                )
                .map(|(input, outputs)| PackedOrderSemanticExpression::Select {
                    input: Box::new(input),
                    outputs,
                }),
            CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::TaggedObject { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Draining { .. }
            | CheckedExpressionKind::Hold { .. }
            | CheckedExpressionKind::Then { output: None, .. }
            | CheckedExpressionKind::MatchArm { output: None, .. }
            | CheckedExpressionKind::Block { result: None, .. }
            | CheckedExpressionKind::Object { .. }
            | CheckedExpressionKind::List { .. }
            | CheckedExpressionKind::MapEntry { .. }
            | CheckedExpressionKind::Map { .. }
            | CheckedExpressionKind::Set { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Bytes { .. }
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => None,
        })();
        active.remove(&(expression, frame_path));
        result
    }

    fn expression_is_total(
        &self,
        expression: CheckedExprId,
        frames: &[PackedOrderFrame],
        active: &mut BTreeSet<(CheckedExprId, Vec<CheckedCallId>)>,
    ) -> bool {
        let frame_path = frames.iter().map(|frame| frame.call).collect::<Vec<_>>();
        if !active.insert((expression, frame_path.clone())) {
            return true;
        }
        let Some(expression_value) = self.rows.expressions.get(expression.0 as usize) else {
            active.remove(&(expression, frame_path));
            return false;
        };
        let total = match &expression_value.kind {
            CheckedExpressionKind::Read { target, source, .. } => {
                if let Some((frame_index, value)) =
                    frames.iter().enumerate().rev().find_map(|(index, frame)| {
                        frame
                            .bindings
                            .get(target)
                            .copied()
                            .map(|value| (index, value))
                    })
                {
                    self.expression_is_total(value, &frames[..frame_index], active)
                } else {
                    self.declaration(*target)
                        .and_then(|declaration| declaration.value)
                        .or_else(|| {
                            source
                                .as_ref()
                                .and_then(|source| self.source_expression(source))
                        })
                        .is_none_or(|value| self.expression_is_total(value, frames, active))
                }
            }
            CheckedExpressionKind::Call { call } => {
                let Some(call) = self.call(*call) else {
                    active.remove(&(expression, frame_path));
                    return false;
                };
                let signature = self.signature(call);
                if signature.kind == CheckedCallableKind::User {
                    let Some(result) = signature.result_expression else {
                        active.remove(&(expression, frame_path));
                        return false;
                    };
                    let mut nested = frames.to_vec();
                    nested.push(PackedOrderFrame {
                        call: call.id().expect("validated packed order call has an ID"),
                        callable: signature.decl_id,
                        bindings: self.input_bindings(call),
                    });
                    self.expression_is_total(result, &nested, active)
                } else {
                    !order_key_call_is_error_capable(self.call_function(call))
                        && self
                            .input_values(call)
                            .into_iter()
                            .all(|value| self.expression_is_total(value, frames, active))
                }
            }
            CheckedExpressionKind::TextTemplate { segments } => {
                segments.iter().all(|segment| match segment {
                    CheckedTextSegment::Static { .. } => true,
                    CheckedTextSegment::Dynamic { value } => {
                        self.expression_is_total(*value, frames, active)
                    }
                })
            }
            CheckedExpressionKind::Infix { left, op, right } => {
                !matches!(op.as_str(), "+" | "-" | "*" | "/" | "%")
                    && self.expression_is_total(*left, frames, active)
                    && self.expression_is_total(*right, frames, active)
            }
            CheckedExpressionKind::Block {
                result: Some(result),
                ..
            }
            | CheckedExpressionKind::Then {
                output: Some(result),
                ..
            }
            | CheckedExpressionKind::MatchArm {
                output: Some(result),
                ..
            } => self.expression_is_total(*result, frames, active),
            CheckedExpressionKind::Latest { branches } => branches
                .iter()
                .all(|branch| self.expression_is_total(*branch, frames, active)),
            CheckedExpressionKind::When { input, arms }
            | CheckedExpressionKind::While { input, arms } => {
                self.expression_is_total(*input, frames, active)
                    && arms
                        .iter()
                        .all(|arm| self.expression_is_total(*arm, frames, active))
            }
            CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::Bits { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::ExternalRead { .. } => true,
            CheckedExpressionKind::MapEntry { key, value } => {
                self.expression_is_total(*key, frames, active)
                    && self.expression_is_total(*value, frames, active)
            }
            CheckedExpressionKind::Map { entries } => entries
                .iter()
                .all(|entry| self.expression_is_total(*entry, frames, active)),
            CheckedExpressionKind::Set { items } => items
                .iter()
                .all(|item| self.expression_is_total(*item, frames, active)),
            CheckedExpressionKind::Absent
            | CheckedExpressionKind::Flush { .. }
            | CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::TaggedObject { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Draining { .. }
            | CheckedExpressionKind::Hold { .. }
            | CheckedExpressionKind::Then { output: None, .. }
            | CheckedExpressionKind::MatchArm { output: None, .. }
            | CheckedExpressionKind::Block { result: None, .. }
            | CheckedExpressionKind::Object { .. }
            | CheckedExpressionKind::List { .. }
            | CheckedExpressionKind::Bytes { .. }
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => false,
        };
        active.remove(&(expression, frame_path));
        total
    }

    fn merge_branch_states(
        &mut self,
        branches: &[CheckedExprId],
        frames: &[PackedOrderFrame],
    ) -> PackedOrderState {
        let mut states = branches
            .iter()
            .map(|branch| self.expression_state(*branch, frames));
        let Some(first) = states.next() else {
            return PackedOrderState::Unordered;
        };
        states.fold(first, |left, right| match (left, right) {
            (PackedOrderState::Invalid { call_path }, _)
            | (_, PackedOrderState::Invalid { call_path }) => {
                PackedOrderState::Invalid { call_path }
            }
            (PackedOrderState::Ordered(left), PackedOrderState::Ordered(right))
                if left.semantic == right.semantic =>
            {
                PackedOrderState::Ordered(left)
            }
            (PackedOrderState::Deferred, _) | (_, PackedOrderState::Deferred) => {
                PackedOrderState::Deferred
            }
            _ => PackedOrderState::Unordered,
        })
    }

    fn expression_is_pure(
        &self,
        expression: CheckedExprId,
        frames: &[PackedOrderFrame],
        active: &mut BTreeSet<(CheckedExprId, Vec<CheckedCallId>)>,
    ) -> bool {
        let frame_path = frames.iter().map(|frame| frame.call).collect::<Vec<_>>();
        if !active.insert((expression, frame_path.clone())) {
            return true;
        }
        let Some(expression) = self.rows.expressions.get(expression.0 as usize) else {
            return false;
        };
        if expression.flow_type.mode != FlowMode::Continuous
            || expression.effect != CheckedEffectSummary::default()
        {
            active.remove(&(expression.id, frame_path));
            return false;
        }
        let children = match &expression.kind {
            CheckedExpressionKind::Read {
                target,
                projection,
                source,
            } if projection.is_empty() => {
                if let Some((frame_index, value)) =
                    frames.iter().enumerate().rev().find_map(|(index, frame)| {
                        frame
                            .bindings
                            .get(target)
                            .copied()
                            .map(|value| (index, value))
                    })
                {
                    let pure = self.expression_is_pure(value, &frames[..frame_index], active);
                    active.remove(&(expression.id, frame_path));
                    return pure;
                }
                self.declaration(*target)
                    .and_then(|declaration| declaration.value)
                    .or_else(|| {
                        source
                            .as_ref()
                            .and_then(|source| self.source_expression(source))
                    })
                    .into_iter()
                    .collect()
            }
            CheckedExpressionKind::Call { call } => self
                .call(*call)
                .map(|call| self.input_values(call))
                .unwrap_or_default(),
            CheckedExpressionKind::TextTemplate { segments } => segments
                .iter()
                .filter_map(|segment| match segment {
                    CheckedTextSegment::Static { .. } => None,
                    CheckedTextSegment::Dynamic { value } => Some(*value),
                })
                .collect(),
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields } => {
                fields.iter().map(|field| field.value).collect()
            }
            CheckedExpressionKind::Draining { input }
            | CheckedExpressionKind::Hold { initial: input, .. } => vec![*input],
            CheckedExpressionKind::Flush { payload } => vec![*payload],
            CheckedExpressionKind::Latest { branches } => branches.clone(),
            CheckedExpressionKind::When { input, arms }
            | CheckedExpressionKind::While { input, arms } => std::iter::once(*input)
                .chain(arms.iter().copied())
                .collect(),
            CheckedExpressionKind::Then { input, output } => std::iter::once(*input)
                .chain(output.iter().copied())
                .collect(),
            CheckedExpressionKind::Infix { left, right, .. } => vec![*left, *right],
            CheckedExpressionKind::MatchArm { output, .. } => output.iter().copied().collect(),
            CheckedExpressionKind::Block { bindings, result } => bindings
                .iter()
                .map(|binding| binding.value)
                .chain(result.iter().copied())
                .collect(),
            CheckedExpressionKind::List { items, .. }
            | CheckedExpressionKind::Bytes { items, .. }
            | CheckedExpressionKind::Set { items } => items.clone(),
            CheckedExpressionKind::Map { entries } => entries.clone(),
            CheckedExpressionKind::MapEntry { key, value } => vec![*key, *value],
            CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::Read { .. }
            | CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::Bits { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Absent
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => Vec::new(),
        };
        let pure = children
            .into_iter()
            .all(|child| self.expression_is_pure(child, frames, active));
        active.remove(&(expression.id, frame_path));
        pure
    }

    fn source_expression(&self, read: &CheckedSourceRead) -> Option<CheckedExprId> {
        self.rows
            .sources
            .get(read.source.0 as usize)
            .filter(|source| source.id == read.source)
            .map(|source| source.expression)
    }

    fn declaration_canonical_path(&self, declaration: &CheckedDeclaration) -> Option<String> {
        if !matches!(
            declaration.kind,
            CheckedDeclarationKind::Field
                | CheckedDeclarationKind::Source
                | CheckedDeclarationKind::Hold
                | CheckedDeclarationKind::List
        ) {
            return None;
        }
        let mut segments = vec![declaration.name.clone()];
        let mut scope = declaration.scope_id;
        let mut visited = BTreeSet::new();
        while scope != LexicalScopeId(0) && visited.insert(scope) {
            let current = self
                .rows
                .scopes
                .iter()
                .find(|candidate| candidate.id == scope)?;
            if current.kind == CheckedScopeKind::Function {
                return None;
            }
            if let Some(owner) = current.owner
                && let Some(owner) = self.declaration(owner)
                && matches!(
                    owner.kind,
                    CheckedDeclarationKind::Field
                        | CheckedDeclarationKind::Source
                        | CheckedDeclarationKind::Hold
                        | CheckedDeclarationKind::List
                )
            {
                segments.push(owner.name.clone());
            }
            scope = current.parent?;
        }
        segments.reverse();
        Some(segments.join("."))
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
