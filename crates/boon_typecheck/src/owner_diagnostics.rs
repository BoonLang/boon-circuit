//! Construction-independent project diagnostic and presentation facts.
//!
//! These facts sit downstream of immutable owner body inference but upstream
//! of checked-row construction.  Diagnostics and verified compatibility
//! assembly consume the same output/render/host authority; the dense assembly
//! is allowed to relocate these stable facts, but not to rediscover them.

use crate::{
    HostPortSyntaxTable, OwnerAbiEnvironment, OwnerBodyInferenceShard, OwnerConstraintSummary,
    OwnerContainingScopeInput, OwnerEffectiveLexicalTarget, OwnerExpressionRef,
    OwnerInferenceAbiEnvironment, OwnerInferenceExpressionRef, OwnerLexicalAccess,
    OwnerLexicalDeclarationTarget, OwnerLexicalPlan, OwnerSignatureOutputBindingPlan,
    OwnerSourceAnchorRole, OwnerSourceAnchorSite, OwnerSourceMap, OwnerStatementId,
    OwnerSyntaxGraph, OwnerSyntaxInput, RenderContractRegistry, SourcePayloadPathLookup,
    TypecheckSyntaxProgram, canonicalize_diagnostics, diagnostic_at_line, host_port_payload_types,
    host_port_table, http_response_type_is_valid, open_object_type, render_slot_type_error,
    statement_contains_output_authority, statement_is_empty_delimiter, substitute_checked_type,
    type_contains_absence, type_for_nested_path, type_is_deferred_order_key, type_is_orderable_key,
    type_may_be_ordered_list, union_structural_type, websocket_actions_type_is_valid,
};
use boon_checked::{
    CheckedCallableKind, CheckedEffectSummary, CheckedValueUse, DiagnosticSeverity, FlowMode,
    FlowType, OwnerDeclarationStableKey, OwnerLexicalDeclarationCapability, OwnerLexicalTargetRef,
    Type, TypeDiagnostic, apply_checked_type_substitutions, is_registered_render_constructor,
};
use boon_contract::SourceBundleDigestV1;
use boon_data::{Bits, ExactNumber};
use boon_parser::ProjectSyntaxSnapshot;
use boon_syntax::{
    AstExprKind, AstParameterKind, AstStatementKind, BytesSizeSyntax, SourceUnitId,
    StableCheckOwnerKey, StableExpressionKey, StableStatementKey,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

const PROJECT_DIAGNOSTIC_FACTS_DOMAIN_V10: &[u8] = b"boon.project-diagnostic-facts.v10\0";
const OWNER_DIAGNOSTIC_REPLAY_FACTS_DOMAIN_V4: &[u8] = b"boon.owner-diagnostic-replay-facts.v4\0";
const OWNER_DIAGNOSTIC_REPLAY_CURRENTNESS_DOMAIN_V4: &[u8] =
    b"boon.owner-diagnostic-replay-currentness.v4\0";
const PROJECT_OUTPUT_FLOW_FACTS_DOMAIN_V1: &[u8] = b"boon.project-output-flow-facts.v1\0";
const SOURCE_UNIT_PROJECT_DIAGNOSTICS_DOMAIN_V1: &[u8] =
    b"boon.source-unit-project-diagnostics.v1\0";
const SOURCE_UNIT_PROJECT_DIAGNOSTICS_CURRENTNESS_DOMAIN_V1: &[u8] =
    b"boon.source-unit-project-diagnostics-currentness.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectDiagnosticFactsError {
    message: String,
}

impl ProjectDiagnosticFactsError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProjectDiagnosticFactsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ProjectDiagnosticFactsError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectOutputRootFact {
    pub name: String,
    pub statement: StableStatementKey,
    pub value: Option<StableExpressionKey>,
    pub ty: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectRenderSlotFact {
    pub owner: StableCheckOwnerKey,
    pub statement: StableStatementKey,
    pub value: Option<StableExpressionKey>,
    pub slot_name: String,
    pub expected_contract: String,
    pub actual_type: Type,
    pub diagnostics: Box<[TypeDiagnostic]>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectOutputTargetFact {
    Parameter {
        owner: StableCheckOwnerKey,
        ordinal: u32,
    },
    Fresh {
        owner: StableCheckOwnerKey,
        call: StableExpressionKey,
        formal_ordinal: u32,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ProjectOutputDriverFact {
    pub owner: StableCheckOwnerKey,
    pub call: StableExpressionKey,
    pub formal_ordinal: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectOutputProducerFact {
    pub target: ProjectOutputTargetFact,
    pub drivers: Box<[ProjectOutputDriverFact]>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProjectOutputDiagnosticSite {
    FunctionParameter {
        owner: StableCheckOwnerKey,
        statement: StableStatementKey,
        ordinal: u32,
    },
    Expression {
        owner: StableCheckOwnerKey,
        expression: StableExpressionKey,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ProjectOutputDiagnosticTemplate {
    site: ProjectOutputDiagnosticSite,
    message: String,
}

/// Span-free cross-owner OUT topology shared by exact flow projection and
/// structural-producer diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectOutputFlowFacts {
    owners: Box<[StableCheckOwnerKey]>,
    producers: Box<[ProjectOutputProducerFact]>,
    list_sources: BTreeMap<ProjectOutputTargetFact, Box<[ProjectOrderExpressionFact]>>,
    forward_sources: BTreeMap<ProjectOutputTargetFact, Box<[ProjectOutputTargetFact]>>,
    diagnostics: Box<[ProjectOutputDiagnosticTemplate]>,
    fingerprint_v1: [u8; 32],
}

impl ProjectOutputFlowFacts {
    pub fn producers(&self) -> &[ProjectOutputProducerFact] {
        &self.producers
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    fn matches_owners<'a>(
        &self,
        owners: impl IntoIterator<Item = &'a StableCheckOwnerKey>,
    ) -> bool {
        let owners = owners.into_iter().collect::<Vec<_>>();
        let unique = owners.iter().copied().collect::<BTreeSet<_>>();
        owners.len() == unique.len() && self.owners.iter().eq(unique.into_iter())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ProjectOrderExpressionFact {
    pub owner: StableCheckOwnerKey,
    pub expression: StableExpressionKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectOrderDirectionFact {
    Ascending,
    Descending,
    Dynamic {
        expression: ProjectOrderExpressionFact,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectOrderKeyFact {
    pub call_path: Box<[ProjectOrderExpressionFact]>,
    pub key: ProjectOrderExpressionFact,
    pub direction: ProjectOrderDirectionFact,
    pub key_type: Type,
    pub pure: bool,
    pub total: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectCallOrderChainFact {
    pub call: ProjectOrderExpressionFact,
    pub keys: Box<[ProjectOrderKeyFact]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectOrderFacts {
    chains: Box<[ProjectCallOrderChainFact]>,
    fingerprint_v1: [u8; 32],
}

impl ProjectOrderFacts {
    pub fn chains(&self) -> &[ProjectCallOrderChainFact] {
        &self.chains
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

/// Stable project facts required by complete diagnostics and later dense
/// relocation. Dense compatibility assembly may relocate the facts, but it is
/// not a second owner of output/render/host/order diagnostic semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectDiagnosticFacts {
    source_bundle_digest_v1: SourceBundleDigestV1,
    output_roots: Box<[ProjectOutputRootFact]>,
    output_producers: Box<[ProjectOutputProducerFact]>,
    order: ProjectOrderFacts,
    render_slots: Box<[ProjectRenderSlotFact]>,
    host_ports: HostPortSyntaxTable,
    host_port_resolution_error: Option<String>,
    diagnostics: Box<[TypeDiagnostic]>,
    fingerprint_v1: [u8; 32],
}

impl ProjectDiagnosticFacts {
    pub const fn source_bundle_digest_v1(&self) -> SourceBundleDigestV1 {
        self.source_bundle_digest_v1
    }

    pub fn output_roots(&self) -> &[ProjectOutputRootFact] {
        &self.output_roots
    }

    pub fn output_producers(&self) -> &[ProjectOutputProducerFact] {
        &self.output_producers
    }

    pub const fn order(&self) -> &ProjectOrderFacts {
        &self.order
    }

    pub fn render_slots(&self) -> &[ProjectRenderSlotFact] {
        &self.render_slots
    }

    pub fn diagnostics(&self) -> &[TypeDiagnostic] {
        &self.diagnostics
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub(crate) const fn host_ports(&self) -> &HostPortSyntaxTable {
        &self.host_ports
    }

    pub(crate) fn host_port_resolution_error(&self) -> Option<&str> {
        self.host_port_resolution_error.as_deref()
    }
}

/// One project diagnostic relocated into a source unit's local coordinate
/// space.
///
/// Legacy registry diagnostics sometimes carry only a global line and leave
/// both byte offsets at zero. `relocate_bytes` preserves that representation
/// exactly when the final project receipt reapplies layout offsets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceUnitProjectDiagnosticRow {
    diagnostic: TypeDiagnostic,
    relocate_bytes: bool,
}

impl SourceUnitProjectDiagnosticRow {
    pub const fn diagnostic(&self) -> &TypeDiagnostic {
        &self.diagnostic
    }

    pub const fn relocate_bytes(&self) -> bool {
        self.relocate_bytes
    }
}

/// Backdatable source-unit-local presentation of project-wide diagnostic
/// decisions. The output fingerprint intentionally excludes project layout
/// and upstream bases; request currentness is carried by the paired evaluation
/// receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceUnitProjectDiagnostics {
    source_unit_id: SourceUnitId,
    rows: Box<[SourceUnitProjectDiagnosticRow]>,
    fingerprint_v1: [u8; 32],
}

impl SourceUnitProjectDiagnostics {
    pub const fn source_unit_id(&self) -> &SourceUnitId {
        &self.source_unit_id
    }

    pub fn rows(&self) -> &[SourceUnitProjectDiagnosticRow] {
        &self.rows
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceUnitProjectDiagnosticsBasis {
    source_bundle_digest_v1: SourceBundleDigestV1,
    project_facts_fingerprint_v1: [u8; 32],
    source_unit_id: SourceUnitId,
    layout_start_line: usize,
    layout_start_byte: usize,
    layout_source_len: usize,
    layout_line_count: usize,
}

impl SourceUnitProjectDiagnosticsBasis {
    fn from_inputs(
        project: &ProjectSyntaxSnapshot,
        source_unit_id: &SourceUnitId,
        project_facts: &ProjectDiagnosticFacts,
    ) -> Result<Self, ProjectDiagnosticFactsError> {
        let source_bundle_digest_v1 = project.source_bundle_digest_v1();
        if project_facts.source_bundle_digest_v1() != source_bundle_digest_v1 {
            return Err(ProjectDiagnosticFactsError::new(
                "source-unit project diagnostics received facts for another source bundle",
            ));
        }
        let layout = project
            .source_layouts()
            .iter()
            .find(|layout| &layout.source_unit_id == source_unit_id)
            .ok_or_else(|| {
                ProjectDiagnosticFactsError::new(
                    "source-unit project diagnostics source unit is absent from the project layout",
                )
            })?;
        Ok(Self {
            source_bundle_digest_v1,
            project_facts_fingerprint_v1: project_facts.fingerprint_v1(),
            source_unit_id: source_unit_id.clone(),
            layout_start_line: layout.start_line,
            layout_start_byte: layout.start_byte,
            layout_source_len: layout.source_len,
            layout_line_count: layout.line_count,
        })
    }

    fn matches_inputs(
        &self,
        project: &ProjectSyntaxSnapshot,
        project_facts: &ProjectDiagnosticFacts,
    ) -> bool {
        Self::from_inputs(project, &self.source_unit_id, project_facts)
            .is_ok_and(|basis| basis == *self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceUnitProjectDiagnosticsCurrentness {
    basis: SourceUnitProjectDiagnosticsBasis,
    result_fingerprint_v1: [u8; 32],
    fingerprint_v1: [u8; 32],
}

impl SourceUnitProjectDiagnosticsCurrentness {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub const fn result_fingerprint_v1(&self) -> [u8; 32] {
        self.result_fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceUnitProjectDiagnosticsEvaluation {
    pub currentness: SourceUnitProjectDiagnosticsCurrentness,
    pub result: Arc<SourceUnitProjectDiagnostics>,
}

impl SourceUnitProjectDiagnosticsEvaluation {
    pub fn matches_inputs(
        &self,
        project: &ProjectSyntaxSnapshot,
        project_facts: &ProjectDiagnosticFacts,
    ) -> bool {
        self.currentness
            .basis
            .matches_inputs(project, project_facts)
            && self.currentness.result_fingerprint_v1 == self.result.fingerprint_v1()
    }
}

pub fn evaluate_source_unit_project_diagnostics(
    project: &ProjectSyntaxSnapshot,
    source_unit_id: &SourceUnitId,
    project_facts: &ProjectDiagnosticFacts,
) -> Result<SourceUnitProjectDiagnosticsEvaluation, ProjectDiagnosticFactsError> {
    let basis =
        SourceUnitProjectDiagnosticsBasis::from_inputs(project, source_unit_id, project_facts)?;
    let line_end = basis
        .layout_start_line
        .checked_add(basis.layout_line_count.max(1))
        .ok_or_else(|| {
            ProjectDiagnosticFactsError::new("project diagnostic line range overflow")
        })?;
    let diagnostics = project_facts.diagnostics();
    // ProjectDiagnosticFacts owns the canonical source ordering. Index the
    // unit range directly instead of rescanning every project row per unit.
    let start = diagnostics.partition_point(|diagnostic| diagnostic.line < basis.layout_start_line);
    let end = diagnostics.partition_point(|diagnostic| diagnostic.line < line_end);
    let byte_end = basis
        .layout_start_byte
        .checked_add(basis.layout_source_len)
        .ok_or_else(|| {
            ProjectDiagnosticFactsError::new("project diagnostic byte range overflow")
        })?;
    let mut rows = Vec::with_capacity(end.saturating_sub(start));
    for diagnostic in &diagnostics[start..end] {
        if diagnostic.line == 0 || diagnostic.start > diagnostic.end {
            return Err(ProjectDiagnosticFactsError::new(
                "project diagnostic has an invalid global source span",
            ));
        }
        let relocate_bytes = diagnostic.start != 0 || diagnostic.end != 0;
        let (local_start, local_end) = if relocate_bytes {
            if diagnostic.start < basis.layout_start_byte || diagnostic.end > byte_end {
                return Err(ProjectDiagnosticFactsError::new(
                    "project diagnostic byte span crosses its source-unit layout",
                ));
            }
            (
                diagnostic.start - basis.layout_start_byte,
                diagnostic.end - basis.layout_start_byte,
            )
        } else {
            (0, 0)
        };
        rows.push(SourceUnitProjectDiagnosticRow {
            diagnostic: TypeDiagnostic {
                severity: diagnostic.severity,
                line: diagnostic.line - basis.layout_start_line + 1,
                start: local_start,
                end: local_end,
                message: diagnostic.message.clone(),
            },
            relocate_bytes,
        });
    }
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        SOURCE_UNIT_PROJECT_DIAGNOSTICS_DOMAIN_V1,
        &(source_unit_id, &rows),
    )
    .map_err(|error| {
        ProjectDiagnosticFactsError::new(format!(
            "cannot fingerprint source-unit project diagnostics: {error}"
        ))
    })?;
    let result = Arc::new(SourceUnitProjectDiagnostics {
        source_unit_id: source_unit_id.clone(),
        rows: rows.into_boxed_slice(),
        fingerprint_v1,
    });
    let result_fingerprint_v1 = result.fingerprint_v1();
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        SOURCE_UNIT_PROJECT_DIAGNOSTICS_CURRENTNESS_DOMAIN_V1,
        &(&basis, result_fingerprint_v1),
    )
    .map_err(|error| {
        ProjectDiagnosticFactsError::new(format!(
            "cannot fingerprint source-unit project diagnostics currentness: {error}"
        ))
    })?;
    Ok(SourceUnitProjectDiagnosticsEvaluation {
        currentness: SourceUnitProjectDiagnosticsCurrentness {
            basis,
            result_fingerprint_v1,
            fingerprint_v1,
        },
        result,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticStableStatementValue {
    statement: StableStatementKey,
    value: ProjectOrderExpressionFact,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticStableExpressionFlow {
    expression: StableExpressionKey,
    flow_type: FlowType,
    flush_type: Option<Type>,
    direct_effect: CheckedEffectSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticStableCallActualFact {
    input: ProjectOrderExpressionFact,
    actual_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticStableMatchedInputFact {
    formal_ordinal: u32,
    formal_name: String,
    formal_kind: crate::OwnerParameterKind,
    expression: ProjectOrderExpressionFact,
    from_pipe: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticStableCallFact {
    expression: StableExpressionKey,
    function: String,
    target: crate::InferredOwnerCallableTarget,
    inputs: Box<[OwnerDiagnosticStableCallActualFact]>,
    valid: bool,
    result: FlowType,
    diagnostic: crate::OwnerDiagnosticCallFact,
    matched_inputs: Box<[OwnerDiagnosticStableMatchedInputFact]>,
    explicit_pass: Option<ProjectOrderExpressionFact>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticStableReadFact {
    expression: StableExpressionKey,
    read: crate::OwnerEffectiveLexicalReadPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticOutputDeclarationFact {
    target: ProjectOutputTargetFact,
    name: String,
    statement: StableStatementKey,
    ordinal: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "target_kind", rename_all = "snake_case")]
enum OwnerDiagnosticOutputCallTargetFact {
    Owner {
        owner: StableCheckOwnerKey,
    },
    Abi {
        function: String,
        kind: CheckedCallableKind,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticOutputCallInputFact {
    formal_ordinal: u32,
    expression: ProjectOrderExpressionFact,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticOutputBindingFact {
    target: ProjectOutputTargetFact,
    formal_ordinal: u32,
    fresh_name: Option<String>,
    forwarding: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerDiagnosticOutputCallFact {
    expression: StableExpressionKey,
    target: OwnerDiagnosticOutputCallTargetFact,
    inputs: Box<[OwnerDiagnosticOutputCallInputFact]>,
    outputs: Box<[OwnerDiagnosticOutputBindingFact]>,
}

/// Stable, span-free replay inputs projected once for one owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerDiagnosticReplayFacts {
    owner: StableCheckOwnerKey,
    containing_scope: OwnerContainingScopeInput,
    function_name: Option<String>,
    expression_flows: Box<[OwnerDiagnosticStableExpressionFlow]>,
    statement_values: Box<[OwnerDiagnosticStableStatementValue]>,
    calls: Box<[OwnerDiagnosticStableCallFact]>,
    reads: Box<[OwnerDiagnosticStableReadFact]>,
    output_declarations: Box<[OwnerDiagnosticOutputDeclarationFact]>,
    output_calls: Box<[OwnerDiagnosticOutputCallFact]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerDiagnosticReplayFacts {
    pub const fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerDiagnosticReplayFactsBasis {
    owner: StableCheckOwnerKey,
    syntax_fingerprint_v1: [u8; 32],
    lexical_plan_fingerprint_v1: [u8; 32],
    body_fingerprint_v1: [u8; 32],
    inference_abi_fingerprint_v1: [u8; 32],
}

impl OwnerDiagnosticReplayFactsBasis {
    fn matches_inputs(
        &self,
        syntax: &OwnerSyntaxInput,
        lexical_plan: &OwnerLexicalPlan,
        body: &OwnerBodyInferenceShard,
        abi: &OwnerInferenceAbiEnvironment,
    ) -> bool {
        self.owner == syntax.owner
            && self.owner == body.owner
            && self.syntax_fingerprint_v1 == syntax.fingerprint_v1()
            && self.lexical_plan_fingerprint_v1 == lexical_plan.fingerprint_v1()
            && self.body_fingerprint_v1 == body.fingerprint_v1()
            && self.inference_abi_fingerprint_v1 == abi.fingerprint_v1()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerDiagnosticReplayFactsCurrentness {
    basis: OwnerDiagnosticReplayFactsBasis,
    result_fingerprint_v1: [u8; 32],
    fingerprint_v1: [u8; 32],
}

impl OwnerDiagnosticReplayFactsCurrentness {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    fn matches_inputs(
        &self,
        syntax: &OwnerSyntaxInput,
        lexical_plan: &OwnerLexicalPlan,
        body: &OwnerBodyInferenceShard,
        abi: &OwnerInferenceAbiEnvironment,
    ) -> bool {
        self.basis.matches_inputs(syntax, lexical_plan, body, abi)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerDiagnosticReplayFactsEvaluation {
    pub currentness: OwnerDiagnosticReplayFactsCurrentness,
    pub result: Arc<OwnerDiagnosticReplayFacts>,
    body: Arc<OwnerBodyInferenceShard>,
}

impl OwnerDiagnosticReplayFactsEvaluation {
    pub fn matches_inputs(
        &self,
        syntax: &OwnerSyntaxInput,
        lexical_plan: &OwnerLexicalPlan,
        body: &OwnerBodyInferenceShard,
        abi: &OwnerInferenceAbiEnvironment,
    ) -> bool {
        self.currentness
            .matches_inputs(syntax, lexical_plan, body, abi)
            && self.currentness.result_fingerprint_v1 == self.result.fingerprint_v1()
    }

    fn matches_project_inputs(
        &self,
        syntax: &OwnerSyntaxInput,
        lexical_plan: &OwnerLexicalPlan,
        summary: &OwnerConstraintSummary,
        abi: &OwnerInferenceAbiEnvironment,
    ) -> bool {
        self.matches_inputs(syntax, lexical_plan, &self.body, abi)
            && summary.matches_signature_plan(&self.body.signature_lexical_plan)
    }
}

fn owner_diagnostic_stable_expression(
    syntax: &OwnerSyntaxInput,
    reference: &OwnerInferenceExpressionRef,
) -> Option<ProjectOrderExpressionFact> {
    match reference {
        OwnerInferenceExpressionRef::Local { expression } => syntax
            .expressions
            .get(expression.0 as usize)
            .map(|row| ProjectOrderExpressionFact {
                owner: syntax.owner.clone(),
                expression: row.stable_key.clone(),
            }),
        OwnerInferenceExpressionRef::External { owner, expression } => {
            Some(ProjectOrderExpressionFact {
                owner: owner.clone(),
                expression: expression.clone(),
            })
        }
    }
}

fn owner_diagnostic_stable_dense_expression(
    syntax: &OwnerSyntaxInput,
    reference: u32,
) -> Option<ProjectOrderExpressionFact> {
    let reference = reference as usize;
    if let Some(expression) = syntax.expressions.get(reference) {
        return Some(ProjectOrderExpressionFact {
            owner: syntax.owner.clone(),
            expression: expression.stable_key.clone(),
        });
    }
    let external = syntax
        .external_expressions
        .get(reference.checked_sub(syntax.expressions.len())?)?;
    Some(ProjectOrderExpressionFact {
        owner: external.owner.clone(),
        expression: external.expression.clone(),
    })
}

fn project_owner_diagnostic_replay_facts_with_lookup(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &OwnerLexicalPlan,
    body: &OwnerBodyInferenceShard,
    callable_kind: impl Fn(&str) -> Option<CheckedCallableKind>,
) -> Result<OwnerDiagnosticReplayFacts, ProjectDiagnosticFactsError> {
    if !lexical_plan.matches_input(syntax) {
        return Err(ProjectDiagnosticFactsError::new(format!(
            "owner diagnostic replay lexical plan does not match syntax for {:?}",
            syntax.owner
        )));
    }
    if body.owner != syntax.owner || !body.signature_lexical_plan.matches_base(lexical_plan) {
        return Err(ProjectDiagnosticFactsError::new(format!(
            "owner diagnostic replay body does not match the lexical plan for {:?}",
            syntax.owner
        )));
    }
    if body.statements.len() != syntax.statements.len()
        || body.expressions.len() != syntax.expressions.len()
    {
        return Err(ProjectDiagnosticFactsError::new(format!(
            "owner diagnostic replay body row coverage differs from syntax for {:?}",
            syntax.owner
        )));
    }
    for (index, (statement, input)) in body.statements.iter().zip(&syntax.statements).enumerate() {
        if statement.id.0 as usize != index
            || input.id as usize != index
            || statement.stable_key != input.stable_key
            || statement.kind != input.kind
            || statement.expression.map(|expression| expression.0) != input.expression
        {
            return Err(ProjectDiagnosticFactsError::new(format!(
                "owner diagnostic replay statement row differs from syntax for {:?}",
                syntax.owner
            )));
        }
    }
    let mut stable_expressions = BTreeMap::new();
    for (index, (expression, input)) in body.expressions.iter().zip(&syntax.expressions).enumerate()
    {
        if expression.id.0 as usize != index
            || expression.stable_key != input.stable_key
            || expression.kind != input.kind
        {
            return Err(ProjectDiagnosticFactsError::new(format!(
                "owner diagnostic replay expression row differs from syntax for {:?}",
                syntax.owner
            )));
        }
        if stable_expressions
            .insert(expression.stable_key.clone(), index)
            .is_some()
        {
            return Err(ProjectDiagnosticFactsError::new(format!(
                "owner diagnostic replay contains duplicate expression identities for {:?}",
                syntax.owner
            )));
        }
    }

    let mut output_declarations = Vec::new();
    if let Some(statement) = syntax.statements.first()
        && let AstStatementKind::Function { parameters, .. } = &statement.kind
    {
        for parameter in parameters
            .iter()
            .filter(|parameter| parameter.kind == AstParameterKind::Out)
        {
            let ordinal = u32::try_from(parameter.ordinal).map_err(|_| {
                ProjectDiagnosticFactsError::new("OUT parameter ordinal exceeds u32")
            })?;
            output_declarations.push(OwnerDiagnosticOutputDeclarationFact {
                target: ProjectOutputTargetFact::Parameter {
                    owner: syntax.owner.clone(),
                    ordinal,
                },
                name: parameter.name.clone(),
                statement: statement.stable_key.clone(),
                ordinal,
            });
        }
    }

    let stable_reference = |reference: &OwnerExpressionRef| match reference {
        OwnerExpressionRef::Local { expression } => syntax
            .expressions
            .get(expression.0 as usize)
            .map(|row| ProjectOrderExpressionFact {
                owner: syntax.owner.clone(),
                expression: row.stable_key.clone(),
            }),
        OwnerExpressionRef::Child { owner, expression } => Some(ProjectOrderExpressionFact {
            owner: owner.clone(),
            expression: expression.clone(),
        }),
    };
    let mut statement_values = Vec::new();
    for statement in &body.statements {
        let Some(value) = lexical_plan
            .graph()
            .statement(OwnerStatementId(statement.id.0))
            .and_then(|statement| statement.canonical_value.as_ref())
        else {
            continue;
        };
        let value = stable_reference(value).ok_or_else(|| {
            ProjectDiagnosticFactsError::new(
                "owner diagnostic replay statement value has no exact stable expression",
            )
        })?;
        statement_values.push(OwnerDiagnosticStableStatementValue {
            statement: statement.stable_key.clone(),
            value,
        });
    }

    let mut calls = Vec::with_capacity(body.calls.len());
    let mut output_calls = Vec::new();
    for call in &body.calls {
        let Some(expression_index) = stable_expressions.get(&call.expression).copied() else {
            return Err(ProjectDiagnosticFactsError::new(
                "owner diagnostic replay call has no exact owner expression",
            ));
        };
        let disposition = match &call.target {
            crate::InferredOwnerCallableTarget::Owner { owner } if call.valid => {
                crate::OwnerDiagnosticCallDisposition::User {
                    owner: owner.clone(),
                }
            }
            crate::InferredOwnerCallableTarget::Authoritative => {
                let kind = callable_kind(&call.function).ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "authoritative owner diagnostic replay call has no exact ABI contract",
                    )
                })?;
                crate::OwnerDiagnosticCallDisposition::Abi { kind }
            }
            crate::InferredOwnerCallableTarget::Owner { .. } => {
                crate::OwnerDiagnosticCallDisposition::Invalid
            }
            crate::InferredOwnerCallableTarget::Unresolved
            | crate::InferredOwnerCallableTarget::Ambiguous { .. }
                if !call.valid =>
            {
                crate::OwnerDiagnosticCallDisposition::Invalid
            }
            crate::InferredOwnerCallableTarget::Unresolved
            | crate::InferredOwnerCallableTarget::Ambiguous { .. } => {
                return Err(ProjectDiagnosticFactsError::new(
                    "valid owner diagnostic replay call has an unresolved or ambiguous target",
                ));
            }
        };
        let diagnostic = crate::OwnerDiagnosticCallFact {
            disposition,
            effect: call.effect,
            type_substitutions: call.type_substitutions.clone(),
        };
        let inputs = call
            .inputs
            .iter()
            .map(|input| {
                let stable_input = owner_diagnostic_stable_expression(syntax, &input.expression)
                    .ok_or_else(|| {
                        ProjectDiagnosticFactsError::new(
                            "owner diagnostic replay call input has no exact stable expression",
                        )
                    })?;
                Ok(OwnerDiagnosticStableCallActualFact {
                    input: stable_input,
                    actual_type: input.actual_type.clone(),
                })
            })
            .collect::<Result<Vec<_>, ProjectDiagnosticFactsError>>()?;
        let plan = if call.valid {
            let plan = body
                .signature_lexical_plan
                .call(expression_index)
                .ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "valid owner diagnostic replay call has no signature lexical row",
                    )
                })?;
            if !plan.valid
                || plan.stable_expression != call.expression
                || plan.function != call.function
            {
                return Err(ProjectDiagnosticFactsError::new(
                    "valid owner diagnostic replay call differs from its signature lexical row",
                ));
            }
            Some(plan)
        } else {
            None
        };
        let matched_inputs = plan
            .into_iter()
            .flat_map(|plan| plan.matched_inputs.iter())
            .map(|input| {
                let expression = owner_diagnostic_stable_dense_expression(syntax, input.expression)
                    .ok_or_else(|| {
                        ProjectDiagnosticFactsError::new(
                            "valid owner diagnostic replay matched input has no stable expression",
                        )
                    })?;
                Ok(OwnerDiagnosticStableMatchedInputFact {
                    formal_ordinal: input.formal_ordinal,
                    formal_name: input.formal_name.clone(),
                    formal_kind: input.formal_kind,
                    expression,
                    from_pipe: input.from_pipe,
                })
            })
            .collect::<Result<Vec<_>, ProjectDiagnosticFactsError>>()?;
        let explicit_pass = plan
            .and_then(|plan| plan.explicit_pass.as_ref())
            .map(|pass| {
                owner_diagnostic_stable_dense_expression(syntax, pass.expression).ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "valid owner diagnostic replay PASS has no stable expression",
                    )
                })
            })
            .transpose()?;
        calls.push(OwnerDiagnosticStableCallFact {
            expression: call.expression.clone(),
            function: call.function.clone(),
            target: call.target.clone(),
            inputs: inputs.into_boxed_slice(),
            valid: call.valid,
            result: call.result.clone(),
            diagnostic: diagnostic.clone(),
            matched_inputs: matched_inputs.into_boxed_slice(),
            explicit_pass,
        });

        if let Some(plan) = plan {
            let target = match &diagnostic.disposition {
                crate::OwnerDiagnosticCallDisposition::User { owner } => {
                    OwnerDiagnosticOutputCallTargetFact::Owner {
                        owner: owner.clone(),
                    }
                }
                crate::OwnerDiagnosticCallDisposition::Abi { kind } => {
                    OwnerDiagnosticOutputCallTargetFact::Abi {
                        function: call.function.clone(),
                        kind: *kind,
                    }
                }
                crate::OwnerDiagnosticCallDisposition::Invalid => {
                    return Err(ProjectDiagnosticFactsError::new(
                        "valid owner diagnostic replay call has an invalid disposition",
                    ));
                }
            };
            let outputs = plan
                .outputs
                .iter()
                .map(|output| {
                    let target = effective_output_target(&syntax.owner, &output.effective_target())
                        .ok_or_else(|| {
                            ProjectDiagnosticFactsError::new(
                                "valid owner diagnostic replay output has no stable target",
                            )
                        })?;
                    Ok(OwnerDiagnosticOutputBindingFact {
                        target,
                        formal_ordinal: output.formal_ordinal(),
                        fresh_name: match output {
                            OwnerSignatureOutputBindingPlan::Fresh { name, .. } => {
                                Some(name.clone())
                            }
                            OwnerSignatureOutputBindingPlan::Forward { .. } => None,
                        },
                        forwarding: matches!(
                            output,
                            OwnerSignatureOutputBindingPlan::Forward { .. }
                        ),
                    })
                })
                .collect::<Result<Vec<_>, ProjectDiagnosticFactsError>>()?;
            if outputs.is_empty() {
                continue;
            }
            let inputs = if matches!(&target, OwnerDiagnosticOutputCallTargetFact::Abi { .. }) {
                plan.matched_inputs
                    .iter()
                    .map(|input| {
                        let expression = owner_diagnostic_stable_dense_expression(
                            syntax,
                            input.expression,
                        )
                        .ok_or_else(|| {
                            ProjectDiagnosticFactsError::new(
                                "valid owner diagnostic replay call input has no stable expression",
                            )
                        })?;
                        Ok(OwnerDiagnosticOutputCallInputFact {
                            formal_ordinal: input.formal_ordinal,
                            expression,
                        })
                    })
                    .collect::<Result<Vec<_>, ProjectDiagnosticFactsError>>()?
            } else {
                Vec::new()
            };
            output_calls.push(OwnerDiagnosticOutputCallFact {
                expression: call.expression.clone(),
                target,
                inputs: inputs.into_boxed_slice(),
                outputs: outputs.into_boxed_slice(),
            });
        }
    }
    calls.sort_by(|left, right| left.expression.cmp(&right.expression));
    if calls
        .windows(2)
        .any(|rows| rows[0].expression == rows[1].expression)
    {
        return Err(ProjectDiagnosticFactsError::new(
            "owner diagnostic replay contains duplicate call expressions",
        ));
    }
    output_declarations.sort_by(|left, right| left.target.cmp(&right.target));
    if output_declarations
        .windows(2)
        .any(|rows| rows[0].target == rows[1].target)
    {
        return Err(ProjectDiagnosticFactsError::new(
            "owner diagnostic replay contains duplicate OUT declarations",
        ));
    }
    output_calls.sort_by(|left, right| left.expression.cmp(&right.expression));
    if output_calls
        .windows(2)
        .any(|rows| rows[0].expression == rows[1].expression)
    {
        return Err(ProjectDiagnosticFactsError::new(
            "owner diagnostic replay contains duplicate output call expressions",
        ));
    }

    if body.signature_lexical_plan.reads().len() != body.expressions.len() {
        return Err(ProjectDiagnosticFactsError::new(format!(
            "owner diagnostic replay lexical read coverage differs from expressions for {:?}",
            syntax.owner
        )));
    }

    let expression_flows = body
        .expressions
        .iter()
        .map(|expression| OwnerDiagnosticStableExpressionFlow {
            expression: expression.stable_key.clone(),
            flow_type: expression.flow_type.clone(),
            flush_type: expression.flush_type.clone(),
            direct_effect: expression.direct_effect,
        })
        .collect::<Vec<_>>();
    let mut reads = body
        .expressions
        .iter()
        .zip(body.signature_lexical_plan.reads())
        .filter_map(|(expression, read)| {
            read.as_ref().map(|read| OwnerDiagnosticStableReadFact {
                expression: expression.stable_key.clone(),
                read: read.clone(),
            })
        })
        .collect::<Vec<_>>();
    reads.sort_by(|left, right| left.expression.cmp(&right.expression));
    if reads
        .windows(2)
        .any(|rows| rows[0].expression == rows[1].expression)
    {
        return Err(ProjectDiagnosticFactsError::new(
            "owner diagnostic replay contains duplicate effective read expressions",
        ));
    }
    let function_name = body
        .statements
        .first()
        .and_then(|statement| match &statement.kind {
            AstStatementKind::Function { name, .. } => Some(name.clone()),
            _ => None,
        });
    let containing_scope = syntax.containing_scope.clone();
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        OWNER_DIAGNOSTIC_REPLAY_FACTS_DOMAIN_V4,
        &(
            &syntax.owner,
            &containing_scope,
            &function_name,
            &expression_flows,
            &statement_values,
            &calls,
            &reads,
            &output_declarations,
            &output_calls,
        ),
    )
    .map_err(|error| {
        ProjectDiagnosticFactsError::new(format!(
            "cannot fingerprint owner diagnostic replay facts: {error}"
        ))
    })?;
    Ok(OwnerDiagnosticReplayFacts {
        owner: syntax.owner.clone(),
        containing_scope,
        function_name,
        expression_flows: expression_flows.into_boxed_slice(),
        statement_values: statement_values.into_boxed_slice(),
        calls: calls.into_boxed_slice(),
        reads: reads.into_boxed_slice(),
        output_declarations: output_declarations.into_boxed_slice(),
        output_calls: output_calls.into_boxed_slice(),
        fingerprint_v1,
    })
}

pub fn project_owner_diagnostic_replay_facts(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &OwnerLexicalPlan,
    body: &OwnerBodyInferenceShard,
    abi: &OwnerInferenceAbiEnvironment,
) -> Result<OwnerDiagnosticReplayFacts, ProjectDiagnosticFactsError> {
    project_owner_diagnostic_replay_facts_with_lookup(syntax, lexical_plan, body, |name| {
        abi.callable(name).map(|contract| contract.kind)
    })
}

pub fn evaluate_owner_diagnostic_replay_facts(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &OwnerLexicalPlan,
    body: Arc<OwnerBodyInferenceShard>,
    abi: &OwnerInferenceAbiEnvironment,
) -> Result<OwnerDiagnosticReplayFactsEvaluation, ProjectDiagnosticFactsError> {
    let basis = OwnerDiagnosticReplayFactsBasis {
        owner: syntax.owner.clone(),
        syntax_fingerprint_v1: syntax.fingerprint_v1(),
        lexical_plan_fingerprint_v1: lexical_plan.fingerprint_v1(),
        body_fingerprint_v1: body.fingerprint_v1(),
        inference_abi_fingerprint_v1: abi.fingerprint_v1(),
    };
    let result = Arc::new(project_owner_diagnostic_replay_facts(
        syntax,
        lexical_plan,
        &body,
        abi,
    )?);
    let result_fingerprint_v1 = result.fingerprint_v1();
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        OWNER_DIAGNOSTIC_REPLAY_CURRENTNESS_DOMAIN_V4,
        &(&basis, result_fingerprint_v1),
    )
    .map_err(|error| {
        ProjectDiagnosticFactsError::new(format!(
            "cannot fingerprint owner diagnostic replay currentness: {error}"
        ))
    })?;
    Ok(OwnerDiagnosticReplayFactsEvaluation {
        currentness: OwnerDiagnosticReplayFactsCurrentness {
            basis,
            result_fingerprint_v1,
            fingerprint_v1,
        },
        result,
        body,
    })
}

struct OwnerFactView<'a> {
    syntax: &'a OwnerSyntaxInput,
    graph: &'a OwnerSyntaxGraph,
    interface: &'a crate::OwnerPublicInterface,
    replay: &'a OwnerDiagnosticReplayFacts,
    source_map: &'a OwnerSourceMap,
}

type StableOrderExpression = ProjectOrderExpressionFact;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectFieldPresence {
    No,
    Yes,
    Maybe,
}

enum ProjectDeferredStyleOrigin {
    Parameter {
        owner: StableCheckOwnerKey,
        ordinal: u32,
    },
    Expression(StableOrderExpression),
}

struct ProjectFactIndex<'a> {
    project: &'a ProjectSyntaxSnapshot,
    owners: BTreeMap<StableCheckOwnerKey, OwnerFactView<'a>>,
    statements: BTreeMap<StableStatementKey, (StableCheckOwnerKey, u32)>,
    expressions: BTreeMap<StableOrderExpression, u32>,
    syntax_expressions: BTreeMap<StableOrderExpression, usize>,
    statement_spans: BTreeMap<StableStatementKey, TypeDiagnosticSpan>,
    expression_spans: BTreeMap<StableOrderExpression, TypeDiagnosticSpan>,
    calls: BTreeMap<StableOrderExpression, &'a OwnerDiagnosticStableCallFact>,
    all_calls: BTreeMap<StableOrderExpression, &'a OwnerDiagnosticStableCallFact>,
    value_resolutions: BTreeMap<StableOrderExpression, &'a crate::OwnerSymbolResolution>,
    value_actuals_by_parameter: BTreeMap<(StableCheckOwnerKey, u32), Vec<StableOrderExpression>>,
    callable_owner_by_owner: BTreeMap<StableCheckOwnerKey, Option<StableCheckOwnerKey>>,
}

impl<'a> ProjectFactIndex<'a> {
    fn new(
        project: &'a ProjectSyntaxSnapshot,
        expected_owners: impl IntoIterator<Item = &'a StableCheckOwnerKey>,
        syntax_inputs: impl IntoIterator<Item = &'a OwnerSyntaxInput>,
        lexical_plans: impl IntoIterator<Item = &'a OwnerLexicalPlan>,
        summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
        interfaces: impl IntoIterator<Item = &'a crate::OwnerPublicInterface>,
        replay_evaluations: impl IntoIterator<Item = &'a OwnerDiagnosticReplayFactsEvaluation>,
        replay_facts: impl IntoIterator<Item = &'a OwnerDiagnosticReplayFacts>,
        inference_abis: impl IntoIterator<
            Item = (&'a StableCheckOwnerKey, &'a OwnerInferenceAbiEnvironment),
        >,
        source_maps: impl IntoIterator<Item = &'a OwnerSourceMap>,
    ) -> Result<Self, ProjectDiagnosticFactsError> {
        let expected = expected_owners
            .into_iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut syntax_statements = BTreeMap::new();
        for slot in 0..project.statement_count() {
            let statement = project.statement_id_for_slot(slot).ok_or_else(|| {
                ProjectDiagnosticFactsError::new("project statement slot has no syntax identity")
            })?;
            let Some(stable) = project.stable_statement_key(statement) else {
                continue;
            };
            if syntax_statements.insert(stable, statement).is_some() {
                return Err(ProjectDiagnosticFactsError::new(
                    "project diagnostic facts received duplicate syntax statement identity",
                ));
            }
        }
        let mut syntax_expressions = BTreeMap::new();
        for slot in 0..project.expression_count() {
            let expression = project.expression_id_for_slot(slot).ok_or_else(|| {
                ProjectDiagnosticFactsError::new("project expression slot has no syntax identity")
            })?;
            let Some(owner) = project.stable_check_owner_for_expression(expression) else {
                continue;
            };
            let Some(stable) = project.stable_expression_key(expression) else {
                continue;
            };
            if syntax_expressions
                .insert(
                    StableOrderExpression {
                        owner,
                        expression: stable,
                    },
                    expression,
                )
                .is_some()
            {
                return Err(ProjectDiagnosticFactsError::new(
                    "project diagnostic facts received duplicate syntax expression identity",
                ));
            }
        }
        let syntax_inputs = unique_by_owner(
            syntax_inputs
                .into_iter()
                .map(|syntax| (&syntax.owner, syntax)),
            "syntax input",
        )?;
        let replay_facts = unique_by_owner(
            replay_facts.into_iter().map(|facts| (facts.owner(), facts)),
            "diagnostic replay facts",
        )?;
        let replay_evaluations = unique_by_owner(
            replay_evaluations
                .into_iter()
                .map(|evaluation| (evaluation.result.owner(), evaluation)),
            "diagnostic replay evaluation",
        )?;
        let inference_abis = unique_by_owner(inference_abis, "diagnostic inference ABI")?;
        let interfaces = unique_by_owner(
            interfaces
                .into_iter()
                .map(|interface| (&interface.owner, interface)),
            "public interface",
        )?;
        let summaries = unique_by_owner(
            summaries
                .into_iter()
                .map(|summary| (&summary.owner, summary)),
            "constraint summary",
        )?;
        let lexical_plans = unique_by_owner(
            lexical_plans.into_iter().map(|plan| (plan.owner(), plan)),
            "lexical plan",
        )?;
        let source_maps = unique_by_owner(
            source_maps
                .into_iter()
                .map(|source_map| (source_map.owner(), source_map)),
            "source map",
        )?;
        for (label, actual) in [
            (
                "syntax input",
                syntax_inputs.keys().cloned().collect::<BTreeSet<_>>(),
            ),
            (
                "public interface",
                interfaces.keys().cloned().collect::<BTreeSet<_>>(),
            ),
            (
                "diagnostic replay facts",
                replay_facts.keys().cloned().collect::<BTreeSet<_>>(),
            ),
            (
                "diagnostic replay evaluation",
                replay_evaluations.keys().cloned().collect::<BTreeSet<_>>(),
            ),
            (
                "diagnostic inference ABI",
                inference_abis.keys().cloned().collect::<BTreeSet<_>>(),
            ),
            (
                "constraint summary",
                summaries.keys().cloned().collect::<BTreeSet<_>>(),
            ),
            (
                "lexical plan",
                lexical_plans.keys().cloned().collect::<BTreeSet<_>>(),
            ),
            (
                "source map",
                source_maps.keys().cloned().collect::<BTreeSet<_>>(),
            ),
        ] {
            if actual != expected {
                return Err(ProjectDiagnosticFactsError::new(format!(
                    "project diagnostic facts {label} coverage differs from the project owner set"
                )));
            }
        }

        let mut owners = BTreeMap::new();
        let mut statements = BTreeMap::new();
        let mut expressions = BTreeMap::new();
        let source_layouts = project
            .source_layouts()
            .iter()
            .map(|layout| {
                (
                    &layout.source_unit_id,
                    (layout.start_line, layout.start_byte),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if source_layouts.len() != project.source_layouts().len() {
            return Err(ProjectDiagnosticFactsError::new(
                "project diagnostic facts received duplicate source-unit layouts",
            ));
        }
        let mut statement_spans = BTreeMap::new();
        let mut expression_spans = BTreeMap::new();
        let mut calls = BTreeMap::new();
        let mut all_calls = BTreeMap::new();
        let mut value_resolutions = BTreeMap::new();
        for owner in &expected {
            let syntax = syntax_inputs[owner];
            let lexical_plan = lexical_plans[owner];
            let summary = summaries[owner];
            let interface = interfaces[owner];
            let replay_evaluation = replay_evaluations[owner];
            let replay = replay_facts[owner];
            let inference_abi = inference_abis[owner];
            let source_map = source_maps[owner];
            let (layout_start_line, layout_start_byte) = source_layouts
                .get(owner.source_unit_id())
                .copied()
                .ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "project diagnostic owner has no source-unit layout",
                    )
                })?;
            if !lexical_plan.matches_input(syntax) {
                return Err(ProjectDiagnosticFactsError::new(format!(
                    "project diagnostic lexical plan does not match syntax for {owner:?}"
                )));
            }
            if replay.owner != *owner || replay.containing_scope != syntax.containing_scope {
                return Err(ProjectDiagnosticFactsError::new(format!(
                    "project diagnostic replay facts do not match owner inputs for {owner:?}"
                )));
            }
            if !replay_evaluation.matches_project_inputs(
                syntax,
                lexical_plan,
                summary,
                inference_abi,
            ) || replay_evaluation.currentness.result_fingerprint_v1 != replay.fingerprint_v1()
            {
                return Err(ProjectDiagnosticFactsError::new(format!(
                    "project diagnostic replay currentness does not match owner inputs for {owner:?}"
                )));
            }
            if replay.expression_flows.len() != syntax.expressions.len() {
                return Err(ProjectDiagnosticFactsError::new(format!(
                    "project diagnostic replay expression coverage differs from syntax for {owner:?}"
                )));
            }
            for (index, input) in syntax.statements.iter().enumerate() {
                if input.id as usize != index {
                    return Err(ProjectDiagnosticFactsError::new(format!(
                        "project diagnostic syntax statement index is not dense for {owner:?}"
                    )));
                }
            }
            for (flow, input) in replay.expression_flows.iter().zip(&syntax.expressions) {
                if flow.expression != input.stable_key {
                    return Err(ProjectDiagnosticFactsError::new(format!(
                        "project diagnostic replay expression row differs from syntax for {owner:?}"
                    )));
                }
            }
            let graph = lexical_plan.graph();
            for statement in &syntax.statements {
                if statements
                    .insert(statement.stable_key.clone(), (owner.clone(), statement.id))
                    .is_some()
                {
                    return Err(ProjectDiagnosticFactsError::new(
                        "project diagnostic facts received duplicate stable statement",
                    ));
                }
            }
            for (index, expression) in syntax.expressions.iter().enumerate() {
                if expressions
                    .insert(
                        StableOrderExpression {
                            owner: owner.clone(),
                            expression: expression.stable_key.clone(),
                        },
                        u32::try_from(index).map_err(|_| {
                            ProjectDiagnosticFactsError::new(
                                "project diagnostic owner expression count exceeds u32",
                            )
                        })?,
                    )
                    .is_some()
                {
                    return Err(ProjectDiagnosticFactsError::new(
                        "project diagnostic facts received duplicate stable expression",
                    ));
                }
            }
            for source in source_map.statements() {
                let span = global_local_span_from_offsets(
                    layout_start_line,
                    layout_start_byte,
                    source.line,
                    source.start,
                    source.end,
                )?;
                if statement_spans
                    .insert(source.stable_key.clone(), span)
                    .is_some()
                {
                    return Err(ProjectDiagnosticFactsError::new(
                        "project diagnostic facts received duplicate statement source span",
                    ));
                }
            }
            for source in source_map.expressions() {
                let stable = StableOrderExpression {
                    owner: owner.clone(),
                    expression: source.expression.clone(),
                };
                let span = global_local_span_from_offsets(
                    layout_start_line,
                    layout_start_byte,
                    source.line,
                    source.start,
                    source.end,
                )?;
                if expression_spans.insert(stable, span).is_some() {
                    return Err(ProjectDiagnosticFactsError::new(
                        "project diagnostic facts received duplicate expression source span",
                    ));
                }
            }
            for call in &replay.calls {
                let expression = StableOrderExpression {
                    owner: owner.clone(),
                    expression: call.expression.clone(),
                };
                if all_calls.insert(expression.clone(), call).is_some() {
                    return Err(ProjectDiagnosticFactsError::new(
                        "project diagnostic facts received duplicate call expression",
                    ));
                }
                if call.valid {
                    calls.insert(expression, call);
                }
            }
            for resolution in &summary.symbol_resolutions {
                let reference = resolution.reference();
                if reference.kind != crate::OwnerReferenceKind::Value {
                    continue;
                }
                let expression = StableOrderExpression {
                    owner: owner.clone(),
                    expression: reference.expression.clone(),
                };
                if value_resolutions.insert(expression, resolution).is_some() {
                    return Err(ProjectDiagnosticFactsError::new(
                        "project diagnostic facts received duplicate value resolution",
                    ));
                }
            }
            owners.insert(
                owner.clone(),
                OwnerFactView {
                    syntax,
                    graph,
                    interface,
                    replay,
                    source_map,
                },
            );
        }
        let mut result = Self {
            project,
            owners,
            statements,
            expressions,
            syntax_expressions,
            statement_spans,
            expression_spans,
            calls,
            all_calls,
            value_resolutions,
            value_actuals_by_parameter: BTreeMap::new(),
            callable_owner_by_owner: BTreeMap::new(),
        };
        result.index_callable_owners()?;
        result.index_parameter_actuals()?;
        Ok(result)
    }

    fn index_callable_owners(&mut self) -> Result<(), ProjectDiagnosticFactsError> {
        fn resolve(
            owner: &StableCheckOwnerKey,
            owners: &BTreeMap<StableCheckOwnerKey, OwnerFactView<'_>>,
            statements: &BTreeMap<StableStatementKey, (StableCheckOwnerKey, u32)>,
            resolved: &mut BTreeMap<StableCheckOwnerKey, Option<StableCheckOwnerKey>>,
            active: &mut BTreeSet<StableCheckOwnerKey>,
        ) -> Result<Option<StableCheckOwnerKey>, ProjectDiagnosticFactsError> {
            if let Some(callable) = resolved.get(owner) {
                return Ok(callable.clone());
            }
            if !active.insert(owner.clone()) {
                return Err(ProjectDiagnosticFactsError::new(
                    "owner diagnostic containment contains a cycle",
                ));
            }
            let view = owners.get(owner).ok_or_else(|| {
                ProjectDiagnosticFactsError::new(
                    "owner diagnostic containment references a missing owner",
                )
            })?;
            let callable = if view.replay.function_name.is_some() {
                Some(owner.clone())
            } else {
                match &view.replay.containing_scope {
                    OwnerContainingScopeInput::ProjectRoot => None,
                    OwnerContainingScopeInput::OwnerStatement {
                        owner: parent,
                        statement,
                    } => {
                        if !statements
                            .get(statement)
                            .is_some_and(|(owner, _)| owner == parent)
                        {
                            return Err(ProjectDiagnosticFactsError::new(
                                "owner diagnostic containment references a foreign statement",
                            ));
                        }
                        resolve(parent, owners, statements, resolved, active)?
                    }
                }
            };
            active.remove(owner);
            resolved.insert(owner.clone(), callable.clone());
            Ok(callable)
        }

        let mut resolved = BTreeMap::new();
        for owner in self.owners.keys() {
            resolve(
                owner,
                &self.owners,
                &self.statements,
                &mut resolved,
                &mut BTreeSet::new(),
            )?;
        }
        self.callable_owner_by_owner = resolved;
        Ok(())
    }

    fn callable_owner(&self, owner: &StableCheckOwnerKey) -> Option<&StableCheckOwnerKey> {
        self.callable_owner_by_owner.get(owner)?.as_ref()
    }

    fn index_parameter_actuals(&mut self) -> Result<(), ProjectDiagnosticFactsError> {
        let mut values = BTreeMap::<(StableCheckOwnerKey, u32), Vec<StableOrderExpression>>::new();
        for call in self.calls.values() {
            let crate::InferredOwnerCallableTarget::Owner { owner: callee } = &call.target else {
                continue;
            };
            if !call.valid {
                return Err(ProjectDiagnosticFactsError::new(
                    "valid owner call index contains an invalid replay row",
                ));
            }
            for input in call
                .matched_inputs
                .iter()
                .filter(|input| input.formal_kind == crate::OwnerParameterKind::Value)
            {
                values
                    .entry((callee.clone(), input.formal_ordinal))
                    .or_default()
                    .push(input.expression.clone());
            }
        }
        self.value_actuals_by_parameter = values;
        Ok(())
    }

    fn statement_view(&self, statement: &StableStatementKey) -> Option<(&OwnerFactView<'a>, u32)> {
        let (owner, statement) = self.statements.get(statement)?;
        Some((self.owners.get(owner)?, *statement))
    }

    fn expression(
        &self,
        owner: &StableCheckOwnerKey,
        expression: &OwnerExpressionRef,
    ) -> Option<(&OwnerDiagnosticStableExpressionFlow, &OwnerSourceMap)> {
        match expression {
            OwnerExpressionRef::Local { expression } => {
                let view = self.owners.get(owner)?;
                let value = view.replay.expression_flows.get(expression.0 as usize)?;
                let syntax = view.syntax.expressions.get(expression.0 as usize)?;
                (value.expression == syntax.stable_key).then_some((value, view.source_map))
            }
            OwnerExpressionRef::Child { owner, expression } => {
                let view = self.owners.get(owner)?;
                let stable = StableOrderExpression {
                    owner: owner.clone(),
                    expression: expression.clone(),
                };
                let index = *self.expressions.get(&stable)? as usize;
                let value = view.replay.expression_flows.get(index)?;
                (value.expression == *expression).then_some((value, view.source_map))
            }
        }
    }

    fn stable_expression_ref(
        &self,
        owner: &StableCheckOwnerKey,
        reference: u32,
    ) -> Option<StableOrderExpression> {
        let view = self.owners.get(owner)?;
        let reference = reference as usize;
        if let Some(expression) = view.syntax.expressions.get(reference) {
            return Some(StableOrderExpression {
                owner: owner.clone(),
                expression: expression.stable_key.clone(),
            });
        }
        let external = view
            .syntax
            .external_expressions
            .get(reference.checked_sub(view.syntax.expressions.len())?)?;
        Some(StableOrderExpression {
            owner: external.owner.clone(),
            expression: external.expression.clone(),
        })
    }

    fn order_expression(
        &self,
        expression: &StableOrderExpression,
    ) -> Option<(
        &OwnerFactView<'a>,
        usize,
        &crate::OwnerExpressionInput,
        &OwnerDiagnosticStableExpressionFlow,
    )> {
        let view = self.owners.get(&expression.owner)?;
        let index = *self.expressions.get(expression)? as usize;
        let syntax = view.syntax.expressions.get(index)?;
        let inferred = view.replay.expression_flows.get(index)?;
        (syntax.stable_key == expression.expression && inferred.expression == expression.expression)
            .then_some((view, index, syntax, inferred))
    }

    fn expression_span(
        &self,
        expression: &StableOrderExpression,
    ) -> Result<TypeDiagnosticSpan, ProjectDiagnosticFactsError> {
        self.expression_spans
            .get(expression)
            .copied()
            .ok_or_else(|| {
                ProjectDiagnosticFactsError::new(
                    "project diagnostic expression has no exact indexed source span",
                )
            })
    }

    fn statement_span(
        &self,
        statement: &StableStatementKey,
    ) -> Result<TypeDiagnosticSpan, ProjectDiagnosticFactsError> {
        self.statement_spans.get(statement).copied().ok_or_else(|| {
            ProjectDiagnosticFactsError::new(
                "project diagnostic statement has no exact indexed source span",
            )
        })
    }

    fn call(&self, expression: &StableOrderExpression) -> Option<&OwnerDiagnosticStableCallFact> {
        self.calls.get(expression).copied()
    }

    fn deferred_style_stable_origin(
        &self,
        target: &OwnerLexicalTargetRef,
    ) -> Option<ProjectDeferredStyleOrigin> {
        let OwnerLexicalTargetRef::Declaration {
            owner, declaration, ..
        } = target
        else {
            return None;
        };
        match declaration {
            OwnerDeclarationStableKey::Parameter { ordinal } => {
                Some(ProjectDeferredStyleOrigin::Parameter {
                    owner: owner.clone(),
                    ordinal: *ordinal,
                })
            }
            OwnerDeclarationStableKey::Public => self
                .public_result(owner)
                .map(ProjectDeferredStyleOrigin::Expression),
            OwnerDeclarationStableKey::Statement { statement } => self
                .stable_statement_value_ref(statement)
                .map(ProjectDeferredStyleOrigin::Expression),
            OwnerDeclarationStableKey::RecordField {
                object, ordinal, ..
            } => self
                .record_field_value(owner, object, *ordinal)
                .map(ProjectDeferredStyleOrigin::Expression),
            OwnerDeclarationStableKey::PatternBinding { .. }
            | OwnerDeclarationStableKey::FreshOut { .. }
            | OwnerDeclarationStableKey::CallContext { .. } => None,
        }
    }

    fn deferred_style_local_origin(
        &self,
        owner: &StableCheckOwnerKey,
        target: &OwnerLexicalDeclarationTarget,
    ) -> Option<ProjectDeferredStyleOrigin> {
        match target {
            OwnerLexicalDeclarationTarget::Parameter { ordinal } => {
                Some(ProjectDeferredStyleOrigin::Parameter {
                    owner: owner.clone(),
                    ordinal: *ordinal,
                })
            }
            OwnerLexicalDeclarationTarget::Statement { statement } => self
                .statement_value_ref(owner, *statement)
                .map(ProjectDeferredStyleOrigin::Expression),
            OwnerLexicalDeclarationTarget::RecordField {
                object, ordinal, ..
            } => {
                let object = self
                    .owners
                    .get(owner)?
                    .syntax
                    .expressions
                    .get(*object as usize)?
                    .stable_key
                    .clone();
                self.record_field_value(owner, &object, *ordinal)
                    .map(ProjectDeferredStyleOrigin::Expression)
            }
            OwnerLexicalDeclarationTarget::Imported { target } => {
                self.deferred_style_stable_origin(target)
            }
            OwnerLexicalDeclarationTarget::PatternBinding { .. }
            | OwnerLexicalDeclarationTarget::Passed
            | OwnerLexicalDeclarationTarget::Ambiguous { .. } => None,
        }
    }

    fn deferred_style_parameter_origin(
        &self,
        expression: &StableOrderExpression,
        suffix: &[String],
        active: &mut BTreeSet<StableOrderExpression>,
    ) -> Option<(StableCheckOwnerKey, u32, Vec<String>)> {
        if !active.insert(expression.clone()) {
            return None;
        }
        let result = if let Some(read) = self.effective_read(expression) {
            let projection = read
                .projection
                .iter()
                .chain(suffix)
                .cloned()
                .collect::<Vec<_>>();
            let origin = match &read.target {
                OwnerEffectiveLexicalTarget::Static { target } => {
                    self.deferred_style_local_origin(&expression.owner, target)
                }
                OwnerEffectiveLexicalTarget::Imported { target } => {
                    self.deferred_style_stable_origin(target)
                }
                OwnerEffectiveLexicalTarget::FreshOut { .. }
                | OwnerEffectiveLexicalTarget::CallContext { .. }
                | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
            };
            match origin {
                Some(ProjectDeferredStyleOrigin::Parameter { owner, ordinal }) => {
                    Some((owner, ordinal, projection))
                }
                Some(ProjectDeferredStyleOrigin::Expression(value)) => {
                    self.deferred_style_parameter_origin(&value, &projection, active)
                }
                None => None,
            }
        } else if let Some(call) = self.call(expression)
            && let crate::InferredOwnerCallableTarget::Owner { owner: callee } = &call.target
        {
            let (origin_owner, ordinal, projection) = self
                .public_result(callee)
                .and_then(|result| self.deferred_style_parameter_origin(&result, suffix, active))?;
            if &origin_owner != callee {
                None
            } else {
                let actual = call
                    .matched_inputs
                    .iter()
                    .find(|input| {
                        input.formal_kind == crate::OwnerParameterKind::Value
                            && input.formal_ordinal == ordinal
                    })
                    .map(|input| input.expression.clone())?;
                self.deferred_style_parameter_origin(&actual, &projection, active)
            }
        } else if let Some(crate::OwnerSymbolResolution::Resolved {
            owner, projection, ..
        }) = self.value_resolutions.get(expression).copied()
        {
            let projection = projection.iter().chain(suffix).cloned().collect::<Vec<_>>();
            self.public_result(owner)
                .and_then(|value| self.deferred_style_parameter_origin(&value, &projection, active))
        } else {
            self.order_expression(expression)
                .and_then(|(_, _, syntax, _)| match &syntax.kind {
                    AstExprKind::Block {
                        result: Some(result),
                        ..
                    }
                    | AstExprKind::MatchArm {
                        output: Some(result),
                        ..
                    } => self.stable_expression_ref(&expression.owner, *result as u32),
                    _ => None,
                })
                .and_then(|value| self.deferred_style_parameter_origin(&value, suffix, active))
        };
        active.remove(expression);
        result
    }

    fn deferred_style_parameter_type(
        &self,
        owner: &StableCheckOwnerKey,
        ordinal: u32,
        projection: &[String],
    ) -> Result<Type, ProjectDiagnosticFactsError> {
        let Some(interface) = self.owners.get(owner).map(|view| view.interface) else {
            return Err(ProjectDiagnosticFactsError::new(
                "owner diagnostic parameter read has no public interface",
            ));
        };
        let Some(parameter) = interface
            .parameters
            .iter()
            .find(|parameter| parameter.ordinal == ordinal)
        else {
            return Err(ProjectDiagnosticFactsError::new(
                "owner diagnostic parameter read has no public interface parameter",
            ));
        };
        let ty = type_for_nested_path(&parameter.flow_type.ty, projection)
            .unwrap_or_else(|| parameter.flow_type.ty.clone());
        // Body call facts publish substitutions in the callee-local
        // alpha namespace, while an interface SCC uses one namespace for
        // all members. Normalize this member's declared variables to the
        // same dense call-local order before replaying substitutions.
        let call_local_variables = interface
            .type_variables
            .iter()
            .enumerate()
            .map(|(index, variable)| {
                Ok((
                    *variable,
                    Type::Var(boon_checked::TypeVar(u32::try_from(index).map_err(
                        |_| {
                            ProjectDiagnosticFactsError::new(
                                "owner diagnostic interface has too many type variables",
                            )
                        },
                    )?)),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, ProjectDiagnosticFactsError>>()?;
        Ok(substitute_checked_type(&ty, &call_local_variables))
    }

    fn effective_read(
        &self,
        expression: &StableOrderExpression,
    ) -> Option<&crate::OwnerEffectiveLexicalReadPlan> {
        let replay = self.owners.get(&expression.owner)?.replay;
        replay
            .reads
            .binary_search_by(|row| row.expression.cmp(&expression.expression))
            .ok()
            .and_then(|index| replay.reads.get(index))
            .map(|row| &row.read)
    }

    fn expression_is_parameter_read(&self, expression: &StableOrderExpression) -> bool {
        let Some(read) = self.effective_read(expression) else {
            return false;
        };
        if read.access != OwnerLexicalAccess::Read
            || !self
                .order_expression(expression)
                .is_some_and(|(_, _, syntax, _)| {
                    matches!(
                        syntax.kind,
                        AstExprKind::Identifier(_) | AstExprKind::Path(_)
                    )
                })
        {
            return false;
        }
        match &read.target {
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
            } => self
                .owners
                .get(&expression.owner)
                .and_then(|view| view.syntax.statements.first())
                .and_then(|statement| match &statement.kind {
                    AstStatementKind::Function { parameters, .. } => parameters
                        .iter()
                        .find(|parameter| parameter.ordinal == *ordinal as usize),
                    _ => None,
                })
                .is_some_and(|parameter| parameter.kind == AstParameterKind::Value),
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Imported { target },
            }
            | OwnerEffectiveLexicalTarget::Imported { target } => {
                matches!(
                    target,
                    OwnerLexicalTargetRef::Declaration {
                        declaration: OwnerDeclarationStableKey::Parameter { .. },
                        capability: OwnerLexicalDeclarationCapability::Value,
                        ..
                    }
                )
            }
            _ => false,
        }
    }

    fn record_field_value(
        &self,
        owner: &StableCheckOwnerKey,
        object: &StableExpressionKey,
        ordinal: u32,
    ) -> Option<StableOrderExpression> {
        let object = StableOrderExpression {
            owner: owner.clone(),
            expression: object.clone(),
        };
        let (_, _, syntax, _) = self.order_expression(&object)?;
        let fields = match &syntax.kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => fields,
            _ => return None,
        };
        let field = fields.get(ordinal as usize)?;
        self.stable_expression_ref(owner, u32::try_from(field.value).ok()?)
    }

    fn declaration_is_stateful(
        &self,
        owner: &StableCheckOwnerKey,
        declaration: &OwnerDeclarationStableKey,
        exact_modes: &BTreeMap<StableOrderExpression, FlowMode>,
        active: &mut BTreeSet<StableOrderExpression>,
    ) -> bool {
        let statement = match declaration {
            OwnerDeclarationStableKey::Public => self
                .owners
                .get(owner)
                .and_then(|view| view.syntax.statements.first())
                .map(|statement| statement.id),
            OwnerDeclarationStableKey::Statement { statement } => self
                .statements
                .get(statement)
                .and_then(|(provider, statement)| (provider == owner).then_some(*statement)),
            OwnerDeclarationStableKey::RecordField {
                object, ordinal, ..
            } => {
                return self
                    .record_field_value(owner, object, *ordinal)
                    .is_some_and(|value| self.expression_is_stateful(&value, exact_modes, active));
            }
            OwnerDeclarationStableKey::Parameter { .. }
            | OwnerDeclarationStableKey::PatternBinding { .. }
            | OwnerDeclarationStableKey::FreshOut { .. }
            | OwnerDeclarationStableKey::CallContext { .. } => None,
        };
        let Some(statement) = statement else {
            return false;
        };
        let Some(view) = self.owners.get(owner) else {
            return false;
        };
        if matches!(
            view.syntax
                .statements
                .get(statement as usize)
                .map(|row| &row.kind),
            Some(AstStatementKind::Hold { .. })
        ) {
            return true;
        }
        self.statement_value_ref(owner, statement)
            .is_some_and(|value| self.expression_is_stateful(&value, exact_modes, active))
    }

    fn expression_is_stateful(
        &self,
        expression: &StableOrderExpression,
        exact_modes: &BTreeMap<StableOrderExpression, FlowMode>,
        active: &mut BTreeSet<StableOrderExpression>,
    ) -> bool {
        if !active.insert(expression.clone()) {
            return false;
        }
        let result =
            self.order_expression(expression)
                .is_some_and(|(view, _, syntax, inferred)| {
                    if matches!(syntax.kind, AstExprKind::Hold { .. }) {
                        return true;
                    }
                    if let AstExprKind::Latest { branches } = &syntax.kind {
                        return branches.first().is_some_and(|branch| {
                            self.stable_expression_ref(&expression.owner, *branch as u32)
                                .is_some_and(|branch| {
                                    exact_modes.get(&branch).copied().or_else(|| {
                                        self.order_expression(&branch)
                                            .map(|(_, _, _, branch)| branch.flow_type.mode)
                                    }) == Some(FlowMode::Continuous)
                                })
                        });
                    }
                    if inferred.direct_effect.writes_state {
                        return true;
                    }
                    let Some(read) = self.effective_read(expression) else {
                        return false;
                    };
                    if read.access != OwnerLexicalAccess::Read
                        || !matches!(
                            syntax.kind,
                            AstExprKind::Identifier(_) | AstExprKind::Path(_)
                        )
                    {
                        return false;
                    }
                    match &read.target {
                        OwnerEffectiveLexicalTarget::Static {
                            target: OwnerLexicalDeclarationTarget::Statement { statement },
                        } => {
                            let declaration =
                                view.syntax
                                    .statements
                                    .get(*statement as usize)
                                    .map(|statement| OwnerDeclarationStableKey::Statement {
                                        statement: statement.stable_key.clone(),
                                    });
                            declaration.is_some_and(|declaration| {
                                self.declaration_is_stateful(
                                    &expression.owner,
                                    &declaration,
                                    exact_modes,
                                    active,
                                )
                            })
                        }
                        OwnerEffectiveLexicalTarget::Static {
                            target:
                                OwnerLexicalDeclarationTarget::RecordField {
                                    object, ordinal, ..
                                },
                        } => view
                            .syntax
                            .expressions
                            .get(*object as usize)
                            .and_then(|object| {
                                self.record_field_value(
                                    &expression.owner,
                                    &object.stable_key,
                                    *ordinal,
                                )
                            })
                            .is_some_and(|value| {
                                self.expression_is_stateful(&value, exact_modes, active)
                            }),
                        OwnerEffectiveLexicalTarget::Static {
                            target: OwnerLexicalDeclarationTarget::Imported { target },
                        }
                        | OwnerEffectiveLexicalTarget::Imported { target } => match target {
                            OwnerLexicalTargetRef::Declaration {
                                owner, declaration, ..
                            } => self.declaration_is_stateful(
                                owner,
                                declaration,
                                exact_modes,
                                active,
                            ),
                            OwnerLexicalTargetRef::ContextFormal { .. }
                            | OwnerLexicalTargetRef::Ambiguous { .. } => false,
                        },
                        _ => false,
                    }
                });
        active.remove(expression);
        result
    }

    fn public_result(&self, owner: &StableCheckOwnerKey) -> Option<StableOrderExpression> {
        let view = self.owners.get(owner)?;
        let root = view.syntax.statements.first()?;
        let value = view
            .graph
            .statement(OwnerStatementId(root.id))?
            .canonical_value
            .as_ref()?;
        match value {
            OwnerExpressionRef::Local { expression } => {
                self.stable_expression_ref(owner, expression.0)
            }
            OwnerExpressionRef::Child { owner, expression } => Some(StableOrderExpression {
                owner: owner.clone(),
                expression: expression.clone(),
            }),
        }
    }

    fn statement_value_ref(
        &self,
        owner: &StableCheckOwnerKey,
        statement: u32,
    ) -> Option<StableOrderExpression> {
        let view = self.owners.get(owner)?;
        let value = view
            .graph
            .statement(OwnerStatementId(statement))?
            .canonical_value
            .as_ref()?;
        match value {
            OwnerExpressionRef::Local { expression } => {
                self.stable_expression_ref(owner, expression.0)
            }
            OwnerExpressionRef::Child { owner, expression } => Some(StableOrderExpression {
                owner: owner.clone(),
                expression: expression.clone(),
            }),
        }
    }

    fn stable_statement_value_ref(
        &self,
        statement: &StableStatementKey,
    ) -> Option<StableOrderExpression> {
        let (view, statement) = self.statement_view(statement)?;
        self.statement_value_ref(&view.syntax.owner, statement)
    }

    fn statement_value(
        &self,
        statement: &StableStatementKey,
    ) -> Result<
        Option<(
            StableExpressionKey,
            FlowType,
            Option<Type>,
            TypeDiagnosticSpan,
        )>,
        ProjectDiagnosticFactsError,
    > {
        let (view, statement_id) = self.statement_view(statement).ok_or_else(|| {
            ProjectDiagnosticFactsError::new("project diagnostic statement has no owner body")
        })?;
        let value = view
            .graph
            .statement(OwnerStatementId(statement_id))
            .and_then(|statement| statement.canonical_value.as_ref());
        let Some(value) = value else {
            return Ok(None);
        };
        let (expression, source_map) =
            self.expression(&view.syntax.owner, value).ok_or_else(|| {
                ProjectDiagnosticFactsError::new(
                    "project diagnostic statement value has no inferred expression",
                )
            })?;
        let span = self.expression_span(&StableOrderExpression {
            owner: source_map.owner().clone(),
            expression: expression.expression.clone(),
        })?;
        Ok(Some((
            expression.expression.clone(),
            expression.flow_type.clone(),
            expression.flush_type.clone(),
            span,
        )))
    }
}

fn unique_by_owner<'a, T>(
    rows: impl IntoIterator<Item = (&'a StableCheckOwnerKey, &'a T)>,
    label: &str,
) -> Result<BTreeMap<StableCheckOwnerKey, &'a T>, ProjectDiagnosticFactsError> {
    let mut result = BTreeMap::new();
    for (owner, row) in rows {
        if result.insert(owner.clone(), row).is_some() {
            return Err(ProjectDiagnosticFactsError::new(format!(
                "project diagnostic facts received duplicate {label} for {owner:?}"
            )));
        }
    }
    Ok(result)
}

fn syntax_expression_span(
    index: &ProjectFactIndex<'_>,
    expression: &boon_syntax::AstExpr,
) -> Result<TypeDiagnosticSpan, ProjectDiagnosticFactsError> {
    if let (Some(owner), Some(stable)) = (
        index
            .project
            .stable_check_owner_for_expression(expression.id),
        index.project.stable_expression_key(expression.id),
    ) && let Some(span) = index.expression_spans.get(&StableOrderExpression {
        owner,
        expression: stable,
    }) {
        return Ok(*span);
    }

    let line = index
        .project
        .global_line(expression.id, expression.line)
        .ok_or_else(|| {
            ProjectDiagnosticFactsError::new(
                "project diagnostic expression has no source-unit line projection",
            )
        })?;
    let start = index
        .project
        .global_byte(expression.id, expression.start)
        .ok_or_else(|| {
            ProjectDiagnosticFactsError::new(
                "project diagnostic expression has no source-unit start projection",
            )
        })?;
    let end = index
        .project
        .global_byte(expression.id, expression.end)
        .ok_or_else(|| {
            ProjectDiagnosticFactsError::new(
                "project diagnostic expression has no source-unit end projection",
            )
        })?;
    Ok(TypeDiagnosticSpan { line, start, end })
}

fn syntax_expression_diagnostic(
    index: &ProjectFactIndex<'_>,
    expression: &boon_syntax::AstExpr,
    message: String,
) -> Result<TypeDiagnostic, ProjectDiagnosticFactsError> {
    let span = syntax_expression_span(index, expression)?;
    Ok(TypeDiagnostic {
        severity: DiagnosticSeverity::Error,
        line: span.line,
        start: span.start,
        end: span.end,
        message,
    })
}

/// Source-shape diagnostics whose authority is the parser plus exact source
/// maps. These checks intentionally precede the remaining flow-based replay:
/// they neither need nor may construct a checked-program database.
fn source_shape_diagnostics(
    index: &ProjectFactIndex<'_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    let expressions = index.project.expressions();
    let byte_items = expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            AstExprKind::BytesLiteral { items, .. } => Some(items.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut diagnostics = Vec::new();
    for expression in expressions.iter() {
        if let Some(raw_input) = crate::checked_pipeline_raw_input(expression) {
            let message = match expression.linked_input {
                Some(linked_input) if expressions.get(linked_input).is_none() => Some(format!(
                    "pipeline expression references missing linked input expression {linked_input}"
                )),
                None if expressions
                    .get(raw_input)
                    .is_some_and(crate::expr_is_pipe_placeholder) =>
                {
                    Some("pipeline continuation is missing its exact linked input".to_owned())
                }
                None if expressions.get(raw_input).is_none() => Some(format!(
                    "pipeline expression references missing direct input expression {raw_input}"
                )),
                _ => None,
            };
            if let Some(message) = message {
                diagnostics.push(syntax_expression_diagnostic(index, expression, message)?);
            }
        }

        let bits = match &expression.kind {
            AstExprKind::BitsLiteral {
                width,
                radix,
                digits,
            }
            | AstExprKind::MatchArm {
                pattern:
                    boon_syntax::AstMatchPattern::Bits {
                        width,
                        radix,
                        digits,
                    },
                ..
            } => Some((*width, *radix, digits.as_str())),
            _ => None,
        };
        if let Some((width, radix, digits)) = bits
            && let Err(error) = Bits::parse_encoded(width, radix, digits)
        {
            diagnostics.push(syntax_expression_diagnostic(
                index,
                expression,
                error.to_string(),
            )?);
        }

        let number = match &expression.kind {
            AstExprKind::Number(value) => Some(value.as_str()),
            AstExprKind::MatchArm {
                pattern: boon_syntax::AstMatchPattern::Number { value },
                ..
            } => Some(value.as_str()),
            _ => None,
        };
        if let Some(literal) = number
            && let Err(error) = ExactNumber::parse_strict(literal, None)
        {
            diagnostics.push(syntax_expression_diagnostic(
                index,
                expression,
                format!("invalid exact Number literal `{literal}`: {error}"),
            )?);
        }

        if matches!(expression.kind, AstExprKind::ByteLiteral { .. })
            && !byte_items.contains(&expression.id)
        {
            diagnostics.push(syntax_expression_diagnostic(
                index,
                expression,
                "byte literals are only valid as direct BYTES constructor items".to_owned(),
            )?);
        }
    }
    Ok(diagnostics)
}

fn expression_structure_diagnostics(
    index: &ProjectFactIndex<'_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    let program = TypecheckSyntaxProgram::UnitNative(index.project.clone());
    let expressions = index.project.expressions();
    let mut diagnostics = Vec::new();
    for expression in expressions.iter() {
        match &expression.kind {
            AstExprKind::BytesLiteral { size, items } => {
                let fixed_size = !matches!(size, BytesSizeSyntax::Dynamic);
                let mut known_len = 0usize;
                let mut all_fixed = true;
                for item in items {
                    let item_type = project_style_expression_type(index, *item);
                    match &item_type {
                        Type::Bytes(boon_checked::BytesType::Fixed(len)) => {
                            known_len = known_len.saturating_add(*len);
                        }
                        Type::Bytes(boon_checked::BytesType::Dynamic) => {
                            all_fixed = false;
                            if fixed_size {
                                let item = expressions.get(*item).ok_or_else(|| {
                                    ProjectDiagnosticFactsError::new(
                                        "BYTES item references a missing syntax expression",
                                    )
                                })?;
                                diagnostics.push(syntax_expression_diagnostic(
                                    index,
                                    item,
                                    "fixed BYTES constructors cannot contain dynamic BYTES"
                                        .to_owned(),
                                )?);
                            }
                        }
                        Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. } => {
                            all_fixed = false;
                        }
                        other => {
                            all_fixed = false;
                            let item = expressions.get(*item).ok_or_else(|| {
                                ProjectDiagnosticFactsError::new(
                                    "BYTES item references a missing syntax expression",
                                )
                            })?;
                            diagnostics.push(syntax_expression_diagnostic(
                                index,
                                item,
                                format!(
                                    "BYTES constructor items must be byte literals or BYTES values, found {}; use Text/to_bytes for explicit TEXT/BYTES conversion",
                                    crate::boon_facing_type_label(other)
                                ),
                            )?);
                        }
                    }
                }
                match size {
                    BytesSizeSyntax::Dynamic => {}
                    BytesSizeSyntax::Infer if !all_fixed => {
                        diagnostics.push(syntax_expression_diagnostic(
                            index,
                            expression,
                            "BYTES[__] length cannot be inferred from dynamic or unknown content"
                                .to_owned(),
                        )?);
                    }
                    BytesSizeSyntax::Fixed(expected)
                        if !items.is_empty() && all_fixed && known_len != *expected =>
                    {
                        diagnostics.push(syntax_expression_diagnostic(
                            index,
                            expression,
                            format!(
                                "BYTES[{expected}] contains {known_len} byte(s); fixed BYTES length must match exactly"
                            ),
                        )?);
                    }
                    BytesSizeSyntax::Infer | BytesSizeSyntax::Fixed(_) => {}
                }
            }
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => {
                let mut explicit_fields = BTreeSet::new();
                for field in fields {
                    if field.spread {
                        let ty = project_style_expression_type(index, field.value);
                        if !crate::type_is_record_spreadable(&ty) {
                            let value = expressions.get(field.value).ok_or_else(|| {
                                ProjectDiagnosticFactsError::new(
                                    "record spread references a missing syntax expression",
                                )
                            })?;
                            diagnostics.push(syntax_expression_diagnostic(
                                index,
                                value,
                                "record spread expects a record value".to_owned(),
                            )?);
                        }
                    } else if !explicit_fields.insert(field.name.as_str()) {
                        let value = expressions.get(field.value).ok_or_else(|| {
                            ProjectDiagnosticFactsError::new(
                                "record field references a missing syntax expression",
                            )
                        })?;
                        diagnostics.push(syntax_expression_diagnostic(
                            index,
                            value,
                            format!("duplicate explicit record field `{}`", field.name),
                        )?);
                    }
                }
                if let AstExprKind::TaggedObject { tag, .. } = &expression.kind
                    && tag == "Oklch"
                {
                    let fields_by_name = fields
                        .iter()
                        .filter(|field| !field.spread)
                        .map(|field| {
                            (
                                field.name.as_str(),
                                project_style_expression_type(index, field.value),
                            )
                        })
                        .collect::<BTreeMap<_, _>>();
                    if !fields_by_name.contains_key("lightness") {
                        diagnostics.push(syntax_expression_diagnostic(
                            index,
                            expression,
                            "tagged object `Oklch[...]` is missing field `lightness`".to_owned(),
                        )?);
                    }
                    for field in fields.iter().filter(|field| {
                        matches!(field.name.as_str(), "lightness" | "chroma" | "hue")
                    }) {
                        if !matches!(
                            fields_by_name.get(field.name.as_str()),
                            Some(Type::Number | Type::Unknown)
                        ) {
                            let value = expressions.get(field.value).ok_or_else(|| {
                                ProjectDiagnosticFactsError::new(
                                    "tagged color field references a missing syntax expression",
                                )
                            })?;
                            diagnostics.push(syntax_expression_diagnostic(
                                index,
                                value,
                                format!(
                                    "tagged object `Oklch[...]` field `{}` must be a number",
                                    field.name
                                ),
                            )?);
                        }
                    }
                }
            }
            AstExprKind::MapEntry { key, .. } => {
                let key_type = project_style_expression_type(index, *key);
                if !crate::type_is_map_key_safe(&key_type) && key_type != Type::Unknown {
                    diagnostics.push(syntax_expression_diagnostic(
                        index,
                        expression,
                        format!(
                            "MAP keys must be closed canonical NUMBER, TEXT, BYTES, BITS, Tag, or object data; found {key_type:?}"
                        ),
                    )?);
                }
            }
            AstExprKind::MapLiteral { entries } => {
                let mut static_keys = BTreeSet::new();
                for entry in entries {
                    let Some(AstExprKind::MapEntry { key, .. }) =
                        expressions.get(*entry).map(|entry| &entry.kind)
                    else {
                        continue;
                    };
                    let Some(key_value) = crate::static_key_value(&program, *key) else {
                        continue;
                    };
                    if !static_keys.insert(key_value) {
                        let key = expressions.get(*key).ok_or_else(|| {
                            ProjectDiagnosticFactsError::new(
                                "MAP key references a missing syntax expression",
                            )
                        })?;
                        diagnostics.push(syntax_expression_diagnostic(
                            index,
                            key,
                            crate::DUPLICATE_MAP_KEY_DIAGNOSTIC.to_owned(),
                        )?);
                    }
                }
            }
            AstExprKind::SetLiteral { items } => {
                let item_type = items
                    .iter()
                    .map(|item| project_style_expression_type(index, *item))
                    .reduce(|existing, extra| crate::widen_structural_type(&existing, &extra))
                    .unwrap_or(Type::Unknown);
                if !crate::type_is_map_key_safe(&item_type) && item_type != Type::Unknown {
                    diagnostics.push(syntax_expression_diagnostic(
                        index,
                        expression,
                        format!(
                            "SET items must be closed canonical NUMBER, TEXT, BYTES, BITS, Tag, or object data; found {item_type:?}"
                        ),
                    )?);
                }
            }
            _ => {}
        }
    }
    Ok(diagnostics)
}

fn recursive_function_diagnostics(
    index: &ProjectFactIndex<'_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    fn visit(
        owner: &StableCheckOwnerKey,
        graph: &BTreeMap<StableCheckOwnerKey, BTreeSet<StableCheckOwnerKey>>,
        names: &BTreeMap<StableCheckOwnerKey, String>,
        spans: &BTreeMap<StableCheckOwnerKey, TypeDiagnosticSpan>,
        visited: &mut BTreeSet<StableCheckOwnerKey>,
        active: &mut Vec<StableCheckOwnerKey>,
        reported: &mut BTreeSet<StableCheckOwnerKey>,
        diagnostics: &mut Vec<TypeDiagnostic>,
    ) -> Result<(), ProjectDiagnosticFactsError> {
        if let Some(position) = active.iter().position(|candidate| candidate == owner) {
            let cycle = active[position..]
                .iter()
                .cloned()
                .chain(std::iter::once(owner.clone()))
                .collect::<Vec<_>>();
            let labels = cycle
                .iter()
                .map(|member| {
                    names.get(member).cloned().ok_or_else(|| {
                        ProjectDiagnosticFactsError::new(
                            "recursive owner call target has no function name",
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            for member in &cycle[..cycle.len().saturating_sub(1)] {
                if !reported.insert(member.clone()) {
                    continue;
                }
                let name = names.get(member).ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "recursive owner function has no declaration name",
                    )
                })?;
                let span = spans.get(member).copied().ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "recursive owner function has no declaration source span",
                    )
                })?;
                diagnostics.push(TypeDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    line: span.line,
                    start: span.start,
                    end: span.end,
                    message: format!(
                        "`FUNCTION {name}` is recursive; recursive functions are not supported by v1 type inference: {}",
                        labels.join(" -> ")
                    ),
                });
            }
            return Ok(());
        }
        if !visited.insert(owner.clone()) {
            return Ok(());
        }
        active.push(owner.clone());
        for callee in graph.get(owner).into_iter().flatten() {
            visit(
                callee,
                graph,
                names,
                spans,
                visited,
                active,
                reported,
                diagnostics,
            )?;
        }
        active.pop();
        Ok(())
    }

    let mut names = BTreeMap::new();
    let mut spans = BTreeMap::new();
    for (owner, view) in &index.owners {
        let Some(name) = &view.replay.function_name else {
            continue;
        };
        let statement = view
            .syntax
            .statements
            .iter()
            .find(|statement| matches!(statement.kind, AstStatementKind::Function { .. }))
            .ok_or_else(|| {
                ProjectDiagnosticFactsError::new(
                    "diagnostic function owner has no function declaration statement",
                )
            })?;
        names.insert(owner.clone(), name.clone());
        spans.insert(owner.clone(), index.statement_span(&statement.stable_key)?);
    }

    let mut graph = names
        .keys()
        .cloned()
        .map(|owner| (owner, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for (owner, view) in &index.owners {
        let Some(caller) = index.callable_owner(owner).cloned() else {
            continue;
        };
        for call in &view.replay.calls {
            let crate::OwnerDiagnosticCallDisposition::User { owner: callee } =
                &call.diagnostic.disposition
            else {
                continue;
            };
            if !names.contains_key(callee) {
                return Err(ProjectDiagnosticFactsError::new(
                    "valid owner call target has no exact function declaration",
                ));
            }
            graph
                .entry(caller.clone())
                .or_default()
                .insert(callee.clone());
        }
    }

    let mut visited = BTreeSet::new();
    let mut active = Vec::new();
    let mut reported = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for owner in graph.keys() {
        visit(
            owner,
            &graph,
            &names,
            &spans,
            &mut visited,
            &mut active,
            &mut reported,
            &mut diagnostics,
        )?;
    }
    Ok(diagnostics)
}

fn stable_syntax_expression(
    index: &ProjectFactIndex<'_>,
    expression: usize,
) -> Option<StableOrderExpression> {
    Some(StableOrderExpression {
        owner: index
            .project
            .stable_check_owner_for_expression(expression)?,
        expression: index.project.stable_expression_key(expression)?,
    })
}

fn host_effect_diagnostics(
    index: &ProjectFactIndex<'_>,
    exact_types: &mut ProjectFlowTypeAnalyzer<'_, '_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    let program = TypecheckSyntaxProgram::UnitNative(index.project.clone());
    let expressions = index.project.expressions();
    let mut diagnostics = Vec::new();
    for expression in expressions.iter() {
        let (operation, inline_args, direct_call) = match &expression.kind {
            AstExprKind::Call { function, args, .. } => (function, args.as_slice(), true),
            AstExprKind::Pipe { op, args, .. } => (op, args.as_slice(), false),
            _ => continue,
        };
        let Some(signature) = crate::host_effect_signature(operation) else {
            continue;
        };
        let Some(stable_call) = stable_syntax_expression(index, expression.id) else {
            continue;
        };
        let Some(call) = index.all_calls.get(&stable_call).copied() else {
            continue;
        };
        if !matches!(
            &call.diagnostic.disposition,
            crate::OwnerDiagnosticCallDisposition::Abi {
                kind: CheckedCallableKind::Builtin
            }
        ) {
            continue;
        }
        if !direct_call {
            diagnostics.push(syntax_expression_diagnostic(
                index,
                expression,
                format!(
                    "typed host effect `{operation}` must use direct named-call syntax, not a pipeline"
                ),
            )?);
            continue;
        }

        let arguments = crate::named_call_argument_exprs(&program, expression.id, inline_args);
        let mut actual = BTreeMap::<&str, usize>::new();
        for (name, value) in &arguments {
            if actual.insert(name.as_str(), *value).is_some() {
                let value = expressions.get(*value).ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "typed host effect argument references a missing expression",
                    )
                })?;
                diagnostics.push(syntax_expression_diagnostic(
                    index,
                    value,
                    format!("typed host effect `{operation}` repeats argument `{name}`"),
                )?);
            }
        }
        for argument in inline_args
            .iter()
            .filter(|argument| argument.is_bare_binding())
        {
            let value = expressions.get(argument.value).ok_or_else(|| {
                ProjectDiagnosticFactsError::new(
                    "typed host effect bare argument references a missing expression",
                )
            })?;
            diagnostics.push(syntax_expression_diagnostic(
                index,
                value,
                format!("typed host effect `{operation}` requires named arguments"),
            )?);
        }

        let expected = signature
            .intent_fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        for name in actual.keys().filter(|name| !expected.contains(**name)) {
            diagnostics.push(syntax_expression_diagnostic(
                index,
                expression,
                format!("typed host effect `{operation}` has no argument `{name}`"),
            )?);
        }
        for (name, value) in &arguments {
            let Some(expected_field) = signature
                .intent_fields
                .iter()
                .find(|field| field.name == *name)
            else {
                continue;
            };
            let value_expression = expressions.get(*value).ok_or_else(|| {
                ProjectDiagnosticFactsError::new(
                    "typed host effect argument references a missing expression",
                )
            })?;
            let actual_type = owner_call_actual_type(index, exact_types, call, *value);
            if !crate::type_is_assignable_to(&actual_type, &expected_field.ty) {
                diagnostics.push(syntax_expression_diagnostic(
                    index,
                    value_expression,
                    format!(
                        "`{operation}` argument `{name}` has incompatible type\nexpected: {}\nfound: {}",
                        crate::boon_facing_type_label(&expected_field.ty),
                        crate::boon_facing_type_label(&actual_type)
                    ),
                )?);
            }
        }
        for name in signature
            .intent_fields
            .iter()
            .filter(|field| field.default.is_none() && !actual.contains_key(field.name.as_str()))
            .map(|field| field.name.as_str())
        {
            diagnostics.push(syntax_expression_diagnostic(
                index,
                expression,
                format!("typed host effect `{operation}` is missing required argument `{name}`"),
            )?);
        }
    }
    Ok(diagnostics)
}

fn owner_call_actual_type(
    index: &ProjectFactIndex<'_>,
    exact_types: &mut ProjectFlowTypeAnalyzer<'_, '_>,
    call: &OwnerDiagnosticStableCallFact,
    expression: usize,
) -> Type {
    let stable = stable_syntax_expression(index, expression);
    let inferred = stable
        .as_ref()
        .and_then(|stable| {
            call.inputs
                .iter()
                .find(|input| input.input == *stable)
                .map(|input| input.actual_type.clone())
        })
        .unwrap_or(Type::Unknown);
    let actual = stable
        .map(|stable| exact_types.expression_type(&stable))
        .filter(|exact| !matches!(exact, Type::Unknown | Type::UnresolvedShape { .. }))
        .unwrap_or(inferred);
    fn has_unresolved_type(ty: &Type) -> bool {
        match ty {
            Type::Unknown | Type::UnresolvedShape { .. } | Type::Var(_) => true,
            Type::Object(shape) => shape.open || shape.fields.values().any(has_unresolved_type),
            Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
                boon_checked::Variant::Tag(_) => false,
                boon_checked::Variant::Tagged { fields, .. } => {
                    fields.fields.values().any(has_unresolved_type)
                }
            }),
            Type::List(item) | Type::Set(item) => has_unresolved_type(item),
            Type::Map { key, value } => has_unresolved_type(key) || has_unresolved_type(value),
            Type::Function { args, result } => {
                args.iter().any(has_unresolved_type) || has_unresolved_type(&result.ty)
            }
            Type::Union(members) => members.iter().any(has_unresolved_type),
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract => false,
        }
    }
    if has_unresolved_type(&actual) {
        Type::Unknown
    } else {
        actual
    }
}

fn builtin_call_diagnostics(
    index: &ProjectFactIndex<'_>,
    abi: &OwnerAbiEnvironment,
    exact_types: &mut ProjectFlowTypeAnalyzer<'_, '_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    fn incompatible(
        index: &ProjectFactIndex<'_>,
        expression: usize,
        message: String,
        diagnostics: &mut Vec<TypeDiagnostic>,
    ) -> Result<(), ProjectDiagnosticFactsError> {
        let expression = index.project.expressions().get(expression).ok_or_else(|| {
            ProjectDiagnosticFactsError::new(
                "builtin diagnostic references a missing syntax expression",
            )
        })?;
        diagnostics.push(syntax_expression_diagnostic(index, expression, message)?);
        Ok(())
    }

    fn true_false(
        index: &ProjectFactIndex<'_>,
        call_expression: &boon_syntax::AstExpr,
        operator: &str,
        actual: &Type,
        diagnostics: &mut Vec<TypeDiagnostic>,
    ) -> Result<(), ProjectDiagnosticFactsError> {
        if crate::type_accepts_true_false_or_unresolved(actual) {
            return Ok(());
        }
        diagnostics.push(syntax_expression_diagnostic(
            index,
            call_expression,
            format!(
                "`{operator}` expects `True` or `False` Tags\nexpected: True | False\nfound: {}",
                crate::boon_facing_type_label(actual)
            ),
        )?);
        Ok(())
    }

    let program = TypecheckSyntaxProgram::UnitNative(index.project.clone());
    let expressions = index.project.expressions();
    let mut diagnostics = Vec::new();
    for expression in expressions.iter() {
        let (function, arguments, pipe_input) = match &expression.kind {
            AstExprKind::Call { function, args, .. } => (function.as_str(), args.as_slice(), None),
            AstExprKind::Pipe {
                input, op, args, ..
            } => (
                op.as_str(),
                args.as_slice(),
                Some(crate::pipeline_source_expr_id(
                    program.statements(),
                    expression.id,
                    *input,
                    program.expressions(),
                )),
            ),
            _ => continue,
        };
        let Some(stable) = stable_syntax_expression(index, expression.id) else {
            continue;
        };
        let Some(call) = index.all_calls.get(&stable).copied() else {
            continue;
        };
        if !matches!(
            call.diagnostic.disposition,
            crate::OwnerDiagnosticCallDisposition::Abi {
                kind: CheckedCallableKind::Builtin
            }
        ) {
            continue;
        }
        if crate::host_effect_signature(function).is_some() {
            continue;
        }
        if crate::session_info_intrinsic_type(function).is_some() {
            if !crate::session_info_intrinsic_allowed(function, abi.role) {
                diagnostics.push(syntax_expression_diagnostic(
                    index,
                    expression,
                    crate::session_info_role_diagnostic(function, abi.role),
                )?);
            }
            if let Some(input) = pipe_input {
                incompatible(
                    index,
                    input,
                    format!("`{function}` does not accept a pipe input"),
                    &mut diagnostics,
                )?;
            }
            for argument in arguments {
                incompatible(
                    index,
                    argument.value,
                    format!("`{function}` does not accept arguments"),
                    &mut diagnostics,
                )?;
            }
            continue;
        }

        if let Some(input) = pipe_input
            && !expressions
                .get(input)
                .is_some_and(crate::expr_is_pipe_placeholder)
        {
            let actual = owner_call_actual_type(index, exact_types, call, input);
            if let Some(expected_label) = crate::builtin_pipe_input_custom_expected_label(function)
            {
                if !crate::builtin_pipe_input_custom_accepts(function, &actual) {
                    incompatible(
                        index,
                        input,
                        format!(
                            "`{function}` pipe input has incompatible type\nexpected: {expected_label}\nfound: {}",
                            crate::boon_facing_type_label(&actual)
                        ),
                        &mut diagnostics,
                    )?;
                }
            } else if let Some(expected) = crate::pipe_input_expected_type(function)
                && !crate::type_is_assignable_to(&actual, &expected)
            {
                incompatible(
                    index,
                    input,
                    format!(
                        "`{function}` pipe input has incompatible type\nexpected: {}\nfound: {}",
                        crate::boon_facing_type_label(&expected),
                        crate::boon_facing_type_label(&actual)
                    ),
                    &mut diagnostics,
                )?;
            }
        }

        let piped = pipe_input.is_some();
        for argument in arguments {
            let name = argument.named_name();
            if is_registered_render_constructor(function)
                && name
                    .is_some_and(|name| !crate::render_arg_should_validate_directly(function, name))
            {
                continue;
            }
            if function == "Bool/toggle" && name == Some("when") {
                continue;
            }
            let actual = owner_call_actual_type(index, exact_types, call, argument.value);
            if let Some(expected_label) =
                crate::builtin_argument_custom_expected_label(function, name, piped)
            {
                if !crate::builtin_argument_custom_accepts(function, name, &actual, piped) {
                    let label = name.unwrap_or("argument");
                    incompatible(
                        index,
                        argument.value,
                        format!(
                            "`{function}` argument `{label}` has incompatible type\nexpected: {expected_label}\nfound: {}",
                            crate::boon_facing_type_label(&actual)
                        ),
                        &mut diagnostics,
                    )?;
                }
            } else if let Some(expected) =
                crate::builtin_argument_expected_type(function, name, piped)
                && !crate::type_is_assignable_to(&actual, &expected)
            {
                let label = name.unwrap_or("argument");
                incompatible(
                    index,
                    argument.value,
                    format!(
                        "`{function}` argument `{label}` has incompatible type\nexpected: {}\nfound: {}",
                        crate::boon_facing_type_label(&expected),
                        crate::boon_facing_type_label(&actual)
                    ),
                    &mut diagnostics,
                )?;
            }
        }

        if matches!(function, "Bool/not" | "Bool/toggle") {
            if let Some(input) = pipe_input.or_else(|| arguments.first().map(|arg| arg.value)) {
                let actual = owner_call_actual_type(index, exact_types, call, input);
                true_false(index, expression, function, &actual, &mut diagnostics)?;
            }
        } else if matches!(function, "Bool/and" | "Bool/or") {
            if let Some(input) = pipe_input {
                let actual = owner_call_actual_type(index, exact_types, call, input);
                true_false(index, expression, function, &actual, &mut diagnostics)?;
            }
            for argument in arguments {
                let actual = owner_call_actual_type(index, exact_types, call, argument.value);
                true_false(index, expression, function, &actual, &mut diagnostics)?;
            }
        }

        if function == "Number/round"
            && let Some(quantum_expression) = arguments
                .iter()
                .find(|argument| argument.named_name() == Some("to"))
                .map(|argument| argument.value)
            && let Some(quantum) = crate::static_exact_number_expr(&program, quantum_expression)
            && !quantum.is_positive()
        {
            incompatible(
                index,
                quantum_expression,
                "`Number/round` argument `to` must be a strictly positive exact Number".to_owned(),
                &mut diagnostics,
            )?;
        }
    }
    Ok(diagnostics)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectDeferredStyleConstraint {
    expression: StableOrderExpression,
    field_name: String,
    expectation: crate::DeferredStyleExpectation,
}

fn project_style_expression_type(index: &ProjectFactIndex<'_>, expression: usize) -> Type {
    stable_syntax_expression(index, expression)
        .and_then(|stable| index.order_expression(&stable))
        .map(|(_, _, _, inferred)| inferred.flow_type.ty.clone())
        .unwrap_or(Type::Unknown)
}

fn project_style_diagnostic(
    index: &ProjectFactIndex<'_>,
    expression: usize,
    message: String,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<(), ProjectDiagnosticFactsError> {
    let expression = index.project.expressions().get(expression).ok_or_else(|| {
        ProjectDiagnosticFactsError::new("style diagnostic references a missing syntax expression")
    })?;
    diagnostics.push(syntax_expression_diagnostic(index, expression, message)?);
    Ok(())
}

fn project_style_color_field(
    index: &ProjectFactIndex<'_>,
    field_name: &str,
    expression: usize,
    constraints: &mut Vec<ProjectDeferredStyleConstraint>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<(), ProjectDiagnosticFactsError> {
    let ty = project_style_expression_type(index, expression);
    if crate::style_type_requires_instantiation(&ty) {
        let expression = stable_syntax_expression(index, expression).ok_or_else(|| {
            ProjectDiagnosticFactsError::new(
                "deferred style color has no exact stable expression identity",
            )
        })?;
        constraints.push(ProjectDeferredStyleConstraint {
            expression,
            field_name: field_name.to_owned(),
            expectation: crate::DeferredStyleExpectation::Color,
        });
    } else if !crate::style_color_accepts_type(&ty) {
        project_style_diagnostic(
            index,
            expression,
            format!(
                "style field `{field_name}` must be `Oklch[...]` or CSS hex text, found `{}`",
                crate::boon_facing_type_label(&ty)
            ),
            diagnostics,
        )?;
    }
    Ok(())
}

fn project_style_nested_object(
    index: &ProjectFactIndex<'_>,
    expression: usize,
    empty_tag: Option<&str>,
    mut check_field: impl FnMut(
        &boon_syntax::AstRecordField,
        &mut Vec<ProjectDeferredStyleConstraint>,
        &mut Vec<TypeDiagnostic>,
    ) -> Result<(), ProjectDiagnosticFactsError>,
    constraints: &mut Vec<ProjectDeferredStyleConstraint>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<(), ProjectDiagnosticFactsError> {
    let Some(expression_row) = index.project.expressions().get(expression) else {
        return Err(ProjectDiagnosticFactsError::new(
            "nested style field references a missing syntax expression",
        ));
    };
    let AstExprKind::Object(fields) = &expression_row.kind else {
        let ty = project_style_expression_type(index, expression);
        if !crate::style_nested_object_accepts_type(&ty, empty_tag) {
            project_style_diagnostic(
                index,
                expression,
                "style nested field must be an object".to_owned(),
                diagnostics,
            )?;
        }
        return Ok(());
    };
    for field in fields {
        check_field(field, constraints, diagnostics)?;
    }
    Ok(())
}

fn project_style_field_value(
    index: &ProjectFactIndex<'_>,
    field_name: &str,
    expression: usize,
    constraints: &mut Vec<ProjectDeferredStyleConstraint>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<(), ProjectDiagnosticFactsError> {
    if crate::is_deleted_public_style_field(field_name) {
        project_style_diagnostic(
            index,
            expression,
            format!("style field `{field_name}` is not public Boon API"),
            diagnostics,
        )?;
        return Ok(());
    }
    match field_name {
        "width" | "height" | "padding" | "gap" => {
            let ty = project_style_expression_type(index, expression);
            if crate::style_type_requires_instantiation(&ty) {
                let expression = stable_syntax_expression(index, expression).ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "deferred style dimension has no exact stable expression identity",
                    )
                })?;
                constraints.push(ProjectDeferredStyleConstraint {
                    expression,
                    field_name: field_name.to_owned(),
                    expectation: crate::DeferredStyleExpectation::Dimension,
                });
            } else if !crate::style_dimension_accepts_type(&ty) {
                project_style_diagnostic(
                    index,
                    expression,
                    format!(
                        "style field `{field_name}` must be a number, `Fill` tag, or `Auto` tag"
                    ),
                    diagnostics,
                )?;
            }
        }
        "font" => {
            project_style_nested_object(
                index,
                expression,
                None,
                |field, constraints, diagnostics| match field.name.as_str() {
                    "size" => {
                        let ty = project_style_expression_type(index, field.value);
                        if crate::style_type_requires_instantiation(&ty) {
                            let expression = stable_syntax_expression(index, field.value)
                                .ok_or_else(|| {
                                    ProjectDiagnosticFactsError::new(
                                        "deferred font size has no exact stable expression identity",
                                    )
                                })?;
                            constraints.push(ProjectDeferredStyleConstraint {
                                expression,
                                field_name: "font.size".to_owned(),
                                expectation: crate::DeferredStyleExpectation::Number,
                            });
                        } else if !matches!(ty, Type::Number) {
                            project_style_diagnostic(
                                index,
                                field.value,
                                "style field `font.size` must be a number".to_owned(),
                                diagnostics,
                            )?;
                        }
                        Ok(())
                    }
                    "color" => project_style_color_field(
                        index,
                        "font.color",
                        field.value,
                        constraints,
                        diagnostics,
                    ),
                    _ => Ok(()),
                },
                constraints,
                diagnostics,
            )?;
        }
        "background" | "border" | "outline" | "borders" => {
            let prefix = field_name.to_owned();
            let empty_tag = (field_name == "outline").then_some("NoOutline");
            project_style_nested_object(
                index,
                expression,
                empty_tag,
                |field, constraints, diagnostics| {
                    if field.name == "color" {
                        project_style_color_field(
                            index,
                            &format!("{prefix}.color"),
                            field.value,
                            constraints,
                            diagnostics,
                        )?;
                    }
                    Ok(())
                },
                constraints,
                diagnostics,
            )?;
        }
        "color" => {
            project_style_color_field(index, "color", expression, constraints, diagnostics)?;
        }
        _ => {}
    }
    Ok(())
}

fn project_style_expression(
    index: &ProjectFactIndex<'_>,
    expression: usize,
    constraints: &mut Vec<ProjectDeferredStyleConstraint>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<(), ProjectDiagnosticFactsError> {
    let Some(expression_row) = index.project.expressions().get(expression) else {
        return Err(ProjectDiagnosticFactsError::new(
            "style value references a missing syntax expression",
        ));
    };
    if matches!(
        expression_row.kind,
        AstExprKind::ListLiteral { .. } | AstExprKind::Delimiter
    ) {
        return Ok(());
    }
    let is_scalar_literal = matches!(
        expression_row.kind,
        AstExprKind::StringLiteral(_)
            | AstExprKind::TextLiteral(_)
            | AstExprKind::Number(_)
            | AstExprKind::Tag(_)
    );
    let AstExprKind::Object(fields) = &expression_row.kind else {
        if is_scalar_literal {
            let ty = project_style_expression_type(index, expression);
            if !crate::is_open_object_type(&ty) {
                project_style_diagnostic(
                    index,
                    expression,
                    "style must be an object".to_owned(),
                    diagnostics,
                )?;
            }
        }
        return Ok(());
    };
    for field in fields {
        project_style_field_value(index, &field.name, field.value, constraints, diagnostics)?;
    }
    Ok(())
}

fn project_style_statement(
    index: &ProjectFactIndex<'_>,
    statement: &boon_syntax::AstStatement,
    constraints: &mut Vec<ProjectDeferredStyleConstraint>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<(), ProjectDiagnosticFactsError> {
    if let Some(expression) = statement.expr {
        project_style_expression(index, expression, constraints, diagnostics)?;
    }
    for child in &statement.children {
        let Some(field) = crate::statement_field(child) else {
            continue;
        };
        let program = TypecheckSyntaxProgram::UnitNative(index.project.clone());
        if let Some(value) = crate::direct_statement_value_expr_id(child, program.expressions()) {
            project_style_field_value(index, field, value, constraints, diagnostics)?;
        } else {
            project_style_statement(index, child, constraints, diagnostics)?;
        }
    }
    Ok(())
}

fn deferred_style_diagnostic_message(
    constraint: &ProjectDeferredStyleConstraint,
    ty: &Type,
) -> String {
    match constraint.expectation {
        crate::DeferredStyleExpectation::Dimension => format!(
            "style field `{}` must be a number, `Fill` tag, or `Auto` tag",
            constraint.field_name
        ),
        crate::DeferredStyleExpectation::Number => {
            format!("style field `{}` must be a number", constraint.field_name)
        }
        crate::DeferredStyleExpectation::Color => format!(
            "style field `{}` must be `Oklch[...]` or CSS hex text, found `{}`",
            constraint.field_name,
            crate::boon_facing_type_label(ty)
        ),
    }
}

fn style_diagnostics(
    index: &ProjectFactIndex<'_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    fn collect_statement(
        index: &ProjectFactIndex<'_>,
        statement: &boon_syntax::AstStatement,
        constraints: &mut Vec<ProjectDeferredStyleConstraint>,
        diagnostics: &mut Vec<TypeDiagnostic>,
    ) -> Result<(), ProjectDiagnosticFactsError> {
        let program = TypecheckSyntaxProgram::UnitNative(index.project.clone());
        if let Some(expression) = statement.expr
            && crate::render_constructor_for_expr(expression, program.expressions()).is_some()
        {
            for child in &statement.children {
                if crate::statement_field(child) == Some("style") {
                    project_style_statement(index, child, constraints, diagnostics)?;
                }
            }
        }
        for child in &statement.children {
            collect_statement(index, child, constraints, diagnostics)?;
        }
        Ok(())
    }

    fn validate_owner(
        owner: &StableCheckOwnerKey,
        substitutions: &BTreeMap<boon_checked::TypeVar, Type>,
        constraints_by_owner: &BTreeMap<
            StableCheckOwnerKey,
            Vec<(ProjectDeferredStyleConstraint, Type, TypeDiagnosticSpan)>,
        >,
        diagnostics: &mut Vec<TypeDiagnostic>,
    ) {
        for (constraint, base, span) in constraints_by_owner.get(owner).into_iter().flatten() {
            let ty = substitute_checked_type(base, substitutions);
            if crate::style_type_requires_instantiation(&ty)
                || crate::deferred_style_expectation_accepts(constraint.expectation, &ty)
            {
                continue;
            }
            diagnostics.push(TypeDiagnostic {
                severity: DiagnosticSeverity::Error,
                line: span.line,
                start: span.start,
                end: span.end,
                message: deferred_style_diagnostic_message(constraint, &ty),
            });
        }
    }

    fn visit_call(
        call_expression: &StableOrderExpression,
        inherited: &BTreeMap<boon_checked::TypeVar, Type>,
        calls: &BTreeMap<StableOrderExpression, &OwnerDiagnosticStableCallFact>,
        calls_by_owner: &BTreeMap<StableCheckOwnerKey, Vec<StableOrderExpression>>,
        constraints_by_owner: &BTreeMap<
            StableCheckOwnerKey,
            Vec<(ProjectDeferredStyleConstraint, Type, TypeDiagnosticSpan)>,
        >,
        active: &mut BTreeSet<StableCheckOwnerKey>,
        diagnostics: &mut Vec<TypeDiagnostic>,
    ) {
        let Some(call) = calls.get(call_expression) else {
            return;
        };
        let crate::OwnerDiagnosticCallDisposition::User { owner } = &call.diagnostic.disposition
        else {
            return;
        };
        let substitutions = crate::compose_checked_type_substitutions(
            inherited,
            &call.diagnostic.type_substitutions,
        );
        validate_owner(owner, &substitutions, constraints_by_owner, diagnostics);
        if !active.insert(owner.clone()) {
            return;
        }
        for nested in calls_by_owner.get(owner).into_iter().flatten() {
            visit_call(
                nested,
                &substitutions,
                calls,
                calls_by_owner,
                constraints_by_owner,
                active,
                diagnostics,
            );
        }
        active.remove(owner);
    }

    let mut constraints = Vec::new();
    let mut diagnostics = Vec::new();
    let program = TypecheckSyntaxProgram::UnitNative(index.project.clone());
    for statement in program.statements() {
        collect_statement(index, statement, &mut constraints, &mut diagnostics)?;
    }
    for expression in index.project.expressions().iter() {
        let (function, arguments) = match &expression.kind {
            AstExprKind::Call { function, args, .. }
            | AstExprKind::Pipe {
                op: function, args, ..
            } => (function, args.as_slice()),
            _ => continue,
        };
        if !is_registered_render_constructor(function) {
            continue;
        }
        let Some(stable) = stable_syntax_expression(index, expression.id) else {
            continue;
        };
        if !matches!(
            index
                .all_calls
                .get(&stable)
                .map(|call| &call.diagnostic.disposition),
            Some(crate::OwnerDiagnosticCallDisposition::Abi {
                kind: CheckedCallableKind::Builtin
            })
        ) {
            continue;
        }
        for argument in arguments
            .iter()
            .filter(|argument| argument.named_name() == Some("style"))
        {
            project_style_expression(index, argument.value, &mut constraints, &mut diagnostics)?;
        }
    }

    constraints.sort();
    constraints.dedup();
    let function_owners = index
        .owners
        .iter()
        .filter_map(|(owner, view)| view.replay.function_name.is_some().then(|| owner.clone()))
        .collect::<BTreeSet<_>>();
    let mut constraints_by_owner = BTreeMap::<
        StableCheckOwnerKey,
        Vec<(ProjectDeferredStyleConstraint, Type, TypeDiagnosticSpan)>,
    >::new();
    for constraint in constraints {
        let owner = index
            .callable_owner(&constraint.expression.owner)
            .cloned()
            .unwrap_or_else(|| constraint.expression.owner.clone());
        let base = if let Some((parameter_owner, ordinal, projection)) =
            index.deferred_style_parameter_origin(&constraint.expression, &[], &mut BTreeSet::new())
        {
            index.deferred_style_parameter_type(&parameter_owner, ordinal, &projection)?
        } else {
            index
                .order_expression(&constraint.expression)
                .map(|(_, _, _, inferred)| inferred.flow_type.ty.clone())
                .ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "deferred style constraint has no inferred expression type",
                    )
                })?
        };
        let span = index.expression_span(&constraint.expression)?;
        constraints_by_owner
            .entry(owner)
            .or_default()
            .push((constraint, base, span));
    }
    for owner in constraints_by_owner
        .keys()
        .filter(|owner| !function_owners.contains(*owner))
    {
        validate_owner(
            owner,
            &BTreeMap::new(),
            &constraints_by_owner,
            &mut diagnostics,
        );
    }
    let mut calls_by_owner = BTreeMap::<StableCheckOwnerKey, Vec<StableOrderExpression>>::new();
    for call in index.all_calls.keys() {
        let owner = index
            .callable_owner(&call.owner)
            .cloned()
            .unwrap_or_else(|| call.owner.clone());
        calls_by_owner.entry(owner).or_default().push(call.clone());
    }
    for call in index.all_calls.keys() {
        let owner = index
            .callable_owner(&call.owner)
            .cloned()
            .unwrap_or_else(|| call.owner.clone());
        if function_owners.contains(&owner) {
            continue;
        }
        visit_call(
            call,
            &BTreeMap::new(),
            &index.all_calls,
            &calls_by_owner,
            &constraints_by_owner,
            &mut BTreeSet::new(),
            &mut diagnostics,
        );
    }
    diagnostics.sort_by(|left, right| {
        (left.line, left.start, left.end, left.message.as_str()).cmp(&(
            right.line,
            right.start,
            right.end,
            right.message.as_str(),
        ))
    });
    diagnostics.dedup();
    Ok(diagnostics)
}

/// Construction-independent source-flow type projection. Reusable owner
/// interfaces intentionally retain generic or demand-shaped fallbacks; source
/// diagnostics need the exact HOLD/branch/declaration value at the authored
/// use site without constructing checked rows.
struct ProjectFlowTypeAnalyzer<'index, 'project> {
    index: &'index ProjectFactIndex<'project>,
    program: TypecheckSyntaxProgram,
    navigation: ProjectOrderAnalyzer<'index, 'project>,
    cache: BTreeMap<StableOrderExpression, Type>,
    active: BTreeSet<StableOrderExpression>,
}

impl<'index, 'project> ProjectFlowTypeAnalyzer<'index, 'project> {
    fn new(index: &'index ProjectFactIndex<'project>) -> Self {
        Self {
            index,
            program: TypecheckSyntaxProgram::UnitNative(index.project.clone()),
            navigation: ProjectOrderAnalyzer::new(index),
            cache: BTreeMap::new(),
            active: BTreeSet::new(),
        }
    }

    fn expression_ref(
        &self,
        owner: &StableCheckOwnerKey,
        reference: usize,
    ) -> Option<StableOrderExpression> {
        self.index
            .stable_expression_ref(owner, u32::try_from(reference).ok()?)
    }

    fn projected_type(ty: &Type, projection: &[String]) -> Option<Type> {
        let Some((field, rest)) = projection.split_first() else {
            return Some(ty.clone());
        };
        match ty {
            Type::Object(shape) => shape
                .fields
                .get(field)
                .and_then(|field| Self::projected_type(field, rest))
                .or_else(|| shape.open.then(crate::open_object_type)),
            Type::VariantSet(variants) => variants
                .iter()
                .filter_map(|variant| match variant {
                    boon_checked::Variant::Tagged { fields, .. } => fields.fields.get(field),
                    boon_checked::Variant::Tag(_) => None,
                })
                .filter_map(|field| Self::projected_type(field, rest))
                .reduce(|left, right| boon_checked::widen_structural_type(&left, &right)),
            Type::Union(members) => members
                .iter()
                .filter_map(|member| Self::projected_type(member, projection))
                .reduce(|left, right| boon_checked::canonical_union_type(vec![left, right])),
            Type::Unknown | Type::UnresolvedShape { .. } | Type::Var(_) => Some(ty.clone()),
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::List(_)
            | Type::Set(_)
            | Type::Map { .. }
            | Type::Function { .. } => None,
        }
    }

    fn project(ty: Type, projection: &[String], fallback: Type) -> Type {
        Self::projected_type(&ty, projection).unwrap_or(fallback)
    }

    fn merged_expression_types(
        &mut self,
        owner: &StableCheckOwnerKey,
        references: &[usize],
        skip_absent: bool,
    ) -> Option<Type> {
        let expressions = references
            .iter()
            .filter_map(|reference| self.expression_ref(owner, *reference))
            .collect::<Vec<_>>();
        expressions
            .iter()
            .map(|expression| self.expression_type(expression))
            .filter(|ty| !skip_absent || !matches!(ty, Type::Absent))
            .reduce(|left, right| boon_checked::widen_structural_type(&left, &right))
    }

    fn list_item_type(ty: &Type) -> Option<Type> {
        match ty {
            Type::List(item) => Some(item.as_ref().clone()),
            Type::Union(members) => members
                .iter()
                .filter_map(Self::list_item_type)
                .reduce(|left, right| boon_checked::widen_structural_type(&left, &right)),
            _ => None,
        }
    }

    fn call_input_type(&mut self, expression: &StableOrderExpression, name: &str) -> Option<Type> {
        let input = self.navigation.call_input(expression, name)?;
        Some(self.expression_type(&input))
    }

    fn call_type(&mut self, expression: &StableOrderExpression, fallback: Type) -> Type {
        let function = self
            .index
            .all_calls
            .get(expression)
            .map(|call| call.function.clone());
        match function.as_deref() {
            Some("List/find") => {
                let list = self.call_input_type(expression, "list");
                list.and_then(|list| Self::list_item_type(&list))
                    .map(|item| {
                        Type::VariantSet(
                            vec![
                                boon_checked::Variant::Tagged {
                                    tag: "Found".to_owned(),
                                    fields: boon_checked::ObjectShape::from_ordered_fields(
                                        [("value".to_owned(), item)],
                                        false,
                                    ),
                                },
                                boon_checked::Variant::Tag("NotFound".to_owned()),
                            ]
                            .into(),
                        )
                    })
                    .unwrap_or(fallback)
            }
            Some("List/map") => self
                .call_input_type(expression, "new")
                .map(|item| Type::List(Type::shared(item)))
                .unwrap_or(fallback),
            Some("List/filter" | "List/retain" | "List/remove" | "List/take" | "List/page") => {
                self.call_input_type(expression, "list").unwrap_or(fallback)
            }
            _ => fallback,
        }
    }

    fn local_target_value(
        &self,
        owner: &StableCheckOwnerKey,
        target: &OwnerLexicalDeclarationTarget,
    ) -> Option<StableOrderExpression> {
        match target {
            OwnerLexicalDeclarationTarget::Statement { statement } => {
                self.index.statement_value_ref(owner, *statement)
            }
            OwnerLexicalDeclarationTarget::RecordField {
                object, ordinal, ..
            } => self
                .index
                .stable_expression_ref(owner, *object)
                .and_then(|object| self.navigation.record_field_value(&object, *ordinal)),
            OwnerLexicalDeclarationTarget::Imported { target } => self
                .navigation
                .stable_target_value(target, &[])
                .map(|(value, _)| value),
            OwnerLexicalDeclarationTarget::Parameter { .. }
            | OwnerLexicalDeclarationTarget::PatternBinding { .. }
            | OwnerLexicalDeclarationTarget::Passed
            | OwnerLexicalDeclarationTarget::Ambiguous { .. } => None,
        }
    }

    fn pattern_binding_type(
        &mut self,
        owner: &StableCheckOwnerKey,
        arm: &StableExpressionKey,
        name: &str,
    ) -> Option<Type> {
        let arm = StableOrderExpression {
            owner: owner.clone(),
            expression: arm.clone(),
        };
        let (_, _, syntax, _) = self.index.order_expression(&arm)?;
        let selector = self
            .index
            .stable_expression_ref(owner, syntax.pattern_selector?)?;
        let pattern = match &syntax.kind {
            AstExprKind::MatchArm { pattern, .. } => pattern.clone(),
            _ => return None,
        };
        let selector = self.expression_type(&selector);
        match pattern {
            boon_syntax::AstMatchPattern::Binding { name: binding } if binding == name => {
                Some(selector)
            }
            boon_syntax::AstMatchPattern::Tag { name: tag, fields }
                if fields.iter().any(|field| field == name) =>
            {
                fn field_type(ty: &Type, tag: &str, field: &str) -> Option<Type> {
                    match ty {
                        Type::VariantSet(variants) => variants
                            .iter()
                            .filter_map(|variant| match variant {
                                boon_checked::Variant::Tagged {
                                    tag: candidate,
                                    fields,
                                } if candidate == tag => fields.fields.get(field).cloned(),
                                _ => None,
                            })
                            .reduce(|left, right| {
                                boon_checked::widen_structural_type(&left, &right)
                            }),
                        Type::Union(members) => members
                            .iter()
                            .filter_map(|member| field_type(member, tag, field))
                            .reduce(|left, right| {
                                boon_checked::canonical_union_type(vec![left, right])
                            }),
                        _ => None,
                    }
                }
                field_type(&selector, &tag, name)
            }
            _ => None,
        }
    }

    fn stable_pattern_binding_type(&mut self, target: &OwnerLexicalTargetRef) -> Option<Type> {
        let OwnerLexicalTargetRef::Declaration {
            owner,
            declaration: OwnerDeclarationStableKey::PatternBinding { selector, name, .. },
            ..
        } = target
        else {
            return None;
        };
        self.pattern_binding_type(owner, selector, name)
    }

    fn read_type(&mut self, expression: &StableOrderExpression, fallback: Type) -> Type {
        if let Some(read) = self.index.effective_read(expression) {
            let binding = match &read.target {
                OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::PatternBinding { arm, name },
                } => self
                    .index
                    .owners
                    .get(&expression.owner)
                    .and_then(|view| view.syntax.expressions.get(*arm as usize))
                    .map(|arm| arm.stable_key.clone())
                    .and_then(|arm| self.pattern_binding_type(&expression.owner, &arm, name)),
                OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::Imported { target },
                }
                | OwnerEffectiveLexicalTarget::Imported { target } => {
                    self.stable_pattern_binding_type(target)
                }
                _ => None,
            };
            if let Some(binding) = binding {
                return Self::project(binding, &read.projection, fallback);
            }
            let value = match &read.target {
                OwnerEffectiveLexicalTarget::Static { target } => {
                    self.local_target_value(&expression.owner, target)
                }
                OwnerEffectiveLexicalTarget::Imported { target } => self
                    .navigation
                    .stable_target_value(target, &[])
                    .map(|(value, _)| value),
                OwnerEffectiveLexicalTarget::FreshOut { .. }
                | OwnerEffectiveLexicalTarget::CallContext { .. }
                | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
            };
            if let Some(value) = value
                && &value != expression
            {
                let ty = self.expression_type(&value);
                return Self::project(ty, &read.projection, fallback);
            }
            return fallback;
        }
        match self.index.value_resolutions.get(expression).copied() {
            Some(crate::OwnerSymbolResolution::Resolved {
                owner, projection, ..
            }) => self
                .index
                .public_result(owner)
                .filter(|value| value != expression)
                .map(|value| {
                    let ty = self.expression_type(&value);
                    Self::project(ty, projection, fallback.clone())
                })
                .unwrap_or(fallback),
            Some(crate::OwnerSymbolResolution::Authoritative { .. })
            | Some(crate::OwnerSymbolResolution::Unresolved { .. })
            | Some(crate::OwnerSymbolResolution::CallableAsValue { .. })
            | Some(crate::OwnerSymbolResolution::Ambiguous { .. })
            | None => fallback,
        }
    }

    fn expression_type(&mut self, expression: &StableOrderExpression) -> Type {
        if let Some(ty) = self.cache.get(expression) {
            return ty.clone();
        }
        let fallback = self
            .index
            .order_expression(expression)
            .map(|(_, _, _, inferred)| inferred.flow_type.ty.clone())
            .unwrap_or(Type::Unknown);
        if !self.active.insert(expression.clone()) {
            return fallback;
        }
        let syntax = self
            .index
            .order_expression(expression)
            .map(|(_, _, syntax, _)| syntax.clone());
        let ty = if let Some(syntax) = syntax {
            match &syntax.kind {
                AstExprKind::StringLiteral(_)
                | AstExprKind::TextLiteral(_)
                | AstExprKind::TextTemplate { .. } => Type::Text,
                AstExprKind::Number(_) => Type::Number,
                AstExprKind::ByteLiteral { .. } => Type::Bytes(boon_checked::BytesType::Fixed(1)),
                AstExprKind::BitsLiteral { width, .. } => Type::Bits { width: *width },
                AstExprKind::BytesLiteral { size, .. } => Type::Bytes(match size {
                    BytesSizeSyntax::Fixed(size) => boon_checked::BytesType::Fixed(*size),
                    BytesSizeSyntax::Dynamic | BytesSizeSyntax::Infer => {
                        boon_checked::BytesType::Dynamic
                    }
                }),
                AstExprKind::Tag(tag) if tag == "SKIP" => Type::Absent,
                AstExprKind::Tag(tag) => {
                    Type::VariantSet(vec![boon_checked::Variant::Tag(tag.clone())].into())
                }
                AstExprKind::Flush { .. } => Type::Absent,
                AstExprKind::Identifier(_) | AstExprKind::Path(_) | AstExprKind::Drain { .. } => {
                    self.read_type(expression, fallback.clone())
                }
                AstExprKind::TaggedObject { tag, fields }
                    if fields.iter().all(|field| !field.spread) =>
                {
                    Type::VariantSet(
                        vec![boon_checked::Variant::Tagged {
                            tag: tag.clone(),
                            fields: boon_checked::ObjectShape::from_ordered_fields::<
                                boon_checked::SharedObjectShape,
                            >(
                                fields.iter().filter_map(|field| {
                                    self.expression_ref(&expression.owner, field.value).map(
                                        |value| (field.name.clone(), self.expression_type(&value)),
                                    )
                                }),
                                false,
                            ),
                        }]
                        .into(),
                    )
                }
                AstExprKind::Object(fields) if fields.iter().all(|field| !field.spread) => {
                    Type::object(boon_checked::ObjectShape::from_ordered_fields(
                        fields.iter().filter_map(|field| {
                            self.expression_ref(&expression.owner, field.value)
                                .map(|value| (field.name.clone(), self.expression_type(&value)))
                        }),
                        false,
                    ))
                }
                AstExprKind::ListLiteral { items, .. } => Type::List(Type::shared(
                    self.merged_expression_types(&expression.owner, items, false)
                        .unwrap_or_else(crate::open_object_type),
                )),
                AstExprKind::SetLiteral { items } => Type::Set(Type::shared(
                    self.merged_expression_types(&expression.owner, items, false)
                        .unwrap_or(Type::Unknown),
                )),
                AstExprKind::MapEntry { key, value } => {
                    let key = self
                        .expression_ref(&expression.owner, *key)
                        .map(|key| self.expression_type(&key))
                        .unwrap_or(Type::Unknown);
                    let value = self
                        .expression_ref(&expression.owner, *value)
                        .map(|value| self.expression_type(&value))
                        .unwrap_or(Type::Unknown);
                    Type::object(boon_checked::ObjectShape::from_ordered_fields(
                        [("key".to_owned(), key), ("value".to_owned(), value)],
                        false,
                    ))
                }
                AstExprKind::MapLiteral { entries } => {
                    let mut keys = Vec::new();
                    let mut values = Vec::new();
                    let entries = entries
                        .iter()
                        .filter_map(|entry| self.expression_ref(&expression.owner, *entry))
                        .collect::<Vec<_>>();
                    for entry in entries {
                        if let Type::Object(shape) = self.expression_type(&entry) {
                            if let Some(key) = shape.fields.get("key") {
                                keys.push(key.clone());
                            }
                            if let Some(value) = shape.fields.get("value") {
                                values.push(value.clone());
                            }
                        }
                    }
                    Type::Map {
                        key: Box::new(
                            keys.into_iter()
                                .reduce(|left, right| {
                                    boon_checked::widen_structural_type(&left, &right)
                                })
                                .unwrap_or(Type::Unknown),
                        ),
                        value: Box::new(
                            values
                                .into_iter()
                                .reduce(|left, right| {
                                    boon_checked::widen_structural_type(&left, &right)
                                })
                                .unwrap_or(Type::Unknown),
                        ),
                    }
                }
                AstExprKind::Infix { op, .. } => {
                    if crate::infix_returns_bool(op) {
                        crate::true_false_type()
                    } else {
                        Type::Number
                    }
                }
                AstExprKind::MatchArm {
                    output: Some(output),
                    ..
                }
                | AstExprKind::Block {
                    result: Some(output),
                    ..
                }
                | AstExprKind::Then {
                    output: Some(output),
                    ..
                } => self
                    .expression_ref(&expression.owner, *output)
                    .map(|output| self.expression_type(&output))
                    .unwrap_or(fallback.clone()),
                AstExprKind::Then {
                    input,
                    output: None,
                }
                | AstExprKind::Draining { input } => self
                    .expression_ref(
                        &expression.owner,
                        syntax
                            .linked_input
                            .map(|input| input as usize)
                            .unwrap_or(*input),
                    )
                    .map(|input| self.expression_type(&input))
                    .unwrap_or(fallback.clone()),
                AstExprKind::When { arms, .. } => self
                    .merged_expression_types(&expression.owner, arms, false)
                    .unwrap_or(fallback.clone()),
                AstExprKind::Pipe { op, arms, .. } if op == "WHILE" => self
                    .merged_expression_types(&expression.owner, arms, false)
                    .unwrap_or(fallback.clone()),
                AstExprKind::Latest { branches } => self
                    .merged_expression_types(&expression.owner, branches, true)
                    .unwrap_or(fallback.clone()),
                AstExprKind::Hold { initial, .. } => {
                    let mut ty = self
                        .expression_ref(
                            &expression.owner,
                            syntax
                                .linked_input
                                .map(|input| input as usize)
                                .unwrap_or(*initial),
                        )
                        .map(|initial| self.expression_type(&initial))
                        .unwrap_or(Type::Unknown);
                    if let Some(syntax_expression) = self.index.syntax_expressions.get(expression) {
                        for update in crate::hold_update_exprs_for_expr(
                            self.program.statements(),
                            *syntax_expression,
                            self.program.expressions(),
                        )
                        .into_iter()
                        .filter_map(|update| stable_syntax_expression(self.index, update))
                        {
                            let update = self.expression_type(&update);
                            if !matches!(update, Type::Absent) {
                                ty = crate::widen_checked_hold_type(&ty, &update);
                            }
                        }
                    }
                    ty
                }
                AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                    if let AstExprKind::Pipe { input, op, .. } = &syntax.kind
                        && let Some(field) = op.strip_prefix("Field/")
                    {
                        self.expression_ref(
                            &expression.owner,
                            syntax
                                .linked_input
                                .map(|input| input as usize)
                                .unwrap_or(*input),
                        )
                        .map(|input| {
                            let input = self.expression_type(&input);
                            Self::projected_type(&input, &[field.to_owned()])
                                .unwrap_or(Type::Unknown)
                        })
                        .unwrap_or(Type::Unknown)
                    } else {
                        let fallback = self
                            .index
                            .all_calls
                            .get(expression)
                            .map(|call| call.result.ty.clone())
                            .unwrap_or(fallback.clone());
                        self.call_type(expression, fallback)
                    }
                }
                AstExprKind::MatchArm { output: None, .. }
                | AstExprKind::Block { result: None, .. } => Type::Absent,
                AstExprKind::Source
                | AstExprKind::Delimiter
                | AstExprKind::Unknown(_)
                | AstExprKind::Arrow { .. }
                | AstExprKind::Object(_)
                | AstExprKind::TaggedObject { .. } => fallback.clone(),
            }
        } else {
            fallback.clone()
        };
        self.active.remove(expression);
        self.cache.insert(expression.clone(), ty.clone());
        ty
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectFlowModeCacheKey {
    kind: u8,
    expression: StableOrderExpression,
    projection: Box<[String]>,
    calls: Box<[StableOrderExpression]>,
}

/// Construction-independent counterpart of the retained checker's projected
/// flow-mode evaluator.
///
/// `FlowType` deliberately describes one value boundary, so a record or list
/// type cannot itself encode that `events.press` is a SOURCE pulse. This
/// evaluator follows the exact owner lexical/call graph and computes that
/// demand-shaped projection without rebuilding checked rows or resolving a
/// name a second time.
struct ProjectFlowModeAnalyzer<'index, 'project> {
    index: &'index ProjectFactIndex<'project>,
    abi: &'index OwnerAbiEnvironment,
    output_flow: &'index ProjectOutputFlowFacts,
    navigation: ProjectOrderAnalyzer<'index, 'project>,
    cache: BTreeMap<ProjectFlowModeCacheKey, FlowMode>,
    active: BTreeSet<ProjectFlowModeCacheKey>,
}

impl<'index, 'project> ProjectFlowModeAnalyzer<'index, 'project> {
    fn field_presence(ty: &Type, field: &str) -> ProjectFieldPresence {
        match ty {
            Type::Object(shape) => {
                if shape.fields.contains_key(field) {
                    ProjectFieldPresence::Yes
                } else if shape.open {
                    ProjectFieldPresence::Maybe
                } else {
                    ProjectFieldPresence::No
                }
            }
            Type::Union(members) => {
                let mut saw_yes = false;
                let mut saw_no = false;
                for member in members.iter() {
                    match Self::field_presence(member, field) {
                        ProjectFieldPresence::Yes => saw_yes = true,
                        ProjectFieldPresence::No => saw_no = true,
                        ProjectFieldPresence::Maybe => return ProjectFieldPresence::Maybe,
                    }
                }
                match (saw_yes, saw_no) {
                    (true, false) => ProjectFieldPresence::Yes,
                    (false, true) => ProjectFieldPresence::No,
                    (true, true) | (false, false) => ProjectFieldPresence::Maybe,
                }
            }
            Type::Unknown | Type::UnresolvedShape { .. } | Type::Var(_) => {
                ProjectFieldPresence::Maybe
            }
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::List(_)
            | Type::Set(_)
            | Type::Map { .. }
            | Type::Function { .. }
            | Type::VariantSet(_) => ProjectFieldPresence::No,
        }
    }

    fn new(
        index: &'index ProjectFactIndex<'project>,
        abi: &'index OwnerAbiEnvironment,
        output_flow: &'index ProjectOutputFlowFacts,
    ) -> Self {
        Self {
            index,
            abi,
            output_flow,
            navigation: ProjectOrderAnalyzer::new(index),
            cache: BTreeMap::new(),
            active: BTreeSet::new(),
        }
    }

    fn cache_key(
        kind: u8,
        expression: &StableOrderExpression,
        projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> ProjectFlowModeCacheKey {
        ProjectFlowModeCacheKey {
            kind,
            expression: expression.clone(),
            projection: projection.to_vec().into_boxed_slice(),
            calls: frames
                .iter()
                .map(|frame| frame.call.clone())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    fn merge(modes: impl IntoIterator<Item = FlowMode>) -> Option<FlowMode> {
        modes.into_iter().reduce(crate::merge_flow_modes)
    }

    fn expression_ref(
        &self,
        owner: &StableCheckOwnerKey,
        reference: usize,
    ) -> Option<StableOrderExpression> {
        self.index
            .stable_expression_ref(owner, u32::try_from(reference).ok()?)
    }

    fn local_target_value(
        &self,
        owner: &StableCheckOwnerKey,
        target: &OwnerLexicalDeclarationTarget,
        frames: &[ProjectOrderFrame],
    ) -> Option<(StableOrderExpression, usize)> {
        match target {
            OwnerLexicalDeclarationTarget::Parameter { ordinal } => {
                frames.iter().enumerate().rev().find_map(|(index, frame)| {
                    (&frame.callable_owner == owner)
                        .then(|| {
                            frame
                                .bindings
                                .get(ordinal)
                                .cloned()
                                .map(|value| (value, index))
                        })
                        .flatten()
                })
            }
            OwnerLexicalDeclarationTarget::Statement { statement } => self
                .index
                .statement_value_ref(owner, *statement)
                .map(|value| (value, frames.len())),
            OwnerLexicalDeclarationTarget::RecordField {
                object, ordinal, ..
            } => self
                .index
                .stable_expression_ref(owner, *object)
                .and_then(|object| self.navigation.record_field_value(&object, *ordinal))
                .map(|value| (value, frames.len())),
            OwnerLexicalDeclarationTarget::Imported { target } => {
                self.navigation.stable_target_value(target, frames)
            }
            OwnerLexicalDeclarationTarget::PatternBinding { .. }
            | OwnerLexicalDeclarationTarget::Passed
            | OwnerLexicalDeclarationTarget::Ambiguous { .. } => None,
        }
    }

    fn dynamic_projection_mode(
        &mut self,
        owner: &StableCheckOwnerKey,
        call: &StableExpressionKey,
        formal_ordinal: u32,
        projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> Option<FlowMode> {
        self.flow_output_target_projection_mode(
            &ProjectOutputTargetFact::Fresh {
                owner: owner.clone(),
                call: call.clone(),
                formal_ordinal,
            },
            projection,
            frames,
            &mut BTreeSet::new(),
        )
    }

    fn parameter_kind(
        &self,
        owner: &StableCheckOwnerKey,
        ordinal: u32,
    ) -> Option<crate::OwnerParameterKind> {
        self.index
            .owners
            .get(owner)?
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.ordinal == ordinal)
            .map(|parameter| parameter.kind)
    }

    fn value_parameter_projection_mode(
        &mut self,
        owner: &StableCheckOwnerKey,
        ordinal: u32,
        projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> Option<FlowMode> {
        if let Some((actual, frame_count)) =
            frames.iter().enumerate().rev().find_map(|(index, frame)| {
                (&frame.callable_owner == owner)
                    .then(|| {
                        frame
                            .bindings
                            .get(&ordinal)
                            .cloned()
                            .map(|actual| (actual, index))
                    })
                    .flatten()
            })
        {
            return Some(self.projection_mode(&actual, projection, &frames[..frame_count]));
        }
        let actuals = self
            .index
            .value_actuals_by_parameter
            .get(&(owner.clone(), ordinal))
            .cloned()
            .unwrap_or_default();
        Self::merge(
            actuals
                .into_iter()
                .map(|actual| self.projection_mode(&actual, projection, frames)),
        )
    }

    fn value_parameter_list_item_projection_mode(
        &mut self,
        owner: &StableCheckOwnerKey,
        ordinal: u32,
        projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> Option<FlowMode> {
        if let Some((actual, frame_count)) =
            frames.iter().enumerate().rev().find_map(|(index, frame)| {
                (&frame.callable_owner == owner)
                    .then(|| {
                        frame
                            .bindings
                            .get(&ordinal)
                            .cloned()
                            .map(|actual| (actual, index))
                    })
                    .flatten()
            })
        {
            return Some(self.list_item_projection_mode(
                &actual,
                projection,
                &frames[..frame_count],
            ));
        }
        let actuals = self
            .index
            .value_actuals_by_parameter
            .get(&(owner.clone(), ordinal))
            .cloned()
            .unwrap_or_default();
        Self::merge(
            actuals
                .into_iter()
                .map(|actual| self.list_item_projection_mode(&actual, projection, frames)),
        )
    }

    fn flow_output_target_projection_mode(
        &mut self,
        target: &ProjectOutputTargetFact,
        projection: &[String],
        frames: &[ProjectOrderFrame],
        visited: &mut BTreeSet<ProjectOutputTargetFact>,
    ) -> Option<FlowMode> {
        if !visited.insert(target.clone()) {
            return None;
        }
        let list_sources = self
            .output_flow
            .list_sources
            .get(target)
            .cloned()
            .unwrap_or_default();
        let forward_sources = self
            .output_flow
            .forward_sources
            .get(target)
            .cloned()
            .unwrap_or_default();
        let list_modes = list_sources
            .into_iter()
            .map(|list| self.list_item_projection_mode(&list, projection, frames))
            .collect::<Vec<_>>();
        let forward_modes = forward_sources
            .into_iter()
            .filter_map(|source| {
                self.flow_output_target_projection_mode(&source, projection, frames, visited)
            })
            .collect::<Vec<_>>();
        visited.remove(target);
        Self::merge(list_modes.into_iter().chain(forward_modes))
    }

    fn out_parameter_projection_mode(
        &mut self,
        owner: &StableCheckOwnerKey,
        ordinal: u32,
        projection: &[String],
        frames: &[ProjectOrderFrame],
        visited: &mut BTreeSet<ProjectOutputTargetFact>,
    ) -> Option<FlowMode> {
        self.flow_output_target_projection_mode(
            &ProjectOutputTargetFact::Parameter {
                owner: owner.clone(),
                ordinal,
            },
            projection,
            frames,
            visited,
        )
    }

    fn stable_target_projection_mode(
        &mut self,
        target: &OwnerLexicalTargetRef,
        projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> Option<FlowMode> {
        let OwnerLexicalTargetRef::Declaration {
            owner, declaration, ..
        } = target
        else {
            return None;
        };
        match declaration {
            OwnerDeclarationStableKey::FreshOut {
                call,
                formal_ordinal,
            } => {
                return self.dynamic_projection_mode(
                    owner,
                    call,
                    *formal_ordinal,
                    projection,
                    frames,
                );
            }
            OwnerDeclarationStableKey::Parameter { ordinal } => {
                return match self.parameter_kind(owner, *ordinal) {
                    Some(crate::OwnerParameterKind::Value) => {
                        self.value_parameter_projection_mode(owner, *ordinal, projection, frames)
                    }
                    Some(crate::OwnerParameterKind::Out) => self.out_parameter_projection_mode(
                        owner,
                        *ordinal,
                        projection,
                        frames,
                        &mut BTreeSet::new(),
                    ),
                    None => None,
                };
            }
            OwnerDeclarationStableKey::Public
            | OwnerDeclarationStableKey::Statement { .. }
            | OwnerDeclarationStableKey::RecordField { .. }
            | OwnerDeclarationStableKey::PatternBinding { .. }
            | OwnerDeclarationStableKey::CallContext { .. } => {}
        }
        let (value, frame_count) = self.navigation.stable_target_value(target, frames)?;
        Some(self.projection_mode(&value, projection, &frames[..frame_count]))
    }

    fn read_mode(
        &mut self,
        expression: &StableOrderExpression,
        extra_projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> Option<FlowMode> {
        let (_, _, _, inferred) = self.index.order_expression(expression)?;
        if let Some(read) = self.index.effective_read(expression) {
            let projection = read
                .projection
                .iter()
                .chain(extra_projection)
                .cloned()
                .collect::<Vec<_>>();
            return match &read.target {
                OwnerEffectiveLexicalTarget::Static { target } => match target {
                    OwnerLexicalDeclarationTarget::Parameter { ordinal } => {
                        match self.parameter_kind(&expression.owner, *ordinal) {
                            Some(crate::OwnerParameterKind::Value) => self
                                .value_parameter_projection_mode(
                                    &expression.owner,
                                    *ordinal,
                                    &projection,
                                    frames,
                                ),
                            Some(crate::OwnerParameterKind::Out) => self
                                .out_parameter_projection_mode(
                                    &expression.owner,
                                    *ordinal,
                                    &projection,
                                    frames,
                                    &mut BTreeSet::new(),
                                ),
                            None => None,
                        }
                    }
                    OwnerLexicalDeclarationTarget::Imported { target } => {
                        self.stable_target_projection_mode(target, &projection, frames)
                    }
                    _ => self
                        .local_target_value(&expression.owner, target, frames)
                        .map(|(value, frame_count)| {
                            self.projection_mode(&value, &projection, &frames[..frame_count])
                        }),
                },
                OwnerEffectiveLexicalTarget::Imported { target } => {
                    self.stable_target_projection_mode(target, &projection, frames)
                }
                OwnerEffectiveLexicalTarget::FreshOut {
                    call,
                    formal_ordinal,
                } => self.dynamic_projection_mode(
                    &expression.owner,
                    call,
                    *formal_ordinal,
                    &projection,
                    frames,
                ),
                OwnerEffectiveLexicalTarget::CallContext { .. } => Some(FlowMode::Continuous),
                OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
            }
            .or(Some(inferred.flow_type.mode));
        }
        match self.index.value_resolutions.get(expression).copied()? {
            crate::OwnerSymbolResolution::Resolved {
                owner, projection, ..
            } => {
                let projection = projection
                    .iter()
                    .chain(extra_projection)
                    .cloned()
                    .collect::<Vec<_>>();
                let value = self.index.public_result(owner)?;
                Some(self.projection_mode(&value, &projection, frames))
            }
            crate::OwnerSymbolResolution::Authoritative { .. }
            | crate::OwnerSymbolResolution::Unresolved { .. }
            | crate::OwnerSymbolResolution::CallableAsValue { .. }
            | crate::OwnerSymbolResolution::Ambiguous { .. } => None,
        }
    }

    fn user_call_frame(
        &self,
        expression: &StableOrderExpression,
        owner: &StableCheckOwnerKey,
        frames: &[ProjectOrderFrame],
    ) -> Option<Vec<ProjectOrderFrame>> {
        if frames.iter().any(|frame| &frame.callable_owner == owner) {
            return None;
        }
        let frame = self.navigation.call_frame(expression, owner)?;
        let mut nested = frames.to_vec();
        nested.push(frame);
        Some(nested)
    }

    fn call_mode(
        &mut self,
        expression: &StableOrderExpression,
        projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> Option<FlowMode> {
        if let Some((_, _, syntax, _)) = self.index.order_expression(expression)
            && let AstExprKind::Pipe { op, input, .. } = &syntax.kind
            && op.starts_with("Field/")
        {
            let input = self.expression_ref(
                &expression.owner,
                syntax
                    .linked_input
                    .map(|input| input as usize)
                    .unwrap_or(*input),
            )?;
            return Some(self.projection_mode(&input, projection, frames));
        }
        let call = self.index.call(expression)?;
        if let crate::InferredOwnerCallableTarget::Owner { owner } = &call.target {
            let result = self.index.public_result(owner)?;
            let nested = self.user_call_frame(expression, owner, frames)?;
            return Some(self.projection_mode(&result, projection, &nested));
        }
        if !projection.is_empty()
            && self.abi.callable(&call.function).is_some_and(|contract| {
                contract.result_specialization
                    == crate::OwnerAbiResultSpecialization::RenderConstructor
            })
        {
            let (field, rest) = projection.split_first()?;
            if let Some(input) = self.navigation.call_input(expression, field) {
                return Some(self.projection_mode(&input, rest, frames));
            }
        }
        if projection.is_empty() && call.function == "List/map" {
            return self
                .navigation
                .call_input(expression, "new")
                .map(|new| self.expression_mode(&new, frames));
        }
        if projection.is_empty() && call.function == "List/latest" {
            return self
                .navigation
                .call_input(expression, "list")
                .map(|list| self.expression_mode(&list, frames));
        }
        projection
            .is_empty()
            .then_some(call.result.mode)
            .or_else(|| {
                matches!(
                    call.result.mode,
                    FlowMode::TickPresent | FlowMode::PresentOrAbsent
                )
                .then_some(call.result.mode)
            })
    }

    fn expression_mode(
        &mut self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
    ) -> FlowMode {
        let key = Self::cache_key(0, expression, &[], frames);
        if let Some(mode) = self.cache.get(&key).copied() {
            return mode;
        }
        let fallback = self
            .index
            .order_expression(expression)
            .map(|(_, _, _, inferred)| inferred.flow_type.mode)
            .unwrap_or(FlowMode::Continuous);
        if !self.active.insert(key.clone()) {
            return fallback;
        }
        let mode = self
            .index
            .order_expression(expression)
            .map(|(_, _, syntax, _)| match &syntax.kind {
                AstExprKind::Source | AstExprKind::Then { .. } => FlowMode::PresentOrAbsent,
                AstExprKind::Tag(tag) if tag == "SKIP" => FlowMode::Absent,
                AstExprKind::Identifier(_) | AstExprKind::Path(_) | AstExprKind::Drain { .. } => {
                    self.read_mode(expression, &[], frames).unwrap_or(fallback)
                }
                AstExprKind::Hold { .. } => FlowMode::Continuous,
                AstExprKind::Pipe { op, .. } if op == "WHILE" => FlowMode::Continuous,
                AstExprKind::Latest { branches } => {
                    crate::latest_flow_mode(branches.iter().filter_map(|branch| {
                        self.expression_ref(&expression.owner, *branch)
                            .map(|branch| self.expression_mode(&branch, frames))
                    }))
                    .unwrap_or(fallback)
                }
                AstExprKind::When { input, .. } | AstExprKind::Draining { input } => self
                    .expression_ref(
                        &expression.owner,
                        syntax
                            .linked_input
                            .map(|input| input as usize)
                            .unwrap_or(*input),
                    )
                    .map(|input| self.expression_mode(&input, frames))
                    .unwrap_or(fallback),
                AstExprKind::Block {
                    result: Some(result),
                    ..
                }
                | AstExprKind::MatchArm {
                    output: Some(result),
                    ..
                } => self
                    .expression_ref(&expression.owner, *result)
                    .map(|result| self.expression_mode(&result, frames))
                    .unwrap_or(fallback),
                AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                    self.call_mode(expression, &[], frames).unwrap_or(fallback)
                }
                _ => fallback,
            })
            .unwrap_or(fallback);
        self.active.remove(&key);
        self.cache.insert(key, mode);
        mode
    }

    fn projection_mode(
        &mut self,
        expression: &StableOrderExpression,
        projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> FlowMode {
        if projection.is_empty() {
            return self.expression_mode(expression, frames);
        }
        let key = Self::cache_key(1, expression, projection, frames);
        if let Some(mode) = self.cache.get(&key).copied() {
            return mode;
        }
        let fallback = self.expression_mode(expression, frames);
        if !self.active.insert(key.clone()) {
            return fallback;
        }
        let mode = self
            .index
            .order_expression(expression)
            .and_then(|(_, _, syntax, _)| match &syntax.kind {
                AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => {
                    let (field, rest) = projection.split_first()?;
                    let mut modes = Vec::new();
                    for candidate in fields {
                        let Some(value) = self.expression_ref(&expression.owner, candidate.value)
                        else {
                            continue;
                        };
                        if !candidate.spread && candidate.name == *field {
                            modes.clear();
                            modes.push(self.projection_mode(&value, rest, frames));
                            continue;
                        }
                        if !candidate.spread {
                            continue;
                        }
                        let presence = self
                            .index
                            .order_expression(&value)
                            .map(|(_, _, _, inferred)| {
                                Self::field_presence(&inferred.flow_type.ty, field)
                            })
                            .unwrap_or(ProjectFieldPresence::Maybe);
                        match presence {
                            ProjectFieldPresence::No => {}
                            ProjectFieldPresence::Yes => {
                                modes.clear();
                                modes.push(self.projection_mode(&value, projection, frames));
                            }
                            ProjectFieldPresence::Maybe => {
                                modes.push(self.projection_mode(&value, projection, frames));
                            }
                        }
                    }
                    Self::merge(modes)
                }
                AstExprKind::Identifier(_) | AstExprKind::Path(_) | AstExprKind::Drain { .. } => {
                    self.read_mode(expression, projection, frames)
                }
                AstExprKind::Block {
                    result: Some(result),
                    ..
                }
                | AstExprKind::MatchArm {
                    output: Some(result),
                    ..
                }
                | AstExprKind::Then {
                    output: Some(result),
                    ..
                } => self
                    .expression_ref(&expression.owner, *result)
                    .map(|result| self.projection_mode(&result, projection, frames)),
                AstExprKind::Then {
                    input: result,
                    output: None,
                }
                | AstExprKind::Hold {
                    initial: result, ..
                }
                | AstExprKind::Draining { input: result } => self
                    .expression_ref(&expression.owner, *result)
                    .map(|result| self.projection_mode(&result, projection, frames)),
                AstExprKind::Latest { branches } => {
                    crate::latest_flow_mode(branches.iter().filter_map(|branch| {
                        self.expression_ref(&expression.owner, *branch)
                            .map(|branch| self.projection_mode(&branch, projection, frames))
                    }))
                }
                AstExprKind::When { arms: branches, .. } => {
                    Self::merge(branches.iter().filter_map(|branch| {
                        self.expression_ref(&expression.owner, *branch)
                            .map(|branch| self.projection_mode(&branch, projection, frames))
                    }))
                }
                AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                    self.call_mode(expression, projection, frames)
                }
                _ => None,
            })
            .unwrap_or(fallback);
        self.active.remove(&key);
        self.cache.insert(key, mode);
        mode
    }

    fn list_item_projection_mode(
        &mut self,
        expression: &StableOrderExpression,
        projection: &[String],
        frames: &[ProjectOrderFrame],
    ) -> FlowMode {
        let key = Self::cache_key(2, expression, projection, frames);
        if let Some(mode) = self.cache.get(&key).copied() {
            return mode;
        }
        if !self.active.insert(key.clone()) {
            return FlowMode::Continuous;
        }
        let mode = self
            .index
            .order_expression(expression)
            .and_then(|(_, _, syntax, _)| match &syntax.kind {
                AstExprKind::ListLiteral { items, .. } => {
                    Self::merge(items.iter().filter_map(|item| {
                        self.expression_ref(&expression.owner, *item)
                            .map(|item| self.projection_mode(&item, projection, frames))
                    }))
                }
                AstExprKind::Block {
                    result: Some(result),
                    ..
                }
                | AstExprKind::Then {
                    output: Some(result),
                    ..
                } => self
                    .expression_ref(&expression.owner, *result)
                    .map(|result| self.list_item_projection_mode(&result, projection, frames)),
                AstExprKind::Then {
                    input: result,
                    output: None,
                }
                | AstExprKind::Hold {
                    initial: result, ..
                }
                | AstExprKind::Draining { input: result } => self
                    .expression_ref(&expression.owner, *result)
                    .map(|result| self.list_item_projection_mode(&result, projection, frames)),
                AstExprKind::Latest { branches } => {
                    crate::latest_flow_mode(branches.iter().filter_map(|branch| {
                        self.expression_ref(&expression.owner, *branch)
                            .map(|branch| {
                                self.list_item_projection_mode(&branch, projection, frames)
                            })
                    }))
                }
                AstExprKind::Identifier(_) | AstExprKind::Path(_) | AstExprKind::Drain { .. } => {
                    let Some(read) = self.index.effective_read(expression) else {
                        let crate::OwnerSymbolResolution::Resolved {
                            owner,
                            projection: resolved_projection,
                            ..
                        } = self.index.value_resolutions.get(expression).copied()?
                        else {
                            return None;
                        };
                        if !resolved_projection.is_empty() {
                            return None;
                        }
                        let value = self.index.public_result(owner)?;
                        return Some(self.list_item_projection_mode(&value, projection, frames));
                    };
                    if !read.projection.is_empty() {
                        return None;
                    }
                    match &read.target {
                        OwnerEffectiveLexicalTarget::Static {
                            target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
                        } if matches!(
                            self.parameter_kind(&expression.owner, *ordinal),
                            Some(crate::OwnerParameterKind::Value)
                        ) =>
                        {
                            self.value_parameter_list_item_projection_mode(
                                &expression.owner,
                                *ordinal,
                                projection,
                                frames,
                            )
                        }
                        OwnerEffectiveLexicalTarget::Static {
                            target: OwnerLexicalDeclarationTarget::Imported { target },
                        }
                        | OwnerEffectiveLexicalTarget::Imported { target } => {
                            let OwnerLexicalTargetRef::Declaration {
                                owner, declaration, ..
                            } = target
                            else {
                                return None;
                            };
                            if let OwnerDeclarationStableKey::Parameter { ordinal } = declaration
                                && matches!(
                                    self.parameter_kind(owner, *ordinal),
                                    Some(crate::OwnerParameterKind::Value)
                                )
                            {
                                self.value_parameter_list_item_projection_mode(
                                    owner, *ordinal, projection, frames,
                                )
                            } else {
                                let (value, frame_count) =
                                    self.navigation.stable_target_value(target, frames)?;
                                Some(self.list_item_projection_mode(
                                    &value,
                                    projection,
                                    &frames[..frame_count],
                                ))
                            }
                        }
                        OwnerEffectiveLexicalTarget::Static { target } => {
                            let (value, frame_count) =
                                self.local_target_value(&expression.owner, target, frames)?;
                            Some(self.list_item_projection_mode(
                                &value,
                                projection,
                                &frames[..frame_count],
                            ))
                        }
                        OwnerEffectiveLexicalTarget::FreshOut {
                            call,
                            formal_ordinal,
                        } => self.dynamic_projection_mode(
                            &expression.owner,
                            call,
                            *formal_ordinal,
                            projection,
                            frames,
                        ),
                        OwnerEffectiveLexicalTarget::CallContext { .. }
                        | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                        | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
                    }
                }
                AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                    let call = self.index.call(expression)?;
                    match call.function.as_str() {
                        "List/map" => self
                            .navigation
                            .call_input(expression, "new")
                            .map(|value| self.projection_mode(&value, projection, frames)),
                        "List/filter" | "List/retain" | "List/remove" | "List/sort_by"
                        | "List/then_by" | "List/take" => self
                            .navigation
                            .call_input(expression, "list")
                            .map(|list| self.list_item_projection_mode(&list, projection, frames)),
                        "List/append" => {
                            Self::merge(
                                self.navigation
                                    .call_input(expression, "list")
                                    .map(|list| {
                                        self.list_item_projection_mode(&list, projection, frames)
                                    })
                                    .into_iter()
                                    .chain(self.navigation.call_input(expression, "item").map(
                                        |item| self.projection_mode(&item, projection, frames),
                                    )),
                            )
                        }
                        _ => match &call.target {
                            crate::InferredOwnerCallableTarget::Owner { owner } => {
                                let result = self.index.public_result(owner)?;
                                let nested = self.user_call_frame(expression, owner, frames)?;
                                Some(self.list_item_projection_mode(&result, projection, &nested))
                            }
                            _ => None,
                        },
                    }
                }
                _ => None,
            })
            .unwrap_or(FlowMode::Continuous);
        self.active.remove(&key);
        self.cache.insert(key, mode);
        mode
    }

    fn modes_for(
        mut self,
        expressions: impl IntoIterator<Item = StableOrderExpression>,
    ) -> BTreeMap<StableOrderExpression, FlowMode> {
        expressions
            .into_iter()
            .map(|expression| {
                let mode = self.expression_mode(&expression, &[]);
                (expression, mode)
            })
            .collect()
    }
}

fn temporal_mode_roots(
    index: &ProjectFactIndex<'_>,
) -> Result<Vec<StableOrderExpression>, ProjectDiagnosticFactsError> {
    let mut roots = BTreeSet::new();
    for expression in index.expressions.keys() {
        let Some((_, _, syntax, _)) = index.order_expression(expression) else {
            return Err(ProjectDiagnosticFactsError::new(
                "temporal mode root is missing from its owner body",
            ));
        };
        let reference = match &syntax.kind {
            AstExprKind::Pipe { input, op, .. } if op == "WHILE" => {
                Some(syntax.linked_input.unwrap_or(*input as u32))
            }
            AstExprKind::Then { input, .. } => Some(syntax.linked_input.unwrap_or(*input as u32)),
            AstExprKind::Latest { branches } => branches
                .first()
                .and_then(|branch| u32::try_from(*branch).ok()),
            _ => None,
        };
        if let Some(reference) = reference
            && let Some(root) = index.stable_expression_ref(&expression.owner, reference)
        {
            roots.insert(root);
        }
    }
    Ok(roots.into_iter().collect())
}

fn temporal_diagnostics(
    index: &ProjectFactIndex<'_>,
    exact_modes: &BTreeMap<StableOrderExpression, FlowMode>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    let mut diagnostics = Vec::new();
    for expression in index.expressions.keys() {
        let Some((_, _, syntax, _)) = index.order_expression(expression) else {
            return Err(ProjectDiagnosticFactsError::new(
                "temporal diagnostic expression is missing from its owner body",
            ));
        };
        match &syntax.kind {
            AstExprKind::Pipe { input, op, .. } if op == "WHILE" => {
                let reference = syntax.linked_input.unwrap_or(*input as u32);
                let Some(input) = index.stable_expression_ref(&expression.owner, reference) else {
                    continue;
                };
                let Some((_, _, _, inferred)) = index.order_expression(&input) else {
                    continue;
                };
                let mode = exact_modes
                    .get(&input)
                    .copied()
                    .unwrap_or(inferred.flow_type.mode);
                if !matches!(mode, FlowMode::Continuous) {
                    let span = index.expression_span(&input)?;
                    diagnostics.push(TypeDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        line: span.line,
                        start: span.start,
                        end: span.end,
                        message: "`WHILE` requires a continuous selector".to_owned(),
                    });
                }
            }
            AstExprKind::Then { input, .. } => {
                let reference = syntax.linked_input.unwrap_or(*input as u32);
                let Some(input) = index.stable_expression_ref(&expression.owner, reference) else {
                    continue;
                };
                let Some((_, _, _, inferred)) = index.order_expression(&input) else {
                    continue;
                };
                let flow_type = FlowType {
                    mode: exact_modes
                        .get(&input)
                        .copied()
                        .unwrap_or(inferred.flow_type.mode),
                    ty: inferred.flow_type.ty.clone(),
                };
                if !matches!(
                    flow_type.mode,
                    FlowMode::TickPresent | FlowMode::PresentOrAbsent
                ) && !index.expression_is_stateful(&input, exact_modes, &mut BTreeSet::new())
                    && !index.expression_is_parameter_read(&input)
                    && !matches!(flow_type.ty, Type::Unknown)
                    && !crate::is_open_object_type(&flow_type.ty)
                {
                    let span = index.expression_span(&input)?;
                    diagnostics.push(TypeDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        line: span.line,
                        start: span.start,
                        end: span.end,
                        message: crate::then_tick_contract_message(&flow_type),
                    });
                }
            }
            AstExprKind::Latest { branches } if branches.len() == 1 => {
                let span = index.expression_span(expression)?;
                diagnostics.push(TypeDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    line: span.line,
                    start: span.start,
                    end: span.end,
                    message:
                        "`LATEST` merges two or more branches; use its single expression directly"
                            .to_owned(),
                });
            }
            _ => {}
        }
    }
    Ok(diagnostics)
}

fn match_pattern_diagnostics(
    index: &ProjectFactIndex<'_>,
    exact_types: &mut ProjectFlowTypeAnalyzer<'_, '_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    let mut diagnostics = Vec::new();
    for expression in index.expressions.keys() {
        let Some((_, _, syntax, _)) = index.order_expression(expression) else {
            return Err(ProjectDiagnosticFactsError::new(
                "match diagnostic expression is missing from its owner body",
            ));
        };
        let (input, arms) = match &syntax.kind {
            AstExprKind::When { input, arms } => (*input, arms.as_slice()),
            AstExprKind::Pipe {
                input, op, arms, ..
            } if op == "WHILE" => (*input, arms.as_slice()),
            _ => continue,
        };
        let input = syntax.linked_input.unwrap_or(input as u32);
        let Some(selector) = index.stable_expression_ref(&expression.owner, input) else {
            continue;
        };
        let selector_type = exact_types.expression_type(&selector);
        for arm in arms {
            let Some(arm) = index.stable_expression_ref(&expression.owner, *arm as u32) else {
                continue;
            };
            let Some((_, _, arm_syntax, _)) = index.order_expression(&arm) else {
                continue;
            };
            let AstExprKind::MatchArm { pattern, .. } = &arm_syntax.kind else {
                continue;
            };
            let Some(pattern) = crate::checked_match_pattern_from_ast(pattern) else {
                continue;
            };
            if crate::checked_match_pattern_compatibility(&pattern, &selector_type) != Some(false) {
                continue;
            }
            let span = index.expression_span(&arm)?;
            diagnostics.push(TypeDiagnostic {
                severity: DiagnosticSeverity::Error,
                line: span.line,
                start: span.start,
                end: span.end,
                message: crate::incompatible_checked_match_pattern_message(
                    &pattern,
                    &selector_type,
                ),
            });
        }
    }
    Ok(diagnostics)
}

fn duplicate_function_diagnostics(
    index: &ProjectFactIndex<'_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    let mut declarations = Vec::new();
    for view in index.owners.values() {
        for statement in &view.syntax.statements {
            let AstStatementKind::Function { name, .. } = &statement.kind else {
                continue;
            };
            let span = index.statement_span(&statement.stable_key)?;
            declarations.push((span.start, name.clone(), span));
        }
    }
    declarations.sort_by_key(|(start, _, _)| *start);
    let mut seen = BTreeSet::new();
    Ok(declarations
        .into_iter()
        .filter_map(|(_, name, span)| {
            (!seen.insert(name.clone())).then_some(TypeDiagnostic {
                severity: DiagnosticSeverity::Error,
                line: span.line,
                start: span.start,
                end: span.end,
                message: format!("function `{name}` is declared more than once"),
            })
        })
        .collect())
}

#[derive(Clone, Copy)]
struct TypeDiagnosticSpan {
    line: usize,
    start: usize,
    end: usize,
}

fn global_local_span_from_offsets(
    layout_start_line: usize,
    layout_start_byte: usize,
    source_line: u64,
    source_start: u64,
    source_end: u64,
) -> Result<TypeDiagnosticSpan, ProjectDiagnosticFactsError> {
    let source_line = usize::try_from(source_line)
        .map_err(|_| ProjectDiagnosticFactsError::new("diagnostic line exceeds usize"))?;
    let line = layout_start_line
        .checked_add(source_line.saturating_sub(1))
        .ok_or_else(|| ProjectDiagnosticFactsError::new("diagnostic line overflow"))?;
    let start = layout_start_byte
        .checked_add(
            usize::try_from(source_start)
                .map_err(|_| ProjectDiagnosticFactsError::new("diagnostic start exceeds usize"))?,
        )
        .ok_or_else(|| ProjectDiagnosticFactsError::new("diagnostic start overflow"))?;
    let end = layout_start_byte
        .checked_add(
            usize::try_from(source_end)
                .map_err(|_| ProjectDiagnosticFactsError::new("diagnostic end exceeds usize"))?,
        )
        .ok_or_else(|| ProjectDiagnosticFactsError::new("diagnostic end overflow"))?;
    if start > end {
        return Err(ProjectDiagnosticFactsError::new(
            "diagnostic source span starts after it ends",
        ));
    }
    Ok(TypeDiagnosticSpan { line, start, end })
}

fn global_anchor_span(
    project: &ProjectSyntaxSnapshot,
    source_map: &OwnerSourceMap,
    site: &OwnerSourceAnchorSite,
    role: OwnerSourceAnchorRole,
) -> Result<TypeDiagnosticSpan, ProjectDiagnosticFactsError> {
    let anchor = source_map.anchor(site, role).ok_or_else(|| {
        ProjectDiagnosticFactsError::new("project diagnostic has no exact source anchor")
    })?;
    let layout = project
        .source_layouts()
        .iter()
        .find(|layout| &layout.source_unit_id == source_map.owner().source_unit_id())
        .ok_or_else(|| {
            ProjectDiagnosticFactsError::new("project diagnostic anchor has no source-unit layout")
        })?;
    let source_line = usize::try_from(anchor.line)
        .map_err(|_| ProjectDiagnosticFactsError::new("diagnostic line exceeds usize"))?;
    let line = layout
        .start_line
        .checked_add(source_line.saturating_sub(1))
        .ok_or_else(|| ProjectDiagnosticFactsError::new("diagnostic line overflow"))?;
    let start = layout
        .start_byte
        .checked_add(
            usize::try_from(anchor.start)
                .map_err(|_| ProjectDiagnosticFactsError::new("diagnostic start exceeds usize"))?,
        )
        .ok_or_else(|| ProjectDiagnosticFactsError::new("diagnostic start overflow"))?;
    let end = layout
        .start_byte
        .checked_add(
            usize::try_from(anchor.end)
                .map_err(|_| ProjectDiagnosticFactsError::new("diagnostic end exceeds usize"))?,
        )
        .ok_or_else(|| ProjectDiagnosticFactsError::new("diagnostic end overflow"))?;
    Ok(TypeDiagnosticSpan { line, start, end })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectAuthorityOriginKey {
    /// Executable construction or call site in the current dynamic scope.
    site: StableOrderExpression,
    /// Exact constructor within that site. One user call may return multiple
    /// independently owned collection authorities.
    constructor: StableOrderExpression,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ProjectAuthorityParentKind {
    MapKey,
    ListOccurrence,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectAuthorityParentSite {
    kind: ProjectAuthorityParentKind,
    authority: ProjectAuthorityOriginKey,
    attachment: StableOrderExpression,
}

type ProjectAuthoritySubstitutions = BTreeMap<(StableCheckOwnerKey, u32), StableOrderExpression>;

struct ProjectAuthorityAnalyzer<'index, 'project> {
    index: &'index ProjectFactIndex<'project>,
}

impl<'index, 'project> ProjectAuthorityAnalyzer<'index, 'project> {
    fn new(index: &'index ProjectFactIndex<'project>) -> Self {
        Self { index }
    }

    fn expression_ref(
        &self,
        owner: &StableCheckOwnerKey,
        reference: usize,
    ) -> Option<StableOrderExpression> {
        self.index
            .stable_expression_ref(owner, u32::try_from(reference).ok()?)
    }

    fn owner_ref(
        &self,
        owner: &StableCheckOwnerKey,
        expression: &OwnerExpressionRef,
    ) -> Option<StableOrderExpression> {
        match expression {
            OwnerExpressionRef::Local { expression } => {
                self.index.stable_expression_ref(owner, expression.0)
            }
            OwnerExpressionRef::Child { owner, expression } => Some(StableOrderExpression {
                owner: owner.clone(),
                expression: expression.clone(),
            }),
        }
    }

    fn record_field_value(
        &self,
        owner: &StableCheckOwnerKey,
        object: u32,
        ordinal: u32,
    ) -> Option<StableOrderExpression> {
        let object = self.index.stable_expression_ref(owner, object)?;
        let (_, _, syntax, _) = self.index.order_expression(&object)?;
        let fields = match &syntax.kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => fields,
            _ => return None,
        };
        self.expression_ref(owner, fields.get(ordinal as usize)?.value)
    }

    fn stable_record_field_value(
        &self,
        owner: &StableCheckOwnerKey,
        object: &StableExpressionKey,
        ordinal: u32,
    ) -> Option<StableOrderExpression> {
        let object = StableOrderExpression {
            owner: owner.clone(),
            expression: object.clone(),
        };
        let (_, _, syntax, _) = self.index.order_expression(&object)?;
        let fields = match &syntax.kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => fields,
            _ => return None,
        };
        self.expression_ref(owner, fields.get(ordinal as usize)?.value)
    }

    fn borrowed_origin(
        expression: &StableOrderExpression,
    ) -> BTreeMap<ProjectAuthorityOriginKey, bool> {
        BTreeMap::from([(
            ProjectAuthorityOriginKey {
                site: expression.clone(),
                constructor: expression.clone(),
            },
            true,
        )])
    }

    fn merge_origins(
        result: &mut BTreeMap<ProjectAuthorityOriginKey, bool>,
        extra: BTreeMap<ProjectAuthorityOriginKey, bool>,
    ) {
        for (origin, borrowed) in extra {
            result
                .entry(origin)
                .and_modify(|existing| *existing |= borrowed)
                .or_insert(borrowed);
        }
    }

    fn origins(
        &self,
        expression: &StableOrderExpression,
    ) -> Result<BTreeMap<ProjectAuthorityOriginKey, bool>, ProjectDiagnosticFactsError> {
        self.origins_inner(
            expression,
            &ProjectAuthoritySubstitutions::new(),
            None,
            None,
            &mut BTreeSet::new(),
        )
    }

    fn declaration_origins(
        &self,
        read_expression: &StableOrderExpression,
        provider: &StableCheckOwnerKey,
        value: Option<StableOrderExpression>,
        substitutions: &ProjectAuthoritySubstitutions,
        fresh_site: Option<&StableOrderExpression>,
        callable_owner: Option<&StableCheckOwnerKey>,
        active: &mut BTreeSet<(StableOrderExpression, Option<StableOrderExpression>)>,
    ) -> Result<BTreeMap<ProjectAuthorityOriginKey, bool>, ProjectDiagnosticFactsError> {
        let Some(value) = value else {
            return Ok(Self::borrowed_origin(read_expression));
        };
        let local_to_callable = callable_owner.is_some_and(|owner| {
            owner == provider || crate::owner_syntax::is_descendant_owner(owner, provider)
        });
        self.origins_inner(
            &value,
            substitutions,
            local_to_callable.then_some(fresh_site).flatten(),
            callable_owner.filter(|_| local_to_callable),
            active,
        )
    }

    fn stable_target_origins(
        &self,
        read_expression: &StableOrderExpression,
        target: &OwnerLexicalTargetRef,
        substitutions: &ProjectAuthoritySubstitutions,
        fresh_site: Option<&StableOrderExpression>,
        callable_owner: Option<&StableCheckOwnerKey>,
        active: &mut BTreeSet<(StableOrderExpression, Option<StableOrderExpression>)>,
    ) -> Result<BTreeMap<ProjectAuthorityOriginKey, bool>, ProjectDiagnosticFactsError> {
        let OwnerLexicalTargetRef::Declaration {
            owner,
            declaration,
            capability,
        } = target
        else {
            return Ok(match target {
                OwnerLexicalTargetRef::ContextFormal { .. } => {
                    Self::borrowed_origin(read_expression)
                }
                OwnerLexicalTargetRef::Ambiguous { .. } => BTreeMap::new(),
                OwnerLexicalTargetRef::Declaration { .. } => unreachable!(),
            });
        };
        if matches!(capability, OwnerLexicalDeclarationCapability::CallableOnly) {
            return Ok(BTreeMap::new());
        }
        if let OwnerDeclarationStableKey::Parameter { ordinal } = declaration {
            if let Some(actual) = substitutions.get(&(owner.clone(), *ordinal)) {
                return self.origins_inner(actual, substitutions, None, None, active);
            }
            return Ok(Self::borrowed_origin(read_expression));
        }
        let value = match declaration {
            OwnerDeclarationStableKey::Public => self.index.public_result(owner),
            OwnerDeclarationStableKey::Statement { statement } => {
                self.index.stable_statement_value_ref(statement)
            }
            OwnerDeclarationStableKey::RecordField {
                object, ordinal, ..
            } => self.stable_record_field_value(owner, object, *ordinal),
            OwnerDeclarationStableKey::Parameter { .. }
            | OwnerDeclarationStableKey::PatternBinding { .. }
            | OwnerDeclarationStableKey::FreshOut { .. }
            | OwnerDeclarationStableKey::CallContext { .. } => None,
        };
        self.declaration_origins(
            read_expression,
            owner,
            value,
            substitutions,
            fresh_site,
            callable_owner,
            active,
        )
    }

    fn read_origins(
        &self,
        expression: &StableOrderExpression,
        substitutions: &ProjectAuthoritySubstitutions,
        fresh_site: Option<&StableOrderExpression>,
        callable_owner: Option<&StableCheckOwnerKey>,
        active: &mut BTreeSet<(StableOrderExpression, Option<StableOrderExpression>)>,
    ) -> Result<BTreeMap<ProjectAuthorityOriginKey, bool>, ProjectDiagnosticFactsError> {
        if let Some(read) = self.index.effective_read(expression) {
            return match &read.target {
                OwnerEffectiveLexicalTarget::Static { target } => match target {
                    OwnerLexicalDeclarationTarget::Parameter { ordinal } => {
                        if let Some(actual) =
                            substitutions.get(&(expression.owner.clone(), *ordinal))
                        {
                            self.origins_inner(actual, substitutions, None, None, active)
                        } else {
                            Ok(Self::borrowed_origin(expression))
                        }
                    }
                    OwnerLexicalDeclarationTarget::Statement { statement } => self
                        .declaration_origins(
                            expression,
                            &expression.owner,
                            self.index
                                .statement_value_ref(&expression.owner, *statement),
                            substitutions,
                            fresh_site,
                            callable_owner,
                            active,
                        ),
                    OwnerLexicalDeclarationTarget::RecordField {
                        object, ordinal, ..
                    } => self.declaration_origins(
                        expression,
                        &expression.owner,
                        self.record_field_value(&expression.owner, *object, *ordinal),
                        substitutions,
                        fresh_site,
                        callable_owner,
                        active,
                    ),
                    OwnerLexicalDeclarationTarget::Imported { target } => self
                        .stable_target_origins(
                            expression,
                            target,
                            substitutions,
                            fresh_site,
                            callable_owner,
                            active,
                        ),
                    OwnerLexicalDeclarationTarget::PatternBinding { .. }
                    | OwnerLexicalDeclarationTarget::Passed => {
                        Ok(Self::borrowed_origin(expression))
                    }
                    OwnerLexicalDeclarationTarget::Ambiguous { .. } => Ok(BTreeMap::new()),
                },
                OwnerEffectiveLexicalTarget::Imported { target } => self.stable_target_origins(
                    expression,
                    target,
                    substitutions,
                    fresh_site,
                    callable_owner,
                    active,
                ),
                OwnerEffectiveLexicalTarget::FreshOut { .. }
                | OwnerEffectiveLexicalTarget::CallContext { .. } => {
                    Ok(Self::borrowed_origin(expression))
                }
                OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                | OwnerEffectiveLexicalTarget::Ambiguous { .. } => Ok(BTreeMap::new()),
            };
        }
        match self.index.value_resolutions.get(expression).copied() {
            Some(crate::OwnerSymbolResolution::Resolved { owner, .. }) => self.declaration_origins(
                expression,
                owner,
                self.index.public_result(owner),
                substitutions,
                fresh_site,
                callable_owner,
                active,
            ),
            Some(crate::OwnerSymbolResolution::Authoritative { .. }) => {
                Ok(Self::borrowed_origin(expression))
            }
            Some(
                crate::OwnerSymbolResolution::Unresolved { .. }
                | crate::OwnerSymbolResolution::CallableAsValue { .. }
                | crate::OwnerSymbolResolution::Ambiguous { .. },
            )
            | None => Ok(BTreeMap::new()),
        }
    }

    fn call_input(
        &self,
        expression: &StableOrderExpression,
        name: &str,
    ) -> Option<StableOrderExpression> {
        let call = self.index.call(expression)?;
        let input = call
            .matched_inputs
            .iter()
            .find(|input| input.formal_name == name)?;
        Some(input.expression.clone())
    }

    fn pipe_input(&self, expression: &StableOrderExpression) -> Option<StableOrderExpression> {
        let call = self.index.call(expression)?;
        call.matched_inputs
            .iter()
            .find(|input| input.from_pipe)
            .map(|input| input.expression.clone())
    }

    fn call_value_inputs(
        &self,
        expression: &StableOrderExpression,
    ) -> Result<Vec<(u32, StableOrderExpression)>, ProjectDiagnosticFactsError> {
        let Some(call) = self.index.call(expression) else {
            return Err(ProjectDiagnosticFactsError::new(
                "valid collection authority call has no exact replay row",
            ));
        };
        call.matched_inputs
            .iter()
            .filter(|input| input.formal_kind == crate::OwnerParameterKind::Value)
            .map(|input| Ok((input.formal_ordinal, input.expression.clone())))
            .collect()
    }

    fn call_origins(
        &self,
        expression: &StableOrderExpression,
        substitutions: &ProjectAuthoritySubstitutions,
        fresh_site: Option<&StableOrderExpression>,
        callable_owner: Option<&StableCheckOwnerKey>,
        active: &mut BTreeSet<(StableOrderExpression, Option<StableOrderExpression>)>,
    ) -> Result<BTreeMap<ProjectAuthorityOriginKey, bool>, ProjectDiagnosticFactsError> {
        let Some(call) = self.index.call(expression) else {
            return Ok(BTreeMap::new());
        };
        let mut result = BTreeMap::new();
        match call.function.as_str() {
            "List/map" | "List/range" => {
                result.insert(
                    ProjectAuthorityOriginKey {
                        site: fresh_site.unwrap_or(expression).clone(),
                        constructor: expression.clone(),
                    },
                    false,
                );
            }
            "Map/upsert" | "Map/remove" | "Set/add" | "Set/remove" | "List/append"
            | "List/filter" | "List/retain" | "List/remove" | "List/sort_by" | "List/then_by"
            | "List/take" => {
                if let Some(receiver) = self.pipe_input(expression) {
                    Self::merge_origins(
                        &mut result,
                        self.origins_inner(
                            &receiver,
                            substitutions,
                            fresh_site,
                            callable_owner,
                            active,
                        )?,
                    );
                }
            }
            "Map/get" => {
                result.insert(
                    ProjectAuthorityOriginKey {
                        site: fresh_site.unwrap_or(expression).clone(),
                        constructor: expression.clone(),
                    },
                    true,
                );
            }
            function if is_registered_render_constructor(function) => {
                for (_, input) in self.call_value_inputs(expression)? {
                    Self::merge_origins(
                        &mut result,
                        self.origins_inner(
                            &input,
                            substitutions,
                            fresh_site,
                            callable_owner,
                            active,
                        )?,
                    );
                }
            }
            _ => match &call.target {
                crate::InferredOwnerCallableTarget::Owner { owner } => {
                    let Some(output) = self.index.public_result(owner) else {
                        return Ok(Self::borrowed_origin(expression));
                    };
                    let mut nested_substitutions = substitutions.clone();
                    for (ordinal, input) in self.call_value_inputs(expression)? {
                        nested_substitutions.insert((owner.clone(), ordinal), input);
                    }
                    Self::merge_origins(
                        &mut result,
                        self.origins_inner(
                            &output,
                            &nested_substitutions,
                            Some(fresh_site.unwrap_or(expression)),
                            Some(owner),
                            active,
                        )?,
                    );
                }
                crate::InferredOwnerCallableTarget::Authoritative => {
                    result = Self::borrowed_origin(expression);
                }
                crate::InferredOwnerCallableTarget::Unresolved
                | crate::InferredOwnerCallableTarget::Ambiguous { .. } => {
                    return Err(ProjectDiagnosticFactsError::new(
                        "valid collection authority call has an unresolved target",
                    ));
                }
            },
        }
        Ok(result)
    }

    fn origins_inner(
        &self,
        expression: &StableOrderExpression,
        substitutions: &ProjectAuthoritySubstitutions,
        fresh_site: Option<&StableOrderExpression>,
        callable_owner: Option<&StableCheckOwnerKey>,
        active: &mut BTreeSet<(StableOrderExpression, Option<StableOrderExpression>)>,
    ) -> Result<BTreeMap<ProjectAuthorityOriginKey, bool>, ProjectDiagnosticFactsError> {
        let Some((_, _, syntax, inferred)) = self.index.order_expression(expression) else {
            return Err(ProjectDiagnosticFactsError::new(
                "collection authority expression has no exact owner row",
            ));
        };
        let active_key = (expression.clone(), fresh_site.cloned());
        if !crate::type_may_contain_collection_authority(&inferred.flow_type.ty)
            || !active.insert(active_key.clone())
        {
            return Ok(BTreeMap::new());
        }
        let mut result = BTreeMap::new();
        match &syntax.kind {
            AstExprKind::ListLiteral { .. }
            | AstExprKind::MapLiteral { .. }
            | AstExprKind::SetLiteral { .. } => {
                result.insert(
                    ProjectAuthorityOriginKey {
                        site: fresh_site.unwrap_or(expression).clone(),
                        constructor: expression.clone(),
                    },
                    false,
                );
            }
            AstExprKind::Identifier(_) | AstExprKind::Path(_) => {
                Self::merge_origins(
                    &mut result,
                    self.read_origins(
                        expression,
                        substitutions,
                        fresh_site,
                        callable_owner,
                        active,
                    )?,
                );
            }
            AstExprKind::Drain { .. } => {
                result = Self::borrowed_origin(expression);
            }
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. }
                if self.index.call(expression).is_some() =>
            {
                Self::merge_origins(
                    &mut result,
                    self.call_origins(
                        expression,
                        substitutions,
                        fresh_site,
                        callable_owner,
                        active,
                    )?,
                );
            }
            AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
                for field in fields {
                    if let Some(value) = self.expression_ref(&expression.owner, field.value) {
                        Self::merge_origins(
                            &mut result,
                            self.origins_inner(
                                &value,
                                substitutions,
                                fresh_site,
                                callable_owner,
                                active,
                            )?,
                        );
                    }
                }
            }
            AstExprKind::Latest { branches } => {
                for branch in branches {
                    if let Some(branch) = self.expression_ref(&expression.owner, *branch) {
                        Self::merge_origins(
                            &mut result,
                            self.origins_inner(
                                &branch,
                                substitutions,
                                fresh_site,
                                callable_owner,
                                active,
                            )?,
                        );
                    }
                }
            }
            AstExprKind::When { arms, .. } | AstExprKind::Pipe { op: _, arms, .. }
                if !arms.is_empty() =>
            {
                for arm in arms {
                    if let Some(arm) = self.expression_ref(&expression.owner, *arm) {
                        Self::merge_origins(
                            &mut result,
                            self.origins_inner(
                                &arm,
                                substitutions,
                                fresh_site,
                                callable_owner,
                                active,
                            )?,
                        );
                    }
                }
            }
            AstExprKind::Then { output, .. } | AstExprKind::MatchArm { output, .. } => {
                if let Some(output) = output
                    && let Some(output) = self.expression_ref(&expression.owner, *output)
                {
                    Self::merge_origins(
                        &mut result,
                        self.origins_inner(
                            &output,
                            substitutions,
                            fresh_site,
                            callable_owner,
                            active,
                        )?,
                    );
                }
            }
            AstExprKind::Block { result: output, .. } => {
                if let Some(output) = output
                    && let Some(output) = self.expression_ref(&expression.owner, *output)
                {
                    Self::merge_origins(
                        &mut result,
                        self.origins_inner(
                            &output,
                            substitutions,
                            fresh_site,
                            callable_owner,
                            active,
                        )?,
                    );
                }
            }
            AstExprKind::Draining { input } | AstExprKind::Hold { initial: input, .. } => {
                if let Some(input) = self.expression_ref(&expression.owner, *input) {
                    Self::merge_origins(
                        &mut result,
                        self.origins_inner(
                            &input,
                            substitutions,
                            fresh_site,
                            callable_owner,
                            active,
                        )?,
                    );
                }
            }
            AstExprKind::MapEntry { value, .. } => {
                if let Some(value) = self.expression_ref(&expression.owner, *value) {
                    Self::merge_origins(
                        &mut result,
                        self.origins_inner(
                            &value,
                            substitutions,
                            fresh_site,
                            callable_owner,
                            active,
                        )?,
                    );
                }
            }
            AstExprKind::StringLiteral(_)
            | AstExprKind::TextLiteral(_)
            | AstExprKind::TextTemplate { .. }
            | AstExprKind::Number(_)
            | AstExprKind::ByteLiteral { .. }
            | AstExprKind::Tag(_)
            | AstExprKind::Flush { .. }
            | AstExprKind::Source
            | AstExprKind::Infix { .. }
            | AstExprKind::BytesLiteral { .. }
            | AstExprKind::BitsLiteral { .. }
            | AstExprKind::Delimiter
            | AstExprKind::Unknown(_)
            | AstExprKind::Arrow { .. }
            | AstExprKind::Call { .. }
            | AstExprKind::Pipe { .. }
            | AstExprKind::When { .. } => {}
        }
        active.remove(&active_key);
        Ok(result)
    }

    fn expression_children(
        &self,
        expression: &StableOrderExpression,
    ) -> Result<Vec<StableOrderExpression>, ProjectDiagnosticFactsError> {
        let Some((view, index, syntax, _)) = self.index.order_expression(expression) else {
            return Err(ProjectDiagnosticFactsError::new(
                "collection authority structural expression has no owner row",
            ));
        };
        if matches!(
            syntax.kind,
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. }
        ) && self.index.all_calls.contains_key(expression)
        {
            if self.index.call(expression).is_none() {
                return Ok(Vec::new());
            }
            return Ok(self
                .call_value_inputs(expression)?
                .into_iter()
                .map(|(_, input)| input)
                .collect());
        }
        view.graph
            .expression_inputs(crate::OwnerExpressionId(index as u32))
            .into_iter()
            .flatten()
            .map(|child| {
                self.owner_ref(&expression.owner, child).ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "collection authority structural child has no stable identity",
                    )
                })
            })
            .collect()
    }

    fn structurally_contains(
        &self,
        root: &StableOrderExpression,
        target: &StableOrderExpression,
    ) -> Result<bool, ProjectDiagnosticFactsError> {
        let mut pending = vec![root.clone()];
        let mut visited = BTreeSet::new();
        while let Some(expression) = pending.pop() {
            if &expression == target {
                return Ok(true);
            }
            if visited.insert(expression.clone()) {
                pending.extend(self.expression_children(&expression)?);
            }
        }
        Ok(false)
    }

    fn diagnostic(
        &self,
        expression: &StableOrderExpression,
        message: String,
    ) -> Result<TypeDiagnostic, ProjectDiagnosticFactsError> {
        self.index.owners.get(&expression.owner).ok_or_else(|| {
            ProjectDiagnosticFactsError::new(
                "collection authority diagnostic expression has no owner",
            )
        })?;
        let span = self.index.expression_span(expression)?;
        Ok(TypeDiagnostic {
            severity: DiagnosticSeverity::Error,
            line: span.line,
            start: span.start,
            end: span.end,
            message,
        })
    }
}

fn project_authority_graph_reaches(
    graph: &BTreeMap<ProjectAuthorityOriginKey, BTreeSet<ProjectAuthorityOriginKey>>,
    root: &ProjectAuthorityOriginKey,
    target: &ProjectAuthorityOriginKey,
) -> bool {
    let mut pending = vec![root.clone()];
    let mut visited = BTreeSet::new();
    while let Some(origin) = pending.pop() {
        if &origin == target {
            return true;
        }
        if visited.insert(origin.clone())
            && let Some(parents) = graph.get(&origin)
        {
            pending.extend(parents.iter().cloned());
        }
    }
    false
}

fn attach_project_authority(
    analyzer: &ProjectAuthorityAnalyzer<'_, '_>,
    root: &StableOrderExpression,
    parent: ProjectAuthorityParentSite,
    parents: &mut BTreeMap<ProjectAuthorityOriginKey, BTreeSet<ProjectAuthorityParentSite>>,
    graph: &mut BTreeMap<ProjectAuthorityOriginKey, BTreeSet<ProjectAuthorityOriginKey>>,
    seen_diagnostics: &mut BTreeSet<(StableOrderExpression, String)>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<(), ProjectDiagnosticFactsError> {
    for (origin, borrowed) in analyzer.origins(root)? {
        let message = if origin == parent.authority
            || project_authority_graph_reaches(graph, &parent.authority, &origin)
        {
            Some(
                "nested collection authority attachment forms an ownership cycle; construct nested LIST, SET, or MAP authorities only inside an acyclic parent value"
                    .to_owned(),
            )
        } else if borrowed {
            Some(
                "nested collection authority escapes its owner or is reattached from an existing owner; construct a fresh LIST, SET, or MAP authority inside this parent value"
                    .to_owned(),
            )
        } else if !analyzer.structurally_contains(root, &origin.site)? {
            Some(
                "collection authority is attached beneath a second parent or beyond its owner lifetime; construct it inside this MAP value or LIST occurrence"
                    .to_owned(),
            )
        } else {
            None
        };
        if let Some(message) = message
            && seen_diagnostics.insert((root.clone(), message.clone()))
        {
            diagnostics.push(analyzer.diagnostic(root, message)?);
        }

        let origin_parents = parents.entry(origin.clone()).or_default();
        origin_parents.insert(parent.clone());
        if origin_parents.len() > 1 {
            let message =
                "collection authority has more than one structural parent; each nested authority must belong to exactly one MAP key or LIST occurrence"
                    .to_owned();
            if seen_diagnostics.insert((root.clone(), message.clone())) {
                diagnostics.push(analyzer.diagnostic(root, message)?);
            }
        }
        graph
            .entry(origin)
            .or_default()
            .insert(parent.authority.clone());
    }
    Ok(())
}

fn collection_authority_diagnostics(
    index: &ProjectFactIndex<'_>,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    let analyzer = ProjectAuthorityAnalyzer::new(index);
    let mut diagnostics = Vec::new();
    let mut seen_diagnostics = BTreeSet::<(StableOrderExpression, String)>::new();
    let mut parents =
        BTreeMap::<ProjectAuthorityOriginKey, BTreeSet<ProjectAuthorityParentSite>>::new();
    let mut graph =
        BTreeMap::<ProjectAuthorityOriginKey, BTreeSet<ProjectAuthorityOriginKey>>::new();
    let mut expressions = index
        .syntax_expressions
        .iter()
        .map(|(expression, syntax_id)| (*syntax_id, expression.clone()))
        .collect::<Vec<_>>();
    expressions.sort_by_key(|(syntax_id, _)| *syntax_id);

    for (_, expression) in expressions {
        let Some((_, _, syntax, _)) = index.order_expression(&expression) else {
            return Err(ProjectDiagnosticFactsError::new(
                "collection authority syntax expression has no owner row",
            ));
        };
        match &syntax.kind {
            AstExprKind::MapLiteral { entries } => {
                let authority = ProjectAuthorityOriginKey {
                    site: expression.clone(),
                    constructor: expression.clone(),
                };
                for entry in entries {
                    let Some(entry) = analyzer.expression_ref(&expression.owner, *entry) else {
                        return Err(ProjectDiagnosticFactsError::new(
                            "collection authority MAP entry has no stable expression",
                        ));
                    };
                    let Some((_, _, entry_syntax, _)) = index.order_expression(&entry) else {
                        return Err(ProjectDiagnosticFactsError::new(
                            "collection authority MAP entry has no owner row",
                        ));
                    };
                    let AstExprKind::MapEntry { value, .. } = &entry_syntax.kind else {
                        continue;
                    };
                    let Some(value) = analyzer.expression_ref(&entry.owner, *value) else {
                        return Err(ProjectDiagnosticFactsError::new(
                            "collection authority MAP value has no stable expression",
                        ));
                    };
                    attach_project_authority(
                        &analyzer,
                        &value,
                        ProjectAuthorityParentSite {
                            kind: ProjectAuthorityParentKind::MapKey,
                            authority: authority.clone(),
                            attachment: entry,
                        },
                        &mut parents,
                        &mut graph,
                        &mut seen_diagnostics,
                        &mut diagnostics,
                    )?;
                }
            }
            AstExprKind::ListLiteral { items, .. } => {
                let authority = ProjectAuthorityOriginKey {
                    site: expression.clone(),
                    constructor: expression.clone(),
                };
                for item in items {
                    let Some(item) = analyzer.expression_ref(&expression.owner, *item) else {
                        return Err(ProjectDiagnosticFactsError::new(
                            "collection authority LIST item has no stable expression",
                        ));
                    };
                    attach_project_authority(
                        &analyzer,
                        &item,
                        ProjectAuthorityParentSite {
                            kind: ProjectAuthorityParentKind::ListOccurrence,
                            authority: authority.clone(),
                            attachment: item.clone(),
                        },
                        &mut parents,
                        &mut graph,
                        &mut seen_diagnostics,
                        &mut diagnostics,
                    )?;
                }
            }
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                let Some(call) = index.call(&expression) else {
                    continue;
                };
                let Some(receiver) = analyzer.pipe_input(&expression) else {
                    continue;
                };
                let attachment = match call.function.as_str() {
                    "Map/upsert" => analyzer
                        .call_input(&expression, "entry")
                        .map(|root| (ProjectAuthorityParentKind::MapKey, root)),
                    "List/append" => analyzer
                        .call_input(&expression, "item")
                        .map(|root| (ProjectAuthorityParentKind::ListOccurrence, root)),
                    _ => None,
                };
                let Some((kind, root)) = attachment else {
                    continue;
                };
                for authority in analyzer.origins(&receiver)?.keys().cloned() {
                    attach_project_authority(
                        &analyzer,
                        &root,
                        ProjectAuthorityParentSite {
                            kind,
                            authority,
                            attachment: expression.clone(),
                        },
                        &mut parents,
                        &mut graph,
                        &mut seen_diagnostics,
                        &mut diagnostics,
                    )?;
                }
            }
            _ => {}
        }
    }
    Ok(diagnostics)
}

#[derive(Clone)]
struct ProjectOutputInfo {
    label: String,
    cycle_label: String,
    site: ProjectOutputDiagnosticSite,
}

#[derive(Clone)]
struct ProjectOutputEdge {
    source: Option<ProjectOutputTargetFact>,
    target: ProjectOutputTargetFact,
    driver: ProjectOutputDriverFact,
    forwarding: bool,
}

fn imported_output_target(target: &OwnerLexicalTargetRef) -> Option<ProjectOutputTargetFact> {
    let OwnerLexicalTargetRef::Declaration {
        owner,
        declaration,
        capability: OwnerLexicalDeclarationCapability::Out { .. },
    } = target
    else {
        return None;
    };
    match declaration {
        OwnerDeclarationStableKey::Parameter { ordinal } => {
            Some(ProjectOutputTargetFact::Parameter {
                owner: owner.clone(),
                ordinal: *ordinal,
            })
        }
        OwnerDeclarationStableKey::FreshOut {
            call,
            formal_ordinal,
        } => Some(ProjectOutputTargetFact::Fresh {
            owner: owner.clone(),
            call: call.clone(),
            formal_ordinal: *formal_ordinal,
        }),
        OwnerDeclarationStableKey::Public
        | OwnerDeclarationStableKey::Statement { .. }
        | OwnerDeclarationStableKey::RecordField { .. }
        | OwnerDeclarationStableKey::PatternBinding { .. }
        | OwnerDeclarationStableKey::CallContext { .. } => None,
    }
}

fn effective_output_target(
    owner: &StableCheckOwnerKey,
    target: &OwnerEffectiveLexicalTarget,
) -> Option<ProjectOutputTargetFact> {
    match target {
        OwnerEffectiveLexicalTarget::Static {
            target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
        } => Some(ProjectOutputTargetFact::Parameter {
            owner: owner.clone(),
            ordinal: *ordinal,
        }),
        OwnerEffectiveLexicalTarget::Static {
            target: OwnerLexicalDeclarationTarget::Imported { target },
        }
        | OwnerEffectiveLexicalTarget::Imported { target } => imported_output_target(target),
        OwnerEffectiveLexicalTarget::FreshOut {
            call,
            formal_ordinal,
        } => Some(ProjectOutputTargetFact::Fresh {
            owner: owner.clone(),
            call: call.clone(),
            formal_ordinal: *formal_ordinal,
        }),
        OwnerEffectiveLexicalTarget::Static { .. }
        | OwnerEffectiveLexicalTarget::CallContext { .. }
        | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
        | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
    }
}

fn collect_output_cycle_nodes(
    graph: &BTreeMap<ProjectOutputTargetFact, BTreeSet<ProjectOutputTargetFact>>,
) -> BTreeSet<ProjectOutputTargetFact> {
    fn visit(
        node: &ProjectOutputTargetFact,
        graph: &BTreeMap<ProjectOutputTargetFact, BTreeSet<ProjectOutputTargetFact>>,
        visited: &mut BTreeSet<ProjectOutputTargetFact>,
        active: &mut Vec<ProjectOutputTargetFact>,
        active_indices: &mut BTreeMap<ProjectOutputTargetFact, usize>,
        cycles: &mut BTreeSet<ProjectOutputTargetFact>,
    ) {
        if let Some(start) = active_indices.get(node).copied() {
            cycles.extend(active[start..].iter().cloned());
            return;
        }
        if !visited.insert(node.clone()) {
            return;
        }
        active_indices.insert(node.clone(), active.len());
        active.push(node.clone());
        if let Some(next) = graph.get(node) {
            for next in next {
                visit(next, graph, visited, active, active_indices, cycles);
            }
        }
        active.pop();
        active_indices.remove(node);
    }

    let mut visited = BTreeSet::new();
    let mut active = Vec::new();
    let mut active_indices = BTreeMap::new();
    let mut cycles = BTreeSet::new();
    for node in graph.keys() {
        visit(
            node,
            graph,
            &mut visited,
            &mut active,
            &mut active_indices,
            &mut cycles,
        );
    }
    cycles
}

fn contextual_list_and_row(operation: crate::OwnerAbiContextualOperation) -> (u32, u32) {
    match operation {
        crate::OwnerAbiContextualOperation::Map { list, row, .. }
        | crate::OwnerAbiContextualOperation::Filter { list, row, .. }
        | crate::OwnerAbiContextualOperation::Retain { list, row, .. }
        | crate::OwnerAbiContextualOperation::Remove { list, row, .. }
        | crate::OwnerAbiContextualOperation::Every { list, row, .. }
        | crate::OwnerAbiContextualOperation::Any { list, row, .. }
        | crate::OwnerAbiContextualOperation::Find { list, row, .. }
        | crate::OwnerAbiContextualOperation::SortBy { list, row, .. }
        | crate::OwnerAbiContextualOperation::ThenBy { list, row, .. } => (list, row),
    }
}

pub fn project_output_flow_facts<'a>(
    abi: &OwnerAbiEnvironment,
    expected_owners: impl IntoIterator<Item = &'a StableCheckOwnerKey>,
    replay_facts: impl IntoIterator<Item = &'a OwnerDiagnosticReplayFacts>,
) -> Result<ProjectOutputFlowFacts, ProjectDiagnosticFactsError> {
    let expected_owners = expected_owners.into_iter().collect::<Vec<_>>();
    let unique_owners = expected_owners
        .iter()
        .copied()
        .cloned()
        .collect::<BTreeSet<_>>();
    if unique_owners.len() != expected_owners.len() {
        return Err(ProjectDiagnosticFactsError::new(
            "project output flow owners contain a duplicate",
        ));
    }
    let owners = unique_owners.into_iter().collect::<Vec<_>>();
    let replay_facts = unique_by_owner(
        replay_facts.into_iter().map(|facts| (facts.owner(), facts)),
        "output-flow replay facts",
    )?;
    if replay_facts.keys().ne(owners.iter()) {
        return Err(ProjectDiagnosticFactsError::new(
            "project output flow replay coverage differs from the project owner set",
        ));
    }

    let mut outputs = BTreeMap::<ProjectOutputTargetFact, ProjectOutputInfo>::new();
    let mut edges = Vec::new();
    let mut list_sources =
        BTreeMap::<ProjectOutputTargetFact, BTreeSet<ProjectOrderExpressionFact>>::new();
    let mut forward_sources =
        BTreeMap::<ProjectOutputTargetFact, BTreeSet<ProjectOutputTargetFact>>::new();

    for owner in &owners {
        let replay = replay_facts[owner];
        for declaration in &replay.output_declarations {
            if !matches!(
                &declaration.target,
                ProjectOutputTargetFact::Parameter { owner: target_owner, ordinal }
                    if target_owner == owner && *ordinal == declaration.ordinal
            ) {
                return Err(ProjectDiagnosticFactsError::new(
                    "owner output declaration has a foreign or mismatched target",
                ));
            }
            let function_name = replay.function_name.as_deref().ok_or_else(|| {
                ProjectDiagnosticFactsError::new("OUT declaration has no owning function")
            })?;
            if outputs
                .insert(
                    declaration.target.clone(),
                    ProjectOutputInfo {
                        label: format!(
                            "output `{}` in `FUNCTION {function_name}`",
                            declaration.name
                        ),
                        cycle_label: format!("output `{}`", declaration.name),
                        site: ProjectOutputDiagnosticSite::FunctionParameter {
                            owner: owner.clone(),
                            statement: declaration.statement.clone(),
                            ordinal: declaration.ordinal,
                        },
                    },
                )
                .is_some()
            {
                return Err(ProjectDiagnosticFactsError::new(
                    "project diagnostics received a duplicate user OUT target",
                ));
            }
        }

        for call in &replay.output_calls {
            for output in &call.outputs {
                let source = match &call.target {
                    OwnerDiagnosticOutputCallTargetFact::Owner { owner: callee } => {
                        let formal = ProjectOutputTargetFact::Parameter {
                            owner: callee.clone(),
                            ordinal: output.formal_ordinal,
                        };
                        forward_sources
                            .entry(output.target.clone())
                            .or_default()
                            .insert(formal.clone());
                        forward_sources
                            .entry(formal.clone())
                            .or_default()
                            .insert(output.target.clone());
                        Some(formal)
                    }
                    OwnerDiagnosticOutputCallTargetFact::Abi { function, kind } => {
                        let contract = abi.callable(function).ok_or_else(|| {
                            ProjectDiagnosticFactsError::new(
                                "valid authoritative OUT call has no exact callable contract",
                            )
                        })?;
                        if contract.kind != *kind {
                            return Err(ProjectDiagnosticFactsError::new(
                                "owner output-flow ABI kind differs from its replay fact",
                            ));
                        }
                        if *kind != CheckedCallableKind::Builtin {
                            return Err(ProjectDiagnosticFactsError::new(
                                "authoritative external OUT cannot seed the project producer graph",
                            ));
                        }
                        if let Some(operation) = contract.contextual_operation {
                            let (list, row) = contextual_list_and_row(operation);
                            if row == output.formal_ordinal {
                                let input = call
                                    .inputs
                                    .iter()
                                    .find(|input| input.formal_ordinal == list)
                                    .ok_or_else(|| {
                                        ProjectDiagnosticFactsError::new(
                                            "contextual flow-mode call has no exact list input",
                                        )
                                    })?;
                                list_sources
                                    .entry(output.target.clone())
                                    .or_default()
                                    .insert(input.expression.clone());
                            }
                        }
                        None
                    }
                };
                if let Some(name) = &output.fresh_name {
                    outputs
                        .entry(output.target.clone())
                        .or_insert_with(|| ProjectOutputInfo {
                            label: format!("fresh output `{name}`"),
                            cycle_label: format!("fresh output `{name}"),
                            site: ProjectOutputDiagnosticSite::Expression {
                                owner: owner.clone(),
                                expression: call.expression.clone(),
                            },
                        });
                }
                edges.push(ProjectOutputEdge {
                    source,
                    target: output.target.clone(),
                    driver: ProjectOutputDriverFact {
                        owner: owner.clone(),
                        call: call.expression.clone(),
                        formal_ordinal: output.formal_ordinal,
                    },
                    forwarding: output.forwarding,
                });
            }
        }
    }

    for edge in &edges {
        if !outputs.contains_key(&edge.target)
            || edge
                .source
                .as_ref()
                .is_some_and(|source| !outputs.contains_key(source))
        {
            return Err(ProjectDiagnosticFactsError::new(
                "OUT producer graph references an undeclared output target",
            ));
        }
    }

    let mut driven = edges
        .iter()
        .filter(|edge| edge.source.is_none())
        .map(|edge| edge.target.clone())
        .collect::<BTreeSet<_>>();
    let mut dependents = BTreeMap::<ProjectOutputTargetFact, Vec<ProjectOutputTargetFact>>::new();
    for edge in &edges {
        if let Some(source) = &edge.source {
            dependents
                .entry(source.clone())
                .or_default()
                .push(edge.target.clone());
        }
    }
    let mut pending = driven.iter().cloned().collect::<VecDeque<_>>();
    while let Some(source) = pending.pop_front() {
        for target in dependents.get(&source).into_iter().flatten() {
            if driven.insert(target.clone()) {
                pending.push_back(target.clone());
            }
        }
    }

    let mut drivers = BTreeMap::<ProjectOutputTargetFact, BTreeSet<ProjectOutputDriverFact>>::new();
    for edge in &edges {
        if edge.source.is_none()
            || edge
                .source
                .as_ref()
                .is_some_and(|source| driven.contains(source))
        {
            drivers
                .entry(edge.target.clone())
                .or_default()
                .insert(edge.driver.clone());
        }
    }

    let mut cycle_graph =
        BTreeMap::<ProjectOutputTargetFact, BTreeSet<ProjectOutputTargetFact>>::new();
    for edge in edges.iter().filter(|edge| edge.forwarding) {
        if let Some(source) = &edge.source {
            cycle_graph
                .entry(edge.target.clone())
                .or_default()
                .insert(source.clone());
        }
    }
    let cycle_nodes = collect_output_cycle_nodes(&cycle_graph);

    let mut facts = Vec::with_capacity(outputs.len());
    let mut diagnostics = Vec::new();
    for (target, info) in outputs {
        let target_drivers = drivers.remove(&target).unwrap_or_default();
        let count = target_drivers.len();
        if count != 1 {
            diagnostics.push(ProjectOutputDiagnosticTemplate {
                site: info.site.clone(),
                message: if count == 0 {
                    format!("{} has no structural producer", info.label)
                } else {
                    format!(
                        "{} has {count} structural producers; exactly one is required",
                        info.label
                    )
                },
            });
        }
        if cycle_nodes.contains(&target) {
            diagnostics.push(ProjectOutputDiagnosticTemplate {
                site: info.site,
                message: format!(
                    "{} participates in an OUT forwarding cycle",
                    info.cycle_label
                ),
            });
        }
        facts.push(ProjectOutputProducerFact {
            target,
            drivers: target_drivers
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        });
    }
    let list_sources = list_sources
        .into_iter()
        .map(|(target, sources)| {
            (
                target,
                sources.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let forward_sources = forward_sources
        .into_iter()
        .map(|(target, sources)| {
            (
                target,
                sources.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        PROJECT_OUTPUT_FLOW_FACTS_DOMAIN_V1,
        &(
            &owners,
            &facts,
            &list_sources,
            &forward_sources,
            &diagnostics,
        ),
    )
    .map_err(|error| {
        ProjectDiagnosticFactsError::new(format!(
            "cannot fingerprint project output flow facts: {error}"
        ))
    })?;
    Ok(ProjectOutputFlowFacts {
        owners: owners.into_boxed_slice(),
        producers: facts.into_boxed_slice(),
        list_sources,
        forward_sources,
        diagnostics: diagnostics.into_boxed_slice(),
        fingerprint_v1,
    })
}

fn output_flow_diagnostics(
    index: &ProjectFactIndex<'_>,
    output_flow: &ProjectOutputFlowFacts,
) -> Result<Vec<TypeDiagnostic>, ProjectDiagnosticFactsError> {
    let mut diagnostics = Vec::with_capacity(output_flow.diagnostics.len());
    for template in &output_flow.diagnostics {
        let span = match &template.site {
            ProjectOutputDiagnosticSite::FunctionParameter {
                owner,
                statement,
                ordinal,
            } => {
                let (site_owner, statement) = index.statements.get(statement).ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "OUT diagnostic parameter site has no stable statement",
                    )
                })?;
                if site_owner != owner {
                    return Err(ProjectDiagnosticFactsError::new(
                        "OUT diagnostic parameter site belongs to a foreign owner",
                    ));
                }
                let view = index.owners.get(owner).ok_or_else(|| {
                    ProjectDiagnosticFactsError::new(
                        "OUT diagnostic parameter site has no owner facts",
                    )
                })?;
                global_anchor_span(
                    index.project,
                    view.source_map,
                    &OwnerSourceAnchorSite::Statement {
                        statement: *statement,
                    },
                    OwnerSourceAnchorRole::FunctionParameter { ordinal: *ordinal },
                )?
            }
            ProjectOutputDiagnosticSite::Expression { owner, expression } => index
                .expression_span(&StableOrderExpression {
                    owner: owner.clone(),
                    expression: expression.clone(),
                })?,
        };
        diagnostics.push(TypeDiagnostic {
            severity: DiagnosticSeverity::Error,
            line: span.line,
            start: span.start,
            end: span.end,
            message: template.message.clone(),
        });
    }
    Ok(diagnostics)
}

#[derive(Clone)]
struct ProjectOrderFrame {
    callable_owner: StableCheckOwnerKey,
    call: StableOrderExpression,
    bindings: BTreeMap<u32, StableOrderExpression>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectOrderKey {
    call_path: Vec<StableOrderExpression>,
    expression: StableOrderExpression,
    direction: ProjectOrderDirectionFact,
    key_type: Type,
    pure: bool,
    total: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectOrderSemanticKey {
    key: Option<ProjectOrderSemanticExpression>,
    direction: ProjectOrderSemanticDirection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProjectOrderSemanticDirection {
    Ascending,
    Descending,
    Dynamic(Option<ProjectOrderSemanticExpression>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProjectOrderSemanticExpression {
    Row(Vec<String>),
    Capture {
        target: ProjectOrderCaptureTarget,
        projection: Vec<String>,
    },
    Project {
        input: Box<ProjectOrderSemanticExpression>,
        fields: Vec<String>,
    },
    Text(String),
    TextTemplate(Vec<ProjectOrderSemanticTextSegment>),
    Number(ExactNumber),
    Bits(Bits),
    Tag(String),
    Call {
        function: String,
        inputs: Vec<(String, ProjectOrderSemanticExpression)>,
    },
    Infix {
        operator: String,
        left: Box<ProjectOrderSemanticExpression>,
        right: Box<ProjectOrderSemanticExpression>,
    },
    Select {
        input: Box<ProjectOrderSemanticExpression>,
        outputs: Vec<ProjectOrderSemanticExpression>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProjectOrderCaptureTarget {
    Declaration {
        owner: StableCheckOwnerKey,
        declaration: OwnerDeclarationStableKey,
    },
    Authoritative(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProjectOrderSemanticTextSegment {
    Static(String),
    Dynamic(ProjectOrderSemanticExpression),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectOrderChain {
    keys: Vec<ProjectOrderKey>,
    semantic: Vec<ProjectOrderSemanticKey>,
}

#[derive(Clone)]
enum ProjectOrderState {
    Ordered(ProjectOrderChain),
    Unordered,
    Deferred,
    Invalid {
        call_path: Vec<StableOrderExpression>,
    },
}

struct ProjectOrderAnalyzer<'index, 'project> {
    index: &'index ProjectFactIndex<'project>,
    states: BTreeMap<(StableOrderExpression, Vec<StableOrderExpression>), ProjectOrderState>,
}

impl<'index, 'project> ProjectOrderAnalyzer<'index, 'project> {
    fn new(index: &'index ProjectFactIndex<'project>) -> Self {
        Self {
            index,
            states: BTreeMap::new(),
        }
    }

    fn expression_ref(
        &self,
        owner: &StableCheckOwnerKey,
        reference: usize,
    ) -> Option<StableOrderExpression> {
        self.index
            .stable_expression_ref(owner, u32::try_from(reference).ok()?)
    }

    fn owner_ref(
        &self,
        owner: &StableCheckOwnerKey,
        expression: &OwnerExpressionRef,
    ) -> Option<StableOrderExpression> {
        match expression {
            OwnerExpressionRef::Local { expression } => {
                self.index.stable_expression_ref(owner, expression.0)
            }
            OwnerExpressionRef::Child { owner, expression } => Some(StableOrderExpression {
                owner: owner.clone(),
                expression: expression.clone(),
            }),
        }
    }

    fn record_field_value(
        &self,
        object: &StableOrderExpression,
        ordinal: u32,
    ) -> Option<StableOrderExpression> {
        let (_, _, syntax, _) = self.index.order_expression(object)?;
        let fields = match &syntax.kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => fields,
            _ => return None,
        };
        self.expression_ref(&object.owner, fields.get(ordinal as usize)?.value)
    }

    fn stable_target_value(
        &self,
        target: &OwnerLexicalTargetRef,
        frames: &[ProjectOrderFrame],
    ) -> Option<(StableOrderExpression, usize)> {
        let OwnerLexicalTargetRef::Declaration {
            owner, declaration, ..
        } = target
        else {
            return None;
        };
        match declaration {
            OwnerDeclarationStableKey::Public => self
                .index
                .public_result(owner)
                .map(|value| (value, frames.len())),
            OwnerDeclarationStableKey::Parameter { ordinal } => {
                frames.iter().enumerate().rev().find_map(|(index, frame)| {
                    (&frame.callable_owner == owner)
                        .then(|| {
                            frame
                                .bindings
                                .get(ordinal)
                                .cloned()
                                .map(|value| (value, index))
                        })
                        .flatten()
                })
            }
            OwnerDeclarationStableKey::Statement { statement } => self
                .index
                .stable_statement_value_ref(statement)
                .map(|value| (value, frames.len())),
            OwnerDeclarationStableKey::RecordField {
                object, ordinal, ..
            } => self
                .record_field_value(
                    &StableOrderExpression {
                        owner: owner.clone(),
                        expression: object.clone(),
                    },
                    *ordinal,
                )
                .map(|value| (value, frames.len())),
            OwnerDeclarationStableKey::PatternBinding { .. }
            | OwnerDeclarationStableKey::FreshOut { .. }
            | OwnerDeclarationStableKey::CallContext { .. } => None,
        }
    }

    fn read_value(
        &self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
    ) -> Option<(StableOrderExpression, usize, bool)> {
        if let Some(read) = self.index.effective_read(expression) {
            let projection_is_empty = read.projection.is_empty();
            let value = match &read.target {
                crate::OwnerEffectiveLexicalTarget::Static { target } => match target {
                    crate::OwnerLexicalDeclarationTarget::Parameter { ordinal } => {
                        frames.iter().enumerate().rev().find_map(|(index, frame)| {
                            (frame.callable_owner == expression.owner)
                                .then(|| {
                                    frame
                                        .bindings
                                        .get(ordinal)
                                        .cloned()
                                        .map(|value| (value, index))
                                })
                                .flatten()
                        })
                    }
                    crate::OwnerLexicalDeclarationTarget::Statement { statement } => self
                        .index
                        .statement_value_ref(&expression.owner, *statement)
                        .map(|value| (value, frames.len())),
                    crate::OwnerLexicalDeclarationTarget::RecordField {
                        object, ordinal, ..
                    } => self
                        .index
                        .stable_expression_ref(&expression.owner, *object)
                        .and_then(|object| self.record_field_value(&object, *ordinal))
                        .map(|value| (value, frames.len())),
                    crate::OwnerLexicalDeclarationTarget::Imported { target } => {
                        self.stable_target_value(target, frames)
                    }
                    crate::OwnerLexicalDeclarationTarget::PatternBinding { .. }
                    | crate::OwnerLexicalDeclarationTarget::Passed
                    | crate::OwnerLexicalDeclarationTarget::Ambiguous { .. } => None,
                },
                crate::OwnerEffectiveLexicalTarget::Imported { target } => {
                    self.stable_target_value(target, frames)
                }
                crate::OwnerEffectiveLexicalTarget::FreshOut { .. }
                | crate::OwnerEffectiveLexicalTarget::CallContext { .. }
                | crate::OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                | crate::OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
            }?;
            return Some((value.0, value.1, projection_is_empty));
        }
        match self.index.value_resolutions.get(expression).copied()? {
            crate::OwnerSymbolResolution::Resolved {
                owner, projection, ..
            } => self
                .index
                .public_result(owner)
                .map(|value| (value, frames.len(), projection.is_empty())),
            crate::OwnerSymbolResolution::Authoritative { .. }
            | crate::OwnerSymbolResolution::Unresolved { .. }
            | crate::OwnerSymbolResolution::CallableAsValue { .. }
            | crate::OwnerSymbolResolution::Ambiguous { .. } => None,
        }
    }

    fn call_input(
        &self,
        call_expression: &StableOrderExpression,
        name: &str,
    ) -> Option<StableOrderExpression> {
        let call = self.index.call(call_expression)?;
        let input = call
            .matched_inputs
            .iter()
            .find(|input| input.formal_name == name)?;
        Some(input.expression.clone())
    }

    fn call_frame(
        &self,
        call_expression: &StableOrderExpression,
        target: &StableCheckOwnerKey,
    ) -> Option<ProjectOrderFrame> {
        let call = self.index.call(call_expression)?;
        let bindings = call
            .matched_inputs
            .iter()
            .filter(|input| input.formal_kind == crate::OwnerParameterKind::Value)
            .map(|input| (input.formal_ordinal, input.expression.clone()))
            .collect();
        Some(ProjectOrderFrame {
            callable_owner: target.clone(),
            call: call_expression.clone(),
            bindings,
        })
    }

    fn read_is_unbound_value_parameter(&self, expression: &StableOrderExpression) -> bool {
        let Some(read) = self.index.effective_read(expression) else {
            return false;
        };
        match &read.target {
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
            } => self
                .index
                .owners
                .get(&expression.owner)
                .and_then(|view| {
                    view.interface
                        .parameters
                        .iter()
                        .find(|parameter| parameter.ordinal == *ordinal)
                })
                .is_some_and(|parameter| parameter.kind == crate::OwnerParameterKind::Value),
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Imported { target },
            }
            | OwnerEffectiveLexicalTarget::Imported { target } => matches!(
                target,
                OwnerLexicalTargetRef::Declaration {
                    declaration: OwnerDeclarationStableKey::Parameter { .. },
                    capability: OwnerLexicalDeclarationCapability::Value,
                    ..
                }
            ),
            _ => false,
        }
    }

    fn expression_state(
        &mut self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> ProjectOrderState {
        let frame_path = frames
            .iter()
            .map(|frame| frame.call.clone())
            .collect::<Vec<_>>();
        let cache_key = (expression.clone(), frame_path.clone());
        if let Some(state) = self.states.get(&cache_key) {
            return state.clone();
        }
        if !active.insert((expression.clone(), frame_path.clone())) {
            return ProjectOrderState::Deferred;
        }
        let result = self.index.order_expression(expression).map_or(
            ProjectOrderState::Unordered,
            |(_, _, syntax, _)| match &syntax.kind {
                AstExprKind::Pipe { op, arms, .. } if op == "WHILE" => {
                    self.merge_branch_states(&expression.owner, arms, frames, active)
                }
                AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                    self.call_state(expression, frames, active)
                }
                AstExprKind::Identifier(_) | AstExprKind::Path(_) => {
                    match self.read_value(expression, frames) {
                        Some((value, frame_count, true)) => {
                            self.expression_state(&value, &frames[..frame_count], active)
                        }
                        Some((_, _, false)) => ProjectOrderState::Unordered,
                        None if self.read_is_unbound_value_parameter(expression) => {
                            ProjectOrderState::Deferred
                        }
                        None => ProjectOrderState::Unordered,
                    }
                }
                AstExprKind::Block {
                    result: Some(result),
                    ..
                }
                | AstExprKind::Then {
                    output: Some(result),
                    ..
                }
                | AstExprKind::MatchArm {
                    output: Some(result),
                    ..
                } => self
                    .expression_ref(&expression.owner, *result)
                    .map_or(ProjectOrderState::Unordered, |result| {
                        self.expression_state(&result, frames, active)
                    }),
                AstExprKind::Latest { branches } => {
                    self.merge_branch_states(&expression.owner, branches, frames, active)
                }
                AstExprKind::When { arms, .. } => {
                    self.merge_branch_states(&expression.owner, arms, frames, active)
                }
                _ => ProjectOrderState::Unordered,
            },
        );
        active.remove(&(expression.clone(), frame_path));
        self.states.insert(cache_key, result.clone());
        result
    }

    fn call_state(
        &mut self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> ProjectOrderState {
        let Some(call) = self.index.call(expression) else {
            return ProjectOrderState::Unordered;
        };
        if !type_may_be_ordered_list(&call.result.ty) {
            return ProjectOrderState::Unordered;
        }
        match call.function.as_str() {
            "List/sort_by" => {
                let Some(key) = self.call_input(expression, "key") else {
                    return ProjectOrderState::Unordered;
                };
                let (key, semantic) = self.order_key(expression, &key, frames);
                ProjectOrderState::Ordered(ProjectOrderChain {
                    keys: vec![key],
                    semantic: vec![semantic],
                })
            }
            "List/then_by" => {
                let Some(list) = self.call_input(expression, "list") else {
                    return ProjectOrderState::Unordered;
                };
                match self.expression_state(&list, frames, active) {
                    ProjectOrderState::Ordered(mut chain) => {
                        let Some(key) = self.call_input(expression, "key") else {
                            return ProjectOrderState::Unordered;
                        };
                        let (key, semantic) = self.order_key(expression, &key, frames);
                        chain.keys.push(key);
                        chain.semantic.push(semantic);
                        ProjectOrderState::Ordered(chain)
                    }
                    ProjectOrderState::Deferred => ProjectOrderState::Deferred,
                    ProjectOrderState::Invalid { call_path } => {
                        ProjectOrderState::Invalid { call_path }
                    }
                    ProjectOrderState::Unordered => ProjectOrderState::Invalid {
                        call_path: frames
                            .iter()
                            .map(|frame| frame.call.clone())
                            .chain(std::iter::once(expression.clone()))
                            .collect(),
                    },
                }
            }
            "List/filter" | "List/retain" | "List/remove" | "List/map" | "List/take"
            | "List/page" => self
                .call_input(expression, "list")
                .map_or(ProjectOrderState::Unordered, |list| {
                    self.expression_state(&list, frames, active)
                }),
            _ => {
                let crate::InferredOwnerCallableTarget::Owner { owner } = &call.target else {
                    return ProjectOrderState::Unordered;
                };
                if frames.iter().any(|frame| &frame.callable_owner == owner) {
                    return ProjectOrderState::Deferred;
                }
                let Some(result) = self.index.public_result(owner) else {
                    return ProjectOrderState::Unordered;
                };
                let Some(frame) = self.call_frame(expression, owner) else {
                    return ProjectOrderState::Unordered;
                };
                let mut nested = frames.to_vec();
                nested.push(frame);
                self.expression_state(&result, &nested, active)
            }
        }
    }

    fn project_semantic(
        input: ProjectOrderSemanticExpression,
        fields: &[String],
    ) -> ProjectOrderSemanticExpression {
        if fields.is_empty() {
            input
        } else {
            ProjectOrderSemanticExpression::Project {
                input: Box::new(input),
                fields: fields.to_vec(),
            }
        }
    }

    fn pattern_semantic(
        &self,
        owner: &StableCheckOwnerKey,
        arm: &StableExpressionKey,
        name: &str,
        projection: &[String],
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> Option<ProjectOrderSemanticExpression> {
        let arm = StableOrderExpression {
            owner: owner.clone(),
            expression: arm.clone(),
        };
        let (_, _, syntax, _) = self.index.order_expression(&arm)?;
        let selector = self
            .index
            .stable_expression_ref(owner, syntax.pattern_selector?)?;
        let mut fields = match &syntax.kind {
            AstExprKind::MatchArm {
                pattern: boon_syntax::AstMatchPattern::Tag { fields, .. },
                ..
            } if fields.contains(&name.to_owned()) => vec![name.to_owned()],
            AstExprKind::MatchArm { .. } => Vec::new(),
            _ => return None,
        };
        fields.extend(projection.iter().cloned());
        self.semantic_expression(&selector, frames, active)
            .map(|selector| Self::project_semantic(selector, &fields))
    }

    fn stable_target_semantic(
        &self,
        target: &OwnerLexicalTargetRef,
        projection: &[String],
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> Option<ProjectOrderSemanticExpression> {
        let OwnerLexicalTargetRef::Declaration {
            owner,
            declaration,
            capability,
        } = target
        else {
            return None;
        };
        if matches!(capability, OwnerLexicalDeclarationCapability::Out { .. })
            || matches!(declaration, OwnerDeclarationStableKey::FreshOut { .. })
        {
            return Some(ProjectOrderSemanticExpression::Row(projection.to_vec()));
        }
        match declaration {
            OwnerDeclarationStableKey::Parameter { ordinal } => {
                frames.iter().enumerate().rev().find_map(|(index, frame)| {
                    (&frame.callable_owner == owner)
                        .then(|| {
                            frame.bindings.get(ordinal).and_then(|value| {
                                self.semantic_expression(value, &frames[..index], active)
                                    .map(|value| Self::project_semantic(value, projection))
                            })
                        })
                        .flatten()
                })
            }
            OwnerDeclarationStableKey::PatternBinding { selector, name, .. } => {
                self.pattern_semantic(owner, selector, name, projection, frames, active)
            }
            OwnerDeclarationStableKey::CallContext { .. } => None,
            OwnerDeclarationStableKey::Public
            | OwnerDeclarationStableKey::Statement { .. }
            | OwnerDeclarationStableKey::RecordField { .. } => {
                let expanded = self
                    .stable_target_value(target, frames)
                    .and_then(|(value, frame_count)| {
                        self.semantic_expression(&value, &frames[..frame_count], active)
                    })
                    .map(|value| Self::project_semantic(value, projection));
                expanded.or_else(|| {
                    Some(ProjectOrderSemanticExpression::Capture {
                        target: ProjectOrderCaptureTarget::Declaration {
                            owner: owner.clone(),
                            declaration: declaration.clone(),
                        },
                        projection: projection.to_vec(),
                    })
                })
            }
            OwnerDeclarationStableKey::FreshOut { .. } => {
                Some(ProjectOrderSemanticExpression::Row(projection.to_vec()))
            }
        }
    }

    fn local_target_semantic(
        &self,
        expression: &StableOrderExpression,
        target: &OwnerLexicalDeclarationTarget,
        projection: &[String],
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> Option<ProjectOrderSemanticExpression> {
        let view = self.index.owners.get(&expression.owner)?;
        match target {
            OwnerLexicalDeclarationTarget::Parameter { ordinal } => {
                let parameter = view
                    .interface
                    .parameters
                    .iter()
                    .find(|parameter| parameter.ordinal == *ordinal)?;
                if parameter.kind == crate::OwnerParameterKind::Out {
                    return Some(ProjectOrderSemanticExpression::Row(projection.to_vec()));
                }
                frames.iter().enumerate().rev().find_map(|(index, frame)| {
                    (frame.callable_owner == expression.owner)
                        .then(|| {
                            frame.bindings.get(ordinal).and_then(|value| {
                                self.semantic_expression(value, &frames[..index], active)
                                    .map(|value| Self::project_semantic(value, projection))
                            })
                        })
                        .flatten()
                })
            }
            OwnerLexicalDeclarationTarget::Statement { statement } => {
                let input = view.syntax.statements.get(*statement as usize)?;
                let value = self
                    .index
                    .statement_value_ref(&expression.owner, *statement);
                let expanded = value
                    .and_then(|value| self.semantic_expression(&value, frames, active))
                    .map(|value| Self::project_semantic(value, projection));
                expanded.or_else(|| {
                    Some(ProjectOrderSemanticExpression::Capture {
                        target: ProjectOrderCaptureTarget::Declaration {
                            owner: expression.owner.clone(),
                            declaration: OwnerDeclarationStableKey::Statement {
                                statement: input.stable_key.clone(),
                            },
                        },
                        projection: projection.to_vec(),
                    })
                })
            }
            OwnerLexicalDeclarationTarget::RecordField {
                object,
                ordinal,
                name,
            } => {
                let object = self
                    .index
                    .stable_expression_ref(&expression.owner, *object)?;
                let value = self.record_field_value(&object, *ordinal);
                let expanded = value
                    .and_then(|value| self.semantic_expression(&value, frames, active))
                    .map(|value| Self::project_semantic(value, projection));
                expanded.or_else(|| {
                    Some(ProjectOrderSemanticExpression::Capture {
                        target: ProjectOrderCaptureTarget::Declaration {
                            owner: expression.owner.clone(),
                            declaration: OwnerDeclarationStableKey::RecordField {
                                object: object.expression,
                                ordinal: *ordinal,
                                name: name.clone(),
                            },
                        },
                        projection: projection.to_vec(),
                    })
                })
            }
            OwnerLexicalDeclarationTarget::PatternBinding { arm, name } => {
                let arm = view
                    .syntax
                    .expressions
                    .get(*arm as usize)?
                    .stable_key
                    .clone();
                self.pattern_semantic(&expression.owner, &arm, name, projection, frames, active)
            }
            OwnerLexicalDeclarationTarget::Imported { target } => {
                self.stable_target_semantic(target, projection, frames, active)
            }
            OwnerLexicalDeclarationTarget::Passed
            | OwnerLexicalDeclarationTarget::Ambiguous { .. } => None,
        }
    }

    fn read_semantic(
        &self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> Option<ProjectOrderSemanticExpression> {
        if let Some(read) = self.index.effective_read(expression) {
            return match &read.target {
                OwnerEffectiveLexicalTarget::Static { target } => {
                    self.local_target_semantic(expression, target, &read.projection, frames, active)
                }
                OwnerEffectiveLexicalTarget::Imported { target } => {
                    self.stable_target_semantic(target, &read.projection, frames, active)
                }
                OwnerEffectiveLexicalTarget::FreshOut { .. } => Some(
                    ProjectOrderSemanticExpression::Row(read.projection.to_vec()),
                ),
                OwnerEffectiveLexicalTarget::CallContext { .. }
                | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
            };
        }
        match self.index.value_resolutions.get(expression).copied()? {
            crate::OwnerSymbolResolution::Resolved {
                owner, projection, ..
            } => {
                let target = OwnerLexicalTargetRef::Declaration {
                    owner: owner.clone(),
                    declaration: OwnerDeclarationStableKey::Public,
                    capability: OwnerLexicalDeclarationCapability::Value,
                };
                self.stable_target_semantic(&target, projection, frames, active)
            }
            crate::OwnerSymbolResolution::Authoritative { reference } => {
                Some(ProjectOrderSemanticExpression::Capture {
                    target: ProjectOrderCaptureTarget::Authoritative(
                        boon_syntax::canonical_value_path(&reference.parts),
                    ),
                    projection: Vec::new(),
                })
            }
            crate::OwnerSymbolResolution::Unresolved { .. }
            | crate::OwnerSymbolResolution::CallableAsValue { .. }
            | crate::OwnerSymbolResolution::Ambiguous { .. } => None,
        }
    }

    fn semantic_expression(
        &self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> Option<ProjectOrderSemanticExpression> {
        let frame_path = frames
            .iter()
            .map(|frame| frame.call.clone())
            .collect::<Vec<_>>();
        if !active.insert((expression.clone(), frame_path.clone())) {
            return None;
        }
        let Some((_, _, syntax, _)) = self.index.order_expression(expression) else {
            active.remove(&(expression.clone(), frame_path));
            return None;
        };
        let result =
            (|| match &syntax.kind {
                AstExprKind::Identifier(_) | AstExprKind::Path(_) => {
                    self.read_semantic(expression, frames, active)
                }
                AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
                    Some(ProjectOrderSemanticExpression::Text(value.clone()))
                }
                AstExprKind::TextTemplate { segments } => segments
                    .iter()
                    .map(|segment| match segment {
                        boon_syntax::AstTextSegment::Static { value } => {
                            Some(ProjectOrderSemanticTextSegment::Static(value.clone()))
                        }
                        boon_syntax::AstTextSegment::Dynamic { value } => self
                            .expression_ref(&expression.owner, *value)
                            .and_then(|value| self.semantic_expression(&value, frames, active))
                            .map(ProjectOrderSemanticTextSegment::Dynamic),
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(ProjectOrderSemanticExpression::TextTemplate),
                AstExprKind::Number(value) => ExactNumber::parse_strict(value, None)
                    .ok()
                    .map(ProjectOrderSemanticExpression::Number),
                AstExprKind::BitsLiteral {
                    width,
                    radix,
                    digits,
                } => Bits::parse_encoded(*width, *radix, digits)
                    .ok()
                    .map(ProjectOrderSemanticExpression::Bits),
                AstExprKind::Tag(name) if name != "SKIP" => {
                    Some(ProjectOrderSemanticExpression::Tag(name.clone()))
                }
                AstExprKind::Pipe {
                    input, op, arms, ..
                } if op == "WHILE" => self
                    .expression_ref(&expression.owner, *input)
                    .and_then(|input| self.semantic_expression(&input, frames, active))
                    .zip(
                        arms.iter()
                            .map(|arm| {
                                self.expression_ref(&expression.owner, *arm)
                                    .and_then(|arm| self.semantic_expression(&arm, frames, active))
                            })
                            .collect::<Option<Vec<_>>>(),
                    )
                    .map(|(input, outputs)| ProjectOrderSemanticExpression::Select {
                        input: Box::new(input),
                        outputs,
                    }),
                AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                    let call = self.index.call(expression)?;
                    match &call.target {
                        crate::InferredOwnerCallableTarget::Owner { owner } => {
                            let result = self.index.public_result(owner)?;
                            let frame = self.call_frame(expression, owner)?;
                            let mut nested = frames.to_vec();
                            nested.push(frame);
                            self.semantic_expression(&result, &nested, active)
                        }
                        crate::InferredOwnerCallableTarget::Authoritative => {
                            let mut inputs = Vec::new();
                            for input in call.matched_inputs.iter().filter(|input| {
                                input.formal_kind == crate::OwnerParameterKind::Value
                            }) {
                                inputs.push((
                                    input.formal_name.clone(),
                                    self.semantic_expression(&input.expression, frames, active)?,
                                ));
                            }
                            if let Some(pass) = &call.explicit_pass {
                                inputs.push((
                                    "PASS".to_owned(),
                                    self.semantic_expression(pass, frames, active)?,
                                ));
                            }
                            Some(ProjectOrderSemanticExpression::Call {
                                function: call.function.clone(),
                                inputs,
                            })
                        }
                        crate::InferredOwnerCallableTarget::Unresolved
                        | crate::InferredOwnerCallableTarget::Ambiguous { .. } => None,
                    }
                }
                AstExprKind::Infix { left, op, right } => self
                    .expression_ref(&expression.owner, *left)
                    .and_then(|left| self.semantic_expression(&left, frames, active))
                    .zip(
                        self.expression_ref(&expression.owner, *right)
                            .and_then(|right| self.semantic_expression(&right, frames, active)),
                    )
                    .map(|(left, right)| ProjectOrderSemanticExpression::Infix {
                        operator: op.clone(),
                        left: Box::new(left),
                        right: Box::new(right),
                    }),
                AstExprKind::Block {
                    result: Some(result),
                    ..
                }
                | AstExprKind::Then {
                    output: Some(result),
                    ..
                }
                | AstExprKind::MatchArm {
                    output: Some(result),
                    ..
                } => self
                    .expression_ref(&expression.owner, *result)
                    .and_then(|result| self.semantic_expression(&result, frames, active)),
                AstExprKind::Latest { branches } => branches
                    .iter()
                    .enumerate()
                    .map(|(index, branch)| {
                        self.expression_ref(&expression.owner, *branch)
                            .and_then(|branch| self.semantic_expression(&branch, frames, active))
                            .map(|value| (index.to_string(), value))
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(|inputs| ProjectOrderSemanticExpression::Call {
                        function: "LATEST".to_owned(),
                        inputs,
                    }),
                AstExprKind::When { input, arms } => self
                    .expression_ref(&expression.owner, *input)
                    .and_then(|input| self.semantic_expression(&input, frames, active))
                    .zip(
                        arms.iter()
                            .map(|arm| {
                                self.expression_ref(&expression.owner, *arm)
                                    .and_then(|arm| self.semantic_expression(&arm, frames, active))
                            })
                            .collect::<Option<Vec<_>>>(),
                    )
                    .map(|(input, outputs)| ProjectOrderSemanticExpression::Select {
                        input: Box::new(input),
                        outputs,
                    }),
                AstExprKind::Drain { .. }
                | AstExprKind::Tag(_)
                | AstExprKind::ByteLiteral { .. }
                | AstExprKind::Flush { .. }
                | AstExprKind::Source
                | AstExprKind::Draining { .. }
                | AstExprKind::Hold { .. }
                | AstExprKind::Then { output: None, .. }
                | AstExprKind::MatchArm { output: None, .. }
                | AstExprKind::Block { result: None, .. }
                | AstExprKind::TaggedObject { .. }
                | AstExprKind::Object(_)
                | AstExprKind::ListLiteral { .. }
                | AstExprKind::BytesLiteral { .. }
                | AstExprKind::Delimiter
                | AstExprKind::Unknown(_)
                | AstExprKind::Arrow { .. }
                | AstExprKind::MapEntry { .. }
                | AstExprKind::MapLiteral { .. }
                | AstExprKind::SetLiteral { .. } => None,
            })();
        active.remove(&(expression.clone(), frame_path));
        result
    }

    fn order_direction(
        &self,
        call_expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
    ) -> (ProjectOrderDirectionFact, ProjectOrderSemanticDirection) {
        let Some(expression) = self.call_input(call_expression, "direction") else {
            return (
                ProjectOrderDirectionFact::Ascending,
                ProjectOrderSemanticDirection::Ascending,
            );
        };
        let semantic = self.semantic_expression(&expression, frames, &mut BTreeSet::new());
        match semantic {
            Some(ProjectOrderSemanticExpression::Tag(value)) if value == "Ascending" => (
                ProjectOrderDirectionFact::Ascending,
                ProjectOrderSemanticDirection::Ascending,
            ),
            Some(ProjectOrderSemanticExpression::Tag(value)) if value == "Descending" => (
                ProjectOrderDirectionFact::Descending,
                ProjectOrderSemanticDirection::Descending,
            ),
            semantic => (
                ProjectOrderDirectionFact::Dynamic { expression },
                ProjectOrderSemanticDirection::Dynamic(semantic),
            ),
        }
    }

    fn order_key(
        &self,
        call_expression: &StableOrderExpression,
        key: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
    ) -> (ProjectOrderKey, ProjectOrderSemanticKey) {
        let mut key_type = self
            .index
            .order_expression(key)
            .map(|(_, _, _, expression)| expression.flow_type.ty.clone())
            .unwrap_or(Type::Unknown);
        if let Some(call) = self.index.call(call_expression) {
            key_type =
                apply_checked_type_substitutions(&key_type, &call.diagnostic.type_substitutions);
        }
        for frame in frames.iter().rev() {
            if let Some(call) = self.index.call(&frame.call) {
                key_type = apply_checked_type_substitutions(
                    &key_type,
                    &call.diagnostic.type_substitutions,
                );
            }
        }
        let (direction, semantic_direction) = self.order_direction(call_expression, frames);
        let call_path = frames
            .iter()
            .map(|frame| frame.call.clone())
            .chain(std::iter::once(call_expression.clone()))
            .collect();
        (
            ProjectOrderKey {
                call_path,
                expression: key.clone(),
                direction,
                key_type,
                pure: self.expression_is_pure(key, frames, &mut BTreeSet::new()),
                total: self.expression_is_total(key, frames, &mut BTreeSet::new()),
            },
            ProjectOrderSemanticKey {
                key: self.semantic_expression(key, frames, &mut BTreeSet::new()),
                direction: semantic_direction,
            },
        )
    }

    fn read_is_total(
        &self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> bool {
        if let Some(read) = self.index.effective_read(expression) {
            if matches!(
                &read.target,
                OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::Passed
                        | OwnerLexicalDeclarationTarget::Ambiguous { .. },
                } | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                    | OwnerEffectiveLexicalTarget::Ambiguous { .. }
                    | OwnerEffectiveLexicalTarget::Imported {
                        target: OwnerLexicalTargetRef::ContextFormal { .. }
                            | OwnerLexicalTargetRef::Ambiguous { .. },
                    }
            ) {
                return false;
            }
            return self
                .read_value(expression, frames)
                .is_none_or(|(value, frame_count, _)| {
                    self.expression_is_total(&value, &frames[..frame_count], active)
                });
        }
        match self.index.value_resolutions.get(expression).copied() {
            Some(crate::OwnerSymbolResolution::Resolved { owner, .. }) => self
                .index
                .public_result(owner)
                .is_some_and(|value| self.expression_is_total(&value, frames, active)),
            Some(crate::OwnerSymbolResolution::Authoritative { .. }) => true,
            Some(
                crate::OwnerSymbolResolution::Unresolved { .. }
                | crate::OwnerSymbolResolution::CallableAsValue { .. }
                | crate::OwnerSymbolResolution::Ambiguous { .. },
            )
            | None => false,
        }
    }

    fn expression_is_total(
        &self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> bool {
        let frame_path = frames
            .iter()
            .map(|frame| frame.call.clone())
            .collect::<Vec<_>>();
        if !active.insert((expression.clone(), frame_path.clone())) {
            return true;
        }
        let Some((_, _, syntax, _)) = self.index.order_expression(expression) else {
            return false;
        };
        let total = match &syntax.kind {
            AstExprKind::Identifier(_) | AstExprKind::Path(_) => {
                self.read_is_total(expression, frames, active)
            }
            AstExprKind::Drain { .. } => false,
            AstExprKind::Pipe {
                input, op, arms, ..
            } if op == "WHILE" => {
                self.expression_ref(&expression.owner, *input)
                    .is_some_and(|input| self.expression_is_total(&input, frames, active))
                    && arms.iter().all(|arm| {
                        self.expression_ref(&expression.owner, *arm)
                            .is_some_and(|arm| self.expression_is_total(&arm, frames, active))
                    })
            }
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                let Some(call) = self.index.call(expression) else {
                    active.remove(&(expression.clone(), frame_path));
                    return false;
                };
                if let crate::InferredOwnerCallableTarget::Owner { owner } = &call.target {
                    let Some(result) = self.index.public_result(owner) else {
                        active.remove(&(expression.clone(), frame_path));
                        return false;
                    };
                    let Some(frame) = self.call_frame(expression, owner) else {
                        active.remove(&(expression.clone(), frame_path));
                        return false;
                    };
                    let mut nested = frames.to_vec();
                    nested.push(frame);
                    self.expression_is_total(&result, &nested, active)
                } else {
                    !crate::order_key_call_is_error_capable(&call.function)
                        && call.matched_inputs.iter().all(|input| {
                            self.expression_is_total(&input.expression, frames, active)
                        })
                        && call
                            .explicit_pass
                            .as_ref()
                            .is_none_or(|pass| self.expression_is_total(pass, frames, active))
                }
            }
            AstExprKind::Infix { left, op, right } => {
                !matches!(op.as_str(), "+" | "-" | "*" | "/" | "%")
                    && self
                        .expression_ref(&expression.owner, *left)
                        .is_some_and(|left| self.expression_is_total(&left, frames, active))
                    && self
                        .expression_ref(&expression.owner, *right)
                        .is_some_and(|right| self.expression_is_total(&right, frames, active))
            }
            AstExprKind::Block {
                result: Some(result),
                ..
            }
            | AstExprKind::Then {
                output: Some(result),
                ..
            }
            | AstExprKind::MatchArm {
                output: Some(result),
                ..
            } => self
                .expression_ref(&expression.owner, *result)
                .is_some_and(|result| self.expression_is_total(&result, frames, active)),
            AstExprKind::Latest { branches } => branches.iter().all(|branch| {
                self.expression_ref(&expression.owner, *branch)
                    .is_some_and(|branch| self.expression_is_total(&branch, frames, active))
            }),
            AstExprKind::When { input, arms } => {
                self.expression_ref(&expression.owner, *input)
                    .is_some_and(|input| self.expression_is_total(&input, frames, active))
                    && arms.iter().all(|arm| {
                        self.expression_ref(&expression.owner, *arm)
                            .is_some_and(|arm| self.expression_is_total(&arm, frames, active))
                    })
            }
            AstExprKind::TextTemplate { segments } => {
                segments.iter().all(|segment| match segment {
                    boon_syntax::AstTextSegment::Static { .. } => true,
                    boon_syntax::AstTextSegment::Dynamic { value } => self
                        .expression_ref(&expression.owner, *value)
                        .is_some_and(|expr| self.expression_is_total(&expr, frames, active)),
                })
            }
            AstExprKind::MapEntry { key, value } => [*key, *value].into_iter().all(|input| {
                self.expression_ref(&expression.owner, input)
                    .is_some_and(|input| self.expression_is_total(&input, frames, active))
            }),
            AstExprKind::MapLiteral { entries } => entries.iter().all(|entry| {
                self.expression_ref(&expression.owner, *entry)
                    .is_some_and(|entry| self.expression_is_total(&entry, frames, active))
            }),
            AstExprKind::SetLiteral { items } => items.iter().all(|item| {
                self.expression_ref(&expression.owner, *item)
                    .is_some_and(|item| self.expression_is_total(&item, frames, active))
            }),
            AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => true,
            AstExprKind::Number(value) => ExactNumber::parse_strict(value, None).is_ok(),
            AstExprKind::BitsLiteral {
                width,
                radix,
                digits,
            } => Bits::parse_encoded(*width, *radix, digits).is_ok(),
            AstExprKind::ByteLiteral { .. } => true,
            AstExprKind::Tag(name) => name != "SKIP",
            AstExprKind::Flush { .. }
            | AstExprKind::Source
            | AstExprKind::Draining { .. }
            | AstExprKind::Hold { .. }
            | AstExprKind::Then { output: None, .. }
            | AstExprKind::MatchArm { output: None, .. }
            | AstExprKind::Block { result: None, .. }
            | AstExprKind::TaggedObject { .. }
            | AstExprKind::Object(_)
            | AstExprKind::ListLiteral { .. }
            | AstExprKind::BytesLiteral { .. }
            | AstExprKind::Delimiter
            | AstExprKind::Unknown(_)
            | AstExprKind::Arrow { .. } => false,
        };
        active.remove(&(expression.clone(), frame_path));
        total
    }

    fn expression_is_pure(
        &self,
        expression: &StableOrderExpression,
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> bool {
        let frame_path = frames
            .iter()
            .map(|frame| frame.call.clone())
            .collect::<Vec<_>>();
        if !active.insert((expression.clone(), frame_path.clone())) {
            return true;
        }
        let Some((view, index, syntax, inferred)) = self.index.order_expression(expression) else {
            return false;
        };
        if inferred.flow_type.mode != FlowMode::Continuous
            || inferred.direct_effect != CheckedEffectSummary::default()
        {
            active.remove(&(expression.clone(), frame_path));
            return false;
        }
        if let Some((value, frame_count, projection_is_empty)) = self.read_value(expression, frames)
            && projection_is_empty
        {
            let pure = self.expression_is_pure(&value, &frames[..frame_count], active);
            active.remove(&(expression.clone(), frame_path));
            return pure;
        }
        if matches!(
            &syntax.kind,
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. }
        ) && let Some(call) = self.index.call(expression)
        {
            let pure = call
                .matched_inputs
                .iter()
                .filter(|input| input.formal_kind == crate::OwnerParameterKind::Value)
                .map(|input| input.expression.clone())
                .chain(call.explicit_pass.iter().cloned())
                .all(|child| self.expression_is_pure(&child, frames, active));
            active.remove(&(expression.clone(), frame_path));
            return pure;
        }
        let children = view
            .graph
            .expression_inputs(crate::OwnerExpressionId(index as u32));
        let pure = children.is_none_or(|children| {
            children.iter().all(|child| {
                self.owner_ref(&expression.owner, child)
                    .is_some_and(|child| self.expression_is_pure(&child, frames, active))
            })
        });
        active.remove(&(expression.clone(), frame_path));
        pure
    }

    fn merge_branch_states(
        &mut self,
        owner: &StableCheckOwnerKey,
        branches: &[usize],
        frames: &[ProjectOrderFrame],
        active: &mut BTreeSet<(StableOrderExpression, Vec<StableOrderExpression>)>,
    ) -> ProjectOrderState {
        let mut states = branches.iter().filter_map(|branch| {
            self.expression_ref(owner, *branch)
                .map(|branch| self.expression_state(&branch, frames, active))
        });
        let Some(first) = states.next() else {
            return ProjectOrderState::Unordered;
        };
        states.fold(first, |left, right| match (left, right) {
            (ProjectOrderState::Invalid { call_path }, _)
            | (_, ProjectOrderState::Invalid { call_path }) => {
                ProjectOrderState::Invalid { call_path }
            }
            (ProjectOrderState::Ordered(left), ProjectOrderState::Ordered(right))
                if left.semantic == right.semantic =>
            {
                ProjectOrderState::Ordered(left)
            }
            (ProjectOrderState::Deferred, _) | (_, ProjectOrderState::Deferred) => {
                ProjectOrderState::Deferred
            }
            _ => ProjectOrderState::Unordered,
        })
    }

    fn diagnostic_span(
        &self,
        expression: &StableOrderExpression,
    ) -> Result<TypeDiagnosticSpan, ProjectDiagnosticFactsError> {
        self.index.owners.get(&expression.owner).ok_or_else(|| {
            ProjectDiagnosticFactsError::new("order diagnostic expression has no owner")
        })?;
        self.index.expression_span(expression)
    }
}

fn project_order_facts(
    index: &ProjectFactIndex<'_>,
) -> Result<(ProjectOrderFacts, Vec<TypeDiagnostic>), ProjectDiagnosticFactsError> {
    let mut diagnostics = Vec::new();
    let mut chains = Vec::new();
    let mut seen = BTreeSet::new();
    let calls = index.calls.keys().cloned().collect::<Vec<_>>();
    let mut analyzer = ProjectOrderAnalyzer::new(index);
    for call in calls {
        let state = analyzer.expression_state(&call, &[], &mut BTreeSet::new());
        match state {
            ProjectOrderState::Invalid { call_path } => {
                let diagnostic_expression = call_path.first().unwrap_or(&call);
                let span = analyzer.diagnostic_span(diagnostic_expression)?;
                let message =
                    "`List/then_by` requires a compatible preceding `List/sort_by` order chain"
                        .to_owned();
                if seen.insert((span.start, span.end, message.clone())) {
                    diagnostics.push(TypeDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        line: span.line,
                        start: span.start,
                        end: span.end,
                        message,
                    });
                }
            }
            ProjectOrderState::Ordered(chain) => {
                chains.push(ProjectCallOrderChainFact {
                    call: call.clone(),
                    keys: chain
                        .keys
                        .iter()
                        .map(|key| ProjectOrderKeyFact {
                            call_path: key.call_path.clone().into_boxed_slice(),
                            key: key.expression.clone(),
                            direction: key.direction.clone(),
                            key_type: key.key_type.clone(),
                            pure: key.pure,
                            total: key.total,
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                });
                for key in chain.keys {
                    let diagnostic_expression = key.call_path.first().unwrap_or(&key.expression);
                    let span = analyzer.diagnostic_span(diagnostic_expression)?;
                    let mut messages = Vec::new();
                    if !type_is_deferred_order_key(&key.key_type)
                        && !type_is_orderable_key(&key.key_type)
                    {
                        messages.push(format!(
                            "list order key has unsupported type\nexpected: finite NUMBER, TEXT, or closed fieldless tags such as `True | False`\nfound: {}",
                            crate::boon_facing_type_label(&key.key_type)
                        ));
                    }
                    if !key.pure {
                        messages
                            .push("list order key must be a continuous pure expression".to_owned());
                    }
                    if !key.total {
                        messages.push(
                            "list order key must be total and cannot use an error-capable conversion or partial operation"
                                .to_owned(),
                        );
                    }
                    for message in messages {
                        if seen.insert((span.start, span.end, message.clone())) {
                            diagnostics.push(TypeDiagnostic {
                                severity: DiagnosticSeverity::Error,
                                line: span.line,
                                start: span.start,
                                end: span.end,
                                message,
                            });
                        }
                    }
                }
            }
            ProjectOrderState::Unordered | ProjectOrderState::Deferred => {}
        }
    }
    chains.sort_by(|left, right| left.call.cmp(&right.call));
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        b"boon.project-order-facts.v1\0",
        &chains,
    )
    .map_err(|error| {
        ProjectDiagnosticFactsError::new(format!("cannot fingerprint project order facts: {error}"))
    })?;
    Ok((
        ProjectOrderFacts {
            chains: chains.into_boxed_slice(),
            fingerprint_v1,
        },
        diagnostics,
    ))
}

fn output_facts(
    project: &ProjectSyntaxSnapshot,
    index: &ProjectFactIndex<'_>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<Vec<ProjectOutputRootFact>, ProjectDiagnosticFactsError> {
    let syntax = TypecheckSyntaxProgram::UnitNative(project.clone());
    let containers = syntax
        .statements()
        .iter()
        .filter(|statement| {
            matches!(&statement.kind, AstStatementKind::Field { name } if name == "outputs")
        })
        .cloned()
        .collect::<Vec<_>>();
    if containers.len() > 1 {
        diagnostics.push(syntax.diagnostic_for_statement(
            containers.get(1),
            "Boon source may declare only one top-level `outputs` registry".to_owned(),
        ));
    }
    let Some(container) = containers.first() else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    let mut names = BTreeSet::new();
    for child in &container.children {
        if let AstStatementKind::Hold {
            field: Some(name), ..
        }
        | AstStatementKind::Source {
            field: Some(name), ..
        } = &child.kind
        {
            diagnostics.push(syntax.diagnostic_for_statement(
                Some(child),
                format!(
                    "output root `{name}` declares SOURCE or HOLD authority; outputs must be reconstructed from existing current values"
                ),
            ));
            continue;
        }
        let name = match &child.kind {
            AstStatementKind::Field { name }
            | AstStatementKind::List {
                field: Some(name), ..
            } => name,
            _ => {
                if !statement_is_empty_delimiter(child, syntax.expressions()) {
                    diagnostics.push(syntax.diagnostic_for_statement(
                        Some(child),
                        "`outputs` accepts only named output fields".to_owned(),
                    ));
                }
                continue;
            }
        };
        if !names.insert(name.clone()) {
            diagnostics.push(
                syntax.diagnostic_for_statement(
                    Some(child),
                    format!("duplicate output root `{name}`"),
                ),
            );
            continue;
        }
        if statement_contains_output_authority(child) {
            diagnostics.push(syntax.diagnostic_for_statement(
                Some(child),
                format!(
                    "output root `{name}` declares SOURCE or HOLD authority; outputs must be reconstructed from existing current values"
                ),
            ));
        }
        let statement = project.stable_statement_key(child.id).ok_or_else(|| {
            ProjectDiagnosticFactsError::new("output root has no stable statement identity")
        })?;
        let value = index.statement_value(&statement)?;
        let (value, ty) = value.map_or(
            (None, Type::Unknown),
            |(value, flow_type, flush_type, _)| {
                let ty = flush_type.as_ref().map_or_else(
                    || flow_type.ty.clone(),
                    |flush_type| union_structural_type(&flow_type.ty, flush_type),
                );
                (Some(value), ty)
            },
        );
        if !crate::host_output_type_is_closed(&ty) {
            diagnostics.push(syntax.diagnostic_for_statement(
                Some(child),
                format!(
                    "output root `{name}` must have a closed scalar, record, or list host-value type; found {}",
                    crate::boon_facing_type_label(&ty)
                ),
            ));
        }
        entries.push(ProjectOutputRootFact {
            name: name.clone(),
            statement,
            value,
            ty,
        });
    }
    if entries.is_empty() {
        diagnostics.push(syntax.diagnostic_for_statement(
            Some(container),
            "`outputs` must declare at least one named output root".to_owned(),
        ));
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn render_facts(
    index: &ProjectFactIndex<'_>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Result<Vec<ProjectRenderSlotFact>, ProjectDiagnosticFactsError> {
    let registry = RenderContractRegistry::default();
    let mut slots = Vec::new();
    for (owner, view) in &index.owners {
        for statement in view
            .syntax
            .statements
            .iter()
            .filter(|statement| statement.value_use == CheckedValueUse::RenderSlot)
        {
            let slot_name = match &statement.kind {
                AstStatementKind::Field { name }
                | AstStatementKind::Source {
                    field: Some(name), ..
                }
                | AstStatementKind::List {
                    field: Some(name), ..
                } => name.clone(),
                _ => "items".to_owned(),
            };
            let expected_contract = registry.slot_contract(&slot_name).to_owned();
            let value = index.statement_value(&statement.stable_key)?;
            let (value_key, actual_type, span) = match value {
                Some((value, flow_type, _, span)) => (Some(value), flow_type.ty, Some(span)),
                None => (
                    None,
                    if matches!(slot_name.as_str(), "items" | "children") {
                        Type::List(Type::shared(open_object_type()))
                    } else {
                        open_object_type()
                    },
                    None,
                ),
            };
            let mut slot_diagnostics = Vec::new();
            if value_key.is_some() && !registry.slot_accepts_type(&slot_name, &actual_type) {
                let span = span.expect("value-bearing render fact has an exact source span");
                slot_diagnostics.push(TypeDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    line: span.line,
                    start: span.start,
                    end: span.end,
                    message: if type_contains_absence(&actual_type) {
                        "`SKIP` cannot be used as a render value".to_owned()
                    } else {
                        render_slot_type_error(&slot_name, &actual_type)
                    },
                });
            }
            diagnostics.extend(slot_diagnostics.iter().cloned());
            slots.push(ProjectRenderSlotFact {
                owner: owner.clone(),
                statement: statement.stable_key.clone(),
                value: value_key,
                slot_name,
                expected_contract,
                actual_type,
                diagnostics: slot_diagnostics.into_boxed_slice(),
            });
        }
    }
    slots.sort_by(|left, right| {
        (&left.owner, &left.statement).cmp(&(&right.owner, &right.statement))
    });
    Ok(slots)
}

fn host_facts(
    project: &ProjectSyntaxSnapshot,
    abi: &OwnerAbiEnvironment,
    outputs: &[ProjectOutputRootFact],
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> (HostPortSyntaxTable, Option<String>) {
    let syntax = TypecheckSyntaxProgram::UnitNative(project.clone());
    let source_paths = abi
        .source_payloads
        .iter()
        .map(|source| source.canonical_path.clone())
        .collect::<BTreeSet<_>>();
    let source_lookup = SourcePayloadPathLookup::new(&source_paths);
    let (host_ports, host_diagnostics) = host_port_table(&syntax, &source_lookup);
    diagnostics.extend(host_diagnostics);

    let source_count = |path: &str| {
        abi.source_payloads
            .iter()
            .filter(|source| source.canonical_path == path)
            .count()
    };
    let output_count = |name: &str| outputs.iter().filter(|output| output.name == name).count();
    let mut resolution_error = None;
    if let Some(http) = &host_ports.http {
        for path in
            std::iter::once(http.request_source.as_str()).chain(http.disconnect_source.as_deref())
        {
            let count = source_count(path);
            if count != 1 {
                resolution_error = Some(format!(
                    "host source `{path}` resolves to {count} exact checked source identities"
                ));
                break;
            }
        }
        if resolution_error.is_none() {
            let count = output_count(&http.response_output);
            if count != 1 {
                if count == 0 {
                    diagnostics.push(diagnostic_at_line(
                        http.line,
                        format!(
                            "host port `http.response` references missing output root `{}`",
                            http.response_output
                        ),
                    ));
                }
                resolution_error = Some(format!(
                    "host output `{}` resolves to {count} exact checked output identities",
                    http.response_output
                ));
            }
        }
    }
    if resolution_error.is_none()
        && let Some(websocket) = &host_ports.websocket
    {
        for path in [
            websocket.open_source.as_str(),
            websocket.message_source.as_str(),
            websocket.close_source.as_str(),
            websocket.error_source.as_str(),
        ] {
            let count = source_count(path);
            if count != 1 {
                resolution_error = Some(format!(
                    "host source `{path}` resolves to {count} exact checked source identities"
                ));
                break;
            }
        }
        if resolution_error.is_none() {
            let count = output_count(&websocket.actions_output);
            if count != 1 {
                if count == 0 {
                    diagnostics.push(diagnostic_at_line(
                        websocket.line,
                        format!(
                            "host port `websocket.actions` references missing output root `{}`",
                            websocket.actions_output
                        ),
                    ));
                }
                resolution_error = Some(format!(
                    "host output `{}` resolves to {count} exact checked output identities",
                    websocket.actions_output
                ));
            }
        }
    }
    if let Some(error) = &resolution_error {
        diagnostics.push(diagnostic_at_line(1, error.clone()));
    }

    for (path, fields) in host_port_payload_types(&host_ports) {
        let Some(source) = abi
            .source_payloads
            .iter()
            .find(|source| source.canonical_path == path)
        else {
            continue;
        };
        let contract = Type::object(boon_checked::ObjectShape::from_ordered_fields(
            fields.into_iter(),
            false,
        ));
        if source.payload_type != contract {
            diagnostics.push(diagnostic_at_line(
                1,
                format!(
                    "checked host source `{path}` payload differs from its exact host contract\nexpected: {contract:?}\nfound: {:?}",
                    source.payload_type
                ),
            ));
            break;
        }
    }

    if let Some(http) = &host_ports.http
        && let Some(output) = outputs
            .iter()
            .find(|output| output.name == http.response_output)
        && !http_response_type_is_valid(&output.ty)
    {
        diagnostics.push(diagnostic_at_line(
            http.line,
            format!(
                "host port `http.response` output `{}` must be exactly `{{ status: Number, body: Bytes }}` or `{{ status: Number, headers: List<{{ name: Text, value: Text|Bytes }}>, body: Bytes }}`; found {}",
                output.name,
                crate::boon_facing_type_label(&output.ty)
            ),
        ));
    }
    if let Some(websocket) = &host_ports.websocket
        && let Some(output) = outputs
            .iter()
            .find(|output| output.name == websocket.actions_output)
        && !websocket_actions_type_is_valid(&output.ty)
    {
        diagnostics.push(diagnostic_at_line(
            websocket.line,
            format!(
                "host port `websocket.actions` output `{}` must be a list of closed generic WebSocket action envelopes; found {}",
                output.name,
                crate::boon_facing_type_label(&output.ty)
            ),
        ));
    }
    (host_ports, resolution_error)
}

pub fn project_diagnostic_facts<'a>(
    project: &'a ProjectSyntaxSnapshot,
    abi: &OwnerAbiEnvironment,
    output_flow: &ProjectOutputFlowFacts,
    expected_owners: impl IntoIterator<Item = &'a StableCheckOwnerKey>,
    syntax_inputs: impl IntoIterator<Item = &'a OwnerSyntaxInput>,
    lexical_plans: impl IntoIterator<Item = &'a OwnerLexicalPlan>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    interfaces: impl IntoIterator<Item = &'a crate::OwnerPublicInterface>,
    replay_evaluations: impl IntoIterator<Item = &'a OwnerDiagnosticReplayFactsEvaluation>,
    replay_facts: impl IntoIterator<Item = &'a OwnerDiagnosticReplayFacts>,
    inference_abis: impl IntoIterator<
        Item = (&'a StableCheckOwnerKey, &'a OwnerInferenceAbiEnvironment),
    >,
    source_maps: impl IntoIterator<Item = &'a OwnerSourceMap>,
) -> Result<ProjectDiagnosticFacts, ProjectDiagnosticFactsError> {
    let expected_owners = expected_owners.into_iter().collect::<Vec<_>>();
    if !output_flow.matches_owners(expected_owners.iter().copied()) {
        return Err(ProjectDiagnosticFactsError::new(
            "project output flow coverage differs from the diagnostic owner set",
        ));
    }
    let index = ProjectFactIndex::new(
        project,
        expected_owners.iter().copied(),
        syntax_inputs,
        lexical_plans,
        summaries,
        interfaces,
        replay_evaluations,
        replay_facts,
        inference_abis,
        source_maps,
    )?;
    let mut diagnostics = Vec::new();
    diagnostics.extend(source_shape_diagnostics(&index)?);
    diagnostics.extend(expression_structure_diagnostics(&index)?);
    diagnostics.extend(recursive_function_diagnostics(&index)?);
    let mut exact_types = ProjectFlowTypeAnalyzer::new(&index);
    diagnostics.extend(host_effect_diagnostics(&index, &mut exact_types)?);
    diagnostics.extend(builtin_call_diagnostics(&index, abi, &mut exact_types)?);
    diagnostics.extend(style_diagnostics(&index)?);
    let exact_modes = ProjectFlowModeAnalyzer::new(&index, abi, output_flow)
        .modes_for(temporal_mode_roots(&index)?);
    diagnostics.extend(temporal_diagnostics(&index, &exact_modes)?);
    diagnostics.extend(match_pattern_diagnostics(&index, &mut exact_types)?);
    diagnostics.extend(duplicate_function_diagnostics(&index)?);
    diagnostics.extend(collection_authority_diagnostics(&index)?);
    diagnostics.extend(output_flow_diagnostics(&index, output_flow)?);
    let output_producers = output_flow.producers.clone();
    let output_roots = output_facts(project, &index, &mut diagnostics)?;
    let render_slots = render_facts(&index, &mut diagnostics)?;
    let (host_ports, host_port_resolution_error) =
        host_facts(project, abi, &output_roots, &mut diagnostics);
    let (order, order_diagnostics) = project_order_facts(&index)?;
    diagnostics.extend(order_diagnostics);
    canonicalize_diagnostics(&mut diagnostics);
    let source_bundle_digest_v1 = project.source_bundle_digest_v1();
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        PROJECT_DIAGNOSTIC_FACTS_DOMAIN_V10,
        &(
            source_bundle_digest_v1,
            abi.fingerprint_v1(),
            &output_producers,
            &output_roots,
            order.fingerprint_v1(),
            &render_slots,
            &host_ports,
            &host_port_resolution_error,
            &diagnostics,
        ),
    )
    .map_err(|error| {
        ProjectDiagnosticFactsError::new(format!(
            "cannot fingerprint project diagnostic facts: {error}"
        ))
    })?;
    Ok(ProjectDiagnosticFacts {
        source_bundle_digest_v1,
        output_roots: output_roots.into_boxed_slice(),
        output_producers,
        order,
        render_slots: render_slots.into_boxed_slice(),
        host_ports,
        host_port_resolution_error,
        diagnostics: diagnostics.into_boxed_slice(),
        fingerprint_v1,
    })
}
