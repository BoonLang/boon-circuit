#[cfg(test)]
use crate::KernelLexicalBindingTarget;
use crate::{
    KernelAbiContextualOperation, KernelAbiInput, KernelCallArgumentKind, KernelCallInputRoleRef,
    KernelCallTargetRef, KernelCheckedSnapshot, KernelDeclarationReference, KernelDefinitionRef,
    KernelExternalTarget, KernelLexicalBindingTargetRef, KernelOwnerId, KernelProjectInput,
    KernelScopeReference, KernelStatePathRef, KernelStatementChildReference,
    KernelStatementReference, KernelValueReference,
};
use boon_checked::{
    CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1, CheckedBlockBinding, CheckedCall,
    CheckedCallEntry, CheckedCallId, CheckedCallResultPath, CheckedCallableContext,
    CheckedCallableKind, CheckedCallableSignature, CheckedContextBinding, CheckedContextFormal,
    CheckedContextScheme, CheckedContextTypeSubstitution, CheckedContextualOperation,
    CheckedDeclaration, CheckedDeclarationKind, CheckedDefinitionExecutionNodeV1,
    CheckedDefinitionExecutionTemplateV1, CheckedDefinitionSelectorV1, CheckedEffectSummary,
    CheckedEvaluationScope, CheckedExprId, CheckedExpression, CheckedExpressionKind,
    CheckedImageKernelPublicationV1, CheckedImageRowDomainV2, CheckedList, CheckedListId,
    CheckedMatchPattern, CheckedParameter, CheckedParameterKind, CheckedParameterRequirement,
    CheckedPassedAccess, CheckedPatternBinding, CheckedProgram, CheckedProgramFields,
    CheckedRecordField, CheckedResourceBinding, CheckedResourceProjectionRequirement,
    CheckedRuntimeFlowTermProjectionV1, CheckedScope, CheckedScopeKind, CheckedSemanticPath,
    CheckedShardCallableKindV2, CheckedShardOwnerKeyV2, CheckedShardProjectionKeyV2,
    CheckedShardRegionV2, CheckedSource, CheckedSourceId, CheckedSourceRead, CheckedSpan,
    CheckedState, CheckedStateId, CheckedStatement, CheckedStatementId, CheckedStatementKind,
    CheckedTextSegment, CheckedTypeSubstitution, CheckedValueUse, ContextFormalId, DeclId,
    FlowMode, FlowType, LexicalScopeId, ObjectShape, ProgramRole, SemanticOccurrence,
    SemanticOccurrenceKind, SharedObjectShape, Type, TypeVar, Variant,
};
use boon_contract::{SourceBundleDigestV1, SymbolId};
use boon_syntax::StableOccurrenceKey;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

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

/// Final checked namespace owned by one referenced stable ABI callable.
///
/// The kernel allocates these rows after all definition-owned declarations,
/// keeping parser-owned IDs and compiler/library ABI IDs in disjoint dense
/// ranges. Only callables actually referenced by this checked snapshot are
/// materialized.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedAbiCallableLayout {
    pub name: Box<str>,
    pub declaration: DeclId,
    pub parameters: Box<[DeclId]>,
    pub type_variables: KernelCheckedRowRange,
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
    pub user_callables: u32,
    pub abi_callables: u32,
    pub callables: u32,
    pub context_formals: u32,
    pub sources: u32,
    pub states: u32,
    pub lists: u32,
    pub resolved_references: u64,
}

/// Complete dense checked rows materialized from one kernel snapshot.
///
/// This is the single projection seam between definition-local kernel
/// artifacts and the existing checked model. Source-unit coordinate rebasing
/// and project-level semantic indexes remain orchestration concerns; callers
/// must not re-run per-row linker methods independently.
#[derive(Debug, Eq, PartialEq)]
pub struct KernelCheckedRows {
    pub scopes: Box<[CheckedScope]>,
    pub declarations: Box<[CheckedDeclaration]>,
    pub expressions: Box<[CheckedExpression]>,
    pub statements: Box<[CheckedStatement]>,
    pub callables: Box<[CheckedCallableSignature]>,
    pub context_formals: Box<[CheckedContextFormal]>,
    pub calls: Box<[CheckedCall]>,
    /// Parser-issued structural identity for each call row at the same dense
    /// ordinal. Checked-image sealing consumes this relocation directly and
    /// never interprets compact checked expression IDs as parser slots.
    pub call_occurrences: Box<[StableOccurrenceKey]>,
    /// Explicit editor/oracle projection. Runtime compilation publishes the
    /// same row cardinality and ownership directly into
    /// `checked_image_publication` and leaves this compatibility family empty.
    pub call_result_paths: Box<[CheckedCallResultPath]>,
    pub pattern_bindings: Box<[CheckedPatternBinding]>,
    /// Move-only packed authority bound by the linker that produced these
    /// rows. Rich paths, origins, and recursive types are projected only by
    /// the explicit oracle/editor method on `KernelCheckedLinkLayout`.
    pub semantic_input: KernelSemanticInputConstructionV1,
    pub sources: Box<[CheckedSource]>,
    pub states: Box<[CheckedState]>,
    pub lists: Box<[CheckedList]>,
    pub runtime_flow_terms: CheckedRuntimeFlowTermProjectionV1,
    /// Compact checked-image topology emitted by the same linker that assigns
    /// final dense row IDs. The compiler appends project metadata rows and the
    /// typechecker consumes it by value; neither stage replays rich checked
    /// expression/statement tables.
    pub checked_image_publication: CheckedImageKernelPublicationV1,
    /// Explicit editor/oracle projection. Runtime compilation consumes the
    /// dense occurrence routes in `checked_image_publication` and never owns
    /// a second span/name-oriented occurrence table.
    pub occurrences: Box<[SemanticOccurrence]>,
    occurrence_ranges: Box<[KernelCheckedRowRange]>,
}

/// Rich checked rows are a presentation demand, not a prerequisite for the
/// runtime image. Keeping this decision at the kernel linker prevents callers
/// from materializing an expensive family and immediately dropping it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelCheckedRowProjectionDemand {
    RuntimePacked,
    EditorRich,
}

impl KernelCheckedRows {
    /// Rebase every source-bearing row owned by one definition from source-unit
    /// coordinates into the project-wide checked coordinate system.
    ///
    /// Keeping this operation beside the row materializer prevents the compiler
    /// facade from maintaining one relocation loop per table and guarantees new
    /// source-bearing tables participate in the same pass.
    pub fn rebase_definition_spans(
        &mut self,
        layout: &KernelCheckedLinkLayout,
        owner: KernelOwnerId,
        start_line: usize,
        start_byte: usize,
    ) -> Result<(), KernelCheckedLinkError> {
        let definition = layout.definition(owner)?;
        for row in checked_range(definition.scopes)? {
            let scope = self.scopes.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked scope linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut scope.span,
                start_line,
                start_byte,
                &format!("kernel checked scope row {row}"),
            )?;
        }
        for declaration_id in checked_range(definition.declarations)? {
            let row = declaration_id.checked_sub(1).ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked declaration row uses reserved identity zero",
                )
            })?;
            let declaration = self.declarations.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked declaration linker references missing row {declaration_id}"
                ))
            })?;
            rebase_checked_span(
                &mut declaration.span,
                start_line,
                start_byte,
                &format!("kernel checked declaration row {declaration_id}"),
            )?;
        }
        for row in checked_range(definition.expressions)? {
            let expression = self.expressions.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked expression linker references missing row {row}"
                ))
            })?;
            rebase_checked_expression_spans(expression, start_line, start_byte)?;
        }
        for row in checked_range(definition.statements)? {
            let statement = self.statements.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked statement linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut statement.span,
                start_line,
                start_byte,
                &format!("kernel checked statement row {row}"),
            )?;
        }
        for row in checked_range(definition.sources)? {
            let source = self.sources.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked SOURCE linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut source.span,
                start_line,
                start_byte,
                &format!("kernel checked SOURCE row {row}"),
            )?;
        }
        for row in checked_range(definition.states)? {
            let state = self.states.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked state linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut state.span,
                start_line,
                start_byte,
                &format!("kernel checked state row {row}"),
            )?;
        }
        for row in checked_range(definition.lists)? {
            let list = self.lists.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked LIST linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut list.span,
                start_line,
                start_byte,
                &format!("kernel checked LIST row {row}"),
            )?;
        }
        for row in checked_range(definition.calls)? {
            let call = self.calls.get_mut(row).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked call linker references missing row {row}"
                ))
            })?;
            rebase_checked_span(
                &mut call.span,
                start_line,
                start_byte,
                &format!("kernel checked call row {row}"),
            )?;
            if let CheckedContextBinding::Explicit { span, .. } = &mut call.context_binding {
                rebase_checked_span(
                    span,
                    start_line,
                    start_byte,
                    &format!("kernel checked call row {row} PASS"),
                )?;
            }
        }
        if !self.occurrence_ranges.is_empty() {
            let occurrence_range =
                *self
                    .occurrence_ranges
                    .get(owner.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel checked occurrence ranges omit definition {}",
                            owner.0,
                        ))
                    })?;
            for row in checked_range(occurrence_range)? {
                let occurrence = self.occurrences.get_mut(row).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked occurrence linker references missing row {row}"
                    ))
                })?;
                rebase_checked_span(
                    &mut occurrence.span,
                    start_line,
                    start_byte,
                    &format!("kernel checked occurrence row {row}"),
                )?;
            }
        } else if !self.occurrences.is_empty() {
            return Err(KernelCheckedLinkError::new(
                "kernel checked occurrence rows have no definition ranges",
            ));
        }
        if let Some(callable) = self
            .callables
            .iter_mut()
            .find(|callable| callable.decl_id == definition.public_declaration)
        {
            for parameter in &mut callable.parameters {
                parameter.start = start_byte.checked_add(parameter.start).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked callable {} parameter start overflowed",
                        callable.name,
                    ))
                })?;
                parameter.end = start_byte.checked_add(parameter.end).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked callable {} parameter end overflowed",
                        callable.name,
                    ))
                })?;
            }
        }
        Ok(())
    }
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
    abi_callables: Box<[KernelCheckedAbiCallableLayout]>,
    abi_callable_by_name: BTreeMap<Box<str>, usize>,
    definition_declarations_end: u32,
    totals: KernelCheckedLinkTotals,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelSemanticResourceProjectionLocatorV1 {
    owner: KernelOwnerId,
    ordinal: u32,
    expression: CheckedExprId,
    target: DeclId,
}

/// One compact call-result path in the linked checked namespace.
///
/// Projection spelling stays in the project text authority. The row owns only
/// the final declaration anchor and a range into one flat `SymbolId` column,
/// so ordinary compilation does not allocate `Vec<String>` per call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelSemanticCallResultPathLocatorV1 {
    call: CheckedCallId,
    anchor: DeclId,
    projection_start: u32,
    projection_len: u32,
}

/// Immutable relocation from one definition-local code module into the
/// checked image bound to this semantic input.
///
/// Execution rows remain in `DefinitionCodeStore`; this compact row is the
/// only project-specific overlay semantic consumers need in order to borrow
/// them. No checked-ID copy of the execution graph is retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelSemanticDefinitionRelocationV1 {
    callable: DeclId,
    expressions: KernelCheckedRowRange,
    calls: KernelCheckedRowRange,
    sources: KernelCheckedRowRange,
    states: KernelCheckedRowRange,
    lists: KernelCheckedRowRange,
}

/// Move-only construction authority transported beside the compatibility
/// checked image.
///
/// This owns the packed definition-code/type authority and its sole dense
/// relocation layout. Transitional `DefinitionArtifact` DTOs are deliberately
/// not retained; later normalized definition modules will extend this input
/// directly. The checked image may still expose rich rows for explicit editor
/// or oracle requests, but ordinary semantic compilation consumes this
/// authority instead of reconstructing packed facts from those rows.
#[derive(Debug, Eq, PartialEq)]
pub struct KernelSemanticInputConstructionV1 {
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    definition_count: usize,
    definition_code: Arc<crate::DefinitionCodeStore>,
    expression_count: u32,
    declaration_count: u32,
    source_count: u32,
    definition_relocations: Box<[KernelSemanticDefinitionRelocationV1]>,
    /// Owners with execution templates, ordered by final callable ID.
    definition_execution_owners: Box<[KernelOwnerId]>,
    call_result_paths: Box<[KernelSemanticCallResultPathLocatorV1]>,
    call_result_path_symbols: Box<[SymbolId]>,
    resource_projections: Box<[KernelSemanticResourceProjectionLocatorV1]>,
    resource_projection_by_expression: Box<[u32]>,
    rich_editor_projection_expected: bool,
    checked_image_pairing: Arc<boon_checked::CheckedImageKernelPairingV1>,
}

/// Checked-image-bound packed input accepted by the kernel semantic route.
///
/// Dense IDs remain revision-local and are valid only together with this
/// exact source/image authority. Public views never expose bare `SymbolId` or
/// `TypeTermId` values.
#[derive(Debug)]
pub struct KernelSemanticInputV1 {
    construction: KernelSemanticInputConstructionV1,
    checked_image_digest: [u8; 32],
}

#[derive(Clone, Copy)]
pub struct KernelSemanticDefinitionExecutionTemplateRef<'a> {
    input: &'a KernelSemanticInputV1,
    template: crate::PackedDefinitionExecutionRef<'a>,
}

pub struct KernelSemanticDefinitionExecutionTemplateIter<'a> {
    input: &'a KernelSemanticInputV1,
    next: usize,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticDefinitionExecutionNodeRef<'a> {
    input: &'a KernelSemanticInputV1,
    node: crate::PackedExecutionNodeRef<'a>,
}

pub struct KernelSemanticDefinitionExecutionNodeIter<'a> {
    input: &'a KernelSemanticInputV1,
    nodes: crate::PackedExecutionNodeIter<'a>,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticDefinitionSelectorRef<'a> {
    input: &'a KernelSemanticInputV1,
    selector: crate::PackedExecutionSelectorRef<'a>,
}

pub struct KernelSemanticDefinitionExpressionIter<'a> {
    input: &'a KernelSemanticInputV1,
    rows: std::slice::Iter<'a, crate::PackedExpressionRef>,
}

pub struct KernelSemanticDefinitionCallIter<'a> {
    input: &'a KernelSemanticInputV1,
    rows: std::slice::Iter<'a, crate::PackedCallRef>,
}

pub struct KernelSemanticDefinitionSourceIter<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
    next: u32,
    len: u32,
}

pub struct KernelSemanticDefinitionStateIter<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
    next: u32,
    len: u32,
}

pub struct KernelSemanticDefinitionListIter<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
    next: u32,
    len: u32,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticResourceProjectionRef<'a> {
    input: &'a KernelSemanticInputV1,
    locator: KernelSemanticResourceProjectionLocatorV1,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticCallResultPathRef<'a> {
    input: &'a KernelSemanticInputV1,
    locator: KernelSemanticCallResultPathLocatorV1,
}

pub struct KernelSemanticResourceOriginIter<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
    requirement_ordinal: usize,
    next: usize,
    len: usize,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticResourceOriginRef<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
    source_owner: KernelOwnerId,
    source: crate::KernelSourceId,
    payload_projection: &'a [boon_contract::SymbolId],
}

impl<'a> Iterator for KernelSemanticDefinitionExecutionTemplateIter<'a> {
    type Item = KernelSemanticDefinitionExecutionTemplateRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let owner = *self
            .input
            .construction
            .definition_execution_owners
            .get(self.next)?;
        self.next += 1;
        let template = self
            .input
            .construction
            .definition_code
            .definition(owner)
            .expect("sealed kernel semantic execution owner has definition code")
            .execution_template()
            .expect("sealed kernel semantic execution owner has a template");
        Some(KernelSemanticDefinitionExecutionTemplateRef {
            input: self.input,
            template,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self
            .input
            .construction
            .definition_execution_owners
            .len()
            .saturating_sub(self.next);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticDefinitionExecutionTemplateIter<'_> {}

impl<'a> KernelSemanticDefinitionExecutionTemplateRef<'a> {
    pub fn callable(self) -> DeclId {
        self.input
            .definition_relocation(self.template.owner())
            .expect("sealed kernel semantic execution owner has a relocation")
            .callable
    }

    pub fn result(self) -> CheckedExprId {
        self.input
            .relocate_expression(self.template.result())
            .expect("sealed kernel semantic execution result relocates")
    }

    pub fn nodes(self) -> KernelSemanticDefinitionExecutionNodeIter<'a> {
        KernelSemanticDefinitionExecutionNodeIter {
            input: self.input,
            nodes: self.template.nodes(),
        }
    }

    pub fn calls(self) -> KernelSemanticDefinitionCallIter<'a> {
        KernelSemanticDefinitionCallIter {
            input: self.input,
            rows: self.template.calls().iter(),
        }
    }

    pub fn sources(self) -> KernelSemanticDefinitionSourceIter<'a> {
        KernelSemanticDefinitionSourceIter {
            input: self.input,
            owner: self.template.owner(),
            next: 0,
            len: u32::try_from(self.template.source_count())
                .expect("sealed kernel semantic SOURCE count fits u32"),
        }
    }

    pub fn states(self) -> KernelSemanticDefinitionStateIter<'a> {
        KernelSemanticDefinitionStateIter {
            input: self.input,
            owner: self.template.owner(),
            next: 0,
            len: u32::try_from(self.template.state_count())
                .expect("sealed kernel semantic state count fits u32"),
        }
    }

    pub fn lists(self) -> KernelSemanticDefinitionListIter<'a> {
        KernelSemanticDefinitionListIter {
            input: self.input,
            owner: self.template.owner(),
            next: 0,
            len: u32::try_from(self.template.list_count())
                .expect("sealed kernel semantic LIST count fits u32"),
        }
    }
}

impl<'a> Iterator for KernelSemanticDefinitionExecutionNodeIter<'a> {
    type Item = KernelSemanticDefinitionExecutionNodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.nodes.next()?;
        Some(KernelSemanticDefinitionExecutionNodeRef {
            input: self.input,
            node,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.nodes.size_hint()
    }
}

impl ExactSizeIterator for KernelSemanticDefinitionExecutionNodeIter<'_> {}

impl<'a> KernelSemanticDefinitionExecutionNodeRef<'a> {
    pub fn expression(self) -> CheckedExprId {
        self.input
            .relocate_expression(self.node.expression())
            .expect("sealed kernel semantic execution node relocates")
    }

    pub fn dependencies(self) -> KernelSemanticDefinitionExpressionIter<'a> {
        KernelSemanticDefinitionExpressionIter {
            input: self.input,
            rows: self.node.dependencies().iter(),
        }
    }

    pub fn call(self) -> Option<CheckedCallId> {
        self.node.call().map(|call| {
            self.input
                .relocate_call(call)
                .expect("sealed kernel semantic execution call relocates")
        })
    }

    pub fn selector(self) -> Option<KernelSemanticDefinitionSelectorRef<'a>> {
        self.node
            .selector()
            .map(|selector| KernelSemanticDefinitionSelectorRef {
                input: self.input,
                selector,
            })
    }
}

impl<'a> KernelSemanticDefinitionSelectorRef<'a> {
    pub fn input(self) -> CheckedExprId {
        self.input
            .relocate_expression(self.selector.input())
            .expect("sealed kernel semantic selector input relocates")
    }

    pub fn arms(self) -> KernelSemanticDefinitionExpressionIter<'a> {
        KernelSemanticDefinitionExpressionIter {
            input: self.input,
            rows: self.selector.arms().iter(),
        }
    }
}

impl<'a> Iterator for KernelSemanticDefinitionExpressionIter<'a> {
    type Item = CheckedExprId;

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next().map(|expression| {
            self.input
                .relocate_expression(*expression)
                .expect("sealed kernel semantic expression relocates")
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.rows.size_hint()
    }
}

impl DoubleEndedIterator for KernelSemanticDefinitionExpressionIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.rows.next_back().map(|expression| {
            self.input
                .relocate_expression(*expression)
                .expect("sealed kernel semantic expression relocates")
        })
    }
}

impl ExactSizeIterator for KernelSemanticDefinitionExpressionIter<'_> {}

impl<'a> Iterator for KernelSemanticDefinitionCallIter<'a> {
    type Item = CheckedCallId;

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next().map(|call| {
            self.input
                .relocate_call(*call)
                .expect("sealed kernel semantic call relocates")
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.rows.size_hint()
    }
}

impl ExactSizeIterator for KernelSemanticDefinitionCallIter<'_> {}

impl<'a> Iterator for KernelSemanticDefinitionSourceIter<'a> {
    type Item = CheckedSourceId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.len {
            return None;
        }
        let ordinal = self.next;
        self.next += 1;
        Some(
            self.input
                .relocate_source(self.owner, ordinal)
                .expect("sealed kernel semantic SOURCE relocates"),
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.next) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticDefinitionSourceIter<'_> {}

impl<'a> Iterator for KernelSemanticDefinitionStateIter<'a> {
    type Item = CheckedStateId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.len {
            return None;
        }
        let ordinal = self.next;
        self.next += 1;
        Some(
            self.input
                .relocate_state(self.owner, ordinal)
                .expect("sealed kernel semantic state relocates"),
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.next) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticDefinitionStateIter<'_> {}

impl<'a> Iterator for KernelSemanticDefinitionListIter<'a> {
    type Item = CheckedListId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.len {
            return None;
        }
        let ordinal = self.next;
        self.next += 1;
        Some(
            self.input
                .relocate_list(self.owner, ordinal)
                .expect("sealed kernel semantic LIST relocates"),
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.next) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticDefinitionListIter<'_> {}

impl KernelSemanticInputConstructionV1 {
    fn definition_relocation(
        &self,
        owner: KernelOwnerId,
    ) -> Option<&KernelSemanticDefinitionRelocationV1> {
        self.definition_relocations.get(owner.0 as usize)
    }

    fn relocate_expression(&self, expression: crate::PackedExpressionRef) -> Option<CheckedExprId> {
        let range = self.definition_relocation(expression.owner())?.expressions;
        range
            .resolve(expression.expression().0, "semantic expression")
            .ok()
            .map(CheckedExprId)
    }

    fn relocate_call(&self, call: crate::PackedCallRef) -> Option<CheckedCallId> {
        let range = self.definition_relocation(call.owner())?.calls;
        range
            .resolve(call.ordinal(), "semantic call")
            .ok()
            .map(CheckedCallId)
    }

    fn relocate_source(&self, owner: KernelOwnerId, ordinal: u32) -> Option<CheckedSourceId> {
        self.definition_relocation(owner)?
            .sources
            .resolve(ordinal, "semantic SOURCE")
            .ok()
            .map(CheckedSourceId)
    }

    fn relocate_state(&self, owner: KernelOwnerId, ordinal: u32) -> Option<CheckedStateId> {
        self.definition_relocation(owner)?
            .states
            .resolve(ordinal, "semantic state")
            .ok()
            .map(CheckedStateId)
    }

    fn relocate_list(&self, owner: KernelOwnerId, ordinal: u32) -> Option<CheckedListId> {
        self.definition_relocation(owner)?
            .lists
            .resolve(ordinal, "semantic LIST")
            .ok()
            .map(CheckedListId)
    }

    fn local_expression(&self, expression: CheckedExprId) -> Option<crate::PackedExpressionRef> {
        let owner = self
            .definition_relocations
            .partition_point(|relocation| relocation.expressions.start <= expression.0)
            .checked_sub(1)?;
        let range = self.definition_relocations.get(owner)?.expressions;
        let local = expression.0.checked_sub(range.start)?;
        if local >= range.len {
            return None;
        }
        Some(crate::PackedExpressionRef::new(
            KernelOwnerId(u32::try_from(owner).ok()?),
            crate::KernelExpressionId(local),
        ))
    }

    fn call_result_path_symbols(
        &self,
        path: KernelSemanticCallResultPathLocatorV1,
    ) -> Option<&[SymbolId]> {
        let start = path.projection_start as usize;
        let end = start.checked_add(path.projection_len as usize)?;
        self.call_result_path_symbols.get(start..end)
    }

    fn materialize_rich_definition_execution(&self) -> Box<[CheckedDefinitionExecutionTemplateV1]> {
        self.definition_execution_owners
            .iter()
            .copied()
            .map(|owner| {
                let template = self
                    .definition_code
                    .definition(owner)
                    .and_then(|code| code.execution_template())
                    .expect("sealed semantic execution owner has a packed template");
                let relocation = self
                    .definition_relocation(owner)
                    .expect("sealed semantic execution owner has a relocation");
                CheckedDefinitionExecutionTemplateV1 {
                    schema: CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1.to_owned(),
                    callable: relocation.callable,
                    result: self
                        .relocate_expression(template.result())
                        .expect("sealed semantic execution result relocates"),
                    nodes: template
                        .nodes()
                        .map(|node| CheckedDefinitionExecutionNodeV1 {
                            expression: self
                                .relocate_expression(node.expression())
                                .expect("sealed semantic execution node relocates"),
                            dependencies: node
                                .dependencies()
                                .iter()
                                .map(|dependency| {
                                    self.relocate_expression(*dependency)
                                        .expect("sealed semantic execution dependency relocates")
                                })
                                .collect(),
                            call: node.call().map(|call| {
                                self.relocate_call(call)
                                    .expect("sealed semantic execution call relocates")
                            }),
                            selector: node.selector().map(|selector| CheckedDefinitionSelectorV1 {
                                input: self
                                    .relocate_expression(selector.input())
                                    .expect("sealed semantic selector input relocates"),
                                arms: selector
                                    .arms()
                                    .iter()
                                    .map(|arm| {
                                        self.relocate_expression(*arm)
                                            .expect("sealed semantic selector arm relocates")
                                    })
                                    .collect(),
                            }),
                        })
                        .collect(),
                    calls: template
                        .calls()
                        .iter()
                        .map(|call| {
                            self.relocate_call(*call)
                                .expect("sealed semantic template call relocates")
                        })
                        .collect(),
                    sources: (0..template.source_count())
                        .map(|ordinal| {
                            self.relocate_source(owner, ordinal as u32)
                                .expect("sealed semantic template SOURCE relocates")
                        })
                        .collect(),
                    states: (0..template.state_count())
                        .map(|ordinal| {
                            self.relocate_state(owner, ordinal as u32)
                                .expect("sealed semantic template state relocates")
                        })
                        .collect(),
                    lists: (0..template.list_count())
                        .map(|ordinal| {
                            self.relocate_list(owner, ordinal as u32)
                                .expect("sealed semantic template LIST relocates")
                        })
                        .collect(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn validate_rich_definition_execution(
        &self,
        rich: &[CheckedDefinitionExecutionTemplateV1],
        allow_missing_runtime_projection: bool,
    ) -> Result<(), KernelCheckedLinkError> {
        if rich.is_empty() && allow_missing_runtime_projection {
            return Ok(());
        }
        if rich.len() != self.definition_execution_owners.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "checked editor image has {} rich definition templates but packed authority has {}",
                rich.len(),
                self.definition_execution_owners.len(),
            )));
        }
        for (ordinal, (rich, owner)) in rich
            .iter()
            .zip(self.definition_execution_owners.iter().copied())
            .enumerate()
        {
            let template = self
                .definition_code
                .definition(owner)
                .and_then(|code| code.execution_template())
                .expect("sealed semantic execution owner has a packed template");
            let relocation = self
                .definition_relocation(owner)
                .expect("sealed semantic execution owner has a relocation");
            if rich.schema != CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1
                || rich.callable != relocation.callable
                || Some(rich.result) != self.relocate_expression(template.result())
                || rich.calls.len() != template.calls().len()
                || !rich
                    .calls
                    .iter()
                    .copied()
                    .eq(template.calls().iter().map(|call| {
                        self.relocate_call(*call)
                            .expect("sealed semantic template call relocates")
                    }))
                || rich.sources.len() != template.source_count()
                || !rich
                    .sources
                    .iter()
                    .copied()
                    .eq((0..template.source_count()).map(|index| {
                        self.relocate_source(owner, index as u32)
                            .expect("sealed semantic template SOURCE relocates")
                    }))
                || rich.states.len() != template.state_count()
                || !rich
                    .states
                    .iter()
                    .copied()
                    .eq((0..template.state_count()).map(|index| {
                        self.relocate_state(owner, index as u32)
                            .expect("sealed semantic template state relocates")
                    }))
                || rich.lists.len() != template.list_count()
                || !rich
                    .lists
                    .iter()
                    .copied()
                    .eq((0..template.list_count()).map(|index| {
                        self.relocate_list(owner, index as u32)
                            .expect("sealed semantic template LIST relocates")
                    }))
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "checked editor definition template {ordinal} differs from packed header authority",
                )));
            }
            if rich.nodes.len() != template.nodes().len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "checked editor definition template {ordinal} has {} nodes but packed authority has {}",
                    rich.nodes.len(),
                    template.nodes().len(),
                )));
            }
            for (node_ordinal, (rich, packed)) in
                rich.nodes.iter().zip(template.nodes()).enumerate()
            {
                let selector_matches = match (&rich.selector, packed.selector()) {
                    (None, None) => true,
                    (Some(rich), Some(packed)) => {
                        Some(rich.input) == self.relocate_expression(packed.input())
                            && rich.arms.len() == packed.arms().len()
                            && rich
                                .arms
                                .iter()
                                .copied()
                                .eq(packed.arms().iter().map(|arm| {
                                    self.relocate_expression(*arm)
                                        .expect("sealed semantic selector arm relocates")
                                }))
                    }
                    (None, Some(_)) | (Some(_), None) => false,
                };
                if Some(rich.expression) != self.relocate_expression(packed.expression())
                    || rich.dependencies.len() != packed.dependencies().len()
                    || !rich
                        .dependencies
                        .iter()
                        .copied()
                        .eq(packed.dependencies().iter().map(|dependency| {
                            self.relocate_expression(*dependency)
                                .expect("sealed semantic execution dependency relocates")
                        }))
                    || rich.call
                        != packed.call().map(|call| {
                            self.relocate_call(call)
                                .expect("sealed semantic execution call relocates")
                        })
                    || !selector_matches
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "checked editor definition template {ordinal} node {node_ordinal} differs from packed authority",
                    )));
                }
            }
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn materialize_rich_definition_execution_templates(
        &self,
    ) -> Box<[CheckedDefinitionExecutionTemplateV1]> {
        self.materialize_rich_definition_execution()
    }

    pub fn resource_projection_count(&self) -> usize {
        self.resource_projections.len()
    }

    fn from_linked_rows(
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        projection_demand: KernelCheckedRowProjectionDemand,
        snapshot: &KernelCheckedSnapshot,
        layout: &KernelCheckedLinkLayout,
        call_result_paths: Box<[KernelSemanticCallResultPathLocatorV1]>,
        call_result_path_symbols: Box<[SymbolId]>,
        resource_projections: Box<[KernelSemanticResourceProjectionLocatorV1]>,
        checked_image_pairing: Arc<boon_checked::CheckedImageKernelPairingV1>,
    ) -> Result<Self, KernelCheckedLinkError> {
        if snapshot.definition_count() != layout.definitions.len()
            || snapshot.definition_code.definition_count() != layout.definitions.len()
        {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic input definition authorities disagree",
            ));
        }
        let mut definition_relocations = Vec::with_capacity(layout.definitions.len());
        let mut definition_execution_owners = Vec::new();
        for definition in &layout.definitions {
            let code = snapshot
                .definition_code
                .definition(definition.owner)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic input has no packed definition {}",
                        definition.owner.0,
                    ))
                })?;
            for (label, packed, linked) in [
                (
                    "expression",
                    code.expression_count(),
                    definition.expressions.len,
                ),
                ("call", code.call_count(), definition.calls.len),
                ("SOURCE", code.source_count(), definition.sources.len),
                ("state", code.state_count(), definition.states.len),
                ("LIST", code.list_count(), definition.lists.len),
            ] {
                if packed != linked as usize {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel semantic definition {} has {packed} packed {label} rows but {linked} linked rows",
                        definition.owner.0,
                    )));
                }
            }
            if let Some(template) = code.execution_template() {
                if template.source_count() != definition.sources.len as usize
                    || template.state_count() != definition.states.len as usize
                    || template.list_count() != definition.lists.len as usize
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel semantic execution template {} resource ranges disagree with its relocation",
                        definition.owner.0,
                    )));
                }
                definition_execution_owners.push(definition.owner);
            }
            definition_relocations.push(KernelSemanticDefinitionRelocationV1 {
                callable: definition.public_declaration,
                expressions: definition.expressions,
                calls: definition.calls,
                sources: definition.sources,
                states: definition.states,
                lists: definition.lists,
            });
        }
        definition_execution_owners
            .sort_unstable_by_key(|owner| definition_relocations[owner.0 as usize].callable.0);
        if definition_execution_owners.windows(2).any(|owners| {
            definition_relocations[owners[0].0 as usize].callable
                == definition_relocations[owners[1].0 as usize].callable
        }) {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic execution templates repeat a callable relocation",
            ));
        }
        let mut previous_call = None;
        let mut next_symbol = 0u32;
        for path in &call_result_paths {
            if path.call.0 >= layout.totals.calls {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic call-result path references missing call {}",
                    path.call.0,
                )));
            }
            if previous_call.is_some_and(|previous| previous >= path.call) {
                return Err(KernelCheckedLinkError::new(
                    "kernel semantic call-result paths are not strictly ordered",
                ));
            }
            previous_call = Some(path.call);
            if path.projection_start != next_symbol {
                return Err(KernelCheckedLinkError::new(
                    "kernel semantic call-result path symbols are not contiguous",
                ));
            }
            next_symbol = next_symbol
                .checked_add(path.projection_len)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(
                        "kernel semantic call-result path symbol range overflows u32",
                    )
                })?;
        }
        if next_symbol as usize != call_result_path_symbols.len()
            || call_result_path_symbols
                .iter()
                .any(|symbol| snapshot.definition_code.symbol(*symbol).is_none())
        {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic call-result path symbols differ from their text authority",
            ));
        }
        let mut resource_projection_by_expression =
            vec![u32::MAX; layout.totals.expressions as usize];
        for (index, requirement) in resource_projections.iter().enumerate() {
            let index = u32::try_from(index).map_err(|_| {
                KernelCheckedLinkError::new("kernel semantic resource-projection count exceeds u32")
            })?;
            let slot = resource_projection_by_expression
                .get_mut(requirement.expression.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic resource projection references missing expression {}",
                        requirement.expression.0,
                    ))
                })?;
            if *slot != u32::MAX {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic input repeats resource projection for expression {}",
                    requirement.expression.0,
                )));
            }
            *slot = index;
        }
        let definition_count = snapshot.definition_count();
        Ok(Self {
            source_bundle_digest_v1,
            role,
            definition_count,
            definition_code: Arc::clone(&snapshot.definition_code),
            expression_count: layout.totals.expressions,
            declaration_count: layout.totals.declarations.saturating_sub(1),
            source_count: layout.totals.sources,
            definition_relocations: definition_relocations.into_boxed_slice(),
            definition_execution_owners: definition_execution_owners.into_boxed_slice(),
            call_result_paths,
            call_result_path_symbols,
            resource_projections,
            resource_projection_by_expression: resource_projection_by_expression.into_boxed_slice(),
            rich_editor_projection_expected: matches!(
                projection_demand,
                KernelCheckedRowProjectionDemand::EditorRich
            ),
            checked_image_pairing,
        })
    }

    pub fn seal(
        self,
        checked: &CheckedProgram,
        pairing_receipt: &boon_checked::CheckedImageKernelPairingReceiptV1,
    ) -> Result<KernelSemanticInputV1, KernelCheckedLinkError> {
        self.validate_checked_shape(
            checked.source_bundle_digest_v1,
            checked.role,
            checked.expressions.len(),
            checked.declarations.len(),
            checked.sources.len(),
        )?;
        let handoff = checked.image_handoff();
        pairing_receipt
            .__kernel_validate(&self.checked_image_pairing, handoff)
            .map_err(KernelCheckedLinkError::new)?;
        let routed_resources = handoff
            .entity_routes
            .iter()
            .filter(|route| route.domain == CheckedImageRowDomainV2::ResourceProjection)
            .count();
        if routed_resources != self.resource_projections.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel semantic input has {} resource projections but checked image routes {routed_resources}",
                self.resource_projections.len(),
            )));
        }
        for (ordinal, requirement) in self.resource_projections.iter().enumerate() {
            let resource_projection = handoff
                .entity_projection(CheckedImageRowDomainV2::ResourceProjection, ordinal)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic resource projection {ordinal} has no checked-image route",
                    ))
                })?;
            let expression_projection = handoff
                .entity_projection(
                    CheckedImageRowDomainV2::Expression,
                    requirement.expression.0 as usize,
                )
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic resource projection {ordinal} expression {} has no checked-image route",
                        requirement.expression.0,
                    ))
                })?;
            if resource_projection != expression_projection {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic resource projection {ordinal} is detached from expression {}",
                    requirement.expression.0,
                )));
            }
            let target_projection = handoff
                .entity_projection(
                    CheckedImageRowDomainV2::Declaration,
                    requirement.target.0 as usize,
                )
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic resource projection {ordinal} target {} has no checked-image route",
                        requirement.target.0,
                    ))
                })?;
            if target_projection != resource_projection
                && !handoff
                    .projection_relocations(resource_projection)
                    .is_some_and(|relocations| relocations.contains(&target_projection))
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic resource projection {ordinal} has no relocation to target {}",
                    requirement.target.0,
                )));
            }
        }
        let input = KernelSemanticInputV1 {
            construction: self,
            checked_image_digest: handoff.local_image_digest,
        };
        if input.construction.rich_editor_projection_expected {
            input.validate_rich_call_result_paths(&checked.call_result_paths)?;
            input.validate_rich_definition_execution_templates(
                &checked.definition_execution_templates,
            )?;
            input.validate_rich_resource_projections(checked)?;
        }
        Ok(input)
    }

    fn validate_checked_shape(
        &self,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        expression_count: usize,
        declaration_count: usize,
        source_count: usize,
    ) -> Result<(), KernelCheckedLinkError> {
        if self.source_bundle_digest_v1 != source_bundle_digest_v1 {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic input source digest differs from checked image",
            ));
        }
        if self.role != role {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic input role differs from checked image",
            ));
        }
        for (label, actual, expected) in [
            (
                "expression",
                expression_count,
                self.expression_count as usize,
            ),
            (
                "declaration",
                declaration_count,
                self.declaration_count as usize,
            ),
            ("source", source_count, self.source_count as usize),
        ] {
            if actual != expected {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic input {label} count {expected} differs from checked count {actual}",
                )));
            }
        }
        Ok(())
    }
}

impl KernelSemanticInputV1 {
    fn definition_relocation(
        &self,
        owner: KernelOwnerId,
    ) -> Option<&KernelSemanticDefinitionRelocationV1> {
        self.construction.definition_relocation(owner)
    }

    fn relocate_expression(&self, expression: crate::PackedExpressionRef) -> Option<CheckedExprId> {
        self.construction.relocate_expression(expression)
    }

    fn relocate_call(&self, call: crate::PackedCallRef) -> Option<CheckedCallId> {
        self.construction.relocate_call(call)
    }

    fn relocate_source(&self, owner: KernelOwnerId, ordinal: u32) -> Option<CheckedSourceId> {
        self.construction.relocate_source(owner, ordinal)
    }

    fn relocate_state(&self, owner: KernelOwnerId, ordinal: u32) -> Option<CheckedStateId> {
        self.construction.relocate_state(owner, ordinal)
    }

    fn relocate_list(&self, owner: KernelOwnerId, ordinal: u32) -> Option<CheckedListId> {
        self.construction.relocate_list(owner, ordinal)
    }

    pub fn validate_checked_authority(
        &self,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        checked_image_digest: [u8; 32],
        expression_count: usize,
        declaration_count: usize,
        source_count: usize,
    ) -> Result<(), KernelCheckedLinkError> {
        self.construction.validate_checked_shape(
            source_bundle_digest_v1,
            role,
            expression_count,
            declaration_count,
            source_count,
        )?;
        if self.checked_image_digest != checked_image_digest {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic input checked-image digest is stale",
            ));
        }
        Ok(())
    }

    pub fn definition_count(&self) -> usize {
        self.construction.definition_count
    }

    /// Borrow one call-result path from the packed kernel authority.
    ///
    /// Absence is distinct from a present path with an empty projection: an
    /// empty function path intentionally contributes no textual prefix.
    pub fn call_result_path(
        &self,
        call: CheckedCallId,
    ) -> Option<KernelSemanticCallResultPathRef<'_>> {
        let index = self
            .construction
            .call_result_paths
            .binary_search_by_key(&call, |path| path.call)
            .ok()?;
        Some(KernelSemanticCallResultPathRef {
            input: self,
            locator: self.construction.call_result_paths[index],
        })
    }

    pub fn validate_rich_call_result_paths(
        &self,
        rich: &[CheckedCallResultPath],
    ) -> Result<(), KernelCheckedLinkError> {
        if rich.is_empty() && !self.construction.rich_editor_projection_expected {
            return Ok(());
        }
        if rich.len() != self.construction.call_result_paths.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "checked editor image has {} rich call-result paths but packed authority has {}",
                rich.len(),
                self.construction.call_result_paths.len(),
            )));
        }
        for (ordinal, (rich, packed)) in rich
            .iter()
            .zip(self.construction.call_result_paths.iter().copied())
            .enumerate()
        {
            let packed = KernelSemanticCallResultPathRef {
                input: self,
                locator: packed,
            };
            if rich.call != packed.call()
                || rich.path.anchor != packed.anchor()
                || !packed
                    .projection()
                    .eq(rich.path.projection.iter().map(String::as_str))
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "checked editor call-result path {ordinal} differs from packed authority",
                )));
            }
        }
        Ok(())
    }

    pub fn definition_execution_templates(
        &self,
    ) -> KernelSemanticDefinitionExecutionTemplateIter<'_> {
        KernelSemanticDefinitionExecutionTemplateIter {
            input: self,
            next: 0,
        }
    }

    pub fn definition_execution_template(
        &self,
        callable: DeclId,
    ) -> Option<KernelSemanticDefinitionExecutionTemplateRef<'_>> {
        let owners = &self.construction.definition_execution_owners;
        let index = owners
            .binary_search_by_key(&callable.0, |owner| {
                self.construction.definition_relocations[owner.0 as usize]
                    .callable
                    .0
            })
            .ok()?;
        let owner = owners[index];
        let template = self
            .construction
            .definition_code
            .definition(owner)?
            .execution_template()?;
        Some(KernelSemanticDefinitionExecutionTemplateRef {
            input: self,
            template,
        })
    }

    pub fn definition_execution_node(
        &self,
        expression: CheckedExprId,
    ) -> Option<(DeclId, KernelSemanticDefinitionExecutionNodeRef<'_>)> {
        let local = self.construction.local_expression(expression)?;
        let node = self.construction.definition_code.execution_node(local)?;
        let callable = self.definition_relocation(node.template_owner())?.callable;
        Some((
            callable,
            KernelSemanticDefinitionExecutionNodeRef { input: self, node },
        ))
    }

    pub fn validate_rich_definition_execution_templates(
        &self,
        rich: &[CheckedDefinitionExecutionTemplateV1],
    ) -> Result<(), KernelCheckedLinkError> {
        self.construction.validate_rich_definition_execution(
            rich,
            !self.construction.rich_editor_projection_expected,
        )
    }

    pub fn resource_projection_count(&self) -> usize {
        self.construction.resource_projections.len()
    }

    pub fn resource_projection(
        &self,
        expression: CheckedExprId,
    ) -> Option<KernelSemanticResourceProjectionRef<'_>> {
        let index = *self
            .construction
            .resource_projection_by_expression
            .get(expression.0 as usize)?;
        (index != u32::MAX).then(|| KernelSemanticResourceProjectionRef {
            input: self,
            locator: self.construction.resource_projections[index as usize],
        })
    }

    pub fn resource_projections(
        &self,
    ) -> impl ExactSizeIterator<Item = KernelSemanticResourceProjectionRef<'_>> {
        self.construction
            .resource_projections
            .iter()
            .copied()
            .map(|locator| KernelSemanticResourceProjectionRef {
                input: self,
                locator,
            })
    }

    /// Validate every editor DTO fact that semantic compilation consumes
    /// against this packed authority. Origin-free `required_type` values are
    /// presentation-only and are not rematerialized merely to compare them a
    /// second time. The ordinary runtime path supplies no rich rows and pays
    /// no projection cost; editor-to-verified promotion uses this check before
    /// discarding the already-inspected compatibility rows.
    pub fn validate_rich_resource_projections(
        &self,
        checked: &CheckedProgramFields,
    ) -> Result<(), KernelCheckedLinkError> {
        if checked.resource_projection_requirements.is_empty()
            && !self.construction.rich_editor_projection_expected
        {
            return Ok(());
        }
        if checked.resource_projection_requirements.len() != self.resource_projection_count() {
            return Err(KernelCheckedLinkError::new(format!(
                "checked editor image has {} rich resource projections but packed authority has {}",
                checked.resource_projection_requirements.len(),
                self.resource_projection_count(),
            )));
        }
        let mut seen = vec![false; self.resource_projection_count()];
        for rich in &checked.resource_projection_requirements {
            let index = self
                .construction
                .resource_projection_by_expression
                .get(rich.expression.0 as usize)
                .copied()
                .filter(|index| *index != u32::MAX)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "checked editor resource projection expression {} has no packed authority",
                        rich.expression.0,
                    ))
                })? as usize;
            if std::mem::replace(&mut seen[index], true) {
                return Err(KernelCheckedLinkError::new(format!(
                    "checked editor resource projection repeats expression {}",
                    rich.expression.0,
                )));
            }
            let packed = KernelSemanticResourceProjectionRef {
                input: self,
                locator: self.construction.resource_projections[index],
            };
            if packed.target() != rich.target
                || !packed
                    .projection()
                    .eq(rich.projection.iter().map(String::as_str))
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "checked editor resource projection {} differs from packed target/path authority",
                    rich.expression.0,
                )));
            }
            if packed.origin_count() != rich.source_origins.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "checked editor resource projection {} has {} origins but packed authority has {}",
                    rich.expression.0,
                    rich.source_origins.len(),
                    packed.origin_count(),
                )));
            }
            for (packed_origin, rich_origin) in packed.origins().zip(&rich.source_origins) {
                if packed_origin.source() != rich_origin.source
                    || !packed_origin
                        .payload_projection()
                        .eq(rich_origin.payload_projection.iter().map(String::as_str))
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "checked editor resource projection {} has an origin that differs from packed authority",
                        rich.expression.0,
                    )));
                }
            }
            if packed.origin_count() > 0 {
                let published_type = checked
                    .expressions
                    .get(rich.expression.0 as usize)
                    .filter(|expression| expression.id == rich.expression)
                    .map(|expression| &expression.flow_type.ty)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "checked editor resource projection references missing expression {}",
                            rich.expression.0,
                        ))
                    })?;
                if &rich.required_type != published_type {
                    return Err(KernelCheckedLinkError::new(format!(
                        "checked editor resource projection {} required type differs from its packed published type",
                        rich.expression.0,
                    )));
                }
            }
        }
        Ok(())
    }
}

impl<'a> KernelSemanticCallResultPathRef<'a> {
    pub const fn call(self) -> CheckedCallId {
        self.locator.call
    }

    pub const fn anchor(self) -> DeclId {
        self.locator.anchor
    }

    pub const fn projection_len(self) -> usize {
        self.locator.projection_len as usize
    }

    pub fn projection(self) -> impl ExactSizeIterator<Item = &'a str> {
        let input = self.input;
        input
            .construction
            .call_result_path_symbols(self.locator)
            .expect("sealed kernel call-result path symbol range is valid")
            .iter()
            .map(move |symbol| {
                input
                    .construction
                    .definition_code
                    .symbol(*symbol)
                    .expect("sealed kernel call-result path symbol belongs to its text authority")
            })
    }
}

impl<'a> KernelSemanticResourceProjectionRef<'a> {
    fn code(self) -> crate::DefinitionCodeRef<'a> {
        self.input
            .construction
            .definition_code
            .definition(self.locator.owner)
            .expect("sealed kernel semantic definition exists")
    }

    pub const fn expression(self) -> CheckedExprId {
        self.locator.expression
    }

    pub const fn target(self) -> DeclId {
        self.locator.target
    }

    pub fn projection(self) -> impl ExactSizeIterator<Item = &'a str> {
        let input = self.input;
        self.code()
            .resource_projection_path_symbols(self.locator.ordinal as usize)
            .expect("sealed kernel semantic resource projection path exists")
            .iter()
            .map(move |symbol| {
                input
                    .construction
                    .definition_code
                    .symbol(*symbol)
                    .expect("sealed kernel semantic symbol belongs to its text authority")
            })
    }

    pub fn origin_count(self) -> usize {
        self.code()
            .resource_projection_origin_count(self.locator.ordinal as usize)
            .expect("sealed kernel semantic resource projection exists")
    }

    pub fn origins(self) -> KernelSemanticResourceOriginIter<'a> {
        KernelSemanticResourceOriginIter {
            input: self.input,
            owner: self.locator.owner,
            requirement_ordinal: self.locator.ordinal as usize,
            next: 0,
            len: self.origin_count(),
        }
    }
}

impl<'a> Iterator for KernelSemanticResourceOriginIter<'a> {
    type Item = KernelSemanticResourceOriginRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.len {
            return None;
        }
        let origin_ordinal = self.next;
        self.next += 1;
        let code = self
            .input
            .construction
            .definition_code
            .definition(self.owner)
            .expect("sealed kernel semantic definition exists");
        let (source_owner, source, payload_projection) = code
            .resource_projection_origin(self.requirement_ordinal, origin_ordinal)
            .expect("sealed kernel semantic resource origin exists");
        Some(KernelSemanticResourceOriginRef {
            input: self.input,
            owner: self.owner,
            source_owner,
            source,
            payload_projection,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.next);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticResourceOriginIter<'_> {}

impl<'a> KernelSemanticResourceOriginRef<'a> {
    pub fn source(self) -> CheckedSourceId {
        let range = self
            .input
            .construction
            .definition_relocations
            .get(self.source_owner.0 as usize)
            .expect("sealed kernel semantic resource origin owner exists")
            .sources;
        CheckedSourceId(
            range
                .resolve(self.source.0, "semantic source")
                .expect("sealed kernel semantic resource origin relocates"),
        )
    }

    pub fn payload_projection(self) -> impl ExactSizeIterator<Item = &'a str> {
        let input = self.input;
        self.payload_projection.iter().map(move |symbol| {
            input
                .construction
                .definition_code
                .symbol(*symbol)
                .expect("sealed kernel semantic symbol belongs to its text authority")
        })
    }

    pub const fn requirement_owner(self) -> KernelOwnerId {
        self.owner
    }
}

struct KernelCheckedBaseRows {
    scopes: Box<[CheckedScope]>,
    declarations: Vec<CheckedDeclaration>,
    statements: Box<[CheckedStatement]>,
    callables: Vec<CheckedCallableSignature>,
    context_formals: Box<[CheckedContextFormal]>,
    sources: Box<[CheckedSource]>,
    states: Box<[CheckedState]>,
    lists: Box<[CheckedList]>,
}

fn checked_link_owner(callable: &CheckedCallableSignature) -> CheckedShardOwnerKeyV2 {
    CheckedShardOwnerKeyV2::Callable {
        role: callable.role,
        callable_kind: CheckedShardCallableKindV2::from(callable.kind),
        name: callable.name.clone(),
        external_identity: callable.external_identity,
    }
}

fn checked_link_definition_projection(
    owner: CheckedShardOwnerKeyV2,
) -> CheckedShardProjectionKeyV2 {
    CheckedShardProjectionKeyV2 {
        owner,
        region: CheckedShardRegionV2::Definition,
    }
}

fn checked_link_interface_projection(owner: CheckedShardOwnerKeyV2) -> CheckedShardProjectionKeyV2 {
    CheckedShardProjectionKeyV2 {
        owner,
        region: CheckedShardRegionV2::Interface,
    }
}

fn checked_link_owner_for_scope(
    scopes: &[CheckedScope],
    callable_owners: &BTreeMap<DeclId, CheckedShardOwnerKeyV2>,
    role: ProgramRole,
    mut scope: LexicalScopeId,
) -> Result<CheckedShardOwnerKeyV2, KernelCheckedLinkError> {
    let mut remaining = scopes.len().saturating_add(1);
    loop {
        if remaining == 0 {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked-image scope {} has an ownership cycle",
                scope.0
            )));
        }
        remaining -= 1;
        let current = scopes
            .get(scope.0 as usize)
            .filter(|candidate| candidate.id == scope)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked-image publication references missing scope {}",
                    scope.0
                ))
            })?;
        if current.kind == CheckedScopeKind::Function
            && let Some(owner) = current.owner
        {
            return callable_owners.get(&owner).cloned().ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked-image function scope {} has no callable owner {}",
                    scope.0, owner.0
                ))
            });
        }
        let Some(parent) = current.parent else {
            return Ok(CheckedShardOwnerKeyV2::ProgramTopLevel { role });
        };
        scope = parent;
    }
}

fn checked_link_declaration<'a>(
    declarations: &'a [CheckedDeclaration],
    id: DeclId,
) -> Option<&'a CheckedDeclaration> {
    id.0.checked_sub(1)
        .and_then(|index| declarations.get(index as usize))
        .filter(|candidate| candidate.id == id)
}

fn checked_link_semantic_path(
    scopes: &[CheckedScope],
    declarations: &[CheckedDeclaration],
    path: &CheckedSemanticPath,
) -> Option<String> {
    let declaration = checked_link_declaration(declarations, path.anchor)?;
    if declaration.kind == CheckedDeclarationKind::Function {
        return (!path.projection.is_empty()).then(|| path.projection.join("."));
    }
    let mut segments = vec![declaration.name.clone()];
    let mut scope = declaration.scope_id;
    let mut visited = BTreeSet::new();
    while scope != LexicalScopeId(0) && visited.insert(scope) {
        let current = scopes
            .get(scope.0 as usize)
            .filter(|candidate| candidate.id == scope)?;
        if current.kind == CheckedScopeKind::Function {
            break;
        }
        if let Some(owner) = current.owner
            && let Some(owner) = checked_link_declaration(declarations, owner)
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
    let mut result = segments.join(".");
    if !path.projection.is_empty() {
        result.push('.');
        result.push_str(&path.projection.join("."));
    }
    Some(result)
}

fn checked_link_authority_projection(
    scopes: &[CheckedScope],
    declarations: &[CheckedDeclaration],
    owner: CheckedShardOwnerKeyV2,
    path: &CheckedSemanticPath,
) -> CheckedShardProjectionKeyV2 {
    if matches!(owner, CheckedShardOwnerKeyV2::ProgramTopLevel { .. })
        && let Some(canonical_path) = checked_link_semantic_path(scopes, declarations, path)
    {
        return CheckedShardProjectionKeyV2 {
            owner,
            region: CheckedShardRegionV2::TopLevelAuthority { canonical_path },
        };
    }
    checked_link_definition_projection(owner)
}

#[allow(clippy::too_many_arguments)]
fn checked_image_publication_v1(
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    scopes: &[CheckedScope],
    declarations: &[CheckedDeclaration],
    statements: &[CheckedStatement],
    expressions: &[CheckedExpression],
    callables: &[CheckedCallableSignature],
    context_formals: &[CheckedContextFormal],
    calls: &[CheckedCall],
    call_occurrences: &[StableOccurrenceKey],
    call_result_paths: &[KernelSemanticCallResultPathLocatorV1],
    pattern_bindings: &[CheckedPatternBinding],
    resource_projection_requirements: &[KernelSemanticResourceProjectionLocatorV1],
    sources: &[CheckedSource],
    states: &[CheckedState],
    lists: &[CheckedList],
    occurrence_targets: &[DeclId],
) -> Result<CheckedImageKernelPublicationV1, KernelCheckedLinkError> {
    let error = |message: String| KernelCheckedLinkError::new(message);
    let root_owner = CheckedShardOwnerKeyV2::ProgramTopLevel { role };
    let root_definition = checked_link_definition_projection(root_owner.clone());
    let root_interface = checked_link_interface_projection(root_owner.clone());
    let callable_owners = callables
        .iter()
        .map(|callable| (callable.decl_id, checked_link_owner(callable)))
        .collect::<BTreeMap<_, _>>();
    if callable_owners.len() != callables.len() {
        return Err(KernelCheckedLinkError::new(
            "kernel checked-image publication received duplicate callable declarations",
        ));
    }
    let scope_owners = scopes
        .iter()
        .map(|scope| checked_link_owner_for_scope(scopes, &callable_owners, role, scope.id))
        .collect::<Result<Vec<_>, _>>()?;
    let mut publication =
        CheckedImageKernelPublicationV1::__kernel_new(source_bundle_digest_v1, role);
    let root_definition_id = publication
        .__kernel_intern_projection(root_definition.clone())
        .map_err(error)?;
    let root_interface_id = publication
        .__kernel_intern_projection(root_interface)
        .map_err(error)?;
    publication
        .__kernel_publish_rows(root_interface_id, 1)
        .map_err(error)?;

    let mut scope_projections = Vec::with_capacity(scopes.len());
    for scope in scopes {
        let projection = publication
            .__kernel_intern_projection(checked_link_definition_projection(
                scope_owners[scope.id.0 as usize].clone(),
            ))
            .map_err(&error)?;
        publication
            .__kernel_publish_rows(projection, 1)
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::Scope,
                scope.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
        scope_projections.push(projection);
    }

    let declaration_slots = declarations
        .last()
        .map(|declaration| declaration.id.0 as usize + 1)
        .unwrap_or(1);
    let mut declaration_projections = vec![None; declaration_slots];
    for declaration in declarations {
        let owner = if declaration.kind == CheckedDeclarationKind::Function {
            callable_owners
                .get(&declaration.id)
                .cloned()
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel function declaration {} has no stable callable owner",
                        declaration.id.0
                    ))
                })?
        } else {
            scope_owners
                .get(declaration.scope_id.0 as usize)
                .cloned()
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel declaration {} references missing scope {}",
                        declaration.id.0, declaration.scope_id.0
                    ))
                })?
        };
        let projection = publication
            .__kernel_intern_projection(checked_link_definition_projection(owner))
            .map_err(&error)?;
        publication
            .__kernel_publish_rows(projection, 1)
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::Declaration,
                declaration.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
        let slot = declaration_projections
            .get_mut(declaration.id.0 as usize)
            .ok_or_else(|| KernelCheckedLinkError::new("declaration route exceeds dense table"))?;
        if slot.replace(projection).is_some() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel declaration {} is published twice",
                declaration.id.0
            )));
        }
    }

    let mut expression_projections = Vec::with_capacity(expressions.len());
    for expression in expressions {
        if expression.id.0 as usize != expression_projections.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked expression {} is non-dense",
                expression.id.0
            )));
        }
        let owner = scope_owners
            .get(expression.scope_id.0 as usize)
            .cloned()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel expression {} references missing scope {}",
                    expression.id.0, expression.scope_id.0
                ))
            })?;
        let projection = publication
            .__kernel_intern_projection(checked_link_definition_projection(owner))
            .map_err(&error)?;
        expression_projections.push(projection);
    }

    let mut statement_projections = Vec::with_capacity(statements.len());
    for statement in statements {
        if statement.id.0 as usize != statement_projections.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked statement {} is non-dense",
                statement.id.0
            )));
        }
        let owner = scope_owners
            .get(statement.scope_id.0 as usize)
            .cloned()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel statement {} references missing scope {}",
                    statement.id.0, statement.scope_id.0
                ))
            })?;
        statement_projections.push(
            publication
                .__kernel_intern_projection(checked_link_definition_projection(owner))
                .map_err(&error)?,
        );
    }

    for statement in statements {
        let projection = statement_projections[statement.id.0 as usize];
        publication
            .__kernel_publish_dependency_row(
                projection,
                statement
                    .value
                    .map(|value| expression_projections[value.0 as usize]),
            )
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::Statement,
                statement.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
    }
    for expression in expressions {
        let projection = expression_projections[expression.id.0 as usize];
        let target = match &expression.kind {
            CheckedExpressionKind::Read { target, .. }
            | CheckedExpressionKind::Drain { target, .. } => declaration_projections
                .get(target.0 as usize)
                .and_then(|projection| *projection),
            _ => None,
        };
        publication
            .__kernel_publish_dependency_row(projection, target)
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::Expression,
                expression.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
    }

    let mut callable_projections = BTreeMap::new();
    for callable in callables {
        let owner = callable_owners
            .get(&callable.decl_id)
            .cloned()
            .ok_or_else(|| KernelCheckedLinkError::new("callable owner index is incomplete"))?;
        let projection = publication
            .__kernel_intern_projection(checked_link_interface_projection(owner))
            .map_err(&error)?;
        publication
            .__kernel_publish_rows(projection, 1)
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::Callable,
                callable.decl_id.0 as usize,
                projection,
            )
            .map_err(&error)?;
        callable_projections.insert(callable.decl_id, projection);
    }
    for formal in context_formals {
        let projection = *callable_projections.get(&formal.callable).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "context formal {} references missing callable {}",
                formal.id.0, formal.callable.0
            ))
        })?;
        publication
            .__kernel_publish_rows(projection, 1)
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::ContextFormal,
                formal.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
    }

    if calls.len() != call_occurrences.len() {
        return Err(KernelCheckedLinkError::new(
            "kernel checked calls and structural occurrences are not aligned",
        ));
    }
    let mut structural_sites = BTreeMap::new();
    let mut call_projections = Vec::with_capacity(calls.len());
    for (call, occurrence) in calls.iter().zip(call_occurrences) {
        let digest =
            boon_checked::checked_structural_call_site_digest_v4(occurrence).map_err(&error)?;
        if structural_sites.insert(digest, occurrence).is_some() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked calls share structural occurrence {occurrence:?}"
            )));
        }
        let owner = call
            .owner_callable
            .and_then(|owner| callable_owners.get(&owner).cloned())
            .unwrap_or_else(|| root_owner.clone());
        let projection = publication
            .__kernel_intern_projection(CheckedShardProjectionKeyV2 {
                owner,
                region: CheckedShardRegionV2::Invocation {
                    authored_call_site_digest: digest,
                    identical_site_reverse_ordinal: 0,
                },
            })
            .map_err(&error)?;
        let callee = *callable_projections.get(&call.callable).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel call {} references missing callable {}",
                call.id.0, call.callable.0
            ))
        })?;
        publication
            .__kernel_publish_dependency_row(projection, [callee])
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::Call,
                call.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
        call_projections.push(projection);
    }
    for path in call_result_paths {
        let projection = call_projections
            .get(path.call.0 as usize)
            .copied()
            .ok_or_else(|| KernelCheckedLinkError::new("call result path has no call route"))?;
        publication
            .__kernel_publish_rows(projection, 1)
            .map_err(&error)?;
    }
    for (index, binding) in pattern_bindings.iter().enumerate() {
        let projection = declaration_projections
            .get(binding.declaration.0 as usize)
            .and_then(|projection| *projection)
            .unwrap_or(root_definition_id);
        publication
            .__kernel_publish_rows(projection, 1)
            .map_err(&error)?;
        publication
            .__kernel_route(CheckedImageRowDomainV2::PatternBinding, index, projection)
            .map_err(&error)?;
    }
    for (index, requirement) in resource_projection_requirements.iter().enumerate() {
        let projection = expression_projections
            .get(requirement.expression.0 as usize)
            .copied()
            .unwrap_or(root_definition_id);
        let target = declaration_projections
            .get(requirement.target.0 as usize)
            .and_then(|projection| *projection);
        publication
            .__kernel_publish_dependency_row(projection, target)
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::ResourceProjection,
                index,
                projection,
            )
            .map_err(&error)?;
    }

    for source in sources {
        let owner = scope_owners[source.owner_scope.0 as usize].clone();
        let projection = publication
            .__kernel_intern_projection(checked_link_authority_projection(
                scopes,
                declarations,
                owner,
                &source.path,
            ))
            .map_err(&error)?;
        publication
            .__kernel_publish_dependency_row(
                projection,
                [expression_projections[source.expression.0 as usize]],
            )
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::Source,
                source.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
    }
    for state in states {
        let owner = scope_owners[state.owner_scope.0 as usize].clone();
        let projection = publication
            .__kernel_intern_projection(checked_link_authority_projection(
                scopes,
                declarations,
                owner,
                &state.path,
            ))
            .map_err(&error)?;
        let binding = declaration_projections
            .get(state.binding_declaration.0 as usize)
            .and_then(|projection| *projection);
        publication
            .__kernel_publish_dependency_row(
                projection,
                [
                    Some(expression_projections[state.expression.0 as usize]),
                    Some(expression_projections[state.initial.0 as usize]),
                    binding,
                ]
                .into_iter()
                .flatten(),
            )
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::State,
                state.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
    }
    for list in lists {
        let owner = scope_owners[list.owner_scope.0 as usize].clone();
        let projection = publication
            .__kernel_intern_projection(checked_link_authority_projection(
                scopes,
                declarations,
                owner,
                &list.path,
            ))
            .map_err(&error)?;
        publication
            .__kernel_publish_dependency_row(
                projection,
                [expression_projections[list.producer.0 as usize]],
            )
            .map_err(&error)?;
        publication
            .__kernel_route(
                CheckedImageRowDomainV2::List,
                list.id.0 as usize,
                projection,
            )
            .map_err(&error)?;
    }
    for (index, target) in occurrence_targets.iter().copied().enumerate() {
        let projection = declaration_projections
            .get(target.0 as usize)
            .and_then(|projection| *projection)
            .unwrap_or(root_definition_id);
        publication
            .__kernel_publish_rows(projection, 1)
            .map_err(&error)?;
        publication
            .__kernel_route(CheckedImageRowDomainV2::Occurrence, index, projection)
            .map_err(&error)?;
    }
    Ok(publication)
}

impl KernelCheckedLinkLayout {
    pub fn new(
        project: &KernelProjectInput,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Self, KernelCheckedLinkError> {
        if project.definition_count() != snapshot.definition_count() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked linker received {} project definitions and {} artifacts",
                project.definition_count(),
                snapshot.definition_count(),
            )));
        }
        if snapshot.definition_facts.len() != snapshot.definition_count() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked linker received {} immutable fact rows and {} solved artifacts",
                snapshot.definition_facts.len(),
                snapshot.definition_count(),
            )));
        }
        if snapshot.definition_code.definition_count() != snapshot.definition_count() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked linker received {} rich definitions and {} packed definitions",
                snapshot.definition_count(),
                snapshot.definition_code.definition_count(),
            )));
        }
        let mut totals = KernelCheckedLinkTotals::default();
        totals.scopes = 1;
        // DeclId(0) is the language-wide absent/external-identity sentinel.
        // Direct checked rows therefore begin at one even though every
        // definition keeps zero-based local declaration IDs.
        totals.declarations = 1;
        let mut definitions = Vec::with_capacity(snapshot.definition_count());
        let mut public_declaration_authorities = Vec::with_capacity(snapshot.definition_count());
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            let facts = definition.facts();
            let code = definition.code();
            let scopes = take_range(&mut totals.scopes, facts.presentation.scopes.len(), "scope")?;
            let expressions = take_range(
                &mut totals.expressions,
                definition.input().nodes.len(),
                "expression",
            )?;
            let statements =
                take_range(&mut totals.statements, facts.statements.len(), "statement")?;
            let declarations = take_range(
                &mut totals.declarations,
                facts.declarations.len(),
                "declaration",
            )?;
            let type_variables = take_range(
                &mut totals.type_variables,
                code.alpha_variable_count(),
                "type variable",
            )?;
            let calls = take_range(&mut totals.calls, definition.call_count(), "call")?;
            let sources = take_range(&mut totals.sources, facts.sources.len(), "source")?;
            let states = take_range(&mut totals.states, definition.state_count(), "state")?;
            let lists = take_range(&mut totals.lists, facts.lists.len(), "list")?;
            let linkage = definition.linkage();
            let root_statement = linkage.root_statement.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} omits its direct-linker root statement",
                    owner.0,
                ))
            })?;
            if matches!(
                facts.statements.get(root_statement.0 as usize),
                Some(crate::KernelStatementInput {
                    kind: crate::KernelStatementKind::Function { .. },
                    ..
                })
            ) {
                totals.user_callables = totals.user_callables.checked_add(1).ok_or_else(|| {
                    KernelCheckedLinkError::new(
                        "kernel checked linker callable namespace exceeds u32",
                    )
                })?;
            }
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
                    if ordinal as usize >= code.formals().len() {
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
        totals.callables = totals.user_callables;
        let definition_declarations_end = totals.declarations;
        let referenced_abi_callables = referenced_abi_callable_names(snapshot)?;
        let mut abi_callables = Vec::with_capacity(referenced_abi_callables.len());
        let mut abi_callable_by_name = BTreeMap::new();
        for name in referenced_abi_callables {
            let callable = project.abi().callable(&name).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked linker references ABI callable `{name}` absent from its immutable project ABI"
                ))
            })?;
            let declaration = DeclId(totals.declarations);
            totals.declarations = totals.declarations.checked_add(1).ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked linker declaration namespace exceeds u32",
                )
            })?;
            let parameter_range = take_range(
                &mut totals.declarations,
                callable.parameters.len(),
                "ABI parameter declaration",
            )?;
            let parameters = (0..parameter_range.len)
                .map(|ordinal| DeclId(parameter_range.start + ordinal))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let type_variable_ordinals = abi_callable_type_variables(callable);
            if type_variable_ordinals
                .iter()
                .enumerate()
                .any(|(expected, variable)| variable.0 as usize != expected)
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel ABI callable `{name}` has a non-dense local type-variable namespace {:?}",
                    type_variable_ordinals,
                )));
            }
            let type_variables = take_range(
                &mut totals.type_variables,
                type_variable_ordinals.len(),
                "ABI type variable",
            )?;
            let index = abi_callables.len();
            if abi_callable_by_name
                .insert(name.clone().into_boxed_str(), index)
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked linker repeats referenced ABI callable `{name}`"
                )));
            }
            abi_callables.push(KernelCheckedAbiCallableLayout {
                name: name.into_boxed_str(),
                declaration,
                parameters,
                type_variables,
            });
            totals.abi_callables = totals.abi_callables.checked_add(1).ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked linker ABI callable namespace exceeds u32",
                )
            })?;
            totals.callables = totals.callables.checked_add(1).ok_or_else(|| {
                KernelCheckedLinkError::new("kernel checked linker callable namespace exceeds u32")
            })?;
        }
        for (index, definition) in snapshot.definition_refs().enumerate() {
            let facts = definition.facts();
            definitions[index].containing_scope = match facts.presentation.containing_scope {
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
            abi_callables: abi_callables.into_boxed_slice(),
            abi_callable_by_name,
            definition_declarations_end,
            totals,
        };
        layout.validate_references(snapshot)?;
        Ok(layout)
    }

    /// Materialize every currently kernel-owned checked row through this one
    /// relocation plan. ABI rows are appended before calls are linked so call
    /// targets resolve in the same namespace as user definitions.
    pub fn materialize_rows(
        &self,
        project: &KernelProjectInput,
        snapshot: &KernelCheckedSnapshot,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        projection_demand: KernelCheckedRowProjectionDemand,
    ) -> Result<KernelCheckedRows, KernelCheckedLinkError> {
        let materialize_expression_rows = || {
            Ok::<_, KernelCheckedLinkError>((
                self.materialize_expressions(snapshot)?.into_vec(),
                self.materialize_runtime_flow_terms(snapshot)?,
            ))
        };
        #[cfg(not(target_family = "wasm"))]
        let (base, (expressions, runtime_flow_terms)) =
            if crate::experimental_parallel_projection_enabled()
                && self.totals.expressions >= 4096
                && std::thread::available_parallelism()
                    .is_ok_and(|parallelism| parallelism.get() >= 2)
            {
                std::thread::scope(|scope| {
                    let expression_worker = scope.spawn(materialize_expression_rows);
                    let base = self.materialize_base_rows(project, snapshot, role)?;
                    let expressions = expression_worker.join().map_err(|_| {
                        KernelCheckedLinkError::new(
                            "kernel checked expression materialization worker panicked",
                        )
                    })??;
                    Ok::<_, KernelCheckedLinkError>((base, expressions))
                })?
            } else {
                (
                    self.materialize_base_rows(project, snapshot, role)?,
                    materialize_expression_rows()?,
                )
            };
        #[cfg(target_family = "wasm")]
        let (base, (expressions, runtime_flow_terms)) = (
            self.materialize_base_rows(project, snapshot, role)?,
            materialize_expression_rows()?,
        );
        let KernelCheckedBaseRows {
            scopes,
            declarations,
            statements,
            callables,
            context_formals,
            sources,
            states,
            lists,
        } = base;
        let (calls, call_occurrences) =
            self.materialize_calls(project, snapshot, &callables, &declarations)?;
        let (packed_call_result_paths, packed_call_result_path_symbols) =
            self.pack_call_result_paths(snapshot, &declarations, &callables, &expressions, &calls)?;
        let call_result_paths = match projection_demand {
            KernelCheckedRowProjectionDemand::RuntimePacked => Box::new([]),
            KernelCheckedRowProjectionDemand::EditorRich => self.materialize_call_result_paths(
                snapshot,
                &packed_call_result_paths,
                &packed_call_result_path_symbols,
            )?,
        };
        let pattern_bindings = self.materialize_pattern_bindings(snapshot)?;
        let semantic_resource_projections = self.semantic_resource_projection_locators(snapshot)?;
        #[cfg(test)]
        {
            let replay =
                CheckedRuntimeFlowTermProjectionV1::derive_from_checked_expressions(&expressions)
                    .map_err(KernelCheckedLinkError::new)?;
            if runtime_flow_terms != replay {
                return Err(KernelCheckedLinkError::new(
                    "kernel direct runtime flow-term handoff differs from rich checked replay",
                ));
            }
        }
        let occurrence_targets = self.occurrence_targets(snapshot, &expressions, &calls)?;
        let (occurrences, occurrence_ranges): (
            Box<[SemanticOccurrence]>,
            Box<[KernelCheckedRowRange]>,
        ) = match projection_demand {
            KernelCheckedRowProjectionDemand::RuntimePacked => (Box::new([]), Box::new([])),
            KernelCheckedRowProjectionDemand::EditorRich => {
                let (occurrences, ranges) =
                    self.materialize_occurrences(snapshot, &declarations, &expressions, &calls)?;
                if !occurrences
                    .iter()
                    .map(|occurrence| occurrence.target)
                    .eq(occurrence_targets.iter().copied())
                {
                    return Err(KernelCheckedLinkError::new(
                        "kernel packed occurrence topology differs from rich editor rows",
                    ));
                }
                (occurrences, ranges)
            }
        };
        #[cfg(test)]
        let rich_definition_execution_oracle = self
            .build_semantic_definition_execution_store_rich_oracle(
                snapshot,
                &scopes,
                &declarations,
                &statements,
                &calls,
            )?;
        let checked_image_publication = checked_image_publication_v1(
            source_bundle_digest_v1,
            role,
            &scopes,
            &declarations,
            &statements,
            &expressions,
            &callables,
            &context_formals,
            &calls,
            &call_occurrences,
            &packed_call_result_paths,
            &pattern_bindings,
            &semantic_resource_projections,
            &sources,
            &states,
            &lists,
            &occurrence_targets,
        )?;
        let semantic_input = KernelSemanticInputConstructionV1::from_linked_rows(
            source_bundle_digest_v1,
            role,
            projection_demand,
            snapshot,
            self,
            packed_call_result_paths,
            packed_call_result_path_symbols,
            semantic_resource_projections,
            checked_image_publication.__kernel_pairing(),
        )?;
        #[cfg(test)]
        if semantic_input.materialize_rich_definition_execution_templates()
            != rich_definition_execution_oracle
        {
            return Err(KernelCheckedLinkError::new(
                "borrowed packed definition execution differs from the independent rich oracle",
            ));
        }
        Ok(KernelCheckedRows {
            scopes,
            declarations: declarations.into_boxed_slice(),
            expressions: expressions.into_boxed_slice(),
            statements,
            callables: callables.into_boxed_slice(),
            context_formals,
            calls,
            call_occurrences,
            call_result_paths,
            pattern_bindings,
            semantic_input,
            sources,
            states,
            lists,
            runtime_flow_terms,
            checked_image_publication,
            occurrences,
            occurrence_ranges,
        })
    }

    fn materialize_base_rows(
        &self,
        project: &KernelProjectInput,
        snapshot: &KernelCheckedSnapshot,
        role: ProgramRole,
    ) -> Result<KernelCheckedBaseRows, KernelCheckedLinkError> {
        let scopes = self.materialize_scopes(snapshot)?;
        let mut declarations = self.materialize_declarations(snapshot)?.into_vec();
        let statements = self.materialize_statements(snapshot)?;
        let sources = self.materialize_sources(snapshot)?;
        let states = self.materialize_states(snapshot)?;
        let lists = self.materialize_lists(snapshot)?;
        let (user_callables, context_formals) = self.materialize_user_callables(snapshot, role)?;
        let mut callables = user_callables.into_vec();
        let (abi_callables, abi_declarations) = self.materialize_abi_callables(project.abi())?;
        callables.extend(abi_callables);
        declarations.extend(abi_declarations);
        Ok(KernelCheckedBaseRows {
            scopes,
            declarations,
            statements,
            callables,
            context_formals,
            sources,
            states,
            lists,
        })
    }

    fn materialize_runtime_flow_terms(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<CheckedRuntimeFlowTermProjectionV1, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "runtime flow-term handoff")?;
        let mut digests = vec![None; self.totals.expressions as usize];
        for (definition, layout) in snapshot.definition_refs().zip(self.definitions.iter()) {
            let owner_index = definition.owner().0 as usize;
            let code = definition.code();
            if code.expressions().len() != definition.input().nodes.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner_index} has {} expression term roots for {} expressions",
                    code.expressions().len(),
                    definition.input().nodes.len()
                )));
            }
            for (local, _) in code.expressions().iter().enumerate() {
                let flow = code.published_expression(local).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {owner_index} has no published expression term {local}"
                    ))
                })?;
                let local = u32::try_from(local).map_err(|_| {
                    KernelCheckedLinkError::new(
                        "kernel definition expression term count exceeds u32",
                    )
                })?;
                let global = layout
                    .expressions
                    .resolve(local, "runtime flow-term expression")?
                    as usize;
                let slot = digests.get_mut(global).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel runtime flow-term expression {global} exceeds its dense table"
                    ))
                })?;
                if slot.replace(flow.runtime_erased_digest).is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel runtime flow-term expression {global} is published twice"
                    )));
                }
            }
        }
        let digests = digests
            .into_iter()
            .enumerate()
            .map(|(expression, digest)| {
                digest.ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel runtime flow-term handoff omits expression {expression}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CheckedRuntimeFlowTermProjectionV1::from_runtime_flow_digests(digests))
    }

    fn semantic_resource_projection_locators(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[KernelSemanticResourceProjectionLocatorV1]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "semantic resource projection")?;
        let mut requirements = Vec::new();
        for owner_index in 0..snapshot.definition_count() {
            let owner = checked_owner_id(owner_index, "semantic resource projection")?;
            let code = snapshot.definition_code.definition(owner).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic resource projection has no definition code for owner {}",
                    owner.0,
                ))
            })?;
            for ordinal in 0..code.resource_projection_requirement_count() {
                let requirement = code
                    .resource_projection_requirements()
                    .get(ordinal)
                    .copied()
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel semantic resource projection {}:{} disappeared",
                            owner.0, ordinal,
                        ))
                    })?;
                for symbol in code
                    .resource_projection_path_symbols(ordinal)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel semantic resource projection {}:{} has an invalid path",
                            owner.0, ordinal,
                        ))
                    })?
                {
                    snapshot.definition_code.symbol(*symbol).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel semantic resource projection {}:{} has a foreign symbol",
                            owner.0, ordinal,
                        ))
                    })?;
                }
                let origin_count =
                    code.resource_projection_origin_count(ordinal)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel semantic resource projection {}:{} has no origin span",
                                owner.0, ordinal,
                            ))
                        })?;
                if origin_count > 0
                    && !code
                        .resource_projection_required_is_published(ordinal)
                        .unwrap_or(false)
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel semantic resource projection {}:{} does not publish its required type",
                        owner.0, ordinal,
                    )));
                }
                for origin_ordinal in 0..origin_count {
                    let (source_owner, source, payload_projection) = code
                        .resource_projection_origin(ordinal, origin_ordinal)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel semantic resource projection {}:{} has an invalid origin {}",
                                owner.0, ordinal, origin_ordinal,
                            ))
                        })?;
                    self.source(source_owner, source.0)?;
                    for symbol in payload_projection {
                        snapshot.definition_code.symbol(*symbol).ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel semantic resource origin {}:{}:{} has a foreign symbol",
                                owner.0, ordinal, origin_ordinal,
                            ))
                        })?;
                    }
                }
                requirements.push(KernelSemanticResourceProjectionLocatorV1 {
                    owner,
                    ordinal: u32::try_from(ordinal).map_err(|_| {
                        KernelCheckedLinkError::new(format!(
                            "kernel semantic resource projection count for owner {} exceeds u32",
                            owner.0,
                        ))
                    })?,
                    expression: self
                        .expression(owner, KernelValueReference::Local(requirement.expression()))?,
                    target: self.declaration(owner, requirement.target())?,
                });
            }
        }
        requirements.sort_unstable_by_key(|requirement| requirement.expression.0);
        if requirements
            .windows(2)
            .any(|pair| pair[0].expression == pair[1].expression)
        {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic resource projections repeat an expression",
            ));
        }
        Ok(requirements.into_boxed_slice())
    }

    /// Explicit rich projection retained for checked-model differential tests
    /// and editor/export requests. Ordinary verified compilation must consume
    /// `KernelSemanticInputV1` instead.
    #[doc(hidden)]
    pub fn materialize_rich_resource_projection_requirements(
        &self,
        snapshot: &KernelCheckedSnapshot,
        expressions: &[CheckedExpression],
    ) -> Result<Box<[CheckedResourceProjectionRequirement]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "resource-projection handoff")?;
        let mut requirements = Vec::new();
        for owner_index in 0..snapshot.definition_count() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelCheckedLinkError::new(
                    "kernel resource-projection definition count exceeds u32",
                )
            })?);
            let code = snapshot.definition_code.definition(owner).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel resource-projection handoff omits definition {owner_index}"
                ))
            })?;
            let type_variables = self.definition(owner)?.type_variables;
            for ordinal in 0..code.resource_projection_requirement_count() {
                let requirement = code
                    .materialize_resource_projection_requirement(ordinal)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {owner_index} omits resource projection {ordinal}"
                        ))
                    })?;
                let source_origins = requirement
                    .origins
                    .into_vec()
                    .into_iter()
                    .map(|origin| {
                        Ok(CheckedSourceRead {
                            source: self.source(origin.owner, origin.source.0)?,
                            payload_projection: origin.payload_projection,
                        })
                    })
                    .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
                let expression =
                    self.expression(owner, KernelValueReference::Local(requirement.expression))?;
                let required_type = match requirement.required_type {
                    Some(required_type) => relocate_type(type_variables, &required_type)?,
                    None => expressions
                        .get(expression.0 as usize)
                        .filter(|row| row.id == expression)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel resource projection references missing checked expression {}",
                                expression.0
                            ))
                        })?
                        .flow_type
                        .ty
                        .clone(),
                };
                requirements.push(CheckedResourceProjectionRequirement {
                    expression,
                    target: self.declaration(owner, requirement.target)?,
                    projection: requirement.projection.into_vec(),
                    source_origins,
                    required_type,
                });
            }
        }
        Ok(requirements.into_boxed_slice())
    }

    /// Publish one dependency-first execution template directly from the
    /// definition artifacts and this layout's final checked relocations.
    ///
    /// This is intentionally part of the same linker pass that creates the
    /// final checked authority rows. Compact expression/shape artifacts supply
    /// the graph, while already-linked declarations, statements, and calls
    /// supply the few execution identities whose semantics live at those row
    /// boundaries. No downstream pass reconstructs a whole-program graph from
    /// the completed rich checked image.
    #[cfg(test)]
    fn build_semantic_definition_execution_store_rich_oracle(
        &self,
        snapshot: &KernelCheckedSnapshot,
        linked_scopes: &[CheckedScope],
        linked_declarations: &[CheckedDeclaration],
        linked_statements: &[CheckedStatement],
        linked_calls: &[CheckedCall],
    ) -> Result<Box<[CheckedDefinitionExecutionTemplateV1]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "definition execution template")?;

        let mut rich_templates = Vec::new();

        let mut call_by_expression = BTreeMap::new();
        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "definition execution template call")?;
            for (ordinal, call) in definition.calls.iter().enumerate() {
                let ordinal = u32::try_from(ordinal).map_err(|_| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} call count exceeds u32",
                        owner.0
                    ))
                })?;
                if call_by_expression
                    .insert((owner, call.expression), ordinal)
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats a call for expression {}",
                        owner.0, call.expression.0
                    )));
                }
            }
        }
        let linked_call_by_id = linked_calls
            .iter()
            .map(|call| (call.id, call))
            .collect::<BTreeMap<_, _>>();
        if linked_call_by_id.len() != linked_calls.len() {
            return Err(KernelCheckedLinkError::new(
                "kernel definition templates received duplicate linked call IDs",
            ));
        }
        let read_provider_callables = definition_template_read_provider_callables(
            snapshot,
            self,
            linked_scopes,
            linked_declarations,
        )?;
        let mut call_dependencies = BTreeMap::<
            (KernelOwnerId, crate::KernelExpressionId),
            Vec<(KernelOwnerId, crate::KernelExpressionId)>,
        >::new();
        for (key @ (owner, _), ordinal) in &call_by_expression {
            let call_id = self.call(*owner, *ordinal)?;
            let call = linked_call_by_id.get(&call_id).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition template references missing linked call {}",
                    call_id.0,
                ))
            })?;
            let consumed = call
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    CheckedCallEntry::Input { value, .. } => Some(*value),
                    CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => None,
                })
                .chain(call.context_binding.explicit().map(|(value, _)| value))
                .collect::<BTreeSet<_>>();
            let artifact = snapshot.definitions[owner.0 as usize]
                .calls
                .get(*ordinal as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} template references missing call ordinal {}",
                        owner.0, ordinal,
                    ))
                })?;
            let mut dependencies = Vec::new();
            let mut matched = BTreeSet::new();
            for input in &artifact.inputs {
                let dependency = definition_template_value(snapshot, *owner, input.value)?;
                let linked =
                    self.expression(dependency.0, KernelValueReference::Local(dependency.1))?;
                if consumed.contains(&linked) {
                    matched.insert(linked);
                    dependencies.push(dependency);
                }
            }
            if matched != consumed {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} template call {} cannot match consumed values {:?} to compact inputs {:?}",
                    owner.0, call_id.0, consumed, matched,
                )));
            }
            call_dependencies.insert(*key, dependencies);
        }
        let statement_child_dependencies =
            definition_template_statement_child_dependencies(snapshot, self, linked_statements)?;

        for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
            let owner = checked_owner_id(owner_index, "definition execution template")?;
            let Some(root_statement) = definition.linkage.root_statement else {
                continue;
            };
            let Some(statement) = definition
                .statements
                .get(root_statement.0 as usize)
                .filter(|statement| statement.id == root_statement)
            else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} template references missing root statement {}",
                    owner.0, root_statement.0
                )));
            };
            if !matches!(statement.kind, crate::KernelStatementKind::Function { .. }) {
                continue;
            }
            let result = definition.linkage.result_expression.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} has no result expression",
                    owner.0
                ))
            })?;
            let root = (owner, result);
            let callable = self.definition(owner)?.public_declaration;
            let mut state = BTreeMap::<(KernelOwnerId, crate::KernelExpressionId), u8>::new();
            let mut dependencies = BTreeMap::<
                (KernelOwnerId, crate::KernelExpressionId),
                Vec<(KernelOwnerId, crate::KernelExpressionId)>,
            >::new();
            let mut nodes = Vec::new();
            let mut calls = Vec::new();
            let mut pending = vec![(root, false)];
            while let Some((key @ (node_owner, expression_id), exiting)) = pending.pop() {
                let node_definition =
                    snapshot
                        .definitions
                        .get(node_owner.0 as usize)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition template references missing owner {}",
                                node_owner.0
                            ))
                        })?;
                let expression = node_definition
                    .expressions
                    .get(expression_id.0 as usize)
                    .filter(|expression| expression.id == expression_id)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition template references missing expression {}:{}",
                            node_owner.0, expression_id.0
                        ))
                    })?;
                if exiting {
                    if state.get(&key).copied() == Some(2) {
                        continue;
                    }
                    state.insert(key, 2);
                    let local_dependencies = dependencies.remove(&key).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition template lost dependencies for expression {}:{}",
                            node_owner.0, expression_id.0
                        ))
                    })?;
                    let checked_dependencies = local_dependencies
                        .iter()
                        .map(|(dependency_owner, dependency)| {
                            self.expression(
                                *dependency_owner,
                                KernelValueReference::Local(*dependency),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let call = call_by_expression
                        .get(&key)
                        .copied()
                        .map(|ordinal| self.call(node_owner, ordinal))
                        .transpose()?;
                    if let Some(call) = call {
                        calls.push(call);
                    }
                    let selector =
                        definition_template_selector(snapshot, node_owner, expression, self)?;
                    nodes.push(CheckedDefinitionExecutionNodeV1 {
                        expression: self
                            .expression(node_owner, KernelValueReference::Local(expression_id))?,
                        dependencies: checked_dependencies,
                        call,
                        selector,
                    });
                    continue;
                }
                match state.get(&key).copied().unwrap_or(0) {
                    2 | 1 => continue,
                    _ => {
                        state.insert(key, 1);
                    }
                }
                let mut relocated_dependencies = definition_template_dependencies(
                    snapshot,
                    &statement_child_dependencies,
                    &call_dependencies,
                    &read_provider_callables,
                    callable,
                    node_owner,
                    expression,
                )?
                .into_iter()
                .map(|dependency @ (dependency_owner, expression)| {
                    self.expression(dependency_owner, KernelValueReference::Local(expression))
                        .map(|relocated| (relocated, dependency))
                })
                .collect::<Result<Vec<_>, _>>()?;
                relocated_dependencies.sort_unstable_by_key(|(relocated, _)| relocated.0);
                relocated_dependencies.dedup_by_key(|(relocated, _)| *relocated);
                let node_dependencies = relocated_dependencies
                    .into_iter()
                    .map(|(_, dependency)| dependency)
                    .collect::<Vec<_>>();
                dependencies.insert(key, node_dependencies.clone());
                pending.push((key, true));
                pending.extend(
                    node_dependencies
                        .into_iter()
                        .rev()
                        .map(|dependency| (dependency, false)),
                );
            }
            calls.sort_unstable_by_key(|call| call.0);
            calls.dedup();
            rich_templates.push(CheckedDefinitionExecutionTemplateV1 {
                schema: CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1.to_owned(),
                callable,
                result: self.expression(owner, KernelValueReference::Local(result))?,
                nodes,
                calls,
                sources: definition
                    .sources
                    .iter()
                    .map(|source| self.source(owner, source.id.0))
                    .collect::<Result<Vec<_>, _>>()?,
                states: definition
                    .states
                    .iter()
                    .map(|state| self.state(owner, state.id.0))
                    .collect::<Result<Vec<_>, _>>()?,
                lists: definition
                    .lists
                    .iter()
                    .map(|list| self.list(owner, list.id.0))
                    .collect::<Result<Vec<_>, _>>()?,
            });
        }
        rich_templates.sort_unstable_by_key(|template| template.callable.0);
        if let Some(repeated) = rich_templates
            .windows(2)
            .find(|templates| templates[0].callable == templates[1].callable)
        {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition templates repeat callable {}",
                repeated[0].callable.0,
            )));
        }
        let mut expressions = BTreeSet::new();
        for template in &rich_templates {
            for node in &template.nodes {
                if !expressions.insert(node.expression) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition templates repeat expression {}",
                        node.expression.0,
                    )));
                }
            }
        }
        Ok(rich_templates.into_boxed_slice())
    }

    /// Link pattern-binding declaration authority directly from the exact
    /// match-arm execution shape retained by each definition artifact.
    ///
    /// The selector is intentionally not rediscovered from a surrounding WHEN
    /// expression: static arm pruning can remove that structural edge while
    /// the binding still owns its authored selector occurrence.
    pub fn materialize_pattern_bindings(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedPatternBinding]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "pattern binding")?;
        let mut bindings = Vec::new();
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for shape in &definition.facts().execution_shapes {
                let crate::KernelExecutionShapeInput::MatchArm {
                    expression,
                    selector,
                    bindings: arm_bindings,
                } = shape
                else {
                    continue;
                };
                let arm = definition
                    .input()
                    .nodes
                    .get(expression.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} match-arm shape references missing expression {}",
                            owner.0, expression.0,
                        ))
                    })?;
                let crate::KernelOwnerNodeKind::MatchArm { pattern } = &arm.kind else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression {} has a match-arm shape but kind {:?}",
                        owner.0, expression.0, arm.kind,
                    )));
                };
                for (ordinal, binding) in arm_bindings.iter().enumerate() {
                    let declaration = definition
                        .facts()
                        .declarations
                        .get(binding.0 as usize)
                        .filter(|declaration| declaration.id == *binding)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} match arm {} references missing binding declaration {}",
                                owner.0, expression.0, binding.0,
                            ))
                        })?;
                    if !matches!(
                        declaration.origin,
                        crate::KernelDeclarationOrigin::PatternBinding {
                            arm: declaration_arm,
                            ordinal: declaration_ordinal,
                        } if declaration_arm == *expression
                            && declaration_ordinal as usize == ordinal
                    ) || declaration.kind != crate::KernelDeclarationKind::PatternBinding
                    {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} match arm {} binding {} has inconsistent declaration authority",
                            owner.0, expression.0, binding.0,
                        )));
                    }
                    let projection = match pattern {
                        crate::KernelPattern::Tag { fields, .. }
                            if fields
                                .iter()
                                .any(|field| field.as_ref() == declaration.name.as_ref()) =>
                        {
                            vec![declaration.name.to_string()]
                        }
                        _ => Vec::new(),
                    };
                    bindings.push(CheckedPatternBinding {
                        declaration: self
                            .declaration(owner, KernelDeclarationReference::Local(*binding))?,
                        selector: self.expression(
                            owner,
                            definition
                                .resolve_value(*selector, expression.0 as usize)
                                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?,
                        )?,
                        projection,
                    });
                }
            }
        }
        bindings.sort_unstable_by_key(|binding| binding.declaration);
        Ok(bindings.into_boxed_slice())
    }

    /// Derive each call's stable storage path into one flat interned-symbol
    /// column. The traversal reuses one projection buffer and one visiting
    /// bitmap for every call; no path owns a `String` or nested vector.
    fn pack_call_result_paths(
        &self,
        snapshot: &KernelCheckedSnapshot,
        declarations: &[CheckedDeclaration],
        callables: &[CheckedCallableSignature],
        expressions: &[CheckedExpression],
        calls: &[CheckedCall],
    ) -> Result<
        (
            Box<[KernelSemanticCallResultPathLocatorV1]>,
            Box<[SymbolId]>,
        ),
        KernelCheckedLinkError,
    > {
        let declaration_slots = declarations
            .iter()
            .map(|declaration| declaration.id.0 as usize)
            .chain(callables.iter().map(|callable| callable.decl_id.0 as usize))
            .max()
            .map_or(1, |last| last.saturating_add(1));
        let mut roots = vec![None; declaration_slots];
        for declaration in declarations {
            let slot = roots.get_mut(declaration.id.0 as usize).ok_or_else(|| {
                KernelCheckedLinkError::new("declaration result-path root exceeds dense table")
            })?;
            if let Some(value) = declaration.value
                && slot.replace(value).is_some()
            {
                return Err(KernelCheckedLinkError::new(
                    "declaration result-path root is published twice",
                ));
            }
        }
        for callable in callables {
            let Some(result) = callable.result_expression else {
                continue;
            };
            let slot = roots.get_mut(callable.decl_id.0 as usize).ok_or_else(|| {
                KernelCheckedLinkError::new("callable result-path root exceeds dense table")
            })?;
            if slot.is_none() {
                *slot = Some(result);
            }
        }
        for (ordinal, call) in calls.iter().enumerate() {
            if call.id.0 as usize != ordinal {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked call-result topology expected dense call {ordinal} but found {}",
                    call.id.0,
                )));
            }
        }
        let text = snapshot
            .definition_code
            .type_store()
            .as_arena()
            .text_snapshot();
        let mut visiting = vec![false; expressions.len()];
        let mut projection = Vec::new();
        let mut symbols = Vec::new();
        let mut result = Vec::new();
        for call in calls {
            let expression = expressions
                .get(call.expression.0 as usize)
                .filter(|expression| expression.id == call.expression)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked call {} references missing expression {}",
                        call.id.0, call.expression.0,
                    ))
                })?;
            let Some(anchor) = expression.declaration else {
                continue;
            };
            let Some(root) = roots.get(anchor.0 as usize).copied().flatten() else {
                continue;
            };
            projection.clear();
            if checked_projection_symbols_to_expression_with_scratch(
                text,
                expressions,
                calls,
                root,
                call.expression,
                &mut visiting,
                &mut projection,
            )? {
                let projection_start = u32::try_from(symbols.len()).map_err(|_| {
                    KernelCheckedLinkError::new(
                        "kernel semantic call-result path symbol start exceeds u32",
                    )
                })?;
                let projection_len = u32::try_from(projection.len()).map_err(|_| {
                    KernelCheckedLinkError::new(
                        "kernel semantic call-result path length exceeds u32",
                    )
                })?;
                symbols.extend_from_slice(&projection);
                result.push(KernelSemanticCallResultPathLocatorV1 {
                    call: call.id,
                    anchor,
                    projection_start,
                    projection_len,
                });
            }
        }
        Ok((result.into_boxed_slice(), symbols.into_boxed_slice()))
    }

    /// Project compact call-result paths only for editor/export and the rich
    /// differential oracle.
    fn materialize_call_result_paths(
        &self,
        snapshot: &KernelCheckedSnapshot,
        paths: &[KernelSemanticCallResultPathLocatorV1],
        symbols: &[SymbolId],
    ) -> Result<Box<[CheckedCallResultPath]>, KernelCheckedLinkError> {
        let text = snapshot
            .definition_code
            .type_store()
            .as_arena()
            .text_snapshot();
        paths
            .iter()
            .map(|path| {
                let start = path.projection_start as usize;
                let end = start
                    .checked_add(path.projection_len as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "kernel editor call-result path range overflows",
                        )
                    })?;
                let projection = symbols
                    .get(start..end)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "kernel editor call-result path range is invalid",
                        )
                    })?
                    .iter()
                    .map(|symbol| {
                        text.symbol(*symbol).map(str::to_owned).ok_or_else(|| {
                            KernelCheckedLinkError::new(
                                "kernel editor call-result path has a foreign symbol",
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(CheckedCallResultPath {
                    call: path.call,
                    path: CheckedSemanticPath {
                        anchor: path.anchor,
                        projection,
                    },
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Vec::into_boxed_slice)
    }

    /// Emit only the dense declaration target of every semantic occurrence.
    /// Checked-image routing depends on this identity and never consumes the
    /// rich occurrence kind or source span.
    fn occurrence_targets(
        &self,
        snapshot: &KernelCheckedSnapshot,
        expressions: &[CheckedExpression],
        calls: &[CheckedCall],
    ) -> Result<Box<[DeclId]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "occurrence topology")?;
        let mut targets = Vec::new();
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            let linked = self.definition(owner)?;

            for declaration in &definition.facts().declarations {
                if matches!(
                    declaration.origin,
                    crate::KernelDeclarationOrigin::RecordField { .. }
                        | crate::KernelDeclarationOrigin::CallbackBinding { .. }
                        | crate::KernelDeclarationOrigin::CallContext { .. }
                ) {
                    continue;
                }
                targets.push(
                    self.declaration(owner, KernelDeclarationReference::Local(declaration.id))?,
                );
            }

            for row in checked_range(linked.calls)? {
                let call = calls
                    .get(row)
                    .filter(|call| call.id.0 as usize == row)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} occurrence topology references missing call row {row}",
                            owner.0,
                        ))
                    })?;
                for entry in &call.entries {
                    match entry {
                        CheckedCallEntry::FreshOut { output, .. } => targets.push(*output),
                        CheckedCallEntry::ForwardOut { target, .. } => targets.push(*target),
                        CheckedCallEntry::Input { .. } => {}
                    }
                }
                targets.extend(call.contexts.iter().map(|context| context.declaration));
                targets.push(call.callable);
                if matches!(call.context_binding, CheckedContextBinding::Explicit { .. }) {
                    targets.push(call.callable);
                }
            }

            for row in checked_range(linked.expressions)? {
                let expression = expressions
                    .get(row)
                    .filter(|expression| expression.id.0 as usize == row)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} occurrence topology references missing expression row {row}",
                            owner.0,
                        ))
                    })?;
                match expression.kind {
                    CheckedExpressionKind::Read { target, .. }
                    | CheckedExpressionKind::Drain { target, .. } => targets.push(target),
                    _ => {}
                }
            }
        }
        Ok(targets.into_boxed_slice())
    }

    /// Emit the complete semantic occurrence inventory while source-local
    /// declaration/call/expression spans are still available. Rows remain
    /// grouped by definition so project coordinate rebasing is one bounded
    /// range update per source owner.
    pub fn materialize_occurrences(
        &self,
        snapshot: &KernelCheckedSnapshot,
        declarations: &[CheckedDeclaration],
        expressions: &[CheckedExpression],
        calls: &[CheckedCall],
    ) -> Result<(Box<[SemanticOccurrence]>, Box<[KernelCheckedRowRange]>), KernelCheckedLinkError>
    {
        self.validate_snapshot_definition_count(snapshot, "occurrence")?;
        let declaration_by_id = declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let mut occurrences = Vec::new();
        let mut ranges = Vec::with_capacity(snapshot.definition_count());
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            let linked = self.definition(owner)?;
            let range_start = u32::try_from(occurrences.len()).map_err(|_| {
                KernelCheckedLinkError::new("kernel checked occurrence count exceeds u32")
            })?;
            let mut declared = BTreeSet::new();

            // Authored declarations precede call-generated occurrences. Inline
            // record fields are lexical projection anchors, not authored-name
            // occurrences in the public checked index. Fresh OUT and
            // call-context declarations are emitted at their exact call
            // position below, matching their source authority.
            for declaration in &definition.facts().declarations {
                if matches!(
                    declaration.origin,
                    crate::KernelDeclarationOrigin::RecordField { .. }
                        | crate::KernelDeclarationOrigin::CallbackBinding { .. }
                        | crate::KernelDeclarationOrigin::CallContext { .. }
                ) {
                    continue;
                }
                let target =
                    self.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
                let row = declaration_by_id.get(&target).copied().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} occurrence references missing declaration {}",
                        owner.0, target.0,
                    ))
                })?;
                if !declared.insert(target) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats declaration occurrence {}",
                        owner.0, target.0,
                    )));
                }
                occurrences.push(SemanticOccurrence {
                    target,
                    kind: SemanticOccurrenceKind::Declaration,
                    span: row.span,
                });
            }

            let mut syntax_by_expression = BTreeMap::new();
            for syntax in &definition.facts().call_syntax {
                if syntax_by_expression
                    .insert(syntax.expression, syntax)
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats authored call expression {}",
                        owner.0, syntax.expression.0,
                    )));
                }
            }
            for ordinal in 0..definition.call_count() {
                let artifact_call = definition.call(ordinal).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} omits packed call {ordinal}",
                        owner.0,
                    ))
                })?;
                let row = linked.calls.start as usize + ordinal;
                let call = calls
                    .get(row)
                    .filter(|call| call.id.0 as usize == row)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} occurrence references missing call row {row}",
                            owner.0,
                        ))
                    })?;
                let syntax = syntax_by_expression
                    .get(&artifact_call.expression())
                    .copied()
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} call expression {} has no authored occurrence surface",
                            owner.0, artifact_call.expression().0,
                        ))
                    })?;
                for entry in &call.entries {
                    match entry {
                        CheckedCallEntry::FreshOut { output, .. } => {
                            let declaration = declaration_by_id.get(output).copied().ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition {} FreshOut occurrence references missing declaration {}",
                                    owner.0, output.0,
                                ))
                            })?;
                            if !declared.insert(*output) {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} repeats FreshOut occurrence {}",
                                    owner.0, output.0,
                                )));
                            }
                            occurrences.push(SemanticOccurrence {
                                target: *output,
                                kind: SemanticOccurrenceKind::FreshOut,
                                span: declaration.span,
                            });
                        }
                        CheckedCallEntry::ForwardOut { name, target, .. } => {
                            let mut arguments = syntax.arguments.iter().filter(|argument| {
                                argument.kind == KernelCallArgumentKind::Named
                                    && argument.name.as_ref() == name
                            });
                            let argument = arguments.next().ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition {} ForwardOut `{name}` has no authored argument occurrence",
                                    owner.0,
                                ))
                            })?;
                            if arguments.next().is_some() {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} ForwardOut `{name}` has multiple authored argument occurrences",
                                    owner.0,
                                )));
                            }
                            occurrences.push(SemanticOccurrence {
                                target: *target,
                                kind: SemanticOccurrenceKind::ForwardOut,
                                span: checked_span(argument.span),
                            });
                        }
                        CheckedCallEntry::Input { .. } => {}
                    }
                }
                for context in &call.contexts {
                    let declaration = declaration_by_id
                        .get(&context.declaration)
                        .copied()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} call-context occurrence references missing declaration {}",
                                owner.0, context.declaration.0,
                            ))
                        })?;
                    if !declared.insert(context.declaration) {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} repeats call-context declaration occurrence {}",
                            owner.0, context.declaration.0,
                        )));
                    }
                    occurrences.push(SemanticOccurrence {
                        target: context.declaration,
                        kind: SemanticOccurrenceKind::Declaration,
                        span: declaration.span,
                    });
                }
                occurrences.push(SemanticOccurrence {
                    target: call.callable,
                    kind: SemanticOccurrenceKind::Call,
                    span: call.span,
                });
                if let CheckedContextBinding::Explicit { span, .. } = call.context_binding {
                    occurrences.push(SemanticOccurrence {
                        target: call.callable,
                        kind: SemanticOccurrenceKind::Pass,
                        span,
                    });
                }
            }
            let expected_declaration_occurrences = definition
                .facts()
                .declarations
                .iter()
                .filter(|declaration| {
                    !matches!(
                        declaration.origin,
                        crate::KernelDeclarationOrigin::RecordField { .. }
                    )
                })
                .count();
            if declared.len() != expected_declaration_occurrences {
                let missing = definition
                    .facts()
                    .declarations
                    .iter()
                    .filter_map(|declaration| {
                        if matches!(
                            declaration.origin,
                            crate::KernelDeclarationOrigin::RecordField { .. }
                        ) {
                            return None;
                        }
                        let target = self
                            .declaration(owner, KernelDeclarationReference::Local(declaration.id))
                            .ok()?;
                        (!declared.contains(&target)).then_some(target.0)
                    })
                    .collect::<Vec<_>>();
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has declarations without exact occurrences: {missing:?}",
                    owner.0,
                )));
            }

            for row in checked_range(linked.expressions)? {
                let expression = expressions.get(row).filter(|expression| {
                    expression.id == CheckedExprId(u32::try_from(row).unwrap_or(u32::MAX))
                });
                let Some(expression) = expression else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} occurrence references missing expression row {row}",
                        owner.0,
                    )));
                };
                let target = match expression.kind {
                    CheckedExpressionKind::Read { target, .. }
                    | CheckedExpressionKind::Drain { target, .. } => target,
                    _ => continue,
                };
                occurrences.push(SemanticOccurrence {
                    target,
                    kind: SemanticOccurrenceKind::Read,
                    span: expression.span,
                });
            }
            let range_end = u32::try_from(occurrences.len()).map_err(|_| {
                KernelCheckedLinkError::new("kernel checked occurrence count exceeds u32")
            })?;
            ranges.push(KernelCheckedRowRange {
                start: range_start,
                len: range_end - range_start,
            });
        }
        Ok((occurrences.into_boxed_slice(), ranges.into_boxed_slice()))
    }

    pub fn definitions(&self) -> &[KernelCheckedDefinitionLayout] {
        &self.definitions
    }

    pub fn abi_callables(&self) -> &[KernelCheckedAbiCallableLayout] {
        &self.abi_callables
    }

    pub fn abi_callable(
        &self,
        name: &str,
    ) -> Result<&KernelCheckedAbiCallableLayout, KernelCheckedLinkError> {
        self.abi_callable_by_name
            .get(name)
            .and_then(|index| self.abi_callables.get(*index))
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked linker references unallocated ABI callable `{name}`"
                ))
            })
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

    fn lexical_declaration_for_scope(
        &self,
        snapshot: &KernelCheckedSnapshot,
        owner: KernelOwnerId,
        scope: KernelScopeReference,
    ) -> Result<Option<DeclId>, KernelCheckedLinkError> {
        let mut owner = owner;
        let mut scope = scope;
        let mut remaining = snapshot
            .definition_facts
            .iter()
            .map(|facts| facts.presentation.scopes.len())
            .sum::<usize>()
            .saturating_add(snapshot.definition_count())
            .saturating_add(1);
        loop {
            if remaining == 0 {
                return Err(KernelCheckedLinkError::new(
                    "kernel checked presentation lexical scopes contain a cycle",
                ));
            }
            remaining -= 1;
            match scope {
                KernelScopeReference::ProjectRoot => return Ok(None),
                KernelScopeReference::Containing => {
                    let facts = snapshot.definition_facts.get(owner.0 as usize).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel lexical declaration lookup references missing definition {}",
                            owner.0,
                        ))
                    })?;
                    scope = facts.presentation.containing_scope;
                }
                KernelScopeReference::Owner {
                    owner: provider,
                    scope: provider_scope,
                } => {
                    owner = provider;
                    scope = KernelScopeReference::Local(provider_scope);
                }
                KernelScopeReference::Local(local) => {
                    let facts = snapshot.definition_facts.get(owner.0 as usize).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel lexical declaration lookup references missing definition {}",
                            owner.0,
                        ))
                    })?;
                    let row = facts
                        .presentation
                        .scopes
                        .get(local.0 as usize)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} lexical declaration lookup references missing scope {}",
                                owner.0, local.0,
                            ))
                        })?;
                    if let Some(declaration) = row.owner {
                        return self.declaration(owner, declaration).map(Some);
                    }
                    scope = row.parent;
                }
            }
        }
    }

    /// Consume compact definition presentation into the final checked lexical
    /// scope namespace without reopening parser arenas or owner-shard rows.
    pub fn materialize_scopes(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedScope]>, KernelCheckedLinkError> {
        if snapshot.definition_count() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked scope materializer has {} definitions for a {}-definition layout",
                snapshot.definition_count(),
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
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            let layout = self.definition(owner)?;
            for scope in &facts.presentation.scopes {
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
    /// declaration namespace. Stable ABI declarations occupy the following
    /// layout-owned range and are emitted by [`Self::materialize_abi_callables`].
    pub fn materialize_declarations(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedDeclaration]>, KernelCheckedLinkError> {
        if snapshot.definition_count() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked declaration materializer has {} definitions for a {}-definition layout",
                snapshot.definition_count(),
                self.definitions.len(),
            )));
        }
        let mut declarations = Vec::with_capacity(
            self.definition_declarations_end
                .checked_sub(1)
                .expect("the checked declaration namespace reserves row zero") as usize,
        );
        let mut type_cache = snapshot.definition_code.materialization_cache();
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            if facts.declarations.len() != facts.presentation.declarations.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has {} declaration artifacts but {} declaration presentations",
                    owner.0,
                    facts.declarations.len(),
                    facts.presentation.declarations.len(),
                )));
            }
            for (declaration, presentation) in definition
                .facts()
                .declarations
                .iter()
                .zip(facts.presentation.declarations.iter())
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
                    flow_type: declaration_flow_type(
                        self,
                        snapshot,
                        owner,
                        declaration,
                        &mut type_cache,
                    )?,
                    value: declaration
                        .value
                        .map(|value| {
                            definition
                                .resolve_value(value, declaration.id.0 as usize)
                                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))
                                .and_then(|value| self.expression(owner, value))
                        })
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
        if declarations.len() + 1 != self.definition_declarations_end as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked declaration materializer produced {} rows for a namespace ending at {}",
                declarations.len(),
                self.definition_declarations_end,
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
        if snapshot.definition_count() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked statement materializer has {} definitions for a {}-definition layout",
                snapshot.definition_count(),
                self.definitions.len(),
            )));
        }

        let mut resources =
            vec![Vec::<CheckedResourceBinding>::new(); self.totals.statements as usize];
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for source in &definition.facts().sources {
                push_statement_resource(
                    &mut resources,
                    self.statement(owner, source.statement)?,
                    CheckedResourceBinding::Source {
                        source: self.source(owner, source.id.0)?,
                    },
                )?;
            }
        }
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for state in definition.states() {
                push_statement_resource(
                    &mut resources,
                    self.statement(owner, state.input().statement)?,
                    CheckedResourceBinding::State {
                        state: self.state(owner, state.id().0)?,
                    },
                )?;
            }
        }
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for list in &definition.facts().lists {
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
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            if facts.statements.len() != facts.presentation.statements.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has {} statement artifacts but {} statement presentations",
                    owner.0,
                    facts.statements.len(),
                    facts.presentation.statements.len(),
                )));
            }
            for (statement, presentation) in definition
                .facts()
                .statements
                .iter()
                .zip(facts.presentation.statements.iter())
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
                        .map(|value| {
                            definition
                                .resolve_value(value, statement.id.0 as usize)
                                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))
                                .and_then(|value| self.expression(owner, value))
                        })
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

    /// Emit every definition expression directly from immutable kernel facts.
    ///
    /// This is the central compatibility-assembler deletion seam: expression
    /// kind, structural children, lexical authority, type, effect, scope, and
    /// source coordinates are all consumed from compact rows produced during
    /// graph construction/solve. No parser or legacy owner DTO is reopened.
    pub fn materialize_expressions(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedExpression]>, KernelCheckedLinkError> {
        if snapshot.definition_count() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked expression materializer has {} definitions for a {}-definition layout",
                snapshot.definition_count(),
                self.definitions.len(),
            )));
        }

        let mut source_paths = BTreeMap::<DeclId, Vec<(Vec<String>, CheckedSourceId)>>::new();
        let mut declaration_metadata =
            BTreeMap::<DeclId, (KernelOwnerId, KernelScopeReference, &str)>::new();
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            for (declaration, presentation) in definition
                .facts()
                .declarations
                .iter()
                .zip(facts.presentation.declarations.iter())
            {
                let linked =
                    self.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
                if declaration_metadata
                    .insert(
                        linked,
                        (owner, presentation.scope, declaration.name.as_ref()),
                    )
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked expression materializer repeats declaration metadata for {}",
                        linked.0,
                    )));
                }
            }
        }
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for source in &definition.facts().sources {
                let source_id = self.source(owner, source.id.0)?;
                let source_declaration = self.declaration(owner, source.declaration)?;
                let source_projection = source
                    .projection
                    .iter()
                    .map(|field| field.to_string())
                    .collect::<Vec<_>>();
                let exact_anchor = self.declaration(owner, source.declaration)?;
                source_paths
                    .entry(exact_anchor)
                    .or_default()
                    .push((source_projection.clone(), source_id));

                // Checked lexical reads name a nested SOURCE by its exact
                // declaration, even when the authored path starts at an
                // enclosing record declaration. Derive every ancestor alias
                // once from compact declaration/scope presentation so reads
                // can be canonicalized without reopening parser or owner DTOs.
                let Some((_, mut scope, source_name)) =
                    declaration_metadata.get(&source_declaration).copied()
                else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel SOURCE declaration {} has no presentation metadata",
                        source_declaration.0,
                    )));
                };
                let mut scope_owner = owner;
                let mut alias_projection =
                    Vec::with_capacity(source_projection.len().saturating_add(4));
                alias_projection.push(source_name.to_string());
                alias_projection.extend(source_projection);
                let mut seen_anchors = BTreeSet::new();
                while let Some(anchor) =
                    self.lexical_declaration_for_scope(snapshot, scope_owner, scope)?
                {
                    if !seen_anchors.insert(anchor) || anchor == source_declaration {
                        break;
                    }
                    source_paths
                        .entry(anchor)
                        .or_default()
                        .push((alias_projection.clone(), source_id));
                    let Some((owner, parent_scope, name)) =
                        declaration_metadata.get(&anchor).copied()
                    else {
                        break;
                    };
                    alias_projection.insert(0, name.to_string());
                    scope_owner = owner;
                    scope = parent_scope;
                }
            }
        }
        for paths in source_paths.values_mut() {
            paths.sort_by_key(|(path, source)| (std::cmp::Reverse(path.len()), *source));
        }

        let mut expressions = Vec::with_capacity(self.totals.expressions as usize);
        let mut type_cache = snapshot.definition_code.materialization_cache();
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer = code.linked_materializer(
                &mut type_cache,
                self.definition(owner)?.type_variables.start,
            );
            let local_len = definition.input().nodes.len();
            if code.expressions().len() != local_len
                || facts.presentation.expressions.len() != local_len
                || facts.expression_payloads.len() != local_len
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} packed flows, expression artifacts, presentation, and payload tables differ: {} / {} / {} / {}",
                    owner.0,
                    code.expressions().len(),
                    local_len,
                    facts.presentation.expressions.len(),
                    facts.expression_payloads.len(),
                )));
            }
            let mut shapes = vec![None; local_len];
            for shape in &facts.execution_shapes {
                let slot = shapes
                    .get_mut(shape.expression().0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} execution shape references missing expression {}",
                            owner.0,
                            shape.expression().0,
                        ))
                    })?;
                if slot.replace(shape).is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats execution shape for expression {}",
                        owner.0,
                        shape.expression().0,
                    )));
                }
            }
            let mut lexical = vec![None; local_len];
            for binding in &facts.lexical_bindings {
                let slot = lexical
                    .get_mut(binding.expression.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} lexical binding references missing expression {}",
                            owner.0, binding.expression.0,
                        ))
                    })?;
                if slot.replace(binding).is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats lexical binding for expression {}",
                        owner.0, binding.expression.0,
                    )));
                }
            }
            let mut calls = vec![None; local_len];
            for ordinal in 0..definition.call_count() {
                let call = definition.call(ordinal).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} omits packed call {ordinal}",
                        owner.0,
                    ))
                })?;
                let slot = calls.get_mut(call.expression().0 as usize).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} call references missing expression {}",
                        owner.0,
                        call.expression().0,
                    ))
                })?;
                if slot
                    .replace(u32::try_from(ordinal).map_err(|_| {
                        KernelCheckedLinkError::new("kernel call ordinal exceeds u32")
                    })?)
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats call for expression {}",
                        owner.0,
                        call.expression().0,
                    )));
                }
            }

            for (((expression, presentation), payload), local_ordinal) in definition
                .input()
                .nodes
                .iter()
                .zip(facts.presentation.expressions.iter())
                .zip(facts.expression_payloads.iter())
                .zip(0..)
            {
                let expression_id =
                    crate::KernelExpressionId(u32::try_from(local_ordinal).map_err(|_| {
                        KernelCheckedLinkError::new(
                            "kernel checked expression materializer local count exceeds u32",
                        )
                    })?);
                if expression_id != presentation.expression {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression rows are not dense at ordinal {local_ordinal}",
                        owner.0,
                    )));
                }
                let id = self.expression(owner, KernelValueReference::Local(expression_id))?;
                if id.0 as usize != expressions.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked expression materializer expected row {} but linked {}",
                        expressions.len(),
                        id.0,
                    )));
                }
                let declaration = match presentation.declaration {
                    Some(declaration) => Some(self.declaration(owner, declaration)?),
                    None => self.lexical_declaration_for_scope(
                        snapshot,
                        owner,
                        presentation.declaration_scope.unwrap_or(presentation.scope),
                    )?,
                };
                let kind = checked_expression_kind(
                    self,
                    owner,
                    definition,
                    facts,
                    expression_id,
                    expression,
                    presentation.span.line,
                    declaration,
                    payload,
                    shapes[local_ordinal],
                    lexical[local_ordinal],
                    calls[local_ordinal],
                    &source_paths,
                )?;
                let effect = definition
                    .expression_effect(expression_id)
                    .expect("validated expression retains effect facts");
                expressions.push(CheckedExpression {
                    id,
                    scope_id: self.scope(owner, presentation.scope)?,
                    declaration,
                    flow_type: materializer
                        .materialize_published_expression(local_ordinal)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} has no packed expression flow {}",
                                owner.0, local_ordinal
                            ))
                        })?,
                    flush_type: materializer.materialize_expression_flush(local_ordinal),
                    effect: CheckedEffectSummary {
                        reads_state: effect.reads_state,
                        writes_state: effect.writes_state,
                        emits_source: effect.emits_source,
                        invokes_host: effect.invokes_host,
                    },
                    kind,
                    span: CheckedSpan {
                        line: presentation.span.line,
                        start: presentation.span.start,
                        end: presentation.span.end,
                    },
                });
            }
        }
        if expressions.len() != self.totals.expressions as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked expression materializer produced {} rows for a {}-row layout",
                expressions.len(),
                self.totals.expressions,
            )));
        }
        Ok(expressions.into_boxed_slice())
    }

    /// Emit authored user-callable signatures and their sparse inherited
    /// PASSED schemes directly from the same definition artifacts that own
    /// declaration and expression rows.
    ///
    /// Builtin/external ABI signatures are intentionally not accepted here;
    /// the compiler facade appends those stable lower-level contracts after
    /// this definition-owned table. Keeping the two authorities separate
    /// prevents parser or legacy owner metadata from leaking into the kernel.
    pub fn materialize_user_callables(
        &self,
        snapshot: &KernelCheckedSnapshot,
        role: ProgramRole,
    ) -> Result<
        (Box<[CheckedCallableSignature]>, Box<[CheckedContextFormal]>),
        KernelCheckedLinkError,
    > {
        self.validate_snapshot_definition_count(snapshot, "user callable")?;
        let mut callables = Vec::with_capacity(self.totals.user_callables as usize);
        let mut context_formals = Vec::with_capacity(self.totals.context_formals as usize);
        let mut type_cache = snapshot.definition_code.materialization_cache();
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer = code.linked_materializer(
                &mut type_cache,
                self.definition(owner)?.type_variables.start,
            );
            let root_statement = definition.linkage().root_statement.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} has no root statement while linking callables",
                    owner.0,
                ))
            })?;
            let Some(root) = facts.statements.get(root_statement.0 as usize) else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} root statement {} is missing",
                    owner.0, root_statement.0,
                )));
            };
            let crate::KernelStatementKind::Function { name, parameters } = &root.kind else {
                if definition.linkage().context_formal_ordinal.is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel non-callable definition {} owns a context formal",
                        owner.0,
                    )));
                }
                continue;
            };
            let KernelDeclarationReference::Local(public_declaration) =
                definition.linkage().public_declaration.ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} has no public declaration",
                        owner.0,
                    ))
                })?
            else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} delegates its public declaration",
                    owner.0,
                )));
            };
            let public_declaration_row = definition
                .facts()
                .declarations
                .get(public_declaration.0 as usize)
                .filter(|declaration| {
                    declaration.id == public_declaration
                        && declaration.kind == crate::KernelDeclarationKind::Function
                })
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} has no exact function declaration {}",
                        owner.0, public_declaration.0,
                    ))
                })?;
            let public_presentation = declaration_presentation(facts, public_declaration)?;
            let body_scope = public_presentation.body_scope.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} has no body scope",
                    owner.0,
                ))
            })?;
            let root_presentation = statement_presentation(facts, root_statement)?;
            if root_presentation.body_scope != Some(body_scope) {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} function declaration and statement disagree on body scope",
                    owner.0,
                )));
            }
            if code.formals().len()
                != parameters.len()
                    + usize::from(definition.linkage().context_formal_ordinal.is_some())
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} has {} parameters, {:?} context formal, and {} solved formals",
                    owner.0,
                    parameters.len(),
                    definition.linkage().context_formal_ordinal,
                    code.formals().len(),
                )));
            }
            let mut checked_parameters = Vec::with_capacity(parameters.len());
            for parameter in parameters.iter() {
                if parameter.ordinal as usize >= parameters.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} parameter ordinal {} is outside 0..{}",
                        owner.0,
                        parameter.ordinal,
                        parameters.len(),
                    )));
                }
                let mut declarations = facts.declarations.iter().filter(|declaration| {
                    matches!(
                        declaration.origin,
                        crate::KernelDeclarationOrigin::Parameter { statement, ordinal }
                            if statement == root_statement && ordinal == parameter.ordinal
                    )
                });
                let declaration = declarations.next().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} parameter {} has no declaration",
                        owner.0, parameter.ordinal,
                    ))
                })?;
                if declarations.next().is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} parameter {} has multiple declarations",
                        owner.0, parameter.ordinal,
                    )));
                }
                let expected_kind = match parameter.kind {
                    crate::KernelParameterKind::Value => {
                        crate::KernelDeclarationKind::ValueParameter
                    }
                    crate::KernelParameterKind::Out => crate::KernelDeclarationKind::OutParameter,
                };
                if declaration.kind != expected_kind || declaration.name != parameter.name {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} parameter {} declaration disagrees with its authored header",
                        owner.0, parameter.ordinal,
                    )));
                }
                let presentation = declaration_presentation(facts, declaration.id)?;
                let evaluation_scope = match parameter.evaluation_scope {
                    crate::KernelParameterEvaluationScope::Parent => CheckedEvaluationScope::Parent,
                    crate::KernelParameterEvaluationScope::Output { parameter_ordinal } => {
                        let output = definition
                            .facts()
                            .declarations
                            .iter()
                            .find(|candidate| {
                                matches!(
                                    candidate.origin,
                                    crate::KernelDeclarationOrigin::Parameter { statement, ordinal }
                                        if statement == root_statement && ordinal == parameter_ordinal
                                )
                            })
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel callable definition {} parameter {} targets missing output parameter {}",
                                    owner.0, parameter.ordinal, parameter_ordinal,
                                ))
                            })?;
                        if output.kind != crate::KernelDeclarationKind::OutParameter {
                            return Err(KernelCheckedLinkError::new(format!(
                                "kernel callable definition {} parameter {} targets non-OUT parameter {}",
                                owner.0, parameter.ordinal, parameter_ordinal,
                            )));
                        }
                        CheckedEvaluationScope::Output {
                            formal: self
                                .declaration(owner, KernelDeclarationReference::Local(output.id))?,
                        }
                    }
                };
                checked_parameters.push(CheckedParameter {
                    decl_id: self
                        .declaration(owner, KernelDeclarationReference::Local(declaration.id))?,
                    name: parameter.name.to_string(),
                    kind: match parameter.kind {
                        crate::KernelParameterKind::Value => CheckedParameterKind::Value,
                        crate::KernelParameterKind::Out => CheckedParameterKind::Out,
                    },
                    ordinal: parameter.ordinal as usize,
                    flow_type: materializer
                        .materialize_formal(parameter.ordinal as usize)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} has no packed formal {}",
                                owner.0, parameter.ordinal
                            ))
                        })?,
                    requirement: CheckedParameterRequirement::Required,
                    evaluation_scope,
                    start: presentation.span.start,
                    end: presentation.span.end,
                });
            }
            checked_parameters.sort_unstable_by_key(|parameter| parameter.ordinal);

            let context_formal = definition
                .linkage()
                .context_formal_ordinal
                .map(|ordinal| {
                    let id = self.definition(owner)?.context_formal.ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel callable definition {} has no linked context formal",
                            owner.0,
                        ))
                    })?;
                    if ordinal as usize != parameters.len() {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel callable definition {} context ordinal {} does not follow {} authored parameters",
                            owner.0,
                            ordinal,
                            parameters.len(),
                        )));
                    }
                    let flow_type = materializer
                        .materialize_formal(ordinal as usize)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} has no packed context formal {}",
                                owner.0, ordinal
                            ))
                        })?;
                    let projections = boon_checked::context_scheme_projections(&flow_type.ty);
                    context_formals.push(CheckedContextFormal {
                        id,
                        callable: self.declaration(
                            owner,
                            KernelDeclarationReference::Local(public_declaration),
                        )?,
                        scheme: CheckedContextScheme {
                            flow_type,
                            projections,
                        },
                    });
                    Ok(id)
                })
                .transpose()?;
            if public_declaration_row.name != *name {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} declaration name {:?} differs from function header {:?}",
                    owner.0, public_declaration_row.name, name,
                )));
            }
            let mut effect = CheckedEffectSummary::default();
            for expression in 0..definition.input().nodes.len() {
                let expression = crate::KernelExpressionId(
                    u32::try_from(expression)
                        .expect("kernel expression count exceeds dense u32 namespace"),
                );
                let row = definition
                    .expression_effect(expression)
                    .expect("validated expression retains effect facts");
                effect.reads_state |= row.reads_state;
                effect.writes_state |= row.writes_state;
                effect.emits_source |= row.emits_source;
                effect.invokes_host |= row.invokes_host;
            }
            callables.push(CheckedCallableSignature {
                decl_id: self
                    .declaration(owner, KernelDeclarationReference::Local(public_declaration))?,
                scope_id: self.scope(owner, KernelScopeReference::Local(body_scope))?,
                kind: CheckedCallableKind::User,
                name: name.to_string(),
                intrinsic: None,
                external_identity: None,
                parameters: checked_parameters,
                contexts: Vec::new(),
                context_formal,
                result: materializer.materialize_result(),
                role,
                effect,
                body: Some(self.statement(owner, KernelStatementReference::Local(root_statement))?),
                result_expression: Some(self.expression(
                    owner,
                    KernelValueReference::Local(
                        definition.linkage().result_expression.ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel callable definition {} has no result expression",
                                owner.0,
                            ))
                        })?,
                    ),
                )?),
                contextual_operation: None,
            });
        }
        self.validate_materialized_count(
            "user callable",
            callables.len(),
            self.totals.user_callables,
        )?;
        self.validate_materialized_count(
            "context formal",
            context_formals.len(),
            self.totals.context_formals,
        )?;
        Ok((
            callables.into_boxed_slice(),
            context_formals.into_boxed_slice(),
        ))
    }

    /// Materialize every referenced builtin/external callable from the
    /// immutable kernel ABI, including its declarations and relocated local
    /// type-variable namespace.
    ///
    /// This is deliberately demand-shaped: an unused compiler/library
    /// contract does not bloat the checked image. The call occurrence and its
    /// ABI signature nevertheless share this one layout authority.
    pub fn materialize_abi_callables(
        &self,
        abi: &KernelAbiInput,
    ) -> Result<(Box<[CheckedCallableSignature]>, Box<[CheckedDeclaration]>), KernelCheckedLinkError>
    {
        let mut callables = Vec::with_capacity(self.totals.abi_callables as usize);
        let mut declarations = Vec::new();
        for layout in &self.abi_callables {
            let callable = abi.callable(&layout.name).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked ABI materializer cannot find `{}` in its immutable ABI",
                    layout.name,
                ))
            })?;
            if callable.parameters.len() != layout.parameters.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked ABI callable `{}` has {} parameters in its layout and {} in its contract",
                    layout.name,
                    layout.parameters.len(),
                    callable.parameters.len(),
                )));
            }
            let parameters = callable
                .parameters
                .iter()
                .zip(layout.parameters.iter().copied())
                .map(|(parameter, decl_id)| {
                    let evaluation_scope = match parameter.evaluation_scope {
                        crate::KernelParameterEvaluationScope::Parent => {
                            CheckedEvaluationScope::Parent
                        }
                        crate::KernelParameterEvaluationScope::Output { parameter_ordinal } => {
                            let formal = layout
                                .parameters
                                .get(parameter_ordinal as usize)
                                .copied()
                                .ok_or_else(|| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel checked ABI callable `{}` parameter `{}` targets missing OUT ordinal {parameter_ordinal}",
                                        layout.name, parameter.name,
                                    ))
                                })?;
                            CheckedEvaluationScope::Output { formal }
                        }
                    };
                    Ok(CheckedParameter {
                        decl_id,
                        name: parameter.name.to_string(),
                        kind: parameter.kind,
                        ordinal: parameter.ordinal as usize,
                        flow_type: relocate_abi_flow_type(layout, &parameter.flow_type)?,
                        requirement: parameter.requirement.clone(),
                        evaluation_scope,
                        start: 0,
                        end: 0,
                    })
                })
                .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
            let contexts = callable
                .contexts
                .iter()
                .map(|context| {
                    let provider = layout
                        .parameters
                        .get(context.provider_parameter_ordinal as usize)
                        .copied()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel checked ABI callable `{}` context `{}` targets missing parameter ordinal {}",
                                layout.name, context.name, context.provider_parameter_ordinal,
                            ))
                        })?;
                    Ok(CheckedCallableContext {
                        name: context.name.to_string(),
                        kind: context.kind,
                        provider,
                        flow_type: relocate_abi_flow_type(layout, &context.flow_type)?,
                    })
                })
                .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
            let result = relocate_abi_flow_type(layout, &callable.result)?;
            callables.push(CheckedCallableSignature {
                decl_id: layout.declaration,
                scope_id: LexicalScopeId(0),
                kind: match callable.kind {
                    crate::KernelCallableKind::Builtin => CheckedCallableKind::Builtin,
                    crate::KernelCallableKind::External => CheckedCallableKind::External,
                    crate::KernelCallableKind::User => {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel immutable ABI unexpectedly contains user callable `{}`",
                            layout.name,
                        )));
                    }
                },
                name: layout.name.to_string(),
                intrinsic: callable.intrinsic,
                external_identity: callable.external_identity,
                parameters: parameters.clone(),
                contexts,
                context_formal: None,
                result: result.clone(),
                role: callable.role,
                effect: callable.effect,
                body: None,
                result_expression: None,
                contextual_operation: callable
                    .contextual_operation
                    .map(|operation| checked_abi_contextual_operation(layout, operation))
                    .transpose()?,
            });
            declarations.push(CheckedDeclaration {
                id: layout.declaration,
                scope_id: LexicalScopeId(0),
                name: layout.name.to_string(),
                kind: match callable.kind {
                    crate::KernelCallableKind::Builtin => CheckedDeclarationKind::Builtin,
                    crate::KernelCallableKind::External => CheckedDeclarationKind::External,
                    crate::KernelCallableKind::User => unreachable!("validated above"),
                },
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Function {
                        args: parameters
                            .iter()
                            .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                            .map(|parameter| parameter.flow_type.ty.clone())
                            .collect(),
                        result: Box::new(result),
                    },
                },
                value: None,
                body_scope: None,
                span: CheckedSpan::default(),
            });
            declarations.extend(parameters.into_iter().map(|parameter| CheckedDeclaration {
                id: parameter.decl_id,
                scope_id: LexicalScopeId(0),
                name: parameter.name,
                kind: match parameter.kind {
                    CheckedParameterKind::Value => CheckedDeclarationKind::ValueParameter,
                    CheckedParameterKind::Out => CheckedDeclarationKind::OutParameter,
                },
                flow_type: parameter.flow_type,
                value: None,
                body_scope: None,
                span: CheckedSpan::default(),
            }));
        }
        self.validate_materialized_count(
            "ABI callable",
            callables.len(),
            self.totals.abi_callables,
        )?;
        let expected_declarations = self
            .totals
            .declarations
            .checked_sub(self.definition_declarations_end)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked ABI declaration range precedes definition declarations",
                )
            })?;
        self.validate_materialized_count(
            "ABI declaration",
            declarations.len(),
            expected_declarations,
        )?;
        Ok((
            callables.into_boxed_slice(),
            declarations.into_boxed_slice(),
        ))
    }

    /// Link every solved call occurrence directly from its definition
    /// artifact and the callable table produced by this same layout.
    ///
    /// No signature matching or type solving happens here. The kernel already
    /// retained the matched input edges, generated OUT/context declarations,
    /// stable type-parameter substitutions, and solved result. This pass only
    /// relocates those facts into final checked IDs.
    pub fn materialize_calls(
        &self,
        project: &KernelProjectInput,
        snapshot: &KernelCheckedSnapshot,
        callables: &[CheckedCallableSignature],
        declarations: &[CheckedDeclaration],
    ) -> Result<(Box<[CheckedCall]>, Box<[StableOccurrenceKey]>), KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "call")?;
        let callable_by_declaration = callables
            .iter()
            .map(|callable| (callable.decl_id, callable))
            .collect::<BTreeMap<_, _>>();
        if callable_by_declaration.len() != callables.len() {
            return Err(KernelCheckedLinkError::new(
                "kernel checked call materializer received duplicate callable declarations",
            ));
        }
        let declaration_by_id = declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let mut calls = Vec::with_capacity(self.totals.calls as usize);
        let mut call_occurrences = Vec::with_capacity(self.totals.calls as usize);
        let mut type_cache = snapshot.definition_code.materialization_cache();
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            let local = self.definition(owner)?;
            let code = definition.code();
            let (packed_calls, call_results) = {
                let mut materializer =
                    code.linked_materializer(&mut type_cache, local.type_variables.start);
                let packed_calls = (0..definition.call_count())
                    .map(|ordinal| {
                        materializer.materialize_call_facts(ordinal).ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} has no packed call facts {}",
                                owner.0, ordinal
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let call_results = (0..definition.call_count())
                    .map(|ordinal| {
                        let call = definition.call(ordinal).ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} omits packed call {ordinal}",
                                owner.0,
                            ))
                        })?;
                        materializer
                            .materialize_expression(call.expression().0 as usize)
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition {} has no packed call result {}",
                                    owner.0,
                                    call.expression().0
                                ))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (packed_calls, call_results)
            };
            let mut syntax_by_expression = BTreeMap::new();
            for syntax in &facts.call_syntax {
                if syntax_by_expression
                    .insert(syntax.expression, syntax)
                    .is_some()
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats call syntax expression {}",
                        owner.0, syntax.expression.0,
                    )));
                }
            }
            let owner_callable = definition
                .linkage()
                .root_statement
                .and_then(|root| facts.statements.get(root.0 as usize))
                .filter(|root| matches!(root.kind, crate::KernelStatementKind::Function { .. }))
                .map(|_| local.public_declaration);
            for (ordinal, (packed_call, call_result)) in
                packed_calls.into_iter().zip(call_results).enumerate()
            {
                let call = definition.call(ordinal).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} omits packed call {ordinal}",
                        owner.0,
                    ))
                })?;
                let syntax = syntax_by_expression
                    .get(&call.expression())
                    .copied()
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} call expression {} has no authored syntax row",
                            owner.0,
                            call.expression().0,
                        ))
                    })?;
                let callable_id = match call.target() {
                    KernelCallTargetRef::User { target, .. } => {
                        self.definition(target)?.public_declaration
                    }
                    KernelCallTargetRef::RenderConstructor { .. }
                    | KernelCallTargetRef::PureBuiltin { .. }
                    | KernelCallTargetRef::FixedAbi
                    | KernelCallTargetRef::HostEffect { .. }
                    | KernelCallTargetRef::FieldProjection { .. } => {
                        self.abi_callable(&syntax.function)?.declaration
                    }
                };
                let target = callable_by_declaration.get(&callable_id).copied().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} call `{}` targets declaration {} without a materialized signature",
                        owner.0, syntax.function, callable_id.0,
                    ))
                })?;
                if target.name != syntax.function.as_ref() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} call syntax names `{}` but its target is `{}`",
                        owner.0, syntax.function, target.name,
                    )));
                }
                let parameter_for_input = |role: KernelCallInputRoleRef<'_>| {
                    let parameter = match role {
                        KernelCallInputRoleRef::Formal { ordinal } => target
                            .parameters
                            .get(ordinal as usize)
                            .filter(|parameter| parameter.ordinal == ordinal as usize),
                        KernelCallInputRoleRef::Abi { name } => target
                            .parameters
                            .iter()
                            .find(|parameter| parameter.name == name)
                            .or_else(|| {
                                (name == "$pipe").then(|| {
                                    target.parameters.iter().find(|parameter| {
                                        parameter.kind == CheckedParameterKind::Value
                                    })
                                })?
                            }),
                    };
                    parameter.ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} call `{}` has an input without a target parameter: {:?}",
                            owner.0, syntax.function, role,
                        ))
                    })
                };
                let pipe_input = syntax
                    .pipe_input
                    .map(|value| {
                        definition
                            .resolve_value(value, call.expression().0 as usize)
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))
                    })
                    .transpose()?;
                let mut entries = Vec::with_capacity(call.inputs().len());
                for input in call.inputs() {
                    let role = call
                        .input_role(input)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let value = call
                        .input_value(input)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    if let (
                        KernelCallTargetRef::User { target, .. },
                        KernelCallInputRoleRef::Formal { ordinal },
                    ) = (call.target(), role)
                        && snapshot.definition(target).is_some_and(|definition| {
                            definition.linkage().context_formal_ordinal == Some(ordinal)
                        })
                    {
                        // PASSED is a context binding, not a normal checked
                        // call entry. It remains in the solver input table so
                        // the occurrence substitution can retain correlation.
                        continue;
                    }
                    let parameter = parameter_for_input(role)?;
                    let from_pipe = pipe_input == Some(value);
                    let argument = (!from_pipe)
                        .then(|| {
                            syntax.arguments.iter().find(|argument| {
                                argument.name.as_ref() == parameter.name
                                    && definition
                                        .resolve_value(
                                            argument.value,
                                            call.expression().0 as usize,
                                        )
                                        .is_ok_and(|argument| argument == value)
                            })
                        })
                        .flatten()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} call `{}` input `{}` has no exact authored argument",
                                owner.0, syntax.function, parameter.name,
                            ))
                        });
                    match parameter.kind {
                        CheckedParameterKind::Value => {
                            if let Ok(argument) = argument.as_ref()
                                && argument.kind != KernelCallArgumentKind::Named
                            {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} call `{}` binds value input `{}` as a bare OUT",
                                    owner.0, syntax.function, parameter.name,
                                )));
                            }
                            entries.push(CheckedCallEntry::Input {
                                formal: parameter.decl_id,
                                name: parameter.name.clone(),
                                value: self.expression(owner, value)?,
                                from_pipe,
                                evaluation_scope: parameter.evaluation_scope,
                            });
                        }
                        CheckedParameterKind::Out => {
                            if from_pipe {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} call `{}` pipes into OUT parameter `{}`",
                                    owner.0, syntax.function, parameter.name,
                                )));
                            }
                            let argument = argument?;
                            match argument.kind {
                                KernelCallArgumentKind::BareBinding => {
                                    let declaration = exact_local_declaration_by_origin(
                                        definition,
                                        crate::KernelDeclarationOrigin::CallbackBinding {
                                            call: call.expression(),
                                            ordinal: parameter.ordinal as u32,
                                        },
                                        "FreshOut",
                                    )?;
                                    let presentation =
                                        declaration_presentation(facts, declaration.id)?;
                                    let scope = presentation.body_scope.ok_or_else(|| {
                                        KernelCheckedLinkError::new(format!(
                                            "kernel definition {} FreshOut `{}` has no output scope",
                                            owner.0, parameter.name,
                                        ))
                                    })?;
                                    entries.push(CheckedCallEntry::FreshOut {
                                        formal: parameter.decl_id,
                                        name: parameter.name.clone(),
                                        output: self.declaration(
                                            owner,
                                            KernelDeclarationReference::Local(declaration.id),
                                        )?,
                                        scope_id: self
                                            .scope(owner, KernelScopeReference::Local(scope))?,
                                    });
                                }
                                KernelCallArgumentKind::Named => {
                                    let KernelValueReference::Local(expression) = value else {
                                        return Err(KernelCheckedLinkError::new(format!(
                                            "kernel definition {} call `{}` forwards OUT `{}` through a non-local occurrence",
                                            owner.0, syntax.function, parameter.name,
                                        )));
                                    };
                                    let binding = definition
                                        .facts()
                                        .lexical_bindings
                                        .iter()
                                        .find(|binding| {
                                            binding.expression == expression
                                                && binding.projection.is_empty()
                                        })
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel definition {} call `{}` forwarded OUT `{}` has no exact lexical target",
                                                owner.0, syntax.function, parameter.name,
                                            ))
                                        })?;
                                    let KernelLexicalBindingTargetRef::Declaration(
                                        target_reference,
                                    ) = definition.resolve_lexical_target(binding).map_err(
                                        |error| KernelCheckedLinkError::new(error.to_string()),
                                    )?
                                    else {
                                        return Err(KernelCheckedLinkError::new(format!(
                                            "kernel definition {} call `{}` forwarded OUT `{}` targets a non-declaration",
                                            owner.0, syntax.function, parameter.name,
                                        )));
                                    };
                                    let target_declaration =
                                        self.declaration(owner, target_reference)?;
                                    let target_name = declaration_by_id
                                        .get(&target_declaration)
                                        .map(|declaration| declaration.name.clone())
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel definition {} call `{}` forwarded OUT target {} has no declaration row",
                                                owner.0, syntax.function, target_declaration.0,
                                            ))
                                        })?;
                                    entries.push(CheckedCallEntry::ForwardOut {
                                        formal: parameter.decl_id,
                                        name: parameter.name.clone(),
                                        target: target_declaration,
                                        target_name,
                                    });
                                }
                            }
                        }
                    }
                }

                let mut contexts = Vec::with_capacity(target.contexts.len());
                for (context_ordinal, context) in target.contexts.iter().enumerate() {
                    let context_ordinal = u32::try_from(context_ordinal).map_err(|_| {
                        KernelCheckedLinkError::new(
                            "kernel checked call context ordinal exceeds u32",
                        )
                    })?;
                    let declaration = exact_local_declaration_by_origin(
                        definition,
                        crate::KernelDeclarationOrigin::CallContext {
                            call: call.expression(),
                            ordinal: context_ordinal,
                        },
                        "call context",
                    )?;
                    if declaration.name.as_ref() != context.name {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} call `{}` context {} is named `{}` instead of `{}`",
                            owner.0,
                            syntax.function,
                            context_ordinal,
                            declaration.name,
                            context.name,
                        )));
                    }
                    let presentation = declaration_presentation(facts, declaration.id)?;
                    contexts.push(boon_checked::CheckedCallContext {
                        declaration: self.declaration(
                            owner,
                            KernelDeclarationReference::Local(declaration.id),
                        )?,
                        signature: context_ordinal as usize,
                        scope_id: self.scope(owner, presentation.scope)?,
                    });
                }

                let context_binding = if let Some(pass) = syntax.pass {
                    CheckedContextBinding::Explicit {
                        value: self.expression(
                            owner,
                            definition
                                .resolve_value(pass.value, call.expression().0 as usize)
                                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?,
                        )?,
                        span: checked_span(pass.span),
                    }
                } else if let KernelCallTargetRef::User {
                    inherited_formal: Some(inherited),
                    ..
                } = call.target()
                {
                    if definition.linkage().context_formal_ordinal != Some(inherited.caller_ordinal)
                    {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} call `{}` inherits caller formal {} without matching linkage",
                            owner.0, syntax.function, inherited.caller_ordinal,
                        )));
                    }
                    CheckedContextBinding::Inherited {
                        formal: local.context_formal.ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} call `{}` inherits a missing context formal",
                                owner.0, syntax.function,
                            ))
                        })?,
                    }
                } else {
                    CheckedContextBinding::None
                };

                let (
                    raw_substitutions,
                    target_variables,
                    target_type_variables,
                    target_variables_are_linked,
                    context,
                ) = match call.target() {
                    KernelCallTargetRef::User { target, .. } => {
                        let target_definition = snapshot.definition(target).ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel call `{}` references missing target definition {}",
                                syntax.function, target.0,
                            ))
                        })?;
                        let target_code =
                            snapshot.definition_code.definition(target).ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel call `{}` has no packed target definition {}",
                                    syntax.function, target.0,
                                ))
                            })?;
                        let target_layout = self.definition(target)?;
                        let mut target_materializer = target_code.linked_materializer(
                            &mut type_cache,
                            target_layout.type_variables.start,
                        );
                        let target_formals = (0..target_code.formals().len())
                            .map(|ordinal| {
                                target_materializer
                                    .materialize_formal(ordinal)
                                    .ok_or_else(|| {
                                        KernelCheckedLinkError::new(format!(
                                            "kernel callable `{}` has no packed formal {ordinal}",
                                            syntax.function,
                                        ))
                                    })
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let target_result = target_materializer.materialize_result();
                        let variables = callable_type_parameter_variables(
                            target_formals.iter(),
                            &target_result,
                        );
                        let context = target_definition
                                .linkage()
                                .context_formal_ordinal
                                .map(|ordinal| {
                                    let flow = target_materializer
                                        .materialize_formal(ordinal as usize)
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel callable `{}` context ordinal {ordinal} is missing",
                                                syntax.function,
                                            ))
                                        })?;
                                    let formal = self
                                        .definition(target)?
                                        .context_formal
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel callable `{}` context has no checked formal",
                                                syntax.function,
                                            ))
                                        })?;
                                    Ok::<_, KernelCheckedLinkError>((
                                        formal,
                                        type_variables_in_flow(&flow),
                                    ))
                                })
                                .transpose()?;
                        (
                            packed_call.substitutions.into_vec(),
                            variables,
                            target_layout.type_variables,
                            true,
                            context,
                        )
                    }
                    KernelCallTargetRef::RenderConstructor { .. }
                    | KernelCallTargetRef::PureBuiltin { .. }
                    | KernelCallTargetRef::FixedAbi
                    | KernelCallTargetRef::HostEffect { .. }
                    | KernelCallTargetRef::FieldProjection { .. } => {
                        let contract =
                            project.abi().callable(&syntax.function).ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel call `{}` has no immutable ABI contract",
                                    syntax.function,
                                ))
                            })?;
                        (
                            packed_call.substitutions.into_vec(),
                            callable_type_parameter_variables(
                                contract
                                    .parameters
                                    .iter()
                                    .map(|parameter| &parameter.flow_type),
                                &contract.result,
                            ),
                            self.abi_callable(&syntax.function)?.type_variables,
                            false,
                            None,
                        )
                    }
                };
                let mut type_substitutions = Vec::with_capacity(raw_substitutions.len());
                let mut contextual_substitutions = Vec::new();
                for substitution in raw_substitutions {
                    let raw_variable = target_variables
                        .get(substitution.variable.0 as usize)
                        .copied()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel call `{}` substitution parameter {} is outside its target scheme",
                                syntax.function, substitution.variable.0,
                            ))
                        })?;
                    let variable = if target_variables_are_linked {
                        raw_variable
                    } else {
                        TypeVar(
                            target_type_variables
                                .resolve(raw_variable.0, "call target type variable")?,
                        )
                    };
                    let value = substitution.value;
                    if let Some((formal, context_variables)) = &context
                        && context_variables.contains(&raw_variable)
                    {
                        contextual_substitutions.push(CheckedContextTypeSubstitution {
                            formal: *formal,
                            variable,
                            value: value.clone(),
                        });
                    }
                    type_substitutions.push(CheckedTypeSubstitution { variable, value });
                }
                let result = call_result;
                let syntax_discriminated_result = packed_call.syntax_discriminated_result;
                let presentation = expression_presentation(facts, call.expression())?;
                let id = self.call(
                    owner,
                    u32::try_from(ordinal).map_err(|_| {
                        KernelCheckedLinkError::new("kernel checked call ordinal exceeds u32")
                    })?,
                )?;
                if id.0 as usize != calls.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked call materializer expected row {} but linked {}",
                        calls.len(),
                        id.0,
                    )));
                }
                calls.push(CheckedCall {
                    id,
                    expression: self
                        .expression(owner, KernelValueReference::Local(call.expression()))?,
                    callable: callable_id,
                    owner_callable,
                    function: syntax.function.to_string(),
                    intrinsic: target.intrinsic,
                    entries,
                    contexts,
                    context_binding,
                    contextual_substitutions,
                    type_substitutions,
                    syntax_discriminated_result,
                    result,
                    role: target.role,
                    span: checked_span(presentation.span),
                });
                call_occurrences.push(syntax.occurrence.clone());
            }
        }
        self.validate_materialized_count("call", calls.len(), self.totals.calls)?;
        self.validate_materialized_count(
            "call occurrence",
            call_occurrences.len(),
            self.totals.calls,
        )?;
        Ok((
            calls.into_boxed_slice(),
            call_occurrences.into_boxed_slice(),
        ))
    }

    /// Emit every SOURCE resource directly from its solved definition row.
    ///
    /// The expression presentation owns the source coordinate and lexical
    /// scope. Resource identity, declaration authority, semantic path, and
    /// payload contract are already explicit in the immutable artifact, so
    /// this pass performs only dense relocation.
    pub fn materialize_sources(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedSource]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "SOURCE")?;
        let mut sources = Vec::with_capacity(self.totals.sources as usize);
        let mut type_cache = snapshot.definition_code.materialization_cache();
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer = code.linked_materializer(
                &mut type_cache,
                self.definition(owner)?.type_variables.start,
            );
            for (ordinal, source) in facts.sources.iter().enumerate() {
                let id = self.source(owner, source.id.0)?;
                if id.0 as usize != sources.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked SOURCE materializer expected row {} but linked {}",
                        sources.len(),
                        id.0,
                    )));
                }
                let presentation = expression_presentation(facts, source.expression)?;
                sources.push(CheckedSource {
                    id,
                    declaration: self.declaration(owner, source.declaration)?,
                    statement: self.statement(owner, source.statement)?,
                    expression: self
                        .expression(owner, KernelValueReference::Local(source.expression))?,
                    owner_scope: self.scope(owner, presentation.scope)?,
                    path: self.semantic_path_parts(
                        owner,
                        source.declaration,
                        &source.projection,
                    )?,
                    interval_ms: source.interval_ms,
                    payload_type: materializer
                        .materialize_source_payload_type(ordinal)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} has no packed SOURCE payload {}",
                                owner.0, ordinal
                            ))
                        })?,
                    span: checked_span(presentation.span),
                });
            }
        }
        self.validate_materialized_count("SOURCE", sources.len(), self.totals.sources)?;
        Ok(sources.into_boxed_slice())
    }

    /// Emit every persistent state row without rediscovering HOLD/LATEST
    /// structure from checked expressions.
    pub fn materialize_states(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedState]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "state")?;
        let mut states = Vec::with_capacity(self.totals.states as usize);
        let mut type_cache = snapshot.definition_code.materialization_cache();
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer = code.linked_materializer(
                &mut type_cache,
                self.definition(owner)?.type_variables.start,
            );
            for (ordinal, state) in definition.states().enumerate() {
                let input = state.input();
                let id = self.state(owner, state.id().0)?;
                if id.0 as usize != states.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked state materializer expected row {} but linked {}",
                        states.len(),
                        id.0,
                    )));
                }
                let (owner_scope, span) =
                    if input.kind == boon_checked::CheckedStateKind::StatementHold {
                        let (statement_owner, statement) =
                            self.local_statement_reference(snapshot, owner, input.statement)?;
                        let presentation = statement_presentation(
                            &snapshot.definition_facts[statement_owner.0 as usize],
                            statement,
                        )?;
                        (
                            self.scope(statement_owner, presentation.scope)?,
                            checked_span(presentation.span),
                        )
                    } else {
                        let presentation = expression_presentation(facts, input.expression)?;
                        (
                            self.scope(owner, presentation.scope)?,
                            checked_span(presentation.span),
                        )
                    };
                states.push(CheckedState {
                    id,
                    binding_declaration: self.declaration(owner, input.binding_declaration)?,
                    declaration: self.declaration(owner, input.declaration)?,
                    statement: self.statement(owner, input.statement)?,
                    expression: self
                        .expression(owner, KernelValueReference::Local(input.expression))?,
                    initial: self.expression(
                        owner,
                        state
                            .initial()
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?,
                    )?,
                    owner_scope,
                    path: match state.path() {
                        KernelStatePathRef::Authored(projection) => {
                            self.semantic_path_parts(owner, input.declaration, projection)?
                        }
                        KernelStatePathRef::Synthetic(ordinal) => CheckedSemanticPath {
                            anchor: self.declaration(owner, input.declaration)?,
                            projection: vec![format!("state_{ordinal}")],
                        },
                    },
                    kind: input.kind,
                    flow_type: materializer
                        .materialize_state_flow(ordinal)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} has no packed state flow {}",
                                owner.0, ordinal
                            ))
                        })?,
                    span,
                });
            }
        }
        self.validate_materialized_count("state", states.len(), self.totals.states)?;
        Ok(states.into_boxed_slice())
    }

    /// Emit every persistent LIST authority from the kernel's single solved
    /// resource table.
    pub fn materialize_lists(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedList]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "LIST")?;
        let mut lists = Vec::with_capacity(self.totals.lists as usize);
        let mut type_cache = snapshot.definition_code.materialization_cache();
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer = code.linked_materializer(
                &mut type_cache,
                self.definition(owner)?.type_variables.start,
            );
            for (ordinal, list) in facts.lists.iter().enumerate() {
                let id = self.list(owner, list.id.0)?;
                if id.0 as usize != lists.len() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked LIST materializer expected row {} but linked {}",
                        lists.len(),
                        id.0,
                    )));
                }
                let presentation = expression_presentation(facts, list.producer)?;
                lists.push(CheckedList {
                    id,
                    declaration: self.declaration(owner, list.declaration)?,
                    statement: self.statement(owner, list.statement)?,
                    producer: self.expression(owner, KernelValueReference::Local(list.producer))?,
                    owner_scope: self.scope(owner, presentation.scope)?,
                    path: self.semantic_path_parts(owner, list.declaration, &list.projection)?,
                    item_type: materializer
                        .materialize_list_item_type(ordinal)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} has no packed LIST item {}",
                                owner.0, ordinal
                            ))
                        })?,
                    capacity: list.capacity,
                    key_policy: list.key_policy,
                    span: checked_span(presentation.span),
                });
            }
        }
        self.validate_materialized_count("LIST", lists.len(), self.totals.lists)?;
        Ok(lists.into_boxed_slice())
    }

    fn validate_snapshot_definition_count(
        &self,
        snapshot: &KernelCheckedSnapshot,
        label: &str,
    ) -> Result<(), KernelCheckedLinkError> {
        if snapshot.definition_count() != self.definitions.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked {label} materializer has {} definitions for a {}-definition layout",
                snapshot.definition_count(),
                self.definitions.len(),
            )));
        }
        Ok(())
    }

    fn validate_materialized_count(
        &self,
        label: &str,
        actual: usize,
        expected: u32,
    ) -> Result<(), KernelCheckedLinkError> {
        if actual != expected as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked {label} materializer produced {actual} rows for a {expected}-row layout",
            )));
        }
        Ok(())
    }

    fn semantic_path_parts(
        &self,
        owner: KernelOwnerId,
        anchor: KernelDeclarationReference,
        projection: &[Box<str>],
    ) -> Result<CheckedSemanticPath, KernelCheckedLinkError> {
        Ok(CheckedSemanticPath {
            anchor: self.declaration(owner, anchor)?,
            projection: projection.iter().map(|field| field.to_string()).collect(),
        })
    }

    fn local_statement_reference(
        &self,
        snapshot: &KernelCheckedSnapshot,
        owner: KernelOwnerId,
        statement: KernelStatementReference,
    ) -> Result<(KernelOwnerId, crate::KernelStatementId), KernelCheckedLinkError> {
        match statement {
            KernelStatementReference::Local(statement) => Ok((owner, statement)),
            KernelStatementReference::OwnerPublic(owner) => {
                let statement = snapshot
                    .definition(owner)
                    .and_then(|definition| definition.linkage().root_statement)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel state references definition {} without a root statement",
                            owner.0,
                        ))
                    })?;
                Ok((owner, statement))
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
            KernelDeclarationReference::OwnerDeclaration { owner, declaration } => Ok(DeclId(
                self.definition(owner)?
                    .declarations
                    .resolve(declaration.0, "external declaration")?,
            )),
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
        for definition in snapshot.definition_refs() {
            let facts = definition.facts();
            let owner = definition.owner();
            let local = self.definition(owner)?.clone();
            let _ = self.scope(owner, facts.presentation.containing_scope)?;
            for scope in &facts.presentation.scopes {
                let _ = local.scopes.resolve(scope.id.0, "scope presentation")?;
                let _ = self.scope(owner, scope.parent)?;
                if let Some(declaration) = scope.owner {
                    let _ = self.declaration(owner, declaration)?;
                    resolved = resolved.saturating_add(1);
                }
                resolved = resolved.saturating_add(1);
            }
            for expression in &facts.presentation.expressions {
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
            for statement in &facts.presentation.statements {
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
            for declaration in &facts.presentation.declarations {
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
            for (expression_index, expression) in definition.input().nodes.iter().enumerate() {
                let expression_id = crate::KernelExpressionId(
                    u32::try_from(expression_index)
                        .expect("kernel expression count exceeds dense u32 namespace"),
                );
                let _ = local
                    .expressions
                    .resolve(expression_id.0, "expression artifact")?;
                for input in &expression.inputs {
                    let value = definition
                        .resolve_value(input.expression, expression_index)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for statement in &facts.statements {
                let _ = local
                    .statements
                    .resolve(statement.id.0, "statement artifact")?;
                if let Some(value) = statement.value {
                    let value = definition
                        .resolve_value(value, statement.id.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
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
            for declaration in &facts.declarations {
                let _ = local
                    .declarations
                    .resolve(declaration.id.0, "declaration artifact")?;
                if let Some(value) = declaration.value {
                    let value = definition
                        .resolve_value(value, declaration.id.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for binding in &facts.lexical_bindings {
                let _ = local
                    .expressions
                    .resolve(binding.expression.0, "lexical occurrence")?;
                match definition
                    .resolve_lexical_target(binding)
                    .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?
                {
                    KernelLexicalBindingTargetRef::Declaration(declaration) => {
                        let _ = self.declaration(owner, declaration)?;
                    }
                    KernelLexicalBindingTargetRef::ContextFormal { ordinal } => {
                        if definition.linkage().context_formal_ordinal != Some(ordinal)
                            || local.context_formal.is_none()
                        {
                            return Err(KernelCheckedLinkError::new(format!(
                                "kernel definition {} lexical context ordinal {ordinal} has no exact context-formal anchor",
                                owner.0,
                            )));
                        }
                    }
                    KernelLexicalBindingTargetRef::Value { provider } => {
                        let _ = self.expression(owner, provider)?;
                    }
                    KernelLexicalBindingTargetRef::RuntimeContext => {}
                }
                resolved = resolved.saturating_add(1);
            }
            for ordinal in 0..definition.call_count() {
                let call = definition.call(ordinal).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} omits packed call {ordinal}",
                        owner.0,
                    ))
                })?;
                let _ = self.call(
                    owner,
                    u32::try_from(ordinal).map_err(|_| {
                        KernelCheckedLinkError::new("kernel call ordinal exceeds u32")
                    })?,
                )?;
                let _ = local
                    .expressions
                    .resolve(call.expression().0, "call expression")?;
                if !matches!(
                    call.node().kind,
                    crate::KernelOwnerNodeKind::UserCall { .. }
                        | crate::KernelOwnerNodeKind::RenderConstructor { .. }
                        | crate::KernelOwnerNodeKind::PureBuiltin { .. }
                        | crate::KernelOwnerNodeKind::FixedAbiCall { .. }
                        | crate::KernelOwnerNodeKind::HostEffect { .. }
                        | crate::KernelOwnerNodeKind::FieldProjection { .. }
                ) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} packed call {} points to a non-call node",
                        owner.0,
                        call.expression().0,
                    )));
                }
                if let KernelCallTargetRef::User { target, .. } = call.target() {
                    let _ = self.definition(target)?.public_declaration;
                    resolved = resolved.saturating_add(1);
                }
                for input in call.inputs() {
                    let _ = call
                        .input_role(input)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let value = call
                        .input_value(input)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for call in &facts.call_syntax {
                let _ = local
                    .expressions
                    .resolve(call.expression.0, "authored call expression")?;
                if let Some(value) = call.pipe_input {
                    let value = definition
                        .resolve_value(value, call.expression.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
                for argument in &call.arguments {
                    let value = definition
                        .resolve_value(argument.value, call.expression.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
                if let Some(pass) = call.pass {
                    let value = definition
                        .resolve_value(pass.value, call.expression.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for shape in &facts.execution_shapes {
                let _ = local
                    .expressions
                    .resolve(shape.expression().0, "execution-shape expression")?;
                match shape {
                    crate::KernelExecutionShapeInput::Conditional { .. } => {}
                    crate::KernelExecutionShapeInput::Record { fields, .. } => {
                        for field in fields {
                            if let Some(declaration) = field.declaration {
                                let declaration = definition
                                    .resolve_structural_declaration(
                                        declaration,
                                        field.value,
                                        shape.expression().0 as usize,
                                    )
                                    .map_err(|error| {
                                        KernelCheckedLinkError::new(error.to_string())
                                    })?;
                                let _ = self.declaration(owner, declaration)?;
                                resolved = resolved.saturating_add(1);
                            }
                            let value = definition
                                .resolve_value(field.value, shape.expression().0 as usize)
                                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                            let _ = self.expression(owner, value)?;
                            resolved = resolved.saturating_add(1);
                        }
                    }
                    crate::KernelExecutionShapeInput::Block {
                        bindings, result, ..
                    } => {
                        for binding in bindings {
                            let declaration = definition
                                .resolve_structural_declaration(
                                    binding.declaration,
                                    binding.value,
                                    shape.expression().0 as usize,
                                )
                                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                            let value = definition
                                .resolve_value(binding.value, shape.expression().0 as usize)
                                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                            let _ = self.declaration(owner, declaration)?;
                            let _ = self.expression(owner, value)?;
                            resolved = resolved.saturating_add(2);
                        }
                        if let Some(result) = result {
                            let value = definition
                                .resolve_value(*result, shape.expression().0 as usize)
                                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                            let _ = self.expression(owner, value)?;
                            resolved = resolved.saturating_add(1);
                        }
                    }
                    crate::KernelExecutionShapeInput::MatchArm {
                        selector, bindings, ..
                    } => {
                        let value = definition
                            .resolve_value(*selector, shape.expression().0 as usize)
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                        let _ = self.expression(owner, value)?;
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
            for source in &facts.sources {
                let _ = self.source(owner, source.id.0)?;
                let _ = self.declaration(owner, source.declaration)?;
                let _ = self.statement(owner, source.statement)?;
                let _ = local
                    .expressions
                    .resolve(source.expression.0, "source expression")?;
                let _ = self.declaration(owner, source.declaration)?;
                resolved = resolved.saturating_add(4);
            }
            for (state_ordinal, packed) in definition.code().states().iter().enumerate() {
                let state = facts
                    .states
                    .get(packed.input_ordinal as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} published state {} references missing candidate {}",
                            owner.0, state_ordinal, packed.input_ordinal,
                        ))
                    })?;
                if state.synthetic_path != packed.synthetic_ordinal().is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} published state {} disagrees on synthetic path authority",
                        owner.0, state_ordinal,
                    )));
                }
                let state_id = crate::KernelStateId(
                    u32::try_from(state_ordinal)
                        .expect("kernel state count exceeds dense u32 namespace"),
                );
                let _ = self.state(owner, state_id.0)?;
                let _ = self.declaration(owner, state.binding_declaration)?;
                let _ = self.declaration(owner, state.declaration)?;
                let _ = self.statement(owner, state.statement)?;
                let _ = local
                    .expressions
                    .resolve(state.expression.0, "state expression")?;
                let initial = definition
                    .resolve_value(state.initial, state.expression.0 as usize)
                    .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                let _ = self.expression(owner, initial)?;
                let _ = self.declaration(owner, state.declaration)?;
                resolved = resolved.saturating_add(6);
            }
            for list in &facts.lists {
                let _ = self.list(owner, list.id.0)?;
                let _ = self.declaration(owner, list.declaration)?;
                let _ = self.statement(owner, list.statement)?;
                let _ = local
                    .expressions
                    .resolve(list.producer.0, "list producer")?;
                let _ = self.declaration(owner, list.declaration)?;
                resolved = resolved.saturating_add(4);
            }
            for (expression, node) in definition.input().nodes.iter().enumerate() {
                if matches!(node.kind, crate::KernelOwnerNodeKind::HostEffect { .. }) {
                    let expression = u32::try_from(expression)
                        .expect("kernel expression count exceeds dense u32 namespace");
                    let _ = local
                        .expressions
                        .resolve(expression, "host-effect expression")?;
                    resolved = resolved.saturating_add(1);
                }
            }
        }
        self.totals.resolved_references = resolved;
        Ok(())
    }
}

fn checked_owner_id(index: usize, label: &str) -> Result<KernelOwnerId, KernelCheckedLinkError> {
    Ok(KernelOwnerId(u32::try_from(index).map_err(|_| {
        KernelCheckedLinkError::new(format!(
            "kernel checked {label} materializer definition count exceeds u32",
        ))
    })?))
}

#[cfg(test)]
fn definition_template_value(
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    value: KernelValueReference,
) -> Result<(KernelOwnerId, crate::KernelExpressionId), KernelCheckedLinkError> {
    match value {
        KernelValueReference::Local(expression) => Ok((owner, expression)),
        KernelValueReference::External(external) => match external.target {
            KernelExternalTarget::Expression(expression) => Ok((external.owner, expression)),
            KernelExternalTarget::Result => {
                let definition = snapshot
                    .definitions
                    .get(external.owner.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition template references missing external owner {}",
                            external.owner.0,
                        ))
                    })?;
                let expression = definition.linkage.result_expression.ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition template references owner {} without a result expression",
                        external.owner.0,
                    ))
                })?;
                Ok((external.owner, expression))
            }
        },
    }
}

#[cfg(test)]
fn definition_template_dependencies(
    snapshot: &KernelCheckedSnapshot,
    statement_child_dependencies: &BTreeMap<
        (KernelOwnerId, crate::KernelExpressionId),
        Vec<(KernelOwnerId, crate::KernelExpressionId)>,
    >,
    call_dependencies: &BTreeMap<
        (KernelOwnerId, crate::KernelExpressionId),
        Vec<(KernelOwnerId, crate::KernelExpressionId)>,
    >,
    read_provider_callables: &BTreeMap<(KernelOwnerId, crate::KernelExpressionId), Option<DeclId>>,
    callable: DeclId,
    owner: KernelOwnerId,
    expression: &crate::KernelExpressionArtifact,
) -> Result<Vec<(KernelOwnerId, crate::KernelExpressionId)>, KernelCheckedLinkError> {
    let definition = snapshot.definitions.get(owner.0 as usize).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition template references missing solved owner {}",
            owner.0,
        ))
    })?;
    let facts = snapshot
        .definition_facts
        .get(owner.0 as usize)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel definition template references missing owner {}",
                owner.0,
            ))
        })?;
    let payload = facts
        .expression_payloads
        .get(expression.id.0 as usize)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel definition {} template expression {} has no semantic payload",
                owner.0, expression.id.0,
            ))
        })?;
    // A non-empty authored delimiter is structurally interpreted as a record
    // by the type engine, but remains a delimiter in the checked expression
    // graph. Its field values are owned by child statements, not expression
    // dependencies. Preserve that checked execution boundary here.
    if matches!(payload, crate::KernelExpressionSemanticPayload::Delimiter) {
        return Ok(Vec::new());
    }
    if let Some(dependencies) = call_dependencies.get(&(owner, expression.id)) {
        return Ok(dependencies.clone());
    }
    let mut dependencies = Vec::new();
    for input in &expression.inputs {
        if matches!(
            input.role,
            crate::KernelOwnerEdgeRole::CallOutArgument { .. }
                | crate::KernelOwnerEdgeRole::HoldUpdate
        ) {
            continue;
        }
        if matches!(input.role, crate::KernelOwnerEdgeRole::ReadProvider) {
            let read_callable = read_provider_callables
                .get(&(owner, expression.id))
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} template read {} has no lexical callable authority",
                        owner.0, expression.id.0,
                    ))
                })?;
            if *read_callable != Some(callable) {
                continue;
            }
        }
        dependencies.push(definition_template_value(snapshot, owner, input.value)?);
    }
    if matches!(
        expression.kind,
        crate::KernelExpressionArtifactKind::Hold
            | crate::KernelExpressionArtifactKind::MatchArm { .. }
    ) {
        dependencies.extend(
            statement_child_dependencies
                .get(&(owner, expression.id))
                .into_iter()
                .flatten()
                .copied(),
        );
    }
    if matches!(expression.kind, crate::KernelExpressionArtifactKind::Block) {
        let mut shapes = definition.execution_shapes.iter().filter(|shape| {
            matches!(
                shape,
                crate::KernelExecutionShapeArtifact::Block {
                    expression: candidate,
                    ..
                } if *candidate == expression.id
            )
        });
        let Some(crate::KernelExecutionShapeArtifact::Block {
            bindings, result, ..
        }) = shapes.next()
        else {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} BLOCK expression {} has no execution shape",
                owner.0, expression.id.0,
            )));
        };
        if shapes.next().is_some() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} repeats a BLOCK shape for expression {}",
                owner.0, expression.id.0,
            )));
        }
        dependencies.extend(
            bindings
                .iter()
                .map(|binding| definition_template_value(snapshot, owner, binding.value))
                .collect::<Result<Vec<_>, _>>()?,
        );
        if let Some(result) = result {
            dependencies.push(definition_template_value(snapshot, owner, *result)?);
        }
    }
    Ok(dependencies)
}

#[cfg(test)]
fn definition_template_read_provider_callables(
    snapshot: &KernelCheckedSnapshot,
    layout: &KernelCheckedLinkLayout,
    linked_scopes: &[CheckedScope],
    linked_declarations: &[CheckedDeclaration],
) -> Result<
    BTreeMap<(KernelOwnerId, crate::KernelExpressionId), Option<DeclId>>,
    KernelCheckedLinkError,
> {
    let scopes = linked_scopes
        .iter()
        .map(|scope| (scope.id, scope))
        .collect::<BTreeMap<_, _>>();
    if scopes.len() != linked_scopes.len() {
        return Err(KernelCheckedLinkError::new(
            "kernel definition templates received duplicate linked scope IDs",
        ));
    }
    let declarations = linked_declarations
        .iter()
        .map(|declaration| (declaration.id, declaration))
        .collect::<BTreeMap<_, _>>();
    if declarations.len() != linked_declarations.len() {
        return Err(KernelCheckedLinkError::new(
            "kernel definition templates received duplicate linked declaration IDs",
        ));
    }
    let mut callable_by_scope = BTreeMap::new();
    for scope in linked_scopes {
        let mut current = scope.id;
        let mut visited = BTreeSet::new();
        let callable = loop {
            if !visited.insert(current) {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition template scope {} contains a parent cycle",
                    scope.id.0,
                )));
            }
            let definition = scopes.get(&current).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition template references missing linked scope {}",
                    current.0,
                ))
            })?;
            if definition.kind == CheckedScopeKind::Function {
                break definition.owner;
            }
            let Some(parent) = definition.parent else {
                break None;
            };
            current = parent;
        };
        callable_by_scope.insert(scope.id, callable);
    }

    let mut result = BTreeMap::new();
    for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
        let owner = checked_owner_id(owner_index, "definition template lexical read")?;
        for binding in &definition.lexical_bindings {
            if !definition
                .expressions
                .get(binding.expression.0 as usize)
                .filter(|expression| expression.id == binding.expression)
                .is_some_and(|expression| {
                    expression
                        .inputs
                        .iter()
                        .any(|input| matches!(input.role, crate::KernelOwnerEdgeRole::ReadProvider))
                })
            {
                continue;
            }
            let callable = match binding.target {
                KernelLexicalBindingTarget::Declaration(reference) => {
                    let declaration = layout.declaration(owner, reference)?;
                    let declaration = declarations.get(&declaration).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} template read {} targets missing declaration {}",
                            owner.0, binding.expression.0, declaration.0,
                        ))
                    })?;
                    if declaration.value.is_none() {
                        None
                    } else {
                        callable_by_scope
                            .get(&declaration.scope_id)
                            .copied()
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition template declaration {} references missing scope {}",
                                    declaration.id.0, declaration.scope_id.0,
                                ))
                            })?
                    }
                }
                KernelLexicalBindingTarget::ContextFormal { .. }
                | KernelLexicalBindingTarget::Value { .. }
                | KernelLexicalBindingTarget::RuntimeContext => None,
            };
            if result
                .insert((owner, binding.expression), callable)
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} repeats lexical callable authority for expression {}",
                    owner.0, binding.expression.0,
                )));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
fn definition_template_statement_child_dependencies(
    snapshot: &KernelCheckedSnapshot,
    layout: &KernelCheckedLinkLayout,
    linked_statements: &[CheckedStatement],
) -> Result<
    BTreeMap<
        (KernelOwnerId, crate::KernelExpressionId),
        Vec<(KernelOwnerId, crate::KernelExpressionId)>,
    >,
    KernelCheckedLinkError,
> {
    let mut expression_keys = BTreeMap::new();
    for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
        let owner = checked_owner_id(owner_index, "definition template expression reverse map")?;
        for expression in &definition.expressions {
            let linked = layout.expression(owner, KernelValueReference::Local(expression.id))?;
            if expression_keys
                .insert(linked, (owner, expression.id))
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition templates repeat linked expression {}",
                    linked.0,
                )));
            }
        }
    }
    let statements = linked_statements
        .iter()
        .map(|statement| (statement.id, statement))
        .collect::<BTreeMap<_, _>>();
    if statements.len() != linked_statements.len() {
        return Err(KernelCheckedLinkError::new(
            "kernel definition templates received duplicate linked statement IDs",
        ));
    }
    let mut statements_by_value = BTreeMap::<CheckedExprId, Vec<CheckedStatementId>>::new();
    for statement in linked_statements {
        if let Some(value) = statement.value {
            statements_by_value
                .entry(value)
                .or_default()
                .push(statement.id);
        }
    }

    let mut result = BTreeMap::new();
    for (owner_index, definition) in snapshot.definitions.iter().enumerate() {
        let owner = checked_owner_id(owner_index, "definition template statement dependency")?;
        for expression in definition.expressions.iter().filter(|expression| {
            matches!(
                expression.kind,
                crate::KernelExpressionArtifactKind::Hold
                    | crate::KernelExpressionArtifactKind::MatchArm { .. }
            )
        }) {
            let linked = layout.expression(owner, KernelValueReference::Local(expression.id))?;
            let values = linked_definition_template_statement_child_values(
                &statements,
                &statements_by_value,
                linked,
            )?;
            let dependencies = values
                .into_iter()
                .map(|value| {
                    expression_keys.get(&value).copied().ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition template statement dependency references missing expression {}",
                            value.0,
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            result.insert((owner, expression.id), dependencies);
        }
    }
    Ok(result)
}

#[cfg(test)]
fn linked_definition_template_statement_child_values(
    statements: &BTreeMap<CheckedStatementId, &CheckedStatement>,
    statements_by_value: &BTreeMap<CheckedExprId, Vec<CheckedStatementId>>,
    expression: CheckedExprId,
) -> Result<Vec<CheckedExprId>, KernelCheckedLinkError> {
    let Some(root) = statements_by_value
        .get(&expression)
        .into_iter()
        .flatten()
        .filter_map(|statement| statements.get(statement).copied())
        .find(|statement| statement.value == Some(expression) && !statement.children.is_empty())
    else {
        return Ok(Vec::new());
    };
    let mut pending = root.children.iter().rev().copied().collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut values = Vec::new();
    while let Some(statement) = pending.pop() {
        if !visited.insert(statement) {
            continue;
        }
        let definition = statements.get(&statement).copied().ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel definition template references missing linked statement {}",
                statement.0,
            ))
        })?;
        match definition.value {
            Some(value) if value == expression => {
                pending.extend(definition.children.iter().rev().copied());
            }
            Some(value) => values.push(value),
            None => pending.extend(definition.children.iter().rev().copied()),
        }
    }
    Ok(values)
}

#[cfg(test)]
fn definition_template_selector(
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    expression: &crate::KernelExpressionArtifact,
    layout: &KernelCheckedLinkLayout,
) -> Result<Option<CheckedDefinitionSelectorV1>, KernelCheckedLinkError> {
    let definition = snapshot.definitions.get(owner.0 as usize).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition template references missing owner {}",
            owner.0,
        ))
    })?;
    let mut matching_shapes = definition.execution_shapes.iter().filter(|shape| {
        matches!(
            shape,
            crate::KernelExecutionShapeArtifact::Conditional {
                expression: candidate,
                ..
            } if *candidate == expression.id
        )
    });
    let Some(shape) = matching_shapes.next() else {
        return Ok(None);
    };
    if matching_shapes.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} repeats a conditional shape for expression {}",
            owner.0, expression.id.0,
        )));
    }
    let crate::KernelExecutionShapeArtifact::Conditional { kind, .. } = shape else {
        unreachable!()
    };
    if *kind == crate::KernelConditionalKind::While {
        return Ok(None);
    }

    let mut selector_inputs = expression
        .inputs
        .iter()
        .filter(|input| matches!(input.role, crate::KernelOwnerEdgeRole::WhenInput));
    let selector = selector_inputs.next().ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition {} WHEN expression {} has no selector input",
            owner.0, expression.id.0,
        ))
    })?;
    if selector_inputs.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} WHEN expression {} has multiple selector inputs",
            owner.0, expression.id.0,
        )));
    }
    let (selector_owner, selector_expression) =
        definition_template_value(snapshot, owner, selector.value)?;
    let input = layout.expression(
        selector_owner,
        KernelValueReference::Local(selector_expression),
    )?;
    let arms = expression
        .inputs
        .iter()
        .filter(|input| matches!(input.role, crate::KernelOwnerEdgeRole::WhenArm))
        .map(|arm| {
            let (arm_owner, arm_expression) =
                definition_template_value(snapshot, owner, arm.value)?;
            layout.expression(arm_owner, KernelValueReference::Local(arm_expression))
        })
        .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
    Ok(Some(CheckedDefinitionSelectorV1 { input, arms }))
}

fn expression_presentation(
    facts: &crate::KernelDefinitionFactsInput,
    expression: crate::KernelExpressionId,
) -> Result<&crate::KernelExpressionPresentation, KernelCheckedLinkError> {
    facts
        .presentation
        .expressions
        .get(expression.0 as usize)
        .filter(|presentation| presentation.expression == expression)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel resource expression {} has no exact presentation row",
                expression.0,
            ))
        })
}

fn statement_presentation(
    facts: &crate::KernelDefinitionFactsInput,
    statement: crate::KernelStatementId,
) -> Result<&crate::KernelStatementPresentation, KernelCheckedLinkError> {
    facts
        .presentation
        .statements
        .get(statement.0 as usize)
        .filter(|presentation| presentation.statement == statement)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel resource statement {} has no exact presentation row",
                statement.0,
            ))
        })
}

fn declaration_presentation(
    facts: &crate::KernelDefinitionFactsInput,
    declaration: crate::KernelDeclarationId,
) -> Result<&crate::KernelDeclarationPresentation, KernelCheckedLinkError> {
    facts
        .presentation
        .declarations
        .get(declaration.0 as usize)
        .filter(|presentation| presentation.declaration == declaration)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel declaration {} has no exact presentation row",
                declaration.0,
            ))
        })
}

fn exact_local_declaration_by_origin<'a>(
    definition: KernelDefinitionRef<'a>,
    origin: crate::KernelDeclarationOrigin,
    label: &str,
) -> Result<&'a crate::KernelDeclarationInput, KernelCheckedLinkError> {
    let mut declarations = definition
        .facts()
        .declarations
        .iter()
        .filter(|declaration| declaration.origin == origin);
    let declaration = declarations.next().ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel checked linker cannot find {label} declaration for {origin:?}"
        ))
    })?;
    if declarations.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel checked linker found multiple {label} declarations for {origin:?}"
        )));
    }
    Ok(declaration)
}

fn checked_range(
    range: KernelCheckedRowRange,
) -> Result<std::ops::Range<usize>, KernelCheckedLinkError> {
    let start = range.start as usize;
    let end = range
        .start
        .checked_add(range.len)
        .ok_or_else(|| KernelCheckedLinkError::new("kernel checked row range overflowed"))?
        as usize;
    Ok(start..end)
}

fn rebase_checked_span(
    span: &mut CheckedSpan,
    start_line: usize,
    start_byte: usize,
    label: &str,
) -> Result<(), KernelCheckedLinkError> {
    span.line =
        start_line
            .checked_add(span.line.checked_sub(1).ok_or_else(|| {
                KernelCheckedLinkError::new(format!("{label} has no source line"))
            })?)
            .ok_or_else(|| KernelCheckedLinkError::new(format!("{label} line overflowed")))?;
    span.start = start_byte
        .checked_add(span.start)
        .ok_or_else(|| KernelCheckedLinkError::new(format!("{label} start overflowed")))?;
    span.end = start_byte
        .checked_add(span.end)
        .ok_or_else(|| KernelCheckedLinkError::new(format!("{label} end overflowed")))?;
    Ok(())
}

fn rebase_checked_expression_spans(
    expression: &mut CheckedExpression,
    start_line: usize,
    start_byte: usize,
) -> Result<(), KernelCheckedLinkError> {
    rebase_checked_span(
        &mut expression.span,
        start_line,
        start_byte,
        &format!("kernel checked expression row {}", expression.id.0),
    )?;
    let structural_fields: &mut [_] = match &mut expression.kind {
        CheckedExpressionKind::TaggedObject { fields, .. }
        | CheckedExpressionKind::Object { fields } => fields.as_mut_slice(),
        _ => &mut [],
    };
    for (ordinal, field) in structural_fields.iter_mut().enumerate() {
        rebase_checked_span(
            &mut field.span,
            start_line,
            start_byte,
            &format!(
                "kernel checked expression row {} field {ordinal}",
                expression.id.0,
            ),
        )?;
    }
    if let CheckedExpressionKind::Block { bindings, .. } = &mut expression.kind {
        for (ordinal, binding) in bindings.iter_mut().enumerate() {
            rebase_checked_span(
                &mut binding.span,
                start_line,
                start_byte,
                &format!(
                    "kernel checked expression row {} BLOCK binding {ordinal}",
                    expression.id.0,
                ),
            )?;
        }
    }
    Ok(())
}

fn checked_span(span: crate::KernelSourceSpan) -> CheckedSpan {
    CheckedSpan {
        line: span.line,
        start: span.start,
        end: span.end,
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
        crate::KernelDeclarationKind::ElementState => CheckedDeclarationKind::ElementState,
    }
}

fn statement_declaration_authority(
    definition: KernelDefinitionRef<'_>,
    statement: &crate::KernelStatementInput,
) -> Result<Option<KernelDeclarationReference>, KernelCheckedLinkError> {
    let mut declarations = definition
        .facts()
        .declarations
        .iter()
        .filter_map(|declaration| {
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
        (owns_authored_declaration && definition.linkage().root_statement == Some(statement.id))
            .then_some(definition.linkage().public_declaration)
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

#[allow(clippy::too_many_arguments)]
fn checked_expression_kind(
    layout: &KernelCheckedLinkLayout,
    owner: KernelOwnerId,
    definition: KernelDefinitionRef<'_>,
    facts: &crate::KernelDefinitionFactsInput,
    expression_id: crate::KernelExpressionId,
    expression: &crate::KernelOwnerNode,
    container_line: usize,
    container_declaration: Option<DeclId>,
    payload: &crate::KernelExpressionSemanticPayload,
    shape: Option<&crate::KernelExecutionShapeInput>,
    lexical: Option<&crate::KernelLexicalBindingInput>,
    call: Option<u32>,
    source_paths: &BTreeMap<DeclId, Vec<(Vec<String>, CheckedSourceId)>>,
) -> Result<CheckedExpressionKind, KernelCheckedLinkError> {
    if let Some(call) = call {
        return Ok(CheckedExpressionKind::Call {
            call: layout.call(owner, call)?,
        });
    }
    if let Some(binding) = lexical {
        let projection = binding
            .projection
            .iter()
            .map(|field| field.to_string())
            .collect::<Vec<_>>();
        return match definition
            .resolve_lexical_target(binding)
            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?
        {
            KernelLexicalBindingTargetRef::Declaration(target) => {
                let target = layout.declaration(owner, target)?;
                Ok(match binding.access {
                    crate::KernelLexicalAccess::Read => {
                        let (target, projection, source) =
                            canonical_checked_source_read(source_paths, target, projection);
                        CheckedExpressionKind::Read {
                            target,
                            source,
                            projection,
                        }
                    }
                    crate::KernelLexicalAccess::Drain => {
                        let (target, projection, _) =
                            canonical_checked_source_read(source_paths, target, projection);
                        CheckedExpressionKind::Drain { target, projection }
                    }
                })
            }
            KernelLexicalBindingTargetRef::ContextFormal { ordinal } => {
                let owner_layout = layout.definition(owner)?;
                if definition.linkage().context_formal_ordinal != Some(ordinal) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression {} reads context ordinal {} but owns {:?}",
                        owner.0,
                        expression_id.0,
                        ordinal,
                        definition.linkage().context_formal_ordinal,
                    )));
                }
                Ok(CheckedExpressionKind::Passed {
                    formal: owner_layout.context_formal.ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} has no linked context formal",
                            owner.0,
                        ))
                    })?,
                    projection,
                    access: match binding.access {
                        crate::KernelLexicalAccess::Read => CheckedPassedAccess::Read,
                        crate::KernelLexicalAccess::Drain => CheckedPassedAccess::Drain,
                    },
                })
            }
            KernelLexicalBindingTargetRef::RuntimeContext => {
                if binding.access == crate::KernelLexicalAccess::Drain {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression {} drains an ABI runtime context",
                        owner.0, expression_id.0,
                    )));
                }
                Ok(CheckedExpressionKind::ExternalRead {
                    canonical_path: lexical_payload_path(payload).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} runtime-context expression {} has no lexical path",
                            owner.0, expression_id.0,
                        ))
                    })?,
                    external_identity: None,
                })
            }
            KernelLexicalBindingTargetRef::Value { provider } => {
                Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} expression {} has an unanchored lexical value provider {provider:?}",
                    owner.0, expression_id.0,
                )))
            }
        };
    }
    if matches!(payload, crate::KernelExpressionSemanticPayload::Delimiter) {
        return Ok(CheckedExpressionKind::Delimiter);
    }
    if let crate::KernelExpressionSemanticPayload::Invalid(tokens) = payload
        && matches!(
            expression.kind,
            crate::KernelOwnerNodeKind::Number
                | crate::KernelOwnerNodeKind::Byte
                | crate::KernelOwnerNodeKind::Bits(_)
        )
    {
        return Ok(CheckedExpressionKind::Invalid {
            tokens: tokens.iter().map(|token| token.to_string()).collect(),
        });
    }

    let inputs = |role: &crate::KernelOwnerEdgeRole| {
        expression
            .inputs
            .iter()
            .filter(|input| &input.role == role)
            .map(|input| {
                definition
                    .resolve_value(input.expression, expression_id.0 as usize)
                    .map_err(|error| KernelCheckedLinkError::new(error.to_string()))
                    .and_then(|value| layout.expression(owner, value))
            })
            .collect::<Result<Vec<_>, _>>()
    };
    let one_input = |role: &crate::KernelOwnerEdgeRole, label: &str| {
        let values = inputs(role)?;
        let [value] = values.as_slice() else {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} expression {} has {} {label} inputs",
                owner.0,
                expression_id.0,
                values.len(),
            )));
        };
        Ok(*value)
    };

    Ok(match &expression.kind {
        crate::KernelOwnerNodeKind::Source(_) => CheckedExpressionKind::Source,
        crate::KernelOwnerNodeKind::Absent => CheckedExpressionKind::Absent,
        crate::KernelOwnerNodeKind::Text => CheckedExpressionKind::Text {
            value: match payload {
                crate::KernelExpressionSemanticPayload::Text(value) => value.to_string(),
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression_id,
                        "text literal",
                    ));
                }
            },
        },
        crate::KernelOwnerNodeKind::TextTemplate => {
            let dynamic = inputs(&crate::KernelOwnerEdgeRole::TextDynamic)?;
            let crate::KernelExpressionSemanticPayload::TextTemplate(segments) = payload else {
                return Err(expression_payload_error(
                    owner,
                    expression_id,
                    "text template",
                ));
            };
            CheckedExpressionKind::TextTemplate {
                segments: segments
                    .iter()
                    .map(|segment| match segment {
                        crate::KernelTextTemplateSegment::Static(value) => {
                            Ok(CheckedTextSegment::Static {
                                value: value.to_string(),
                            })
                        }
                        crate::KernelTextTemplateSegment::Dynamic(ordinal) => dynamic
                            .get(*ordinal as usize)
                            .copied()
                            .map(|value| CheckedTextSegment::Dynamic { value })
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel definition {} text template {} references missing dynamic segment {}",
                                    owner.0, expression_id.0, ordinal,
                                ))
                            }),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            }
        }
        crate::KernelOwnerNodeKind::Number => CheckedExpressionKind::Number {
            value: match payload {
                crate::KernelExpressionSemanticPayload::Number(value) => value.clone(),
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression_id,
                        "number literal",
                    ));
                }
            },
        },
        crate::KernelOwnerNodeKind::Byte => CheckedExpressionKind::BytesByte {
            value: match payload {
                crate::KernelExpressionSemanticPayload::Byte(value) => *value,
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression_id,
                        "byte literal",
                    ));
                }
            },
        },
        crate::KernelOwnerNodeKind::Bits(_) => CheckedExpressionKind::Bits {
            value: match payload {
                crate::KernelExpressionSemanticPayload::Bits(value) => value.clone(),
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression_id,
                        "bits literal",
                    ));
                }
            },
        },
        crate::KernelOwnerNodeKind::Tag(name) => CheckedExpressionKind::Tag {
            name: name.to_string(),
        },
        crate::KernelOwnerNodeKind::Record { tag } => {
            let Some(crate::KernelExecutionShapeInput::Record { fields, .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} record expression {} has no exact execution shape",
                    owner.0, expression_id.0,
                )));
            };
            let fields = checked_record_fields(
                layout,
                owner,
                definition,
                expression_id,
                container_line,
                container_declaration,
                fields,
            )?;
            match tag {
                Some(tag) => CheckedExpressionKind::TaggedObject {
                    tag: tag.to_string(),
                    fields,
                },
                None => CheckedExpressionKind::Object { fields },
            }
        }
        crate::KernelOwnerNodeKind::Block => {
            let Some(crate::KernelExecutionShapeInput::Block {
                bindings, result, ..
            }) = shape
            else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} BLOCK expression {} has no exact execution shape",
                    owner.0, expression_id.0,
                )));
            };
            CheckedExpressionKind::Block {
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        Ok(CheckedBlockBinding {
                            declaration: layout.declaration(
                                owner,
                                definition
                                    .resolve_structural_declaration(
                                        binding.declaration,
                                        binding.value,
                                        expression_id.0 as usize,
                                    )
                                    .map_err(|error| {
                                        KernelCheckedLinkError::new(error.to_string())
                                    })?,
                            )?,
                            value: layout.expression(
                                owner,
                                definition
                                    .resolve_value(binding.value, expression_id.0 as usize)
                                    .map_err(|error| {
                                        KernelCheckedLinkError::new(error.to_string())
                                    })?,
                            )?,
                            span: checked_nested_span(container_line, binding.span),
                        })
                    })
                    .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?,
                result: result
                    .map(|result| {
                        definition
                            .resolve_value(result, expression_id.0 as usize)
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))
                            .and_then(|value| layout.expression(owner, value))
                    })
                    .transpose()?,
            }
        }
        crate::KernelOwnerNodeKind::Collection { kind, capacity } => match kind {
            crate::KernelCollectionKind::List => CheckedExpressionKind::List {
                capacity: *capacity,
                items: inputs(&crate::KernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Bytes => CheckedExpressionKind::Bytes {
                fixed_size: *capacity,
                items: inputs(&crate::KernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Set => CheckedExpressionKind::Set {
                items: inputs(&crate::KernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Map => CheckedExpressionKind::Map {
                entries: inputs(&crate::KernelOwnerEdgeRole::MapEntry)?,
            },
        },
        crate::KernelOwnerNodeKind::MapEntry => CheckedExpressionKind::MapEntry {
            key: one_input(&crate::KernelOwnerEdgeRole::MapKey, "map-key")?,
            value: one_input(&crate::KernelOwnerEdgeRole::MapValue, "map-value")?,
        },
        crate::KernelOwnerNodeKind::Latest => CheckedExpressionKind::Latest {
            branches: inputs(&crate::KernelOwnerEdgeRole::LatestBranch)?,
        },
        crate::KernelOwnerNodeKind::When => {
            let Some(crate::KernelExecutionShapeInput::Conditional { kind, .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} conditional expression {} has no exact execution shape",
                    owner.0, expression_id.0,
                )));
            };
            let input = one_input(
                &crate::KernelOwnerEdgeRole::WhenInput,
                "conditional selector",
            )?;
            let arms = inputs(&crate::KernelOwnerEdgeRole::WhenArm)?;
            match kind {
                crate::KernelConditionalKind::When => CheckedExpressionKind::When { input, arms },
                crate::KernelConditionalKind::While => CheckedExpressionKind::While { input, arms },
            }
        }
        crate::KernelOwnerNodeKind::Then => CheckedExpressionKind::Then {
            input: one_input(&crate::KernelOwnerEdgeRole::ThenInput, "THEN input")?,
            output: inputs(&crate::KernelOwnerEdgeRole::ThenOutput)?
                .into_iter()
                .next(),
        },
        crate::KernelOwnerNodeKind::Infix { operation } => CheckedExpressionKind::Infix {
            left: one_input(&crate::KernelOwnerEdgeRole::InfixLeft, "infix-left")?,
            op: operation.to_string(),
            right: one_input(&crate::KernelOwnerEdgeRole::InfixRight, "infix-right")?,
        },
        crate::KernelOwnerNodeKind::Draining => CheckedExpressionKind::Draining {
            input: one_input(&crate::KernelOwnerEdgeRole::DrainingInput, "DRAINING")?,
        },
        crate::KernelOwnerNodeKind::Hold => CheckedExpressionKind::Hold {
            initial: one_input(&crate::KernelOwnerEdgeRole::HoldInitial, "HOLD initial")?,
            name: match payload {
                crate::KernelExpressionSemanticPayload::HoldName(name) => name.to_string(),
                _ => return Err(expression_payload_error(owner, expression_id, "HOLD name")),
            },
        },
        crate::KernelOwnerNodeKind::MatchArm { .. } => {
            let Some(crate::KernelExecutionShapeInput::MatchArm { bindings, .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} match arm {} has no exact execution shape",
                    owner.0, expression_id.0,
                )));
            };
            CheckedExpressionKind::MatchArm {
                pattern: checked_match_pattern(payload).ok_or_else(|| {
                    expression_payload_error(owner, expression_id, "match pattern")
                })?,
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        layout.declaration(owner, KernelDeclarationReference::Local(*binding))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                output: inputs(&crate::KernelOwnerEdgeRole::MatchOutput)?
                    .into_iter()
                    .next(),
            }
        }
        crate::KernelOwnerNodeKind::Arrow => CheckedExpressionKind::Invalid {
            tokens: vec!["unconsumed_arrow".to_owned()],
        },
        crate::KernelOwnerNodeKind::Flush => CheckedExpressionKind::Flush {
            payload: one_input(&crate::KernelOwnerEdgeRole::FlushPayload, "FLUSH payload")?,
        },
        crate::KernelOwnerNodeKind::Delimiter => CheckedExpressionKind::Delimiter,
        crate::KernelOwnerNodeKind::Unknown => CheckedExpressionKind::Invalid {
            tokens: match payload {
                crate::KernelExpressionSemanticPayload::Invalid(tokens) => {
                    tokens.iter().map(|token| token.to_string()).collect()
                }
                crate::KernelExpressionSemanticPayload::LexicalPath(path) => vec![
                    "unresolved_value".to_owned(),
                    path.iter()
                        .map(|part| part.as_ref())
                        .collect::<Vec<_>>()
                        .join("/"),
                ],
                _ => vec!["unknown_expression".to_owned()],
            },
        },
        crate::KernelOwnerNodeKind::Known(_)
        | crate::KernelOwnerNodeKind::FormalRead { .. }
        | crate::KernelOwnerNodeKind::ContextRead { .. }
        | crate::KernelOwnerNodeKind::LexicalRead { .. }
        | crate::KernelOwnerNodeKind::ValueRead { .. }
        | crate::KernelOwnerNodeKind::DerivedRead { .. }
        | crate::KernelOwnerNodeKind::PatternRead { .. }
        | crate::KernelOwnerNodeKind::CollectionItemRead
        | crate::KernelOwnerNodeKind::FreshOut => {
            let stable = facts.relocations.expressions.get(expression_id.0 as usize);
            let span = facts
                .presentation
                .expressions
                .get(expression_id.0 as usize)
                .map(|presentation| presentation.span);
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} read expression {} ({stable:?}, span {span:?}) has no lexical authority",
                owner.0, expression_id.0,
            )));
        }
        crate::KernelOwnerNodeKind::UserCall { .. }
        | crate::KernelOwnerNodeKind::FieldProjection { .. }
        | crate::KernelOwnerNodeKind::RenderConstructor { .. }
        | crate::KernelOwnerNodeKind::PureBuiltin { .. }
        | crate::KernelOwnerNodeKind::FixedAbiCall { .. }
        | crate::KernelOwnerNodeKind::HostEffect { .. } => {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} call expression {} has no call artifact",
                owner.0, expression_id.0,
            )));
        }
    })
}

fn checked_record_fields(
    layout: &KernelCheckedLinkLayout,
    owner: KernelOwnerId,
    definition: KernelDefinitionRef<'_>,
    expression: crate::KernelExpressionId,
    container_line: usize,
    container_declaration: Option<DeclId>,
    fields: &[crate::KernelExecutionRecordFieldInput],
) -> Result<Vec<CheckedRecordField>, KernelCheckedLinkError> {
    fields
        .iter()
        .map(|field| {
            Ok(CheckedRecordField {
                declaration: field
                    .declaration
                    .map(|declaration| {
                        definition
                            .resolve_structural_declaration(
                                declaration,
                                field.value,
                                expression.0 as usize,
                            )
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))
                            .and_then(|declaration| layout.declaration(owner, declaration))
                    })
                    .transpose()?
                    .or(container_declaration),
                name: field.name.to_string(),
                value: layout.expression(
                    owner,
                    definition
                        .resolve_value(field.value, expression.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?,
                )?,
                spread: field.spread,
                span: checked_nested_span(container_line, field.span),
            })
        })
        .collect()
}

fn checked_nested_span(line: usize, span: crate::KernelSourceSpan) -> CheckedSpan {
    CheckedSpan {
        line,
        start: span.start,
        end: span.end,
    }
}

fn checked_match_pattern(
    payload: &crate::KernelExpressionSemanticPayload,
) -> Option<CheckedMatchPattern> {
    let crate::KernelExpressionSemanticPayload::MatchPattern(pattern) = payload else {
        return None;
    };
    Some(match pattern {
        crate::KernelMatchPatternPayload::Wildcard => CheckedMatchPattern::Wildcard,
        crate::KernelMatchPatternPayload::Number(value) => CheckedMatchPattern::Number {
            value: value.clone(),
        },
        crate::KernelMatchPatternPayload::Text(value) => CheckedMatchPattern::Text {
            value: value.to_string(),
        },
        crate::KernelMatchPatternPayload::Tag { name, fields } => CheckedMatchPattern::Tag {
            name: name.to_string(),
            fields: fields.iter().map(|field| field.to_string()).collect(),
        },
        crate::KernelMatchPatternPayload::Binding(name) => CheckedMatchPattern::Binding {
            name: name.to_string(),
        },
        crate::KernelMatchPatternPayload::Bits(value) => CheckedMatchPattern::Bits {
            value: value.clone(),
        },
        crate::KernelMatchPatternPayload::Invalid => return None,
    })
}

fn checked_projection_symbols_to_expression_with_scratch(
    text: &boon_contract::ProjectTextSnapshot,
    expressions: &[CheckedExpression],
    calls: &[CheckedCall],
    current: CheckedExprId,
    target: CheckedExprId,
    visiting: &mut [bool],
    projection: &mut Vec<SymbolId>,
) -> Result<bool, KernelCheckedLinkError> {
    if current == target {
        return Ok(true);
    }
    let Some(active) = visiting.get_mut(current.0 as usize) else {
        return Ok(false);
    };
    if *active {
        return Ok(false);
    }
    *active = true;
    let result = (|| {
        let Some(expression) = expressions
            .get(current.0 as usize)
            .filter(|expression| expression.id == current)
        else {
            return Ok(false);
        };
        macro_rules! reaches {
            ($child:expr) => {
                checked_projection_symbols_to_expression_with_scratch(
                    text,
                    expressions,
                    calls,
                    $child,
                    target,
                    visiting,
                    projection,
                )?
            };
        }
        Ok(match &expression.kind {
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields } => {
                let mut found = false;
                for field in fields {
                    let symbol = text.lookup_symbol(&field.name).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "checked object field `{}` is absent from the project text authority",
                            field.name,
                        ))
                    })?;
                    projection.push(symbol);
                    if reaches!(field.value) {
                        found = true;
                        break;
                    }
                    projection.pop();
                }
                found
            }
            CheckedExpressionKind::Call { call } => {
                let Some(call) = calls
                    .get(call.0 as usize)
                    .filter(|candidate| candidate.id == *call)
                else {
                    return Ok(false);
                };
                let mut found = false;
                for entry in &call.entries {
                    if let CheckedCallEntry::Input { value, .. } = entry
                        && reaches!(*value)
                    {
                        found = true;
                        break;
                    }
                }
                found
            }
            CheckedExpressionKind::Draining { input }
            | CheckedExpressionKind::Hold { initial: input, .. } => reaches!(*input),
            CheckedExpressionKind::Flush { payload } => reaches!(*payload),
            CheckedExpressionKind::When { input, arms }
            | CheckedExpressionKind::While { input, arms } => {
                if reaches!(*input) {
                    true
                } else {
                    let mut found = false;
                    for arm in arms {
                        if reaches!(*arm) {
                            found = true;
                            break;
                        }
                    }
                    found
                }
            }
            CheckedExpressionKind::Then { input, output } => {
                if reaches!(*input) {
                    true
                } else if let Some(output) = output {
                    reaches!(*output)
                } else {
                    false
                }
            }
            CheckedExpressionKind::Infix { left, right, .. } => reaches!(*left) || reaches!(*right),
            CheckedExpressionKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    reaches!(*output)
                } else {
                    false
                }
            }
            CheckedExpressionKind::Block { bindings, result } => {
                let mut found = false;
                for binding in bindings {
                    if reaches!(binding.value) {
                        found = true;
                        break;
                    }
                }
                if found {
                    true
                } else if let Some(result) = result {
                    reaches!(*result)
                } else {
                    false
                }
            }
            CheckedExpressionKind::List { items, .. }
            | CheckedExpressionKind::Bytes { items, .. }
            | CheckedExpressionKind::Set { items }
            | CheckedExpressionKind::Latest { branches: items } => {
                let mut found = false;
                for item in items {
                    if reaches!(*item) {
                        found = true;
                        break;
                    }
                }
                found
            }
            CheckedExpressionKind::Map { entries } => {
                let mut found = false;
                for entry in entries {
                    if reaches!(*entry) {
                        found = true;
                        break;
                    }
                }
                found
            }
            CheckedExpressionKind::MapEntry { key, value } => reaches!(*key) || reaches!(*value),
            CheckedExpressionKind::TextTemplate { segments } => {
                let mut found = false;
                for segment in segments {
                    if let CheckedTextSegment::Dynamic { value } = segment
                        && reaches!(*value)
                    {
                        found = true;
                        break;
                    }
                }
                found
            }
            CheckedExpressionKind::Read { .. }
            | CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::Bits { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Absent
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => false,
        })
    })();
    visiting[current.0 as usize] = false;
    result
}

fn lexical_payload_path(payload: &crate::KernelExpressionSemanticPayload) -> Option<String> {
    let crate::KernelExpressionSemanticPayload::LexicalPath(path) = payload else {
        return None;
    };
    Some(
        path.iter()
            .map(|part| part.as_ref())
            .collect::<Vec<_>>()
            .join("."),
    )
}

fn canonical_checked_source_read(
    source_paths: &BTreeMap<DeclId, Vec<(Vec<String>, CheckedSourceId)>>,
    target: DeclId,
    projection: Vec<String>,
) -> (DeclId, Vec<String>, Option<CheckedSourceRead>) {
    let Some(candidates) = source_paths.get(&target) else {
        return (target, projection, None);
    };
    let mut matches = candidates.iter().filter_map(|(path, source)| {
        projection
            .strip_prefix(path.as_slice())
            .map(|payload| (*source, path.len(), payload))
    });
    let Some((source, path_len, payload)) = matches.next() else {
        return (target, projection, None);
    };
    if matches
        .next()
        .is_some_and(|(_, candidate_len, _)| candidate_len == path_len)
    {
        return (target, projection, None);
    }
    let payload_projection = payload
        .strip_prefix(&["event".to_owned()])
        .or_else(|| payload.strip_prefix(&["events".to_owned()]))
        .unwrap_or(payload)
        .to_vec();
    (
        target,
        projection,
        Some(CheckedSourceRead {
            source,
            payload_projection,
        }),
    )
}

fn expression_payload_error(
    owner: KernelOwnerId,
    expression: crate::KernelExpressionId,
    expected: &str,
) -> KernelCheckedLinkError {
    KernelCheckedLinkError::new(format!(
        "kernel definition {} expression {} has no exact {expected} payload",
        owner.0, expression.0,
    ))
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
    declaration: &crate::KernelDeclarationInput,
    cache: &mut crate::DefinitionTypeMaterializationCache,
) -> Result<FlowType, KernelCheckedLinkError> {
    let definition = snapshot.definition(owner).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel checked declaration flow references missing definition {}",
            owner.0,
        ))
    })?;
    let code = definition.code();
    let mut materializer =
        code.linked_materializer(cache, layout.definition(owner)?.type_variables.start);
    if let Some(flow_type) = materializer.materialize_declaration_flow(declaration.id.0 as usize) {
        return Ok(flow_type);
    }
    if declaration.kind == crate::KernelDeclarationKind::Function {
        if definition.linkage().public_declaration
            != Some(KernelDeclarationReference::Local(declaration.id))
        {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} function declaration {} is not its public authority",
                owner.0, declaration.id.0,
            )));
        }
        let mut arguments = definition
            .facts()
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
                materializer
                    .materialize_formal(ordinal as usize)
                    .map(|formal| formal.ty)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} function value parameter {ordinal} has no solved formal",
                            owner.0,
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Function {
                args: arguments,
                result: Box::new(materializer.materialize_result()),
            },
        });
    }
    if definition.linkage().public_declaration
        == Some(KernelDeclarationReference::Local(declaration.id))
    {
        return Ok(materializer.materialize_result());
    }
    drop(materializer);
    match declaration.origin {
        crate::KernelDeclarationOrigin::Parameter { ordinal, .. } => code
            .linked_materializer(cache, layout.definition(owner)?.type_variables.start)
            .materialize_formal(ordinal as usize)
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
                cache,
            )
        }
        crate::KernelDeclarationOrigin::CallbackBinding { call, ordinal } => fresh_out_flow_type(
            layout,
            snapshot,
            owner,
            declaration.name.as_ref(),
            call,
            ordinal,
            cache,
        ),
        crate::KernelDeclarationOrigin::CallContext { .. } => {
            Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} call-context declaration {} has no declared flow type",
                owner.0, declaration.id.0,
            )))
        }
        crate::KernelDeclarationOrigin::Statement { .. }
        | crate::KernelDeclarationOrigin::RecordField { .. } => {
            let value = declaration.value.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} declaration {} has no value authority",
                    owner.0, declaration.id.0,
                ))
            })?;
            let value = definition
                .resolve_value(value, declaration.id.0 as usize)
                .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
            relocated_value_flow_type(layout, snapshot, owner, value, cache)
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
    cache: &mut crate::DefinitionTypeMaterializationCache,
) -> Result<FlowType, KernelCheckedLinkError> {
    let definition = snapshot
        .definition(owner)
        .ok_or_else(|| KernelCheckedLinkError::new("FreshOut definition is missing"))?;
    let mut calls = definition
        .facts()
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
    let provider = definition
        .resolve_value(provider, call.0 as usize)
        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
    let KernelValueReference::Local(provider) = provider else {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} FreshOut formal {ordinal} `{declaration_name}` has an external bare-OUT provider",
            owner.0,
        )));
    };
    let expression = definition
        .input()
        .nodes
        .get(provider.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("FreshOut provider expression is missing"))?;
    if !matches!(expression.kind, crate::KernelOwnerNodeKind::FreshOut) {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel definition {} FreshOut formal {ordinal} `{declaration_name}` provider {} has kind {:?}",
            owner.0, provider.0, expression.kind,
        )));
    }
    let code = snapshot
        .definition_code
        .definition(owner)
        .ok_or_else(|| KernelCheckedLinkError::new("FreshOut packed definition is missing"))?;
    code.linked_materializer(cache, layout.definition(owner)?.type_variables.start)
        .materialize_expression(provider.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("FreshOut packed flow is missing"))
}

fn pattern_binding_flow_type(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    arm: crate::KernelExpressionId,
    ordinal: u32,
    declaration_name: &str,
    cache: &mut crate::DefinitionTypeMaterializationCache,
) -> Result<FlowType, KernelCheckedLinkError> {
    let definition = snapshot
        .definition(owner)
        .ok_or_else(|| KernelCheckedLinkError::new("pattern-binding definition is missing"))?;
    let arm_expression = definition
        .input()
        .nodes
        .get(arm.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("pattern-binding match arm is missing"))?;
    let crate::KernelOwnerNodeKind::MatchArm { pattern } = &arm_expression.kind else {
        return Err(KernelCheckedLinkError::new(
            "pattern-binding declaration does not name a match arm",
        ));
    };
    let mut shapes = definition
        .facts()
        .execution_shapes
        .iter()
        .filter_map(|shape| match shape {
            crate::KernelExecutionShapeInput::MatchArm {
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
    let selector = definition
        .resolve_value(selector, arm.0 as usize)
        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
    let (_, selector) = value_flow_authority_cached(layout, snapshot, owner, selector, cache)?;
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
    Ok(FlowType {
        mode: FlowMode::Continuous,
        ty,
    })
}

fn relocated_value_flow_type(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    value: KernelValueReference,
    cache: &mut crate::DefinitionTypeMaterializationCache,
) -> Result<FlowType, KernelCheckedLinkError> {
    value_flow_authority_cached(layout, snapshot, owner, value, cache).map(|(_, flow)| flow)
}

fn value_flow_authority_cached(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    owner: KernelOwnerId,
    value: KernelValueReference,
    cache: &mut crate::DefinitionTypeMaterializationCache,
) -> Result<(KernelOwnerId, FlowType), KernelCheckedLinkError> {
    let (authority, expression) = match value {
        KernelValueReference::Local(expression) => (owner, Some(expression)),
        KernelValueReference::External(external) => match external.target {
            KernelExternalTarget::Expression(expression) => (external.owner, Some(expression)),
            KernelExternalTarget::Result => (external.owner, None),
        },
    };
    let code = snapshot
        .definition_code
        .definition(authority)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel value references missing definition {}",
                authority.0,
            ))
        })?;
    let mut materializer =
        code.linked_materializer(cache, layout.definition(authority)?.type_variables.start);
    let flow = match expression {
        Some(expression) => materializer
            .materialize_expression(expression.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} value references missing expression {}",
                    authority.0, expression.0,
                ))
            })?,
        None => materializer.materialize_result(),
    };
    Ok((authority, flow))
}

fn callable_type_parameter_variables<'a>(
    formals: impl IntoIterator<Item = &'a FlowType>,
    result: &FlowType,
) -> Vec<TypeVar> {
    let mut parameters = Vec::new();
    for formal in formals {
        collect_callable_type_parameter_variables(&formal.ty, &mut parameters);
    }
    collect_callable_type_parameter_variables(&result.ty, &mut parameters);
    parameters
}

fn collect_callable_type_parameter_variables(ty: &Type, parameters: &mut Vec<TypeVar>) {
    match ty {
        Type::Var(variable) => {
            if !parameters.contains(variable) {
                u32::try_from(parameters.len())
                    .expect("kernel checked callable type-parameter count exceeds u32");
                parameters.push(*variable);
            }
        }
        Type::Object(shape) => {
            for field in shape.ordered_fields().into_iter().map(|(_, field)| field) {
                collect_callable_type_parameter_variables(field, parameters);
            }
        }
        Type::List(item) | Type::Set(item) => {
            collect_callable_type_parameter_variables(item, parameters);
        }
        Type::Map { key, value } => {
            collect_callable_type_parameter_variables(key, parameters);
            collect_callable_type_parameter_variables(value, parameters);
        }
        Type::Function { args, result } => {
            for argument in args {
                collect_callable_type_parameter_variables(argument, parameters);
            }
            collect_callable_type_parameter_variables(&result.ty, parameters);
        }
        Type::VariantSet(variants) => {
            for variant in variants {
                if let Variant::Tagged { fields, .. } = variant {
                    for field in fields.ordered_fields().into_iter().map(|(_, field)| field) {
                        collect_callable_type_parameter_variables(field, parameters);
                    }
                }
            }
        }
        Type::Union(members) => {
            for member in members {
                collect_callable_type_parameter_variables(member, parameters);
            }
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => {}
    }
}

fn type_variables_in_flow(flow: &FlowType) -> BTreeSet<TypeVar> {
    let mut variables = BTreeSet::new();
    collect_flow_type_variables(flow, &mut variables);
    variables
}

fn referenced_abi_callable_names(
    snapshot: &KernelCheckedSnapshot,
) -> Result<BTreeSet<String>, KernelCheckedLinkError> {
    let mut names = BTreeSet::new();
    for definition in snapshot.definition_refs() {
        let owner = definition.owner().0;
        let mut syntax_by_expression = BTreeMap::new();
        for syntax in &definition.facts().call_syntax {
            if syntax_by_expression
                .insert(syntax.expression, syntax.function.as_ref())
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner} repeats call syntax for expression {}",
                    syntax.expression.0,
                )));
            }
        }
        for ordinal in 0..definition.call_count() {
            let call = definition.call(ordinal).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {owner} omits packed call {ordinal}",
                ))
            })?;
            if matches!(call.target(), KernelCallTargetRef::User { .. }) {
                continue;
            }
            let function = syntax_by_expression
                .get(&call.expression())
                .copied()
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {owner} ABI call expression {} has no authored call identity",
                        call.expression().0,
                    ))
                })?;
            if let KernelCallTargetRef::HostEffect { operation } = call.target()
                && operation != function
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner} host call expression {} names `{function}` but targets `{operation}`",
                    call.expression().0,
                )));
            }
            if let KernelCallTargetRef::FieldProjection { field } = call.target()
                && function.strip_prefix("Field/") != Some(field)
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner} field projection call {} names `{function}` instead of `Field/{field}`",
                    call.expression().0,
                )));
            }
            names.insert(function.to_owned());
        }
    }
    Ok(names)
}

fn abi_callable_type_variables(callable: &crate::KernelCallableAbiInput) -> BTreeSet<TypeVar> {
    let mut variables = BTreeSet::new();
    for parameter in &callable.parameters {
        collect_flow_type_variables(&parameter.flow_type, &mut variables);
    }
    for context in &callable.contexts {
        collect_flow_type_variables(&context.flow_type, &mut variables);
    }
    collect_flow_type_variables(&callable.result, &mut variables);
    variables
}

fn relocate_abi_flow_type(
    layout: &KernelCheckedAbiCallableLayout,
    flow_type: &FlowType,
) -> Result<FlowType, KernelCheckedLinkError> {
    Ok(FlowType {
        mode: flow_type.mode,
        ty: relocate_type(layout.type_variables, &flow_type.ty)?,
    })
}

fn checked_abi_contextual_operation(
    layout: &KernelCheckedAbiCallableLayout,
    operation: KernelAbiContextualOperation,
) -> Result<CheckedContextualOperation, KernelCheckedLinkError> {
    let parameter = |ordinal: u32, role: &str| {
        layout
            .parameters
            .get(ordinal as usize)
            .copied()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel ABI callable `{}` contextual {role} references missing parameter ordinal {ordinal}",
                    layout.name,
                ))
            })
    };
    Ok(match operation {
        KernelAbiContextualOperation::Map { list, row, body } => CheckedContextualOperation::Map {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            body: parameter(body, "body")?,
        },
        KernelAbiContextualOperation::Filter {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Filter {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Retain {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Retain {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Remove {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Remove {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Every {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Every {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Any {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Any {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::Find {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Find {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            predicate: parameter(predicate, "predicate")?,
        },
        KernelAbiContextualOperation::SortBy {
            list,
            row,
            key,
            direction,
        } => CheckedContextualOperation::SortBy {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            key: parameter(key, "key")?,
            direction: parameter(direction, "direction")?,
        },
        KernelAbiContextualOperation::ThenBy {
            list,
            row,
            key,
            direction,
        } => CheckedContextualOperation::ThenBy {
            list: parameter(list, "list")?,
            row: parameter(row, "row")?,
            key: parameter(key, "key")?,
            direction: parameter(direction, "direction")?,
        },
    })
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
    Ok(relocate_type_inner(type_variables, ty)?.unwrap_or_else(|| ty.clone()))
}

/// Relocate only branches that contain a local type variable. Closed rich
/// views are compatibility projections over immutable shared nodes and must
/// not be rebuilt merely to cross the checked linker.
fn relocate_type_inner(
    type_variables: KernelCheckedRowRange,
    ty: &Type,
) -> Result<Option<Type>, KernelCheckedLinkError> {
    Ok(match ty {
        Type::Var(variable) => {
            let relocated = TypeVar(type_variables.resolve(variable.0, "type variable")?);
            (relocated != *variable).then_some(Type::Var(relocated))
        }
        Type::VariantSet(variants) => {
            let mut relocated_variants = None;
            for (index, variant) in variants.iter().enumerate() {
                let Variant::Tagged { tag, fields } = variant else {
                    continue;
                };
                if let Some(fields) = relocate_object_shape_inner(type_variables, fields)? {
                    relocated_variants
                        .get_or_insert_with(|| variants.iter().cloned().collect::<Vec<_>>())
                        [index] = Variant::Tagged {
                        tag: tag.clone(),
                        fields: fields.into(),
                    };
                }
            }
            relocated_variants.map(|variants| Type::VariantSet(variants.into()))
        }
        Type::Object(shape) => relocate_object_shape_inner(type_variables, shape)?
            .map(|shape| Type::Object(shape.into())),
        Type::List(item) => {
            relocate_type_inner(type_variables, item)?.map(|item| Type::List(Type::shared(item)))
        }
        Type::Set(item) => {
            relocate_type_inner(type_variables, item)?.map(|item| Type::Set(Type::shared(item)))
        }
        Type::Function { args, result } => {
            let mut relocated_args = None;
            for (index, argument) in args.iter().enumerate() {
                if let Some(argument) = relocate_type_inner(type_variables, argument)? {
                    relocated_args.get_or_insert_with(|| args.clone())[index] = argument;
                }
            }
            let relocated_result = relocate_type_inner(type_variables, &result.ty)?;
            (relocated_args.is_some() || relocated_result.is_some()).then(|| Type::Function {
                args: relocated_args.unwrap_or_else(|| args.clone()),
                result: Box::new(FlowType {
                    mode: result.mode,
                    ty: relocated_result.unwrap_or_else(|| result.ty.clone()),
                }),
            })
        }
        Type::Union(members) => {
            let mut relocated_members = None;
            for (index, member) in members.iter().enumerate() {
                if let Some(member) = relocate_type_inner(type_variables, member)? {
                    relocated_members.get_or_insert_with(|| members.clone())[index] = member;
                }
            }
            relocated_members.map(Type::Union)
        }
        Type::Map { key, value } => {
            let relocated_key = relocate_type_inner(type_variables, key)?;
            let relocated_value = relocate_type_inner(type_variables, value)?;
            (relocated_key.is_some() || relocated_value.is_some()).then(|| Type::Map {
                key: Box::new(relocated_key.unwrap_or_else(|| key.as_ref().clone())),
                value: Box::new(relocated_value.unwrap_or_else(|| value.as_ref().clone())),
            })
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown
        | Type::Bits { .. } => None,
    })
}

fn relocate_object_shape_inner(
    type_variables: KernelCheckedRowRange,
    shape: &SharedObjectShape,
) -> Result<Option<ObjectShape>, KernelCheckedLinkError> {
    let mut relocated_fields = None;
    for (name, ty) in &shape.fields {
        if let Some(ty) = relocate_type_inner(type_variables, ty)? {
            relocated_fields
                .get_or_insert_with(|| shape.fields.clone())
                .insert(name.clone(), ty);
        }
    }
    Ok(relocated_fields.map(|fields| ObjectShape {
        fields,
        field_order: shape.field_order.clone(),
        open: shape.open,
    }))
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
        KernelDeclarationReference::OwnerDeclaration { owner, declaration } => Ok(DeclId(
            definitions
                .get(owner.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked linker references missing public declaration owner {}",
                        owner.0
                    ))
                })?
                .declarations
                .resolve(declaration.0, "delegated public declaration")?,
        )),
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
        KernelExpressionRelocation, KernelExternalExpression, KernelLexicalAccess,
        KernelLexicalBindingInput, KernelLexicalBindingTargetInput, KernelOwnerEdgeRole,
        KernelOwnerInputEdge, KernelOwnerNode, KernelOwnerNodeKind, KernelOwnerProgramInput,
        KernelProjectProgramInput, KernelSession, KernelStatementId, KernelStatementInput,
        KernelStatementKind, KernelStatementValueUse,
    };
    use boon_checked::FlowMode;
    use boon_contract::PackedTextCatalogBuilder;
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

    #[test]
    fn packed_call_result_projection_traversal_has_independent_known_answers() {
        fn expression(id: u32, kind: CheckedExpressionKind) -> CheckedExpression {
            CheckedExpression {
                id: CheckedExprId(id),
                scope_id: LexicalScopeId(0),
                declaration: None,
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Unknown,
                },
                flush_type: None,
                effect: CheckedEffectSummary::default(),
                kind,
                span: CheckedSpan::default(),
            }
        }

        fn field(name: &str, value: u32) -> CheckedRecordField {
            CheckedRecordField {
                declaration: None,
                name: name.to_owned(),
                value: CheckedExprId(value),
                spread: false,
                span: CheckedSpan::default(),
            }
        }

        fn projection(
            text: &boon_contract::ProjectTextSnapshot,
            expressions: &[CheckedExpression],
            root: u32,
            target: u32,
        ) -> Option<Vec<String>> {
            let mut visiting = vec![false; expressions.len()];
            let mut symbols = Vec::new();
            checked_projection_symbols_to_expression_with_scratch(
                text,
                expressions,
                &[],
                CheckedExprId(root),
                CheckedExprId(target),
                &mut visiting,
                &mut symbols,
            )
            .unwrap()
            .then(|| {
                symbols
                    .iter()
                    .map(|symbol| text.symbol(*symbol).unwrap().to_owned())
                    .collect()
            })
        }

        let mut text = PackedTextCatalogBuilder::new();
        for name in ["outer", "inner", "cycle", "alternate", "leaf"] {
            text.intern_symbol(name).unwrap();
        }
        let text = text.freeze();

        let nested = vec![
            expression(
                0,
                CheckedExpressionKind::Object {
                    fields: vec![field("outer", 1)],
                },
            ),
            expression(
                1,
                CheckedExpressionKind::Object {
                    fields: vec![field("inner", 2)],
                },
            ),
            expression(
                2,
                CheckedExpressionKind::Number {
                    value: boon_data::ExactNumber::from_u64(1),
                },
            ),
        ];
        assert_eq!(
            projection(&text, &nested, 0, 2),
            Some(vec!["outer".to_owned(), "inner".to_owned()]),
        );
        assert_eq!(
            projection(&text, &nested, 2, 2),
            Some(Vec::new()),
            "a function result rooted at its call expression has a present empty path",
        );
        assert_eq!(projection(&text, &nested, 0, 3), None);

        let cyclic = vec![
            expression(
                0,
                CheckedExpressionKind::Object {
                    fields: vec![field("cycle", 0), field("alternate", 1)],
                },
            ),
            expression(
                1,
                CheckedExpressionKind::Object {
                    fields: vec![field("leaf", 2)],
                },
            ),
            expression(
                2,
                CheckedExpressionKind::Number {
                    value: boon_data::ExactNumber::from_u64(2),
                },
            ),
        ];
        assert_eq!(
            projection(&text, &cyclic, 0, 2),
            Some(vec!["alternate".to_owned(), "leaf".to_owned()]),
            "a failed cyclic branch must backtrack before the reachable sibling",
        );
    }

    #[test]
    fn type_relocation_copies_only_changed_branches_and_fails_closed() {
        let closed_list = Type::List(Type::shared(Type::Text));
        let root = Type::object(ObjectShape::from_ordered_fields(
            [
                ("closed".to_owned(), closed_list),
                ("generic".to_owned(), Type::Var(TypeVar(0))),
            ],
            false,
        ));
        let relocated = relocate_type(KernelCheckedRowRange { start: 7, len: 1 }, &root)
            .expect("one dense local variable must relocate");
        let (Type::Object(original), Type::Object(relocated)) = (&root, &relocated) else {
            panic!("test types must remain objects");
        };
        let (Type::List(original_closed), Type::List(relocated_closed)) = (
            original.fields.get("closed").unwrap(),
            relocated.fields.get("closed").unwrap(),
        ) else {
            panic!("closed field must remain a list");
        };
        assert!(boon_checked::SharedType::ptr_eq(
            original_closed,
            relocated_closed
        ));
        assert_eq!(
            relocated.fields.get("generic"),
            Some(&Type::Var(TypeVar(7)))
        );

        let identity = Type::List(Type::shared(Type::Var(TypeVar(0))));
        let relocated_identity =
            relocate_type(KernelCheckedRowRange { start: 0, len: 1 }, &identity).unwrap();
        let (Type::List(original), Type::List(relocated)) = (&identity, &relocated_identity) else {
            panic!("identity test types must remain lists");
        };
        assert!(boon_checked::SharedType::ptr_eq(original, relocated));

        let out_of_range = Type::List(Type::shared(Type::Var(TypeVar(1))));
        let error = relocate_type(KernelCheckedRowRange { start: 4, len: 1 }, &out_of_range)
            .expect_err("a nested non-dense local variable must fail closed");
        assert!(error.to_string().contains("outside local range"));
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
                    declaration_scope: None,
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
                declared_flow_type: None,
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
        let mut provider_facts = facts(&unit, &provider_key, "provider");
        provider_facts.expression_payloads[0] =
            crate::KernelExpressionSemanticPayload::Number(boon_data::ExactNumber::from_u64(1));
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
        let rows = layout
            .materialize_rows(
                &project,
                &snapshot,
                SourceBundleDigestV1::new(
                    "kernel-link-test.bn",
                    [boon_contract::SourceBundleUnit::new(
                        "kernel-link-test.bn",
                        "",
                    )],
                )
                .unwrap(),
                ProgramRole::Client,
                KernelCheckedRowProjectionDemand::EditorRich,
            )
            .expect("one linker call must materialize the complete checked-row surface");
        assert_eq!(rows.scopes.len(), 1);
        assert_eq!(rows.declarations.len(), 2);
        assert_eq!(rows.expressions.len(), 2);
        assert_eq!(rows.statements.len(), 2);
        assert!(rows.callables.is_empty());
        assert!(rows.context_formals.is_empty());
        assert!(rows.calls.is_empty());
        assert!(rows.call_occurrences.is_empty());
        assert!(rows.call_result_paths.is_empty());
        assert!(rows.pattern_bindings.is_empty());
        assert_eq!(rows.semantic_input.resource_projection_count(), 0);
        assert_eq!(rows.occurrences.len(), 3);
        assert_eq!(
            rows.occurrences
                .iter()
                .filter(|occurrence| occurrence.kind == SemanticOccurrenceKind::Declaration)
                .count(),
            2,
        );
        assert_eq!(
            rows.occurrences
                .iter()
                .filter(|occurrence| occurrence.kind == SemanticOccurrenceKind::Read)
                .count(),
            1,
        );
        assert!(rows.sources.is_empty());
        assert!(rows.states.is_empty());
        assert!(rows.lists.is_empty());

        let consumer = snapshot.definition(KernelOwnerId(1)).unwrap();
        let imported = consumer
            .resolve_value(consumer.input().nodes[0].inputs[0].expression, 0)
            .unwrap();
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
        Arc::make_mut(&mut scoped.definition_facts)[0]
            .presentation
            .scopes = vec![crate::KernelScopePresentation {
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
        Arc::make_mut(&mut scoped.definition_facts)[1]
            .presentation
            .containing_scope = KernelScopeReference::Owner {
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
        Arc::make_mut(&mut scoped.definition_facts)[1].statements[0].value_use =
            KernelStatementValueUse::RenderSlot;
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

        let mut missing_scope = scoped.clone();
        Arc::make_mut(&mut missing_scope.definition_facts)[1]
            .presentation
            .containing_scope = KernelScopeReference::Owner {
            owner: KernelOwnerId(0),
            scope: crate::KernelScopeId(99),
        };
        let error = KernelCheckedLinkLayout::new(&project, &missing_scope)
            .expect_err("a missing enclosing scope must fail before row materialization");
        assert!(error.to_string().contains("containing scope"));

        let mut delegated = (*snapshot).clone();
        Arc::make_mut(&mut delegated.definition_facts)[1]
            .linkage
            .public_declaration = Some(KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)));
        let delegated_layout = KernelCheckedLinkLayout::new(&project, &delegated)
            .expect("a nested definition may share its enclosing public declaration");
        assert_eq!(
            delegated_layout.definitions()[1].public_declaration,
            DeclId(1)
        );

        Arc::make_mut(&mut delegated.definition_facts)[0]
            .linkage
            .public_declaration = Some(KernelDeclarationReference::OwnerPublic(KernelOwnerId(1)));
        let error = KernelCheckedLinkLayout::new(&project, &delegated)
            .expect_err("public declaration authority cycles must fail closed");
        assert!(error.to_string().contains("contain a cycle"));

        let mut invalid = (*snapshot).clone();
        Arc::make_mut(&mut invalid.program).owners[1].external_expressions[0].owner =
            KernelOwnerId(99);
        let error = KernelCheckedLinkLayout::new(&project, &invalid)
            .expect_err("an unlinked external owner must fail before row allocation");
        assert!(error.to_string().contains("missing definition 99"));
    }
}
