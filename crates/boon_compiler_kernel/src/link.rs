#[path = "order.rs"]
mod order;
pub use order::*;

#[cfg(test)]
use crate::KernelLexicalBindingTarget;
use crate::definition_code::DefinitionTypeMaterializationCache;
use crate::{
    KernelAbiContextualOperation, KernelCallArgumentKind, KernelCallInputRoleRef,
    KernelCallTargetRef, KernelCheckedSnapshot, KernelDeclarationReference, KernelDefinitionRef,
    KernelExternalTarget, KernelLexicalBindingTargetRef, KernelOwnerId, KernelProjectInput,
    KernelScopeReference, KernelStatementChildReference, KernelStatementReference,
    KernelValueReference,
};
use boon_checked::{
    CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1, CheckedBlockBinding, CheckedCall,
    CheckedCallEntry, CheckedCallId, CheckedCallResultPath, CheckedCallableContext,
    CheckedCallableKind, CheckedCallableSignature, CheckedContextBinding, CheckedContextFormal,
    CheckedContextScheme, CheckedContextTypeSubstitution, CheckedContextualOperation,
    CheckedDeclaration, CheckedDeclarationKind, CheckedDefinitionExecutionNodeV1,
    CheckedDefinitionExecutionTemplateV1, CheckedDefinitionSelectorV1, CheckedEffectSummary,
    CheckedEvaluationScope, CheckedExprId, CheckedExpression, CheckedExpressionKind,
    CheckedExternalDeclarationIdentityV1, CheckedImageKernelExpectedRouteV1,
    CheckedImageKernelOwnershipExpectationV1, CheckedImageKernelPublicationV1,
    CheckedImageRowDomainV2, CheckedList, CheckedListId, CheckedMatchPattern, CheckedParameter,
    CheckedParameterDefault, CheckedParameterKind, CheckedParameterRequirement,
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
use boon_contract::{PathId, SourceBundleDigestV1, SymbolId};
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
    pub callable: crate::KernelAbiCallableId,
    pub declaration: DeclId,
    pub parameters: KernelCheckedRowRange,
    pub type_variables: KernelCheckedRowRange,
}

/// Explicit relocation of one packed callable parameter into the final
/// checked type-variable namespace.
///
/// `parameter` is the target-bound packed traversal ordinal, while
/// `linked_local` is the callable's own alpha coordinate.  Keeping both
/// coordinates prevents compatibility/oracle code from inferring generic
/// identity from the numeric shape of a rich [`TypeVar`] range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelCheckedCallableTypeParameterLayout {
    pub target: crate::KernelCallableSchemeId,
    pub callable: DeclId,
    pub parameter: crate::KernelTypeParameterId,
    pub linked_local: u32,
    pub linked_variable: TypeVar,
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

/// Exact compact entity cardinalities shared by the RuntimePacked checked
/// publication and semantic authority.
///
/// This intentionally contains scalars only. The compiler facade maps it into
/// the typechecker's consuming seal context without reconstructing any checked
/// row family.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelSemanticEntityCountsV1 {
    pub scopes: usize,
    pub declarations: usize,
    pub statements: usize,
    pub expressions: usize,
    pub callables: usize,
    pub context_formals: usize,
    pub calls: usize,
    pub pattern_bindings: usize,
    pub sources: usize,
    pub states: usize,
    pub lists: usize,
    pub occurrences: usize,
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
    /// Explicit editor/oracle projection. RuntimePacked keeps the permanent
    /// packed call columns in `semantic_input` and leaves this rich family
    /// empty.
    pub calls: Box<[CheckedCall]>,
    /// Parser-issued structural identities for the EditorRich call rows.
    /// RuntimePacked publishes the retained authored-site digests directly
    /// and leaves this compatibility sidecar empty.
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

/// Packed-only checked linker product for ordinary runtime compilation.
///
/// This is deliberately a different type from [`KernelCheckedRows`]: owning a
/// runtime linker result proves that no rich scope, declaration, expression,
/// statement, call, source, state, LIST, occurrence, recursive `Type`, or
/// presentation `String` family was materialized along the way. The three
/// retained authorities are consumed by the typechecker and semantic handoff.
#[derive(Debug, Eq, PartialEq)]
pub struct KernelRuntimePackedLinkV1 {
    semantic_input: KernelSemanticInputConstructionV1,
    runtime_flow_terms: CheckedRuntimeFlowTermProjectionV1,
    checked_image_publication: CheckedImageKernelPublicationV1,
}

impl KernelRuntimePackedLinkV1 {
    /// Install one source-unit relocation without projecting rich checked
    /// spans. Packed definition rows retain local spans plus this compact
    /// relocation until semantic consumers ask for a presentation span.
    pub fn rebase_definition_spans(
        &mut self,
        owner: KernelOwnerId,
        start_line: usize,
        start_byte: usize,
    ) -> Result<(), KernelCheckedLinkError> {
        self.semantic_input
            .rebase_definition_spans(owner, start_line, start_byte)
    }

    /// Scope that owns one definition's checked-image authority.
    pub fn definition_authority_root_scope(
        &self,
        owner: KernelOwnerId,
    ) -> Result<LexicalScopeId, KernelCheckedLinkError> {
        self.semantic_input
            .definition_relocation(owner)
            .map(|relocation| relocation.authority_root_scope)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel packed linker references missing definition {}",
                    owner.0,
                ))
            })
    }

    pub fn entity_counts(&self) -> KernelSemanticEntityCountsV1 {
        self.semantic_input.entity_counts()
    }

    pub fn call_count(&self) -> usize {
        self.semantic_input.call_count()
    }

    pub fn into_parts(
        self,
    ) -> (
        KernelSemanticInputConstructionV1,
        CheckedRuntimeFlowTermProjectionV1,
        CheckedImageKernelPublicationV1,
    ) {
        (
            self.semantic_input,
            self.runtime_flow_terms,
            self.checked_image_publication,
        )
    }
}

/// Packed topology shared by the runtime-only linker and the rich editor
/// oracle. All text remains interned; the only owned columns here are dense
/// IDs and compact projection symbols that survive into semantic lowering.
struct KernelPackedLinkTopologyV1 {
    call_result_paths: Box<[KernelSemanticCallResultPathLocatorV1]>,
    call_result_path_symbols: Box<[SymbolId]>,
    pattern_bindings: Box<[KernelSemanticPatternBindingLocatorV1]>,
    resource_projections: Box<[KernelSemanticResourceProjectionLocatorV1]>,
    occurrence_targets: Box<[DeclId]>,
    checked_image_publication: CheckedImageKernelPublicationV1,
    checked_image_ownership_expectation: CheckedImageKernelOwnershipExpectationV1,
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
        if !self.calls.is_empty() {
            if self.calls.len() != layout.totals.calls as usize {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked rich call projection has {} of {} rows",
                    self.calls.len(),
                    layout.totals.calls,
                )));
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
        self.semantic_input
            .rebase_definition_spans(owner, start_line, start_byte)?;
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
    definition_declarations_end: u32,
    totals: KernelCheckedLinkTotals,
}

/// Allocation-free view of one packed call after its definition-local IDs
/// have been assigned final checked coordinates.
///
/// This is intentionally private to the linker. Runtime compilation must not
/// build a second compact call DTO merely to avoid the rich `CheckedCall`
/// projection; it borrows the permanent definition-code columns directly.
#[derive(Clone, Copy)]
struct KernelCheckedPackedCallRef<'a> {
    layout: &'a KernelCheckedLinkLayout,
    snapshot: &'a KernelCheckedSnapshot,
    owner: KernelOwnerId,
    ordinal: u32,
}

impl<'a> KernelCheckedPackedCallRef<'a> {
    fn definition(self) -> Result<KernelDefinitionRef<'a>, KernelCheckedLinkError> {
        self.snapshot.definition(self.owner).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel checked packed call references missing definition {}",
                self.owner.0,
            ))
        })
    }

    fn code(self) -> Result<crate::definition_code::DefinitionCodeRef<'a>, KernelCheckedLinkError> {
        self.snapshot
            .definition_code
            .definition(self.owner)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked packed call references missing code definition {}",
                    self.owner.0,
                ))
            })
    }

    fn id(self) -> Result<CheckedCallId, KernelCheckedLinkError> {
        self.layout.call(self.owner, self.ordinal)
    }

    fn expression(self) -> Result<CheckedExprId, KernelCheckedLinkError> {
        let expression = self
            .code()?
            .call_expression(self.ordinal as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} omits packed call {} expression",
                    self.owner.0, self.ordinal,
                ))
            })?;
        self.layout
            .expression(self.owner, KernelValueReference::Local(expression))
    }

    fn target(self) -> Result<crate::KernelCallableSchemeId, KernelCheckedLinkError> {
        self.code()?
            .call_target(self.ordinal as usize)
            .flatten()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} packed call {} has no retained target",
                    self.owner.0, self.ordinal,
                ))
            })
    }

    fn callable(self) -> Result<DeclId, KernelCheckedLinkError> {
        match self.target()? {
            crate::KernelCallableSchemeId::User(owner) => {
                Ok(self.layout.definition(owner)?.public_declaration)
            }
            crate::KernelCallableSchemeId::Abi(callable) => {
                Ok(self.layout.abi_callable(callable)?.declaration)
            }
        }
    }

    fn owner_callable(self) -> Result<Option<DeclId>, KernelCheckedLinkError> {
        let definition = self.definition()?;
        Ok(definition
            .linkage()
            .root_statement
            .and_then(|root| definition.runtime_facts().statements().get(root.0 as usize))
            .filter(|root| matches!(root.kind, crate::PackedStatementKind::Function { .. }))
            .map(|_| {
                self.layout
                    .definition(self.owner)
                    .expect("validated packed call owner has a layout")
                    .public_declaration
            }))
    }

    fn entries(self) -> Result<&'a [crate::PackedCallEntry], KernelCheckedLinkError> {
        self.code()?
            .call_entries(self.ordinal as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} packed call {} has no entry span",
                    self.owner.0, self.ordinal,
                ))
            })
    }

    fn contexts(self) -> Result<&'a [crate::PackedCallContext], KernelCheckedLinkError> {
        self.code()?
            .call_contexts(self.ordinal as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} packed call {} has no context span",
                    self.owner.0, self.ordinal,
                ))
            })
    }

    fn context_binding(self) -> Result<crate::PackedCallContextBinding, KernelCheckedLinkError> {
        self.code()?
            .call_context_binding(self.ordinal as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} packed call {} has no context binding",
                    self.owner.0, self.ordinal,
                ))
            })
    }

    fn authored_site_digest_v4(self) -> Result<[u8; 32], KernelCheckedLinkError> {
        self.code()?
            .call_authored_site_digest_v4(self.ordinal as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} packed call {} has no authored-site digest",
                    self.owner.0, self.ordinal,
                ))
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelSemanticResourceProjectionLocatorV1 {
    owner: KernelOwnerId,
    ordinal: u32,
    expression: CheckedExprId,
    target: DeclId,
}

/// One pattern binding in the final checked namespace.
///
/// A tag-field projection is at most one segment for this language construct,
/// so the compact row retains an optional interned symbol rather than a
/// per-binding `Vec<String>`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelSemanticPatternBindingLocatorV1 {
    declaration: DeclId,
    selector: CheckedExprId,
    projection: Option<SymbolId>,
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
    authority_root_scope: LexicalScopeId,
    owner_callable: Option<DeclId>,
    context_formal: Option<ContextFormalId>,
    context_formal_ordinal: Option<u32>,
    result_expression: CheckedExprId,
    containing_scope: LexicalScopeId,
    scopes: KernelCheckedRowRange,
    declarations: KernelCheckedRowRange,
    statements: KernelCheckedRowRange,
    type_variables: KernelCheckedRowRange,
    expressions: KernelCheckedRowRange,
    calls: KernelCheckedRowRange,
    sources: KernelCheckedRowRange,
    states: KernelCheckedRowRange,
    lists: KernelCheckedRowRange,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct KernelSemanticDefinitionSpanRelocationV1 {
    /// Zero means the source-unit relocation has not been installed yet.
    start_line: usize,
    start_byte: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelSemanticAbiRelocationV1 {
    callable: DeclId,
    parameters: KernelCheckedRowRange,
    type_variables: KernelCheckedRowRange,
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
    /// The same immutable structural authority that issued every local
    /// expression coordinate retained by `definition_code`. Semantic row views
    /// borrow it directly; they never rebuild rich checked expression rows.
    program: Arc<crate::PackedKernelProjectProgram>,
    definition_code: Arc<crate::DefinitionCodeStore>,
    scope_count: u32,
    expression_count: u32,
    declaration_count: u32,
    statement_count: u32,
    callable_count: u32,
    context_formal_count: u32,
    call_count: u32,
    source_count: u32,
    state_count: u32,
    list_count: u32,
    pattern_binding_count: u32,
    occurrence_count: u32,
    definition_relocations: Box<[KernelSemanticDefinitionRelocationV1]>,
    definition_span_relocations: Box<[KernelSemanticDefinitionSpanRelocationV1]>,
    /// Dense by immutable `KernelAbiCallableId`; unreferenced ABI schemes have
    /// no checked declaration relocation but remain available as packed type
    /// authorities in `definition_code`.
    abi_relocations: Box<[Option<KernelSemanticAbiRelocationV1>]>,
    /// User and referenced ABI schemes in the exact checked-callable order.
    ///
    /// Declaration IDs are sparse and start after the reserved zero sentinel,
    /// so this one flat locator column replaces every downstream
    /// `Vec<Option<callable-index>>` sidecar without indexing by `DeclId`.
    callable_schemes: Box<[crate::KernelCallableSchemeId]>,
    /// Owners with execution templates, ordered by final callable ID.
    definition_execution_owners: Box<[KernelOwnerId]>,
    call_result_paths: Box<[KernelSemanticCallResultPathLocatorV1]>,
    call_result_path_symbols: Box<[SymbolId]>,
    pattern_bindings: Box<[KernelSemanticPatternBindingLocatorV1]>,
    resource_projections: Box<[KernelSemanticResourceProjectionLocatorV1]>,
    resource_projection_by_expression: Box<[u32]>,
    rich_editor_projection_expected: bool,
    checked_image_ownership_expectation: Option<CheckedImageKernelOwnershipExpectationV1>,
    checked_image_pairing: Arc<boon_checked::CheckedImageKernelPairingV1>,
}

/// Checked-image-bound packed input accepted by the kernel semantic route.
///
/// Dense IDs remain revision-local and are valid only together with this
/// exact source/image authority. Public views never expose bare `SymbolId` or
/// `TypeTermId` values.
#[derive(Debug, Eq, PartialEq)]
pub struct KernelSemanticInputV1 {
    construction: KernelSemanticInputConstructionV1,
    checked_image_digest: [u8; 32],
}

/// Borrowed definition-local row authority in one sealed semantic input.
///
/// Every iterator returned from this view walks the existing flat owner/fact
/// columns. The view owns no row, text, path, or recursive type projection.
#[derive(Clone, Copy)]
pub struct KernelSemanticDefinitionRowsRef<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
}

pub struct KernelSemanticDefinitionRowsIter<'a> {
    input: &'a KernelSemanticInputV1,
    next: u32,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticScopeRef<'a> {
    definition: Option<KernelSemanticDefinitionRowsRef<'a>>,
    ordinal: u32,
}

pub struct KernelSemanticScopeIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    next: u32,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticDeclarationRef<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    ordinal: u32,
}

pub struct KernelSemanticDeclarationIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    next: u32,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticStatementRef<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    ordinal: u32,
}

pub struct KernelSemanticStatementIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    next: u32,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticExpressionRef<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    ordinal: u32,
}

pub struct KernelSemanticExpressionIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    next: u32,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticSourceRef<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    ordinal: u32,
}

pub struct KernelSemanticSourceIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    next: u32,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticStateRef<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    ordinal: u32,
}

pub struct KernelSemanticStateIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    next: u32,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticListRef<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    ordinal: u32,
}

pub struct KernelSemanticListIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    next: u32,
}

/// Borrowed authored semantic path. The anchor already uses the final checked
/// declaration namespace; projection spelling remains in the one project text
/// catalog and is yielded as `&str` without a `Vec<String>` projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSemanticPathRef<'a> {
    anchor: DeclId,
    projection: KernelSemanticTextPathRef<'a>,
}

pub struct KernelSemanticPathIter<'a> {
    path: KernelSemanticTextPathRef<'a>,
    next: usize,
}

/// One text-authority-qualified path without a declaration anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSemanticTextPathRef<'a> {
    path: crate::PackedKernelPathRef<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelSemanticStatementKindRef<'a> {
    Function {
        name: &'a str,
    },
    Field {
        name: &'a str,
    },
    Source {
        field: Option<&'a str>,
        event: Option<&'a str>,
    },
    Hold {
        field: Option<&'a str>,
        name: Option<&'a str>,
    },
    List {
        field: Option<&'a str>,
        capacity: Option<u32>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticStatementParameterRef<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    parameter: &'a crate::PackedStatementParameter,
}

pub struct KernelSemanticStatementParameterIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    parameters: std::slice::Iter<'a, crate::PackedStatementParameter>,
}

pub struct KernelSemanticStatementChildIter<'a> {
    definition: KernelSemanticDefinitionRowsRef<'a>,
    children: std::slice::Iter<'a, KernelStatementChildReference>,
}

/// Stable structural operation for one expression. Text-bearing variants
/// borrow spelling from the retained project catalog, and path-bearing variants
/// expose [`KernelSemanticPathRef`] rather than allocating path components.
#[derive(Clone, Copy)]
pub enum KernelSemanticExpressionOperationRef<'a> {
    Known(KernelPackedTypeRef<'a>),
    Source(KernelPackedTypeRef<'a>),
    Absent,
    Text,
    TextTemplate,
    Number,
    Byte,
    Bits(u32),
    Tag(&'a str),
    Record {
        tag: Option<&'a str>,
    },
    Block,
    Collection {
        kind: crate::KernelCollectionKind,
        capacity: Option<u32>,
    },
    MapEntry,
    FormalRead {
        formal: u32,
        fields: KernelSemanticTextPathRef<'a>,
    },
    ContextRead {
        formal: u32,
        fields: KernelSemanticTextPathRef<'a>,
    },
    LexicalRead {
        fields: KernelSemanticTextPathRef<'a>,
    },
    ValueRead {
        fields: KernelSemanticTextPathRef<'a>,
        mode_narrowing: Option<CheckedExprId>,
    },
    DerivedRead {
        fields: KernelSemanticTextPathRef<'a>,
    },
    PatternRead {
        pattern: KernelSemanticPatternRef<'a>,
        fields: KernelSemanticTextPathRef<'a>,
    },
    CollectionItemRead,
    FreshOut,
    UserCall {
        target: KernelOwnerId,
        inherited_formal: Option<crate::KernelInheritedFormal>,
    },
    RenderConstructor(KernelSemanticRenderConstructorRef<'a>),
    PureBuiltin(crate::KernelPureBuiltinKind),
    FixedAbiCall {
        result: KernelPackedTypeRef<'a>,
    },
    HostEffect {
        operation: &'a str,
    },
    Latest,
    When,
    Then,
    Infix {
        operation: &'a str,
    },
    Draining,
    Hold,
    MatchArm {
        pattern: KernelSemanticPatternRef<'a>,
    },
    Arrow,
    Delimiter,
    Unknown,
    Flush,
    FieldProjection {
        field: &'a str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelSemanticPatternRef<'a> {
    Wildcard,
    Number,
    Text,
    Bits {
        width: u32,
    },
    Tag {
        name: &'a str,
        fields: KernelSemanticTextPathRef<'a>,
    },
    Binding {
        name: &'a str,
    },
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelSemanticRenderConstructorRef<'a> {
    Fixed(&'a str),
    StripeDirection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelSemanticExpressionInputRoleRef<'a> {
    RecordField { name: &'a str, spread: bool },
    TextDynamic,
    BlockResult,
    CollectionItem,
    MapEntry,
    MapKey,
    MapValue,
    ReadProvider,
    CallArgument { ordinal: u32 },
    CallOutArgument { ordinal: u32 },
    AbiArgument { name: &'a str },
    LatestBranch,
    WhenInput,
    WhenArm,
    ThenInput,
    ThenOutput,
    InfixLeft,
    InfixRight,
    DrainingInput,
    HoldInitial,
    HoldUpdate,
    MatchOutput,
    ArrowOutput,
    FlushPayload,
}

#[derive(Clone, Copy)]
pub struct KernelSemanticExpressionInputRef<'a> {
    expression: KernelSemanticExpressionRef<'a>,
    edge: &'a crate::PackedKernelOwnerInputEdge,
}

pub struct KernelSemanticExpressionInputIter<'a> {
    expression: KernelSemanticExpressionRef<'a>,
    edges: std::slice::Iter<'a, crate::PackedKernelOwnerInputEdge>,
}

#[derive(Clone, Copy)]
pub enum KernelSemanticStatePathRef<'a> {
    Authored(KernelSemanticPathRef<'a>),
    /// Generated state spelling is intentionally kept as a numeric coordinate;
    /// synthesizing `state_N` as `&str` would require a second owned string.
    Synthetic {
        anchor: DeclId,
        ordinal: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KernelPackedTypeScope {
    Definition(KernelOwnerId),
    Abi(crate::KernelAbiCallableId),
}

/// Scope-qualified borrowed reference to one packed type term.
///
/// A raw `TypeTermId` is never exposed: its store and definition/ABI-local
/// alpha namespace are both part of this value. Structural consumers inspect
/// it without allocation; explicit compatibility code may materialize one rich
/// checked type through the final linked alpha namespace.
#[derive(Clone, Copy)]
pub struct KernelPackedTypeRef<'a> {
    input: &'a KernelSemanticInputV1,
    scope: KernelPackedTypeScope,
    term: crate::TypeTermId,
}

#[derive(Clone, Copy)]
pub struct KernelPackedFlowRef<'a> {
    mode: FlowMode,
    ty: KernelPackedTypeRef<'a>,
}

/// Borrowed generic scheme targeted by one call.
///
/// User and ABI schemes share one packed term store but retain disjoint alpha
/// namespaces. The target identity carried here keeps every returned flow and
/// parameter token bound to the correct namespace.
#[derive(Clone, Copy)]
pub struct KernelCallableSchemeRef<'a> {
    input: &'a KernelSemanticInputV1,
    target: crate::KernelCallableSchemeId,
}

pub struct KernelCallableSchemeIter<'a> {
    input: &'a KernelSemanticInputV1,
    schemes: std::slice::Iter<'a, crate::KernelCallableSchemeId>,
}

/// Borrowed parameter row qualified by its callable scheme and packed type
/// namespace. Text and types remain in the immutable kernel authorities.
#[derive(Clone, Copy)]
pub struct KernelSemanticCallableParameterRef<'a> {
    callable: KernelCallableSchemeRef<'a>,
    ordinal: u32,
    declaration: DeclId,
}

pub struct KernelSemanticCallableParameterIter<'a> {
    callable: KernelCallableSchemeRef<'a>,
    next: u32,
    len: u32,
}

/// Borrowed ABI call-context row. User callables currently have no such rows;
/// PASSED is represented independently by a context formal.
#[derive(Clone, Copy)]
pub struct KernelSemanticCallableContextRef<'a> {
    callable: KernelCallableSchemeRef<'a>,
    ordinal: u32,
}

pub struct KernelSemanticCallableContextIter<'a> {
    callable: KernelCallableSchemeRef<'a>,
    next: u32,
    len: u32,
}

/// One user-callable PASSED scheme. Its flow is a borrowed packed formal, not
/// a reconstructed `CheckedContextScheme` with nested `Vec<String>` paths.
#[derive(Clone, Copy)]
pub struct KernelSemanticContextFormalRef<'a> {
    callable: KernelCallableSchemeRef<'a>,
    id: ContextFormalId,
    ordinal: u32,
}

/// Required/optional parameter policy with borrowed interned/default text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelSemanticParameterRequirementRef<'a> {
    Required,
    CallableProfile(&'a str),
    Tag(&'a str),
    ExactInteger(i64),
    Text(&'a str),
}

/// Global declaration lookup spanning definition-owned rows and the synthetic
/// declaration rows of referenced ABI callables and their parameters.
#[derive(Clone, Copy)]
pub enum KernelSemanticResolvedDeclarationRef<'a> {
    Definition(KernelSemanticDeclarationRef<'a>),
    AbiCallable(KernelCallableSchemeRef<'a>),
    AbiParameter(KernelSemanticCallableParameterRef<'a>),
}

/// Target-bound generic parameter token. Its ordinal is meaningful only in
/// the callable scheme retained in this value.
#[derive(Clone, Copy)]
pub struct KernelCallableTypeParameterRef<'a> {
    scheme: KernelCallableSchemeRef<'a>,
    ordinal: crate::KernelTypeParameterId,
}

#[derive(Clone, Copy)]
pub struct KernelDefinitionCallTypeFactsRef<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
    target: crate::KernelCallableSchemeId,
    base_result: crate::PackedFlow,
    published_result: crate::PackedFlow,
    substitutions: &'a [crate::PackedCallTypeSubstitution],
    syntax_discriminated_result: bool,
}

#[derive(Clone, Copy)]
pub struct KernelDefinitionCallTypeSubstitutionRef<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
    target: crate::KernelCallableSchemeId,
    substitution: &'a crate::PackedCallTypeSubstitution,
}

/// Explicit, phase-scoped compatibility projector for packed semantic types.
///
/// Structural consumers use [`KernelPackedTypeRef`] directly and allocate
/// nothing. A rich checked/editor boundary creates one of these and reuses its
/// recursive export cache for the complete projection instead of allocating a
/// project-sized cache for each type root.
#[doc(hidden)]
pub struct KernelSemanticTypeMaterializer<'a> {
    input: &'a KernelSemanticInputV1,
    cache: DefinitionTypeMaterializationCache,
    scope: Option<KernelPackedTypeScope>,
    variables: BTreeMap<TypeVar, TypeVar>,
    next: u32,
    alpha_end: u32,
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
pub struct KernelSemanticCallRef<'a> {
    input: &'a KernelSemanticInputV1,
    owner: KernelOwnerId,
    ordinal: u32,
}

pub struct KernelSemanticCallIter<'a> {
    input: &'a KernelSemanticInputV1,
    next: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelSemanticCallEntryRef {
    Input {
        parameter_ordinal: u32,
        value: CheckedExprId,
        from_pipe: bool,
    },
    FreshOut {
        parameter_ordinal: u32,
        output: DeclId,
        scope: LexicalScopeId,
    },
    ForwardOut {
        parameter_ordinal: u32,
        target: DeclId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSemanticCallContextRef {
    pub declaration: DeclId,
    pub signature_ordinal: u32,
    pub scope: LexicalScopeId,
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
pub struct KernelSemanticPatternBindingRef<'a> {
    input: &'a KernelSemanticInputV1,
    locator: KernelSemanticPatternBindingLocatorV1,
}

pub struct KernelSemanticPatternBindingIter<'a> {
    input: &'a KernelSemanticInputV1,
    rows: std::slice::Iter<'a, KernelSemanticPatternBindingLocatorV1>,
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

impl<'a> KernelSemanticDefinitionRowsRef<'a> {
    fn definition(self) -> KernelDefinitionRef<'a> {
        KernelDefinitionRef::from_authorities(
            &self.input.construction.program,
            &self.input.construction.definition_code,
            self.owner,
        )
        .expect("sealed kernel semantic definition remains in every packed authority")
    }

    fn relocation(self) -> &'a KernelSemanticDefinitionRelocationV1 {
        self.input
            .definition_relocation(self.owner)
            .expect("sealed kernel semantic definition has one relocation")
    }

    fn text_path(self, path: PathId) -> KernelSemanticTextPathRef<'a> {
        KernelSemanticTextPathRef {
            path: self
                .definition()
                .input()
                .path(path)
                .expect("sealed kernel semantic path belongs to its project text authority"),
        }
    }

    fn path(self, anchor: KernelDeclarationReference, path: PathId) -> KernelSemanticPathRef<'a> {
        KernelSemanticPathRef {
            anchor: self
                .input
                .construction
                .relocate_declaration(self.owner, anchor)
                .expect("sealed kernel semantic path anchor relocates"),
            projection: self.text_path(path),
        }
    }

    fn symbol(self, symbol: SymbolId) -> &'a str {
        self.definition()
            .input()
            .symbol(symbol)
            .expect("sealed kernel semantic symbol belongs to its project text authority")
    }

    pub const fn owner(self) -> KernelOwnerId {
        self.owner
    }

    pub fn containing_scope(self) -> LexicalScopeId {
        self.relocation().containing_scope
    }

    /// Scope that owns this definition's stable checked-image authority.
    pub fn authority_root_scope(self) -> LexicalScopeId {
        self.relocation().authority_root_scope
    }

    pub fn root_statement(self) -> CheckedStatementId {
        let root = self
            .definition()
            .linkage()
            .root_statement
            .expect("sealed kernel semantic definition has a root statement");
        CheckedStatementId(
            self.relocation()
                .statements
                .resolve(root.0, "semantic root statement")
                .expect("sealed kernel semantic root statement relocates"),
        )
    }

    pub fn public_declaration(self) -> DeclId {
        self.relocation().callable
    }

    pub fn result_expression(self) -> CheckedExprId {
        self.relocation().result_expression
    }

    pub fn scope_count(self) -> usize {
        self.definition().runtime_facts().scopes().len()
    }

    pub fn scopes(self) -> KernelSemanticScopeIter<'a> {
        KernelSemanticScopeIter {
            definition: self,
            next: 0,
        }
    }

    pub fn scope(self, ordinal: usize) -> Option<KernelSemanticScopeRef<'a>> {
        (ordinal < self.scope_count()).then_some(KernelSemanticScopeRef {
            definition: Some(self),
            ordinal: u32::try_from(ordinal).ok()?,
        })
    }

    pub fn declaration_count(self) -> usize {
        self.definition().runtime_facts().declarations().len()
    }

    pub fn declarations(self) -> KernelSemanticDeclarationIter<'a> {
        KernelSemanticDeclarationIter {
            definition: self,
            next: 0,
        }
    }

    pub fn declaration(self, ordinal: usize) -> Option<KernelSemanticDeclarationRef<'a>> {
        (ordinal < self.declaration_count()).then_some(KernelSemanticDeclarationRef {
            definition: self,
            ordinal: u32::try_from(ordinal).ok()?,
        })
    }

    pub fn statement_count(self) -> usize {
        self.definition().runtime_facts().statements().len()
    }

    pub fn statements(self) -> KernelSemanticStatementIter<'a> {
        KernelSemanticStatementIter {
            definition: self,
            next: 0,
        }
    }

    pub fn statement(self, ordinal: usize) -> Option<KernelSemanticStatementRef<'a>> {
        (ordinal < self.statement_count()).then_some(KernelSemanticStatementRef {
            definition: self,
            ordinal: u32::try_from(ordinal).ok()?,
        })
    }

    pub fn expression_count(self) -> usize {
        self.definition().input().node_count()
    }

    pub fn expressions(self) -> KernelSemanticExpressionIter<'a> {
        KernelSemanticExpressionIter {
            definition: self,
            next: 0,
        }
    }

    pub fn expression(self, ordinal: usize) -> Option<KernelSemanticExpressionRef<'a>> {
        (ordinal < self.expression_count()).then_some(KernelSemanticExpressionRef {
            definition: self,
            ordinal: u32::try_from(ordinal).ok()?,
        })
    }

    pub fn source_count(self) -> usize {
        self.definition().runtime_facts().sources().len()
    }

    pub fn sources(self) -> KernelSemanticSourceIter<'a> {
        KernelSemanticSourceIter {
            definition: self,
            next: 0,
        }
    }

    pub fn source(self, ordinal: usize) -> Option<KernelSemanticSourceRef<'a>> {
        (ordinal < self.source_count()).then_some(KernelSemanticSourceRef {
            definition: self,
            ordinal: u32::try_from(ordinal).ok()?,
        })
    }

    pub fn state_count(self) -> usize {
        self.definition().code().state_count()
    }

    pub fn states(self) -> KernelSemanticStateIter<'a> {
        KernelSemanticStateIter {
            definition: self,
            next: 0,
        }
    }

    pub fn state(self, ordinal: usize) -> Option<KernelSemanticStateRef<'a>> {
        (ordinal < self.state_count()).then_some(KernelSemanticStateRef {
            definition: self,
            ordinal: u32::try_from(ordinal).ok()?,
        })
    }

    pub fn list_count(self) -> usize {
        self.definition().runtime_facts().lists().len()
    }

    pub fn lists(self) -> KernelSemanticListIter<'a> {
        KernelSemanticListIter {
            definition: self,
            next: 0,
        }
    }

    pub fn list(self, ordinal: usize) -> Option<KernelSemanticListRef<'a>> {
        (ordinal < self.list_count()).then_some(KernelSemanticListRef {
            definition: self,
            ordinal: u32::try_from(ordinal).ok()?,
        })
    }
}

impl<'a> Iterator for KernelSemanticDefinitionRowsIter<'a> {
    type Item = KernelSemanticDefinitionRowsRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next as usize >= self.input.definition_count() {
            return None;
        }
        let owner = KernelOwnerId(self.next);
        self.next += 1;
        self.input.definition_rows(owner)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self
            .input
            .definition_count()
            .saturating_sub(self.next as usize);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticDefinitionRowsIter<'_> {}

macro_rules! impl_semantic_definition_row_iter {
    ($iter:ident, $item:ident, $count:ident, $get:ident) => {
        impl<'a> Iterator for $iter<'a> {
            type Item = $item<'a>;

            fn next(&mut self) -> Option<Self::Item> {
                if self.next as usize >= self.definition.$count() {
                    return None;
                }
                let ordinal = self.next;
                self.next += 1;
                self.definition.$get(ordinal as usize)
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = self.definition.$count().saturating_sub(self.next as usize);
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for $iter<'_> {}
    };
}

impl_semantic_definition_row_iter!(
    KernelSemanticScopeIter,
    KernelSemanticScopeRef,
    scope_count,
    scope
);
impl_semantic_definition_row_iter!(
    KernelSemanticDeclarationIter,
    KernelSemanticDeclarationRef,
    declaration_count,
    declaration
);
impl_semantic_definition_row_iter!(
    KernelSemanticStatementIter,
    KernelSemanticStatementRef,
    statement_count,
    statement
);
impl_semantic_definition_row_iter!(
    KernelSemanticExpressionIter,
    KernelSemanticExpressionRef,
    expression_count,
    expression
);
impl_semantic_definition_row_iter!(
    KernelSemanticSourceIter,
    KernelSemanticSourceRef,
    source_count,
    source
);
impl_semantic_definition_row_iter!(
    KernelSemanticStateIter,
    KernelSemanticStateRef,
    state_count,
    state
);
impl_semantic_definition_row_iter!(
    KernelSemanticListIter,
    KernelSemanticListRef,
    list_count,
    list
);

impl<'a> KernelSemanticTextPathRef<'a> {
    pub fn len(self) -> usize {
        self.path.len()
    }

    pub fn is_empty(self) -> bool {
        self.path.is_empty()
    }

    pub fn names(self) -> KernelSemanticPathIter<'a> {
        KernelSemanticPathIter {
            path: self,
            next: 0,
        }
    }
}

impl<'a> KernelSemanticPathRef<'a> {
    pub const fn anchor(self) -> DeclId {
        self.anchor
    }

    pub const fn projection(self) -> KernelSemanticTextPathRef<'a> {
        self.projection
    }
}

impl<'a> Iterator for KernelSemanticPathIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let name = self.path.path.name_at(self.next)?;
        self.next += 1;
        Some(name)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.path.len().saturating_sub(self.next);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticPathIter<'_> {}

impl<'a> KernelSemanticScopeRef<'a> {
    fn row(self) -> Option<&'a crate::PackedScopePresentation> {
        let definition = self.definition?;
        definition
            .definition()
            .runtime_facts()
            .scopes()
            .get(self.ordinal as usize)
    }

    pub fn id(self) -> LexicalScopeId {
        match self.definition {
            None => LexicalScopeId(0),
            Some(definition) => LexicalScopeId(
                definition
                    .relocation()
                    .scopes
                    .resolve(self.ordinal, "semantic scope")
                    .expect("sealed kernel semantic scope relocates"),
            ),
        }
    }

    pub fn parent(self) -> Option<LexicalScopeId> {
        let definition = self.definition?;
        Some(
            definition
                .input
                .construction
                .relocate_scope(definition.owner, self.row()?.parent)
                .expect("sealed kernel semantic parent scope relocates"),
        )
    }

    pub fn owner(self) -> Option<DeclId> {
        let definition = self.definition?;
        self.row()?.owner.map(|owner| {
            definition
                .input
                .construction
                .relocate_declaration(definition.owner, owner)
                .expect("sealed kernel semantic scope owner relocates")
        })
    }

    pub fn kind(self) -> CheckedScopeKind {
        match self.row().map(|row| row.kind) {
            None => CheckedScopeKind::Root,
            Some(crate::KernelScopeKind::Function) => CheckedScopeKind::Function,
            Some(crate::KernelScopeKind::Block) => CheckedScopeKind::Block,
            Some(crate::KernelScopeKind::Record) => CheckedScopeKind::Record,
            Some(crate::KernelScopeKind::RepeatedOutput) => CheckedScopeKind::RepeatedOutput,
            Some(crate::KernelScopeKind::CallContext) => CheckedScopeKind::CallContext,
        }
    }

    pub fn origin(self) -> Option<crate::KernelScopeOrigin> {
        self.row().map(|row| row.origin)
    }

    pub fn span(self) -> Option<CheckedSpan> {
        match self.definition {
            None => Some(CheckedSpan::default()),
            Some(definition) => definition
                .input
                .construction
                .rebase_span(definition.owner, self.row()?.span.materialize()),
        }
    }
}

impl<'a> KernelSemanticDeclarationRef<'a> {
    fn row(self) -> &'a crate::PackedDeclaration {
        self.definition
            .definition()
            .runtime_facts()
            .declarations()
            .get(self.ordinal as usize)
            .expect("sealed kernel semantic declaration row exists")
    }

    fn presentation(self) -> &'a crate::PackedDeclarationPresentation {
        self.definition
            .definition()
            .runtime_facts()
            .declaration_presentations()
            .get(self.ordinal as usize)
            .filter(|presentation| presentation.declaration == self.row().id)
            .expect("sealed kernel semantic declaration presentation is dense")
    }

    pub fn id(self) -> DeclId {
        DeclId(
            self.definition
                .relocation()
                .declarations
                .resolve(self.ordinal, "semantic declaration")
                .expect("sealed kernel semantic declaration relocates"),
        )
    }

    pub fn scope(self) -> LexicalScopeId {
        self.definition
            .input
            .construction
            .relocate_scope(self.definition.owner, self.presentation().scope)
            .expect("sealed kernel semantic declaration scope relocates")
    }

    pub fn name(self) -> &'a str {
        self.definition.symbol(self.row().name)
    }

    pub fn kind(self) -> CheckedDeclarationKind {
        checked_declaration_kind(self.row().kind)
    }

    pub fn origin(self) -> crate::KernelDeclarationOrigin {
        self.row().origin
    }

    /// Direct packed declaration flow when this declaration owns one. Function,
    /// parameter, pattern, OUT, and value-derived effective flows remain
    /// callable/expression relations rather than a fabricated rich type row.
    pub fn flow(self) -> Option<KernelPackedFlowRef<'a>> {
        self.definition.input.declared_declaration_flow(self.id())
    }

    pub fn value(self) -> Option<CheckedExprId> {
        let value = self.row().value?;
        let value = self
            .definition
            .definition()
            .resolve_value(value, self.ordinal as usize)
            .expect("sealed kernel semantic declaration value resolves");
        self.definition
            .input
            .construction
            .relocate_value(self.definition.owner, value)
    }

    pub fn body_scope(self) -> Option<LexicalScopeId> {
        self.presentation().body_scope.map(|scope| {
            self.definition
                .input
                .construction
                .relocate_scope(self.definition.owner, KernelScopeReference::Local(scope))
                .expect("sealed kernel semantic declaration body scope relocates")
        })
    }

    pub fn span(self) -> Option<CheckedSpan> {
        self.definition.input.construction.rebase_span(
            self.definition.owner,
            self.presentation().span.materialize(),
        )
    }
}

impl<'a> KernelSemanticStatementRef<'a> {
    fn row(self) -> &'a crate::PackedStatement {
        self.definition
            .definition()
            .runtime_facts()
            .statements()
            .get(self.ordinal as usize)
            .expect("sealed kernel semantic statement row exists")
    }

    fn presentation(self) -> &'a crate::PackedStatementPresentation {
        self.definition
            .definition()
            .runtime_facts()
            .statement_presentations()
            .get(self.ordinal as usize)
            .filter(|presentation| presentation.statement == self.row().id)
            .expect("sealed kernel semantic statement presentation is dense")
    }

    pub fn id(self) -> CheckedStatementId {
        CheckedStatementId(
            self.definition
                .relocation()
                .statements
                .resolve(self.ordinal, "semantic statement")
                .expect("sealed kernel semantic statement relocates"),
        )
    }

    pub fn scope(self) -> LexicalScopeId {
        self.definition
            .input
            .construction
            .relocate_scope(self.definition.owner, self.presentation().scope)
            .expect("sealed kernel semantic statement scope relocates")
    }

    pub fn declaration(self) -> Option<DeclId> {
        statement_declaration_authority(self.definition.definition(), self.row())
            .expect("sealed kernel semantic statement declaration authority is unique")
            .map(|declaration| {
                self.definition
                    .input
                    .construction
                    .relocate_declaration(self.definition.owner, declaration)
                    .expect("sealed kernel semantic statement declaration relocates")
            })
    }

    pub fn kind(self) -> KernelSemanticStatementKindRef<'a> {
        match self.row().kind {
            crate::PackedStatementKind::Function { name, .. } => {
                KernelSemanticStatementKindRef::Function {
                    name: self.definition.symbol(name),
                }
            }
            crate::PackedStatementKind::Field { name } => KernelSemanticStatementKindRef::Field {
                name: self.definition.symbol(name),
            },
            crate::PackedStatementKind::Source { field, event } => {
                KernelSemanticStatementKindRef::Source {
                    field: field.map(|name| self.definition.symbol(name)),
                    event: event.map(|name| self.definition.symbol(name)),
                }
            }
            crate::PackedStatementKind::Hold { field, name } => {
                KernelSemanticStatementKindRef::Hold {
                    field: field.map(|name| self.definition.symbol(name)),
                    name: name.map(|name| self.definition.symbol(name)),
                }
            }
            crate::PackedStatementKind::List { field, capacity } => {
                KernelSemanticStatementKindRef::List {
                    field: field.map(|name| self.definition.symbol(name)),
                    capacity,
                }
            }
            crate::PackedStatementKind::Block => KernelSemanticStatementKindRef::Block,
            crate::PackedStatementKind::Spread => KernelSemanticStatementKindRef::Spread,
            crate::PackedStatementKind::Expression => KernelSemanticStatementKindRef::Expression,
        }
    }

    pub fn parameters(self) -> KernelSemanticStatementParameterIter<'a> {
        let parameters = self
            .definition
            .definition()
            .runtime_facts()
            .statement_parameters(self.row())
            .unwrap_or(&[])
            .iter();
        KernelSemanticStatementParameterIter {
            definition: self.definition,
            parameters,
        }
    }

    pub fn value(self) -> Option<CheckedExprId> {
        let value = self.row().value?;
        let value = self
            .definition
            .definition()
            .resolve_value(value, self.ordinal as usize)
            .expect("sealed kernel semantic statement value resolves");
        self.definition
            .input
            .construction
            .relocate_value(self.definition.owner, value)
    }

    pub fn value_use(self) -> CheckedValueUse {
        match self.row().value_use {
            crate::KernelStatementValueUse::RuntimeValue => CheckedValueUse::RuntimeValue,
            crate::KernelStatementValueUse::RenderSlot => CheckedValueUse::RenderSlot,
        }
    }

    pub fn children(self) -> KernelSemanticStatementChildIter<'a> {
        KernelSemanticStatementChildIter {
            definition: self.definition,
            children: self
                .definition
                .definition()
                .runtime_facts()
                .statement_children(self.row())
                .iter(),
        }
    }

    pub fn body_scope(self) -> Option<LexicalScopeId> {
        self.presentation().body_scope.map(|scope| {
            self.definition
                .input
                .construction
                .relocate_scope(self.definition.owner, KernelScopeReference::Local(scope))
                .expect("sealed kernel semantic statement body scope relocates")
        })
    }

    pub fn span(self) -> Option<CheckedSpan> {
        self.definition.input.construction.rebase_span(
            self.definition.owner,
            self.presentation().span.materialize(),
        )
    }
}

impl<'a> Iterator for KernelSemanticStatementParameterIter<'a> {
    type Item = KernelSemanticStatementParameterRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.parameters
            .next()
            .map(|parameter| KernelSemanticStatementParameterRef {
                definition: self.definition,
                parameter,
            })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.parameters.size_hint()
    }
}

impl ExactSizeIterator for KernelSemanticStatementParameterIter<'_> {}

impl<'a> KernelSemanticStatementParameterRef<'a> {
    pub fn name(self) -> &'a str {
        self.definition.symbol(self.parameter.name)
    }

    pub const fn kind(self) -> crate::KernelParameterKind {
        self.parameter.kind
    }

    pub const fn ordinal(self) -> u32 {
        self.parameter.ordinal
    }

    pub const fn evaluation_scope(self) -> crate::KernelParameterEvaluationScope {
        self.parameter.evaluation_scope
    }
}

impl<'a> Iterator for KernelSemanticStatementChildIter<'a> {
    type Item = CheckedStatementId;

    fn next(&mut self) -> Option<Self::Item> {
        let child = *self.children.next()?;
        Some(match child {
            KernelStatementChildReference::Local(child) => CheckedStatementId(
                self.definition
                    .relocation()
                    .statements
                    .resolve(child.0, "semantic statement child")
                    .expect("sealed local statement child relocates"),
            ),
            KernelStatementChildReference::Owner(owner) => self
                .definition
                .input
                .definition_rows(owner)
                .expect("sealed owner statement child exists")
                .root_statement(),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.children.size_hint()
    }
}

impl ExactSizeIterator for KernelSemanticStatementChildIter<'_> {}

impl<'a> KernelSemanticExpressionRef<'a> {
    fn row(self) -> &'a crate::PackedKernelOwnerNode {
        self.definition
            .definition()
            .input()
            .node(crate::KernelExpressionId(self.ordinal))
            .expect("sealed kernel semantic expression row exists")
    }

    fn presentation(self) -> &'a crate::PackedExpressionPresentation {
        self.definition
            .definition()
            .runtime_facts()
            .expression_presentations()
            .get(self.ordinal as usize)
            .filter(|presentation| presentation.expression.0 == self.ordinal)
            .expect("sealed kernel semantic expression presentation is dense")
    }

    fn packed_type(self, reference: crate::KernelTypeRef) -> KernelPackedTypeRef<'a> {
        let term = self
            .definition
            .input
            .construction
            .definition_code
            .type_store()
            .as_arena()
            .resolve_type_ref(reference)
            .expect("sealed expression input type belongs to its packed authority");
        self.definition
            .input
            .definition_type_ref(self.definition.owner, term)
    }

    fn pattern(self, pattern: crate::PackedKernelPattern) -> KernelSemanticPatternRef<'a> {
        match pattern {
            crate::PackedKernelPattern::Wildcard => KernelSemanticPatternRef::Wildcard,
            crate::PackedKernelPattern::Number => KernelSemanticPatternRef::Number,
            crate::PackedKernelPattern::Text => KernelSemanticPatternRef::Text,
            crate::PackedKernelPattern::Bits { width } => KernelSemanticPatternRef::Bits { width },
            crate::PackedKernelPattern::Tag { name, fields } => KernelSemanticPatternRef::Tag {
                name: self.definition.symbol(name),
                fields: self.definition.text_path(fields),
            },
            crate::PackedKernelPattern::Binding { name } => KernelSemanticPatternRef::Binding {
                name: self.definition.symbol(name),
            },
            crate::PackedKernelPattern::Invalid => KernelSemanticPatternRef::Invalid,
        }
    }

    pub fn id(self) -> CheckedExprId {
        CheckedExprId(
            self.definition
                .relocation()
                .expressions
                .resolve(self.ordinal, "semantic expression")
                .expect("sealed kernel semantic expression relocates"),
        )
    }

    pub fn scope(self) -> LexicalScopeId {
        self.definition
            .input
            .construction
            .relocate_scope(self.definition.owner, self.presentation().scope)
            .expect("sealed kernel semantic expression scope relocates")
    }

    pub fn declaration(self) -> Option<DeclId> {
        match self.presentation().declaration {
            Some(declaration) => self
                .definition
                .input
                .construction
                .relocate_declaration(self.definition.owner, declaration),
            None => self.definition.input.lexical_declaration_for_scope(
                self.definition.owner,
                self.presentation()
                    .declaration_scope
                    .unwrap_or(self.presentation().scope),
            ),
        }
    }

    pub fn flow(self) -> KernelPackedFlowRef<'a> {
        self.definition
            .input
            .published_expression_flow(self.id())
            .expect("sealed kernel semantic expression has a published packed flow")
    }

    pub fn base_flow(self) -> KernelPackedFlowRef<'a> {
        self.definition
            .input
            .base_expression_flow(self.id())
            .expect("sealed kernel semantic expression has a base packed flow")
    }

    pub fn input_mode(self) -> FlowMode {
        self.row().mode
    }

    pub fn effect(self) -> crate::KernelEffectSummary {
        self.definition
            .definition()
            .expression_effect(crate::KernelExpressionId(self.ordinal))
            .expect("sealed kernel semantic expression has effect facts")
    }

    pub fn operation(self) -> KernelSemanticExpressionOperationRef<'a> {
        use crate::PackedKernelOwnerNodeKind as Kind;
        match self.row().kind {
            Kind::Known(reference) => {
                KernelSemanticExpressionOperationRef::Known(self.packed_type(reference))
            }
            Kind::Source(reference) => {
                KernelSemanticExpressionOperationRef::Source(self.packed_type(reference))
            }
            Kind::Absent => KernelSemanticExpressionOperationRef::Absent,
            Kind::Text => KernelSemanticExpressionOperationRef::Text,
            Kind::TextTemplate => KernelSemanticExpressionOperationRef::TextTemplate,
            Kind::Number => KernelSemanticExpressionOperationRef::Number,
            Kind::Byte => KernelSemanticExpressionOperationRef::Byte,
            Kind::Bits(width) => KernelSemanticExpressionOperationRef::Bits(width),
            Kind::Tag(name) => {
                KernelSemanticExpressionOperationRef::Tag(self.definition.symbol(name))
            }
            Kind::Record { tag } => KernelSemanticExpressionOperationRef::Record {
                tag: tag.map(|name| self.definition.symbol(name)),
            },
            Kind::Block => KernelSemanticExpressionOperationRef::Block,
            Kind::Collection { kind, capacity } => {
                KernelSemanticExpressionOperationRef::Collection { kind, capacity }
            }
            Kind::MapEntry => KernelSemanticExpressionOperationRef::MapEntry,
            Kind::FormalRead { formal, fields } => {
                KernelSemanticExpressionOperationRef::FormalRead {
                    formal,
                    fields: self.definition.text_path(fields),
                }
            }
            Kind::ContextRead { formal, fields } => {
                KernelSemanticExpressionOperationRef::ContextRead {
                    formal,
                    fields: self.definition.text_path(fields),
                }
            }
            Kind::LexicalRead { fields } => KernelSemanticExpressionOperationRef::LexicalRead {
                fields: self.definition.text_path(fields),
            },
            Kind::ValueRead {
                fields,
                mode_narrowing,
            } => KernelSemanticExpressionOperationRef::ValueRead {
                fields: self.definition.text_path(fields),
                mode_narrowing: mode_narrowing.map(|expression| {
                    CheckedExprId(
                        self.definition
                            .relocation()
                            .expressions
                            .resolve(expression.0, "semantic mode narrowing")
                            .expect("sealed mode-narrowing expression relocates"),
                    )
                }),
            },
            Kind::DerivedRead { fields } => KernelSemanticExpressionOperationRef::DerivedRead {
                fields: self.definition.text_path(fields),
            },
            Kind::PatternRead { pattern, fields } => {
                KernelSemanticExpressionOperationRef::PatternRead {
                    pattern: self.pattern(pattern),
                    fields: self.definition.text_path(fields),
                }
            }
            Kind::CollectionItemRead => KernelSemanticExpressionOperationRef::CollectionItemRead,
            Kind::FreshOut => KernelSemanticExpressionOperationRef::FreshOut,
            Kind::UserCall {
                target,
                inherited_formal,
            } => KernelSemanticExpressionOperationRef::UserCall {
                target,
                inherited_formal,
            },
            Kind::RenderConstructor { kind } => {
                KernelSemanticExpressionOperationRef::RenderConstructor(match kind {
                    crate::PackedKernelRenderConstructorKind::Fixed(name) => {
                        KernelSemanticRenderConstructorRef::Fixed(self.definition.symbol(name))
                    }
                    crate::PackedKernelRenderConstructorKind::StripeDirection => {
                        KernelSemanticRenderConstructorRef::StripeDirection
                    }
                })
            }
            Kind::PureBuiltin { kind } => KernelSemanticExpressionOperationRef::PureBuiltin(kind),
            Kind::FixedAbiCall { result } => KernelSemanticExpressionOperationRef::FixedAbiCall {
                result: self.packed_type(result),
            },
            Kind::HostEffect { operation } => KernelSemanticExpressionOperationRef::HostEffect {
                operation: self.definition.symbol(operation),
            },
            Kind::Latest => KernelSemanticExpressionOperationRef::Latest,
            Kind::When => KernelSemanticExpressionOperationRef::When,
            Kind::Then => KernelSemanticExpressionOperationRef::Then,
            Kind::Infix { operation } => KernelSemanticExpressionOperationRef::Infix {
                operation: self.definition.symbol(operation),
            },
            Kind::Draining => KernelSemanticExpressionOperationRef::Draining,
            Kind::Hold => KernelSemanticExpressionOperationRef::Hold,
            Kind::MatchArm { pattern } => KernelSemanticExpressionOperationRef::MatchArm {
                pattern: self.pattern(pattern),
            },
            Kind::Arrow => KernelSemanticExpressionOperationRef::Arrow,
            Kind::Delimiter => KernelSemanticExpressionOperationRef::Delimiter,
            Kind::Unknown => KernelSemanticExpressionOperationRef::Unknown,
            Kind::Flush => KernelSemanticExpressionOperationRef::Flush,
            Kind::FieldProjection { field } => {
                KernelSemanticExpressionOperationRef::FieldProjection {
                    field: self.definition.symbol(field),
                }
            }
        }
    }

    pub fn inputs(self) -> KernelSemanticExpressionInputIter<'a> {
        KernelSemanticExpressionInputIter {
            expression: self,
            edges: self
                .row()
                .inputs(self.definition.definition().input())
                .iter(),
        }
    }

    pub fn span(self) -> Option<CheckedSpan> {
        self.definition.input.construction.rebase_span(
            self.definition.owner,
            self.presentation().span.materialize(),
        )
    }
}

impl<'a> Iterator for KernelSemanticExpressionInputIter<'a> {
    type Item = KernelSemanticExpressionInputRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.edges
            .next()
            .map(|edge| KernelSemanticExpressionInputRef {
                expression: self.expression,
                edge,
            })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.edges.size_hint()
    }
}

impl ExactSizeIterator for KernelSemanticExpressionInputIter<'_> {}

impl<'a> KernelSemanticExpressionInputRef<'a> {
    pub fn value(self) -> CheckedExprId {
        let value = self
            .expression
            .definition
            .definition()
            .resolve_value(self.edge.expression, self.expression.ordinal as usize)
            .expect("sealed kernel semantic expression input resolves");
        self.expression
            .definition
            .input
            .construction
            .relocate_value(self.expression.definition.owner, value)
            .expect("sealed kernel semantic expression input relocates")
    }

    pub fn role(self) -> KernelSemanticExpressionInputRoleRef<'a> {
        use crate::PackedKernelOwnerEdgeRole as Role;
        match self.edge.role {
            Role::RecordField { name, spread } => {
                KernelSemanticExpressionInputRoleRef::RecordField {
                    name: self.expression.definition.symbol(name),
                    spread,
                }
            }
            Role::TextDynamic => KernelSemanticExpressionInputRoleRef::TextDynamic,
            Role::BlockResult => KernelSemanticExpressionInputRoleRef::BlockResult,
            Role::CollectionItem => KernelSemanticExpressionInputRoleRef::CollectionItem,
            Role::MapEntry => KernelSemanticExpressionInputRoleRef::MapEntry,
            Role::MapKey => KernelSemanticExpressionInputRoleRef::MapKey,
            Role::MapValue => KernelSemanticExpressionInputRoleRef::MapValue,
            Role::ReadProvider => KernelSemanticExpressionInputRoleRef::ReadProvider,
            Role::CallArgument { ordinal } => {
                KernelSemanticExpressionInputRoleRef::CallArgument { ordinal }
            }
            Role::CallOutArgument { ordinal } => {
                KernelSemanticExpressionInputRoleRef::CallOutArgument { ordinal }
            }
            Role::AbiArgument { name } => KernelSemanticExpressionInputRoleRef::AbiArgument {
                name: self.expression.definition.symbol(name),
            },
            Role::LatestBranch => KernelSemanticExpressionInputRoleRef::LatestBranch,
            Role::WhenInput => KernelSemanticExpressionInputRoleRef::WhenInput,
            Role::WhenArm => KernelSemanticExpressionInputRoleRef::WhenArm,
            Role::ThenInput => KernelSemanticExpressionInputRoleRef::ThenInput,
            Role::ThenOutput => KernelSemanticExpressionInputRoleRef::ThenOutput,
            Role::InfixLeft => KernelSemanticExpressionInputRoleRef::InfixLeft,
            Role::InfixRight => KernelSemanticExpressionInputRoleRef::InfixRight,
            Role::DrainingInput => KernelSemanticExpressionInputRoleRef::DrainingInput,
            Role::HoldInitial => KernelSemanticExpressionInputRoleRef::HoldInitial,
            Role::HoldUpdate => KernelSemanticExpressionInputRoleRef::HoldUpdate,
            Role::MatchOutput => KernelSemanticExpressionInputRoleRef::MatchOutput,
            Role::ArrowOutput => KernelSemanticExpressionInputRoleRef::ArrowOutput,
            Role::FlushPayload => KernelSemanticExpressionInputRoleRef::FlushPayload,
        }
    }
}

impl<'a> KernelSemanticSourceRef<'a> {
    fn row(self) -> &'a crate::PackedSource {
        self.definition
            .definition()
            .runtime_facts()
            .sources()
            .get(self.ordinal as usize)
            .expect("sealed kernel semantic SOURCE row exists")
    }

    fn presentation(self) -> &'a crate::PackedExpressionPresentation {
        expression_presentation(
            self.definition.definition().runtime_facts(),
            self.row().expression,
        )
        .expect("sealed kernel semantic SOURCE has an expression presentation")
    }

    pub fn id(self) -> CheckedSourceId {
        self.definition
            .input
            .construction
            .relocate_source(self.definition.owner, self.ordinal)
            .expect("sealed kernel semantic SOURCE relocates")
    }

    pub fn declaration(self) -> DeclId {
        self.definition
            .input
            .construction
            .relocate_declaration(self.definition.owner, self.row().declaration)
            .expect("sealed kernel semantic SOURCE declaration relocates")
    }

    pub fn statement(self) -> CheckedStatementId {
        self.definition
            .input
            .relocate_statement(self.definition.owner, self.row().statement)
            .expect("sealed kernel semantic SOURCE statement relocates")
    }

    pub fn expression(self) -> CheckedExprId {
        CheckedExprId(
            self.definition
                .relocation()
                .expressions
                .resolve(self.row().expression.0, "semantic SOURCE expression")
                .expect("sealed kernel semantic SOURCE expression relocates"),
        )
    }

    pub fn owner_scope(self) -> LexicalScopeId {
        self.definition
            .input
            .construction
            .relocate_scope(self.definition.owner, self.presentation().scope)
            .expect("sealed kernel semantic SOURCE scope relocates")
    }

    pub fn path(self) -> KernelSemanticPathRef<'a> {
        self.definition
            .path(self.row().declaration, self.row().projection)
    }

    pub fn interval_ms(self) -> Option<u64> {
        self.row().interval_ms
    }

    pub fn payload_type(self) -> KernelPackedTypeRef<'a> {
        let term = self
            .definition
            .definition()
            .code()
            .source_payload_term(self.ordinal as usize)
            .expect("sealed kernel semantic SOURCE has a payload term");
        self.definition
            .input
            .definition_type_ref(self.definition.owner, term)
    }

    pub fn span(self) -> Option<CheckedSpan> {
        self.definition.input.construction.rebase_span(
            self.definition.owner,
            self.presentation().span.materialize(),
        )
    }
}

impl<'a> KernelSemanticStateRef<'a> {
    fn published(self) -> crate::PackedPublishedState {
        *self
            .definition
            .definition()
            .code()
            .states()
            .get(self.ordinal as usize)
            .expect("sealed kernel semantic published state row exists")
    }

    fn row(self) -> &'a crate::PackedState {
        self.definition
            .definition()
            .runtime_facts()
            .states()
            .get(self.published().input_ordinal as usize)
            .expect("sealed kernel semantic state input row exists")
    }

    pub fn id(self) -> CheckedStateId {
        self.definition
            .input
            .construction
            .relocate_state(self.definition.owner, self.ordinal)
            .expect("sealed kernel semantic state relocates")
    }

    pub fn binding_declaration(self) -> DeclId {
        self.definition
            .input
            .construction
            .relocate_declaration(self.definition.owner, self.row().binding_declaration)
            .expect("sealed kernel semantic state binding declaration relocates")
    }

    pub fn declaration(self) -> DeclId {
        self.definition
            .input
            .construction
            .relocate_declaration(self.definition.owner, self.row().declaration)
            .expect("sealed kernel semantic state declaration relocates")
    }

    pub fn statement(self) -> CheckedStatementId {
        self.definition
            .input
            .relocate_statement(self.definition.owner, self.row().statement)
            .expect("sealed kernel semantic state statement relocates")
    }

    pub fn expression(self) -> CheckedExprId {
        CheckedExprId(
            self.definition
                .relocation()
                .expressions
                .resolve(self.row().expression.0, "semantic state expression")
                .expect("sealed kernel semantic state expression relocates"),
        )
    }

    pub fn initial(self) -> CheckedExprId {
        let value = self
            .definition
            .definition()
            .resolve_value(self.row().initial, self.row().expression.0 as usize)
            .expect("sealed kernel semantic state initial value resolves");
        self.definition
            .input
            .construction
            .relocate_value(self.definition.owner, value)
            .expect("sealed kernel semantic state initial value relocates")
    }

    pub fn owner_scope(self) -> LexicalScopeId {
        if self.row().kind == boon_checked::CheckedStateKind::StatementHold {
            let (owner, statement) = self
                .definition
                .input
                .local_statement_reference(self.definition.owner, self.row().statement)
                .expect("sealed kernel semantic state statement resolves");
            let definition = self
                .definition
                .input
                .definition_rows(owner)
                .expect("sealed kernel semantic state statement owner exists");
            let presentation = definition
                .definition()
                .runtime_facts()
                .statement_presentations()
                .get(statement.0 as usize)
                .expect("sealed kernel semantic state statement presentation exists");
            definition
                .input
                .construction
                .relocate_scope(owner, presentation.scope)
                .expect("sealed kernel semantic state statement scope relocates")
        } else {
            let presentation = expression_presentation(
                self.definition.definition().runtime_facts(),
                self.row().expression,
            )
            .expect("sealed kernel semantic state expression presentation exists");
            self.definition
                .input
                .construction
                .relocate_scope(self.definition.owner, presentation.scope)
                .expect("sealed kernel semantic state expression scope relocates")
        }
    }

    pub fn path(self) -> KernelSemanticStatePathRef<'a> {
        match self.published().synthetic_ordinal() {
            Some(ordinal) => KernelSemanticStatePathRef::Synthetic {
                anchor: self.declaration(),
                ordinal,
            },
            None => KernelSemanticStatePathRef::Authored(
                self.definition
                    .path(self.row().declaration, self.row().projection),
            ),
        }
    }

    pub fn kind(self) -> boon_checked::CheckedStateKind {
        self.row().kind
    }

    pub fn flow(self) -> KernelPackedFlowRef<'a> {
        let flow = self.published().flow;
        KernelPackedFlowRef {
            mode: flow.mode,
            ty: self
                .definition
                .input
                .definition_type_ref(self.definition.owner, flow.term),
        }
    }

    pub fn span(self) -> Option<CheckedSpan> {
        if self.row().kind == boon_checked::CheckedStateKind::StatementHold {
            let (owner, statement) = self
                .definition
                .input
                .local_statement_reference(self.definition.owner, self.row().statement)?;
            let definition = self.definition.input.definition_rows(owner)?;
            let presentation = definition
                .definition()
                .runtime_facts()
                .statement_presentations()
                .get(statement.0 as usize)?;
            definition
                .input
                .construction
                .rebase_span(owner, presentation.span.materialize())
        } else {
            let presentation = expression_presentation(
                self.definition.definition().runtime_facts(),
                self.row().expression,
            )
            .ok()?;
            self.definition
                .input
                .construction
                .rebase_span(self.definition.owner, presentation.span.materialize())
        }
    }
}

impl<'a> KernelSemanticListRef<'a> {
    fn row(self) -> &'a crate::PackedList {
        self.definition
            .definition()
            .runtime_facts()
            .lists()
            .get(self.ordinal as usize)
            .expect("sealed kernel semantic LIST row exists")
    }

    fn presentation(self) -> &'a crate::PackedExpressionPresentation {
        expression_presentation(
            self.definition.definition().runtime_facts(),
            self.row().producer,
        )
        .expect("sealed kernel semantic LIST has an expression presentation")
    }

    pub fn id(self) -> CheckedListId {
        self.definition
            .input
            .construction
            .relocate_list(self.definition.owner, self.ordinal)
            .expect("sealed kernel semantic LIST relocates")
    }

    pub fn declaration(self) -> DeclId {
        self.definition
            .input
            .construction
            .relocate_declaration(self.definition.owner, self.row().declaration)
            .expect("sealed kernel semantic LIST declaration relocates")
    }

    pub fn statement(self) -> CheckedStatementId {
        self.definition
            .input
            .relocate_statement(self.definition.owner, self.row().statement)
            .expect("sealed kernel semantic LIST statement relocates")
    }

    pub fn producer(self) -> CheckedExprId {
        CheckedExprId(
            self.definition
                .relocation()
                .expressions
                .resolve(self.row().producer.0, "semantic LIST producer")
                .expect("sealed kernel semantic LIST producer relocates"),
        )
    }

    pub fn owner_scope(self) -> LexicalScopeId {
        self.definition
            .input
            .construction
            .relocate_scope(self.definition.owner, self.presentation().scope)
            .expect("sealed kernel semantic LIST scope relocates")
    }

    pub fn path(self) -> KernelSemanticPathRef<'a> {
        self.definition
            .path(self.row().declaration, self.row().projection)
    }

    pub fn item_type(self) -> KernelPackedTypeRef<'a> {
        let term = self
            .definition
            .definition()
            .code()
            .list_item_term(self.ordinal as usize)
            .expect("sealed kernel semantic LIST has an item term");
        self.definition
            .input
            .definition_type_ref(self.definition.owner, term)
    }

    pub fn capacity(self) -> Option<u32> {
        self.row().capacity
    }

    pub fn key_policy(self) -> boon_checked::CheckedListKeyPolicy {
        self.row().key_policy
    }

    pub fn span(self) -> Option<CheckedSpan> {
        self.definition.input.construction.rebase_span(
            self.definition.owner,
            self.presentation().span.materialize(),
        )
    }
}

impl<'a> Iterator for KernelSemanticCallIter<'a> {
    type Item = KernelSemanticCallRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.input.construction.call_count {
            return None;
        }
        let id = CheckedCallId(self.next);
        self.next += 1;
        Some(
            self.input
                .call(id)
                .expect("sealed kernel semantic call iterator remains dense"),
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.input.construction.call_count.saturating_sub(self.next) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticCallIter<'_> {}

impl<'a> KernelSemanticCallRef<'a> {
    fn code(self) -> crate::DefinitionCodeRef<'a> {
        self.input
            .construction
            .definition_code
            .definition(self.owner)
            .expect("sealed kernel semantic call owner has definition code")
    }

    pub fn id(self) -> CheckedCallId {
        self.input
            .construction
            .relocate_call(crate::PackedCallRef::new(self.owner, self.ordinal))
            .expect("sealed kernel semantic call relocates")
    }

    pub fn expression(self) -> CheckedExprId {
        let expression = self
            .code()
            .call_expression(self.ordinal as usize)
            .expect("sealed kernel semantic call has an expression");
        self.input
            .construction
            .relocate_expression(crate::PackedExpressionRef::new(self.owner, expression))
            .expect("sealed kernel semantic call expression relocates")
    }

    pub fn target_scheme(self) -> KernelCallableSchemeRef<'a> {
        let target = self
            .code()
            .call_target(self.ordinal as usize)
            .flatten()
            .expect("sealed kernel semantic call has a target scheme");
        self.input.callable_scheme_ref(target)
    }

    pub fn callable(self) -> DeclId {
        match self.target_scheme().target {
            crate::KernelCallableSchemeId::User(owner) => {
                self.input
                    .definition_relocation(owner)
                    .expect("sealed user call target relocates")
                    .callable
            }
            crate::KernelCallableSchemeId::Abi(callable) => {
                self.input
                    .construction
                    .abi_relocation(callable)
                    .expect("sealed ABI call target relocates")
                    .callable
            }
        }
    }

    pub fn owner_callable(self) -> Option<DeclId> {
        self.input
            .definition_relocation(self.owner)
            .expect("sealed kernel semantic call owner relocates")
            .owner_callable
    }

    pub fn function(self) -> &'a str {
        let symbol = self
            .code()
            .call_function(self.ordinal as usize)
            .expect("sealed kernel semantic call has a function symbol");
        self.input
            .construction
            .definition_code
            .symbol(symbol)
            .expect("sealed kernel semantic call function belongs to its text authority")
    }

    pub fn entry_count(self) -> usize {
        self.code()
            .call_entries(self.ordinal as usize)
            .expect("sealed kernel semantic call has an entry span")
            .len()
    }

    pub fn entry(self, ordinal: usize) -> Option<KernelSemanticCallEntryRef> {
        self.code()
            .call_entries(self.ordinal as usize)?
            .get(ordinal)
            .copied()
            .map(|entry| self.relocate_entry(entry))
    }

    pub fn entries(self) -> impl ExactSizeIterator<Item = KernelSemanticCallEntryRef> + 'a {
        self.code()
            .call_entries(self.ordinal as usize)
            .expect("sealed kernel semantic call has an entry span")
            .iter()
            .copied()
            .map(move |entry| self.relocate_entry(entry))
    }

    fn relocate_entry(self, entry: crate::PackedCallEntry) -> KernelSemanticCallEntryRef {
        match entry {
            crate::PackedCallEntry::Input {
                parameter_ordinal,
                value,
                from_pipe,
            } => KernelSemanticCallEntryRef::Input {
                parameter_ordinal,
                value: self
                    .input
                    .construction
                    .relocate_value(self.owner, value)
                    .expect("sealed kernel semantic call input relocates"),
                from_pipe,
            },
            crate::PackedCallEntry::FreshOut {
                parameter_ordinal,
                output,
                scope,
            } => KernelSemanticCallEntryRef::FreshOut {
                parameter_ordinal,
                output: self
                    .input
                    .construction
                    .relocate_declaration(self.owner, KernelDeclarationReference::Local(output))
                    .expect("sealed kernel semantic FreshOut relocates"),
                scope: self
                    .input
                    .construction
                    .relocate_scope(self.owner, KernelScopeReference::Local(scope))
                    .expect("sealed kernel semantic FreshOut scope relocates"),
            },
            crate::PackedCallEntry::ForwardOut {
                parameter_ordinal,
                target,
            } => KernelSemanticCallEntryRef::ForwardOut {
                parameter_ordinal,
                target: self
                    .input
                    .construction
                    .relocate_declaration(self.owner, target)
                    .expect("sealed kernel semantic ForwardOut target relocates"),
            },
        }
    }

    pub fn context_count(self) -> usize {
        self.code()
            .call_contexts(self.ordinal as usize)
            .expect("sealed kernel semantic call has a context span")
            .len()
    }

    pub fn context(self, ordinal: usize) -> Option<KernelSemanticCallContextRef> {
        self.code()
            .call_contexts(self.ordinal as usize)?
            .get(ordinal)
            .copied()
            .map(|context| self.relocate_context(context))
    }

    pub fn contexts(self) -> impl ExactSizeIterator<Item = KernelSemanticCallContextRef> + 'a {
        self.code()
            .call_contexts(self.ordinal as usize)
            .expect("sealed kernel semantic call has a context span")
            .iter()
            .copied()
            .map(move |context| self.relocate_context(context))
    }

    fn relocate_context(self, context: crate::PackedCallContext) -> KernelSemanticCallContextRef {
        KernelSemanticCallContextRef {
            declaration: self
                .input
                .construction
                .relocate_declaration(
                    self.owner,
                    KernelDeclarationReference::Local(context.declaration),
                )
                .expect("sealed kernel semantic call context declaration relocates"),
            signature_ordinal: context.signature_ordinal,
            scope: self
                .input
                .construction
                .relocate_scope(self.owner, context.scope)
                .expect("sealed kernel semantic call context scope relocates"),
        }
    }

    pub fn context_binding(self) -> CheckedContextBinding {
        match self
            .code()
            .call_context_binding(self.ordinal as usize)
            .expect("sealed kernel semantic call has a context binding")
        {
            crate::PackedCallContextBinding::None => CheckedContextBinding::None,
            crate::PackedCallContextBinding::Explicit { value, span } => {
                CheckedContextBinding::Explicit {
                    value: self
                        .input
                        .construction
                        .relocate_value(self.owner, value)
                        .expect("sealed kernel semantic explicit PASS value relocates"),
                    span: self
                        .input
                        .construction
                        .rebase_span(self.owner, span)
                        .expect("sealed kernel semantic explicit PASS span relocates"),
                }
            }
            crate::PackedCallContextBinding::Inherited {
                caller_formal_ordinal,
            } => CheckedContextBinding::Inherited {
                formal: self
                    .input
                    .definition_relocation(self.owner)
                    .filter(|relocation| {
                        relocation.context_formal_ordinal == Some(caller_formal_ordinal)
                    })
                    .and_then(|relocation| relocation.context_formal)
                    .expect("sealed kernel semantic inherited PASS formal relocates"),
            },
        }
    }

    pub fn result(self) -> KernelPackedFlowRef<'a> {
        self.type_facts().base_result()
    }

    /// Borrow the caller-owned packed result and substitution facts for this
    /// call without exposing the semantic input authority itself.
    pub fn type_facts(self) -> KernelDefinitionCallTypeFactsRef<'a> {
        self.input
            .call_type_facts(self.id())
            .expect("sealed kernel semantic call has packed type facts")
    }

    /// Final expression publication after occurrence-local widening.
    ///
    /// This is deliberately separate from [`Self::result`], which preserves
    /// the historical `CheckedCall::result` contract.
    pub fn published_result(self) -> KernelPackedFlowRef<'a> {
        self.type_facts().published_result()
    }

    pub fn syntax_discriminated_result(self) -> bool {
        self.code()
            .call_syntax_discriminated_result(self.ordinal as usize)
            .expect("sealed kernel semantic call has a syntax result flag")
    }

    pub fn span(self) -> CheckedSpan {
        let span = self
            .code()
            .call_span(self.ordinal as usize)
            .expect("sealed kernel semantic call has a source span");
        self.input
            .construction
            .rebase_span(self.owner, span)
            .expect("sealed kernel semantic call span relocates")
    }

    pub fn authored_site_digest_v4(self) -> [u8; 32] {
        self.code()
            .call_authored_site_digest_v4(self.ordinal as usize)
            .expect("sealed kernel semantic call has an authored-site digest")
    }
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
    fn local_row(
        &self,
        id: u32,
        range: impl Fn(&KernelSemanticDefinitionRelocationV1) -> KernelCheckedRowRange,
    ) -> Option<(KernelOwnerId, u32)> {
        let end = self
            .definition_relocations
            .partition_point(|relocation| range(relocation).start <= id);
        for owner in (0..end).rev() {
            let row_range = range(self.definition_relocations.get(owner)?);
            let local = id.checked_sub(row_range.start)?;
            if local < row_range.len {
                return Some((KernelOwnerId(u32::try_from(owner).ok()?), local));
            }
            // Empty definition-local ranges can share the next non-empty
            // range's start. Continue through that equal-start group, but once
            // the start precedes `id`, every earlier disjoint range ends no
            // later and therefore cannot own the row.
            if row_range.start < id {
                break;
            }
        }
        None
    }

    fn definition_relocation(
        &self,
        owner: KernelOwnerId,
    ) -> Option<&KernelSemanticDefinitionRelocationV1> {
        self.definition_relocations.get(owner.0 as usize)
    }

    fn abi_relocation(
        &self,
        callable: crate::KernelAbiCallableId,
    ) -> Option<&KernelSemanticAbiRelocationV1> {
        self.abi_relocations
            .get(callable.0 as usize)
            .and_then(Option::as_ref)
    }

    fn callable_declaration(&self, target: crate::KernelCallableSchemeId) -> Option<DeclId> {
        match target {
            crate::KernelCallableSchemeId::User(owner) => {
                let relocation = self.definition_relocation(owner)?;
                (relocation.owner_callable == Some(relocation.callable))
                    .then_some(relocation.callable)
            }
            crate::KernelCallableSchemeId::Abi(callable) => self
                .abi_relocation(callable)
                .map(|relocation| relocation.callable),
        }
    }

    fn callable_index(&self, declaration: DeclId) -> Option<usize> {
        self.callable_schemes
            .binary_search_by_key(&declaration.0, |target| {
                self.callable_declaration(*target)
                    .expect("sealed callable locator has a declaration")
                    .0
            })
            .ok()
    }

    fn relocate_expression(&self, expression: crate::PackedExpressionRef) -> Option<CheckedExprId> {
        let range = self.definition_relocation(expression.owner())?.expressions;
        range
            .resolve(expression.expression().0, "semantic expression")
            .ok()
            .map(CheckedExprId)
    }

    fn relocate_value(
        &self,
        owner: KernelOwnerId,
        value: KernelValueReference,
    ) -> Option<CheckedExprId> {
        let expression = match value {
            KernelValueReference::Local(expression) => {
                crate::PackedExpressionRef::new(owner, expression)
            }
            KernelValueReference::External(external) => match external.target {
                KernelExternalTarget::Expression(expression) => {
                    crate::PackedExpressionRef::new(external.owner, expression)
                }
                KernelExternalTarget::Result => {
                    return self
                        .definition_relocation(external.owner)
                        .map(|relocation| relocation.result_expression);
                }
            },
        };
        self.relocate_expression(expression)
    }

    fn relocate_scope(
        &self,
        owner: KernelOwnerId,
        scope: KernelScopeReference,
    ) -> Option<LexicalScopeId> {
        match scope {
            KernelScopeReference::ProjectRoot => Some(LexicalScopeId(0)),
            KernelScopeReference::Containing => self
                .definition_relocation(owner)
                .map(|relocation| relocation.containing_scope),
            KernelScopeReference::Local(scope) => self
                .definition_relocation(owner)?
                .scopes
                .resolve(scope.0, "semantic scope")
                .ok()
                .map(LexicalScopeId),
            KernelScopeReference::Owner {
                owner: target,
                scope,
            } => self
                .definition_relocation(target)?
                .scopes
                .resolve(scope.0, "semantic owner scope")
                .ok()
                .map(LexicalScopeId),
        }
    }

    fn relocate_declaration(
        &self,
        owner: KernelOwnerId,
        declaration: KernelDeclarationReference,
    ) -> Option<DeclId> {
        match declaration {
            KernelDeclarationReference::Local(declaration) => self
                .definition_relocation(owner)?
                .declarations
                .resolve(declaration.0, "semantic declaration")
                .ok()
                .map(DeclId),
            KernelDeclarationReference::OwnerPublic(target) => {
                self.definition_relocation(target).map(|row| row.callable)
            }
            KernelDeclarationReference::OwnerDeclaration {
                owner: target,
                declaration,
            } => self
                .definition_relocation(target)?
                .declarations
                .resolve(declaration.0, "semantic owner declaration")
                .ok()
                .map(DeclId),
        }
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
        let (owner, local) = self.local_row(expression.0, |relocation| relocation.expressions)?;
        Some(crate::PackedExpressionRef::new(
            owner,
            crate::KernelExpressionId(local),
        ))
    }

    fn local_declaration(&self, declaration: DeclId) -> Option<(KernelOwnerId, u32)> {
        self.local_row(declaration.0, |relocation| relocation.declarations)
    }

    fn local_scope(&self, scope: LexicalScopeId) -> Option<(KernelOwnerId, u32)> {
        if scope.0 == 0 {
            None
        } else {
            self.local_row(scope.0, |relocation| relocation.scopes)
        }
    }

    fn local_statement(&self, statement: CheckedStatementId) -> Option<(KernelOwnerId, u32)> {
        self.local_row(statement.0, |relocation| relocation.statements)
    }

    fn local_source(&self, source: CheckedSourceId) -> Option<(KernelOwnerId, u32)> {
        self.local_row(source.0, |relocation| relocation.sources)
    }

    fn local_state(&self, state: CheckedStateId) -> Option<(KernelOwnerId, u32)> {
        self.local_row(state.0, |relocation| relocation.states)
    }

    fn local_list(&self, list: CheckedListId) -> Option<(KernelOwnerId, u32)> {
        self.local_row(list.0, |relocation| relocation.lists)
    }

    fn local_call(&self, call: CheckedCallId) -> Option<crate::PackedCallRef> {
        let owner = self
            .definition_relocations
            .partition_point(|relocation| relocation.calls.start <= call.0)
            .checked_sub(1)?;
        let range = self.definition_relocations.get(owner)?.calls;
        let local = call.0.checked_sub(range.start)?;
        let owner = KernelOwnerId(u32::try_from(owner).ok()?);
        (local < range.len).then(|| crate::PackedCallRef::new(owner, local))
    }

    fn call_result_path_symbols(
        &self,
        path: KernelSemanticCallResultPathLocatorV1,
    ) -> Option<&[SymbolId]> {
        let start = path.projection_start as usize;
        let end = start.checked_add(path.projection_len as usize)?;
        self.call_result_path_symbols.get(start..end)
    }

    fn rebase_definition_spans(
        &mut self,
        owner: KernelOwnerId,
        start_line: usize,
        start_byte: usize,
    ) -> Result<(), KernelCheckedLinkError> {
        if start_line == 0 {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic span relocation has no source line",
            ));
        }
        let slot = self
            .definition_span_relocations
            .get_mut(owner.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic span relocation references missing definition {}",
                    owner.0,
                ))
            })?;
        if slot.start_line != 0 {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel semantic definition {} spans were rebased twice",
                owner.0,
            )));
        }
        let code = self.definition_code.definition(owner).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel semantic span relocation has no definition code {}",
                owner.0,
            ))
        })?;
        for ordinal in 0..code.call_count() {
            let mut span = checked_span(code.call_span(ordinal).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic definition {} omits packed call span {ordinal}",
                    owner.0,
                ))
            })?);
            rebase_checked_span(
                &mut span,
                start_line,
                start_byte,
                &format!("kernel semantic definition {} call {ordinal}", owner.0),
            )?;
            if let crate::PackedCallContextBinding::Explicit { span, .. } =
                code.call_context_binding(ordinal).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic definition {} omits packed call binding {ordinal}",
                        owner.0,
                    ))
                })?
            {
                let mut span = checked_span(span);
                rebase_checked_span(
                    &mut span,
                    start_line,
                    start_byte,
                    &format!("kernel semantic definition {} call {ordinal} PASS", owner.0),
                )?;
            }
        }
        *slot = KernelSemanticDefinitionSpanRelocationV1 {
            start_line,
            start_byte,
        };
        Ok(())
    }

    fn rebase_span(
        &self,
        owner: KernelOwnerId,
        span: crate::KernelSourceSpan,
    ) -> Option<CheckedSpan> {
        let relocation = self
            .definition_span_relocations
            .get(owner.0 as usize)
            .filter(|relocation| relocation.start_line != 0)?;
        Some(CheckedSpan {
            line: relocation
                .start_line
                .checked_add(span.line.checked_sub(1)?)?,
            start: relocation.start_byte.checked_add(span.start)?,
            end: relocation.start_byte.checked_add(span.end)?,
        })
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

    /// Number of calls owned by the packed definition-code authority.
    ///
    /// Runtime checked rows deliberately omit their rich call projection, so
    /// orchestration and sealing must use this count rather than infer it from
    /// `CheckedProgramFields::calls`.
    pub fn call_count(&self) -> usize {
        self.call_count as usize
    }

    /// Exact routed entity counts for the sibling compact checked-image seal.
    pub fn entity_counts(&self) -> KernelSemanticEntityCountsV1 {
        KernelSemanticEntityCountsV1 {
            scopes: self.scope_count as usize,
            declarations: self.declaration_count as usize,
            statements: self.statement_count as usize,
            expressions: self.expression_count as usize,
            callables: self.callable_count as usize,
            context_formals: self.context_formal_count as usize,
            calls: self.call_count as usize,
            pattern_bindings: self.pattern_binding_count as usize,
            sources: self.source_count as usize,
            states: self.state_count as usize,
            lists: self.list_count as usize,
            occurrences: self.occurrence_count as usize,
        }
    }

    /// Resolve one definition's authority scope from the packed relocation
    /// table without constructing a rich callable signature.
    pub fn definition_authority_root_scope(
        &self,
        owner: KernelOwnerId,
    ) -> Result<LexicalScopeId, KernelCheckedLinkError> {
        self.definition_relocation(owner)
            .map(|relocation| relocation.authority_root_scope)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic input references missing definition {}",
                    owner.0,
                ))
            })
    }

    /// Iterate user-callable declarations from the packed scheme locator.
    /// Runtime metadata publication needs only these dense IDs, never names,
    /// parameter vectors, or recursive rich types.
    pub fn user_callable_declarations(&self) -> impl Iterator<Item = DeclId> + '_ {
        self.callable_schemes.iter().filter_map(|scheme| {
            let crate::KernelCallableSchemeId::User(owner) = *scheme else {
                return None;
            };
            Some(
                self.definition_relocation(owner)
                    .expect("sealed user callable has a definition relocation")
                    .callable,
            )
        })
    }

    /// Take the independently derived ownership plan for one consuming
    /// typechecker validation. No full projection/route slab survives into
    /// the sealed semantic input.
    #[doc(hidden)]
    pub fn take_checked_image_ownership_expectation(
        &mut self,
    ) -> Result<CheckedImageKernelOwnershipExpectationV1, KernelCheckedLinkError> {
        self.checked_image_ownership_expectation
            .take()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked-image ownership expectation was already consumed",
                )
            })
    }

    /// Freeze the sibling ownership stores after the compiler has appended
    /// all metadata row counts. Subsequent publication mutation fails closed.
    #[doc(hidden)]
    pub fn __compiler_freeze_checked_image_ownership(
        &mut self,
        publication: &CheckedImageKernelPublicationV1,
    ) -> Result<(), KernelCheckedLinkError> {
        self.checked_image_ownership_expectation
            .as_mut()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked-image ownership expectation was already consumed",
                )
            })?
            .__kernel_freeze_against(publication)
            .map_err(KernelCheckedLinkError::new)
    }

    /// Resolve compiler-owned metadata through the sole compact ownership
    /// topology. The payload publication intentionally exposes no key or route
    /// lookup API.
    #[doc(hidden)]
    pub fn __compiler_checked_image_projection_for_route(
        &self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
    ) -> Result<Option<boon_checked::CheckedImageKernelProjectionIdV1>, KernelCheckedLinkError>
    {
        self.checked_image_ownership_expectation
            .as_ref()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked-image ownership expectation was already consumed",
                )
            })?
            .__compiler_projection_for_route(domain, dense_index)
            .map_err(KernelCheckedLinkError::new)
    }

    #[doc(hidden)]
    pub fn __compiler_checked_image_definition_projection(
        &self,
        projection: boon_checked::CheckedImageKernelProjectionIdV1,
    ) -> Result<Option<boon_checked::CheckedImageKernelProjectionIdV1>, KernelCheckedLinkError>
    {
        self.checked_image_ownership_expectation
            .as_ref()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked-image ownership expectation was already consumed",
                )
            })?
            .__compiler_definition_projection(projection)
            .map_err(KernelCheckedLinkError::new)
    }

    #[doc(hidden)]
    pub fn __compiler_checked_image_root_definition_projection(
        &self,
    ) -> Result<boon_checked::CheckedImageKernelProjectionIdV1, KernelCheckedLinkError> {
        self.checked_image_ownership_expectation
            .as_ref()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "kernel checked-image ownership expectation was already consumed",
                )
            })?
            .__compiler_root_definition_projection()
            .map_err(KernelCheckedLinkError::new)
    }

    pub const fn source_bundle_digest_v1(&self) -> SourceBundleDigestV1 {
        self.source_bundle_digest_v1
    }

    pub const fn role(&self) -> ProgramRole {
        self.role
    }

    /// Borrow exact resource expression/target routes without cloning their
    /// packed projection paths or required types.
    pub fn resource_route_pairs(
        &self,
    ) -> impl ExactSizeIterator<Item = (CheckedExprId, DeclId)> + '_ {
        self.resource_projections
            .iter()
            .map(|route| (route.expression, route.target))
    }

    fn from_linked_rows(
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        projection_demand: KernelCheckedRowProjectionDemand,
        snapshot: &KernelCheckedSnapshot,
        layout: &KernelCheckedLinkLayout,
        call_result_paths: Box<[KernelSemanticCallResultPathLocatorV1]>,
        call_result_path_symbols: Box<[SymbolId]>,
        pattern_bindings: Box<[KernelSemanticPatternBindingLocatorV1]>,
        resource_projections: Box<[KernelSemanticResourceProjectionLocatorV1]>,
        occurrence_count: usize,
        checked_image_ownership_expectation: CheckedImageKernelOwnershipExpectationV1,
        checked_image_pairing: Arc<boon_checked::CheckedImageKernelPairingV1>,
    ) -> Result<Self, KernelCheckedLinkError> {
        if snapshot.definition_count() != layout.definitions.len()
            || snapshot.definition_code.definition_count() != layout.definitions.len()
            || snapshot.program.definition_count() != layout.definitions.len()
        {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic input definition authorities disagree",
            ));
        }
        let mut definition_relocations = Vec::with_capacity(layout.definitions.len());
        let mut definition_execution_owners = Vec::new();
        let mut callable_schemes = Vec::with_capacity(layout.totals.callables as usize);
        for definition in &layout.definitions {
            let definition_view = snapshot.definition(definition.owner).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic input has no definition view {}",
                    definition.owner.0,
                ))
            })?;
            let code = snapshot
                .definition_code
                .definition(definition.owner)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic input has no packed definition {}",
                        definition.owner.0,
                    ))
                })?;
            let facts = code.runtime_facts();
            let owner_program = snapshot.program.owner(definition.owner).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic input has no packed owner program {}",
                    definition.owner.0,
                ))
            })?;
            for (label, packed, linked) in [
                ("scope", facts.scopes().len(), definition.scopes.len),
                (
                    "declaration",
                    code.declaration_count(),
                    definition.declarations.len,
                ),
                (
                    "statement",
                    facts.statements().len(),
                    definition.statements.len,
                ),
                (
                    "type variable",
                    code.alpha_variable_count(),
                    definition.type_variables.len,
                ),
                (
                    "expression",
                    owner_program.node_count(),
                    definition.expressions.len,
                ),
                (
                    "solved expression",
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
            let owner_callable = definition_view
                .linkage()
                .root_statement
                .and_then(|root| {
                    definition_view
                        .runtime_facts()
                        .statements()
                        .get(root.0 as usize)
                })
                .filter(|root| matches!(root.kind, crate::PackedStatementKind::Function { .. }))
                .map(|_| definition.public_declaration);
            if owner_callable.is_none()
                && definition_view.linkage().context_formal_ordinal.is_some()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel non-callable definition {} owns a context formal",
                    definition.owner.0,
                )));
            }
            if let Some(callable) = owner_callable {
                if callable != definition.public_declaration {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel semantic definition {} callable declaration differs from its public declaration",
                        definition.owner.0,
                    )));
                }
                let root = definition_view
                    .linkage()
                    .root_statement
                    .expect("callable classification requires a root statement");
                let root = definition_view
                    .runtime_facts()
                    .statements()
                    .get(root.0 as usize)
                    .expect("validated callable root statement exists");
                let crate::PackedStatementKind::Function {
                    name: function_name,
                    ..
                } = root.kind
                else {
                    unreachable!("callable classification requires a function root")
                };
                let KernelDeclarationReference::Local(public_declaration) = definition_view
                    .linkage()
                    .public_declaration
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel callable definition {} has no public declaration",
                            definition.owner.0,
                        ))
                    })?
                else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} delegates its public declaration",
                        definition.owner.0,
                    )));
                };
                if layout.declaration(
                    definition.owner,
                    KernelDeclarationReference::Local(public_declaration),
                )? != callable
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} public declaration does not relocate to {}",
                        definition.owner.0, callable.0,
                    )));
                }
                let public_row = facts
                    .declarations()
                    .get(public_declaration.0 as usize)
                    .filter(|row| {
                        row.id == public_declaration
                            && row.kind == crate::KernelDeclarationKind::Function
                    })
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel callable definition {} has no exact function declaration {}",
                            definition.owner.0, public_declaration.0,
                        ))
                    })?;
                if public_row.name != function_name {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} declaration name differs from its function header",
                        definition.owner.0,
                    )));
                }
                let declaration_body_scope = declaration_presentation(facts, public_declaration)?
                    .body_scope
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel callable definition {} has no declaration body scope",
                            definition.owner.0,
                        ))
                    })?;
                if statement_presentation(facts, root.id)?.body_scope
                    != Some(declaration_body_scope)
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel callable definition {} function declaration and statement disagree on body scope",
                        definition.owner.0,
                    )));
                }
                let parameters = definition_view
                    .runtime_facts()
                    .statement_parameters(root)
                    .expect("function root owns a parameter span");
                for ordinal in 0..parameters.len() {
                    let ordinal = u32::try_from(ordinal).map_err(|_| {
                        KernelCheckedLinkError::new("kernel callable parameter count exceeds u32")
                    })?;
                    let parameter = parameters
                        .get(ordinal as usize)
                        .filter(|parameter| parameter.ordinal == ordinal)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel semantic callable {} does not have parameter ordinal {ordinal} in canonical order",
                                callable.0,
                            ))
                        })?;
                    let expected_kind = match parameter.kind {
                        crate::KernelParameterKind::Value => {
                            crate::KernelDeclarationKind::ValueParameter
                        }
                        crate::KernelParameterKind::Out => {
                            crate::KernelDeclarationKind::OutParameter
                        }
                    };
                    facts
                        .declarations()
                        .get(parameter.declaration.0 as usize)
                        .filter(|declaration| {
                            declaration.id == parameter.declaration
                                && declaration.origin
                                    == crate::KernelDeclarationOrigin::Parameter {
                                        statement: root.id,
                                        ordinal,
                                    }
                                && declaration.kind == expected_kind
                                && declaration.name == parameter.name
                        })
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel semantic callable {} parameter ordinal {ordinal} has no exact declaration",
                                callable.0,
                            ))
                        })?;
                    if let crate::KernelParameterEvaluationScope::Output { parameter_ordinal } =
                        parameter.evaluation_scope
                    {
                        let output = parameters
                            .get(parameter_ordinal as usize)
                            .filter(|output| {
                                output.ordinal == parameter_ordinal
                                    && output.kind == crate::KernelParameterKind::Out
                            })
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel semantic callable {} parameter ordinal {ordinal} targets missing or non-OUT parameter {parameter_ordinal}",
                                    callable.0,
                                ))
                            })?;
                        if facts
                            .declarations()
                            .get(output.declaration.0 as usize)
                            .is_none_or(|candidate| {
                                candidate.id != output.declaration
                                    || candidate.kind != crate::KernelDeclarationKind::OutParameter
                            })
                        {
                            return Err(KernelCheckedLinkError::new(format!(
                                "kernel semantic callable {} output parameter {parameter_ordinal} has no exact OUT declaration",
                                callable.0,
                            )));
                        }
                    }
                }
                let context_count =
                    usize::from(definition_view.linkage().context_formal_ordinal.is_some());
                if code.formals().len() != parameters.len().saturating_add(context_count)
                    || definition_view
                        .linkage()
                        .context_formal_ordinal
                        .is_some_and(|ordinal| ordinal as usize != parameters.len())
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel semantic callable {} parameter/context rows disagree with its {} packed formals",
                        callable.0,
                        code.formals().len(),
                    )));
                }
                callable_schemes.push(crate::KernelCallableSchemeId::User(definition.owner));
            }
            let mut authority_root_scope = None;
            for provider in snapshot.definition_refs() {
                let provider_layout = layout.definition(provider.owner())?;
                let declaration = definition.public_declaration.0;
                let end = provider_layout
                    .declarations
                    .start
                    .checked_add(provider_layout.declarations.len)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "kernel semantic declaration relocation range overflows u32",
                        )
                    })?;
                if declaration < provider_layout.declarations.start || declaration >= end {
                    continue;
                }
                let local = declaration - provider_layout.declarations.start;
                let presentation = provider
                    .runtime_facts()
                    .declaration_presentations()
                    .get(local as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel semantic public declaration {} has no presentation",
                            definition.public_declaration.0,
                        ))
                    })?;
                let scope = if owner_callable.is_some() {
                    presentation.body_scope.ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel semantic callable declaration {} has no body scope",
                            definition.public_declaration.0,
                        ))
                    })?
                } else {
                    match presentation.scope {
                        KernelScopeReference::Local(scope) => scope,
                        scope => {
                            authority_root_scope = Some(layout.scope(provider.owner(), scope)?);
                            break;
                        }
                    }
                };
                authority_root_scope =
                    Some(layout.scope(provider.owner(), KernelScopeReference::Local(scope))?);
                break;
            }
            let authority_root_scope = authority_root_scope.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic definition {} has no authority root scope for declaration {}",
                    definition.owner.0, definition.public_declaration.0,
                ))
            })?;
            definition_relocations.push(KernelSemanticDefinitionRelocationV1 {
                callable: definition.public_declaration,
                authority_root_scope,
                owner_callable,
                context_formal: definition.context_formal,
                context_formal_ordinal: definition_view.linkage().context_formal_ordinal,
                result_expression: definition.result_expression,
                containing_scope: definition.containing_scope,
                scopes: definition.scopes,
                declarations: definition.declarations,
                statements: definition.statements,
                type_variables: definition.type_variables,
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
        let mut abi_relocations = vec![None; snapshot.definition_code.abi_callable_count()];
        for callable in &layout.abi_callables {
            let slot = abi_relocations
                .get_mut(callable.callable.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic ABI relocation {} is outside its packed scheme table",
                        callable.callable.0,
                    ))
                })?;
            if slot.is_some() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic input repeats ABI relocation {}",
                    callable.callable.0,
                )));
            }
            let scheme = snapshot
                .definition_code
                .abi_callable_scheme(callable.callable)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel semantic input has no packed ABI scheme {}",
                        callable.callable.0,
                    ))
                })?;
            if scheme.callable() != callable.callable {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic ABI scheme {} has mismatched identity {}",
                    callable.callable.0,
                    scheme.callable().0,
                )));
            }
            if callable.type_variables.len != scheme.variable_count() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic ABI scheme {} has {} linked variables for {} packed variables",
                    callable.callable.0,
                    callable.type_variables.len,
                    scheme.variable_count(),
                )));
            }
            for parameter in scheme.type_parameters() {
                let expected_local = parameter
                    .source
                    .0
                    .checked_sub(scheme.variable_base().0)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel semantic ABI scheme {} parameter {} precedes variable base {}",
                            callable.callable.0,
                            parameter.source.0,
                            scheme.variable_base().0,
                        ))
                    })?;
                if expected_local != parameter.linked_local
                    || parameter.linked_local >= callable.type_variables.len
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel semantic ABI scheme {} parameter {} maps to invalid linked alpha {} in a {}-row namespace",
                        callable.callable.0,
                        parameter.source.0,
                        parameter.linked_local,
                        callable.type_variables.len,
                    )));
                }
            }
            *slot = Some(KernelSemanticAbiRelocationV1 {
                callable: callable.declaration,
                parameters: callable.parameters,
                type_variables: callable.type_variables,
            });
            callable_schemes.push(crate::KernelCallableSchemeId::Abi(callable.callable));
        }
        if callable_schemes.len() != layout.totals.callables as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel semantic callable locator has {} rows for a {}-row layout",
                callable_schemes.len(),
                layout.totals.callables,
            )));
        }
        let callable_declaration = |target: crate::KernelCallableSchemeId| match target {
            crate::KernelCallableSchemeId::User(owner) => definition_relocations
                .get(owner.0 as usize)
                .map(|relocation| relocation.callable),
            crate::KernelCallableSchemeId::Abi(callable) => abi_relocations
                .get(callable.0 as usize)
                .and_then(Option::as_ref)
                .map(|relocation| relocation.callable),
        };
        for pair in callable_schemes.windows(2) {
            let left = callable_declaration(pair[0]).ok_or_else(|| {
                KernelCheckedLinkError::new("kernel semantic callable locator has no declaration")
            })?;
            let right = callable_declaration(pair[1]).ok_or_else(|| {
                KernelCheckedLinkError::new("kernel semantic callable locator has no declaration")
            })?;
            if left >= right {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic callable declarations are not strictly ordered: {} then {}",
                    left.0, right.0,
                )));
            }
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
            program: Arc::clone(&snapshot.program),
            definition_code: Arc::clone(&snapshot.definition_code),
            scope_count: layout.totals.scopes,
            expression_count: layout.totals.expressions,
            declaration_count: layout.totals.declarations.saturating_sub(1),
            statement_count: layout.totals.statements,
            callable_count: layout.totals.callables,
            context_formal_count: layout.totals.context_formals,
            call_count: layout.totals.calls,
            source_count: layout.totals.sources,
            state_count: layout.totals.states,
            list_count: layout.totals.lists,
            pattern_binding_count: u32::try_from(pattern_bindings.len()).map_err(|_| {
                KernelCheckedLinkError::new("kernel semantic pattern-binding count exceeds u32")
            })?,
            occurrence_count: u32::try_from(occurrence_count).map_err(|_| {
                KernelCheckedLinkError::new("kernel semantic occurrence count exceeds u32")
            })?,
            definition_relocations: definition_relocations.into_boxed_slice(),
            definition_span_relocations: vec![
                KernelSemanticDefinitionSpanRelocationV1::default();
                definition_count
            ]
            .into_boxed_slice(),
            abi_relocations: abi_relocations.into_boxed_slice(),
            callable_schemes: callable_schemes.into_boxed_slice(),
            definition_execution_owners: definition_execution_owners.into_boxed_slice(),
            call_result_paths,
            call_result_path_symbols,
            pattern_bindings,
            resource_projections,
            resource_projection_by_expression: resource_projection_by_expression.into_boxed_slice(),
            rich_editor_projection_expected: matches!(
                projection_demand,
                KernelCheckedRowProjectionDemand::EditorRich
            ),
            checked_image_ownership_expectation: Some(checked_image_ownership_expectation),
            checked_image_pairing,
        })
    }

    /// Debug-build firewall between the permanent packed topology and the
    /// compatibility `CheckedCall` graph. Production release compilation does
    /// not pay for this replay; editor/oracle tests exercise it before the
    /// rich owner is deleted from RuntimePacked.
    #[cfg(debug_assertions)]
    fn validate_rich_call_topology(
        &self,
        calls: &[CheckedCall],
        occurrences: &[StableOccurrenceKey],
        callables: &[CheckedCallableSignature],
        declarations: &[CheckedDeclaration],
    ) -> Result<(), KernelCheckedLinkError> {
        if calls.len() != self.call_count as usize || occurrences.len() != calls.len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel packed call topology has {} rows but rich calls/occurrences have {}/{}",
                self.call_count,
                calls.len(),
                occurrences.len(),
            )));
        }
        for (index, (rich, occurrence)) in calls.iter().zip(occurrences).enumerate() {
            let id = CheckedCallId(u32::try_from(index).map_err(|_| {
                KernelCheckedLinkError::new("kernel packed call topology count exceeds u32")
            })?);
            if rich.id != id {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel rich call {} is not dense at index {index}",
                    rich.id.0,
                )));
            }
            let local = self.local_call(id).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel packed call topology has no local coordinate for call {}",
                    id.0,
                ))
            })?;
            let code = self
                .definition_code
                .definition(local.owner())
                .expect("local call always has definition code");
            let ordinal = local.ordinal() as usize;
            let expression = code.call_expression(ordinal).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel packed call topology omits call {} expression",
                    id.0,
                ))
            })?;
            if self.relocate_expression(crate::PackedExpressionRef::new(local.owner(), expression))
                != Some(rich.expression)
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel packed call {} expression differs from rich topology",
                    id.0,
                )));
            }
            let relocation = self
                .definition_relocation(local.owner())
                .expect("local call always has a relocation");
            if relocation.owner_callable != rich.owner_callable
                || code
                    .call_function(ordinal)
                    .and_then(|symbol| self.definition_code.symbol(symbol))
                    != Some(rich.function.as_str())
                || checked_span(code.call_span(ordinal).expect("sealed call has span")) != rich.span
                || code.call_syntax_discriminated_result(ordinal)
                    != Some(rich.syntax_discriminated_result)
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel packed call {} header differs from rich topology",
                    id.0,
                )));
            }
            let target = code.call_target(ordinal).flatten().ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel packed call {} has no target scheme",
                    id.0,
                ))
            })?;
            let callable = match target {
                crate::KernelCallableSchemeId::User(owner) => {
                    self.definition_relocation(owner).map(|row| row.callable)
                }
                crate::KernelCallableSchemeId::Abi(callable) => {
                    self.abi_relocation(callable).map(|row| row.callable)
                }
            }
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel packed call {} target has no relocation",
                    id.0,
                ))
            })?;
            let signature = callables
                .iter()
                .find(|candidate| candidate.decl_id == callable)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel packed call {} target {} has no signature",
                        id.0, callable.0,
                    ))
                })?;
            if rich.callable != callable
                || rich.function != signature.name
                || rich.entries.len()
                    != code
                        .call_entries(ordinal)
                        .expect("sealed call has entries")
                        .len()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel packed call {} target or entry count differs from rich topology",
                    id.0,
                )));
            }
            for (entry_index, (packed, rich_entry)) in code
                .call_entries(ordinal)
                .expect("sealed call has entries")
                .iter()
                .copied()
                .zip(&rich.entries)
                .enumerate()
            {
                let parameter = signature
                    .parameters
                    .get(packed.parameter_ordinal() as usize)
                    .filter(|parameter| parameter.ordinal == packed.parameter_ordinal() as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel packed call {} entry {entry_index} has no target parameter {}",
                            id.0,
                            packed.parameter_ordinal(),
                        ))
                    })?;
                let matches = match (packed, rich_entry) {
                    (
                        crate::PackedCallEntry::Input {
                            value, from_pipe, ..
                        },
                        CheckedCallEntry::Input {
                            formal,
                            name,
                            value: rich_value,
                            from_pipe: rich_pipe,
                            evaluation_scope,
                        },
                    ) => {
                        *formal == parameter.decl_id
                            && name == &parameter.name
                            && self.relocate_value(local.owner(), value) == Some(*rich_value)
                            && from_pipe == *rich_pipe
                            && *evaluation_scope == parameter.evaluation_scope
                    }
                    (
                        crate::PackedCallEntry::FreshOut { output, scope, .. },
                        CheckedCallEntry::FreshOut {
                            formal,
                            name,
                            output: rich_output,
                            scope_id,
                        },
                    ) => {
                        *formal == parameter.decl_id
                            && name == &parameter.name
                            && self.relocate_declaration(
                                local.owner(),
                                KernelDeclarationReference::Local(output),
                            ) == Some(*rich_output)
                            && self
                                .relocate_scope(local.owner(), KernelScopeReference::Local(scope))
                                == Some(*scope_id)
                    }
                    (
                        crate::PackedCallEntry::ForwardOut { target, .. },
                        CheckedCallEntry::ForwardOut {
                            formal,
                            name,
                            target: rich_target,
                            target_name,
                        },
                    ) => {
                        let target = self.relocate_declaration(local.owner(), target);
                        *formal == parameter.decl_id
                            && name == &parameter.name
                            && target == Some(*rich_target)
                            && declarations
                                .iter()
                                .find(|declaration| Some(declaration.id) == target)
                                .is_some_and(|declaration| declaration.name == *target_name)
                    }
                    _ => false,
                };
                if !matches {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel packed call {} entry {entry_index} differs from rich topology",
                        id.0,
                    )));
                }
            }
            let packed_contexts = code
                .call_contexts(ordinal)
                .expect("sealed call has contexts");
            if packed_contexts.len() != rich.contexts.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel packed call {} context count differs from rich topology",
                    id.0,
                )));
            }
            for (packed, rich_context) in packed_contexts.iter().zip(&rich.contexts) {
                if self.relocate_declaration(
                    local.owner(),
                    KernelDeclarationReference::Local(packed.declaration),
                ) != Some(rich_context.declaration)
                    || packed.signature_ordinal as usize != rich_context.signature
                    || self.relocate_scope(local.owner(), packed.scope)
                        != Some(rich_context.scope_id)
                {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel packed call {} context {} differs from rich topology",
                        id.0, packed.signature_ordinal,
                    )));
                }
            }
            let packed_binding = match code
                .call_context_binding(ordinal)
                .expect("sealed call has context binding")
            {
                crate::PackedCallContextBinding::None => CheckedContextBinding::None,
                crate::PackedCallContextBinding::Explicit { value, span } => {
                    CheckedContextBinding::Explicit {
                        value: self
                            .relocate_value(local.owner(), value)
                            .expect("sealed explicit PASS value relocates"),
                        span: checked_span(span),
                    }
                }
                crate::PackedCallContextBinding::Inherited {
                    caller_formal_ordinal,
                } => CheckedContextBinding::Inherited {
                    formal: (relocation.context_formal_ordinal == Some(caller_formal_ordinal))
                        .then_some(relocation.context_formal)
                        .flatten()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel packed call {} inherits a mismatched context formal {caller_formal_ordinal}",
                                id.0,
                            ))
                        })?,
                },
            };
            if packed_binding != rich.context_binding {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel packed call {} PASS binding differs from rich topology",
                    id.0,
                )));
            }
            #[cfg(debug_assertions)]
            if code.call_authored_site_digest_v4(ordinal)
                != Some(
                    boon_checked::checked_structural_call_site_digest_v4(occurrence)
                        .map_err(KernelCheckedLinkError::new)?,
                )
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel packed call {} authored identity differs from rich topology",
                    id.0,
                )));
            }
        }
        Ok(())
    }

    pub fn seal(
        self,
        checked: &CheckedProgram,
        pairing_receipt: &boon_checked::CheckedImageKernelPairingReceiptV1,
    ) -> Result<KernelSemanticInputV1, KernelCheckedLinkError> {
        if self.checked_image_ownership_expectation.is_some() {
            return Err(KernelCheckedLinkError::new(
                "typechecker did not consume the checked-image ownership topology",
            ));
        }
        self.validate_checked_shape(
            checked.source_bundle_digest_v1,
            checked.role,
            checked.expressions.len(),
            checked.declarations.len(),
            checked.sources.len(),
        )?;
        let handoff = checked.image_handoff();
        self.validate_checked_handoff(handoff, pairing_receipt)?;
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

    /// Bind the packed semantic authority directly to the compact runtime
    /// checked capability.
    ///
    /// This path never constructs or borrows `CheckedProgramFields`. The
    /// typechecker has already consumed the sibling publication and validated
    /// every routed domain; this final boundary proves construction identity,
    /// runtime-flow authority, resource relocations, and source-span
    /// relocation before the semantic rows become visible.
    pub fn seal_runtime(
        self,
        checked: &boon_checked::RuntimePackedCheckedSealV1,
    ) -> Result<KernelSemanticInputV1, KernelCheckedLinkError> {
        if self.checked_image_ownership_expectation.is_some() {
            return Err(KernelCheckedLinkError::new(
                "typechecker did not consume the checked-image ownership topology",
            ));
        }
        let handoff = checked.image_handoff();
        if self.source_bundle_digest_v1 != handoff.source_bundle_digest_v1 {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic input source digest differs from compact checked image",
            ));
        }
        if self.role != handoff.role {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic input role differs from compact checked image",
            ));
        }
        if checked.runtime_flow_terms().expression_count() != self.expression_count as usize {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel semantic input has {} expressions but compact runtime-flow authority has {}",
                self.expression_count,
                checked.runtime_flow_terms().expression_count(),
            )));
        }
        checked
            .runtime_flow_terms()
            .validate_authority(
                self.source_bundle_digest_v1,
                self.role,
                handoff.local_image_digest,
            )
            .map_err(KernelCheckedLinkError::new)?;
        self.validate_runtime_checked_handoff(handoff, checked.pairing_receipt())?;
        Ok(KernelSemanticInputV1 {
            construction: self,
            checked_image_digest: handoff.local_image_digest,
        })
    }

    fn validate_checked_handoff(
        &self,
        handoff: &boon_checked::CheckedImageHandoffV4,
        pairing_receipt: &boon_checked::CheckedImageKernelPairingReceiptV1,
    ) -> Result<(), KernelCheckedLinkError> {
        pairing_receipt
            .__kernel_validate(&self.checked_image_pairing, handoff)
            .map_err(KernelCheckedLinkError::new)?;
        self.validate_checked_handoff_contents(handoff)
    }

    fn validate_runtime_checked_handoff(
        &self,
        handoff: &boon_checked::CheckedImageHandoffV4,
        pairing_receipt: &boon_checked::CheckedImageKernelPairingReceiptV1,
    ) -> Result<(), KernelCheckedLinkError> {
        pairing_receipt
            .__kernel_validate_sealed(&self.checked_image_pairing, handoff)
            .map_err(KernelCheckedLinkError::new)?;
        self.validate_checked_handoff_contents(handoff)
    }

    fn validate_checked_handoff_contents(
        &self,
        handoff: &boon_checked::CheckedImageHandoffV4,
    ) -> Result<(), KernelCheckedLinkError> {
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
        self.validate_definition_span_relocations()?;
        Ok(())
    }

    fn validate_definition_span_relocations(&self) -> Result<(), KernelCheckedLinkError> {
        for (owner, relocation) in self.definition_span_relocations.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner).map_err(|_| {
                KernelCheckedLinkError::new("kernel semantic definition count exceeds u32")
            })?);
            let code = self.definition_code.definition(owner).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic span relocation has no definition code {}",
                    owner.0,
                ))
            })?;
            let definition = crate::KernelDefinitionRef::from_authorities(
                &self.program,
                &self.definition_code,
                owner,
            )
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel semantic span relocation has no definition authority {}",
                    owner.0,
                ))
            })?;
            let owns_callable = definition
                .linkage()
                .root_statement
                .and_then(|root| code.runtime_facts().statements().get(root.0 as usize))
                .is_some_and(|root| {
                    matches!(root.kind, crate::PackedStatementKind::Function { .. })
                });
            if (code.call_count() != 0 || owns_callable) && relocation.start_line == 0 {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel semantic definition {} has source-bearing semantic rows but no installed source-span relocation",
                    owner.0,
                )));
            }
        }
        Ok(())
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
    /// Revalidate that a consumed RuntimePacked program is the exact sibling
    /// of this already sealed semantic input. The expensive route proof ran at
    /// `seal_runtime`; this final boundary is an O(1) pointer/digest check that
    /// prevents independently sealed same-shaped capabilities from swapping.
    #[doc(hidden)]
    pub fn validate_runtime_checked_identity(
        &self,
        handoff: &boon_checked::CheckedImageHandoffV4,
        pairing_receipt: &boon_checked::CheckedImageKernelPairingReceiptV1,
    ) -> Result<(), KernelCheckedLinkError> {
        if self.checked_image_digest != handoff.local_image_digest {
            return Err(KernelCheckedLinkError::new(
                "kernel semantic input and RuntimePacked program have different checked images",
            ));
        }
        pairing_receipt
            .__kernel_validate_identity(&self.construction.checked_image_pairing, handoff)
            .map_err(KernelCheckedLinkError::new)
    }

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

    /// Borrow the synthetic project-root scope without constructing a rich
    /// checked scope row.
    pub fn project_root_scope(&self) -> KernelSemanticScopeRef<'_> {
        KernelSemanticScopeRef {
            definition: None,
            ordinal: 0,
        }
    }

    pub const fn source_bundle_digest_v1(&self) -> SourceBundleDigestV1 {
        self.construction.source_bundle_digest_v1
    }

    pub const fn role(&self) -> ProgramRole {
        self.construction.role
    }

    pub fn entity_counts(&self) -> KernelSemanticEntityCountsV1 {
        self.construction.entity_counts()
    }

    /// Iterate callable schemes in the exact dense order used by semantic
    /// callable IDs. The iterator borrows one flat locator slab and allocates
    /// nothing.
    pub fn callables(&self) -> KernelCallableSchemeIter<'_> {
        KernelCallableSchemeIter {
            input: self,
            schemes: self.construction.callable_schemes.iter(),
        }
    }

    pub fn callable_count(&self) -> usize {
        self.construction.callable_schemes.len()
    }

    pub fn callable_at(&self, index: usize) -> Option<KernelCallableSchemeRef<'_>> {
        Some(self.callable_scheme_ref(*self.construction.callable_schemes.get(index)?))
    }

    pub fn callable(&self, declaration: DeclId) -> Option<KernelCallableSchemeRef<'_>> {
        self.callable_at(self.callable_index(declaration)?)
    }

    pub fn callable_index(&self, declaration: DeclId) -> Option<usize> {
        self.construction.callable_index(declaration)
    }

    /// Resolve a final checked declaration without allocating a rich row.
    pub fn declaration(
        &self,
        declaration: DeclId,
    ) -> Option<KernelSemanticResolvedDeclarationRef<'_>> {
        if declaration.0 == 0 {
            return None;
        }
        if let Some((owner, ordinal)) = self.construction.local_declaration(declaration) {
            return self
                .definition_rows(owner)?
                .declaration(ordinal as usize)
                .map(KernelSemanticResolvedDeclarationRef::Definition);
        }

        // All ABI declarations follow the definition-owned declaration ranges.
        // The last callable declaration not after this ID is therefore the only
        // ABI scheme that can own it or its contiguous parameter range.
        let end = self
            .construction
            .callable_schemes
            .partition_point(|target| {
                self.construction
                    .callable_declaration(*target)
                    .is_some_and(|candidate| candidate <= declaration)
            });
        let target = *self
            .construction
            .callable_schemes
            .get(end.checked_sub(1)?)?;
        let crate::KernelCallableSchemeId::Abi(abi) = target else {
            return None;
        };
        let callable = self.callable_scheme_ref(target);
        let relocation = self.construction.abi_relocation(abi)?;
        if declaration == relocation.callable {
            return Some(KernelSemanticResolvedDeclarationRef::AbiCallable(callable));
        }
        let local = declaration.0.checked_sub(relocation.parameters.start)?;
        (local < relocation.parameters.len).then_some(
            KernelSemanticResolvedDeclarationRef::AbiParameter(callable.parameter(local as usize)?),
        )
    }

    /// Resolve one final checked scope directly into its definition-local
    /// packed row. Scope zero is the synthetic project root and owns no row.
    pub fn scope(&self, scope: LexicalScopeId) -> Option<KernelSemanticScopeRef<'_>> {
        if scope.0 == 0 {
            return Some(self.project_root_scope());
        }
        let (owner, ordinal) = self.construction.local_scope(scope)?;
        self.definition_rows(owner)?.scope(ordinal as usize)
    }

    pub fn statement(
        &self,
        statement: CheckedStatementId,
    ) -> Option<KernelSemanticStatementRef<'_>> {
        let (owner, ordinal) = self.construction.local_statement(statement)?;
        self.definition_rows(owner)?.statement(ordinal as usize)
    }

    pub fn expression(&self, expression: CheckedExprId) -> Option<KernelSemanticExpressionRef<'_>> {
        let expression = self.construction.local_expression(expression)?;
        self.definition_rows(expression.owner())?
            .expression(expression.expression().0 as usize)
    }

    pub fn source(&self, source: CheckedSourceId) -> Option<KernelSemanticSourceRef<'_>> {
        let (owner, ordinal) = self.construction.local_source(source)?;
        self.definition_rows(owner)?.source(ordinal as usize)
    }

    pub fn state(&self, state: CheckedStateId) -> Option<KernelSemanticStateRef<'_>> {
        let (owner, ordinal) = self.construction.local_state(state)?;
        self.definition_rows(owner)?.state(ordinal as usize)
    }

    pub fn list(&self, list: CheckedListId) -> Option<KernelSemanticListRef<'_>> {
        let (owner, ordinal) = self.construction.local_list(list)?;
        self.definition_rows(owner)?.list(ordinal as usize)
    }

    /// Iterate definitions in their sealed dense `KernelOwnerId` order.
    pub fn definitions(&self) -> KernelSemanticDefinitionRowsIter<'_> {
        KernelSemanticDefinitionRowsIter {
            input: self,
            next: 0,
        }
    }

    /// Borrow all semantic rows owned by one packed definition.
    pub fn definition_rows(
        &self,
        owner: KernelOwnerId,
    ) -> Option<KernelSemanticDefinitionRowsRef<'_>> {
        self.definition_relocation(owner)?;
        self.construction.program.owner(owner)?;
        self.construction.definition_code.definition(owner)?;
        Some(KernelSemanticDefinitionRowsRef { input: self, owner })
    }

    fn lexical_declaration_for_scope(
        &self,
        owner: KernelOwnerId,
        scope: KernelScopeReference,
    ) -> Option<DeclId> {
        let mut owner = owner;
        let mut scope = scope;
        let mut remaining = (self.construction.scope_count as usize)
            .saturating_add(self.definition_count())
            .saturating_add(1);
        loop {
            assert!(
                remaining != 0,
                "sealed kernel semantic lexical scopes remain acyclic"
            );
            remaining -= 1;
            match scope {
                KernelScopeReference::ProjectRoot => return None,
                KernelScopeReference::Containing => {
                    scope = self
                        .definition_rows(owner)
                        .expect("sealed semantic containing definition exists")
                        .definition()
                        .runtime_facts()
                        .containing_scope();
                }
                KernelScopeReference::Owner {
                    owner: provider,
                    scope: provider_scope,
                } => {
                    owner = provider;
                    scope = KernelScopeReference::Local(provider_scope);
                }
                KernelScopeReference::Local(local) => {
                    let definition = self
                        .definition_rows(owner)
                        .expect("sealed semantic lexical-scope definition exists");
                    let row = definition
                        .definition()
                        .runtime_facts()
                        .scopes()
                        .get(local.0 as usize)
                        .expect("sealed semantic lexical-scope row exists");
                    if let Some(declaration) = row.owner {
                        return Some(
                            self.construction
                                .relocate_declaration(owner, declaration)
                                .expect("sealed semantic lexical-scope owner relocates"),
                        );
                    }
                    scope = row.parent;
                }
            }
        }
    }

    fn local_statement_reference(
        &self,
        owner: KernelOwnerId,
        statement: KernelStatementReference,
    ) -> Option<(KernelOwnerId, crate::KernelStatementId)> {
        match statement {
            KernelStatementReference::Local(statement) => Some((owner, statement)),
            KernelStatementReference::OwnerPublic(owner) => Some((
                owner,
                self.definition_rows(owner)?
                    .definition()
                    .linkage()
                    .root_statement?,
            )),
        }
    }

    fn relocate_statement(
        &self,
        owner: KernelOwnerId,
        statement: KernelStatementReference,
    ) -> Option<CheckedStatementId> {
        let (owner, statement) = self.local_statement_reference(owner, statement)?;
        self.definition_relocation(owner)?
            .statements
            .resolve(statement.0, "semantic statement")
            .ok()
            .map(CheckedStatementId)
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

    pub fn call_count(&self) -> usize {
        self.construction.call_count as usize
    }

    /// Iterate the one packed call-topology authority in final checked order.
    pub fn calls(&self) -> KernelSemanticCallIter<'_> {
        KernelSemanticCallIter {
            input: self,
            next: 0,
        }
    }

    pub fn call(&self, call: CheckedCallId) -> Option<KernelSemanticCallRef<'_>> {
        let call = self.construction.local_call(call)?;
        Some(KernelSemanticCallRef {
            input: self,
            owner: call.owner(),
            ordinal: call.ordinal(),
        })
    }

    fn packed_type_ref(
        &self,
        scope: KernelPackedTypeScope,
        term: crate::TypeTermId,
    ) -> KernelPackedTypeRef<'_> {
        KernelPackedTypeRef {
            input: self,
            scope,
            term,
        }
    }

    fn definition_type_ref(
        &self,
        owner: KernelOwnerId,
        term: crate::TypeTermId,
    ) -> KernelPackedTypeRef<'_> {
        self.packed_type_ref(KernelPackedTypeScope::Definition(owner), term)
    }

    fn callable_scheme_ref(
        &self,
        target: crate::KernelCallableSchemeId,
    ) -> KernelCallableSchemeRef<'_> {
        KernelCallableSchemeRef {
            input: self,
            target,
        }
    }

    /// Create one explicit rich-type compatibility projector for this input.
    /// Hot semantic consumers should prefer the borrowed structural views.
    #[doc(hidden)]
    pub fn compatibility_type_materializer(&self) -> KernelSemanticTypeMaterializer<'_> {
        KernelSemanticTypeMaterializer {
            input: self,
            cache: self.construction.definition_code.materialization_cache(),
            scope: None,
            variables: BTreeMap::new(),
            next: 0,
            alpha_end: 0,
        }
    }

    /// Borrow the checked occurrence flow directly from the packed definition
    /// store. Resource-required publication overrides are already reflected.
    pub fn published_expression_flow(
        &self,
        expression: CheckedExprId,
    ) -> Option<KernelPackedFlowRef<'_>> {
        let expression = self.construction.local_expression(expression)?;
        let flow = self
            .construction
            .definition_code
            .definition(expression.owner())?
            .published_expression(expression.expression().0 as usize)?;
        Some(KernelPackedFlowRef {
            mode: flow.mode,
            ty: self.definition_type_ref(expression.owner(), flow.term),
        })
    }

    /// Borrow the occurrence flow before resource publication overrides.
    pub fn base_expression_flow(
        &self,
        expression: CheckedExprId,
    ) -> Option<KernelPackedFlowRef<'_>> {
        let expression = self.construction.local_expression(expression)?;
        let flow = self
            .construction
            .definition_code
            .definition(expression.owner())?
            .base_expression(expression.expression().0 as usize)?;
        Some(KernelPackedFlowRef {
            mode: flow.mode,
            ty: self.definition_type_ref(expression.owner(), flow.term),
        })
    }

    /// Borrow a declaration flow that was authored directly in this
    /// definition artifact. Effective function, parameter, pattern, OUT, and
    /// value-backed declaration flows remain compatibility-linker derivations
    /// until the complete declaration authority is packed.
    pub fn declared_declaration_flow(
        &self,
        declaration: DeclId,
    ) -> Option<KernelPackedFlowRef<'_>> {
        let (owner, ordinal) = self.construction.local_declaration(declaration)?;
        let flow = self
            .construction
            .definition_code
            .definition(owner)?
            .declared_declaration_flow(ordinal as usize)?;
        Some(KernelPackedFlowRef {
            mode: flow.mode,
            ty: self.definition_type_ref(owner, flow.term),
        })
    }

    /// Borrow the caller-owned packed values of one call's type
    /// substitutions. Keys remain callee parameter ordinals; no global
    /// `TypeVar` namespace is invented at this boundary.
    pub fn call_type_facts(
        &self,
        call: CheckedCallId,
    ) -> Option<KernelDefinitionCallTypeFactsRef<'_>> {
        let call = self.construction.local_call(call)?;
        let code = self.construction.definition_code.definition(call.owner())?;
        let ordinal = call.ordinal() as usize;
        let expression = code.call_expression(ordinal)?;
        let target = code.call_target(ordinal)??;
        let base_result = code.base_expression(expression.0 as usize)?;
        let published_result = code.published_expression(expression.0 as usize)?;
        Some(KernelDefinitionCallTypeFactsRef {
            input: self,
            owner: call.owner(),
            target,
            base_result: crate::PackedFlow {
                mode: base_result.mode,
                term: base_result.term,
            },
            published_result: crate::PackedFlow {
                mode: published_result.mode,
                term: published_result.term,
            },
            substitutions: code.call_type_substitutions(ordinal)?,
            syntax_discriminated_result: code.call_syntax_discriminated_result(ordinal)?,
        })
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

    pub fn pattern_binding_count(&self) -> usize {
        self.construction.pattern_bindings.len()
    }

    pub fn pattern_bindings(&self) -> KernelSemanticPatternBindingIter<'_> {
        KernelSemanticPatternBindingIter {
            input: self,
            rows: self.construction.pattern_bindings.iter(),
        }
    }

    /// Borrow one pattern binding by its final declaration identity.
    ///
    /// The linker seals this column in declaration order, so semantic hot
    /// paths do not need to reconstruct a rich binding table or linearly scan
    /// every binding for each pattern read.
    pub fn pattern_binding(
        &self,
        declaration: DeclId,
    ) -> Option<KernelSemanticPatternBindingRef<'_>> {
        let index = self
            .construction
            .pattern_bindings
            .binary_search_by_key(&declaration, |binding| binding.declaration)
            .ok()?;
        Some(KernelSemanticPatternBindingRef {
            input: self,
            locator: self.construction.pattern_bindings[index],
        })
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

impl<'a> KernelPackedTypeRef<'a> {
    fn with_term(self, term: crate::TypeTermId) -> Self {
        Self { term, ..self }
    }

    /// Test whether this packed term contains one target-qualified callable
    /// parameter. The walk follows the immutable hash-consed DAG directly and
    /// allocates neither a rich recursive `Type` nor a visited collection.
    pub fn contains_parameter(self, parameter: KernelCallableTypeParameterRef<'a>) -> bool {
        if !std::ptr::eq(self.input, parameter.scheme.input)
            || self.scope != parameter.scheme.type_scope()
        {
            return false;
        }
        let variable = parameter.source_variable();
        fn contains(
            arena: &crate::TypeTermArena,
            term: crate::TypeTermId,
            variable: crate::TypeVariableId,
        ) -> bool {
            match arena.term(term) {
                crate::TypeTerm::Variable(candidate) => candidate == variable,
                crate::TypeTerm::VariantSet(variants) => {
                    variants.iter().any(|variant| match variant {
                        crate::VariantTerm::Tag(_) => false,
                        crate::VariantTerm::Tagged { fields, .. } => {
                            contains(arena, *fields, variable)
                        }
                    })
                }
                crate::TypeTerm::Object { fields, .. } => fields
                    .canonical_iter()
                    .any(|field| contains(arena, field.ty, variable)),
                crate::TypeTerm::List(item) | crate::TypeTerm::Set(item) => {
                    contains(arena, item, variable)
                }
                crate::TypeTerm::Function { args, result, .. } => {
                    args.iter()
                        .copied()
                        .any(|arg| contains(arena, arg, variable))
                        || contains(arena, result, variable)
                }
                crate::TypeTerm::Union(members) => members
                    .iter()
                    .copied()
                    .any(|member| contains(arena, member, variable)),
                crate::TypeTerm::Map { key, value } => {
                    contains(arena, key, variable) || contains(arena, value, variable)
                }
                crate::TypeTerm::Text
                | crate::TypeTerm::Number
                | crate::TypeTerm::Bytes(_)
                | crate::TypeTerm::Absent
                | crate::TypeTerm::OpenObjectPlaceholder
                | crate::TypeTerm::RenderContract
                | crate::TypeTerm::UnresolvedShape(_)
                | crate::TypeTerm::Unknown
                | crate::TypeTerm::Bits(_) => false,
            }
        }

        let arena = self
            .input
            .construction
            .definition_code
            .type_store()
            .as_arena();
        contains(arena, self.term, variable)
    }
}

impl KernelSemanticTypeMaterializer<'_> {
    /// Materialize one type from this projector's packed input through the
    /// definition or ABI scheme's final linked alpha namespace.
    pub fn materialize_type(
        &mut self,
        ty: KernelPackedTypeRef<'_>,
    ) -> Result<Type, KernelCheckedLinkError> {
        if !std::ptr::eq(self.input, ty.input) {
            return Err(KernelCheckedLinkError::new(
                "a semantic type projector cannot materialize a foreign packed input",
            ));
        }
        if self.scope != Some(ty.scope) {
            match ty.scope {
                KernelPackedTypeScope::Definition(owner) => {
                    let code = self
                        .input
                        .construction
                        .definition_code
                        .definition(owner)
                        .expect("sealed kernel semantic type owner exists");
                    let alpha_start = self
                        .input
                        .construction
                        .definition_relocation(owner)
                        .expect("sealed kernel semantic type relocation exists")
                        .type_variables
                        .start;
                    self.alpha_end =
                        code.populate_linked_variables(&mut self.variables, alpha_start);
                }
                KernelPackedTypeScope::Abi(callable) => {
                    let scheme = self
                        .input
                        .construction
                        .definition_code
                        .abi_callable_scheme(callable)
                        .expect("sealed kernel semantic ABI scheme exists");
                    let relocation = self
                        .input
                        .construction
                        .abi_relocation(callable)
                        .expect("a semantic ABI type has a checked relocation");
                    self.variables.clear();
                    for local in 0..scheme.variable_count() {
                        let linked = relocation
                            .type_variables
                            .resolve(local, "semantic ABI type variable")?;
                        let source = scheme.variable_base().0.checked_add(local).ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel semantic ABI scheme {} variable namespace overflows u32",
                                callable.0,
                            ))
                        })?;
                        self.variables.insert(TypeVar(source), TypeVar(linked));
                    }
                    self.alpha_end = relocation
                        .type_variables
                        .start
                        .checked_add(relocation.type_variables.len)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(
                                "kernel semantic ABI type-variable namespace overflows u32",
                            )
                        })?;
                }
            }
            self.next = self.alpha_end;
            self.scope = Some(ty.scope);
        }
        Ok(self
            .input
            .construction
            .definition_code
            .materialize_linked_type_term(
                &mut self.cache,
                &mut self.variables,
                &mut self.next,
                self.alpha_end,
                ty.term,
            ))
    }

    pub fn materialize_flow(
        &mut self,
        flow: KernelPackedFlowRef<'_>,
    ) -> Result<FlowType, KernelCheckedLinkError> {
        Ok(FlowType {
            mode: flow.mode,
            ty: self.materialize_type(flow.ty)?,
        })
    }
}

impl boon_checked::CheckedTypeView for KernelPackedTypeRef<'_> {
    fn list_item(self) -> Option<Self> {
        match self
            .input
            .construction
            .definition_code
            .type_store()
            .as_arena()
            .term(self.term)
        {
            crate::TypeTerm::List(item) => Some(self.with_term(item)),
            _ => None,
        }
    }

    fn is_text(self) -> bool {
        matches!(
            self.input
                .construction
                .definition_code
                .type_store()
                .as_arena()
                .term(self.term),
            crate::TypeTerm::Text
        )
    }

    fn is_number(self) -> bool {
        matches!(
            self.input
                .construction
                .definition_code
                .type_store()
                .as_arena()
                .term(self.term),
            crate::TypeTerm::Number
        )
    }

    fn is_render_contract(self) -> bool {
        matches!(
            self.input
                .construction
                .definition_code
                .type_store()
                .as_arena()
                .term(self.term),
            crate::TypeTerm::RenderContract
        )
    }

    fn object_field(self, name: &str) -> Option<Self> {
        let arena = self
            .input
            .construction
            .definition_code
            .type_store()
            .as_arena();
        let crate::TypeTerm::Object { fields, .. } = arena.term(self.term) else {
            return None;
        };
        fields
            .canonical_iter()
            .find(|field| arena.name(field.name) == name)
            .map(|field| self.with_term(field.ty))
    }

    fn all_variants_are_bare_tags(self, mut predicate: impl FnMut(&str) -> bool) -> bool {
        let arena = self
            .input
            .construction
            .definition_code
            .type_store()
            .as_arena();
        let crate::TypeTerm::VariantSet(variants) = arena.term(self.term) else {
            return false;
        };
        variants.iter().all(|variant| match variant {
            crate::VariantTerm::Tag(tag) => predicate(arena.name(*tag)),
            crate::VariantTerm::Tagged { .. } => false,
        })
    }
}

impl<'a> KernelPackedFlowRef<'a> {
    pub const fn mode(self) -> FlowMode {
        self.mode
    }

    pub const fn ty(self) -> KernelPackedTypeRef<'a> {
        self.ty
    }
}

impl<'a> Iterator for KernelCallableSchemeIter<'a> {
    type Item = KernelCallableSchemeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.schemes
            .next()
            .copied()
            .map(|target| self.input.callable_scheme_ref(target))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.schemes.size_hint()
    }
}

impl ExactSizeIterator for KernelCallableSchemeIter<'_> {}

impl<'a> KernelCallableSchemeRef<'a> {
    pub const fn identity(self) -> crate::KernelCallableSchemeId {
        self.target
    }

    fn user_definition(self) -> Option<KernelSemanticDefinitionRowsRef<'a>> {
        let crate::KernelCallableSchemeId::User(owner) = self.target else {
            return None;
        };
        self.input.definition_rows(owner)
    }

    fn abi_scheme(self) -> Option<crate::definition_code::PackedAbiCallableSchemeRef<'a>> {
        let crate::KernelCallableSchemeId::Abi(callable) = self.target else {
            return None;
        };
        self.input
            .construction
            .definition_code
            .abi_callable_scheme(callable)
    }

    fn user_root_statement(
        self,
    ) -> Option<(
        KernelSemanticDefinitionRowsRef<'a>,
        crate::KernelStatementId,
    )> {
        let definition = self.user_definition()?;
        let root = definition.definition().linkage().root_statement?;
        let row = definition
            .definition()
            .runtime_facts()
            .statements()
            .get(root.0 as usize)?;
        matches!(row.kind, crate::PackedStatementKind::Function { .. })
            .then_some((definition, root))
    }

    fn user_parameter_rows(self) -> Option<&'a [crate::PackedStatementParameter]> {
        let (definition, root) = self.user_root_statement()?;
        let facts = definition.definition().runtime_facts();
        facts.statement_parameters(facts.statements().get(root.0 as usize)?)
    }

    pub fn index(self) -> usize {
        self.input
            .callable_index(self.declaration())
            .expect("sealed callable scheme remains in its ordered locator slab")
    }

    pub fn declaration(self) -> DeclId {
        match self.target {
            crate::KernelCallableSchemeId::User(owner) => {
                self.input
                    .construction
                    .definition_relocation(owner)
                    .expect("sealed callable scheme owner has a relocation")
                    .callable
            }
            crate::KernelCallableSchemeId::Abi(callable) => {
                self.input
                    .construction
                    .abi_relocation(callable)
                    .expect("sealed ABI callable scheme has a relocation")
                    .callable
            }
        }
    }

    pub fn scope(self) -> LexicalScopeId {
        match self.target {
            crate::KernelCallableSchemeId::User(_) => self
                .user_definition()
                .expect("sealed user callable definition exists")
                .authority_root_scope(),
            crate::KernelCallableSchemeId::Abi(_) => LexicalScopeId(0),
        }
    }

    pub fn kind(self) -> CheckedCallableKind {
        match self.target {
            crate::KernelCallableSchemeId::User(_) => CheckedCallableKind::User,
            crate::KernelCallableSchemeId::Abi(_) => match self
                .abi_scheme()
                .expect("sealed ABI callable scheme exists")
                .kind()
            {
                crate::KernelCallableKind::User => {
                    unreachable!("immutable ABI cannot contain a user callable")
                }
                crate::KernelCallableKind::Builtin => CheckedCallableKind::Builtin,
                crate::KernelCallableKind::External => CheckedCallableKind::External,
            },
        }
    }

    pub fn name(self) -> &'a str {
        match self.target {
            crate::KernelCallableSchemeId::User(_) => {
                let (definition, root) = self
                    .user_root_statement()
                    .expect("sealed user callable has a function root");
                let row = definition
                    .definition()
                    .runtime_facts()
                    .statements()
                    .get(root.0 as usize)
                    .expect("sealed callable root statement exists");
                let crate::PackedStatementKind::Function { name, .. } = row.kind else {
                    unreachable!("validated user callable root is a function")
                };
                definition.symbol(name)
            }
            crate::KernelCallableSchemeId::Abi(_) => self
                .abi_scheme()
                .expect("sealed ABI callable scheme exists")
                .name(),
        }
    }

    pub fn intrinsic(self) -> Option<boon_checked::CheckedIntrinsicV1> {
        self.abi_scheme().and_then(|scheme| scheme.intrinsic())
    }

    pub fn external_identity(self) -> Option<CheckedExternalDeclarationIdentityV1> {
        self.abi_scheme()
            .and_then(|scheme| scheme.external_identity())
    }

    pub fn parameter_count(self) -> usize {
        match self.target {
            crate::KernelCallableSchemeId::User(_) => self
                .user_parameter_rows()
                .expect("sealed user callable has parameters")
                .len(),
            crate::KernelCallableSchemeId::Abi(_) => self
                .abi_scheme()
                .expect("sealed ABI callable scheme exists")
                .parameters()
                .len(),
        }
    }

    pub fn parameters(self) -> KernelSemanticCallableParameterIter<'a> {
        KernelSemanticCallableParameterIter {
            callable: self,
            next: 0,
            len: u32::try_from(self.parameter_count())
                .expect("sealed callable parameter count fits u32"),
        }
    }

    pub fn parameter(self, ordinal: usize) -> Option<KernelSemanticCallableParameterRef<'a>> {
        let ordinal = u32::try_from(ordinal).ok()?;
        if ordinal as usize >= self.parameter_count() {
            return None;
        }
        let declaration = match self.target {
            crate::KernelCallableSchemeId::User(owner) => {
                let parameter = self.user_parameter_rows()?.get(ordinal as usize)?;
                if parameter.ordinal != ordinal {
                    return None;
                }
                self.input.construction.relocate_declaration(
                    owner,
                    KernelDeclarationReference::Local(parameter.declaration),
                )?
            }
            crate::KernelCallableSchemeId::Abi(callable) => DeclId(
                self.input
                    .construction
                    .abi_relocation(callable)?
                    .parameters
                    .resolve(ordinal, "semantic ABI parameter")
                    .ok()?,
            ),
        };
        Some(KernelSemanticCallableParameterRef {
            callable: self,
            ordinal,
            declaration,
        })
    }

    pub fn context_count(self) -> usize {
        self.abi_scheme()
            .map_or(0, |scheme| scheme.contexts().len())
    }

    pub fn contexts(self) -> KernelSemanticCallableContextIter<'a> {
        KernelSemanticCallableContextIter {
            callable: self,
            next: 0,
            len: u32::try_from(self.context_count())
                .expect("sealed callable context count fits u32"),
        }
    }

    pub fn context(self, ordinal: usize) -> Option<KernelSemanticCallableContextRef<'a>> {
        (ordinal < self.context_count()).then_some(KernelSemanticCallableContextRef {
            callable: self,
            ordinal: u32::try_from(ordinal).ok()?,
        })
    }

    pub fn context_formal(self) -> Option<KernelSemanticContextFormalRef<'a>> {
        let definition = self.user_definition()?;
        let relocation = definition.relocation();
        Some(KernelSemanticContextFormalRef {
            callable: self,
            id: relocation.context_formal?,
            ordinal: relocation.context_formal_ordinal?,
        })
    }

    pub fn requires_pass(self) -> bool {
        self.context_formal().is_some()
    }

    pub fn role(self) -> ProgramRole {
        self.abi_scheme()
            .map_or(self.input.role(), |scheme| scheme.role())
    }

    pub fn effect(self) -> CheckedEffectSummary {
        match self.target {
            crate::KernelCallableSchemeId::User(_) => {
                let effect = self
                    .user_definition()
                    .expect("sealed user callable definition exists")
                    .definition()
                    .code()
                    .effect_summary();
                CheckedEffectSummary {
                    reads_state: effect.reads_state,
                    writes_state: effect.writes_state,
                    emits_source: effect.emits_source,
                    invokes_host: effect.invokes_host,
                }
            }
            crate::KernelCallableSchemeId::Abi(_) => self
                .abi_scheme()
                .expect("sealed ABI callable scheme exists")
                .effect(),
        }
    }

    pub fn body(self) -> Option<CheckedStatementId> {
        self.user_definition()
            .map(|definition| definition.root_statement())
    }

    pub fn result_expression(self) -> Option<CheckedExprId> {
        self.user_definition()
            .map(|definition| definition.result_expression())
    }

    pub fn contextual_operation(self) -> Option<CheckedContextualOperation> {
        let operation = self.abi_scheme()?.contextual_operation()?;
        let parameter = |ordinal: u32| {
            self.parameter(ordinal as usize)
                .map(|row| row.declaration())
        };
        Some(match operation {
            crate::KernelAbiContextualOperation::Map { list, row, body } => {
                CheckedContextualOperation::Map {
                    list: parameter(list)?,
                    row: parameter(row)?,
                    body: parameter(body)?,
                }
            }
            crate::KernelAbiContextualOperation::Filter {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Filter {
                list: parameter(list)?,
                row: parameter(row)?,
                predicate: parameter(predicate)?,
            },
            crate::KernelAbiContextualOperation::Retain {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Retain {
                list: parameter(list)?,
                row: parameter(row)?,
                predicate: parameter(predicate)?,
            },
            crate::KernelAbiContextualOperation::Remove {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Remove {
                list: parameter(list)?,
                row: parameter(row)?,
                predicate: parameter(predicate)?,
            },
            crate::KernelAbiContextualOperation::Every {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Every {
                list: parameter(list)?,
                row: parameter(row)?,
                predicate: parameter(predicate)?,
            },
            crate::KernelAbiContextualOperation::Any {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Any {
                list: parameter(list)?,
                row: parameter(row)?,
                predicate: parameter(predicate)?,
            },
            crate::KernelAbiContextualOperation::Find {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Find {
                list: parameter(list)?,
                row: parameter(row)?,
                predicate: parameter(predicate)?,
            },
            crate::KernelAbiContextualOperation::SortBy {
                list,
                row,
                key,
                direction,
            } => CheckedContextualOperation::SortBy {
                list: parameter(list)?,
                row: parameter(row)?,
                key: parameter(key)?,
                direction: parameter(direction)?,
            },
            crate::KernelAbiContextualOperation::ThenBy {
                list,
                row,
                key,
                direction,
            } => CheckedContextualOperation::ThenBy {
                list: parameter(list)?,
                row: parameter(row)?,
                key: parameter(key)?,
                direction: parameter(direction)?,
            },
        })
    }

    fn type_scope(self) -> KernelPackedTypeScope {
        match self.target {
            crate::KernelCallableSchemeId::User(owner) => KernelPackedTypeScope::Definition(owner),
            crate::KernelCallableSchemeId::Abi(callable) => KernelPackedTypeScope::Abi(callable),
        }
    }

    fn flow(self, flow: crate::PackedFlow) -> KernelPackedFlowRef<'a> {
        KernelPackedFlowRef {
            mode: flow.mode,
            ty: self.input.packed_type_ref(self.type_scope(), flow.term),
        }
    }

    pub fn formal_count(self) -> usize {
        match self.target {
            crate::KernelCallableSchemeId::User(owner) => self
                .input
                .construction
                .definition_code
                .definition(owner)
                .expect("sealed callable scheme owner exists")
                .formals()
                .len(),
            crate::KernelCallableSchemeId::Abi(callable) => self
                .input
                .construction
                .definition_code
                .abi_callable_scheme(callable)
                .expect("sealed ABI callable scheme exists")
                .formals()
                .len(),
        }
    }

    pub fn formal(self, ordinal: usize) -> Option<KernelPackedFlowRef<'a>> {
        let flow = match self.target {
            crate::KernelCallableSchemeId::User(owner) => {
                let flow = *self
                    .input
                    .construction
                    .definition_code
                    .definition(owner)?
                    .formals()
                    .get(ordinal)?;
                crate::PackedFlow {
                    mode: flow.mode,
                    term: flow.term,
                }
            }
            crate::KernelCallableSchemeId::Abi(callable) => *self
                .input
                .construction
                .definition_code
                .abi_callable_scheme(callable)?
                .formals()
                .get(ordinal)?,
        };
        Some(self.flow(flow))
    }

    pub fn result(self) -> KernelPackedFlowRef<'a> {
        let flow = match self.target {
            crate::KernelCallableSchemeId::User(owner) => self
                .input
                .construction
                .definition_code
                .definition(owner)
                .expect("sealed callable scheme owner exists")
                .result_flow(),
            crate::KernelCallableSchemeId::Abi(callable) => self
                .input
                .construction
                .definition_code
                .abi_callable_scheme(callable)
                .expect("sealed ABI callable scheme exists")
                .result(),
        };
        self.flow(flow)
    }

    pub fn type_parameter_count(self) -> usize {
        match self.target {
            crate::KernelCallableSchemeId::User(owner) => self
                .input
                .construction
                .definition_code
                .definition(owner)
                .expect("sealed callable scheme owner exists")
                .callable_type_parameters()
                .len(),
            crate::KernelCallableSchemeId::Abi(callable) => self
                .input
                .construction
                .definition_code
                .abi_callable_scheme(callable)
                .expect("sealed ABI callable scheme exists")
                .type_parameters()
                .len(),
        }
    }

    pub fn type_parameter(self, ordinal: usize) -> Option<KernelCallableTypeParameterRef<'a>> {
        (ordinal < self.type_parameter_count()).then_some(KernelCallableTypeParameterRef {
            scheme: self,
            ordinal: crate::KernelTypeParameterId(u32::try_from(ordinal).ok()?),
        })
    }
}

impl<'a> Iterator for KernelSemanticCallableParameterIter<'a> {
    type Item = KernelSemanticCallableParameterRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.len {
            return None;
        }
        let ordinal = self.next;
        self.next += 1;
        self.callable.parameter(ordinal as usize)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.next) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticCallableParameterIter<'_> {}

impl<'a> KernelSemanticCallableParameterRef<'a> {
    fn user_row(self) -> Option<&'a crate::PackedStatementParameter> {
        self.callable
            .user_parameter_rows()?
            .get(self.ordinal as usize)
            .filter(|row| row.ordinal == self.ordinal)
    }

    fn abi_row(self) -> Option<&'a crate::definition_code::PackedAbiParameter> {
        self.callable
            .abi_scheme()?
            .parameters()
            .get(self.ordinal as usize)
            .filter(|row| row.ordinal == self.ordinal)
    }

    pub const fn callable(self) -> KernelCallableSchemeRef<'a> {
        self.callable
    }

    pub const fn ordinal(self) -> usize {
        self.ordinal as usize
    }

    pub const fn declaration(self) -> DeclId {
        self.declaration
    }

    pub fn name(self) -> &'a str {
        match self.callable.target {
            crate::KernelCallableSchemeId::User(_) => {
                let row = self
                    .user_row()
                    .expect("sealed user callable parameter row exists");
                self.callable
                    .user_definition()
                    .expect("sealed user callable definition exists")
                    .symbol(row.name)
            }
            crate::KernelCallableSchemeId::Abi(_) => {
                let row = self
                    .abi_row()
                    .expect("sealed ABI callable parameter row exists");
                self.callable
                    .abi_scheme()
                    .expect("sealed ABI callable scheme exists")
                    .symbol(row.name)
                    .expect("sealed ABI parameter name belongs to its text authority")
            }
        }
    }

    pub fn kind(self) -> CheckedParameterKind {
        match self.callable.target {
            crate::KernelCallableSchemeId::User(_) => match self
                .user_row()
                .expect("sealed user callable parameter row exists")
                .kind
            {
                crate::KernelParameterKind::Value => CheckedParameterKind::Value,
                crate::KernelParameterKind::Out => CheckedParameterKind::Out,
            },
            crate::KernelCallableSchemeId::Abi(_) => {
                self.abi_row()
                    .expect("sealed ABI callable parameter row exists")
                    .kind
            }
        }
    }

    pub fn flow(self) -> KernelPackedFlowRef<'a> {
        self.callable
            .formal(self.ordinal as usize)
            .expect("sealed callable parameter has a packed formal flow")
    }

    pub fn requirement(self) -> KernelSemanticParameterRequirementRef<'a> {
        let Some(row) = self.abi_row() else {
            return KernelSemanticParameterRequirementRef::Required;
        };
        let scheme = self
            .callable
            .abi_scheme()
            .expect("ABI parameter always has its scheme");
        match row.requirement {
            crate::PackedAbiParameterRequirement::Required => {
                KernelSemanticParameterRequirementRef::Required
            }
            crate::PackedAbiParameterRequirement::CallableProfile(symbol) => {
                KernelSemanticParameterRequirementRef::CallableProfile(
                    scheme
                        .symbol(symbol)
                        .expect("sealed ABI profile belongs to its text authority"),
                )
            }
            crate::PackedAbiParameterRequirement::Tag(symbol) => {
                KernelSemanticParameterRequirementRef::Tag(
                    scheme
                        .symbol(symbol)
                        .expect("sealed ABI tag belongs to its text authority"),
                )
            }
            crate::PackedAbiParameterRequirement::ExactInteger(value) => {
                KernelSemanticParameterRequirementRef::ExactInteger(value)
            }
            crate::PackedAbiParameterRequirement::Text(span) => {
                KernelSemanticParameterRequirementRef::Text(
                    scheme
                        .default_text(span)
                        .expect("sealed ABI default-text span is valid UTF-8"),
                )
            }
        }
    }

    pub fn evaluation_scope(self) -> CheckedEvaluationScope {
        let scope = match self.callable.target {
            crate::KernelCallableSchemeId::User(_) => {
                self.user_row()
                    .expect("sealed user callable parameter row exists")
                    .evaluation_scope
            }
            crate::KernelCallableSchemeId::Abi(_) => {
                self.abi_row()
                    .expect("sealed ABI callable parameter row exists")
                    .evaluation_scope
            }
        };
        match scope {
            crate::KernelParameterEvaluationScope::Parent => CheckedEvaluationScope::Parent,
            crate::KernelParameterEvaluationScope::Output { parameter_ordinal } => {
                let output = self
                    .callable
                    .parameter(parameter_ordinal as usize)
                    .expect("sealed callable evaluation scope targets a parameter");
                assert_eq!(
                    output.kind(),
                    CheckedParameterKind::Out,
                    "sealed callable evaluation scope targets an OUT parameter",
                );
                CheckedEvaluationScope::Output {
                    formal: output.declaration(),
                }
            }
        }
    }

    pub fn span(self) -> CheckedSpan {
        match self.callable.target {
            crate::KernelCallableSchemeId::User(_) => self
                .callable
                .input
                .declaration(self.declaration)
                .and_then(KernelSemanticResolvedDeclarationRef::span)
                .expect("sealed user callable parameter span is rebased"),
            crate::KernelCallableSchemeId::Abi(_) => CheckedSpan::default(),
        }
    }

    pub fn start(self) -> usize {
        self.span().start
    }

    pub fn end(self) -> usize {
        self.span().end
    }
}

impl KernelSemanticParameterRequirementRef<'_> {
    pub const fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }

    pub const fn is_optional(self) -> bool {
        !self.is_required()
    }
}

impl<'a> Iterator for KernelSemanticCallableContextIter<'a> {
    type Item = KernelSemanticCallableContextRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.len {
            return None;
        }
        let ordinal = self.next;
        self.next += 1;
        Some(KernelSemanticCallableContextRef {
            callable: self.callable,
            ordinal,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.next) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for KernelSemanticCallableContextIter<'_> {}

impl<'a> KernelSemanticCallableContextRef<'a> {
    fn row(self) -> &'a crate::definition_code::PackedAbiContext {
        self.callable
            .abi_scheme()
            .expect("callable contexts belong to ABI schemes")
            .contexts()
            .get(self.ordinal as usize)
            .expect("sealed ABI context ordinal is valid")
    }

    pub fn name(self) -> &'a str {
        let row = self.row();
        self.callable
            .abi_scheme()
            .expect("callable contexts belong to ABI schemes")
            .symbol(row.name)
            .expect("sealed ABI context name belongs to its text authority")
    }

    pub fn kind(self) -> boon_checked::CheckedCallContextKind {
        self.row().kind
    }

    pub fn provider(self) -> DeclId {
        self.callable
            .parameter(self.row().provider_parameter_ordinal as usize)
            .expect("sealed ABI context provider parameter exists")
            .declaration()
    }

    pub fn flow(self) -> KernelPackedFlowRef<'a> {
        self.callable.flow(self.row().flow)
    }
}

impl<'a> KernelSemanticContextFormalRef<'a> {
    pub const fn id(self) -> ContextFormalId {
        self.id
    }

    pub fn callable(self) -> DeclId {
        self.callable.declaration()
    }

    pub fn flow(self) -> KernelPackedFlowRef<'a> {
        self.callable
            .formal(self.ordinal as usize)
            .expect("sealed PASSED context formal has a packed flow")
    }
}

impl<'a> KernelSemanticResolvedDeclarationRef<'a> {
    pub fn id(self) -> DeclId {
        match self {
            Self::Definition(row) => row.id(),
            Self::AbiCallable(row) => row.declaration(),
            Self::AbiParameter(row) => row.declaration(),
        }
    }

    pub fn scope(self) -> LexicalScopeId {
        match self {
            Self::Definition(row) => row.scope(),
            Self::AbiCallable(_) | Self::AbiParameter(_) => LexicalScopeId(0),
        }
    }

    pub fn name(self) -> &'a str {
        match self {
            Self::Definition(row) => row.name(),
            Self::AbiCallable(row) => row.name(),
            Self::AbiParameter(row) => row.name(),
        }
    }

    pub fn kind(self) -> CheckedDeclarationKind {
        match self {
            Self::Definition(row) => row.kind(),
            Self::AbiCallable(row) => match row.kind() {
                CheckedCallableKind::Builtin => CheckedDeclarationKind::Builtin,
                CheckedCallableKind::External => CheckedDeclarationKind::External,
                CheckedCallableKind::User => {
                    unreachable!("definition-owned user declarations use the definition variant")
                }
            },
            Self::AbiParameter(row) => match row.kind() {
                CheckedParameterKind::Value => CheckedDeclarationKind::ValueParameter,
                CheckedParameterKind::Out => CheckedDeclarationKind::OutParameter,
            },
        }
    }

    pub fn value(self) -> Option<CheckedExprId> {
        match self {
            Self::Definition(row) => row.value(),
            Self::AbiCallable(_) | Self::AbiParameter(_) => None,
        }
    }

    pub fn body_scope(self) -> Option<LexicalScopeId> {
        match self {
            Self::Definition(row) => row.body_scope(),
            Self::AbiCallable(_) | Self::AbiParameter(_) => None,
        }
    }

    pub fn span(self) -> Option<CheckedSpan> {
        match self {
            Self::Definition(row) => row.span(),
            Self::AbiCallable(_) | Self::AbiParameter(_) => Some(CheckedSpan::default()),
        }
    }
}

impl<'a> KernelCallableTypeParameterRef<'a> {
    pub const fn scheme(self) -> KernelCallableSchemeRef<'a> {
        self.scheme
    }

    pub const fn ordinal(self) -> crate::KernelTypeParameterId {
        self.ordinal
    }

    fn source_variable(self) -> crate::TypeVariableId {
        match self.scheme.target {
            crate::KernelCallableSchemeId::User(owner) => {
                self.scheme
                    .input
                    .construction
                    .definition_code
                    .definition(owner)
                    .expect("sealed callable parameter owner exists")
                    .callable_type_parameters()[self.ordinal.0 as usize]
                    .source
            }
            crate::KernelCallableSchemeId::Abi(callable) => {
                self.scheme
                    .input
                    .construction
                    .definition_code
                    .abi_callable_scheme(callable)
                    .expect("sealed ABI callable parameter scheme exists")
                    .type_parameters()[self.ordinal.0 as usize]
                    .source
            }
        }
    }

    /// Alpha coordinate inside this callable's own linked type-variable
    /// range. Unlike [`Self::ordinal`], this is independent of packed term
    /// traversal order.
    pub fn linked_local(self) -> u32 {
        match self.scheme.target {
            crate::KernelCallableSchemeId::User(owner) => {
                self.scheme
                    .input
                    .construction
                    .definition_code
                    .definition(owner)
                    .expect("sealed callable parameter owner exists")
                    .callable_type_parameters()[self.ordinal.0 as usize]
                    .linked_local
            }
            crate::KernelCallableSchemeId::Abi(callable) => {
                self.scheme
                    .input
                    .construction
                    .definition_code
                    .abi_callable_scheme(callable)
                    .expect("sealed ABI callable parameter scheme exists")
                    .type_parameters()[self.ordinal.0 as usize]
                    .linked_local
            }
        }
    }

    /// Final checked type-variable identity for this retained target
    /// parameter. The mapping follows the scheme's explicit `linked_local`
    /// row; it never reconstructs parameter order from a rich recursive type.
    pub fn linked_variable(self) -> TypeVar {
        let variables = match self.scheme.target {
            crate::KernelCallableSchemeId::User(owner) => {
                let variables = self
                    .scheme
                    .input
                    .construction
                    .definition_relocation(owner)
                    .expect("sealed callable parameter owner relocates")
                    .type_variables;
                variables
            }
            crate::KernelCallableSchemeId::Abi(callable) => {
                let variables = self
                    .scheme
                    .input
                    .construction
                    .abi_relocation(callable)
                    .expect("sealed ABI callable parameter relocates")
                    .type_variables;
                variables
            }
        };
        TypeVar(
            variables
                .resolve(self.linked_local(), "callable type parameter")
                .expect("sealed callable parameter linked alpha is valid"),
        )
    }
}

impl<'a> KernelDefinitionCallTypeFactsRef<'a> {
    pub fn target_scheme(self) -> KernelCallableSchemeRef<'a> {
        self.input.callable_scheme_ref(self.target)
    }

    pub fn base_result(self) -> KernelPackedFlowRef<'a> {
        KernelPackedFlowRef {
            mode: self.base_result.mode,
            ty: self
                .input
                .definition_type_ref(self.owner, self.base_result.term),
        }
    }

    pub fn published_result(self) -> KernelPackedFlowRef<'a> {
        KernelPackedFlowRef {
            mode: self.published_result.mode,
            ty: self
                .input
                .definition_type_ref(self.owner, self.published_result.term),
        }
    }

    pub fn substitutions(
        self,
    ) -> impl ExactSizeIterator<Item = KernelDefinitionCallTypeSubstitutionRef<'a>> {
        let input = self.input;
        let owner = self.owner;
        let target = self.target;
        self.substitutions.iter().map(
            move |substitution| KernelDefinitionCallTypeSubstitutionRef {
                input,
                owner,
                target,
                substitution,
            },
        )
    }

    pub fn substitution_count(self) -> usize {
        self.substitutions.len()
    }

    pub fn substitution(
        self,
        ordinal: usize,
    ) -> Option<KernelDefinitionCallTypeSubstitutionRef<'a>> {
        self.substitutions.get(ordinal).map(|substitution| {
            KernelDefinitionCallTypeSubstitutionRef {
                input: self.input,
                owner: self.owner,
                target: self.target,
                substitution,
            }
        })
    }

    pub const fn syntax_discriminated_result(self) -> bool {
        self.syntax_discriminated_result
    }
}

impl<'a> KernelDefinitionCallTypeSubstitutionRef<'a> {
    pub fn parameter(self) -> KernelCallableTypeParameterRef<'a> {
        self.input
            .callable_scheme_ref(self.target)
            .type_parameter(self.substitution.variable.0 as usize)
            .expect("sealed call substitution parameter belongs to its target scheme")
    }

    pub fn value(self) -> KernelPackedTypeRef<'a> {
        self.input
            .definition_type_ref(self.owner, self.substitution.term)
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

impl<'a> Iterator for KernelSemanticPatternBindingIter<'a> {
    type Item = KernelSemanticPatternBindingRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.rows
            .next()
            .copied()
            .map(|locator| KernelSemanticPatternBindingRef {
                input: self.input,
                locator,
            })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.rows.size_hint()
    }
}

impl ExactSizeIterator for KernelSemanticPatternBindingIter<'_> {}

impl<'a> KernelSemanticPatternBindingRef<'a> {
    pub const fn declaration(self) -> DeclId {
        self.locator.declaration
    }

    pub const fn selector(self) -> CheckedExprId {
        self.locator.selector
    }

    pub fn projection(self) -> impl ExactSizeIterator<Item = &'a str> {
        self.locator.projection.into_iter().map(|symbol| {
            self.input
                .construction
                .definition_code
                .symbol(symbol)
                .expect("sealed pattern-binding symbol belongs to its text authority")
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn checked_link_declaration<'a>(
    declarations: &'a [CheckedDeclaration],
    id: DeclId,
) -> Option<&'a CheckedDeclaration> {
    id.0.checked_sub(1)
        .and_then(|index| declarations.get(index as usize))
        .filter(|candidate| candidate.id == id)
}

#[cfg(test)]
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

#[cfg(test)]
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

#[derive(Clone, Copy)]
enum KernelPublicationPathV1 {
    Packed(PathId),
    SyntheticState(u32),
}

fn checked_link_kernel_callable_kind(
    kind: crate::KernelCallableKind,
) -> CheckedShardCallableKindV2 {
    match kind {
        crate::KernelCallableKind::User => CheckedShardCallableKindV2::User,
        crate::KernelCallableKind::Builtin => CheckedShardCallableKindV2::Builtin,
        crate::KernelCallableKind::External => CheckedShardCallableKindV2::External,
    }
}

fn packed_publication_declaration<'a>(
    snapshot: &'a KernelCheckedSnapshot,
    declaration_locations: &[Option<(KernelOwnerId, crate::KernelDeclarationId)>],
    declaration: DeclId,
) -> Result<
    (
        KernelDefinitionRef<'a>,
        &'a crate::PackedDeclaration,
        &'a crate::PackedDeclarationPresentation,
    ),
    KernelCheckedLinkError,
> {
    let (owner, local) = declaration_locations
        .get(declaration.0 as usize)
        .copied()
        .flatten()
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "packed checked publication references non-definition declaration {}",
                declaration.0,
            ))
        })?;
    let definition = snapshot.definition(owner).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "packed checked publication references missing definition {}",
            owner.0,
        ))
    })?;
    let facts = definition.runtime_facts();
    let row = facts
        .declarations()
        .get(local.0 as usize)
        .filter(|row| row.id == local)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "packed checked publication references missing declaration {}:{}",
                owner.0, local.0,
            ))
        })?;
    let presentation = declaration_presentation(facts, local)?;
    Ok((definition, row, presentation))
}

fn packed_publication_scope_owner_declaration(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    scope_locations: &[Option<(KernelOwnerId, crate::KernelScopeId)>],
    mut scope: LexicalScopeId,
) -> Result<Option<DeclId>, KernelCheckedLinkError> {
    let mut remaining = scope_locations.len().saturating_add(1);
    loop {
        if remaining == 0 {
            return Err(KernelCheckedLinkError::new(format!(
                "packed checked publication scope {} has an ownership cycle",
                scope.0,
            )));
        }
        remaining -= 1;
        if scope == LexicalScopeId(0) {
            return Ok(None);
        }
        let (owner, local) = scope_locations
            .get(scope.0 as usize)
            .copied()
            .flatten()
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "packed checked publication references missing scope {}",
                    scope.0,
                ))
            })?;
        let definition = snapshot.definition(owner).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "packed checked publication references missing scope owner {}",
                owner.0,
            ))
        })?;
        let row = definition
            .runtime_facts()
            .scopes()
            .get(local.0 as usize)
            .filter(|row| row.id == local)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "packed checked publication references missing scope {}:{}",
                    owner.0, local.0,
                ))
            })?;
        if row.kind == crate::KernelScopeKind::Function
            && let Some(declaration) = row.owner
        {
            let declaration = layout.declaration(owner, declaration)?;
            return Ok(Some(declaration));
        }
        scope = layout.scope(owner, row.parent)?;
    }
}

fn packed_publication_scope_owner(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    scope_locations: &[Option<(KernelOwnerId, crate::KernelScopeId)>],
    callable_owners: &[Option<CheckedShardOwnerKeyV2>],
    role: ProgramRole,
    scope: LexicalScopeId,
) -> Result<CheckedShardOwnerKeyV2, KernelCheckedLinkError> {
    let Some(declaration) =
        packed_publication_scope_owner_declaration(layout, snapshot, scope_locations, scope)?
    else {
        return Ok(CheckedShardOwnerKeyV2::ProgramTopLevel { role });
    };
    callable_owners
        .get(declaration.0 as usize)
        .and_then(|owner| owner.clone())
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "packed function scope {} has no callable owner {}",
                scope.0, declaration.0,
            ))
        })
}

fn push_canonical_path_segment(path: &mut String, segment: &str) {
    if !path.is_empty() {
        path.push('.');
    }
    path.push_str(segment);
}

#[allow(clippy::too_many_arguments)]
fn checked_link_packed_authority_projection(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    scope_locations: &[Option<(KernelOwnerId, crate::KernelScopeId)>],
    declaration_locations: &[Option<(KernelOwnerId, crate::KernelDeclarationId)>],
    owner: CheckedShardOwnerKeyV2,
    anchor: DeclId,
    projection: KernelPublicationPathV1,
    ancestor_symbols: &mut Vec<SymbolId>,
) -> Result<CheckedShardProjectionKeyV2, KernelCheckedLinkError> {
    if !matches!(owner, CheckedShardOwnerKeyV2::ProgramTopLevel { .. }) {
        return Ok(checked_link_definition_projection(owner));
    }

    let (definition, declaration, presentation) =
        packed_publication_declaration(snapshot, declaration_locations, anchor)?;
    let packed_path = match projection {
        KernelPublicationPathV1::Packed(path) => {
            Some(snapshot.program.path(path).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "packed checked authority declaration {} references foreign path {path:?}",
                    anchor.0,
                ))
            })?)
        }
        KernelPublicationPathV1::SyntheticState(_) => None,
    };
    let projection_is_empty = match projection {
        KernelPublicationPathV1::Packed(_) => packed_path.is_some_and(|path| path.is_empty()),
        KernelPublicationPathV1::SyntheticState(_) => false,
    };
    if declaration.kind == crate::KernelDeclarationKind::Function && projection_is_empty {
        return Ok(checked_link_definition_projection(owner));
    }

    ancestor_symbols.clear();
    if declaration.kind != crate::KernelDeclarationKind::Function {
        ancestor_symbols.push(declaration.name);
        let mut scope = layout.scope(definition.owner(), presentation.scope)?;
        let mut remaining = scope_locations.len().saturating_add(1);
        while scope != LexicalScopeId(0) {
            if remaining == 0 {
                return Err(KernelCheckedLinkError::new(format!(
                    "packed checked authority declaration {} has a scope cycle",
                    anchor.0,
                )));
            }
            remaining -= 1;
            let (scope_owner, local) = scope_locations
                .get(scope.0 as usize)
                .copied()
                .flatten()
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed checked authority references missing scope {}",
                        scope.0,
                    ))
                })?;
            let scope_definition = snapshot.definition(scope_owner).ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "packed checked authority references missing definition {}",
                    scope_owner.0,
                ))
            })?;
            let scope_row = scope_definition
                .runtime_facts()
                .scopes()
                .get(local.0 as usize)
                .filter(|row| row.id == local)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed checked authority references missing scope {}:{}",
                        scope_owner.0, local.0,
                    ))
                })?;
            if scope_row.kind == crate::KernelScopeKind::Function {
                break;
            }
            if let Some(owner_declaration) = scope_row.owner {
                let owner_declaration = layout.declaration(scope_owner, owner_declaration)?;
                let (_, owner_row, _) = packed_publication_declaration(
                    snapshot,
                    declaration_locations,
                    owner_declaration,
                )?;
                if matches!(
                    owner_row.kind,
                    crate::KernelDeclarationKind::Field
                        | crate::KernelDeclarationKind::Source
                        | crate::KernelDeclarationKind::Hold
                        | crate::KernelDeclarationKind::List
                ) {
                    ancestor_symbols.push(owner_row.name);
                }
            }
            scope = layout.scope(scope_owner, scope_row.parent)?;
        }
        ancestor_symbols.reverse();
    }

    let projected_symbol_count = packed_path.map_or(0, |path| path.len());
    let mut canonical_path = String::with_capacity(
        ancestor_symbols
            .iter()
            .filter_map(|symbol| snapshot.program.symbol(*symbol))
            .map(str::len)
            .sum::<usize>()
            .saturating_add(projected_symbol_count.saturating_mul(8))
            .saturating_add(
                ancestor_symbols
                    .len()
                    .saturating_add(projected_symbol_count),
            ),
    );
    for symbol in ancestor_symbols.iter().copied() {
        let name = snapshot.program.symbol(symbol).ok_or_else(|| {
            KernelCheckedLinkError::new("packed checked authority ancestor has a foreign symbol")
        })?;
        push_canonical_path_segment(&mut canonical_path, name);
    }
    if let Some(path) = packed_path {
        for symbol in path.iter() {
            let name = snapshot.program.symbol(symbol).ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "packed checked authority projection has a foreign symbol",
                )
            })?;
            push_canonical_path_segment(&mut canonical_path, name);
        }
    } else if let KernelPublicationPathV1::SyntheticState(ordinal) = projection {
        if !canonical_path.is_empty() {
            canonical_path.push('.');
        }
        fmt::Write::write_fmt(&mut canonical_path, format_args!("state_{ordinal}"))
            .expect("writing a state ordinal into String cannot fail");
    }
    if canonical_path.is_empty() {
        return Ok(checked_link_definition_projection(owner));
    }
    Ok(CheckedShardProjectionKeyV2 {
        owner,
        region: CheckedShardRegionV2::TopLevelAuthority { canonical_path },
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TempCheckedImageProjectionIdV1(u32);

struct BuildingKernelCheckedImagePrehashEntryV1 {
    key: CheckedShardProjectionKeyV2,
    digest: [u8; 32],
    projection: TempCheckedImageProjectionIdV1,
}

#[derive(Clone, Copy)]
struct BuildingKernelCheckedImageExpectedRouteV1 {
    domain: CheckedImageRowDomainV2,
    dense_index: u32,
    projection: TempCheckedImageProjectionIdV1,
}

/// Complete packed-layout ownership authority built before publication starts.
///
/// Projection keys are canonicalized once and move directly into the final V4
/// handoff. The sibling publication owns payload only; it never stores keys or
/// routes.
struct KernelCheckedImageOwnershipPlanV1 {
    projections: Box<[boon_checked::CheckedImageKernelPlannedProjectionV1]>,
    routes: Box<[CheckedImageKernelExpectedRouteV1]>,
}

struct BuildingKernelCheckedImageOwnershipPlanV1 {
    prehash_by_key: Vec<BuildingKernelCheckedImagePrehashEntryV1>,
    routes: Vec<BuildingKernelCheckedImageExpectedRouteV1>,
}

impl BuildingKernelCheckedImageOwnershipPlanV1 {
    fn new() -> Self {
        Self {
            prehash_by_key: Vec::new(),
            routes: Vec::new(),
        }
    }

    fn intern_projection(
        &mut self,
        key: CheckedShardProjectionKeyV2,
    ) -> Result<TempCheckedImageProjectionIdV1, KernelCheckedLinkError> {
        match self
            .prehash_by_key
            .binary_search_by(|entry| entry.key.cmp(&key))
        {
            Ok(index) => return Ok(self.prehash_by_key[index].projection),
            Err(index) => {
                let digest = boon_checked::checked_image_projection_key_digest_v4(&key)
                    .map_err(KernelCheckedLinkError::new)?;
                let projection = TempCheckedImageProjectionIdV1(
                    u32::try_from(self.prehash_by_key.len()).map_err(|_| {
                        KernelCheckedLinkError::new(
                            "kernel checked-image projection catalog exceeds u32",
                        )
                    })?,
                );
                self.prehash_by_key.insert(
                    index,
                    BuildingKernelCheckedImagePrehashEntryV1 {
                        key,
                        digest,
                        projection,
                    },
                );
                return Ok(projection);
            }
        }
    }

    fn route(
        &mut self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
        projection: TempCheckedImageProjectionIdV1,
    ) -> Result<(), KernelCheckedLinkError> {
        self.routes.push(BuildingKernelCheckedImageExpectedRouteV1 {
            domain,
            dense_index: u32::try_from(dense_index).map_err(|_| {
                KernelCheckedLinkError::new("kernel checked-image expected route exceeds u32")
            })?,
            projection,
        });
        Ok(())
    }

    fn finish(mut self) -> Result<KernelCheckedImageOwnershipPlanV1, KernelCheckedLinkError> {
        let mut projection_digests = self
            .prehash_by_key
            .iter()
            .map(|entry| entry.digest)
            .collect::<Vec<_>>();
        projection_digests.sort_unstable();
        if projection_digests.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(KernelCheckedLinkError::new(
                "kernel checked-image ownership keys contain a stable-digest collision",
            ));
        }
        self.routes
            .sort_unstable_by_key(|route| (route.domain, route.dense_index));
        if self.routes.windows(2).any(|pair| {
            (pair[0].domain, pair[0].dense_index) == (pair[1].domain, pair[1].dense_index)
        }) {
            return Err(KernelCheckedLinkError::new(
                "kernel checked-image ownership plan contains a duplicate entity route",
            ));
        }
        let mut canonical_by_temp = vec![u32::MAX; self.prehash_by_key.len()];
        for (canonical, entry) in self.prehash_by_key.iter().enumerate() {
            canonical_by_temp[entry.projection.0 as usize] =
                u32::try_from(canonical).map_err(|_| {
                    KernelCheckedLinkError::new(
                        "kernel checked-image projection catalog exceeds u32",
                    )
                })?;
        }
        let routes = self
            .routes
            .into_iter()
            .map(|route| {
                let projection = canonical_by_temp
                    .get(route.projection.0 as usize)
                    .copied()
                    .filter(|projection| *projection != u32::MAX)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "kernel checked-image ownership route references a foreign projection",
                        )
                    })?;
                Ok(CheckedImageKernelExpectedRouteV1::__kernel_new(
                    route.domain,
                    route.dense_index,
                    projection,
                ))
            })
            .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
        let mut definition_by_temp = vec![None; self.prehash_by_key.len()];
        let mut group_start = 0usize;
        while group_start < self.prehash_by_key.len() {
            let owner = &self.prehash_by_key[group_start].key.owner;
            let mut group_end = group_start + 1;
            while group_end < self.prehash_by_key.len()
                && self.prehash_by_key[group_end].key.owner == *owner
            {
                group_end += 1;
            }
            let definition = self.prehash_by_key[group_start..group_end]
                .iter()
                .position(|entry| entry.key.region == CheckedShardRegionV2::Definition)
                .map(|offset| group_start + offset)
                .map(u32::try_from)
                .transpose()
                .map_err(|_| {
                    KernelCheckedLinkError::new(
                        "kernel checked-image projection catalog exceeds u32",
                    )
                })?;
            for entry in &self.prehash_by_key[group_start..group_end] {
                definition_by_temp[entry.projection.0 as usize] = definition;
            }
            group_start = group_end;
        }
        let projections = self
            .prehash_by_key
            .into_iter()
            .map(|entry| {
                boon_checked::CheckedImageKernelPlannedProjectionV1::__kernel_new(
                    entry.key,
                    entry.digest,
                    definition_by_temp[entry.projection.0 as usize],
                )
            })
            .collect::<Vec<_>>();
        Ok(KernelCheckedImageOwnershipPlanV1 {
            projections: projections.into_boxed_slice(),
            routes: routes.into_boxed_slice(),
        })
    }
}

impl KernelCheckedImageOwnershipPlanV1 {
    fn projection(
        &self,
        key: &CheckedShardProjectionKeyV2,
    ) -> Result<boon_checked::CheckedImageKernelProjectionIdV1, String> {
        let index = self
            .projections
            .binary_search_by(|entry| entry.__kernel_key().cmp(key))
            .ok()
            .ok_or_else(|| {
                "kernel checked-image publication produced a projection absent from the completed ownership plan"
                    .to_owned()
            })?;
        Ok(
            boon_checked::CheckedImageKernelProjectionIdV1::__kernel_new(
                u32::try_from(index).map_err(|_| {
                    "kernel checked-image projection catalog exceeds u32".to_owned()
                })?,
            ),
        )
    }

    fn route(
        &self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
    ) -> Result<(usize, boon_checked::CheckedImageKernelProjectionIdV1), String> {
        let dense_index = u32::try_from(dense_index)
            .map_err(|_| "kernel checked-image route exceeds u32".to_owned())?;
        let index = self
            .routes
            .binary_search_by_key(&(domain, dense_index), |route| route.__kernel_coordinates())
            .map_err(|_| {
                format!("kernel checked-image plan has no {domain:?} route {dense_index}")
            })?;
        Ok((index, self.routes[index].__kernel_projection()))
    }

    fn projection_count(&self) -> usize {
        self.projections.len()
    }

    fn route_count(&self) -> usize {
        self.routes.len()
    }

    fn install(
        self,
        expectation: &mut CheckedImageKernelOwnershipExpectationV1,
    ) -> Result<(), String> {
        let Self {
            projections,
            routes,
        } = self;
        expectation.__kernel_install_compact_topology(projections, routes)
    }
}

/// Publication-only writer. The expected route plan is already complete when
/// this value is created and can be consulted solely for exact-key prehashes.
/// No expectation route can be read or mutated from this writer.
struct KernelCheckedImagePublicationBuilderV1<'a> {
    publication: CheckedImageKernelPublicationV1,
    ownership_plan: &'a KernelCheckedImageOwnershipPlanV1,
}

impl<'a> KernelCheckedImagePublicationBuilderV1<'a> {
    fn new(
        publication: CheckedImageKernelPublicationV1,
        ownership_plan: &'a KernelCheckedImageOwnershipPlanV1,
    ) -> Self {
        Self {
            publication,
            ownership_plan,
        }
    }

    fn __kernel_intern_projection(
        &mut self,
        key: CheckedShardProjectionKeyV2,
    ) -> Result<boon_checked::CheckedImageKernelProjectionIdV1, String> {
        self.ownership_plan.projection(&key)
    }

    fn __kernel_publish_rows(
        &mut self,
        projection: boon_checked::CheckedImageKernelProjectionIdV1,
        row_count: u32,
    ) -> Result<(), String> {
        self.publication
            .__kernel_publish_rows(projection, row_count)
    }

    fn validated_route(
        &self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
        projection: boon_checked::CheckedImageKernelProjectionIdV1,
    ) -> Result<usize, String> {
        let (route, expected) = self.ownership_plan.route(domain, dense_index)?;
        if expected != projection {
            return Err(format!(
                "kernel checked-image {domain:?} route {dense_index} differs from its independent ownership plan"
            ));
        }
        Ok(route)
    }

    fn publish_routed_rows(
        &mut self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
        projection: boon_checked::CheckedImageKernelProjectionIdV1,
        row_count: u32,
    ) -> Result<(), String> {
        let route = self.validated_route(domain, dense_index, projection)?;
        self.publication
            .__kernel_publish_routed_rows(route, projection, row_count)
            .map_err(|error| {
                format!("kernel checked-image {domain:?} route {dense_index}: {error}")
            })
    }

    fn publish_routed_dependency_row(
        &mut self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
        projection: boon_checked::CheckedImageKernelProjectionIdV1,
        relocations: &[boon_checked::CheckedImageKernelProjectionIdV1],
    ) -> Result<(), String> {
        let route = self.validated_route(domain, dense_index, projection)?;
        self.publication
            .__kernel_publish_routed_dependency_row(route, projection, relocations)
            .map_err(|error| {
                format!("kernel checked-image {domain:?} route {dense_index}: {error}")
            })
    }

    fn into_publication(self) -> CheckedImageKernelPublicationV1 {
        self.publication
    }
}

/// Derive checked ownership independently from publication IDs and writers.
///
/// This pass is deliberately complete before `checked_image_publication_v1`
/// starts its separate publication traversal. Only exact-key prehash lookup is
/// shared afterward; entity routes can never be copied out of this plan.
#[allow(clippy::too_many_arguments)]
fn checked_image_ownership_plan_v1(
    role: ProgramRole,
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    expression_declaration_targets: &[u32],
    pattern_bindings: &[KernelSemanticPatternBindingLocatorV1],
    resource_projection_requirements: &[KernelSemanticResourceProjectionLocatorV1],
    occurrence_targets: &[DeclId],
) -> Result<KernelCheckedImageOwnershipPlanV1, KernelCheckedLinkError> {
    if expression_declaration_targets.len() != layout.totals.expressions as usize {
        return Err(KernelCheckedLinkError::new(
            "packed checked ownership lexical target count differs from expressions",
        ));
    }
    let root_owner = CheckedShardOwnerKeyV2::ProgramTopLevel { role };
    let root_definition = checked_link_definition_projection(root_owner.clone());
    let root_interface = checked_link_interface_projection(root_owner.clone());

    let mut scope_locations = vec![None; layout.totals.scopes as usize];
    let mut declaration_locations = vec![None; layout.totals.declarations as usize];
    for definition in layout.definitions() {
        for local in 0..definition.scopes.len {
            let linked = definition.scopes.resolve(local, "ownership scope")? as usize;
            let slot = scope_locations.get_mut(linked).ok_or_else(|| {
                KernelCheckedLinkError::new("ownership scope exceeds its dense table")
            })?;
            if slot
                .replace((definition.owner, crate::KernelScopeId(local)))
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(
                    "ownership scope is owned twice",
                ));
            }
        }
        for local in 0..definition.declarations.len {
            let linked = definition
                .declarations
                .resolve(local, "ownership declaration")? as usize;
            let slot = declaration_locations.get_mut(linked).ok_or_else(|| {
                KernelCheckedLinkError::new("ownership declaration exceeds its dense table")
            })?;
            if slot
                .replace((definition.owner, crate::KernelDeclarationId(local)))
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(
                    "ownership declaration is owned twice",
                ));
            }
        }
    }

    let mut callable_owners = vec![None; layout.totals.declarations as usize];
    let mut callable_count = 0usize;
    for definition in snapshot.definition_refs() {
        let root = definition.linkage().root_statement.ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel definition {} has no ownership root statement",
                definition.owner().0,
            ))
        })?;
        let statement = definition
            .runtime_facts()
            .statements()
            .get(root.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} ownership root statement {} is missing",
                    definition.owner().0,
                    root.0,
                ))
            })?;
        let crate::PackedStatementKind::Function { name, .. } = statement.kind else {
            continue;
        };
        let declaration = layout.definition(definition.owner())?.public_declaration;
        let name = definition.input().symbol(name).ok_or_else(|| {
            KernelCheckedLinkError::new("packed ownership callable has a foreign name symbol")
        })?;
        let slot = callable_owners
            .get_mut(declaration.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new("ownership callable exceeds declaration namespace")
            })?;
        if slot
            .replace(CheckedShardOwnerKeyV2::Callable {
                role,
                callable_kind: CheckedShardCallableKindV2::User,
                name: name.to_owned(),
                external_identity: None,
            })
            .is_some()
        {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel ownership repeats callable declaration {}",
                declaration.0,
            )));
        }
        callable_count += 1;
    }
    for callable_layout in layout.abi_callables() {
        let callable = snapshot
            .definition_code
            .abi_callable_scheme(callable_layout.callable)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel ownership has no ABI callable {}",
                    callable_layout.callable.0,
                ))
            })?;
        if callable.kind() == crate::KernelCallableKind::User {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel ownership ABI unexpectedly contains user callable `{}`",
                callable.name(),
            )));
        }
        let slot = callable_owners
            .get_mut(callable_layout.declaration.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new("ownership ABI callable exceeds declaration namespace")
            })?;
        if slot
            .replace(CheckedShardOwnerKeyV2::Callable {
                role: callable.role(),
                callable_kind: checked_link_kernel_callable_kind(callable.kind()),
                name: callable.name().to_owned(),
                external_identity: callable.external_identity(),
            })
            .is_some()
        {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel ownership repeats ABI callable declaration {}",
                callable_layout.declaration.0,
            )));
        }
        callable_count += 1;
    }
    if callable_count != layout.totals.callables as usize {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel ownership found {callable_count} callable owners for {} callables",
            layout.totals.callables,
        )));
    }

    let mut plan = BuildingKernelCheckedImageOwnershipPlanV1::new();
    let root_definition_digest = plan.intern_projection(root_definition.clone())?;
    plan.intern_projection(root_interface)?;

    // Materialize each callable's exact rich owner key once per unique region.
    // Every entity below carries only its compact declaration/digest authority;
    // repeated scopes and rows never clone the callable name String.
    let mut callable_definition_digests = vec![None; callable_owners.len()];
    let mut callable_interface_digests = vec![None; callable_owners.len()];
    for definition in snapshot.definition_refs() {
        let declaration = layout.definition(definition.owner())?.public_declaration;
        let Some(owner) = callable_owners
            .get(declaration.0 as usize)
            .and_then(Option::as_ref)
        else {
            continue;
        };
        callable_definition_digests[declaration.0 as usize] =
            Some(plan.intern_projection(checked_link_definition_projection(owner.clone()))?);
    }
    for (declaration, owner) in callable_owners.iter().enumerate() {
        let Some(owner) = owner else { continue };
        callable_interface_digests[declaration] =
            Some(plan.intern_projection(checked_link_interface_projection(owner.clone()))?);
    }

    let mut scope_owner_declarations = Vec::with_capacity(layout.totals.scopes as usize);
    let mut scope_digests = Vec::with_capacity(layout.totals.scopes as usize);
    for scope in 0..layout.totals.scopes {
        let owner = packed_publication_scope_owner_declaration(
            layout,
            snapshot,
            &scope_locations,
            LexicalScopeId(scope),
        )?;
        let digest = owner.map_or(Ok(root_definition_digest), |declaration| {
            callable_definition_digests
                .get(declaration.0 as usize)
                .and_then(|digest| *digest)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "packed function scope {scope} has no callable owner {}",
                        declaration.0,
                    ))
                })
        })?;
        scope_owner_declarations.push(owner);
        scope_digests.push(digest);
        plan.route(CheckedImageRowDomainV2::Scope, scope as usize, digest)?;
    }

    let mut declaration_digests = vec![None; layout.totals.declarations as usize];
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        if facts.declarations().len() != facts.declaration_presentations().len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} ownership declaration rows disagree",
                owner.0,
            )));
        }
        for (declaration, presentation) in facts
            .declarations()
            .iter()
            .zip(facts.declaration_presentations())
        {
            if declaration.id != presentation.declaration {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} ownership declaration {} has presentation {}",
                    owner.0, declaration.id.0, presentation.declaration.0,
                )));
            }
            let id =
                layout.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
            let digest = if declaration.kind == crate::KernelDeclarationKind::Function {
                callable_definition_digests
                    .get(id.0 as usize)
                    .and_then(|digest| *digest)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel ownership function declaration {} has no callable owner",
                            id.0,
                        ))
                    })?
            } else {
                let scope = layout.scope(owner, presentation.scope)?;
                scope_digests
                    .get(scope.0 as usize)
                    .copied()
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel ownership declaration {} references missing scope {}",
                            id.0, scope.0,
                        ))
                    })?
            };
            plan.route(CheckedImageRowDomainV2::Declaration, id.0 as usize, digest)?;
            let slot = declaration_digests.get_mut(id.0 as usize).ok_or_else(|| {
                KernelCheckedLinkError::new("ownership declaration route exceeds dense table")
            })?;
            if slot.replace(digest).is_some() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel ownership declaration {} is routed twice",
                    id.0,
                )));
            }
        }
    }
    for callable in layout.abi_callables() {
        for declaration in std::iter::once(callable.declaration).chain(
            (0..callable.parameters.len).map(|ordinal| DeclId(callable.parameters.start + ordinal)),
        ) {
            plan.route(
                CheckedImageRowDomainV2::Declaration,
                declaration.0 as usize,
                root_definition_digest,
            )?;
            let slot = declaration_digests
                .get_mut(declaration.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(
                        "ownership ABI declaration route exceeds dense table",
                    )
                })?;
            if slot.replace(root_definition_digest).is_some() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel ownership ABI declaration {} is routed twice",
                    declaration.0,
                )));
            }
        }
    }

    let mut expression_digests = Vec::with_capacity(layout.totals.expressions as usize);
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        if facts.expression_presentations().len() != definition.input().node_count() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} ownership expression rows disagree",
                owner.0,
            )));
        }
        for (local, presentation) in facts.expression_presentations().iter().enumerate() {
            if presentation.expression.0 as usize != local {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} ownership expression {} is non-dense at {local}",
                    owner.0, presentation.expression.0,
                )));
            }
            let id =
                layout.expression(owner, KernelValueReference::Local(presentation.expression))?;
            if id.0 as usize != expression_digests.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel ownership expression {} is non-dense",
                    id.0,
                )));
            }
            let scope = layout.scope(owner, presentation.scope)?;
            expression_digests.push(scope_digests.get(scope.0 as usize).copied().ok_or_else(
                || {
                    KernelCheckedLinkError::new(format!(
                        "kernel ownership expression {} references missing scope {}",
                        id.0, scope.0,
                    ))
                },
            )?);
        }
    }

    let mut statement_digests = Vec::with_capacity(layout.totals.statements as usize);
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        if facts.statements().len() != facts.statement_presentations().len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} ownership statement rows disagree",
                owner.0,
            )));
        }
        for (statement, presentation) in facts
            .statements()
            .iter()
            .zip(facts.statement_presentations())
        {
            if statement.id != presentation.statement {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} ownership statement {} has presentation {}",
                    owner.0, statement.id.0, presentation.statement.0,
                )));
            }
            let id = layout.statement(owner, KernelStatementReference::Local(statement.id))?;
            if id.0 as usize != statement_digests.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel ownership statement {} is non-dense",
                    id.0,
                )));
            }
            let scope = layout.scope(owner, presentation.scope)?;
            statement_digests.push(scope_digests.get(scope.0 as usize).copied().ok_or_else(
                || {
                    KernelCheckedLinkError::new(format!(
                        "kernel ownership statement {} references missing scope {}",
                        id.0, scope.0,
                    ))
                },
            )?);
        }
    }
    for (statement, digest) in statement_digests.iter().copied().enumerate() {
        plan.route(CheckedImageRowDomainV2::Statement, statement, digest)?;
    }
    for (expression, digest) in expression_digests.iter().copied().enumerate() {
        plan.route(CheckedImageRowDomainV2::Expression, expression, digest)?;
    }

    let mut callable_digests = vec![None; layout.totals.declarations as usize];
    let mut route_callable = |declaration: DeclId| -> Result<(), KernelCheckedLinkError> {
        let digest = callable_interface_digests
            .get(declaration.0 as usize)
            .and_then(|digest| *digest)
            .ok_or_else(|| KernelCheckedLinkError::new("ownership callable index is incomplete"))?;
        plan.route(
            CheckedImageRowDomainV2::Callable,
            declaration.0 as usize,
            digest,
        )?;
        let slot = callable_digests
            .get_mut(declaration.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new("ownership callable route exceeds dense table")
            })?;
        if slot.replace(digest).is_some() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel ownership callable {} is routed twice",
                declaration.0,
            )));
        }
        Ok(())
    };
    for definition in snapshot.definition_refs() {
        let declaration = layout.definition(definition.owner())?.public_declaration;
        if callable_owners
            .get(declaration.0 as usize)
            .is_some_and(Option::is_some)
        {
            route_callable(declaration)?;
        }
    }
    for callable in layout.abi_callables() {
        route_callable(callable.declaration)?;
    }
    drop(route_callable);

    for definition in layout.definitions() {
        let Some(formal) = definition.context_formal else {
            continue;
        };
        let digest = callable_digests
            .get(definition.public_declaration.0 as usize)
            .and_then(|digest| *digest)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "ownership context formal {} references missing callable {}",
                    formal.0, definition.public_declaration.0,
                ))
            })?;
        plan.route(
            CheckedImageRowDomainV2::ContextFormal,
            formal.0 as usize,
            digest,
        )?;
    }

    let mut structural_sites = Vec::with_capacity(layout.totals.calls as usize);
    layout.for_each_packed_call(snapshot, |call| {
        let id = call.id()?;
        let digest = call.authored_site_digest_v4()?;
        structural_sites.push(digest);
        let owner = call
            .owner_callable()?
            .and_then(|owner| {
                callable_owners
                    .get(owner.0 as usize)
                    .and_then(|owner| owner.clone())
            })
            .unwrap_or_else(|| root_owner.clone());
        let projection_digest = plan.intern_projection(CheckedShardProjectionKeyV2 {
            owner,
            region: CheckedShardRegionV2::Invocation {
                authored_call_site_digest: digest,
                identical_site_reverse_ordinal: 0,
            },
        })?;
        plan.route(
            CheckedImageRowDomainV2::Call,
            id.0 as usize,
            projection_digest,
        )
    })?;
    structural_sites.sort_unstable();
    if let Some(repeated) = structural_sites.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel ownership calls share authored-site digest {:?}",
            repeated[0],
        )));
    }

    for (index, binding) in pattern_bindings.iter().enumerate() {
        let digest = declaration_digests
            .get(binding.declaration.0 as usize)
            .and_then(|digest| *digest)
            .unwrap_or(root_definition_digest);
        plan.route(CheckedImageRowDomainV2::PatternBinding, index, digest)?;
    }
    for (index, requirement) in resource_projection_requirements.iter().enumerate() {
        let digest = expression_digests
            .get(requirement.expression.0 as usize)
            .copied()
            .unwrap_or(root_definition_digest);
        plan.route(CheckedImageRowDomainV2::ResourceProjection, index, digest)?;
    }

    let mut ancestor_symbols = Vec::new();
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        for source in facts.sources() {
            let id = layout.source(owner, source.id.0)?;
            let presentation = expression_presentation(facts, source.expression)?;
            let owner_scope = layout.scope(owner, presentation.scope)?;
            let owner_declaration = scope_owner_declarations
                .get(owner_scope.0 as usize)
                .copied()
                .ok_or_else(|| KernelCheckedLinkError::new("ownership SOURCE scope is missing"))?;
            let anchor = layout.declaration(owner, source.declaration)?;
            let digest = match owner_declaration {
                Some(declaration) => callable_definition_digests
                    .get(declaration.0 as usize)
                    .and_then(|digest| *digest)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "ownership SOURCE callable scope has no definition digest",
                        )
                    })?,
                None => plan.intern_projection(checked_link_packed_authority_projection(
                    layout,
                    snapshot,
                    &scope_locations,
                    &declaration_locations,
                    root_owner.clone(),
                    anchor,
                    KernelPublicationPathV1::Packed(source.projection),
                    &mut ancestor_symbols,
                )?)?,
            };
            plan.route(CheckedImageRowDomainV2::Source, id.0 as usize, digest)?;
        }
    }
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        for (ordinal, state) in definition.code().states().iter().copied().enumerate() {
            let input = facts
                .states()
                .get(state.input_ordinal as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} ownership state {} references missing input {}",
                        owner.0, ordinal, state.input_ordinal,
                    ))
                })?;
            let id = layout.state(
                owner,
                u32::try_from(ordinal)
                    .map_err(|_| KernelCheckedLinkError::new("kernel state ordinal exceeds u32"))?,
            )?;
            let owner_scope = if input.kind == boon_checked::CheckedStateKind::StatementHold {
                let (statement_owner, statement) =
                    layout.local_statement_reference(snapshot, owner, input.statement)?;
                let statement_definition =
                    snapshot.definition(statement_owner).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel ownership state references missing definition {}",
                            statement_owner.0,
                        ))
                    })?;
                let presentation =
                    statement_presentation(statement_definition.runtime_facts(), statement)?;
                layout.scope(statement_owner, presentation.scope)?
            } else {
                let presentation = expression_presentation(facts, input.expression)?;
                layout.scope(owner, presentation.scope)?
            };
            let owner_declaration = scope_owner_declarations
                .get(owner_scope.0 as usize)
                .copied()
                .ok_or_else(|| KernelCheckedLinkError::new("ownership state scope is missing"))?;
            let anchor = layout.declaration(owner, input.declaration)?;
            let path = state.synthetic_ordinal().map_or(
                KernelPublicationPathV1::Packed(input.projection),
                KernelPublicationPathV1::SyntheticState,
            );
            let digest = match owner_declaration {
                Some(declaration) => callable_definition_digests
                    .get(declaration.0 as usize)
                    .and_then(|digest| *digest)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "ownership state callable scope has no definition digest",
                        )
                    })?,
                None => plan.intern_projection(checked_link_packed_authority_projection(
                    layout,
                    snapshot,
                    &scope_locations,
                    &declaration_locations,
                    root_owner.clone(),
                    anchor,
                    path,
                    &mut ancestor_symbols,
                )?)?,
            };
            plan.route(CheckedImageRowDomainV2::State, id.0 as usize, digest)?;
        }
    }
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        for list in facts.lists() {
            let id = layout.list(owner, list.id.0)?;
            let presentation = expression_presentation(facts, list.producer)?;
            let owner_scope = layout.scope(owner, presentation.scope)?;
            let owner_declaration = scope_owner_declarations
                .get(owner_scope.0 as usize)
                .copied()
                .ok_or_else(|| KernelCheckedLinkError::new("ownership LIST scope is missing"))?;
            let anchor = layout.declaration(owner, list.declaration)?;
            let digest = match owner_declaration {
                Some(declaration) => callable_definition_digests
                    .get(declaration.0 as usize)
                    .and_then(|digest| *digest)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(
                            "ownership LIST callable scope has no definition digest",
                        )
                    })?,
                None => plan.intern_projection(checked_link_packed_authority_projection(
                    layout,
                    snapshot,
                    &scope_locations,
                    &declaration_locations,
                    root_owner.clone(),
                    anchor,
                    KernelPublicationPathV1::Packed(list.projection),
                    &mut ancestor_symbols,
                )?)?,
            };
            plan.route(CheckedImageRowDomainV2::List, id.0 as usize, digest)?;
        }
    }
    for (index, target) in occurrence_targets.iter().copied().enumerate() {
        let digest = declaration_digests
            .get(target.0 as usize)
            .and_then(|digest| *digest)
            .unwrap_or(root_definition_digest);
        plan.route(CheckedImageRowDomainV2::Occurrence, index, digest)?;
    }
    plan.finish()
}

#[allow(clippy::too_many_arguments)]
fn checked_image_publication_v1(
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    expression_declaration_targets: &[u32],
    call_result_paths: &[KernelSemanticCallResultPathLocatorV1],
    pattern_bindings: &[KernelSemanticPatternBindingLocatorV1],
    resource_projection_requirements: &[KernelSemanticResourceProjectionLocatorV1],
    occurrence_targets: &[DeclId],
) -> Result<
    (
        CheckedImageKernelPublicationV1,
        CheckedImageKernelOwnershipExpectationV1,
    ),
    KernelCheckedLinkError,
> {
    if expression_declaration_targets.len() != layout.totals.expressions as usize {
        return Err(KernelCheckedLinkError::new(
            "packed checked publication lexical target count differs from expressions",
        ));
    }
    let error = |message: String| KernelCheckedLinkError::new(message);
    let root_owner = CheckedShardOwnerKeyV2::ProgramTopLevel { role };
    let root_definition = checked_link_definition_projection(root_owner.clone());
    let root_interface = checked_link_interface_projection(root_owner.clone());
    let ownership_plan = checked_image_ownership_plan_v1(
        role,
        layout,
        snapshot,
        expression_declaration_targets,
        pattern_bindings,
        resource_projection_requirements,
        occurrence_targets,
    )?;
    let (checked_image_publication, mut checked_image_ownership_expectation) =
        CheckedImageKernelPublicationV1::__kernel_new_pair(
            source_bundle_digest_v1,
            role,
            ownership_plan.projection_count(),
            ownership_plan.route_count(),
        );

    // Final dense coordinates point back into one immutable packed row. These
    // two temporary columns replace rich scope/declaration DTOs and are
    // dropped as soon as publication is sealed.
    let mut scope_locations = vec![None; layout.totals.scopes as usize];
    let mut declaration_locations = vec![None; layout.totals.declarations as usize];
    for definition in layout.definitions() {
        for local in 0..definition.scopes.len {
            let linked = definition.scopes.resolve(local, "publication scope")? as usize;
            let slot = scope_locations.get_mut(linked).ok_or_else(|| {
                KernelCheckedLinkError::new("publication scope exceeds its dense table")
            })?;
            if slot
                .replace((definition.owner, crate::KernelScopeId(local)))
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(
                    "publication scope is owned twice",
                ));
            }
        }
        for local in 0..definition.declarations.len {
            let linked = definition
                .declarations
                .resolve(local, "publication declaration")? as usize;
            let slot = declaration_locations.get_mut(linked).ok_or_else(|| {
                KernelCheckedLinkError::new("publication declaration exceeds its dense table")
            })?;
            if slot
                .replace((definition.owner, crate::KernelDeclarationId(local)))
                .is_some()
            {
                return Err(KernelCheckedLinkError::new(
                    "publication declaration is owned twice",
                ));
            }
        }
    }

    // Callable stable owners are the only publication facts that legitimately
    // own spelling. Construct one owner per callable from the permanent text
    // catalog/ABI and address it by the callable's sparse declaration ID.
    let mut callable_owners = vec![None; layout.totals.declarations as usize];
    let mut callable_count = 0usize;
    for definition in snapshot.definition_refs() {
        let root = definition.linkage().root_statement.ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel definition {} has no publication root statement",
                definition.owner().0,
            ))
        })?;
        let Some(statement) = definition.runtime_facts().statements().get(root.0 as usize) else {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} publication root statement {} is missing",
                definition.owner().0,
                root.0,
            )));
        };
        let crate::PackedStatementKind::Function { name, .. } = statement.kind else {
            continue;
        };
        let declaration = layout.definition(definition.owner())?.public_declaration;
        let name = definition.input().symbol(name).ok_or_else(|| {
            KernelCheckedLinkError::new("packed user callable has a foreign name symbol")
        })?;
        let slot = callable_owners
            .get_mut(declaration.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new("user callable exceeds declaration namespace")
            })?;
        if slot
            .replace(CheckedShardOwnerKeyV2::Callable {
                role,
                callable_kind: CheckedShardCallableKindV2::User,
                name: name.to_owned(),
                external_identity: None,
            })
            .is_some()
        {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel publication repeats callable declaration {}",
                declaration.0,
            )));
        }
        callable_count += 1;
    }
    for callable_layout in layout.abi_callables() {
        let callable = snapshot
            .definition_code
            .abi_callable_scheme(callable_layout.callable)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel publication has no ABI callable {}",
                    callable_layout.callable.0,
                ))
            })?;
        if callable.kind() == crate::KernelCallableKind::User {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel publication ABI unexpectedly contains user callable `{}`",
                callable.name(),
            )));
        }
        let slot = callable_owners
            .get_mut(callable_layout.declaration.0 as usize)
            .ok_or_else(|| {
                KernelCheckedLinkError::new("ABI callable exceeds declaration namespace")
            })?;
        if slot
            .replace(CheckedShardOwnerKeyV2::Callable {
                role: callable.role(),
                callable_kind: checked_link_kernel_callable_kind(callable.kind()),
                name: callable.name().to_owned(),
                external_identity: callable.external_identity(),
            })
            .is_some()
        {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel publication repeats ABI callable declaration {}",
                callable_layout.declaration.0,
            )));
        }
        callable_count += 1;
    }
    if callable_count != layout.totals.callables as usize {
        return Err(KernelCheckedLinkError::new(format!(
            "kernel publication found {callable_count} callable owners for {} callables",
            layout.totals.callables,
        )));
    }

    let mut scope_owners = Vec::with_capacity(layout.totals.scopes as usize);
    for scope in 0..layout.totals.scopes {
        scope_owners.push(packed_publication_scope_owner(
            layout,
            snapshot,
            &scope_locations,
            &callable_owners,
            role,
            LexicalScopeId(scope),
        )?);
    }

    let mut publication =
        KernelCheckedImagePublicationBuilderV1::new(checked_image_publication, &ownership_plan);
    let root_definition_id = publication
        .__kernel_intern_projection(root_definition.clone())
        .map_err(&error)?;
    let root_interface_id = publication
        .__kernel_intern_projection(root_interface)
        .map_err(&error)?;
    publication
        .__kernel_publish_rows(root_interface_id, 1)
        .map_err(&error)?;

    let mut scope_projections = Vec::with_capacity(scope_owners.len());
    for (scope, owner) in scope_owners.iter().cloned().enumerate() {
        let projection = publication
            .__kernel_intern_projection(checked_link_definition_projection(owner))
            .map_err(&error)?;
        publication
            .publish_routed_rows(CheckedImageRowDomainV2::Scope, scope, projection, 1)
            .map_err(&error)?;
        scope_projections.push(projection);
    }

    let mut declaration_projections = vec![None; layout.totals.declarations as usize];
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        if facts.declarations().len() != facts.declaration_presentations().len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} declaration publication rows disagree",
                owner.0,
            )));
        }
        for (declaration, presentation) in facts
            .declarations()
            .iter()
            .zip(facts.declaration_presentations())
        {
            if declaration.id != presentation.declaration {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} declaration {} has presentation {}",
                    owner.0, declaration.id.0, presentation.declaration.0,
                )));
            }
            let id =
                layout.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
            let owner = if declaration.kind == crate::KernelDeclarationKind::Function {
                callable_owners
                    .get(id.0 as usize)
                    .and_then(|owner| owner.clone())
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel function declaration {} has no stable callable owner",
                            id.0,
                        ))
                    })?
            } else {
                let scope = layout.scope(owner, presentation.scope)?;
                scope_owners.get(scope.0 as usize).cloned().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel declaration {} references missing scope {}",
                        id.0, scope.0,
                    ))
                })?
            };
            let projection = publication
                .__kernel_intern_projection(checked_link_definition_projection(owner))
                .map_err(&error)?;
            publication
                .publish_routed_rows(
                    CheckedImageRowDomainV2::Declaration,
                    id.0 as usize,
                    projection,
                    1,
                )
                .map_err(&error)?;
            let slot = declaration_projections
                .get_mut(id.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new("declaration route exceeds dense table")
                })?;
            if slot.replace(projection).is_some() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel declaration {} is published twice",
                    id.0,
                )));
            }
        }
    }
    for callable in layout.abi_callables() {
        for declaration in std::iter::once(callable.declaration).chain(
            (0..callable.parameters.len).map(|ordinal| DeclId(callable.parameters.start + ordinal)),
        ) {
            let projection = publication
                .__kernel_intern_projection(root_definition.clone())
                .map_err(&error)?;
            publication
                .publish_routed_rows(
                    CheckedImageRowDomainV2::Declaration,
                    declaration.0 as usize,
                    projection,
                    1,
                )
                .map_err(&error)?;
            let slot = declaration_projections
                .get_mut(declaration.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new("ABI declaration route exceeds dense table")
                })?;
            if slot.replace(projection).is_some() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel ABI declaration {} is published twice",
                    declaration.0,
                )));
            }
        }
    }

    let mut expression_projections = Vec::with_capacity(layout.totals.expressions as usize);
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        if facts.expression_presentations().len() != definition.input().node_count() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} expression publication rows disagree",
                owner.0,
            )));
        }
        for (local, presentation) in facts.expression_presentations().iter().enumerate() {
            if presentation.expression.0 as usize != local {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} expression presentation {} is non-dense at {local}",
                    owner.0, presentation.expression.0,
                )));
            }
            let id =
                layout.expression(owner, KernelValueReference::Local(presentation.expression))?;
            if id.0 as usize != expression_projections.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked expression {} is non-dense",
                    id.0,
                )));
            }
            let scope = layout.scope(owner, presentation.scope)?;
            let owner = scope_owners.get(scope.0 as usize).cloned().ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel expression {} references missing scope {}",
                    id.0, scope.0,
                ))
            })?;
            expression_projections.push(
                publication
                    .__kernel_intern_projection(checked_link_definition_projection(owner))
                    .map_err(&error)?,
            );
        }
    }

    let mut statement_projections = Vec::with_capacity(layout.totals.statements as usize);
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        if facts.statements().len() != facts.statement_presentations().len() {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} statement publication rows disagree",
                owner.0,
            )));
        }
        for (statement, presentation) in facts
            .statements()
            .iter()
            .zip(facts.statement_presentations())
        {
            if statement.id != presentation.statement {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} statement {} has presentation {}",
                    owner.0, statement.id.0, presentation.statement.0,
                )));
            }
            let id = layout.statement(owner, KernelStatementReference::Local(statement.id))?;
            if id.0 as usize != statement_projections.len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked statement {} is non-dense",
                    id.0,
                )));
            }
            let scope = layout.scope(owner, presentation.scope)?;
            let owner = scope_owners.get(scope.0 as usize).cloned().ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel statement {} references missing scope {}",
                    id.0, scope.0,
                ))
            })?;
            statement_projections.push(
                publication
                    .__kernel_intern_projection(checked_link_definition_projection(owner))
                    .map_err(&error)?,
            );
        }
    }

    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        for statement in definition.runtime_facts().statements() {
            let id = layout.statement(owner, KernelStatementReference::Local(statement.id))?;
            let projection = statement_projections[id.0 as usize];
            let dependency = statement
                .value
                .map(|value| {
                    definition
                        .resolve_value(value, statement.id.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))
                        .and_then(|value| layout.expression(owner, value))
                        .and_then(|expression| {
                            expression_projections
                                .get(expression.0 as usize)
                                .copied()
                                .ok_or_else(|| {
                                    KernelCheckedLinkError::new(
                                        "statement dependency has no expression projection",
                                    )
                                })
                        })
                })
                .transpose()?;
            publication
                .publish_routed_dependency_row(
                    CheckedImageRowDomainV2::Statement,
                    id.0 as usize,
                    projection,
                    dependency.as_slice(),
                )
                .map_err(&error)?;
        }
    }
    for (expression, projection) in expression_projections.iter().copied().enumerate() {
        let target = expression_declaration_targets[expression];
        let dependency = (target != 0)
            .then(|| {
                declaration_projections
                    .get(target as usize)
                    .and_then(|projection| *projection)
            })
            .flatten();
        publication
            .publish_routed_dependency_row(
                CheckedImageRowDomainV2::Expression,
                expression,
                projection,
                dependency.as_slice(),
            )
            .map_err(&error)?;
    }

    let mut callable_projections = vec![None; layout.totals.declarations as usize];
    let mut publish_callable = |declaration: DeclId,
                                publication: &mut KernelCheckedImagePublicationBuilderV1|
     -> Result<(), KernelCheckedLinkError> {
        let owner = callable_owners
            .get(declaration.0 as usize)
            .and_then(|owner| owner.clone())
            .ok_or_else(|| KernelCheckedLinkError::new("callable owner index is incomplete"))?;
        let projection = publication
            .__kernel_intern_projection(checked_link_interface_projection(owner))
            .map_err(&error)?;
        publication
            .publish_routed_rows(
                CheckedImageRowDomainV2::Callable,
                declaration.0 as usize,
                projection,
                1,
            )
            .map_err(&error)?;
        let slot = callable_projections
            .get_mut(declaration.0 as usize)
            .ok_or_else(|| KernelCheckedLinkError::new("callable route exceeds dense table"))?;
        if slot.replace(projection).is_some() {
            return Err(KernelCheckedLinkError::new(format!(
                "callable declaration {} is published twice",
                declaration.0,
            )));
        }
        Ok(())
    };
    for definition in snapshot.definition_refs() {
        let declaration = layout.definition(definition.owner())?.public_declaration;
        if callable_owners
            .get(declaration.0 as usize)
            .is_some_and(Option::is_some)
        {
            publish_callable(declaration, &mut publication)?;
        }
    }
    for callable in layout.abi_callables() {
        publish_callable(callable.declaration, &mut publication)?;
    }
    drop(publish_callable);

    for definition in layout.definitions() {
        let Some(formal) = definition.context_formal else {
            continue;
        };
        let projection = callable_projections
            .get(definition.public_declaration.0 as usize)
            .and_then(|projection| *projection)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "context formal {} references missing callable {}",
                    formal.0, definition.public_declaration.0,
                ))
            })?;
        publication
            .publish_routed_rows(
                CheckedImageRowDomainV2::ContextFormal,
                formal.0 as usize,
                projection,
                1,
            )
            .map_err(&error)?;
    }

    let mut structural_sites = BTreeSet::new();
    let mut call_projections = Vec::with_capacity(layout.totals.calls as usize);
    layout.for_each_packed_call(snapshot, |call| {
        let id = call.id()?;
        let digest = call.authored_site_digest_v4()?;
        if !structural_sites.insert(digest) {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked calls share authored-site digest {digest:?}",
            )));
        }
        let owner = call
            .owner_callable()?
            .and_then(|owner| {
                callable_owners
                    .get(owner.0 as usize)
                    .and_then(|owner| owner.clone())
            })
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
        let callable = call.callable()?;
        let callee = callable_projections
            .get(callable.0 as usize)
            .and_then(|projection| *projection)
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel call {} references missing callable {}",
                    id.0, callable.0,
                ))
            })?;
        publication
            .publish_routed_dependency_row(
                CheckedImageRowDomainV2::Call,
                id.0 as usize,
                projection,
                &[callee],
            )
            .map_err(&error)?;
        call_projections.push(projection);
        Ok(())
    })?;
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
            .publish_routed_rows(
                CheckedImageRowDomainV2::PatternBinding,
                index,
                projection,
                1,
            )
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
            .publish_routed_dependency_row(
                CheckedImageRowDomainV2::ResourceProjection,
                index,
                projection,
                target.as_slice(),
            )
            .map_err(&error)?;
    }

    let mut ancestor_symbols = Vec::new();
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        for source in facts.sources() {
            let id = layout.source(owner, source.id.0)?;
            let presentation = expression_presentation(facts, source.expression)?;
            let owner_scope = layout.scope(owner, presentation.scope)?;
            let owner_key = scope_owners
                .get(owner_scope.0 as usize)
                .cloned()
                .ok_or_else(|| KernelCheckedLinkError::new("SOURCE owner scope is missing"))?;
            let anchor = layout.declaration(owner, source.declaration)?;
            let projection = publication
                .__kernel_intern_projection(checked_link_packed_authority_projection(
                    layout,
                    snapshot,
                    &scope_locations,
                    &declaration_locations,
                    owner_key,
                    anchor,
                    KernelPublicationPathV1::Packed(source.projection),
                    &mut ancestor_symbols,
                )?)
                .map_err(&error)?;
            let expression =
                layout.expression(owner, KernelValueReference::Local(source.expression))?;
            let dependency = expression_projections
                .get(expression.0 as usize)
                .copied()
                .ok_or_else(|| KernelCheckedLinkError::new("SOURCE expression is missing"))?;
            publication
                .publish_routed_dependency_row(
                    CheckedImageRowDomainV2::Source,
                    id.0 as usize,
                    projection,
                    &[dependency],
                )
                .map_err(&error)?;
        }
    }
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        for (ordinal, state) in definition.code().states().iter().copied().enumerate() {
            let input = facts
                .states()
                .get(state.input_ordinal as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} state {} references missing input {}",
                        owner.0, ordinal, state.input_ordinal,
                    ))
                })?;
            let id = layout.state(
                owner,
                u32::try_from(ordinal)
                    .map_err(|_| KernelCheckedLinkError::new("kernel state ordinal exceeds u32"))?,
            )?;
            let owner_scope = if input.kind == boon_checked::CheckedStateKind::StatementHold {
                let (statement_owner, statement) =
                    layout.local_statement_reference(snapshot, owner, input.statement)?;
                let statement_definition =
                    snapshot.definition(statement_owner).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel state references missing definition {}",
                            statement_owner.0,
                        ))
                    })?;
                let presentation =
                    statement_presentation(statement_definition.runtime_facts(), statement)?;
                layout.scope(statement_owner, presentation.scope)?
            } else {
                let presentation = expression_presentation(facts, input.expression)?;
                layout.scope(owner, presentation.scope)?
            };
            let owner_key = scope_owners
                .get(owner_scope.0 as usize)
                .cloned()
                .ok_or_else(|| KernelCheckedLinkError::new("state owner scope is missing"))?;
            let anchor = layout.declaration(owner, input.declaration)?;
            let path = state.synthetic_ordinal().map_or(
                KernelPublicationPathV1::Packed(input.projection),
                KernelPublicationPathV1::SyntheticState,
            );
            let projection = publication
                .__kernel_intern_projection(checked_link_packed_authority_projection(
                    layout,
                    snapshot,
                    &scope_locations,
                    &declaration_locations,
                    owner_key,
                    anchor,
                    path,
                    &mut ancestor_symbols,
                )?)
                .map_err(&error)?;
            let expression =
                layout.expression(owner, KernelValueReference::Local(input.expression))?;
            let initial = layout.expression(
                owner,
                definition
                    .resolve_value(input.initial, input.expression.0 as usize)
                    .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?,
            )?;
            let binding = layout.declaration(owner, input.binding_declaration)?;
            let mut dependencies = [root_definition_id; 3];
            let mut dependency_count = 0;
            for dependency in [
                expression_projections.get(expression.0 as usize).copied(),
                expression_projections.get(initial.0 as usize).copied(),
                declaration_projections
                    .get(binding.0 as usize)
                    .and_then(|projection| *projection),
            ]
            .into_iter()
            .flatten()
            {
                dependencies[dependency_count] = dependency;
                dependency_count += 1;
            }
            publication
                .publish_routed_dependency_row(
                    CheckedImageRowDomainV2::State,
                    id.0 as usize,
                    projection,
                    &dependencies[..dependency_count],
                )
                .map_err(&error)?;
        }
    }
    for definition in snapshot.definition_refs() {
        let owner = definition.owner();
        let facts = definition.runtime_facts();
        for list in facts.lists() {
            let id = layout.list(owner, list.id.0)?;
            let presentation = expression_presentation(facts, list.producer)?;
            let owner_scope = layout.scope(owner, presentation.scope)?;
            let owner_key = scope_owners
                .get(owner_scope.0 as usize)
                .cloned()
                .ok_or_else(|| KernelCheckedLinkError::new("LIST owner scope is missing"))?;
            let anchor = layout.declaration(owner, list.declaration)?;
            let projection = publication
                .__kernel_intern_projection(checked_link_packed_authority_projection(
                    layout,
                    snapshot,
                    &scope_locations,
                    &declaration_locations,
                    owner_key,
                    anchor,
                    KernelPublicationPathV1::Packed(list.projection),
                    &mut ancestor_symbols,
                )?)
                .map_err(&error)?;
            let producer = layout.expression(owner, KernelValueReference::Local(list.producer))?;
            let dependency = expression_projections
                .get(producer.0 as usize)
                .copied()
                .ok_or_else(|| KernelCheckedLinkError::new("LIST producer is missing"))?;
            publication
                .publish_routed_dependency_row(
                    CheckedImageRowDomainV2::List,
                    id.0 as usize,
                    projection,
                    &[dependency],
                )
                .map_err(&error)?;
        }
    }
    for (index, target) in occurrence_targets.iter().copied().enumerate() {
        let projection = declaration_projections
            .get(target.0 as usize)
            .and_then(|projection| *projection)
            .unwrap_or(root_definition_id);
        publication
            .publish_routed_rows(CheckedImageRowDomainV2::Occurrence, index, projection, 1)
            .map_err(&error)?;
    }
    let publication = publication.into_publication();
    ownership_plan
        .install(&mut checked_image_ownership_expectation)
        .map_err(&error)?;
    Ok((publication, checked_image_ownership_expectation))
}

#[cfg(test)]
struct KernelCheckedImageRichOraclePublicationBuilderV1 {
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    plan: BuildingKernelCheckedImageOwnershipPlanV1,
    payloads: Vec<KernelCheckedImageRichOraclePayloadV1>,
}

#[cfg(test)]
#[derive(Default)]
struct KernelCheckedImageRichOraclePayloadV1 {
    row_count: u32,
    dependency_row_count: u32,
    relocations: Vec<TempCheckedImageProjectionIdV1>,
}

#[cfg(test)]
impl KernelCheckedImageRichOraclePublicationBuilderV1 {
    fn new(source_bundle_digest_v1: SourceBundleDigestV1, role: ProgramRole) -> Self {
        Self {
            source_bundle_digest_v1,
            role,
            plan: BuildingKernelCheckedImageOwnershipPlanV1::new(),
            payloads: Vec::new(),
        }
    }

    fn __kernel_intern_projection(
        &mut self,
        key: CheckedShardProjectionKeyV2,
    ) -> Result<TempCheckedImageProjectionIdV1, String> {
        let projection = self
            .plan
            .intern_projection(key)
            .map_err(|error| error.to_string())?;
        if projection.0 as usize == self.payloads.len() {
            self.payloads
                .push(KernelCheckedImageRichOraclePayloadV1::default());
        }
        Ok(projection)
    }

    fn __kernel_publish_rows(
        &mut self,
        projection: TempCheckedImageProjectionIdV1,
        row_count: u32,
    ) -> Result<(), String> {
        let payload = self
            .payloads
            .get_mut(projection.0 as usize)
            .ok_or_else(|| "rich oracle row references a missing projection".to_owned())?;
        payload.row_count = payload
            .row_count
            .checked_add(row_count)
            .ok_or_else(|| "rich oracle row count exceeds u32".to_owned())?;
        Ok(())
    }

    fn __kernel_publish_dependency_row(
        &mut self,
        projection: TempCheckedImageProjectionIdV1,
        relocations: impl IntoIterator<Item = TempCheckedImageProjectionIdV1>,
    ) -> Result<(), String> {
        let projection_count = self.payloads.len();
        let payload = self
            .payloads
            .get_mut(projection.0 as usize)
            .ok_or_else(|| "rich oracle dependency source is missing".to_owned())?;
        payload.row_count = payload
            .row_count
            .checked_add(1)
            .ok_or_else(|| "rich oracle row count exceeds u32".to_owned())?;
        let mut has_relocation = false;
        for target in relocations {
            if target.0 as usize >= projection_count {
                return Err("rich oracle dependency target is missing".to_owned());
            }
            if target != projection {
                payload.relocations.push(target);
                has_relocation = true;
            }
        }
        if has_relocation {
            payload.dependency_row_count = payload
                .dependency_row_count
                .checked_add(1)
                .ok_or_else(|| "rich oracle dependency count exceeds u32".to_owned())?;
        }
        Ok(())
    }

    fn __kernel_route(
        &mut self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
        projection: TempCheckedImageProjectionIdV1,
    ) -> Result<(), String> {
        self.plan
            .route(domain, dense_index, projection)
            .map_err(|error| error.to_string())
    }

    fn into_parts(
        self,
    ) -> Result<
        (
            CheckedImageKernelPublicationV1,
            CheckedImageKernelOwnershipExpectationV1,
        ),
        String,
    > {
        let Self {
            source_bundle_digest_v1,
            role,
            plan,
            payloads,
        } = self;
        let mut canonical_by_temp = vec![u32::MAX; plan.prehash_by_key.len()];
        for (canonical, entry) in plan.prehash_by_key.iter().enumerate() {
            canonical_by_temp[entry.projection.0 as usize] = u32::try_from(canonical)
                .map_err(|_| "rich oracle projection catalog exceeds u32".to_owned())?;
        }
        let plan = plan.finish().map_err(|error| error.to_string())?;
        let (mut publication, mut expectation) = CheckedImageKernelPublicationV1::__kernel_new_pair(
            source_bundle_digest_v1,
            role,
            plan.projection_count(),
            plan.route_count(),
        );
        let mut route_ordinals_by_projection = vec![Vec::new(); plan.projection_count()];
        for (route_ordinal, route) in plan.routes.iter().enumerate() {
            route_ordinals_by_projection
                .get_mut(route.__kernel_projection().as_usize())
                .ok_or_else(|| "rich oracle route references a missing projection".to_owned())?
                .push(route_ordinal);
        }
        for (temporary, payload) in payloads.into_iter().enumerate() {
            let projection = boon_checked::CheckedImageKernelProjectionIdV1::__kernel_new(
                canonical_by_temp[temporary],
            );
            let independent_rows = payload
                .row_count
                .checked_sub(payload.dependency_row_count)
                .ok_or_else(|| "rich oracle dependency count exceeds its row count".to_owned())?;
            let relocations = payload
                .relocations
                .iter()
                .map(|target| {
                    boon_checked::CheckedImageKernelProjectionIdV1::__kernel_new(
                        canonical_by_temp[target.0 as usize],
                    )
                })
                .collect::<Vec<_>>();
            let routes = &route_ordinals_by_projection[projection.as_usize()];
            let route_count = u32::try_from(routes.len())
                .map_err(|_| "rich oracle projection route count exceeds u32".to_owned())?;
            if route_count > payload.row_count {
                return Err(format!(
                    "rich oracle projection {} has {route_count} routes for {} rows",
                    projection.as_usize(),
                    payload.row_count,
                ));
            }
            let routed_dependencies = payload.dependency_row_count.min(route_count);
            let routed_independent = route_count - routed_dependencies;
            let remaining_independent = independent_rows
                .checked_sub(routed_independent)
                .ok_or_else(|| {
                    format!(
                        "rich oracle projection {} has too many independent routes",
                        projection.as_usize(),
                    )
                })?;
            for route_ordinal in routes.iter().copied().take(routed_dependencies as usize) {
                publication.__kernel_publish_routed_dependency_row(
                    route_ordinal,
                    projection,
                    &relocations,
                )?;
            }
            for route_ordinal in routes.iter().copied().skip(routed_dependencies as usize) {
                publication.__kernel_publish_routed_rows(route_ordinal, projection, 1)?;
            }
            if remaining_independent != 0 {
                publication.__kernel_publish_rows(projection, remaining_independent)?;
            }
            for _ in routed_dependencies..payload.dependency_row_count {
                publication.__kernel_publish_dependency_row(projection, &relocations)?;
            }
        }
        plan.install(&mut expectation)?;
        Ok((publication, expectation))
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn checked_image_publication_rich_oracle_v1(
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    scopes: &[CheckedScope],
    declarations: &[CheckedDeclaration],
    statements: &[CheckedStatement],
    expressions: &[CheckedExpression],
    callables: &[CheckedCallableSignature],
    context_formals: &[CheckedContextFormal],
    call_result_paths: &[KernelSemanticCallResultPathLocatorV1],
    pattern_bindings: &[KernelSemanticPatternBindingLocatorV1],
    resource_projection_requirements: &[KernelSemanticResourceProjectionLocatorV1],
    sources: &[CheckedSource],
    states: &[CheckedState],
    lists: &[CheckedList],
    occurrence_targets: &[DeclId],
) -> Result<
    (
        CheckedImageKernelPublicationV1,
        CheckedImageKernelOwnershipExpectationV1,
    ),
    KernelCheckedLinkError,
> {
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
        KernelCheckedImageRichOraclePublicationBuilderV1::new(source_bundle_digest_v1, role);
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

    let mut structural_sites = BTreeSet::new();
    let mut call_projections = Vec::with_capacity(layout.totals.calls as usize);
    layout.for_each_packed_call(snapshot, |call| {
        let id = call.id()?;
        let digest = call.authored_site_digest_v4()?;
        if !structural_sites.insert(digest) {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked calls share authored-site digest {digest:?}",
            )));
        }
        let owner = call
            .owner_callable()?
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
        let callable = call.callable()?;
        let callee = *callable_projections.get(&callable).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel call {} references missing callable {}",
                id.0, callable.0,
            ))
        })?;
        publication
            .__kernel_publish_dependency_row(projection, [callee])
            .map_err(&error)?;
        publication
            .__kernel_route(CheckedImageRowDomainV2::Call, id.0 as usize, projection)
            .map_err(&error)?;
        call_projections.push(projection);
        Ok(())
    })?;
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
    publication.into_parts().map_err(&error)
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
            let facts = definition.runtime_facts();
            let code = definition.code();
            let scopes = take_range(&mut totals.scopes, facts.scopes().len(), "scope")?;
            let expressions = take_range(
                &mut totals.expressions,
                definition.input().nodes().len(),
                "expression",
            )?;
            let statements = take_range(
                &mut totals.statements,
                facts.statements().len(),
                "statement",
            )?;
            let declarations = take_range(
                &mut totals.declarations,
                facts.declarations().len(),
                "declaration",
            )?;
            let type_variables = take_range(
                &mut totals.type_variables,
                code.alpha_variable_count(),
                "type variable",
            )?;
            let calls = take_range(&mut totals.calls, definition.call_count(), "call")?;
            let sources = take_range(&mut totals.sources, facts.sources().len(), "source")?;
            let states = take_range(&mut totals.states, definition.state_count(), "state")?;
            let lists = take_range(&mut totals.lists, facts.lists().len(), "list")?;
            let linkage = definition.linkage();
            let root_statement = linkage.root_statement.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} omits its direct-linker root statement",
                    owner.0,
                ))
            })?;
            if matches!(
                facts.statements().get(root_statement.0 as usize),
                Some(crate::PackedStatement {
                    kind: crate::PackedStatementKind::Function { .. },
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
        let referenced_abi_callables = referenced_abi_callable_ids(snapshot)?;
        let mut abi_callables = Vec::with_capacity(referenced_abi_callables.len());
        for callable_id in referenced_abi_callables {
            let callable = snapshot
                .definition_code
                .abi_callable_scheme(callable_id)
                .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked linker references ABI callable ID {} absent from its packed ABI catalog",
                    callable_id.0,
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
                callable.parameters().len(),
                "ABI parameter declaration",
            )?;
            let type_variables = take_range(
                &mut totals.type_variables,
                callable.variable_count() as usize,
                "ABI type variable",
            )?;
            abi_callables.push(KernelCheckedAbiCallableLayout {
                callable: callable_id,
                declaration,
                parameters: parameter_range,
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
            let facts = definition.runtime_facts();
            definitions[index].containing_scope = match facts.containing_scope() {
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
            definition_declarations_end,
            totals,
        };
        layout.validate_references(snapshot)?;
        Ok(layout)
    }

    /// Link the checked-image and semantic topology without projecting a rich
    /// checked row family. This helper is also reused by the EditorRich oracle
    /// so both products prove the exact same dense routes and publication
    /// order.
    fn packed_link_topology(
        &self,
        snapshot: &KernelCheckedSnapshot,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
    ) -> Result<KernelPackedLinkTopologyV1, KernelCheckedLinkError> {
        let (call_result_paths, call_result_path_symbols) =
            self.pack_call_result_paths(snapshot)?;
        let pattern_bindings = self.pack_pattern_bindings(snapshot)?;
        let resource_projections = self.semantic_resource_projection_locators(snapshot)?;
        let expression_declaration_targets = self.expression_declaration_targets(snapshot)?;
        let occurrence_targets =
            self.occurrence_targets(snapshot, &expression_declaration_targets)?;
        let (checked_image_publication, checked_image_ownership_expectation) =
            checked_image_publication_v1(
                source_bundle_digest_v1,
                role,
                self,
                snapshot,
                &expression_declaration_targets,
                &call_result_paths,
                &pattern_bindings,
                &resource_projections,
                &occurrence_targets,
            )?;
        Ok(KernelPackedLinkTopologyV1 {
            call_result_paths,
            call_result_path_symbols,
            pattern_bindings,
            resource_projections,
            occurrence_targets,
            checked_image_publication,
            checked_image_ownership_expectation,
        })
    }

    /// Produce the ordinary runtime checked linker handoff directly from the
    /// packed snapshot.
    ///
    /// Unlike [`Self::materialize_rows`], this function has no projection
    /// demand parameter and cannot allocate rich compatibility rows. Editor
    /// tools and differential tests keep using the explicit rich method.
    pub fn link_runtime_packed(
        &self,
        snapshot: &KernelCheckedSnapshot,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
    ) -> Result<KernelRuntimePackedLinkV1, KernelCheckedLinkError> {
        let runtime_flow_terms = self.materialize_runtime_flow_terms(snapshot)?;
        let KernelPackedLinkTopologyV1 {
            call_result_paths,
            call_result_path_symbols,
            pattern_bindings,
            resource_projections,
            occurrence_targets,
            checked_image_publication,
            checked_image_ownership_expectation,
        } = self.packed_link_topology(snapshot, source_bundle_digest_v1, role)?;
        let occurrence_count = occurrence_targets.len();
        let checked_image_pairing = checked_image_publication.__kernel_unfrozen_pairing();
        let semantic_input = KernelSemanticInputConstructionV1::from_linked_rows(
            source_bundle_digest_v1,
            role,
            KernelCheckedRowProjectionDemand::RuntimePacked,
            snapshot,
            self,
            call_result_paths,
            call_result_path_symbols,
            pattern_bindings,
            resource_projections,
            occurrence_count,
            checked_image_ownership_expectation,
            checked_image_pairing,
        )?;
        Ok(KernelRuntimePackedLinkV1 {
            semantic_input,
            runtime_flow_terms,
            checked_image_publication,
        })
    }

    /// Materialize every currently kernel-owned checked row through this one
    /// relocation plan. ABI rows are appended before calls are linked so call
    /// targets resolve in the same namespace as user definitions.
    pub fn materialize_rows(
        &self,
        snapshot: &KernelCheckedSnapshot,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        projection_demand: KernelCheckedRowProjectionDemand,
    ) -> Result<KernelCheckedRows, KernelCheckedLinkError> {
        let mut type_cache = snapshot.definition_code.materialization_cache();
        let materialize_expression_rows = |type_cache: &mut DefinitionTypeMaterializationCache| {
            Ok::<_, KernelCheckedLinkError>((
                self.materialize_expressions_with_cache(snapshot, type_cache)?
                    .into_vec(),
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
                    let expression_worker = scope.spawn(|| {
                        let mut expression_type_cache =
                            snapshot.definition_code.materialization_cache();
                        materialize_expression_rows(&mut expression_type_cache)
                    });
                    let base = self.materialize_base_rows(
                        snapshot,
                        role,
                        projection_demand,
                        &mut type_cache,
                    )?;
                    let expressions = expression_worker.join().map_err(|_| {
                        KernelCheckedLinkError::new(
                            "kernel checked expression materialization worker panicked",
                        )
                    })??;
                    Ok::<_, KernelCheckedLinkError>((base, expressions))
                })?
            } else {
                (
                    self.materialize_base_rows(snapshot, role, projection_demand, &mut type_cache)?,
                    materialize_expression_rows(&mut type_cache)?,
                )
            };
        #[cfg(target_family = "wasm")]
        let (base, (expressions, runtime_flow_terms)) = (
            self.materialize_base_rows(snapshot, role, projection_demand, &mut type_cache)?,
            materialize_expression_rows(&mut type_cache)?,
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
        let (calls, call_occurrences): (Box<[CheckedCall]>, Box<[StableOccurrenceKey]>) =
            match projection_demand {
                KernelCheckedRowProjectionDemand::RuntimePacked => (Box::new([]), Box::new([])),
                KernelCheckedRowProjectionDemand::EditorRich => self.materialize_calls_with_cache(
                    snapshot,
                    &callables,
                    &declarations,
                    &mut type_cache,
                )?,
            };
        drop(type_cache);
        let KernelPackedLinkTopologyV1 {
            call_result_paths: packed_call_result_paths,
            call_result_path_symbols: packed_call_result_path_symbols,
            pattern_bindings: packed_pattern_bindings,
            resource_projections: semantic_resource_projections,
            occurrence_targets,
            checked_image_publication,
            checked_image_ownership_expectation,
        } = self.packed_link_topology(snapshot, source_bundle_digest_v1, role)?;
        let call_result_paths = match projection_demand {
            KernelCheckedRowProjectionDemand::RuntimePacked => Box::new([]),
            KernelCheckedRowProjectionDemand::EditorRich => self.materialize_call_result_paths(
                snapshot,
                &packed_call_result_paths,
                &packed_call_result_path_symbols,
            )?,
        };
        let pattern_bindings = match projection_demand {
            KernelCheckedRowProjectionDemand::RuntimePacked => Box::new([]),
            KernelCheckedRowProjectionDemand::EditorRich => {
                self.materialize_pattern_bindings_from_packed(snapshot, &packed_pattern_bindings)?
            }
        };
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
        let rich_definition_execution_oracle = matches!(
            projection_demand,
            KernelCheckedRowProjectionDemand::EditorRich
        )
        .then(|| {
            self.build_semantic_definition_execution_store_rich_oracle(
                snapshot,
                &scopes,
                &declarations,
                &statements,
                &calls,
            )
        })
        .transpose()?;
        #[cfg(test)]
        if matches!(
            projection_demand,
            KernelCheckedRowProjectionDemand::EditorRich
        ) {
            let (rich_publication_oracle, rich_expectation_oracle) =
                checked_image_publication_rich_oracle_v1(
                    source_bundle_digest_v1,
                    role,
                    self,
                    snapshot,
                    &scopes,
                    &declarations,
                    &statements,
                    &expressions,
                    &callables,
                    &context_formals,
                    &packed_call_result_paths,
                    &packed_pattern_bindings,
                    &semantic_resource_projections,
                    &sources,
                    &states,
                    &lists,
                    &occurrence_targets,
                )?;
            if checked_image_publication != rich_publication_oracle {
                return Err(KernelCheckedLinkError::new(
                    "packed checked-image publication differs from rich editor oracle",
                ));
            }
            // The ownership expectations intentionally use different writers
            // and insertion orders. Freeze the rich oracle against its own
            // publication so this test still proves its complete route plan;
            // the production expectation is frozen later, after compiler
            // metadata rows are appended.
            let mut rich_expectation_oracle = rich_expectation_oracle;
            rich_expectation_oracle
                .__kernel_freeze_against(&rich_publication_oracle)
                .map_err(KernelCheckedLinkError::new)?;
        }
        let checked_image_pairing = checked_image_publication.__kernel_unfrozen_pairing();
        let semantic_input = KernelSemanticInputConstructionV1::from_linked_rows(
            source_bundle_digest_v1,
            role,
            projection_demand,
            snapshot,
            self,
            packed_call_result_paths,
            packed_call_result_path_symbols,
            packed_pattern_bindings,
            semantic_resource_projections,
            occurrence_targets.len(),
            checked_image_ownership_expectation,
            checked_image_pairing,
        )?;
        #[cfg(debug_assertions)]
        if matches!(
            projection_demand,
            KernelCheckedRowProjectionDemand::EditorRich
        ) {
            semantic_input.validate_rich_call_topology(
                &calls,
                &call_occurrences,
                &callables,
                &declarations,
            )?;
        }
        #[cfg(test)]
        if rich_definition_execution_oracle.is_some_and(|oracle| {
            semantic_input.materialize_rich_definition_execution_templates() != oracle
        }) {
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
        snapshot: &KernelCheckedSnapshot,
        role: ProgramRole,
        projection_demand: KernelCheckedRowProjectionDemand,
        type_cache: &mut DefinitionTypeMaterializationCache,
    ) -> Result<KernelCheckedBaseRows, KernelCheckedLinkError> {
        let scopes = self.materialize_scopes(snapshot)?;
        let mut declarations = self
            .materialize_declarations_with_cache(snapshot, type_cache)?
            .into_vec();
        let statements = self.materialize_statements(snapshot)?;
        let sources = self.materialize_sources_with_cache(snapshot, type_cache)?;
        let states = self.materialize_states_with_cache(snapshot, type_cache)?;
        let lists = self.materialize_lists_with_cache(snapshot, type_cache)?;
        let (mut callables, context_formals): (
            Vec<CheckedCallableSignature>,
            Box<[CheckedContextFormal]>,
        ) = match projection_demand {
            KernelCheckedRowProjectionDemand::RuntimePacked => (Vec::new(), Box::new([])),
            KernelCheckedRowProjectionDemand::EditorRich => {
                let (callables, context_formals) =
                    self.materialize_user_callables_with_cache(snapshot, role, type_cache)?;
                (callables.into_vec(), context_formals)
            }
        };
        let (abi_callables, abi_declarations) =
            self.materialize_abi_callables_with_cache(snapshot, projection_demand, type_cache)?;
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
            if code.expressions().len() != definition.input().nodes().len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {owner_index} has {} expression term roots for {} expressions",
                    code.expressions().len(),
                    definition.input().nodes().len()
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

    /// Link pattern bindings into compact IDs and an optional interned field
    /// symbol. The selector is read from the exact match-arm execution shape;
    /// static arm pruning can remove the surrounding WHEN edge without
    /// removing this authored binding authority.
    fn pack_pattern_bindings(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[KernelSemanticPatternBindingLocatorV1]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "pattern binding")?;
        let mut bindings = Vec::new();
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            let facts = definition.runtime_facts();
            for shape in facts.execution_shapes() {
                let crate::PackedExecutionShape::MatchArm {
                    expression,
                    selector,
                    ..
                } = shape
                else {
                    continue;
                };
                let arm_bindings = facts
                    .execution_match_bindings(shape)
                    .expect("match-arm shape owns a binding span");
                let arm = definition
                    .input()
                    .nodes()
                    .get(expression.0 as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} match-arm shape references missing expression {}",
                            owner.0, expression.0,
                        ))
                    })?;
                let crate::PackedKernelOwnerNodeKind::MatchArm { pattern } = arm.kind else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} expression {} has a match-arm shape but kind {:?}",
                        owner.0, expression.0, arm.kind,
                    )));
                };
                for (ordinal, binding) in arm_bindings.iter().enumerate() {
                    let declaration = facts
                        .declarations
                        ()
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
                        crate::PackedKernelPattern::Tag { fields, .. }
                            if definition.input().path(fields).is_some_and(|fields| {
                                fields.iter().any(|field| field == declaration.name)
                            }) =>
                        {
                            Some(declaration.name)
                        }
                        _ => None,
                    };
                    bindings.push(KernelSemanticPatternBindingLocatorV1 {
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

    /// Explicit rich pattern projection for editor/export and differential
    /// tests. RuntimePacked retains only `pack_pattern_bindings`.
    pub fn materialize_pattern_bindings(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[CheckedPatternBinding]>, KernelCheckedLinkError> {
        self.materialize_pattern_bindings_from_packed(
            snapshot,
            &self.pack_pattern_bindings(snapshot)?,
        )
    }

    fn materialize_pattern_bindings_from_packed(
        &self,
        snapshot: &KernelCheckedSnapshot,
        packed: &[KernelSemanticPatternBindingLocatorV1],
    ) -> Result<Box<[CheckedPatternBinding]>, KernelCheckedLinkError> {
        packed
            .iter()
            .map(|binding| {
                Ok(CheckedPatternBinding {
                    declaration: binding.declaration,
                    selector: binding.selector,
                    projection: binding
                        .projection
                        .map(|symbol| {
                            snapshot
                                .definition_code
                                .symbol(symbol)
                                .map(str::to_owned)
                                .ok_or_else(|| {
                                    KernelCheckedLinkError::new(
                                        "kernel pattern binding has a foreign symbol",
                                    )
                                })
                        })
                        .transpose()?
                        .into_iter()
                        .collect(),
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Vec::into_boxed_slice)
    }

    /// Derive each call's stable storage path into one flat interned-symbol
    /// column. The traversal reuses one projection buffer and one visiting
    /// bitmap for every call; no path owns a `String` or nested vector.
    fn pack_call_result_paths(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<
        (
            Box<[KernelSemanticCallResultPathLocatorV1]>,
            Box<[SymbolId]>,
        ),
        KernelCheckedLinkError,
    > {
        self.validate_snapshot_definition_count(snapshot, "call-result path")?;

        // DeclId zero is reserved, while `totals.declarations` is the exact
        // end of the linked namespace. Retain only one optional packed root
        // coordinate per declaration instead of materializing rich
        // declarations and callable signatures merely to rediscover these
        // values.
        let mut roots = vec![None; self.totals.declarations as usize];
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for declaration in definition.runtime_facts().declarations() {
                let linked =
                    self.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
                let Some(value) = declaration.value else {
                    continue;
                };
                let value = definition
                    .resolve_value(value, declaration.id.0 as usize)
                    .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                let value = self.expression(owner, value)?;
                let slot = roots.get_mut(linked.0 as usize).ok_or_else(|| {
                    KernelCheckedLinkError::new("declaration result-path root exceeds dense table")
                })?;
                if slot.replace(value).is_some() {
                    return Err(KernelCheckedLinkError::new(
                        "declaration result-path root is published twice",
                    ));
                }
            }

            // Function declarations intentionally have no declaration value;
            // their result expression is owned by the normalized definition
            // linkage. This is the packed equivalent of the old callable
            // signature fallback.
            if let Some(result) = definition.linkage().result_expression {
                let callable = self.definition(owner)?.public_declaration;
                let slot = roots.get_mut(callable.0 as usize).ok_or_else(|| {
                    KernelCheckedLinkError::new("callable result-path root exceeds dense table")
                })?;
                if slot.is_none() {
                    *slot = Some(self.expression(owner, KernelValueReference::Local(result))?);
                }
            }
        }

        // Calls are sparse among expression rows. One reusable dense scratch
        // column makes recursive call traversal allocation-free and avoids a
        // per-node map or scan.
        let mut call_by_expression = vec![None; self.totals.expressions as usize];
        self.for_each_packed_call(snapshot, |call| {
            let expression = call.expression()?;
            let slot = call_by_expression
                .get_mut(expression.0 as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(
                        "packed call expression exceeds the dense expression table",
                    )
                })?;
            if slot.replace(call.id()?).is_some() {
                return Err(KernelCheckedLinkError::new(
                    "packed call expression is published twice",
                ));
            }
            Ok(())
        })?;

        let mut visiting = vec![false; self.totals.expressions as usize];
        let mut projection = Vec::new();
        let mut symbols = Vec::new();
        let mut result = Vec::new();
        self.for_each_packed_call(snapshot, |call| {
            let call_id = call.id()?;
            let call_expression = call.expression()?;
            let definition = call.definition()?;
            let local_expression = call
                .code()?
                .call_expression(call.ordinal as usize)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} omits packed call {} expression",
                        call.owner.0, call.ordinal,
                    ))
                })?;
            let presentation =
                expression_presentation(definition.runtime_facts(), local_expression)?;
            let anchor = match presentation.declaration {
                Some(declaration) => Some(self.declaration(call.owner, declaration)?),
                None => self.lexical_declaration_for_scope(
                    snapshot,
                    call.owner,
                    presentation.declaration_scope.unwrap_or(presentation.scope),
                )?,
            };
            let Some(anchor) = anchor else {
                return Ok(());
            };
            let Some(root) = roots.get(anchor.0 as usize).copied().flatten() else {
                return Ok(());
            };
            projection.clear();
            if packed_projection_symbols_to_expression_with_scratch(
                self,
                snapshot,
                &call_by_expression,
                root,
                call_expression,
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
                    call: call_id,
                    anchor,
                    projection_start,
                    projection_len,
                });
            }
            Ok(())
        })?;
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
    /// Dense lexical declaration dependency for every linked expression.
    ///
    /// Zero means the expression is not a declaration-backed READ/DRAIN. The
    /// reserved declaration identity makes that encoding unambiguous, while
    /// `u32::MAX` remains construction-only so duplicate lexical rows fail
    /// before the column is frozen.
    fn expression_declaration_targets(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[u32]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "lexical dependency")?;
        let mut targets = vec![u32::MAX; self.totals.expressions as usize];
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for binding in definition.runtime_facts().lexical_bindings() {
                let expression =
                    self.expression(owner, KernelValueReference::Local(binding.expression))?;
                let slot = targets.get_mut(expression.0 as usize).ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} lexical dependency references missing expression {}",
                        owner.0, binding.expression.0,
                    ))
                })?;
                if *slot != u32::MAX {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} repeats lexical dependency expression {}",
                        owner.0, binding.expression.0,
                    )));
                }
                *slot = match binding.target {
                    crate::KernelLexicalBindingTargetInput::Declaration(target) => {
                        self.declaration(owner, target)?.0
                    }
                    crate::KernelLexicalBindingTargetInput::ContextFormal { .. }
                    | crate::KernelLexicalBindingTargetInput::Value { .. }
                    | crate::KernelLexicalBindingTargetInput::RuntimeContext => 0,
                };
            }
        }
        for target in &mut targets {
            if *target == u32::MAX {
                *target = 0;
            }
        }
        Ok(targets.into_boxed_slice())
    }

    fn occurrence_targets(
        &self,
        snapshot: &KernelCheckedSnapshot,
        expression_declaration_targets: &[u32],
    ) -> Result<Box<[DeclId]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "occurrence topology")?;
        if expression_declaration_targets.len() != self.totals.expressions as usize {
            return Err(KernelCheckedLinkError::new(
                "occurrence topology lexical dependency count differs from expressions",
            ));
        }
        let mut targets = Vec::new();
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            let linked = self.definition(owner)?;
            let facts = definition.runtime_facts();

            for declaration in facts.declarations() {
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

            for ordinal in 0..linked.calls.len {
                let call = KernelCheckedPackedCallRef {
                    layout: self,
                    snapshot,
                    owner,
                    ordinal,
                };
                for entry in call.entries()? {
                    match entry {
                        crate::PackedCallEntry::FreshOut { output, .. } => targets.push(
                            self.declaration(owner, KernelDeclarationReference::Local(*output))?,
                        ),
                        crate::PackedCallEntry::ForwardOut { target, .. } => {
                            targets.push(self.declaration(owner, *target)?)
                        }
                        crate::PackedCallEntry::Input { .. } => {}
                    }
                }
                for context in call.contexts()? {
                    targets.push(self.declaration(
                        owner,
                        KernelDeclarationReference::Local(context.declaration),
                    )?);
                }
                let callable = call.callable()?;
                targets.push(callable);
                if matches!(
                    call.context_binding()?,
                    crate::PackedCallContextBinding::Explicit { .. }
                ) {
                    targets.push(callable);
                }
            }

            // Rich occurrence rows are ordered by dense expression ID, not by
            // parser discovery order. Reuse the same compact dependency column
            // that checked-image publication consumes.
            for row in checked_range(linked.expressions)? {
                let target = expression_declaration_targets[row];
                if target != 0 {
                    targets.push(DeclId(target));
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
            let facts = definition.runtime_facts();
            let range_start = u32::try_from(occurrences.len()).map_err(|_| {
                KernelCheckedLinkError::new("kernel checked occurrence count exceeds u32")
            })?;
            let mut declared = BTreeSet::new();

            // Authored declarations precede call-generated occurrences. Inline
            // record fields are lexical projection anchors, not authored-name
            // occurrences in the public checked index. Fresh OUT and
            // call-context declarations are emitted at their exact call
            // position below, matching their source authority.
            for declaration in facts.declarations() {
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
            for syntax in facts.calls() {
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
                            let mut arguments =
                                facts.call_arguments(syntax).iter().filter(|argument| {
                                    argument.kind == KernelCallArgumentKind::Named
                                        && definition.input().symbol(argument.name) == Some(name)
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
                                span: checked_span(argument.span.materialize()),
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
            let expected_declaration_occurrences = facts
                .declarations()
                .iter()
                .filter(|declaration| {
                    !matches!(
                        declaration.origin,
                        crate::KernelDeclarationOrigin::RecordField { .. }
                    )
                })
                .count();
            if declared.len() != expected_declaration_occurrences {
                let missing = facts
                    .declarations()
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

    /// Project retained callable-parameter identities through this exact
    /// checked layout.
    ///
    /// This is an explicit compatibility boundary. Runtime packed consumers
    /// keep using target-bound parameter tokens; editor and differential
    /// projections can request this small sidecar when they must compare rich
    /// checked programs whose absolute `TypeVar` ranges differ.
    pub fn callable_type_parameter_layouts(
        &self,
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<Box<[KernelCheckedCallableTypeParameterLayout]>, KernelCheckedLinkError> {
        let mut parameters = Vec::new();
        for definition in &self.definitions {
            let code = snapshot
                .definition_code
                .definition(definition.owner)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked linker has no packed callable scheme for definition {}",
                        definition.owner.0,
                    ))
                })?;
            for (ordinal, parameter) in code.callable_type_parameters().iter().enumerate() {
                parameters.push(KernelCheckedCallableTypeParameterLayout {
                    target: crate::KernelCallableSchemeId::User(definition.owner),
                    callable: definition.public_declaration,
                    parameter: crate::KernelTypeParameterId(u32::try_from(ordinal).map_err(
                        |_| {
                            KernelCheckedLinkError::new(
                                "kernel callable type-parameter ordinal exceeds u32",
                            )
                        },
                    )?),
                    linked_local: parameter.linked_local,
                    linked_variable: TypeVar(
                        definition
                            .type_variables
                            .resolve(parameter.linked_local, "callable type parameter")?,
                    ),
                });
            }
        }
        for callable in &self.abi_callables {
            let scheme = snapshot
                .definition_code
                .abi_callable_scheme(callable.callable)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked linker has no packed ABI callable scheme {}",
                        callable.callable.0,
                    ))
                })?;
            for (ordinal, parameter) in scheme.type_parameters().iter().enumerate() {
                parameters.push(KernelCheckedCallableTypeParameterLayout {
                    target: crate::KernelCallableSchemeId::Abi(callable.callable),
                    callable: callable.declaration,
                    parameter: crate::KernelTypeParameterId(u32::try_from(ordinal).map_err(
                        |_| {
                            KernelCheckedLinkError::new(
                                "kernel ABI type-parameter ordinal exceeds u32",
                            )
                        },
                    )?),
                    linked_local: parameter.linked_local,
                    linked_variable: TypeVar(
                        callable
                            .type_variables
                            .resolve(parameter.linked_local, "ABI callable type parameter")?,
                    ),
                });
            }
        }
        parameters.sort_unstable_by_key(|parameter| (parameter.target, parameter.parameter));
        for (index, parameter) in parameters.iter().enumerate() {
            if parameters[..index]
                .iter()
                .rev()
                .take_while(|previous| previous.target == parameter.target)
                .any(|previous| {
                    previous.parameter == parameter.parameter
                        || previous.linked_local == parameter.linked_local
                        || previous.linked_variable == parameter.linked_variable
                })
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked callable scheme {:?} repeats a parameter, linked alpha, or linked variable",
                    parameter.target,
                )));
            }
        }
        Ok(parameters.into_boxed_slice())
    }

    pub fn abi_callables(&self) -> &[KernelCheckedAbiCallableLayout] {
        &self.abi_callables
    }

    pub fn abi_callable(
        &self,
        callable: crate::KernelAbiCallableId,
    ) -> Result<&KernelCheckedAbiCallableLayout, KernelCheckedLinkError> {
        self.abi_callables
            .binary_search_by_key(&callable, |layout| layout.callable)
            .ok()
            .and_then(|index| self.abi_callables.get(index))
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked linker references unallocated ABI callable ID {}",
                    callable.0,
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

    /// Resolve a final checked expression coordinate back to the one packed
    /// definition/local row that owns it. Prefix ranges are sorted and
    /// disjoint, so this is a logarithmic borrowed view with no reverse map.
    fn local_expression(
        &self,
        expression: CheckedExprId,
    ) -> Result<(KernelOwnerId, crate::KernelExpressionId), KernelCheckedLinkError> {
        let owner = self
            .definitions
            .partition_point(|definition| definition.expressions.start <= expression.0)
            .checked_sub(1)
            .and_then(|owner| {
                self.definitions
                    .get(owner)
                    .map(|definition| (owner, definition))
            })
            .filter(|(_, definition)| {
                expression.0
                    < definition
                        .expressions
                        .start
                        .checked_add(definition.expressions.len)
                        .unwrap_or(u32::MAX)
            })
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked linker references missing expression {}",
                    expression.0,
                ))
            })?;
        Ok((
            KernelOwnerId(
                u32::try_from(owner.0).map_err(|_| {
                    KernelCheckedLinkError::new("kernel definition count exceeds u32")
                })?,
            ),
            crate::KernelExpressionId(
                expression
                    .0
                    .checked_sub(owner.1.expressions.start)
                    .expect("partitioned expression does not precede its owner range"),
            ),
        ))
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
        let mut remaining = (self.totals.scopes as usize)
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
                    let definition = snapshot.definition(owner).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel lexical declaration lookup references missing definition {}",
                            owner.0,
                        ))
                    })?;
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
                    let definition = snapshot.definition(owner).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel lexical declaration lookup references missing definition {}",
                            owner.0,
                        ))
                    })?;
                    let row = definition
                        .runtime_facts()
                        .scopes()
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
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            let layout = self.definition(owner)?;
            for scope in facts.scopes() {
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
                    span: checked_span(scope.span.materialize()),
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
        let mut type_cache = snapshot.definition_code.materialization_cache();
        self.materialize_declarations_with_cache(snapshot, &mut type_cache)
    }

    fn materialize_declarations_with_cache(
        &self,
        snapshot: &KernelCheckedSnapshot,
        type_cache: &mut DefinitionTypeMaterializationCache,
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
        for definition in snapshot.definition_refs() {
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            if facts.declarations().len() != facts.declaration_presentations().len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has {} declaration artifacts but {} declaration presentations",
                    owner.0,
                    facts.declarations().len(),
                    facts.declaration_presentations().len(),
                )));
            }
            for (declaration, presentation) in facts
                .declarations()
                .iter()
                .zip(facts.declaration_presentations().iter())
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
                    name: packed_symbol(definition, declaration.name, "declaration")?.to_owned(),
                    kind: checked_declaration_kind(declaration.kind),
                    flow_type: declaration_flow_type(
                        self,
                        snapshot,
                        owner,
                        declaration,
                        type_cache,
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
                    span: checked_span(presentation.span.materialize()),
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
            for source in definition.runtime_facts().sources() {
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
            let facts = definition.runtime_facts();
            for (ordinal, published) in definition.code().states().iter().enumerate() {
                let state = facts
                    .states()
                    .get(published.input_ordinal as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} published state {} references missing input {}",
                            owner.0, ordinal, published.input_ordinal,
                        ))
                    })?;
                let local_state = u32::try_from(ordinal).map_err(|_| {
                    KernelCheckedLinkError::new("kernel published state ordinal exceeds u32")
                })?;
                push_statement_resource(
                    &mut resources,
                    self.statement(owner, state.statement)?,
                    CheckedResourceBinding::State {
                        state: self.state(owner, local_state)?,
                    },
                )?;
            }
        }
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            for list in definition.runtime_facts().lists() {
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
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            if facts.statements().len() != facts.statement_presentations().len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} has {} statement artifacts but {} statement presentations",
                    owner.0,
                    facts.statements().len(),
                    facts.statement_presentations().len(),
                )));
            }
            for (statement, presentation) in facts
                .statements()
                .iter()
                .zip(facts.statement_presentations().iter())
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
                    kind: checked_statement_kind(definition, statement.kind, declaration)?,
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
                    children: facts
                        .statement_children(statement)
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
                    span: checked_span(presentation.span.materialize()),
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
        let mut type_cache = snapshot.definition_code.materialization_cache();
        self.materialize_expressions_with_cache(snapshot, &mut type_cache)
    }

    fn materialize_expressions_with_cache(
        &self,
        snapshot: &KernelCheckedSnapshot,
        type_cache: &mut DefinitionTypeMaterializationCache,
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
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            for (declaration, presentation) in facts
                .declarations()
                .iter()
                .zip(facts.declaration_presentations().iter())
            {
                let linked =
                    self.declaration(owner, KernelDeclarationReference::Local(declaration.id))?;
                if declaration_metadata
                    .insert(
                        linked,
                        (
                            owner,
                            presentation.scope,
                            packed_symbol(definition, declaration.name, "declaration metadata")?,
                        ),
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
            for source in definition.runtime_facts().sources() {
                let source_id = self.source(owner, source.id.0)?;
                let source_declaration = self.declaration(owner, source.declaration)?;
                let source_projection =
                    packed_path_strings(definition, source.projection, "SOURCE projection")?;
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
        for definition in snapshot.definition_refs() {
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer =
                code.linked_materializer(type_cache, self.definition(owner)?.type_variables.start);
            let local_len = definition.input().nodes().len();
            if code.expressions().len() != local_len
                || facts.expression_presentations().len() != local_len
                || facts.expression_payloads().len() != local_len
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} packed flows, expression artifacts, presentation, and payload tables differ: {} / {} / {} / {}",
                    owner.0,
                    code.expressions().len(),
                    local_len,
                    facts.expression_presentations().len(),
                    facts.expression_payloads().len(),
                )));
            }
            let mut shapes = vec![None; local_len];
            for shape in facts.execution_shapes() {
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
            for binding in facts.lexical_bindings() {
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
                .nodes()
                .iter()
                .zip(facts.expression_presentations().iter())
                .zip(facts.expression_payloads().iter())
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
                    presentation.span.line as usize,
                    declaration,
                    *payload,
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
                    span: checked_span(presentation.span.materialize()),
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
        let mut type_cache = snapshot.definition_code.materialization_cache();
        self.materialize_user_callables_with_cache(snapshot, role, &mut type_cache)
    }

    fn materialize_user_callables_with_cache(
        &self,
        snapshot: &KernelCheckedSnapshot,
        role: ProgramRole,
        type_cache: &mut DefinitionTypeMaterializationCache,
    ) -> Result<
        (Box<[CheckedCallableSignature]>, Box<[CheckedContextFormal]>),
        KernelCheckedLinkError,
    > {
        self.validate_snapshot_definition_count(snapshot, "user callable")?;
        let mut callables = Vec::with_capacity(self.totals.user_callables as usize);
        let mut context_formals = Vec::with_capacity(self.totals.context_formals as usize);
        for definition in snapshot.definition_refs() {
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer =
                code.linked_materializer(type_cache, self.definition(owner)?.type_variables.start);
            let root_statement = definition.linkage().root_statement.ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel definition {} has no root statement while linking callables",
                    owner.0,
                ))
            })?;
            let Some(root) = facts.statements().get(root_statement.0 as usize) else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} root statement {} is missing",
                    owner.0, root_statement.0,
                )));
            };
            let crate::PackedStatementKind::Function { name, .. } = root.kind else {
                if definition.linkage().context_formal_ordinal.is_some() {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel non-callable definition {} owns a context formal",
                        owner.0,
                    )));
                }
                continue;
            };
            let parameters = facts
                .statement_parameters(root)
                .expect("function statement owns parameter span");
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
            let public_declaration_row = facts
                .declarations()
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
                let mut declarations = facts.declarations().iter().filter(|declaration| {
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
                            .runtime_facts()
                            .declarations()
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
                    name: packed_symbol(definition, parameter.name, "callable parameter")?
                        .to_owned(),
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
                    start: presentation.span.start as usize,
                    end: presentation.span.end as usize,
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
            if public_declaration_row.name != name {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel callable definition {} declaration name {:?} differs from function header {:?}",
                    owner.0,
                    packed_symbol(
                        definition,
                        public_declaration_row.name,
                        "callable declaration"
                    )?,
                    packed_symbol(definition, name, "function header")?,
                )));
            }
            let mut effect = CheckedEffectSummary::default();
            for expression in 0..definition.input().nodes().len() {
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
                name: packed_symbol(definition, name, "function header")?.to_owned(),
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
        snapshot: &KernelCheckedSnapshot,
    ) -> Result<(Box<[CheckedCallableSignature]>, Box<[CheckedDeclaration]>), KernelCheckedLinkError>
    {
        let mut type_cache = snapshot.definition_code.materialization_cache();
        self.materialize_abi_callables_with_cache(
            snapshot,
            KernelCheckedRowProjectionDemand::EditorRich,
            &mut type_cache,
        )
    }

    fn materialize_abi_callables_with_cache(
        &self,
        snapshot: &KernelCheckedSnapshot,
        projection_demand: KernelCheckedRowProjectionDemand,
        type_cache: &mut DefinitionTypeMaterializationCache,
    ) -> Result<(Box<[CheckedCallableSignature]>, Box<[CheckedDeclaration]>), KernelCheckedLinkError>
    {
        let callable_capacity = match projection_demand {
            KernelCheckedRowProjectionDemand::RuntimePacked => 0,
            KernelCheckedRowProjectionDemand::EditorRich => self.totals.abi_callables as usize,
        };
        let mut callables = Vec::with_capacity(callable_capacity);
        let mut declarations = Vec::new();
        for layout in &self.abi_callables {
            let scheme = snapshot
                .definition_code
                .abi_callable_scheme(layout.callable)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked ABI materializer cannot find packed scheme ID {}",
                        layout.callable.0,
                    ))
                })?;
            let name = scheme.name();
            if scheme.parameters().len() != layout.parameters.len as usize {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked ABI callable `{}` has {} parameters in its layout and {} in its packed catalog",
                    name,
                    layout.parameters.len,
                    scheme.parameters().len(),
                )));
            }
            if scheme.formals().len() != scheme.parameters().len() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked ABI callable `{name}` has {} packed formals and {} parameter rows",
                    scheme.formals().len(),
                    scheme.parameters().len(),
                )));
            }
            if layout.type_variables.len != scheme.variable_count() {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel checked ABI callable `{name}` has {} linked variables and {} packed variables",
                    layout.type_variables.len,
                    scheme.variable_count(),
                )));
            }
            let alpha_end = layout
                .type_variables
                .start
                .checked_add(layout.type_variables.len)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel checked ABI callable `{name}` type-variable namespace overflows u32"
                    ))
                })?;
            let mut variables = BTreeMap::<TypeVar, TypeVar>::new();
            for local in 0..scheme.variable_count() {
                variables.insert(
                    TypeVar(
                        scheme
                            .variable_base()
                            .0
                            .checked_add(local)
                            .ok_or_else(|| {
                                KernelCheckedLinkError::new(format!(
                                    "kernel checked ABI callable `{name}` packed variable namespace overflows u32"
                                ))
                            })?,
                    ),
                    TypeVar(layout.type_variables.resolve(local, "ABI type variable")?),
                );
            }
            for parameter in scheme.type_parameters() {
                let linked = layout
                    .type_variables
                    .resolve(parameter.linked_local, "ABI type variable")?;
                if variables.get(&TypeVar(parameter.source.0)).copied() != Some(TypeVar(linked)) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel checked ABI callable `{name}` has inconsistent packed parameter {}",
                        parameter.source.0,
                    )));
                }
            }
            let mut next = alpha_end;
            let parameters = scheme
                .parameters()
                .iter()
                .zip((0..layout.parameters.len).map(|ordinal| {
                    DeclId(layout.parameters.start + ordinal)
                }))
                .zip(scheme.formals().iter().copied())
                .map(|((parameter, decl_id), packed_flow)| {
                    let evaluation_scope = match parameter.evaluation_scope {
                        crate::KernelParameterEvaluationScope::Parent => {
                            CheckedEvaluationScope::Parent
                        }
                        crate::KernelParameterEvaluationScope::Output { parameter_ordinal } => {
                            let formal = layout
                                .parameters
                                .resolve(parameter_ordinal, "ABI OUT parameter")
                                .map(DeclId)
                                .map_err(|_| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel checked ABI callable `{}` parameter `{}` targets missing OUT ordinal {parameter_ordinal}",
                                        name,
                                        scheme.symbol(parameter.name).unwrap_or("<foreign>"),
                                    ))
                                })?;
                            CheckedEvaluationScope::Output { formal }
                        }
                    };
                    let parameter_name = scheme.symbol(parameter.name).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel checked ABI callable `{name}` has a foreign parameter-name symbol {}",
                            parameter.name.as_u32(),
                        ))
                    })?;
                    let requirement = match parameter.requirement {
                        crate::PackedAbiParameterRequirement::Required => {
                            CheckedParameterRequirement::Required
                        }
                        crate::PackedAbiParameterRequirement::CallableProfile(symbol) => {
                            CheckedParameterRequirement::Optional {
                                default: CheckedParameterDefault::CallableProfile {
                                    profile: scheme
                                        .symbol(symbol)
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel checked ABI callable `{name}` has a foreign profile symbol {}",
                                                symbol.as_u32(),
                                            ))
                                        })?
                                        .to_owned(),
                                },
                            }
                        }
                        crate::PackedAbiParameterRequirement::Tag(symbol) => {
                            CheckedParameterRequirement::Optional {
                                default: CheckedParameterDefault::Tag {
                                    name: scheme
                                        .symbol(symbol)
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel checked ABI callable `{name}` has a foreign tag symbol {}",
                                                symbol.as_u32(),
                                            ))
                                        })?
                                        .to_owned(),
                                },
                            }
                        }
                        crate::PackedAbiParameterRequirement::ExactInteger(value) => {
                            CheckedParameterRequirement::Optional {
                                default: CheckedParameterDefault::ExactInteger { value },
                            }
                        }
                        crate::PackedAbiParameterRequirement::Text(span) => {
                            CheckedParameterRequirement::Optional {
                                default: CheckedParameterDefault::Text {
                                    value: scheme
                                        .default_text(span)
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel checked ABI callable `{name}` has an invalid default-text span"
                                            ))
                                        })?
                                        .to_owned(),
                                },
                            }
                        }
                    };
                    Ok(CheckedParameter {
                        decl_id,
                        name: parameter_name.to_owned(),
                        kind: parameter.kind,
                        ordinal: parameter.ordinal as usize,
                        flow_type: FlowType {
                            mode: packed_flow.mode,
                            ty: snapshot.definition_code.materialize_linked_type_term(
                                type_cache,
                                &mut variables,
                                &mut next,
                                alpha_end,
                                packed_flow.term,
                            ),
                        },
                        requirement,
                        evaluation_scope,
                        start: 0,
                        end: 0,
                    })
                })
                .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?;
            let contexts = if matches!(
                projection_demand,
                KernelCheckedRowProjectionDemand::EditorRich
            ) {
                scheme
                    .contexts()
                    .iter()
                    .map(|context| {
                    let provider = layout
                        .parameters
                        .resolve(
                            context.provider_parameter_ordinal,
                            "ABI context provider parameter",
                        )
                        .map(DeclId)
                        .map_err(|_| {
                            KernelCheckedLinkError::new(format!(
                                "kernel checked ABI callable `{}` context `{}` targets missing parameter ordinal {}",
                                name,
                                scheme.symbol(context.name).unwrap_or("<foreign>"),
                                context.provider_parameter_ordinal,
                            ))
                        })?;
                    let context_name = scheme.symbol(context.name).ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel checked ABI callable `{name}` has a foreign context-name symbol {}",
                            context.name.as_u32(),
                        ))
                    })?;
                    Ok(CheckedCallableContext {
                        name: context_name.to_owned(),
                        kind: context.kind,
                        provider,
                        flow_type: FlowType {
                            mode: context.flow.mode,
                            ty: snapshot.definition_code.materialize_linked_type_term(
                                type_cache,
                                &mut variables,
                                &mut next,
                                alpha_end,
                                context.flow.term,
                            ),
                        },
                    })
                    })
                    .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?
            } else {
                Vec::new()
            };
            let packed_result = scheme.result();
            let result = FlowType {
                mode: packed_result.mode,
                ty: snapshot.definition_code.materialize_linked_type_term(
                    type_cache,
                    &mut variables,
                    &mut next,
                    alpha_end,
                    packed_result.term,
                ),
            };
            if matches!(
                projection_demand,
                KernelCheckedRowProjectionDemand::EditorRich
            ) {
                callables.push(CheckedCallableSignature {
                    decl_id: layout.declaration,
                    scope_id: LexicalScopeId(0),
                    kind: match scheme.kind() {
                        crate::KernelCallableKind::Builtin => CheckedCallableKind::Builtin,
                        crate::KernelCallableKind::External => CheckedCallableKind::External,
                        crate::KernelCallableKind::User => {
                            return Err(KernelCheckedLinkError::new(format!(
                                "kernel immutable ABI unexpectedly contains user callable `{}`",
                                name,
                            )));
                        }
                    },
                    name: name.to_string(),
                    intrinsic: scheme.intrinsic(),
                    external_identity: scheme.external_identity(),
                    parameters: parameters.clone(),
                    contexts,
                    context_formal: None,
                    result: result.clone(),
                    role: scheme.role(),
                    effect: scheme.effect(),
                    body: None,
                    result_expression: None,
                    contextual_operation: scheme
                        .contextual_operation()
                        .map(|operation| checked_abi_contextual_operation(layout, name, operation))
                        .transpose()?,
                });
            }
            declarations.push(CheckedDeclaration {
                id: layout.declaration,
                scope_id: LexicalScopeId(0),
                name: name.to_string(),
                kind: match scheme.kind() {
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
        let expected_callables = match projection_demand {
            KernelCheckedRowProjectionDemand::RuntimePacked => 0,
            KernelCheckedRowProjectionDemand::EditorRich => self.totals.abi_callables,
        };
        self.validate_materialized_count("ABI callable", callables.len(), expected_callables)?;
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
        snapshot: &KernelCheckedSnapshot,
        callables: &[CheckedCallableSignature],
        declarations: &[CheckedDeclaration],
    ) -> Result<(Box<[CheckedCall]>, Box<[StableOccurrenceKey]>), KernelCheckedLinkError> {
        let mut type_cache = snapshot.definition_code.materialization_cache();
        self.materialize_calls_with_cache(snapshot, callables, declarations, &mut type_cache)
    }

    fn materialize_calls_with_cache(
        &self,
        snapshot: &KernelCheckedSnapshot,
        callables: &[CheckedCallableSignature],
        declarations: &[CheckedDeclaration],
        type_cache: &mut DefinitionTypeMaterializationCache,
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
        for definition in snapshot.definition_refs() {
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            let local = self.definition(owner)?;
            let code = definition.code();
            let (packed_calls, call_results) = {
                let mut materializer =
                    code.linked_materializer(type_cache, local.type_variables.start);
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
            for syntax in facts.calls() {
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
                .and_then(|root| facts.statements().get(root.0 as usize))
                .filter(|root| matches!(root.kind, crate::PackedStatementKind::Function { .. }))
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
                let syntax_function = packed_symbol(definition, syntax.function, "call function")?;
                let target_scheme = code.call_target(ordinal).flatten().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} call expression {} has no retained target scheme",
                        owner.0,
                        call.expression().0,
                    ))
                })?;
                let callable_id = match target_scheme {
                    crate::KernelCallableSchemeId::User(target) => {
                        self.definition(target)?.public_declaration
                    }
                    crate::KernelCallableSchemeId::Abi(callable) => {
                        self.abi_callable(callable)?.declaration
                    }
                };
                let target = callable_by_declaration.get(&callable_id).copied().ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {} call `{}` targets declaration {} without a materialized signature",
                        owner.0, syntax_function, callable_id.0,
                    ))
                })?;
                if target.name != syntax_function {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {} call syntax names `{}` but its target is `{}`",
                        owner.0, syntax_function, target.name,
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
                            owner.0, syntax_function, role,
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
                            facts.call_arguments(syntax).iter().find(|argument| {
                                definition.input().symbol(argument.name) == Some(&parameter.name)
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
                                owner.0, syntax_function, parameter.name,
                            ))
                        });
                    match parameter.kind {
                        CheckedParameterKind::Value => {
                            if let Ok(argument) = argument.as_ref()
                                && argument.kind != KernelCallArgumentKind::Named
                            {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel definition {} call `{}` binds value input `{}` as a bare OUT",
                                    owner.0, syntax_function, parameter.name,
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
                                    owner.0, syntax_function, parameter.name,
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
                                            owner.0, syntax_function, parameter.name,
                                        )));
                                    };
                                    let binding = definition
                                        .runtime_facts()
                                        .lexical_bindings()
                                        .iter()
                                        .find(|binding| {
                                            binding.expression == expression
                                                && definition
                                                    .input()
                                                    .path(binding.projection)
                                                    .is_some_and(|path| path.is_empty())
                                        })
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel definition {} call `{}` forwarded OUT `{}` has no exact lexical target",
                                                owner.0, syntax_function, parameter.name,
                                            ))
                                        })?;
                                    let crate::KernelLexicalBindingTargetInput::Declaration(
                                        target_reference,
                                    ) = binding.target
                                    else {
                                        return Err(KernelCheckedLinkError::new(format!(
                                            "kernel definition {} call `{}` forwarded OUT `{}` targets a non-declaration",
                                            owner.0, syntax_function, parameter.name,
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
                                                owner.0, syntax_function, target_declaration.0,
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
                    let declaration_name =
                        packed_symbol(definition, declaration.name, "call context declaration")?;
                    if declaration_name != context.name {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {} call `{}` context {} is named `{}` instead of `{}`",
                            owner.0,
                            syntax_function,
                            context_ordinal,
                            declaration_name,
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
                        span: checked_span(pass.span.materialize()),
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
                            owner.0, syntax_function, inherited.caller_ordinal,
                        )));
                    }
                    CheckedContextBinding::Inherited {
                        formal: local.context_formal.ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} call `{}` inherits a missing context formal",
                                owner.0, syntax_function,
                            ))
                        })?,
                    }
                } else {
                    CheckedContextBinding::None
                };

                let (raw_substitutions, target_parameters, target_type_variables, context) =
                    match call.target() {
                        KernelCallTargetRef::User { target, .. } => {
                            if target_scheme != crate::KernelCallableSchemeId::User(target) {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel call `{}` retained a target scheme that differs from user owner {}",
                                    syntax_function, target.0,
                                )));
                            }
                            let target_definition =
                                snapshot.definition(target).ok_or_else(|| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel call `{}` references missing target definition {}",
                                        syntax_function, target.0,
                                    ))
                                })?;
                            let target_code =
                                snapshot.definition_code.definition(target).ok_or_else(|| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel call `{}` has no packed target definition {}",
                                        syntax_function, target.0,
                                    ))
                                })?;
                            let target_layout = self.definition(target)?;
                            let context = target_definition
                                .linkage()
                                .context_formal_ordinal
                                .map(|ordinal| {
                                    let mut target_materializer = target_code.linked_materializer(
                                        type_cache,
                                        target_layout.type_variables.start,
                                    );
                                    let flow = target_materializer
                                        .materialize_formal(ordinal as usize)
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel callable `{}` context ordinal {ordinal} is missing",
                                                syntax_function,
                                            ))
                                        })?;
                                    let formal = self
                                        .definition(target)?
                                        .context_formal
                                        .ok_or_else(|| {
                                            KernelCheckedLinkError::new(format!(
                                                "kernel callable `{}` context has no checked formal",
                                                syntax_function,
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
                                target_code.callable_type_parameters(),
                                target_layout.type_variables,
                                context,
                            )
                        }
                        KernelCallTargetRef::RenderConstructor { .. }
                        | KernelCallTargetRef::PureBuiltin { .. }
                        | KernelCallTargetRef::FixedAbi
                        | KernelCallTargetRef::HostEffect { .. }
                        | KernelCallTargetRef::FieldProjection { .. } => {
                            let crate::KernelCallableSchemeId::Abi(callable) = target_scheme else {
                                return Err(KernelCheckedLinkError::new(format!(
                                    "kernel ABI call `{}` retained a user target scheme",
                                    syntax_function,
                                )));
                            };
                            let target_layout = self.abi_callable(callable)?;
                            let scheme = snapshot
                                .definition_code
                                .abi_callable_scheme(target_layout.callable)
                                .ok_or_else(|| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel call `{}` has no retained packed ABI scheme {}",
                                        syntax_function, target_layout.callable.0,
                                    ))
                                })?;
                            (
                                packed_call.substitutions.into_vec(),
                                scheme.type_parameters(),
                                target_layout.type_variables,
                                None,
                            )
                        }
                    };
                let mut type_substitutions = Vec::with_capacity(raw_substitutions.len());
                let mut contextual_substitutions = Vec::new();
                for substitution in raw_substitutions {
                    let parameter = target_parameters
                        .get(substitution.variable.0 as usize)
                        .copied()
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel call `{}` substitution parameter {} is outside its target scheme",
                                syntax_function, substitution.variable.0,
                            ))
                        })?;
                    let variable = TypeVar(
                        target_type_variables
                            .resolve(parameter.linked_local, "call target type variable")?,
                    );
                    let value = substitution.value;
                    if let Some((formal, context_variables)) = &context
                        && context_variables.contains(&variable)
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
                    function: syntax_function.to_owned(),
                    intrinsic: target.intrinsic,
                    entries,
                    contexts,
                    context_binding,
                    contextual_substitutions,
                    type_substitutions,
                    syntax_discriminated_result,
                    result,
                    role: target.role,
                    span: checked_span(presentation.span.materialize()),
                });
                call_occurrences.push(facts.call_occurrence(syntax).clone());
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
        let mut type_cache = snapshot.definition_code.materialization_cache();
        self.materialize_sources_with_cache(snapshot, &mut type_cache)
    }

    fn materialize_sources_with_cache(
        &self,
        snapshot: &KernelCheckedSnapshot,
        type_cache: &mut DefinitionTypeMaterializationCache,
    ) -> Result<Box<[CheckedSource]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "SOURCE")?;
        let mut sources = Vec::with_capacity(self.totals.sources as usize);
        for definition in snapshot.definition_refs() {
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer =
                code.linked_materializer(type_cache, self.definition(owner)?.type_variables.start);
            for (ordinal, source) in facts.sources().iter().enumerate() {
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
                    path: self.semantic_packed_path_parts(
                        definition,
                        source.declaration,
                        source.projection,
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
                    span: checked_span(presentation.span.materialize()),
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
        let mut type_cache = snapshot.definition_code.materialization_cache();
        self.materialize_states_with_cache(snapshot, &mut type_cache)
    }

    fn materialize_states_with_cache(
        &self,
        snapshot: &KernelCheckedSnapshot,
        type_cache: &mut DefinitionTypeMaterializationCache,
    ) -> Result<Box<[CheckedState]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "state")?;
        let mut states = Vec::with_capacity(self.totals.states as usize);
        for definition in snapshot.definition_refs() {
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer =
                code.linked_materializer(type_cache, self.definition(owner)?.type_variables.start);
            for (ordinal, state) in code.states().iter().copied().enumerate() {
                let input = facts
                    .states()
                    .get(state.input_ordinal as usize)
                    .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {} published state {} references missing input {}",
                            owner.0, ordinal, state.input_ordinal,
                        ))
                    })?;
                let local_id = u32::try_from(ordinal).map_err(|_| {
                    KernelCheckedLinkError::new("kernel published state ordinal exceeds u32")
                })?;
                let id = self.state(owner, local_id)?;
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
                            snapshot
                                .definition(statement_owner)
                                .ok_or_else(|| {
                                    KernelCheckedLinkError::new(format!(
                                        "kernel state references missing definition {}",
                                        statement_owner.0,
                                    ))
                                })?
                                .runtime_facts(),
                            statement,
                        )?;
                        (
                            self.scope(statement_owner, presentation.scope)?,
                            checked_span(presentation.span.materialize()),
                        )
                    } else {
                        let presentation = expression_presentation(facts, input.expression)?;
                        (
                            self.scope(owner, presentation.scope)?,
                            checked_span(presentation.span.materialize()),
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
                        definition
                            .resolve_value(input.initial, input.expression.0 as usize)
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?,
                    )?,
                    owner_scope,
                    path: match state.synthetic_ordinal() {
                        None => self.semantic_packed_path_parts(
                            definition,
                            input.declaration,
                            input.projection,
                        )?,
                        Some(ordinal) => CheckedSemanticPath {
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
        let mut type_cache = snapshot.definition_code.materialization_cache();
        self.materialize_lists_with_cache(snapshot, &mut type_cache)
    }

    fn materialize_lists_with_cache(
        &self,
        snapshot: &KernelCheckedSnapshot,
        type_cache: &mut DefinitionTypeMaterializationCache,
    ) -> Result<Box<[CheckedList]>, KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "LIST")?;
        let mut lists = Vec::with_capacity(self.totals.lists as usize);
        for definition in snapshot.definition_refs() {
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            let code = definition.code();
            let mut materializer =
                code.linked_materializer(type_cache, self.definition(owner)?.type_variables.start);
            for (ordinal, list) in facts.lists().iter().enumerate() {
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
                    path: self.semantic_packed_path_parts(
                        definition,
                        list.declaration,
                        list.projection,
                    )?,
                    item_type: materializer
                        .materialize_list_item_type(ordinal)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {} has no packed LIST item {}",
                                owner.0, ordinal
                            ))
                        })?,
                    capacity: list.capacity.map(|capacity| capacity as usize),
                    key_policy: list.key_policy,
                    span: checked_span(presentation.span.materialize()),
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

    fn semantic_packed_path_parts(
        &self,
        definition: KernelDefinitionRef<'_>,
        anchor: KernelDeclarationReference,
        projection: PathId,
    ) -> Result<CheckedSemanticPath, KernelCheckedLinkError> {
        Ok(CheckedSemanticPath {
            anchor: self.declaration(definition.owner(), anchor)?,
            projection: packed_path_strings(definition, projection, "semantic path")?,
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

    fn packed_call<'a>(
        &'a self,
        snapshot: &'a KernelCheckedSnapshot,
        id: CheckedCallId,
    ) -> Result<KernelCheckedPackedCallRef<'a>, KernelCheckedLinkError> {
        let index = self
            .definitions
            .partition_point(|definition| definition.calls.start <= id.0)
            .checked_sub(1)
            .and_then(|index| self.definitions.get(index))
            .filter(|definition| {
                id.0 < definition
                    .calls
                    .start
                    .checked_add(definition.calls.len)
                    .unwrap_or(u32::MAX)
            })
            .ok_or_else(|| {
                KernelCheckedLinkError::new(format!(
                    "kernel checked linker references missing packed call {}",
                    id.0,
                ))
            })?;
        let ordinal = id.0.checked_sub(index.calls.start).ok_or_else(|| {
            KernelCheckedLinkError::new("kernel checked packed call precedes its definition range")
        })?;
        let call = KernelCheckedPackedCallRef {
            layout: self,
            snapshot,
            owner: index.owner,
            ordinal,
        };
        if call.id()? != id {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel checked packed call {} does not round-trip through its relocation",
                id.0,
            )));
        }
        Ok(call)
    }

    fn for_each_packed_call(
        &self,
        snapshot: &KernelCheckedSnapshot,
        mut visit: impl FnMut(KernelCheckedPackedCallRef<'_>) -> Result<(), KernelCheckedLinkError>,
    ) -> Result<(), KernelCheckedLinkError> {
        self.validate_snapshot_definition_count(snapshot, "packed call topology")?;
        let mut expected = 0u32;
        for definition in snapshot.definition_refs() {
            let owner = definition.owner();
            let linked = self.definition(owner)?;
            if linked.calls.start != expected
                || linked.calls.len as usize != definition.call_count()
            {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} packed call range {}..{} differs from its {} retained calls",
                    owner.0,
                    linked.calls.start,
                    linked.calls.start.saturating_add(linked.calls.len),
                    definition.call_count(),
                )));
            }
            for ordinal in 0..linked.calls.len {
                let call = KernelCheckedPackedCallRef {
                    layout: self,
                    snapshot,
                    owner,
                    ordinal,
                };
                if call.id()?.0 != expected {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel packed call topology expected dense row {expected} but linked {}",
                        call.id()?.0,
                    )));
                }
                visit(call)?;
                expected = expected.checked_add(1).ok_or_else(|| {
                    KernelCheckedLinkError::new("kernel packed call count exceeds u32")
                })?;
            }
        }
        if expected != self.totals.calls {
            return Err(KernelCheckedLinkError::new(format!(
                "kernel packed call topology linked {expected} of {} calls",
                self.totals.calls,
            )));
        }
        Ok(())
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
            let facts = definition.runtime_facts();
            let owner = definition.owner();
            let local = self.definition(owner)?.clone();
            let _ = self.scope(owner, facts.containing_scope())?;
            for scope in facts.scopes() {
                let _ = local.scopes.resolve(scope.id.0, "scope presentation")?;
                let _ = self.scope(owner, scope.parent)?;
                if let Some(declaration) = scope.owner {
                    let _ = self.declaration(owner, declaration)?;
                    resolved = resolved.saturating_add(1);
                }
                resolved = resolved.saturating_add(1);
            }
            for expression in facts.expression_presentations() {
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
            for statement in facts.statement_presentations() {
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
            for declaration in facts.declaration_presentations() {
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
            for (expression_index, expression) in definition.input().nodes().iter().enumerate() {
                let expression_id = crate::KernelExpressionId(
                    u32::try_from(expression_index)
                        .expect("kernel expression count exceeds dense u32 namespace"),
                );
                let _ = local
                    .expressions
                    .resolve(expression_id.0, "expression artifact")?;
                for input in expression.inputs(definition.input()) {
                    let value = definition
                        .resolve_value(input.expression, expression_index)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                    let _ = self.expression(owner, value)?;
                    resolved = resolved.saturating_add(1);
                }
            }
            for statement in facts.statements() {
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
                for child in facts.statement_children(statement) {
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
            for declaration in facts.declarations() {
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
            for binding in facts.lexical_bindings() {
                let _ = local
                    .expressions
                    .resolve(binding.expression.0, "lexical occurrence")?;
                match binding.target {
                    crate::KernelLexicalBindingTargetInput::Declaration(declaration) => {
                        let _ = self.declaration(owner, declaration)?;
                    }
                    crate::KernelLexicalBindingTargetInput::ContextFormal { ordinal } => {
                        if definition.linkage().context_formal_ordinal != Some(ordinal)
                            || local.context_formal.is_none()
                        {
                            return Err(KernelCheckedLinkError::new(format!(
                                "kernel definition {} lexical context ordinal {ordinal} has no exact context-formal anchor",
                                owner.0,
                            )));
                        }
                    }
                    crate::KernelLexicalBindingTargetInput::Value { provider } => {
                        let provider = definition
                            .resolve_value(provider, binding.expression.0 as usize)
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                        let _ = self.expression(owner, provider)?;
                    }
                    crate::KernelLexicalBindingTargetInput::RuntimeContext => {}
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
                    crate::PackedKernelOwnerNodeKind::UserCall { .. }
                        | crate::PackedKernelOwnerNodeKind::RenderConstructor { .. }
                        | crate::PackedKernelOwnerNodeKind::PureBuiltin { .. }
                        | crate::PackedKernelOwnerNodeKind::FixedAbiCall { .. }
                        | crate::PackedKernelOwnerNodeKind::HostEffect { .. }
                        | crate::PackedKernelOwnerNodeKind::FieldProjection { .. }
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
            for call in facts.calls() {
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
                for argument in facts.call_arguments(call) {
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
            for shape in facts.execution_shapes() {
                let _ = local
                    .expressions
                    .resolve(shape.expression().0, "execution-shape expression")?;
                match shape {
                    crate::PackedExecutionShape::Conditional { .. } => {}
                    crate::PackedExecutionShape::Record { .. } => {
                        for field in facts
                            .execution_fields(shape)
                            .expect("record execution shape owns fields")
                        {
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
                    crate::PackedExecutionShape::Block { result, .. } => {
                        for binding in facts
                            .execution_bindings(shape)
                            .expect("BLOCK execution shape owns bindings")
                        {
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
                    crate::PackedExecutionShape::MatchArm { selector, .. } => {
                        let value = definition
                            .resolve_value(*selector, shape.expression().0 as usize)
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                        let _ = self.expression(owner, value)?;
                        resolved = resolved.saturating_add(1);
                        for binding in facts
                            .execution_match_bindings(shape)
                            .expect("match-arm execution shape owns bindings")
                        {
                            let _ = local
                                .declarations
                                .resolve(binding.0, "match binding declaration")?;
                            resolved = resolved.saturating_add(1);
                        }
                    }
                }
            }
            for source in facts.sources() {
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
                    .states()
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
            for list in facts.lists() {
                let _ = self.list(owner, list.id.0)?;
                let _ = self.declaration(owner, list.declaration)?;
                let _ = self.statement(owner, list.statement)?;
                let _ = local
                    .expressions
                    .resolve(list.producer.0, "list producer")?;
                let _ = self.declaration(owner, list.declaration)?;
                resolved = resolved.saturating_add(4);
            }
            for (expression, node) in definition.input().nodes().iter().enumerate() {
                if matches!(
                    node.kind,
                    crate::PackedKernelOwnerNodeKind::HostEffect { .. }
                ) {
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

fn expression_presentation<'a>(
    facts: crate::PackedDefinitionFactsRef<'a>,
    expression: crate::KernelExpressionId,
) -> Result<&'a crate::PackedExpressionPresentation, KernelCheckedLinkError> {
    facts
        .expression_presentations()
        .get(expression.0 as usize)
        .filter(|presentation| presentation.expression == expression)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel resource expression {} has no exact presentation row",
                expression.0,
            ))
        })
}

fn statement_presentation<'a>(
    facts: crate::PackedDefinitionFactsRef<'a>,
    statement: crate::KernelStatementId,
) -> Result<&'a crate::PackedStatementPresentation, KernelCheckedLinkError> {
    facts
        .statement_presentations()
        .get(statement.0 as usize)
        .filter(|presentation| presentation.statement == statement)
        .ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "kernel resource statement {} has no exact presentation row",
                statement.0,
            ))
        })
}

fn declaration_presentation<'a>(
    facts: crate::PackedDefinitionFactsRef<'a>,
    declaration: crate::KernelDeclarationId,
) -> Result<&'a crate::PackedDeclarationPresentation, KernelCheckedLinkError> {
    facts
        .declaration_presentations()
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
) -> Result<&'a crate::PackedDeclaration, KernelCheckedLinkError> {
    let mut declarations = definition
        .runtime_facts()
        .declarations()
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

fn packed_symbol<'a>(
    definition: KernelDefinitionRef<'a>,
    symbol: SymbolId,
    label: &str,
) -> Result<&'a str, KernelCheckedLinkError> {
    definition.input().symbol(symbol).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition {} {label} references foreign symbol {symbol:?}",
            definition.owner().0,
        ))
    })
}

fn packed_path_strings(
    definition: KernelDefinitionRef<'_>,
    path: PathId,
    label: &str,
) -> Result<Vec<String>, KernelCheckedLinkError> {
    let path = definition.input().path(path).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel definition {} {label} references foreign path {path:?}",
            definition.owner().0,
        ))
    })?;
    path.iter()
        .map(|symbol| packed_symbol(definition, symbol, label).map(str::to_owned))
        .collect()
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
    statement: &crate::PackedStatement,
) -> Result<Option<KernelDeclarationReference>, KernelCheckedLinkError> {
    let mut declarations =
        definition
            .runtime_facts()
            .declarations()
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
        statement.kind,
        crate::PackedStatementKind::Function { .. }
            | crate::PackedStatementKind::Field { .. }
            | crate::PackedStatementKind::Source { field: Some(_), .. }
            | crate::PackedStatementKind::Hold { field: Some(_), .. }
            | crate::PackedStatementKind::List { field: Some(_), .. }
    );
    Ok(declaration.or_else(|| {
        (owns_authored_declaration && definition.linkage().root_statement == Some(statement.id))
            .then_some(definition.linkage().public_declaration)
            .flatten()
    }))
}

fn checked_statement_kind(
    definition: KernelDefinitionRef<'_>,
    kind: crate::PackedStatementKind,
    declaration: Option<DeclId>,
) -> Result<CheckedStatementKind, KernelCheckedLinkError> {
    Ok(match kind {
        crate::PackedStatementKind::Function { .. } => CheckedStatementKind::Function {
            declaration: declaration.ok_or_else(|| {
                KernelCheckedLinkError::new("kernel function statement has no declaration")
            })?,
        },
        crate::PackedStatementKind::Field { .. } => CheckedStatementKind::Field {
            declaration: declaration.ok_or_else(|| {
                KernelCheckedLinkError::new("kernel field statement has no declaration")
            })?,
        },
        crate::PackedStatementKind::Source { event, .. } => CheckedStatementKind::Source {
            declaration,
            event: event
                .map(|event| packed_symbol(definition, event, "SOURCE event").map(str::to_owned))
                .transpose()?,
        },
        crate::PackedStatementKind::Hold { name, .. } => CheckedStatementKind::Hold {
            declaration,
            name: name
                .map(|name| packed_symbol(definition, name, "HOLD name").map(str::to_owned))
                .transpose()?,
        },
        crate::PackedStatementKind::List { capacity, .. } => CheckedStatementKind::List {
            declaration,
            capacity: capacity.map(|capacity| capacity as usize),
        },
        crate::PackedStatementKind::Block => CheckedStatementKind::Block,
        crate::PackedStatementKind::Spread => CheckedStatementKind::Spread,
        crate::PackedStatementKind::Expression => CheckedStatementKind::Expression,
    })
}

#[allow(clippy::too_many_arguments)]
fn checked_expression_kind(
    layout: &KernelCheckedLinkLayout,
    owner: KernelOwnerId,
    definition: KernelDefinitionRef<'_>,
    facts: crate::PackedDefinitionFactsRef<'_>,
    expression_id: crate::KernelExpressionId,
    expression: &crate::PackedKernelOwnerNode,
    container_line: usize,
    container_declaration: Option<DeclId>,
    payload: crate::PackedExpressionPayload,
    shape: Option<&crate::PackedExecutionShape>,
    lexical: Option<&crate::PackedLexicalBinding>,
    call: Option<u32>,
    source_paths: &BTreeMap<DeclId, Vec<(Vec<String>, CheckedSourceId)>>,
) -> Result<CheckedExpressionKind, KernelCheckedLinkError> {
    if let Some(call) = call {
        return Ok(CheckedExpressionKind::Call {
            call: layout.call(owner, call)?,
        });
    }
    if let Some(binding) = lexical {
        let projection = packed_path_strings(definition, binding.projection, "lexical projection")?;
        let target = match binding.target {
            crate::KernelLexicalBindingTargetInput::Declaration(target) => {
                KernelLexicalBindingTargetRef::Declaration(target)
            }
            crate::KernelLexicalBindingTargetInput::ContextFormal { ordinal } => {
                KernelLexicalBindingTargetRef::ContextFormal { ordinal }
            }
            crate::KernelLexicalBindingTargetInput::Value { provider } => {
                KernelLexicalBindingTargetRef::Value {
                    provider: definition
                        .resolve_value(provider, binding.expression.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?,
                }
            }
            crate::KernelLexicalBindingTargetInput::RuntimeContext => {
                KernelLexicalBindingTargetRef::RuntimeContext
            }
        };
        return match target {
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
                    canonical_path: lexical_payload_path(definition, payload)?.ok_or_else(|| {
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
    if matches!(payload, crate::PackedExpressionPayload::Delimiter) {
        return Ok(CheckedExpressionKind::Delimiter);
    }
    if matches!(payload, crate::PackedExpressionPayload::Invalid(_))
        && matches!(
            expression.kind,
            crate::PackedKernelOwnerNodeKind::Number
                | crate::PackedKernelOwnerNodeKind::Byte
                | crate::PackedKernelOwnerNodeKind::Bits(_)
        )
    {
        return Ok(CheckedExpressionKind::Invalid {
            tokens: facts
                .invalid_tokens(payload)
                .expect("invalid payload owns token spans")
                .map(str::to_owned)
                .collect(),
        });
    }

    let inputs = |role: &crate::PackedKernelOwnerEdgeRole| {
        expression
            .inputs(definition.input())
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
    let one_input = |role: &crate::PackedKernelOwnerEdgeRole, label: &str| {
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
        crate::PackedKernelOwnerNodeKind::Source(_) => CheckedExpressionKind::Source,
        crate::PackedKernelOwnerNodeKind::Absent => CheckedExpressionKind::Absent,
        crate::PackedKernelOwnerNodeKind::Text => CheckedExpressionKind::Text {
            value: match payload {
                crate::PackedExpressionPayload::Text(value) => facts.literal_text(value).to_owned(),
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression_id,
                        "text literal",
                    ));
                }
            },
        },
        crate::PackedKernelOwnerNodeKind::TextTemplate => {
            let dynamic = inputs(&crate::PackedKernelOwnerEdgeRole::TextDynamic)?;
            let crate::PackedExpressionPayload::TextTemplate(_) = payload else {
                return Err(expression_payload_error(
                    owner,
                    expression_id,
                    "text template",
                ));
            };
            let segments = facts
                .template_segments(payload)
                .expect("text-template payload owns segment span");
            CheckedExpressionKind::TextTemplate {
                segments: segments
                    .iter()
                    .map(|segment| match segment {
                        crate::PackedTextTemplateSegment::Static(value) => {
                            Ok(CheckedTextSegment::Static {
                                value: facts.literal_text(*value).to_owned(),
                            })
                        }
                        crate::PackedTextTemplateSegment::Dynamic(ordinal) => dynamic
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
        crate::PackedKernelOwnerNodeKind::Number => CheckedExpressionKind::Number {
            value: match payload {
                crate::PackedExpressionPayload::Number(value) => facts.number(value).clone(),
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression_id,
                        "number literal",
                    ));
                }
            },
        },
        crate::PackedKernelOwnerNodeKind::Byte => CheckedExpressionKind::BytesByte {
            value: match payload {
                crate::PackedExpressionPayload::Byte(value) => value,
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression_id,
                        "byte literal",
                    ));
                }
            },
        },
        crate::PackedKernelOwnerNodeKind::Bits(_) => CheckedExpressionKind::Bits {
            value: match payload {
                crate::PackedExpressionPayload::Bits(value) => facts.bits(value).clone(),
                _ => {
                    return Err(expression_payload_error(
                        owner,
                        expression_id,
                        "bits literal",
                    ));
                }
            },
        },
        crate::PackedKernelOwnerNodeKind::Tag(name) => CheckedExpressionKind::Tag {
            name: definition
                .input()
                .symbol(*name)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(
                        "packed tag references text outside its project authority",
                    )
                })?
                .to_owned(),
        },
        crate::PackedKernelOwnerNodeKind::Record { tag } => {
            let Some(shape @ crate::PackedExecutionShape::Record { .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} record expression {} has no exact execution shape",
                    owner.0, expression_id.0,
                )));
            };
            let fields = facts
                .execution_fields(shape)
                .expect("record execution shape owns fields");
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
                    tag: definition
                        .input()
                        .symbol(*tag)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(
                                "packed record tag references text outside its project authority",
                            )
                        })?
                        .to_owned(),
                    fields,
                },
                None => CheckedExpressionKind::Object { fields },
            }
        }
        crate::PackedKernelOwnerNodeKind::Block => {
            let Some(shape @ crate::PackedExecutionShape::Block { result, .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} BLOCK expression {} has no exact execution shape",
                    owner.0, expression_id.0,
                )));
            };
            let bindings = facts
                .execution_bindings(shape)
                .expect("BLOCK execution shape owns bindings");
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
                            span: checked_nested_span(container_line, binding.span.materialize()),
                        })
                    })
                    .collect::<Result<Vec<_>, KernelCheckedLinkError>>()?,
                result: (*result)
                    .map(|result| {
                        definition
                            .resolve_value(result, expression_id.0 as usize)
                            .map_err(|error| KernelCheckedLinkError::new(error.to_string()))
                            .and_then(|value| layout.expression(owner, value))
                    })
                    .transpose()?,
            }
        }
        crate::PackedKernelOwnerNodeKind::Collection { kind, capacity } => match kind {
            crate::KernelCollectionKind::List => CheckedExpressionKind::List {
                capacity: capacity.map(|capacity| capacity as usize),
                items: inputs(&crate::PackedKernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Bytes => CheckedExpressionKind::Bytes {
                fixed_size: capacity.map(|capacity| capacity as usize),
                items: inputs(&crate::PackedKernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Set => CheckedExpressionKind::Set {
                items: inputs(&crate::PackedKernelOwnerEdgeRole::CollectionItem)?,
            },
            crate::KernelCollectionKind::Map => CheckedExpressionKind::Map {
                entries: inputs(&crate::PackedKernelOwnerEdgeRole::MapEntry)?,
            },
        },
        crate::PackedKernelOwnerNodeKind::MapEntry => CheckedExpressionKind::MapEntry {
            key: one_input(&crate::PackedKernelOwnerEdgeRole::MapKey, "map-key")?,
            value: one_input(&crate::PackedKernelOwnerEdgeRole::MapValue, "map-value")?,
        },
        crate::PackedKernelOwnerNodeKind::Latest => CheckedExpressionKind::Latest {
            branches: inputs(&crate::PackedKernelOwnerEdgeRole::LatestBranch)?,
        },
        crate::PackedKernelOwnerNodeKind::When => {
            let Some(crate::PackedExecutionShape::Conditional { kind, .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} conditional expression {} has no exact execution shape",
                    owner.0, expression_id.0,
                )));
            };
            let input = one_input(
                &crate::PackedKernelOwnerEdgeRole::WhenInput,
                "conditional selector",
            )?;
            let arms = inputs(&crate::PackedKernelOwnerEdgeRole::WhenArm)?;
            match *kind {
                crate::KernelConditionalKind::When => CheckedExpressionKind::When { input, arms },
                crate::KernelConditionalKind::While => CheckedExpressionKind::While { input, arms },
            }
        }
        crate::PackedKernelOwnerNodeKind::Then => CheckedExpressionKind::Then {
            input: one_input(&crate::PackedKernelOwnerEdgeRole::ThenInput, "THEN input")?,
            output: inputs(&crate::PackedKernelOwnerEdgeRole::ThenOutput)?
                .into_iter()
                .next(),
        },
        crate::PackedKernelOwnerNodeKind::Infix { operation } => CheckedExpressionKind::Infix {
            left: one_input(&crate::PackedKernelOwnerEdgeRole::InfixLeft, "infix-left")?,
            op: definition
                .input()
                .symbol(*operation)
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(
                        "packed infix operation references text outside its project authority",
                    )
                })?
                .to_owned(),
            right: one_input(&crate::PackedKernelOwnerEdgeRole::InfixRight, "infix-right")?,
        },
        crate::PackedKernelOwnerNodeKind::Draining => CheckedExpressionKind::Draining {
            input: one_input(&crate::PackedKernelOwnerEdgeRole::DrainingInput, "DRAINING")?,
        },
        crate::PackedKernelOwnerNodeKind::Hold => CheckedExpressionKind::Hold {
            initial: one_input(
                &crate::PackedKernelOwnerEdgeRole::HoldInitial,
                "HOLD initial",
            )?,
            name: match payload {
                crate::PackedExpressionPayload::HoldName(name) => {
                    packed_symbol(definition, name, "HOLD payload")?.to_owned()
                }
                _ => return Err(expression_payload_error(owner, expression_id, "HOLD name")),
            },
        },
        crate::PackedKernelOwnerNodeKind::MatchArm { .. } => {
            let Some(shape @ crate::PackedExecutionShape::MatchArm { .. }) = shape else {
                return Err(KernelCheckedLinkError::new(format!(
                    "kernel definition {} match arm {} has no exact execution shape",
                    owner.0, expression_id.0,
                )));
            };
            let bindings = facts
                .execution_match_bindings(shape)
                .expect("match-arm execution shape owns bindings");
            CheckedExpressionKind::MatchArm {
                pattern: checked_match_pattern(definition, facts, payload)?.ok_or_else(|| {
                    expression_payload_error(owner, expression_id, "match pattern")
                })?,
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        layout.declaration(owner, KernelDeclarationReference::Local(*binding))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                output: inputs(&crate::PackedKernelOwnerEdgeRole::MatchOutput)?
                    .into_iter()
                    .next(),
            }
        }
        crate::PackedKernelOwnerNodeKind::Arrow => CheckedExpressionKind::Invalid {
            tokens: vec!["unconsumed_arrow".to_owned()],
        },
        crate::PackedKernelOwnerNodeKind::Flush => CheckedExpressionKind::Flush {
            payload: one_input(
                &crate::PackedKernelOwnerEdgeRole::FlushPayload,
                "FLUSH payload",
            )?,
        },
        crate::PackedKernelOwnerNodeKind::Delimiter => CheckedExpressionKind::Delimiter,
        crate::PackedKernelOwnerNodeKind::Unknown => CheckedExpressionKind::Invalid {
            tokens: match payload {
                crate::PackedExpressionPayload::Invalid(_) => facts
                    .invalid_tokens(payload)
                    .expect("invalid payload owns token spans")
                    .map(str::to_owned)
                    .collect(),
                crate::PackedExpressionPayload::LexicalPath(path) => vec![
                    "unresolved_value".to_owned(),
                    packed_path_strings(definition, path, "unresolved lexical path")?.join("/"),
                ],
                _ => vec!["unknown_expression".to_owned()],
            },
        },
        crate::PackedKernelOwnerNodeKind::Known(_)
        | crate::PackedKernelOwnerNodeKind::FormalRead { .. }
        | crate::PackedKernelOwnerNodeKind::ContextRead { .. }
        | crate::PackedKernelOwnerNodeKind::LexicalRead { .. }
        | crate::PackedKernelOwnerNodeKind::ValueRead { .. }
        | crate::PackedKernelOwnerNodeKind::DerivedRead { .. }
        | crate::PackedKernelOwnerNodeKind::PatternRead { .. }
        | crate::PackedKernelOwnerNodeKind::CollectionItemRead
        | crate::PackedKernelOwnerNodeKind::FreshOut => {
            let span = facts
                .expression_presentations()
                .get(expression_id.0 as usize)
                .map(|presentation| presentation.span);
            return Err(KernelCheckedLinkError::new(format!(
                "kernel definition {} read expression {} (span {span:?}) has no lexical authority",
                owner.0, expression_id.0,
            )));
        }
        crate::PackedKernelOwnerNodeKind::UserCall { .. }
        | crate::PackedKernelOwnerNodeKind::FieldProjection { .. }
        | crate::PackedKernelOwnerNodeKind::RenderConstructor { .. }
        | crate::PackedKernelOwnerNodeKind::PureBuiltin { .. }
        | crate::PackedKernelOwnerNodeKind::FixedAbiCall { .. }
        | crate::PackedKernelOwnerNodeKind::HostEffect { .. } => {
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
    fields: &[crate::PackedExecutionRecordField],
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
                name: packed_symbol(definition, field.name, "record field")?.to_owned(),
                value: layout.expression(
                    owner,
                    definition
                        .resolve_value(field.value, expression.0 as usize)
                        .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?,
                )?,
                spread: field.spread,
                span: checked_nested_span(container_line, field.span.materialize()),
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
    definition: KernelDefinitionRef<'_>,
    facts: crate::PackedDefinitionFactsRef<'_>,
    payload: crate::PackedExpressionPayload,
) -> Result<Option<CheckedMatchPattern>, KernelCheckedLinkError> {
    let crate::PackedExpressionPayload::MatchPattern(pattern) = payload else {
        return Ok(None);
    };
    Ok(Some(match pattern {
        crate::PackedMatchPatternPayload::Wildcard => CheckedMatchPattern::Wildcard,
        crate::PackedMatchPatternPayload::Number(value) => CheckedMatchPattern::Number {
            value: facts.number(value).clone(),
        },
        crate::PackedMatchPatternPayload::Text(value) => CheckedMatchPattern::Text {
            value: facts.literal_text(value).to_owned(),
        },
        crate::PackedMatchPatternPayload::Tag { name, fields } => CheckedMatchPattern::Tag {
            name: packed_symbol(definition, name, "match tag")?.to_owned(),
            fields: packed_path_strings(definition, fields, "match tag fields")?,
        },
        crate::PackedMatchPatternPayload::Binding(name) => CheckedMatchPattern::Binding {
            name: packed_symbol(definition, name, "match binding")?.to_owned(),
        },
        crate::PackedMatchPatternPayload::Bits(value) => CheckedMatchPattern::Bits {
            value: facts.bits(value).clone(),
        },
        crate::PackedMatchPatternPayload::Invalid => return Ok(None),
    }))
}

fn exact_packed_input<'a>(
    inputs: &'a [crate::PackedKernelOwnerInputEdge],
    role: crate::PackedKernelOwnerEdgeRole,
    label: &str,
) -> Result<&'a crate::PackedKernelOwnerInputEdge, KernelCheckedLinkError> {
    let mut matching = inputs.iter().filter(|input| input.role == role);
    let input = matching.next().ok_or_else(|| {
        KernelCheckedLinkError::new(format!("packed {label} has no required input"))
    })?;
    if matching.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "packed {label} has multiple required inputs",
        )));
    }
    Ok(input)
}

fn exact_packed_execution_shape<'a>(
    facts: crate::PackedDefinitionFactsRef<'a>,
    expression: crate::KernelExpressionId,
) -> Result<Option<&'a crate::PackedExecutionShape>, KernelCheckedLinkError> {
    let mut matching = facts
        .execution_shapes()
        .iter()
        .filter(|shape| shape.expression() == expression);
    let shape = matching.next();
    if matching.next().is_some() {
        return Err(KernelCheckedLinkError::new(format!(
            "packed expression {} has multiple execution shapes",
            expression.0,
        )));
    }
    Ok(shape)
}

/// Traverse the permanent packed graph to derive one call-result projection.
///
/// This deliberately mirrors the rich checked projection oracle below, but
/// follows definition-local nodes through the link layout. It owns only the
/// caller-provided visitation bitmap and one reusable SymbolId stack; no rich
/// expression, field-name String, or nested path vector is constructed.
fn packed_projection_symbols_to_expression_with_scratch(
    layout: &KernelCheckedLinkLayout,
    snapshot: &KernelCheckedSnapshot,
    call_by_expression: &[Option<CheckedCallId>],
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
        let (owner, local) = layout.local_expression(current)?;
        let definition = snapshot.definition(owner).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "packed projection references missing definition {}",
                owner.0,
            ))
        })?;
        let node = definition.input().node(local).ok_or_else(|| {
            KernelCheckedLinkError::new(format!(
                "packed projection references missing expression {}:{}",
                owner.0, local.0,
            ))
        })?;
        let inputs = node.inputs(definition.input());

        macro_rules! reaches_encoded {
            ($encoded:expr) => {{
                let value = definition
                    .resolve_value($encoded, local.0 as usize)
                    .map_err(|error| KernelCheckedLinkError::new(error.to_string()))?;
                let linked = layout.expression(owner, value)?;
                packed_projection_symbols_to_expression_with_scratch(
                    layout,
                    snapshot,
                    call_by_expression,
                    linked,
                    target,
                    visiting,
                    projection,
                )?
            }};
        }

        macro_rules! reaches_linked {
            ($linked:expr) => {{
                packed_projection_symbols_to_expression_with_scratch(
                    layout,
                    snapshot,
                    call_by_expression,
                    $linked,
                    target,
                    visiting,
                    projection,
                )?
            }};
        }

        if let Some(call) = call_by_expression
            .get(current.0 as usize)
            .copied()
            .flatten()
        {
            let call = layout.packed_call(snapshot, call)?;
            for entry in call.entries()? {
                if let crate::PackedCallEntry::Input { value, .. } = entry
                    && reaches_linked!(layout.expression(call.owner, *value)?)
                {
                    return Ok(true);
                }
            }
            return Ok(false);
        }

        use crate::PackedKernelOwnerEdgeRole as Role;
        use crate::PackedKernelOwnerNodeKind as Kind;
        Ok(match node.kind {
            Kind::Record { .. } => {
                let Some(shape @ crate::PackedExecutionShape::Record { .. }) =
                    exact_packed_execution_shape(definition.runtime_facts(), local)?
                else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed record {}:{} has no exact execution shape",
                        owner.0, local.0,
                    )));
                };
                let fields = definition
                    .runtime_facts()
                    .execution_fields(shape)
                    .expect("record execution shape owns fields");
                let mut found = false;
                for field in fields {
                    projection.push(field.name);
                    if reaches_encoded!(field.value) {
                        found = true;
                        break;
                    }
                    projection.pop();
                }
                found
            }
            Kind::TextTemplate => {
                let mut found = false;
                for input in inputs
                    .iter()
                    .filter(|input| input.role == Role::TextDynamic)
                {
                    if reaches_encoded!(input.expression) {
                        found = true;
                        break;
                    }
                }
                found
            }
            Kind::Block => {
                let Some(shape @ crate::PackedExecutionShape::Block { result, .. }) =
                    exact_packed_execution_shape(definition.runtime_facts(), local)?
                else {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed BLOCK {}:{} has no exact execution shape",
                        owner.0, local.0,
                    )));
                };
                let mut found = false;
                for binding in definition
                    .runtime_facts()
                    .execution_bindings(shape)
                    .expect("BLOCK execution shape owns bindings")
                {
                    if reaches_encoded!(binding.value) {
                        found = true;
                        break;
                    }
                }
                if !found && let Some(result) = *result {
                    found = reaches_encoded!(result);
                }
                found
            }
            Kind::Collection { kind, .. } => {
                let role = match kind {
                    crate::KernelCollectionKind::Map => Role::MapEntry,
                    crate::KernelCollectionKind::List
                    | crate::KernelCollectionKind::Bytes
                    | crate::KernelCollectionKind::Set => Role::CollectionItem,
                };
                let mut found = false;
                for input in inputs.iter().filter(|input| input.role == role) {
                    if reaches_encoded!(input.expression) {
                        found = true;
                        break;
                    }
                }
                found
            }
            Kind::MapEntry => {
                let key = exact_packed_input(inputs, Role::MapKey, "map entry key")?;
                if reaches_encoded!(key.expression) {
                    true
                } else {
                    let value = exact_packed_input(inputs, Role::MapValue, "map entry value")?;
                    reaches_encoded!(value.expression)
                }
            }
            Kind::Latest => {
                let mut found = false;
                for input in inputs
                    .iter()
                    .filter(|input| input.role == Role::LatestBranch)
                {
                    if reaches_encoded!(input.expression) {
                        found = true;
                        break;
                    }
                }
                found
            }
            Kind::When => {
                if !matches!(
                    exact_packed_execution_shape(definition.runtime_facts(), local)?,
                    Some(crate::PackedExecutionShape::Conditional { .. })
                ) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed conditional {}:{} has no exact execution shape",
                        owner.0, local.0,
                    )));
                }
                let input = exact_packed_input(inputs, Role::WhenInput, "conditional selector")?;
                if reaches_encoded!(input.expression) {
                    true
                } else {
                    let mut found = false;
                    for arm in inputs.iter().filter(|input| input.role == Role::WhenArm) {
                        if reaches_encoded!(arm.expression) {
                            found = true;
                            break;
                        }
                    }
                    found
                }
            }
            Kind::Then => {
                let input = exact_packed_input(inputs, Role::ThenInput, "THEN input")?;
                if reaches_encoded!(input.expression) {
                    true
                } else if let Some(output) =
                    inputs.iter().find(|input| input.role == Role::ThenOutput)
                {
                    reaches_encoded!(output.expression)
                } else {
                    false
                }
            }
            Kind::Infix { .. } => {
                let left = exact_packed_input(inputs, Role::InfixLeft, "infix left")?;
                if reaches_encoded!(left.expression) {
                    true
                } else {
                    let right = exact_packed_input(inputs, Role::InfixRight, "infix right")?;
                    reaches_encoded!(right.expression)
                }
            }
            Kind::Draining => reaches_encoded!(
                exact_packed_input(inputs, Role::DrainingInput, "DRAINING")?.expression
            ),
            Kind::Hold => reaches_encoded!(
                exact_packed_input(inputs, Role::HoldInitial, "HOLD initial")?.expression
            ),
            Kind::Flush => reaches_encoded!(
                exact_packed_input(inputs, Role::FlushPayload, "FLUSH payload")?.expression
            ),
            Kind::MatchArm { .. } => {
                if !matches!(
                    exact_packed_execution_shape(definition.runtime_facts(), local)?,
                    Some(crate::PackedExecutionShape::MatchArm { .. })
                ) {
                    return Err(KernelCheckedLinkError::new(format!(
                        "packed match arm {}:{} has no exact execution shape",
                        owner.0, local.0,
                    )));
                }
                if let Some(output) = inputs.iter().find(|input| input.role == Role::MatchOutput) {
                    reaches_encoded!(output.expression)
                } else {
                    false
                }
            }
            Kind::UserCall { .. }
            | Kind::RenderConstructor { .. }
            | Kind::PureBuiltin { .. }
            | Kind::FixedAbiCall { .. }
            | Kind::HostEffect { .. }
            | Kind::FieldProjection { .. } => {
                return Err(KernelCheckedLinkError::new(format!(
                    "packed call expression {}:{} has no retained call row",
                    owner.0, local.0,
                )));
            }
            Kind::Known(_)
            | Kind::Source(_)
            | Kind::Absent
            | Kind::Text
            | Kind::Number
            | Kind::Byte
            | Kind::Bits(_)
            | Kind::Tag(_)
            | Kind::FormalRead { .. }
            | Kind::ContextRead { .. }
            | Kind::LexicalRead { .. }
            | Kind::ValueRead { .. }
            | Kind::DerivedRead { .. }
            | Kind::PatternRead { .. }
            | Kind::CollectionItemRead
            | Kind::FreshOut
            | Kind::Arrow
            | Kind::Delimiter
            | Kind::Unknown => false,
        })
    })();
    visiting[current.0 as usize] = false;
    result
}

#[cfg(test)]
fn checked_projection_symbols_to_expression_with_scratch(
    text: &boon_contract::ProjectTextSnapshot,
    packed_calls: Option<(&KernelCheckedLinkLayout, &KernelCheckedSnapshot)>,
    expressions: &[CheckedExpression],
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
                    packed_calls,
                    expressions,
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
                let Some((layout, snapshot)) = packed_calls else {
                    return Ok(false);
                };
                let call = layout.packed_call(snapshot, *call)?;
                let mut found = false;
                for entry in call.entries()? {
                    if let crate::PackedCallEntry::Input { value, .. } = entry
                        && reaches!(layout.expression(call.owner, *value)?)
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

fn lexical_payload_path(
    definition: KernelDefinitionRef<'_>,
    payload: crate::PackedExpressionPayload,
) -> Result<Option<String>, KernelCheckedLinkError> {
    let crate::PackedExpressionPayload::LexicalPath(path) = payload else {
        return Ok(None);
    };
    Ok(Some(
        packed_path_strings(definition, path, "lexical payload")?.join("."),
    ))
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
    declaration: &crate::PackedDeclaration,
    cache: &mut crate::DefinitionTypeMaterializationCache,
) -> Result<FlowType, KernelCheckedLinkError> {
    let definition = snapshot.definition(owner).ok_or_else(|| {
        KernelCheckedLinkError::new(format!(
            "kernel checked declaration flow references missing definition {}",
            owner.0,
        ))
    })?;
    let code = definition.code();
    let declaration_name = packed_symbol(definition, declaration.name, "declaration flow")?;
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
            .runtime_facts()
            .declarations()
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
                declaration_name,
                cache,
            )
        }
        crate::KernelDeclarationOrigin::CallbackBinding { call, ordinal } => fresh_out_flow_type(
            layout,
            snapshot,
            owner,
            declaration_name,
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
    let facts = definition.runtime_facts();
    let mut calls = facts
        .calls()
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
    let mut providers = facts
        .call_arguments(call_syntax)
        .iter()
        .filter_map(|argument| {
            (argument.kind == crate::KernelCallArgumentKind::BareBinding
                && definition.input().symbol(argument.name) == Some(declaration_name))
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
        .nodes()
        .get(provider.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("FreshOut provider expression is missing"))?;
    if !matches!(expression.kind, crate::PackedKernelOwnerNodeKind::FreshOut) {
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
        .nodes()
        .get(arm.0 as usize)
        .ok_or_else(|| KernelCheckedLinkError::new("pattern-binding match arm is missing"))?;
    let crate::PackedKernelOwnerNodeKind::MatchArm { pattern } = arm_expression.kind else {
        return Err(KernelCheckedLinkError::new(
            "pattern-binding declaration does not name a match arm",
        ));
    };
    let mut shapes = definition
        .runtime_facts()
        .execution_shapes()
        .iter()
        .filter_map(|shape| match shape {
            crate::PackedExecutionShape::MatchArm {
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
        crate::PackedKernelPattern::Binding { name }
            if definition.input().symbol(name) == Some(declaration_name) =>
        {
            selector.ty.clone()
        }
        crate::PackedKernelPattern::Tag { name, fields }
            if definition
                .input()
                .path(fields)
                .and_then(|fields| fields.name_at(ordinal as usize))
                == Some(declaration_name) =>
        {
            let name = definition.input().symbol(name).ok_or_else(|| {
                KernelCheckedLinkError::new(
                    "packed match pattern tag references text outside its project authority",
                )
            })?;
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
                    } if tag == name => payload.fields.get(declaration_name).cloned(),
                    Variant::Tag(_) | Variant::Tagged { .. } => None,
                })
                .unwrap_or(Type::Unknown)
        }
        crate::PackedKernelPattern::Wildcard
        | crate::PackedKernelPattern::Number
        | crate::PackedKernelPattern::Text
        | crate::PackedKernelPattern::Bits { .. }
        | crate::PackedKernelPattern::Tag { .. }
        | crate::PackedKernelPattern::Binding { .. }
        | crate::PackedKernelPattern::Invalid => Type::Unknown,
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

fn type_variables_in_flow(flow: &FlowType) -> BTreeSet<TypeVar> {
    let mut variables = BTreeSet::new();
    collect_flow_type_variables(flow, &mut variables);
    variables
}

fn referenced_abi_callable_ids(
    snapshot: &KernelCheckedSnapshot,
) -> Result<BTreeSet<crate::KernelAbiCallableId>, KernelCheckedLinkError> {
    let mut callables = BTreeSet::new();
    for definition in snapshot.definition_refs() {
        let owner = definition.owner().0;
        let code = definition.code();
        let mut syntax_by_expression = BTreeMap::new();
        for syntax in definition.runtime_facts().calls() {
            let function = packed_symbol(definition, syntax.function, "ABI call function")?;
            if syntax_by_expression
                .insert(syntax.expression, function)
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
            let retained = code
                .call_target(ordinal)
                .flatten()
                .ok_or_else(|| {
                    KernelCheckedLinkError::new(format!(
                        "kernel definition {owner} call expression {} has no retained callable-scheme target",
                        call.expression().0,
                    ))
                })?;
            let parameter_count = match call.target() {
                KernelCallTargetRef::User { target, .. } => {
                    if retained != crate::KernelCallableSchemeId::User(target) {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {owner} user call expression {} retained target {retained:?} instead of owner {}",
                            call.expression().0,
                            target.0,
                        )));
                    }
                    snapshot
                        .definition_code
                        .definition(target)
                        .ok_or_else(|| {
                            KernelCheckedLinkError::new(format!(
                                "kernel definition {owner} calls missing packed definition {}",
                                target.0,
                            ))
                        })?
                        .callable_type_parameters()
                        .len()
                }
                KernelCallTargetRef::RenderConstructor { .. }
                | KernelCallTargetRef::PureBuiltin { .. }
                | KernelCallTargetRef::FixedAbi
                | KernelCallTargetRef::HostEffect { .. }
                | KernelCallTargetRef::FieldProjection { .. } => {
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
                    let crate::KernelCallableSchemeId::Abi(callable) = retained else {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {owner} ABI call expression {} retained non-ABI target {retained:?}",
                            call.expression().0,
                        )));
                    };
                    let scheme = snapshot
                        .definition_code
                        .abi_callable_scheme(callable)
                        .ok_or_else(|| {
                        KernelCheckedLinkError::new(format!(
                            "kernel definition {owner} references ABI callable `{function}` absent from its packed ABI catalog",
                        ))
                    })?;
                    if scheme.name() != function {
                        return Err(KernelCheckedLinkError::new(format!(
                            "kernel definition {owner} ABI call expression {} names `{function}` but retained packed callable `{}`",
                            call.expression().0,
                            scheme.name(),
                        )));
                    }
                    callables.insert(callable);
                    scheme.type_parameters().len()
                }
            };
            for substitution in code
                .call_type_substitutions(ordinal)
                .expect("sealed call substitution span is valid")
            {
                if substitution.variable.0 as usize >= parameter_count {
                    return Err(KernelCheckedLinkError::new(format!(
                        "kernel definition {owner} call expression {} substitution parameter {} is outside its target scheme's {parameter_count} parameters",
                        call.expression().0,
                        substitution.variable.0,
                    )));
                }
            }
        }
    }
    Ok(callables)
}

fn checked_abi_contextual_operation(
    layout: &KernelCheckedAbiCallableLayout,
    name: &str,
    operation: KernelAbiContextualOperation,
) -> Result<CheckedContextualOperation, KernelCheckedLinkError> {
    let parameter = |ordinal: u32, role: &str| {
        layout
            .parameters
            .resolve(ordinal, "ABI contextual parameter")
            .map(DeclId)
            .map_err(|_| {
                KernelCheckedLinkError::new(format!(
                    "kernel ABI callable `{}` contextual {role} references missing parameter ordinal {ordinal}",
                    name,
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

    #[test]
    fn independent_ownership_plan_rejects_an_actual_publication_route_swap() {
        let role = ProgramRole::Client;
        let source_bundle_digest_v1 = SourceBundleDigestV1::new(
            "ownership-swap.bn",
            [boon_contract::SourceBundleUnit::new(
                "ownership-swap.bn",
                "value: 1",
            )],
        )
        .expect("build ownership-swap source digest");
        let owner = |name: &str| CheckedShardOwnerKeyV2::Callable {
            role,
            callable_kind: CheckedShardCallableKindV2::User,
            name: name.to_owned(),
            external_identity: None,
        };
        let expected_definition = checked_link_definition_projection(owner("expected"));
        let swapped_definition = checked_link_definition_projection(owner("swapped"));

        let mut preparation = BuildingKernelCheckedImageOwnershipPlanV1::new();
        let expected = preparation
            .intern_projection(expected_definition.clone())
            .expect("prepare expected Definition projection");
        preparation
            .intern_projection(swapped_definition.clone())
            .expect("prepare swapped Definition projection");
        preparation
            .route(CheckedImageRowDomainV2::Expression, 0, expected)
            .expect("prepare expected Expression route");
        let plan = preparation.finish().expect("finish independent plan");

        let (publication, mut expectation) = CheckedImageKernelPublicationV1::__kernel_new_pair(
            source_bundle_digest_v1,
            role,
            plan.projection_count(),
            plan.route_count(),
        );
        let mut actual = KernelCheckedImagePublicationBuilderV1::new(publication, &plan);
        let expected = actual
            .__kernel_intern_projection(expected_definition)
            .expect("publish independently selected expected Definition key");
        let wrong = actual
            .__kernel_intern_projection(swapped_definition)
            .expect("publish independently selected wrong-owner Definition key");

        // Fault injection is deliberately after actual-key selection and
        // immediately before the production publication writer. The expected
        // route is already complete and cannot observe this replacement.
        let error = actual
            .publish_routed_rows(CheckedImageRowDomainV2::Expression, 0, wrong, 1)
            .expect_err("independent ownership must reject the actual route swap");
        assert!(error.contains("independent ownership plan"), "{error}");
        actual
            .publish_routed_rows(CheckedImageRowDomainV2::Expression, 0, expected, 1)
            .expect("the correct route remains publishable after the rejected owner swap");
        actual
            .__kernel_publish_rows(wrong, 1)
            .expect("the unrelated planned projection receives its own row");
        let publication = actual.into_publication();
        plan.install(&mut expectation)
            .expect("install the independent plan after publication");
        expectation
            .__kernel_freeze_against(&publication)
            .expect("rejected owner swap leaves no route-coverage residue");
    }

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
                None,
                expressions,
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
    fn generic_union_substitutions_link_to_their_explicit_target_alphas() {
        let unit = SourceUnitId::from_path("generic-union.bn").unwrap();
        let provider_key = owner_key(&unit, "provider");
        let consumer_key = owner_key(&unit, "consumer");
        let actual = boon_checked::canonical_union_type(vec![
            Type::List(Type::shared(Type::Number)),
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Text)],
                false,
            )),
        ]);
        let provider = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Known(actual),
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
                kind: KernelOwnerNodeKind::PureBuiltin {
                    kind: crate::KernelPureBuiltinKind::RecordConstructor,
                },
                inputs: vec![KernelOwnerInputEdge {
                    role: KernelOwnerEdgeRole::AbiArgument {
                        name: "$pipe".into(),
                    },
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
        provider_facts.presentation.expressions[0].declaration = None;
        provider_facts.lexical_bindings = vec![KernelLexicalBindingInput {
            expression: KernelExpressionId(0),
            target: KernelLexicalBindingTargetInput::Declaration(
                KernelDeclarationReference::Local(KernelDeclarationId(0)),
            ),
            projection: Box::new([]),
            access: KernelLexicalAccess::Read,
        }]
        .into_boxed_slice();
        let mut consumer_facts = facts(&unit, &consumer_key, "consumer");
        let occurrence = StableOccurrenceKey {
            source_unit_id: unit.clone(),
            route: boon_syntax::StableOccurrenceRoute {
                owner: None,
                statement_route: Vec::new(),
                expression_route: Vec::new(),
            },
        };
        let authored_site_digest_v4 =
            boon_checked::checked_structural_call_site_digest_v4(&occurrence).unwrap();
        consumer_facts.call_syntax = vec![crate::KernelCallSyntaxInput {
            expression: KernelExpressionId(0),
            occurrence,
            authored_site_digest_v4,
            function: "Generic/union".into(),
            pipe_input: Some(KernelExpressionId(1)),
            arguments: Box::new([]),
            pass: None,
        }]
        .into_boxed_slice();

        let list_parameter = TypeVar(0);
        let object_parameter = TypeVar(1);
        let formal = boon_checked::canonical_union_type(vec![
            Type::List(Type::shared(Type::Var(list_parameter))),
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Var(object_parameter))],
                false,
            )),
        ]);
        let abi = crate::KernelAbiInput::new(
            ProgramRole::Client,
            [crate::KernelCallableAbiInput {
                name: "Generic/union".into(),
                kind: crate::KernelCallableKind::Builtin,
                intrinsic: None,
                external_identity: None,
                parameters: vec![crate::KernelAbiParameterInput {
                    name: "value".into(),
                    kind: CheckedParameterKind::Value,
                    ordinal: 0,
                    flow_type: FlowType {
                        mode: FlowMode::Continuous,
                        ty: formal,
                    },
                    requirement: CheckedParameterRequirement::Required,
                    evaluation_scope: crate::KernelParameterEvaluationScope::Parent,
                }]
                .into_boxed_slice(),
                contexts: Box::new([]),
                result: FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Number,
                },
                result_specialization: crate::KernelAbiResultSpecialization::Fixed,
                role: ProgramRole::Client,
                effect: CheckedEffectSummary::default(),
                contextual_operation: None,
            }],
        )
        .unwrap();
        let project = KernelProjectInput::new_with_abi(
            KernelProjectProgramInput {
                owners: vec![provider, consumer].into_boxed_slice(),
            },
            vec![provider_facts, consumer_facts].into_boxed_slice(),
            vec![provider_key, consumer_key].into_boxed_slice(),
            abi,
        )
        .unwrap();
        let mut session = KernelSession::new(project);
        let checked = session.check(CheckDemand::CheckedImage).unwrap();
        let KernelCheckProduct::CheckedImage(snapshot) = checked.product else {
            unreachable!()
        };
        let layout = KernelCheckedLinkLayout::new(session.project(), &snapshot).unwrap();
        let [abi_layout] = layout.abi_callables() else {
            panic!("generic union fixture must allocate one ABI callable")
        };
        assert_eq!(
            layout
                .callable_type_parameter_layouts(&snapshot)
                .expect("retained ABI parameters relocate through the exact checked layout")
                .as_ref(),
            [
                KernelCheckedCallableTypeParameterLayout {
                    target: crate::KernelCallableSchemeId::Abi(crate::KernelAbiCallableId(0)),
                    callable: abi_layout.declaration,
                    parameter: crate::KernelTypeParameterId(0),
                    linked_local: object_parameter.0,
                    linked_variable: TypeVar(abi_layout.type_variables.start + object_parameter.0,),
                },
                KernelCheckedCallableTypeParameterLayout {
                    target: crate::KernelCallableSchemeId::Abi(crate::KernelAbiCallableId(0)),
                    callable: abi_layout.declaration,
                    parameter: crate::KernelTypeParameterId(1),
                    linked_local: list_parameter.0,
                    linked_variable: TypeVar(abi_layout.type_variables.start + list_parameter.0,),
                },
            ],
        );
        let rows = layout
            .materialize_rows(
                &snapshot,
                SourceBundleDigestV1::new(
                    "generic-union.bn",
                    [boon_contract::SourceBundleUnit::new("generic-union.bn", "")],
                )
                .unwrap(),
                ProgramRole::Client,
                KernelCheckedRowProjectionDemand::EditorRich,
            )
            .unwrap();
        let [call] = rows.calls.as_ref() else {
            panic!("generic union fixture must link one checked call")
        };
        let substitutions = call
            .type_substitutions
            .iter()
            .map(|substitution| (substitution.variable, substitution.value.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            substitutions.get(&object_parameter),
            Some(&Type::Text),
            "packed parameter zero is Object<U> and must link to U, not rich Union's first T",
        );
        assert_eq!(
            substitutions.get(&list_parameter),
            Some(&Type::Number),
            "packed parameter one is List<T> and must link to T",
        );
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
        let program = KernelProjectProgramInput {
            owners: vec![provider, consumer].into_boxed_slice(),
        };
        let definition_facts = vec![provider_facts, consumer_facts].into_boxed_slice();
        let definition_keys = vec![provider_key, consumer_key].into_boxed_slice();
        let project = KernelProjectInput::new(
            program.clone(),
            definition_facts.clone(),
            definition_keys.clone(),
        )
        .unwrap();
        let mut session = KernelSession::new(project);
        let checked = session.check(CheckDemand::CheckedImage).unwrap();
        let KernelCheckProduct::CheckedImage(snapshot) = checked.product else {
            unreachable!()
        };
        let layout = KernelCheckedLinkLayout::new(session.project(), &snapshot).unwrap();
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

        let packed = layout
            .link_runtime_packed(
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
            )
            .expect("runtime linker must produce only compact authorities");
        let runtime_rows = layout
            .materialize_rows(
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
                KernelCheckedRowProjectionDemand::RuntimePacked,
            )
            .expect("transitional runtime rows remain a differential oracle");
        assert_eq!(packed.semantic_input, runtime_rows.semantic_input);
        assert_eq!(packed.runtime_flow_terms, runtime_rows.runtime_flow_terms);
        assert_eq!(
            packed.checked_image_publication,
            runtime_rows.checked_image_publication,
        );
        assert_eq!(
            packed
                .definition_authority_root_scope(KernelOwnerId(0))
                .unwrap(),
            LexicalScopeId(0),
        );

        let consumer = snapshot.definition(KernelOwnerId(1)).unwrap();
        let imported = consumer
            .resolve_value(
                consumer.input().nodes()[0].inputs(consumer.input())[0].expression,
                0,
            )
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

        let mut scoped_facts = definition_facts.clone();
        scoped_facts[0].presentation.scopes = vec![crate::KernelScopePresentation {
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
        scoped_facts[1].presentation.containing_scope = KernelScopeReference::Owner {
            owner: KernelOwnerId(0),
            scope: crate::KernelScopeId(0),
        };
        scoped_facts[1].statements[0].value_use = KernelStatementValueUse::RenderSlot;
        let scoped_project = KernelProjectInput::new(
            program.clone(),
            scoped_facts.clone(),
            definition_keys.clone(),
        )
        .unwrap();
        let mut scoped_session = KernelSession::new(scoped_project);
        let scoped_checked = scoped_session.check(CheckDemand::CheckedImage).unwrap();
        let KernelCheckProduct::CheckedImage(scoped) = scoped_checked.product else {
            unreachable!()
        };
        let scoped_layout = KernelCheckedLinkLayout::new(scoped_session.project(), &scoped)
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

        let mut missing_scope_facts = scoped_facts.clone();
        missing_scope_facts[1].presentation.containing_scope = KernelScopeReference::Owner {
            owner: KernelOwnerId(0),
            scope: crate::KernelScopeId(99),
        };
        let missing_scope_project = KernelProjectInput::new(
            program.clone(),
            missing_scope_facts,
            definition_keys.clone(),
        )
        .unwrap();
        let mut missing_scope_session = KernelSession::new(missing_scope_project);
        let missing_scope_checked = missing_scope_session
            .check(CheckDemand::CheckedImage)
            .unwrap();
        let KernelCheckProduct::CheckedImage(missing_scope) = missing_scope_checked.product else {
            unreachable!()
        };
        let error = KernelCheckedLinkLayout::new(missing_scope_session.project(), &missing_scope)
            .expect_err("a missing enclosing scope must fail before row materialization");
        assert!(error.to_string().contains("containing scope"));

        let mut delegated_facts = definition_facts.clone();
        delegated_facts[1].statements[0].kind = KernelStatementKind::Source {
            field: None,
            event: None,
        };
        delegated_facts[1].declarations = Box::new([]);
        delegated_facts[1].presentation.declarations = Box::new([]);
        delegated_facts[1].presentation.expressions[0].declaration = None;
        delegated_facts[1].linkage.public_declaration =
            Some(KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)));
        let delegated_project = KernelProjectInput::new(
            program.clone(),
            delegated_facts.clone(),
            definition_keys.clone(),
        )
        .unwrap();
        let mut delegated_session = KernelSession::new(delegated_project);
        let delegated_checked = delegated_session.check(CheckDemand::CheckedImage).unwrap();
        let KernelCheckProduct::CheckedImage(delegated) = delegated_checked.product else {
            unreachable!()
        };
        let delegated_layout =
            KernelCheckedLinkLayout::new(delegated_session.project(), &delegated)
                .expect("a nested definition may share its enclosing public declaration");
        assert_eq!(
            delegated_layout.definitions()[1].public_declaration,
            DeclId(1)
        );

        delegated_facts[0].statements[0].kind = KernelStatementKind::Source {
            field: None,
            event: None,
        };
        delegated_facts[0].declarations = Box::new([]);
        delegated_facts[0].presentation.declarations = Box::new([]);
        delegated_facts[0].presentation.expressions[0].declaration = None;
        delegated_facts[0].linkage.public_declaration =
            Some(KernelDeclarationReference::OwnerPublic(KernelOwnerId(1)));
        let cycle_project =
            KernelProjectInput::new(program, delegated_facts, definition_keys).unwrap();
        let mut cycle_session = KernelSession::new(cycle_project);
        let error = cycle_session
            .check(CheckDemand::CheckedImage)
            .expect_err("public declaration authority cycles must fail before publication");
        assert!(error.to_string().contains("cycle"));

        let mut invalid = (*snapshot).clone();
        Arc::make_mut(&mut invalid.program)
            .corrupt_external_owner_for_test(1, 0, KernelOwnerId(99))
            .expect("the fixture retains its external expression");
        let error = KernelCheckedLinkLayout::new(session.project(), &invalid)
            .expect_err("an unlinked external owner must fail before row allocation");
        assert!(error.to_string().contains("missing definition 99"));
    }
}
