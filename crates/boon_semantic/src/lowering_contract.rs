//! Semantic-owned lowering metadata, output contracts, and host-port bindings.
//!
//! This module is the flag-day boundary for the remaining typechecker lowering
//! side tables. Checked coordinates and canonical paths are consumed here only
//! to migrate opaque [`CheckedProgram`] evidence onto normalized semantic IDs.
//! Downstream lowering must use the semantic identities emitted by this module
//! and must not repeat these checked/name-based joins.

use crate::{
    ResolvedOutGraph, SemanticBindingId, SemanticBindingTargetV1, SemanticCallableId,
    SemanticExecutionGraphV1, SemanticExprId, SemanticExpressionKind, SemanticFieldId,
    SemanticListId, SemanticLocalBindingId, SemanticParameterId, SemanticReactiveGraphV1,
    SemanticResourceGraphV1, SemanticRootKindV1, SemanticSourceId, SemanticSourceOrigin,
    SemanticStateId, SemanticStatementId, SemanticStatementOrigin, SemanticValueId,
    SemanticValueListAuthorityId, checked_semantic_root_specs_v1,
};
use boon_contract::SourceBundleDigestV1;
use boon_typecheck::{
    CheckedCallableKind, CheckedEffectSummary, CheckedExprId, CheckedProgram, CheckedSourceId,
    CheckedStatementId, DeclId, DiagnosticSeverity, FlowType, NamedValueTypeOrigin, Type,
    TypeDiagnostic, Variant,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SEMANTIC_LOWERING_CONTRACT_SCHEMA_V1: &str = "boon.semantic-lowering-contract.v1";
pub const SEMANTIC_LOWERING_METADATA_SCHEMA_V1: &str = "boon.semantic-lowering-metadata.v1";

const SEMANTIC_LOWERING_CONTRACT_DIGEST_DOMAIN: &[u8] = b"boon.semantic-lowering-contract.v1\0";
const SEMANTIC_LOWERING_METADATA_DIGEST_DOMAIN: &[u8] = b"boon.semantic-lowering-metadata.v1\0";

macro_rules! typed_lowering_id {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(
                Clone,
                Copy,
                Debug,
                Eq,
                Hash,
                Ord,
                PartialEq,
                PartialOrd,
                Serialize,
                Deserialize,
            )]
            #[serde(transparent)]
            pub struct $name(pub usize);

            impl $name {
                pub const fn as_usize(self) -> usize {
                    self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(formatter)
                }
            }
        )+
    };
}

typed_lowering_id!(
    SemanticSourceUnitId,
    SemanticSourceExpressionId,
    SemanticNamedValueId,
    SemanticDiagnosticId,
    SemanticRenderSlotId,
    SemanticSourcePayloadShapeId,
    SemanticOutputContractId,
    SemanticHostPortId,
);

macro_rules! digest_type {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name([u8; 32]);

        impl $name {
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }

            pub fn to_hex(self) -> String {
                self.0.iter().map(|byte| format!("{byte:02x}")).collect()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.to_hex())
            }
        }
    };
}

digest_type!(SemanticLoweringMetadataDigestV1);
digest_type!(SemanticLoweringContractDigestV1);

/// The complete A-C authority: lowering metadata, output contracts, and host
/// ports. Scope/storage topology and full view bindings intentionally live in
/// the next boundary and are not represented by placeholders here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticLoweringContractV1 {
    pub schema: String,
    pub metadata: SemanticLoweringMetadataV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_contracts: Vec<SemanticOutputContractV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_ports: Vec<SemanticHostPortBindingV1>,
    /// Collection computations that are proven not to escape, persist, or
    /// expose incremental authority identity. Backends may lower only these
    /// exact regions to private mutable storage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transient_collections: Vec<SemanticTransientCollectionV1>,
    pub digest: SemanticLoweringContractDigestV1,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticTransientCollectionKindV1 {
    List,
    Map,
    Set,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticTransientMapEntryV1 {
    pub key: SemanticExprId,
    pub value: SemanticExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticTransientCollectionStepV1 {
    ListAppend {
        expression: SemanticExprId,
        item: SemanticExprId,
    },
    MapUpsert {
        expression: SemanticExprId,
        key: SemanticExprId,
        value: SemanticExprId,
    },
    MapRemove {
        expression: SemanticExprId,
        key: SemanticExprId,
    },
    SetAdd {
        expression: SemanticExprId,
        item: SemanticExprId,
    },
    SetRemove {
        expression: SemanticExprId,
        item: SemanticExprId,
    },
}

impl SemanticTransientCollectionStepV1 {
    pub const fn expression(&self) -> SemanticExprId {
        match self {
            Self::ListAppend { expression, .. }
            | Self::MapUpsert { expression, .. }
            | Self::MapRemove { expression, .. }
            | Self::SetAdd { expression, .. }
            | Self::SetRemove { expression, .. } => *expression,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticTransientCollectionResultV1 {
    ListGet {
        expression: SemanticExprId,
        position: SemanticExprId,
    },
    ListLength {
        expression: SemanticExprId,
    },
    ListIsNotEmpty {
        expression: SemanticExprId,
    },
    MapGet {
        expression: SemanticExprId,
        key: SemanticExprId,
    },
    SetContains {
        expression: SemanticExprId,
        item: SemanticExprId,
    },
}

impl SemanticTransientCollectionResultV1 {
    pub const fn expression(&self) -> SemanticExprId {
        match self {
            Self::ListGet { expression, .. }
            | Self::ListLength { expression }
            | Self::ListIsNotEmpty { expression }
            | Self::MapGet { expression, .. }
            | Self::SetContains { expression, .. } => *expression,
        }
    }
}

/// A deterministic proof product for one linear, non-escaping collection
/// computation. `authority_flow` includes the constructor, every local alias
/// read and mutation, and the terminal scalar observation in evaluation order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticTransientCollectionV1 {
    pub kind: SemanticTransientCollectionKindV1,
    pub constructor: SemanticExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_capacity: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_items: Vec<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map_entries: Vec<SemanticTransientMapEntryV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set_items: Vec<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<SemanticTransientCollectionStepV1>,
    pub result: SemanticTransientCollectionResultV1,
    pub authority_flow: Vec<SemanticExprId>,
    /// Exact upper bound charged by the private collection kernel.
    pub operation_work_budget: u64,
    /// Maximum number of elements the private storage may hold.
    pub storage_growth_budget: usize,
    /// Transient lowering never snapshots or clones the whole collection.
    pub snapshot_copy_budget: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticLoweringMetadataV1 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub original_source_expression_count: usize,
    pub checked_expression_count: usize,
    pub dynamic_fallback_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_units: Vec<SemanticSourceUnitV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expression_types: Vec<SemanticSourceExpressionTypeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub function_types: Vec<SemanticFunctionTypeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_value_types: Vec<SemanticNamedValueTypeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub render_slots: Vec<SemanticRenderSlotV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_payload_shapes: Vec<SemanticSourcePayloadShapeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<SemanticTypeDiagnosticV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub program_diagnostics: Vec<SemanticDiagnosticId>,
    pub digest: SemanticLoweringMetadataDigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceUnitV1 {
    pub id: SemanticSourceUnitId,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    pub start_line: usize,
    pub line_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceExpressionTypeV1 {
    pub id: SemanticSourceExpressionId,
    /// Checked provenance only. Semantic execution identity is carried by
    /// `occurrences`.
    pub checked_expression: CheckedExprId,
    pub occurrences: Vec<SemanticSourceExpressionOccurrenceV1>,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceExpressionOccurrenceV1 {
    pub expression: SemanticExprId,
    /// Exact normalized type after contextual substitution. This may differ
    /// from the source expression's principal checked type.
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticFunctionTypeV1 {
    pub callable: SemanticCallableId,
    pub checked_callable: DeclId,
    /// Diagnostic spelling retained from the checked side table.
    pub name: String,
    pub parameters: Vec<SemanticFunctionParameterTypeV1>,
    pub result: FlowType,
    pub effect: CheckedEffectSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticFunctionParameterTypeV1 {
    pub parameter: SemanticParameterId,
    pub formal: DeclId,
    pub ordinal: usize,
    /// Diagnostic spelling retained from the checked side table.
    pub name: String,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticNamedValueTypeV1 {
    pub id: SemanticNamedValueId,
    /// Exact parser/typechecker statement-site identity. This, together with
    /// checked origin provenance, defines the named-value identity; the path
    /// below is presentation only and may repeat.
    pub checked_statement: CheckedStatementId,
    /// Canonical typechecker path retained for source/editor diagnostics. This
    /// string is not an execution or storage identity.
    pub diagnostic_path: String,
    pub origins: Vec<SemanticNamedValueTypeOriginV1>,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticNamedValueTypeOriginV1 {
    /// Exact checked provenance retained for audit. All executable identities
    /// are carried by the semantic collections below.
    pub checked: NamedValueTypeOrigin,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statements: Vec<SemanticStatementId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<SemanticValueId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SemanticBindingId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SemanticSourceId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<SemanticStateId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lists: Vec<SemanticListId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_list_authorities: Vec<SemanticValueListAuthorityId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticRenderSlotV1 {
    pub id: SemanticRenderSlotId,
    pub statement: SemanticStatementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<SemanticExprId>,
    pub slot_name: String,
    pub expected_contract: String,
    pub actual_type: Type,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<SemanticDiagnosticId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourcePayloadShapeV1 {
    pub id: SemanticSourcePayloadShapeId,
    /// Canonical checked path retained as diagnostic evidence for the one-time
    /// migration onto `sources`.
    pub diagnostic_path: String,
    pub checked_sources: Vec<CheckedSourceId>,
    pub sources: Vec<SemanticSourceId>,
    pub payload_type: Type,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<SemanticSourcePayloadFieldV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourcePayloadFieldV1 {
    pub name: String,
    pub data_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticTypeDiagnosticV1 {
    pub id: SemanticDiagnosticId,
    pub severity: DiagnosticSeverity,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticOutputContractKindV1 {
    RetainedVisualDocument,
    RetainedVisualScene,
    HostValue,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOutputDemandPolicyV1 {
    HostDemanded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticOutputContractV1 {
    pub id: SemanticOutputContractId,
    pub ordinal: usize,
    /// Diagnostic host-facing label. Identity is `id`.
    pub root: String,
    /// Diagnostic source path. Identity is `id`.
    pub value_path: String,
    pub contract: SemanticOutputContractKindV1,
    pub demand: SemanticOutputDemandPolicyV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<Type>,
    pub declaration: DeclId,
    pub checked_statement: CheckedStatementId,
    pub statement: SemanticStatementId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub binding: SemanticBindingId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<SemanticFieldId>,
    pub line: usize,
    pub typed_contract_known: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticHostSourceBindingV1 {
    pub source: SemanticSourceId,
    /// Diagnostic path copied from the checked host declaration.
    pub diagnostic_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticHostOutputBindingV1 {
    pub output: SemanticOutputContractId,
    pub output_ordinal: usize,
    pub statement: SemanticStatementId,
    /// Diagnostic spelling copied from the checked host declaration.
    pub diagnostic_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticHostPortBindingV1 {
    pub id: SemanticHostPortId,
    pub line: usize,
    pub kind: SemanticHostPortKindV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticHostPortKindV1 {
    HttpServer {
        request: SemanticHostSourceBindingV1,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disconnect: Option<SemanticHostSourceBindingV1>,
        response: SemanticHostOutputBindingV1,
    },
    WebSocketServer {
        open: SemanticHostSourceBindingV1,
        message: SemanticHostSourceBindingV1,
        close: SemanticHostSourceBindingV1,
        error: SemanticHostSourceBindingV1,
        actions: SemanticHostOutputBindingV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticLoweringContractError {
    message: String,
}

impl SemanticLoweringContractError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SemanticLoweringContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SemanticLoweringContractError {}

/// Build the deterministic semantic A-C authority from the opaque checked
/// artifact and already-normalized semantic graphs.
pub(crate) fn build_semantic_lowering_contract(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    out_net: &ResolvedOutGraph,
) -> Result<SemanticLoweringContractV1, SemanticLoweringContractError> {
    execution
        .validate(out_net)
        .map_err(SemanticLoweringContractError::new)?;
    resources
        .validate(execution, out_net)
        .map_err(SemanticLoweringContractError::new)?;
    reactive
        .validate(execution, resources, out_net)
        .map_err(|error| SemanticLoweringContractError::new(error.to_string()))?;

    build_semantic_lowering_contract_from_validated_inputs(checked, execution, resources, reactive)
}

pub(crate) fn build_semantic_lowering_contract_from_validated_inputs(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
) -> Result<SemanticLoweringContractV1, SemanticLoweringContractError> {
    let trace = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
    macro_rules! lowering_phase {
        ($name:literal, $expression:expr) => {{
            let started = trace.then(std::time::Instant::now);
            let result = $expression;
            if let Some(started) = started {
                eprintln!(
                    concat!(
                        "boon_semantic lowering_contract ",
                        $name,
                        ":done elapsed_ms={:.3}"
                    ),
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
            result
        }};
    }
    let metadata = lowering_phase!(
        "metadata",
        build_lowering_metadata(checked, execution, resources, reactive)
    )?;
    let output_contracts = lowering_phase!(
        "output_contracts",
        build_output_contracts(checked, execution, reactive, &metadata.render_slots)
    )?;
    let host_ports = lowering_phase!(
        "host_ports",
        build_host_ports(checked, resources, &output_contracts)
    )?;
    let transient_collections = lowering_phase!(
        "transient_collections",
        build_transient_collections(execution, resources)
    )?;
    let mut contract = SemanticLoweringContractV1 {
        schema: SEMANTIC_LOWERING_CONTRACT_SCHEMA_V1.to_owned(),
        metadata,
        output_contracts,
        host_ports,
        transient_collections,
        digest: SemanticLoweringContractDigestV1([0; 32]),
    };
    contract.digest = lowering_phase!("digest", lowering_contract_digest(&contract))?;
    Ok(contract)
}

impl SemanticLoweringContractV1 {
    /// Fail closed by re-deriving all checked-to-semantic joins and both
    /// canonical digests.
    pub(crate) fn validate(
        &self,
        checked: &CheckedProgram,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        out_net: &ResolvedOutGraph,
    ) -> Result<(), SemanticLoweringContractError> {
        let expected =
            build_semantic_lowering_contract(checked, execution, resources, reactive, out_net)?;
        if self != &expected {
            return Err(SemanticLoweringContractError::new(
                "semantic lowering contract differs from its deterministic semantic derivation",
            ));
        }
        Ok(())
    }
}

fn build_lowering_metadata(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
) -> Result<SemanticLoweringMetadataV1, SemanticLoweringContractError> {
    let lowering = &checked.lowering_metadata;
    let source_units = lowering
        .source_units
        .iter()
        .enumerate()
        .map(|(index, source)| SemanticSourceUnitV1 {
            id: SemanticSourceUnitId(index),
            path: source.path.clone(),
            module: source.module.clone(),
            start_line: source.start_line,
            line_count: source.line_count,
        })
        .collect::<Vec<_>>();
    validate_source_units(&source_units)?;

    let expression_types = build_expression_types(checked, execution)?;
    let function_types = build_function_types(checked, execution)?;
    let named_value_types = build_named_value_types(checked, execution, resources, reactive)?;
    let source_payload_shapes = build_source_payload_shapes(checked, resources)?;

    let mut diagnostics = Vec::new();
    let program_diagnostics = lowering
        .diagnostics
        .iter()
        .map(|diagnostic| push_diagnostic(&mut diagnostics, diagnostic))
        .collect();
    let render_slots = build_render_slots(checked, execution, &mut diagnostics)?;

    if lowering.checked_expression_count != checked.expressions.len() {
        return Err(SemanticLoweringContractError::new(format!(
            "checked lowering metadata reports {} checked expressions, but the opaque checked graph has {}",
            lowering.checked_expression_count,
            checked.expressions.len()
        )));
    }

    let mut metadata = SemanticLoweringMetadataV1 {
        schema: SEMANTIC_LOWERING_METADATA_SCHEMA_V1.to_owned(),
        source_bundle_digest_v1: checked.source_bundle_digest_v1,
        original_source_expression_count: lowering.original_source_expression_count,
        checked_expression_count: lowering.checked_expression_count,
        dynamic_fallback_count: lowering.dynamic_fallback_count,
        source_units,
        expression_types,
        function_types,
        named_value_types,
        render_slots,
        source_payload_shapes,
        diagnostics,
        program_diagnostics,
        digest: SemanticLoweringMetadataDigestV1([0; 32]),
    };
    metadata.digest = lowering_metadata_digest(&metadata)?;
    Ok(metadata)
}

fn validate_source_units(
    source_units: &[SemanticSourceUnitV1],
) -> Result<(), SemanticLoweringContractError> {
    let mut paths = BTreeSet::new();
    for (index, source) in source_units.iter().enumerate() {
        if source.id != SemanticSourceUnitId(index) {
            return Err(SemanticLoweringContractError::new(
                "semantic source-unit IDs are not dense",
            ));
        }
        if source.path.is_empty() || source.line_count == 0 {
            return Err(SemanticLoweringContractError::new(format!(
                "semantic source unit {} has an empty path or zero line count",
                source.id
            )));
        }
        if !paths.insert(source.path.as_str()) {
            return Err(SemanticLoweringContractError::new(format!(
                "semantic lowering metadata contains duplicate source-unit path `{}`",
                source.path
            )));
        }
    }
    Ok(())
}

fn build_expression_types(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
) -> Result<Vec<SemanticSourceExpressionTypeV1>, SemanticLoweringContractError> {
    let lowering = &checked.lowering_metadata;
    let mut occurrences_by_checked = vec![Vec::new(); checked.expressions.len()];
    for origin in &execution.checked_expression_origins {
        let checked_index = origin.checked_expression.0 as usize;
        let Some(occurrences) = occurrences_by_checked.get_mut(checked_index) else {
            return Err(SemanticLoweringContractError::new(format!(
                "semantic occurrence {} references out-of-range checked expression {}",
                origin.expression, origin.checked_expression.0
            )));
        };
        occurrences.push(origin.expression);
    }
    for occurrences in &mut occurrences_by_checked {
        occurrences.sort();
        occurrences.dedup();
    }
    let mut entries = lowering.expr_type_table.entries.iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.expr_id);
    if entries.len() != lowering.original_source_expression_count {
        return Err(SemanticLoweringContractError::new(format!(
            "source expression type table has {} entries, expected total source coverage {}",
            entries.len(),
            lowering.original_source_expression_count
        )));
    }

    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            if entry.expr_id != index {
                return Err(SemanticLoweringContractError::new(format!(
                    "source expression type table is not dense at {index}; found {}",
                    entry.expr_id
                )));
            }
            let checked_id = CheckedExprId(u32::try_from(entry.expr_id).map_err(|_| {
                SemanticLoweringContractError::new(format!(
                    "source expression {} exceeds checked identity range",
                    entry.expr_id
                ))
            })?);
            let checked_expression = checked
                .expressions
                .get(entry.expr_id)
                .filter(|expression| expression.id == checked_id)
                .ok_or_else(|| {
                    SemanticLoweringContractError::new(format!(
                        "source expression {} has no dense opaque checked expression",
                        entry.expr_id
                    ))
                })?;
            if checked_expression.flow_type != entry.flow_type {
                return Err(SemanticLoweringContractError::new(format!(
                    "source expression {} type table differs from its checked expression",
                    entry.expr_id
                )));
            }
            let occurrence_ids = occurrences_by_checked
                .get(entry.expr_id)
                .expect("checked expression occurrence index is dense");
            // Function declarations and other source-only expressions can be
            // fully checked without producing a normalized runtime node. An
            // explicitly empty vector is the exact coverage result; it is not
            // a synthesized fallback identity.
            let occurrences = occurrence_ids
                .iter()
                .copied()
                .map(|occurrence| {
                    let expression = require_expression(execution, occurrence)?;
                    if expression.checked_expr_id != checked_id {
                        return Err(SemanticLoweringContractError::new(format!(
                            "semantic occurrence {occurrence} differs from source expression {} provenance",
                            entry.expr_id
                        )));
                    }
                    Ok(SemanticSourceExpressionOccurrenceV1 {
                        expression: occurrence,
                        flow_type: expression.flow_type.clone(),
                    })
                })
                .collect::<Result<Vec<_>, SemanticLoweringContractError>>()?;
            Ok(SemanticSourceExpressionTypeV1 {
                id: SemanticSourceExpressionId(index),
                checked_expression: checked_id,
                occurrences,
                flow_type: entry.flow_type.clone(),
            })
        })
        .collect()
}

fn build_function_types(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
) -> Result<Vec<SemanticFunctionTypeV1>, SemanticLoweringContractError> {
    let user_callables = execution
        .callables
        .iter()
        .filter(|callable| callable.kind == CheckedCallableKind::User)
        .collect::<Vec<_>>();
    if checked.lowering_metadata.function_type_table.entries.len() != user_callables.len() {
        return Err(SemanticLoweringContractError::new(format!(
            "function type table has {} entries for {} semantic user callables",
            checked.lowering_metadata.function_type_table.entries.len(),
            user_callables.len()
        )));
    }

    let mut used = BTreeSet::new();
    let mut functions = Vec::with_capacity(user_callables.len());
    for entry in &checked.lowering_metadata.function_type_table.entries {
        let matches = user_callables
            .iter()
            .filter(|callable| callable.checked_callable == entry.callable)
            .copied()
            .collect::<Vec<_>>();
        let [callable] = matches.as_slice() else {
            return Err(SemanticLoweringContractError::new(format!(
                "function type entry `{}` checked callable {} resolves to {} semantic user callables",
                entry.name,
                entry.callable.0,
                matches.len()
            )));
        };
        if callable.name != entry.name
            || callable.parameters.len() != entry.parameters.len()
            || callable.result != entry.result
            || callable.effect != entry.effect
        {
            return Err(SemanticLoweringContractError::new(format!(
                "function type entry `{}` differs from semantic callable {}",
                entry.name, callable.id
            )));
        }
        for (parameter, entry_parameter) in callable.parameters.iter().zip(&entry.parameters) {
            if parameter.formal != entry_parameter.formal
                || parameter.ordinal != entry_parameter.ordinal
                || parameter.name != entry_parameter.name
                || parameter.flow_type != entry_parameter.flow_type
            {
                return Err(SemanticLoweringContractError::new(format!(
                    "function type entry `{}` parameter {} differs from semantic parameter {:?}",
                    entry.name, entry_parameter.ordinal, parameter.id
                )));
            }
        }
        if !used.insert(callable.id) {
            return Err(SemanticLoweringContractError::new(format!(
                "semantic user callable {} is represented by multiple function type entries",
                callable.id
            )));
        }
        let parameters = callable
            .parameters
            .iter()
            .map(|parameter| SemanticFunctionParameterTypeV1 {
                parameter: parameter.id,
                formal: parameter.formal,
                ordinal: parameter.ordinal,
                name: parameter.name.clone(),
                flow_type: parameter.flow_type.clone(),
            })
            .collect();
        functions.push(SemanticFunctionTypeV1 {
            callable: callable.id,
            checked_callable: callable.checked_callable,
            name: entry.name.clone(),
            parameters,
            result: entry.result.clone(),
            effect: entry.effect,
        });
    }
    functions.sort_by_key(|function| function.callable);
    Ok(functions)
}

fn build_named_value_types(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
) -> Result<Vec<SemanticNamedValueTypeV1>, SemanticLoweringContractError> {
    let table = &checked.lowering_metadata.named_value_type_table;
    if table
        .checked_statement_sites
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err(SemanticLoweringContractError::new(
            "named-value checked statement sites are not strictly ordered",
        ));
    }
    let mut entries = table.entries.iter().collect::<Vec<_>>();
    let original_entries = entries.clone();
    entries.sort_by(|left, right| {
        left.origins
            .cmp(&right.origins)
            .then_with(|| left.path.cmp(&right.path))
    });
    if entries != original_entries {
        return Err(SemanticLoweringContractError::new(
            "named-value entries are not ordered by exact checked origins before diagnostic path",
        ));
    }
    let mut covered_sites = Vec::with_capacity(entries.len());
    let mut previous = None::<&[NamedValueTypeOrigin]>;
    let named_values = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            if entry.path.is_empty() {
                return Err(SemanticLoweringContractError::new(
                    "named-value type entry has an empty diagnostic path",
                ));
            }
            let identity = entry.origins.as_slice();
            if previous == Some(identity) {
                return Err(SemanticLoweringContractError::new(format!(
                    "named-value type table contains duplicate exact checked site for `{}`",
                    entry.path
                )));
            }
            previous = Some(identity);
            if entry.origins.is_empty() {
                return Err(SemanticLoweringContractError::new(format!(
                    "named-value type `{}` has no structural checked origin",
                    entry.path
                )));
            }
            let sites = entry
                .origins
                .iter()
                .filter_map(|origin| origin.statement)
                .collect::<BTreeSet<_>>();
            if sites.len() != 1 {
                return Err(SemanticLoweringContractError::new(format!(
                    "named-value type `{}` does not have one exact checked statement site",
                    entry.path
                )));
            }
            let checked_statement = *sites.iter().next().expect("one checked statement site");
            if entry
                .origins
                .iter()
                .any(|origin| origin.statement != Some(checked_statement))
            {
                return Err(SemanticLoweringContractError::new(format!(
                    "named-value type `{}` contains origin provenance outside checked statement {}",
                    entry.path, checked_statement.0
                )));
            }
            covered_sites.push(checked_statement);
            let origins = entry
                .origins
                .iter()
                .map(|origin| build_named_value_origin(origin, execution, resources, reactive))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(SemanticNamedValueTypeV1 {
                id: SemanticNamedValueId(index),
                checked_statement,
                diagnostic_path: entry.path.clone(),
                origins,
                flow_type: entry.flow_type.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    covered_sites.sort();
    if covered_sites != table.checked_statement_sites {
        return Err(SemanticLoweringContractError::new(format!(
            "semantic named-value sites do not exactly cover checked authority: entries {covered_sites:?}, table {:?}",
            table.checked_statement_sites
        )));
    }
    Ok(named_values)
}

fn build_named_value_origin(
    origin: &NamedValueTypeOrigin,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
) -> Result<SemanticNamedValueTypeOriginV1, SemanticLoweringContractError> {
    let mut statements = origin
        .statement
        .into_iter()
        .flat_map(|checked_statement| {
            execution.statements.iter().filter_map(move |statement| {
                matches!(
                    statement.origin,
                    SemanticStatementOrigin::Checked {
                        statement: candidate,
                    } if candidate == checked_statement
                )
                .then_some(statement.id)
            })
        })
        .collect::<Vec<_>>();
    statements.sort();
    statements.dedup();

    let mut expressions = origin
        .value
        .into_iter()
        .flat_map(|checked_expression| {
            execution
                .checked_expression_origins
                .iter()
                .filter(move |candidate| candidate.checked_expression == checked_expression)
                .map(|candidate| candidate.expression)
        })
        .collect::<Vec<_>>();
    expressions.sort();
    expressions.dedup();
    let values = expressions
        .iter()
        .map(|expression| {
            require_expression(execution, *expression).map(|expression| expression.value_id)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut sources = origin
        .source
        .into_iter()
        .flat_map(|checked_source| {
            resources.sources.iter().filter_map(move |source| {
                matches!(
                    source.origin,
                    SemanticSourceOrigin::Checked { source: candidate }
                        if candidate == checked_source
                )
                .then_some(source.id)
            })
        })
        .collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    let mut states = origin
        .state
        .into_iter()
        .flat_map(|checked_state| {
            resources
                .states
                .iter()
                .filter(move |state| state.checked_state == checked_state)
                .map(|state| state.id)
        })
        .collect::<Vec<_>>();
    states.sort();
    states.dedup();
    let mut lists = origin
        .list
        .into_iter()
        .flat_map(|checked_list| {
            resources.lists.iter().filter_map(move |list| {
                matches!(
                    list.origin,
                    crate::SemanticListResourceOriginV1::CheckedLiteral {
                        checked_list: candidate,
                    } if candidate == checked_list
                )
                .then_some(list.id)
            })
        })
        .collect::<Vec<_>>();
    // A computed named list can reuse a checked literal's keyed authority, so
    // the checked named-value row does not itself carry CheckedListId. Join
    // that occurrence through the exact declaration/statement/producer
    // identity instead of losing the list and later mistaking one nested row
    // field for the whole named value.
    lists.extend(resources.lists.iter().filter_map(|list| {
        (origin.declaration == Some(list.declaration)
            && statements.contains(&list.statement)
            && expressions.contains(&list.producer))
        .then_some(list.id)
    }));
    lists.sort();
    lists.dedup();
    let mut value_list_authorities = origin
        .list
        .into_iter()
        .flat_map(|checked_list| {
            resources
                .value_list_authorities
                .iter()
                .filter_map(move |authority| {
                    matches!(
                        authority.origin,
                        crate::SemanticListResourceOriginV1::CheckedLiteral {
                            checked_list: candidate,
                        } if candidate == checked_list
                    )
                    .then_some(authority.id)
                })
        })
        .collect::<Vec<_>>();
    value_list_authorities.extend(resources.value_list_authorities.iter().filter_map(
        |authority| {
            (origin.declaration == Some(authority.declaration)
                && statements.contains(&authority.statement)
                && expressions.contains(&authority.producer))
            .then_some(authority.id)
        },
    ));
    value_list_authorities.sort();
    value_list_authorities.dedup();

    if origin.source.is_some() && sources.is_empty()
        || origin.state.is_some() && states.is_empty()
        || origin.list.is_some() && lists.is_empty() && value_list_authorities.is_empty()
    {
        return Err(SemanticLoweringContractError::new(
            "named-value checked resource origin has no semantic resource identity",
        ));
    }

    let statement_set = statements.iter().copied().collect::<BTreeSet<_>>();
    let expression_set = expressions.iter().copied().collect::<BTreeSet<_>>();
    let source_set = sources.iter().copied().collect::<BTreeSet<_>>();
    let state_set = states.iter().copied().collect::<BTreeSet<_>>();
    let list_set = lists.iter().copied().collect::<BTreeSet<_>>();
    let value_list_set = value_list_authorities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut bindings = reactive
        .bindings
        .iter()
        .filter(|binding| {
            origin
                .declaration
                .is_none_or(|declaration| binding.declaration == declaration)
                && (statement_set.is_empty() || statement_set.contains(&binding.statement))
                && (expression_set.is_empty() || expression_set.contains(&binding.producer))
                && match binding.target {
                    SemanticBindingTargetV1::Field { .. } => {
                        source_set.is_empty()
                            && state_set.is_empty()
                            && list_set.is_empty()
                            && value_list_set.is_empty()
                    }
                    SemanticBindingTargetV1::Source { source } => source_set.contains(&source),
                    SemanticBindingTargetV1::State { state } => state_set.contains(&state),
                    SemanticBindingTargetV1::List { list } => list_set.contains(&list),
                }
        })
        .map(|binding| binding.id)
        .collect::<Vec<_>>();
    bindings.sort();
    bindings.dedup();

    if statements.is_empty()
        && expressions.is_empty()
        && sources.is_empty()
        && states.is_empty()
        && lists.is_empty()
        && value_list_authorities.is_empty()
        && bindings.is_empty()
    {
        return Err(SemanticLoweringContractError::new(
            "named-value checked origin has no semantic identity",
        ));
    }
    Ok(SemanticNamedValueTypeOriginV1 {
        checked: origin.clone(),
        statements,
        expressions,
        values,
        bindings,
        sources,
        states,
        lists,
        value_list_authorities,
    })
}

fn build_render_slots(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    diagnostics: &mut Vec<SemanticTypeDiagnosticV1>,
) -> Result<Vec<SemanticRenderSlotV1>, SemanticLoweringContractError> {
    let mut slots = checked
        .lowering_metadata
        .render_slot_table
        .slots
        .iter()
        .collect::<Vec<_>>();
    slots.sort_by_key(|slot| slot.slot_statement_id);
    let mut seen = BTreeSet::new();
    slots
        .into_iter()
        .enumerate()
        .map(|(index, slot)| {
            if !seen.insert(slot.slot_statement_id) {
                return Err(SemanticLoweringContractError::new(format!(
                    "render-slot table repeats checked statement {}",
                    slot.slot_statement_id
                )));
            }
            let checked_statement_id =
                CheckedStatementId(u32::try_from(slot.slot_statement_id).map_err(|_| {
                    SemanticLoweringContractError::new(format!(
                        "render-slot statement {} exceeds checked identity range",
                        slot.slot_statement_id
                    ))
                })?);
            let checked_statement = require_checked_statement(checked, checked_statement_id)?;
            let statement = exact_checked_statement(execution, checked_statement_id)?;
            let semantic_statement = require_statement(execution, statement)?;
            let value = match slot.value_expr_id {
                Some(raw) => {
                    let checked_value = CheckedExprId(u32::try_from(raw).map_err(|_| {
                        SemanticLoweringContractError::new(format!(
                            "render-slot value expression {raw} exceeds checked identity range"
                        ))
                    })?);
                    if checked_statement.value != Some(checked_value) {
                        return Err(SemanticLoweringContractError::new(format!(
                            "render-slot statement {} value {:?} differs from checked statement value {:?}",
                            slot.slot_statement_id, slot.value_expr_id, checked_statement.value
                        )));
                    }
                    let semantic_value = semantic_statement.value.ok_or_else(|| {
                        SemanticLoweringContractError::new(format!(
                            "render-slot semantic statement {statement} has no value"
                        ))
                    })?;
                    if require_expression(execution, semantic_value)?.checked_expr_id
                        != checked_value
                    {
                        return Err(SemanticLoweringContractError::new(format!(
                            "render-slot semantic value {semantic_value} has stale checked provenance"
                        )));
                    }
                    Some(semantic_value)
                }
                None => {
                    if checked_statement.value.is_some() || semantic_statement.value.is_some() {
                        return Err(SemanticLoweringContractError::new(format!(
                            "render-slot statement {} omits a value that exists in checked/semantic execution",
                            slot.slot_statement_id
                        )));
                    }
                    None
                }
            };
            let slot_diagnostics = slot
                .diagnostics
                .iter()
                .map(|diagnostic| push_diagnostic(diagnostics, diagnostic))
                .collect();
            Ok(SemanticRenderSlotV1 {
                id: SemanticRenderSlotId(index),
                statement,
                value,
                slot_name: slot.slot_name.clone(),
                expected_contract: slot.expected_contract.clone(),
                actual_type: slot.actual_type.clone(),
                diagnostics: slot_diagnostics,
            })
        })
        .collect()
}

fn build_source_payload_shapes(
    checked: &CheckedProgram,
    resources: &SemanticResourceGraphV1,
) -> Result<Vec<SemanticSourcePayloadShapeV1>, SemanticLoweringContractError> {
    let entries = &checked.lowering_metadata.source_payload_shape_table;
    validate_source_payload_shape_identity_coverage(checked, entries)?;
    let expected_checked_sources = checked
        .sources
        .iter()
        .map(|source| source.id)
        .collect::<Vec<_>>();
    let mut covered_checked_sources = Vec::new();
    let mut previous_first = None;
    for entry in entries {
        if entry.checked_sources.is_empty() {
            return Err(SemanticLoweringContractError::new(
                "source payload shape entry has no exact checked source identities",
            ));
        }
        if !entry
            .checked_sources
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err(SemanticLoweringContractError::new(format!(
                "source payload shape `{}` checked source identities are not strictly ordered",
                entry.diagnostic_path
            )));
        }
        let first = entry.checked_sources[0];
        if previous_first.is_some_and(|previous| previous >= first) {
            return Err(SemanticLoweringContractError::new(
                "source payload shape entries are not ordered by exact checked source identity",
            ));
        }
        previous_first = Some(first);
        covered_checked_sources.extend(entry.checked_sources.iter().copied());
    }
    if covered_checked_sources != expected_checked_sources {
        return Err(SemanticLoweringContractError::new(format!(
            "source payload shapes do not exactly cover checked source identities: table {:?}, checked {:?}",
            covered_checked_sources, expected_checked_sources
        )));
    }

    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let checked_source_set = entry
                .checked_sources
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            for checked_source_id in &entry.checked_sources {
                let checked_source = checked
                    .sources
                    .iter()
                    .find(|source| source.id == *checked_source_id)
                    .ok_or_else(|| {
                        SemanticLoweringContractError::new(format!(
                            "source payload shape `{}` references missing checked source {}",
                            entry.diagnostic_path, checked_source_id.0
                        ))
                    })?;
                let diagnostic_path =
                    checked.semantic_path(&checked_source.path).ok_or_else(|| {
                        SemanticLoweringContractError::new(format!(
                            "checked source {} has no diagnostic semantic path",
                            checked_source.id.0
                        ))
                    })?;
                if diagnostic_path != entry.diagnostic_path {
                    return Err(SemanticLoweringContractError::new(format!(
                        "source payload shape checked source {} diagnostic path `{diagnostic_path}` differs from `{}`",
                        checked_source.id.0, entry.diagnostic_path
                    )));
                }
                if checked_source.payload_type != entry.payload_type {
                    return Err(SemanticLoweringContractError::new(format!(
                        "source payload shape checked source {} payload type differs",
                        checked_source.id.0
                    )));
                }
            }
            let mut sources = resources
                .sources
                .iter()
                .filter_map(|source| match source.origin {
                    SemanticSourceOrigin::Checked { source: checked_source }
                        if checked_source_set.contains(&checked_source) =>
                    {
                        Some(source.id)
                    }
                    SemanticSourceOrigin::Checked { .. }
                    | SemanticSourceOrigin::ProducerInvocation { .. } => None,
                })
                .collect::<Vec<_>>();
            sources.sort();
            sources.dedup();
            let expected_fields = payload_fields(&entry.payload_type);
            let actual_fields = entry
                .fields
                .iter()
                .map(|field| SemanticSourcePayloadFieldV1 {
                    name: field.name.clone(),
                    data_type: field.ty.clone(),
                })
                .collect::<Vec<_>>();
            if actual_fields != expected_fields {
                return Err(SemanticLoweringContractError::new(format!(
                    "source payload shape `{}` field table differs from its payload type",
                    entry.diagnostic_path
                )));
            }
            for source_id in &sources {
                let resource = resources
                    .sources
                    .get(source_id.as_usize())
                    .filter(|source| source.id == *source_id)
                    .ok_or_else(|| {
                        SemanticLoweringContractError::new(format!(
                            "source payload shape `{}` references missing semantic source {source_id}",
                            entry.diagnostic_path
                        ))
                    })?;
                let SemanticSourceOrigin::Checked {
                    source: checked_source,
                } = resource.origin
                else {
                    return Err(SemanticLoweringContractError::new(
                        "source payload shape unexpectedly references a producer source",
                    ));
                };
                let checked_source = checked
                    .sources
                    .iter()
                    .find(|source| source.id == checked_source)
                    .ok_or_else(|| {
                        SemanticLoweringContractError::new(format!(
                            "semantic source {source_id} has missing checked provenance"
                        ))
                    })?;
                if resource.payload_type != checked_source.payload_type {
                    return Err(SemanticLoweringContractError::new(format!(
                        "semantic source {source_id} payload differs from checked source {}",
                        checked_source.id.0
                    )));
                }
            }
            Ok(SemanticSourcePayloadShapeV1 {
                id: SemanticSourcePayloadShapeId(index),
                diagnostic_path: entry.diagnostic_path.clone(),
                checked_sources: entry.checked_sources.clone(),
                sources,
                payload_type: entry.payload_type.clone(),
                fields: actual_fields,
            })
        })
        .collect()
}

fn validate_source_payload_shape_identity_coverage(
    checked: &CheckedProgram,
    entries: &[boon_typecheck::SourcePayloadShapeEntry],
) -> Result<(), SemanticLoweringContractError> {
    let expected = checked
        .sources
        .iter()
        .map(|source| source.id)
        .collect::<Vec<_>>();
    let mut actual = Vec::new();
    let mut previous_first = None;
    for entry in entries {
        let Some(first) = entry.checked_sources.first().copied() else {
            return Err(SemanticLoweringContractError::new(
                "source payload shape entry has no exact checked source identities",
            ));
        };
        if !entry
            .checked_sources
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err(SemanticLoweringContractError::new(format!(
                "source payload shape `{}` checked source identities are not strictly ordered",
                entry.diagnostic_path
            )));
        }
        if previous_first.is_some_and(|previous| previous >= first) {
            return Err(SemanticLoweringContractError::new(
                "source payload shape entries are not ordered by exact checked source identity",
            ));
        }
        previous_first = Some(first);
        actual.extend(entry.checked_sources.iter().copied());
    }
    if actual != expected {
        return Err(SemanticLoweringContractError::new(format!(
            "source payload shapes do not exactly cover checked source identities: table {actual:?}, checked {expected:?}"
        )));
    }
    Ok(())
}

fn payload_fields(payload_type: &Type) -> Vec<SemanticSourcePayloadFieldV1> {
    let Type::Object(shape) = payload_type else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    shape
        .field_order
        .iter()
        .chain(shape.fields.keys())
        .filter_map(|name| {
            if !seen.insert(name.clone()) {
                return None;
            }
            shape
                .fields
                .get(name)
                .map(|data_type| SemanticSourcePayloadFieldV1 {
                    name: name.clone(),
                    data_type: data_type.clone(),
                })
        })
        .collect()
}

fn push_diagnostic(
    diagnostics: &mut Vec<SemanticTypeDiagnosticV1>,
    diagnostic: &TypeDiagnostic,
) -> SemanticDiagnosticId {
    let id = SemanticDiagnosticId(diagnostics.len());
    diagnostics.push(SemanticTypeDiagnosticV1 {
        id,
        severity: diagnostic.severity,
        line: diagnostic.line,
        start: diagnostic.start,
        end: diagnostic.end,
        message: diagnostic.message.clone(),
    });
    id
}

#[derive(Clone)]
struct RawOutputContract {
    root: String,
    value_path: String,
    contract: SemanticOutputContractKindV1,
    data_type: Option<Type>,
    declaration: DeclId,
    checked_statement: CheckedStatementId,
    statement: SemanticStatementId,
    expression: SemanticExprId,
    value: SemanticValueId,
    line: usize,
}

fn build_output_contracts(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    reactive: &SemanticReactiveGraphV1,
    render_slots: &[SemanticRenderSlotV1],
) -> Result<Vec<SemanticOutputContractV1>, SemanticLoweringContractError> {
    execution
        .validate_checked_roots(checked)
        .map_err(SemanticLoweringContractError::new)?;
    let expected =
        checked_semantic_root_specs_v1(checked).map_err(SemanticLoweringContractError::new)?;
    if expected.len() != execution.roots.len() {
        return Err(SemanticLoweringContractError::new(format!(
            "semantic execution roots contain {} entries but lowering C expected {} exact output roots",
            execution.roots.len(),
            expected.len()
        )));
    }
    expected
        .into_iter()
        .zip(&execution.roots)
        .enumerate()
        .map(|(index, (expected, root))| {
            let contract = match root.kind {
                SemanticRootKindV1::RetainedVisualDocument => {
                    SemanticOutputContractKindV1::RetainedVisualDocument
                }
                SemanticRootKindV1::RetainedVisualScene => {
                    SemanticOutputContractKindV1::RetainedVisualScene
                }
                SemanticRootKindV1::HostValue => SemanticOutputContractKindV1::HostValue,
            };
            bind_output_contract(
                SemanticOutputContractId(index),
                RawOutputContract {
                    root: expected.root,
                    value_path: expected.value_path,
                    contract,
                    data_type: expected.data_type,
                    declaration: root.declaration,
                    checked_statement: root.checked_statement,
                    statement: root.statement,
                    expression: root.expression,
                    value: root.value,
                    line: expected.line,
                },
                execution,
                reactive,
                render_slots,
            )
        })
        .collect()
}

fn bind_output_contract(
    id: SemanticOutputContractId,
    declaration: RawOutputContract,
    execution: &SemanticExecutionGraphV1,
    reactive: &SemanticReactiveGraphV1,
    render_slots: &[SemanticRenderSlotV1],
) -> Result<SemanticOutputContractV1, SemanticLoweringContractError> {
    let statement = declaration.statement;
    let statement_definition = require_statement(execution, statement)?;
    if statement_definition.declaration != Some(declaration.declaration)
        || statement_definition.call_instance.is_some()
        || statement_definition.value != Some(declaration.expression)
    {
        return Err(SemanticLoweringContractError::new(format!(
            "output `{}` semantic statement {statement} differs from its checked declaration",
            declaration.value_path
        )));
    }
    let expression = declaration.expression;
    let expression_definition = require_expression(execution, expression)?;
    let value = declaration.value;
    if expression_definition.value_id != value {
        return Err(SemanticLoweringContractError::new(format!(
            "output `{}` semantic expression {expression} value {} differs from execution-root value {value}",
            declaration.value_path, expression_definition.value_id
        )));
    }
    let binding_matches = reactive
        .bindings
        .iter()
        .filter(|binding| {
            binding.declaration == declaration.declaration
                && binding.statement == statement
                && binding.call_instance.is_none()
                // A top-level output statement can execute the exact expanded
                // result of a user-call occurrence. Its lexical statement is
                // still global while the value correctly retains that
                // occurrence's static owner.
                && binding.owner == expression_definition.owner
                && binding.producer == expression
                && binding.value == value
        })
        .collect::<Vec<_>>();
    let [binding] = binding_matches.as_slice() else {
        let candidates = reactive
            .bindings
            .iter()
            .filter(|binding| binding.declaration == declaration.declaration)
            .map(|binding| {
                format!(
                    "{}[statement={},call_instance={:?},owner={:?},producer={},value={}]",
                    binding.id,
                    binding.statement,
                    binding.call_instance,
                    binding.owner,
                    binding.producer,
                    binding.value,
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(SemanticLoweringContractError::new(format!(
            "output `{}` resolves to {} exact semantic storage bindings for statement={statement}, expression={expression}, value={value}; declaration candidates: [{}]",
            declaration.value_path,
            binding_matches.len(),
            candidates,
        )));
    };
    let typed_contract_known = match declaration.contract {
        SemanticOutputContractKindV1::RetainedVisualDocument => exact_visual_contract_known(
            execution,
            statement,
            expression,
            SemanticOutputContractKindV1::RetainedVisualDocument,
            render_slots,
        )?,
        SemanticOutputContractKindV1::RetainedVisualScene => exact_visual_contract_known(
            execution,
            statement,
            expression,
            SemanticOutputContractKindV1::RetainedVisualScene,
            render_slots,
        )?,
        SemanticOutputContractKindV1::HostValue => declaration
            .data_type
            .as_ref()
            .is_some_and(type_is_closed_host_data),
    };
    Ok(SemanticOutputContractV1 {
        id,
        ordinal: id.as_usize(),
        root: declaration.root,
        value_path: declaration.value_path,
        contract: declaration.contract,
        demand: SemanticOutputDemandPolicyV1::HostDemanded,
        data_type: declaration.data_type,
        declaration: declaration.declaration,
        checked_statement: declaration.checked_statement,
        statement,
        expression,
        value,
        binding: binding.id,
        field: match binding.target {
            SemanticBindingTargetV1::Field { field } => Some(field),
            SemanticBindingTargetV1::Source { .. }
            | SemanticBindingTargetV1::State { .. }
            | SemanticBindingTargetV1::List { .. } => None,
        },
        line: declaration.line,
        typed_contract_known,
    })
}

fn build_host_ports(
    checked: &CheckedProgram,
    resources: &SemanticResourceGraphV1,
    outputs: &[SemanticOutputContractV1],
) -> Result<Vec<SemanticHostPortBindingV1>, SemanticLoweringContractError> {
    let table = &checked.lowering_metadata.host_port_table;
    let mut ports = Vec::with_capacity(
        usize::from(table.http.is_some()) + usize::from(table.websocket.is_some()),
    );
    if let Some(http) = &table.http {
        ports.push(SemanticHostPortBindingV1 {
            id: SemanticHostPortId(ports.len()),
            line: http.line,
            kind: SemanticHostPortKindV1::HttpServer {
                request: resolve_host_source(checked, resources, &http.request)?,
                disconnect: http
                    .disconnect
                    .as_ref()
                    .map(|binding| resolve_host_source(checked, resources, binding))
                    .transpose()?,
                response: resolve_host_output(outputs, &http.response)?,
            },
        });
    }
    if let Some(websocket) = &table.websocket {
        ports.push(SemanticHostPortBindingV1 {
            id: SemanticHostPortId(ports.len()),
            line: websocket.line,
            kind: SemanticHostPortKindV1::WebSocketServer {
                open: resolve_host_source(checked, resources, &websocket.open)?,
                message: resolve_host_source(checked, resources, &websocket.message)?,
                close: resolve_host_source(checked, resources, &websocket.close)?,
                error: resolve_host_source(checked, resources, &websocket.error)?,
                actions: resolve_host_output(outputs, &websocket.actions)?,
            },
        });
    }
    Ok(ports)
}

fn resolve_host_source(
    checked: &CheckedProgram,
    resources: &SemanticResourceGraphV1,
    binding: &boon_typecheck::CheckedHostSourcePortBinding,
) -> Result<SemanticHostSourceBindingV1, SemanticLoweringContractError> {
    let checked_matches = checked
        .sources
        .iter()
        .filter(|source| source.id == binding.source)
        .collect::<Vec<_>>();
    let [checked_source_definition] = checked_matches.as_slice() else {
        return Err(SemanticLoweringContractError::new(format!(
            "host source `{}` checked identity {} resolves to {} checked sources",
            binding.diagnostic_path,
            binding.source.0,
            checked_matches.len()
        )));
    };
    if checked
        .semantic_path(&checked_source_definition.path)
        .as_deref()
        != Some(binding.diagnostic_path.as_str())
    {
        return Err(SemanticLoweringContractError::new(format!(
            "host source {} diagnostic path `{}` differs from its checked source",
            binding.source.0, binding.diagnostic_path
        )));
    }
    let matches = resources
        .sources
        .iter()
        .filter(|source| {
            matches!(
                source.origin,
                SemanticSourceOrigin::Checked { source: candidate }
                    if candidate == binding.source
            )
        })
        .collect::<Vec<_>>();
    let [source] = matches.as_slice() else {
        return Err(SemanticLoweringContractError::new(format!(
            "host source `{}` checked identity {} resolves to {} semantic sources",
            binding.diagnostic_path,
            binding.source.0,
            matches.len()
        )));
    };
    Ok(SemanticHostSourceBindingV1 {
        source: source.id,
        diagnostic_path: binding.diagnostic_path.clone(),
    })
}

fn resolve_host_output(
    outputs: &[SemanticOutputContractV1],
    binding: &boon_typecheck::CheckedHostOutputPortBinding,
) -> Result<SemanticHostOutputBindingV1, SemanticLoweringContractError> {
    let matches = outputs
        .iter()
        .filter(|output| {
            output.contract == SemanticOutputContractKindV1::HostValue
                && output.declaration == binding.declaration
                && output.checked_statement == binding.statement
        })
        .collect::<Vec<_>>();
    let [output] = matches.as_slice() else {
        return Err(SemanticLoweringContractError::new(format!(
            "host output `{}` declaration {} statement {} resolves to {} semantic host output contracts",
            binding.diagnostic_name,
            binding.declaration.0,
            binding.statement.0,
            matches.len()
        )));
    };
    if output.root != binding.diagnostic_name {
        return Err(SemanticLoweringContractError::new(format!(
            "host output statement {} diagnostic name `{}` differs from output contract `{}`",
            binding.statement.0, binding.diagnostic_name, output.root
        )));
    }
    Ok(SemanticHostOutputBindingV1 {
        output: output.id,
        output_ordinal: output.ordinal,
        statement: output.statement,
        diagnostic_name: binding.diagnostic_name.clone(),
    })
}

fn exact_checked_statement(
    execution: &SemanticExecutionGraphV1,
    checked_statement: CheckedStatementId,
) -> Result<SemanticStatementId, SemanticLoweringContractError> {
    let matches = execution
        .statements
        .iter()
        .filter(|statement| {
            statement.call_instance.is_none()
                && matches!(
                    statement.origin,
                    SemanticStatementOrigin::Checked { statement: candidate }
                        if candidate == checked_statement
                )
        })
        .map(|statement| statement.id)
        .collect::<Vec<_>>();
    let [statement] = matches.as_slice() else {
        return Err(SemanticLoweringContractError::new(format!(
            "checked statement {} resolves to {} exact root semantic statements",
            checked_statement.0,
            matches.len()
        )));
    };
    Ok(*statement)
}

fn require_checked_statement(
    checked: &CheckedProgram,
    id: CheckedStatementId,
) -> Result<&boon_typecheck::CheckedStatement, SemanticLoweringContractError> {
    let matches = checked
        .statements
        .iter()
        .filter(|statement| statement.id == id)
        .collect::<Vec<_>>();
    let [statement] = matches.as_slice() else {
        return Err(SemanticLoweringContractError::new(format!(
            "checked statement {} resolves to {} opaque checked statements",
            id.0,
            matches.len()
        )));
    };
    Ok(*statement)
}

fn require_statement(
    execution: &SemanticExecutionGraphV1,
    id: SemanticStatementId,
) -> Result<&crate::SemanticStatement, SemanticLoweringContractError> {
    execution
        .statement(id)
        .map_err(SemanticLoweringContractError::new)
}

fn require_expression(
    execution: &SemanticExecutionGraphV1,
    id: SemanticExprId,
) -> Result<&crate::SemanticExpression, SemanticLoweringContractError> {
    execution
        .expression(id)
        .map_err(SemanticLoweringContractError::new)
}

fn exact_visual_contract_known(
    execution: &SemanticExecutionGraphV1,
    root_statement: SemanticStatementId,
    root: SemanticExprId,
    contract: SemanticOutputContractKindV1,
    render_slots: &[SemanticRenderSlotV1],
) -> Result<bool, SemanticLoweringContractError> {
    if !resolved_visual_type(&require_expression(execution, root)?.flow_type.ty) {
        return Ok(false);
    }
    for slot in render_slots {
        if statement_descends_from(execution, slot.statement, root_statement)?
            && (!slot.diagnostics.is_empty()
                || slot.expected_contract.is_empty()
                || !resolved_visual_type(&slot.actual_type))
        {
            return Ok(false);
        }
    }

    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = require_expression(execution, expression_id)?;
        if let SemanticExpressionKind::Call {
            call,
            callable,
            callable_kind,
            name,
            function,
            result,
            parameter_bindings,
            ..
        } = &expression.kind
            && exact_visual_constructor(contract, function)
        {
            let callable_definition = execution
                .callables
                .get(callable.as_usize())
                .filter(|candidate| candidate.id == *callable)
                .ok_or_else(|| {
                    SemanticLoweringContractError::new(format!(
                        "visual constructor expression {expression_id} references missing callable {callable}"
                    ))
                })?;
            let call_definition = execution
                .calls
                .get(call.as_usize())
                .filter(|candidate| candidate.id == *call)
                .ok_or_else(|| {
                    SemanticLoweringContractError::new(format!(
                        "visual constructor expression {expression_id} references missing call {call}"
                    ))
                })?;
            let value_parameters = callable_definition
                .parameters
                .iter()
                .filter(|parameter| parameter.kind == boon_typecheck::CheckedParameterKind::Value)
                .collect::<Vec<_>>();
            let exact_formals = parameter_bindings.len() == value_parameters.len()
                && parameter_bindings
                    .iter()
                    .zip(value_parameters)
                    .all(|(binding, parameter)| {
                        binding.formal == parameter.formal
                            && binding.ordinal == parameter.ordinal
                            && binding.name == parameter.name
                    });
            if *callable_kind != crate::SemanticCallableKind::Builtin
                || callable_definition.kind != CheckedCallableKind::Builtin
                || call_definition.callable != *callable
                || call_definition.function != *function
                || callable_definition.name != *name
                || call_definition.result != *result
                || call_definition.checked_expression != expression.checked_expr_id
                || !exact_formals
            {
                return Err(SemanticLoweringContractError::new(format!(
                    "visual constructor expression {expression_id} lacks exact builtin callable/formal identity: \
                     callable kind={callable_kind:?}/{:?}, call callable={}/{callable}, \
                     function={:?}/{function:?}, name={:?}/{name:?}, \
                     call result={:?}/{result:?}, \
                     checked expression={:?}/{:?}, exact formals={exact_formals}",
                    callable_definition.kind,
                    call_definition.callable,
                    call_definition.function,
                    callable_definition.name,
                    call_definition.result,
                    call_definition.checked_expression,
                    expression.checked_expr_id,
                )));
            }
            if resolved_visual_type(&result.ty) {
                return Ok(true);
            }
        }
        pending.extend(expression.kind.direct_children());
    }
    Ok(false)
}

fn exact_visual_constructor(contract: SemanticOutputContractKindV1, function: &str) -> bool {
    const DOCUMENT_CONSTRUCTORS: &[&str] = &[
        "Document/new",
        "Element/container",
        "Element/stripe",
        "Element/text",
        "Element/label",
        "Element/paragraph",
        "Element/link",
        "Element/button",
        "Element/checkbox",
        "Element/text_input",
        "Element/program",
        "Element/embedded_media",
        "Element/map",
    ];
    const SCENE_CONSTRUCTORS: &[&str] = &[
        "Scene/new",
        "Scene/Element/stripe",
        "Scene/Element/block",
        "Scene/Element/text",
        "Scene/Element/text_input",
        "Scene/Element/program",
        "Scene/Element/checkbox",
        "Scene/Element/label",
        "Scene/Element/button",
        "Scene/Element/paragraph",
        "Scene/Element/link",
        "Scene/Element/embedded_media",
        "Scene/Element/map",
    ];
    match contract {
        SemanticOutputContractKindV1::RetainedVisualDocument => {
            DOCUMENT_CONSTRUCTORS.contains(&function)
        }
        SemanticOutputContractKindV1::RetainedVisualScene => SCENE_CONSTRUCTORS.contains(&function),
        SemanticOutputContractKindV1::HostValue => false,
    }
}

fn resolved_visual_type(ty: &Type) -> bool {
    match ty {
        Type::Unknown | Type::UnresolvedShape { .. } | Type::Var(_) | Type::Function { .. } => {
            false
        }
        Type::List(item) | Type::Set(item) => resolved_visual_type(item),
        Type::Map { key, value } => resolved_visual_type(key) && resolved_visual_type(value),
        Type::Union(members) => !members.is_empty() && members.iter().all(resolved_visual_type),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::VariantSet(_)
        | Type::Object(_)
        | Type::RenderContract => true,
    }
}

const MAX_TRANSIENT_COLLECTION_OPERATIONS: usize = 4_096;

#[derive(Clone, Debug)]
struct TransientAuthorityTrace {
    constructor: SemanticExprId,
    flow: Vec<SemanticExprId>,
    steps: Vec<SemanticTransientCollectionStepV1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransientCollectionTerminal {
    ListGet,
    ListLength,
    ListIsNotEmpty,
    MapGet,
    SetContains,
}

fn transient_local_values(
    execution: &SemanticExecutionGraphV1,
) -> Result<BTreeMap<SemanticLocalBindingId, SemanticExprId>, SemanticLoweringContractError> {
    let mut local_values = BTreeMap::new();
    for expression in &execution.expressions {
        let SemanticExpressionKind::Block { bindings, .. } = &expression.kind else {
            continue;
        };
        for binding in bindings {
            if local_values.insert(binding.id, binding.value).is_some() {
                return Err(SemanticLoweringContractError::new(format!(
                    "semantic transient analysis found duplicate local binding {}",
                    binding.id
                )));
            }
        }
    }
    Ok(local_values)
}

fn reachable_lowering_expressions(
    execution: &SemanticExecutionGraphV1,
    local_values: &BTreeMap<SemanticLocalBindingId, SemanticExprId>,
) -> Result<BTreeSet<SemanticExprId>, SemanticLoweringContractError> {
    let child_statements = execution
        .statements
        .iter()
        .flat_map(|statement| statement.children.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut pending = execution
        .statements
        .iter()
        .filter(|statement| !child_statements.contains(&statement.id))
        .filter_map(|statement| statement.value)
        .chain(execution.roots.iter().map(|root| root.expression))
        .chain(execution.functions.iter().map(|function| function.root))
        .chain(execution.sources.iter().map(|source| source.expression))
        .chain(execution.states.iter().map(|state| state.expression))
        .collect::<Vec<_>>();
    let mut reachable = BTreeSet::new();
    loop {
        while let Some(expression_id) = pending.pop() {
            if !reachable.insert(expression_id) {
                continue;
            }
            let expression = execution
                .expressions
                .get(expression_id.as_usize())
                .filter(|candidate| candidate.id == expression_id)
                .ok_or_else(|| {
                    SemanticLoweringContractError::new(format!(
                        "transient reachability references missing expression {expression_id}"
                    ))
                })?;
            if let SemanticExpressionKind::LocalRead { binding, .. } = &expression.kind {
                let value = local_values.get(binding).copied().ok_or_else(|| {
                    SemanticLoweringContractError::new(format!(
                        "semantic transient reachability cannot resolve local binding {binding} \
                         read by expression {expression_id}"
                    ))
                })?;
                pending.push(value);
            }
            pending.extend(expression.kind.direct_children());
        }
        let reverse_markers = execution
            .expressions
            .iter()
            .filter_map(|expression| match expression.kind {
                SemanticExpressionKind::Draining { input }
                    if reachable.contains(&input) && !reachable.contains(&expression.id) =>
                {
                    Some(expression.id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if reverse_markers.is_empty() {
            break;
        }
        pending.extend(reverse_markers);
    }
    Ok(reachable)
}

fn build_transient_collections(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
) -> Result<Vec<SemanticTransientCollectionV1>, SemanticLoweringContractError> {
    let local_values = transient_local_values(execution)?;
    let reachable = reachable_lowering_expressions(execution, &local_values)?;
    let durable_list_constructors = resources
        .lists
        .iter()
        .map(|list| list.producer)
        .chain(
            resources
                .value_list_authorities
                .iter()
                .filter(|authority| {
                    !matches!(authority.role, crate::SemanticValueListRoleV1::InlineValue)
                })
                .map(|authority| authority.producer),
        )
        .collect::<BTreeSet<_>>();
    // Value-consumer edges deliberately omit a BLOCK's declaration edge for
    // binding values. A binding owns a value lexically; its LocalRead nodes
    // are the actual consumers and are added as explicit alias edges below.
    let mut consumers = BTreeMap::<SemanticExprId, Vec<SemanticExprId>>::new();
    for expression in execution
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        match &expression.kind {
            SemanticExpressionKind::Block { result, .. } => {
                consumers.entry(*result).or_default().push(expression.id);
            }
            kind => {
                for child in kind.direct_children() {
                    consumers.entry(child).or_default().push(expression.id);
                }
            }
        }
        if let SemanticExpressionKind::LocalRead {
            binding,
            projection,
            ..
        } = &expression.kind
            && projection.is_empty()
        {
            let Some(value) = local_values.get(binding).copied() else {
                return Err(SemanticLoweringContractError::new(format!(
                    "semantic transient analysis cannot resolve local binding {binding}"
                )));
            };
            consumers.entry(value).or_default().push(expression.id);
        }
    }

    let mut claimed = BTreeSet::new();
    let mut regions = Vec::new();
    for terminal in execution
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        let SemanticExpressionKind::Call {
            callable_kind,
            function,
            effect,
            arguments,
            contexts,
            ..
        } = &terminal.kind
        else {
            continue;
        };
        if *callable_kind != crate::SemanticCallableKind::Builtin
            || *effect != CheckedEffectSummary::default()
            || !contexts.is_empty()
        {
            continue;
        }
        let (kind, terminal_kind, result_operand_name) = match function.as_str() {
            "List/get" => (
                SemanticTransientCollectionKindV1::List,
                TransientCollectionTerminal::ListGet,
                Some("position"),
            ),
            "List/count" | "List/length" => (
                SemanticTransientCollectionKindV1::List,
                TransientCollectionTerminal::ListLength,
                None,
            ),
            "List/is_not_empty" => (
                SemanticTransientCollectionKindV1::List,
                TransientCollectionTerminal::ListIsNotEmpty,
                None,
            ),
            "Map/get" => (
                SemanticTransientCollectionKindV1::Map,
                TransientCollectionTerminal::MapGet,
                Some("key"),
            ),
            "Set/contains" => (
                SemanticTransientCollectionKindV1::Set,
                TransientCollectionTerminal::SetContains,
                Some("item"),
            ),
            _ => continue,
        };
        let Some(receiver) =
            transient_collection_input(arguments, transient_collection_receiver_name(kind))
        else {
            continue;
        };
        let result_operand = match result_operand_name {
            Some(name) => {
                let Some(operand) = transient_named_input(arguments, name) else {
                    continue;
                };
                if !transient_operand_is_safe(execution, &local_values, operand) {
                    continue;
                }
                Some(operand)
            }
            None => None,
        };

        let Some(mut trace) = trace_transient_authority(
            execution,
            &local_values,
            receiver,
            kind,
            &mut BTreeSet::new(),
        ) else {
            continue;
        };
        trace.flow.push(terminal.id);
        if trace.flow.len() < 2
            || trace.flow.iter().copied().collect::<BTreeSet<_>>().len() != trace.flow.len()
            || trace
                .flow
                .windows(2)
                .any(|edge| consumers.get(&edge[0]).map(Vec::as_slice).unwrap_or(&[]) != [edge[1]])
            || trace
                .flow
                .iter()
                .any(|expression| claimed.contains(expression))
        {
            continue;
        }

        let Some(constructor) = execution
            .expressions
            .get(trace.constructor.as_usize())
            .filter(|candidate| candidate.id == trace.constructor)
        else {
            return Err(SemanticLoweringContractError::new(format!(
                "semantic transient constructor {} is missing",
                trace.constructor
            )));
        };
        if kind == SemanticTransientCollectionKindV1::List
            && durable_list_constructors.contains(&trace.constructor)
        {
            continue;
        }
        if !type_is_closed_host_data(&constructor.flow_type.ty) {
            continue;
        }

        let (list_items, map_entries, set_items, declared_capacity) =
            match (&kind, &constructor.kind) {
                (
                    SemanticTransientCollectionKindV1::List,
                    SemanticExpressionKind::List { capacity, items },
                ) => {
                    if items
                        .iter()
                        .any(|item| !transient_operand_is_safe(execution, &local_values, *item))
                    {
                        continue;
                    }
                    (items.clone(), Vec::new(), Vec::new(), *capacity)
                }
                (
                    SemanticTransientCollectionKindV1::Map,
                    SemanticExpressionKind::Map { entries },
                ) => {
                    let mut lowered = Vec::with_capacity(entries.len());
                    let mut valid = true;
                    for entry in entries {
                        let Some(entry) = execution
                            .expressions
                            .get(entry.as_usize())
                            .filter(|candidate| candidate.id == *entry)
                        else {
                            return Err(SemanticLoweringContractError::new(
                                "semantic transient MAP references a missing entry",
                            ));
                        };
                        let SemanticExpressionKind::MapEntry { key, value } = entry.kind else {
                            return Err(SemanticLoweringContractError::new(
                                "semantic transient MAP contains a non-entry expression",
                            ));
                        };
                        if !transient_operand_is_safe(execution, &local_values, key)
                            || !transient_operand_is_safe(execution, &local_values, value)
                        {
                            valid = false;
                            break;
                        }
                        lowered.push(SemanticTransientMapEntryV1 { key, value });
                    }
                    if !valid {
                        continue;
                    }
                    (Vec::new(), lowered, Vec::new(), None)
                }
                (SemanticTransientCollectionKindV1::Set, SemanticExpressionKind::Set { items }) => {
                    if items
                        .iter()
                        .any(|item| !transient_operand_is_safe(execution, &local_values, *item))
                    {
                        continue;
                    }
                    (Vec::new(), Vec::new(), items.clone(), None)
                }
                _ => continue,
            };

        let initial_len = list_items.len().max(map_entries.len()).max(set_items.len());
        let growth_steps = trace
            .steps
            .iter()
            .filter(|step| {
                matches!(
                    step,
                    SemanticTransientCollectionStepV1::ListAppend { .. }
                        | SemanticTransientCollectionStepV1::MapUpsert { .. }
                        | SemanticTransientCollectionStepV1::SetAdd { .. }
                )
            })
            .count();
        let attempted_storage_growth = initial_len.saturating_add(growth_steps);
        let storage_growth_budget = declared_capacity
            .map(|capacity| attempted_storage_growth.min(capacity))
            .unwrap_or(attempted_storage_growth);
        let operation_count = initial_len
            .saturating_add(trace.steps.len())
            .saturating_add(1);
        if operation_count > MAX_TRANSIENT_COLLECTION_OPERATIONS {
            continue;
        }
        let result = match terminal_kind {
            TransientCollectionTerminal::ListGet => SemanticTransientCollectionResultV1::ListGet {
                expression: terminal.id,
                position: result_operand.expect("LIST get terminal has a position"),
            },
            TransientCollectionTerminal::ListLength => {
                SemanticTransientCollectionResultV1::ListLength {
                    expression: terminal.id,
                }
            }
            TransientCollectionTerminal::ListIsNotEmpty => {
                SemanticTransientCollectionResultV1::ListIsNotEmpty {
                    expression: terminal.id,
                }
            }
            TransientCollectionTerminal::MapGet => SemanticTransientCollectionResultV1::MapGet {
                expression: terminal.id,
                key: result_operand.expect("MAP get terminal has a key"),
            },
            TransientCollectionTerminal::SetContains => {
                SemanticTransientCollectionResultV1::SetContains {
                    expression: terminal.id,
                    item: result_operand.expect("SET contains terminal has an item"),
                }
            }
        };
        claimed.extend(trace.flow.iter().copied());
        regions.push(SemanticTransientCollectionV1 {
            kind,
            constructor: trace.constructor,
            declared_capacity,
            list_items,
            map_entries,
            set_items,
            steps: trace.steps,
            result,
            authority_flow: trace.flow,
            operation_work_budget: u64::try_from(operation_count).unwrap_or(u64::MAX),
            storage_growth_budget,
            snapshot_copy_budget: 0,
        });
    }
    Ok(regions)
}

fn trace_transient_authority(
    execution: &SemanticExecutionGraphV1,
    local_values: &BTreeMap<SemanticLocalBindingId, SemanticExprId>,
    expression_id: SemanticExprId,
    kind: SemanticTransientCollectionKindV1,
    active: &mut BTreeSet<SemanticExprId>,
) -> Option<TransientAuthorityTrace> {
    if !active.insert(expression_id) {
        return None;
    }
    let expression = execution
        .expressions
        .get(expression_id.as_usize())
        .filter(|candidate| candidate.id == expression_id)?;
    let result = match &expression.kind {
        SemanticExpressionKind::List { .. } if kind == SemanticTransientCollectionKindV1::List => {
            Some(TransientAuthorityTrace {
                constructor: expression_id,
                flow: vec![expression_id],
                steps: Vec::new(),
            })
        }
        SemanticExpressionKind::Map { .. } if kind == SemanticTransientCollectionKindV1::Map => {
            Some(TransientAuthorityTrace {
                constructor: expression_id,
                flow: vec![expression_id],
                steps: Vec::new(),
            })
        }
        SemanticExpressionKind::Set { .. } if kind == SemanticTransientCollectionKindV1::Set => {
            Some(TransientAuthorityTrace {
                constructor: expression_id,
                flow: vec![expression_id],
                steps: Vec::new(),
            })
        }
        SemanticExpressionKind::LocalRead {
            binding,
            projection,
            ..
        } if projection.is_empty() => {
            let value = local_values.get(binding).copied()?;
            let mut trace =
                trace_transient_authority(execution, local_values, value, kind, active)?;
            trace.flow.push(expression_id);
            Some(trace)
        }
        SemanticExpressionKind::Call {
            callable_kind,
            function,
            effect,
            arguments,
            contexts,
            ..
        } if *callable_kind == crate::SemanticCallableKind::Builtin
            && *effect == CheckedEffectSummary::default()
            && contexts.is_empty() =>
        {
            let receiver =
                transient_collection_input(arguments, transient_collection_receiver_name(kind))?;
            let step = transient_collection_step(
                execution,
                local_values,
                expression_id,
                kind,
                function,
                arguments,
            )?;
            let mut trace =
                trace_transient_authority(execution, local_values, receiver, kind, active)?;
            trace.flow.push(expression_id);
            trace.steps.push(step);
            Some(trace)
        }
        _ => None,
    };
    active.remove(&expression_id);
    result
}

fn transient_collection_step(
    execution: &SemanticExecutionGraphV1,
    local_values: &BTreeMap<SemanticLocalBindingId, SemanticExprId>,
    expression: SemanticExprId,
    kind: SemanticTransientCollectionKindV1,
    function: &str,
    arguments: &[crate::SemanticCallArgument],
) -> Option<SemanticTransientCollectionStepV1> {
    let step = match (kind, function) {
        (SemanticTransientCollectionKindV1::List, "List/append") => {
            let item = transient_named_input(arguments, "item")?;
            if !transient_operand_is_safe(execution, local_values, item) {
                return None;
            }
            SemanticTransientCollectionStepV1::ListAppend { expression, item }
        }
        (SemanticTransientCollectionKindV1::Map, "Map/upsert") => {
            let entry = transient_named_input(arguments, "entry")?;
            let entry = execution
                .expressions
                .get(entry.as_usize())
                .filter(|candidate| candidate.id == entry)?;
            let SemanticExpressionKind::Object(fields) = &entry.kind else {
                return None;
            };
            if fields.len() != 2 || fields.iter().any(|field| field.spread) {
                return None;
            }
            let mut key = None;
            let mut value = None;
            for field in fields {
                match field.name.as_str() {
                    "key" if key.replace(field.value).is_none() => {}
                    "value" if value.replace(field.value).is_none() => {}
                    _ => return None,
                }
            }
            let key = key?;
            let value = value?;
            if !transient_operand_is_safe(execution, local_values, key)
                || !transient_operand_is_safe(execution, local_values, value)
            {
                return None;
            }
            SemanticTransientCollectionStepV1::MapUpsert {
                expression,
                key,
                value,
            }
        }
        (SemanticTransientCollectionKindV1::Map, "Map/remove") => {
            let key = transient_named_input(arguments, "key")?;
            if !transient_operand_is_safe(execution, local_values, key) {
                return None;
            }
            SemanticTransientCollectionStepV1::MapRemove { expression, key }
        }
        (SemanticTransientCollectionKindV1::Set, "Set/add") => {
            let item = transient_named_input(arguments, "item")?;
            if !transient_operand_is_safe(execution, local_values, item) {
                return None;
            }
            SemanticTransientCollectionStepV1::SetAdd { expression, item }
        }
        (SemanticTransientCollectionKindV1::Set, "Set/remove") => {
            let item = transient_named_input(arguments, "item")?;
            if !transient_operand_is_safe(execution, local_values, item) {
                return None;
            }
            SemanticTransientCollectionStepV1::SetRemove { expression, item }
        }
        _ => return None,
    };
    Some(step)
}

const fn transient_collection_receiver_name(
    kind: SemanticTransientCollectionKindV1,
) -> &'static str {
    match kind {
        SemanticTransientCollectionKindV1::List => "list",
        SemanticTransientCollectionKindV1::Map => "map",
        SemanticTransientCollectionKindV1::Set => "set",
    }
}

fn transient_collection_input(
    arguments: &[crate::SemanticCallArgument],
    receiver_name: &str,
) -> Option<SemanticExprId> {
    let pipe_inputs = arguments
        .iter()
        .filter(|argument| argument.from_pipe)
        .map(|argument| argument.value)
        .collect::<Vec<_>>();
    let named_inputs = arguments
        .iter()
        .filter(|argument| argument.name == receiver_name && !argument.from_pipe)
        .map(|argument| argument.value)
        .collect::<Vec<_>>();
    match (pipe_inputs.as_slice(), named_inputs.as_slice()) {
        ([input], []) | ([], [input]) => Some(*input),
        _ => None,
    }
}

fn transient_named_input(
    arguments: &[crate::SemanticCallArgument],
    name: &str,
) -> Option<SemanticExprId> {
    let mut inputs = arguments
        .iter()
        .filter(|argument| argument.name == name && !argument.from_pipe)
        .map(|argument| argument.value);
    let input = inputs.next()?;
    inputs.next().is_none().then_some(input)
}

fn transient_operand_is_safe(
    execution: &SemanticExecutionGraphV1,
    local_values: &BTreeMap<SemanticLocalBindingId, SemanticExprId>,
    root: SemanticExprId,
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let Some(expression) = execution
            .expressions
            .get(expression_id.as_usize())
            .filter(|candidate| candidate.id == expression_id)
        else {
            return false;
        };
        if expression.effect != CheckedEffectSummary::default()
            || type_contains_collection(&expression.flow_type.ty)
        {
            return false;
        }
        match &expression.kind {
            SemanticExpressionKind::CanonicalRead { .. }
            | SemanticExpressionKind::ExternalRead { .. }
            | SemanticExpressionKind::ElementState { .. }
            | SemanticExpressionKind::Drain { .. }
            | SemanticExpressionKind::Source { .. }
            | SemanticExpressionKind::Materialize { .. }
            | SemanticExpressionKind::Draining { .. }
            | SemanticExpressionKind::Hold { .. }
            | SemanticExpressionKind::Latest { .. }
            | SemanticExpressionKind::When { .. }
            | SemanticExpressionKind::Then { .. }
            | SemanticExpressionKind::Flush { .. }
            | SemanticExpressionKind::FlushBoundary { .. }
            | SemanticExpressionKind::MaterializationLocal { .. }
            | SemanticExpressionKind::Map { .. }
            | SemanticExpressionKind::Set { .. }
            | SemanticExpressionKind::List { .. } => return false,
            SemanticExpressionKind::Call {
                callable_kind,
                contexts,
                ..
            } if *callable_kind != crate::SemanticCallableKind::Builtin || !contexts.is_empty() => {
                return false;
            }
            SemanticExpressionKind::LocalRead {
                binding,
                projection,
                ..
            } if projection.is_empty() => {
                let Some(value) = local_values.get(binding).copied() else {
                    return false;
                };
                pending.push(value);
            }
            _ => {}
        }
        pending.extend(expression.kind.direct_children());
    }
    true
}

fn type_contains_collection(data_type: &Type) -> bool {
    match data_type {
        Type::List(_) | Type::Set(_) | Type::Map { .. } => true,
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields.fields.values().any(type_contains_collection),
        }),
        Type::Object(shape) => shape.fields.values().any(type_contains_collection),
        Type::Union(members) => members.iter().any(type_contains_collection),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::Function { .. }
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

fn statement_descends_from(
    execution: &SemanticExecutionGraphV1,
    candidate: SemanticStatementId,
    root: SemanticStatementId,
) -> Result<bool, SemanticLoweringContractError> {
    let mut current = Some(candidate);
    let mut visited = BTreeSet::new();
    while let Some(statement) = current {
        if !visited.insert(statement) {
            return Err(SemanticLoweringContractError::new(
                "semantic visual statement ancestry contains a cycle",
            ));
        }
        if statement == root {
            return Ok(true);
        }
        current = require_statement(execution, statement)?.parent;
    }
    Ok(false)
}

fn type_is_closed_host_data(data_type: &Type) -> bool {
    match data_type {
        Type::Text | Type::Number | Type::Bytes(_) | Type::Bits { .. } => true,
        Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            Variant::Tag(_) => true,
            Variant::Tagged { fields, .. } => {
                !fields.open && fields.fields.values().all(type_is_closed_host_data)
            }
        }),
        Type::Object(shape) => !shape.open && shape.fields.values().all(type_is_closed_host_data),
        Type::List(item) | Type::Set(item) => type_is_closed_host_data(item),
        Type::Map { key, value } => {
            type_is_closed_host_data(key) && type_is_closed_host_data(value)
        }
        Type::Union(members) => !members.is_empty() && members.iter().all(type_is_closed_host_data),
        Type::Absent
        | Type::Function { .. }
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

#[derive(Serialize)]
struct LoweringMetadataDigestPayload<'a> {
    schema: &'a str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    original_source_expression_count: usize,
    checked_expression_count: usize,
    dynamic_fallback_count: usize,
    source_units: &'a [SemanticSourceUnitV1],
    expression_types: &'a [SemanticSourceExpressionTypeV1],
    function_types: &'a [SemanticFunctionTypeV1],
    named_value_types: &'a [SemanticNamedValueTypeV1],
    render_slots: &'a [SemanticRenderSlotV1],
    source_payload_shapes: &'a [SemanticSourcePayloadShapeV1],
    diagnostics: &'a [SemanticTypeDiagnosticV1],
    program_diagnostics: &'a [SemanticDiagnosticId],
}

fn lowering_metadata_digest(
    metadata: &SemanticLoweringMetadataV1,
) -> Result<SemanticLoweringMetadataDigestV1, SemanticLoweringContractError> {
    let payload = LoweringMetadataDigestPayload {
        schema: &metadata.schema,
        source_bundle_digest_v1: metadata.source_bundle_digest_v1,
        original_source_expression_count: metadata.original_source_expression_count,
        checked_expression_count: metadata.checked_expression_count,
        dynamic_fallback_count: metadata.dynamic_fallback_count,
        source_units: &metadata.source_units,
        expression_types: &metadata.expression_types,
        function_types: &metadata.function_types,
        named_value_types: &metadata.named_value_types,
        render_slots: &metadata.render_slots,
        source_payload_shapes: &metadata.source_payload_shapes,
        diagnostics: &metadata.diagnostics,
        program_diagnostics: &metadata.program_diagnostics,
    };
    boon_contract::canonical_serde_hash_v1(SEMANTIC_LOWERING_METADATA_DIGEST_DOMAIN, &payload)
        .map(SemanticLoweringMetadataDigestV1)
        .map_err(|error| {
            SemanticLoweringContractError::new(format!(
                "failed to hash semantic lowering metadata: {error}"
            ))
        })
}

#[derive(Serialize)]
struct LoweringContractDigestPayload<'a> {
    schema: &'a str,
    metadata: &'a SemanticLoweringMetadataV1,
    output_contracts: &'a [SemanticOutputContractV1],
    host_ports: &'a [SemanticHostPortBindingV1],
    transient_collections: &'a [SemanticTransientCollectionV1],
}

fn lowering_contract_digest(
    contract: &SemanticLoweringContractV1,
) -> Result<SemanticLoweringContractDigestV1, SemanticLoweringContractError> {
    let payload = LoweringContractDigestPayload {
        schema: &contract.schema,
        metadata: &contract.metadata,
        output_contracts: &contract.output_contracts,
        host_ports: &contract.host_ports,
        transient_collections: &contract.transient_collections,
    };
    boon_contract::canonical_serde_hash_v1(SEMANTIC_LOWERING_CONTRACT_DIGEST_DOMAIN, &payload)
        .map(SemanticLoweringContractDigestV1)
        .map_err(|error| {
            SemanticLoweringContractError::new(format!(
                "failed to hash semantic lowering contract: {error}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_and_semantic(name: &str, source: &str) -> (CheckedProgram, crate::SemanticProgram) {
        let parsed = boon_parser::parse_source(name, source).expect("parse");
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "unexpected type diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("checked program");
        let semantic = crate::elaborate(checked.clone(), &[]).expect("semantic elaboration");
        (checked, semantic)
    }

    fn contract_for(
        checked: &CheckedProgram,
        semantic: &crate::SemanticProgram,
    ) -> SemanticLoweringContractV1 {
        build_semantic_lowering_contract(
            checked,
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.resolved_out_graph(),
        )
        .expect("semantic lowering contract")
    }

    #[test]
    fn transient_reachability_follows_nested_function_block_locals() {
        let (_, semantic) = checked_and_semantic(
            "nested-function-block-local.bn",
            r#"
store: [
    rows:
        LIST { [value: 1] }
        |> List/map(item, new: make_row(initial: item.value))
    count: rows |> List/count()
]

FUNCTION make_row(initial) {
    [
        edit: SOURCE
        edited: BLOCK {
            draft: LATEST {
                initial
                edit |> THEN { initial }
            }
            draft
        }
    ]
}
"#,
        );
        assert!(
            semantic
                .execution_graph()
                .expressions
                .iter()
                .any(|expression| matches!(
                    expression.kind,
                    SemanticExpressionKind::LocalRead { .. }
                )),
            "the fixture must retain a semantic local read"
        );
    }

    #[test]
    fn server_outputs_bind_exact_semantic_output_and_host_port_ids() {
        let (checked, semantic) = checked_and_semantic(
            "server-outputs.bn",
            include_str!("../../../examples/server_outputs.bn"),
        );
        let contract = contract_for(&checked, &semantic);
        assert_eq!(contract.output_contracts.len(), 2);
        assert!(
            contract
                .output_contracts
                .iter()
                .all(|output| output.contract == SemanticOutputContractKindV1::HostValue)
        );
        assert!(
            contract
                .output_contracts
                .iter()
                .all(|output| output.typed_contract_known)
        );
        let [port] = contract.host_ports.as_slice() else {
            panic!("expected exactly one host port");
        };
        let SemanticHostPortKindV1::HttpServer {
            request,
            disconnect,
            response,
        } = &port.kind
        else {
            panic!("expected HTTP host port");
        };
        assert_eq!(request.diagnostic_path, "store.request_received");
        assert!(disconnect.is_none());
        assert_eq!(response.diagnostic_name, "api_response");
        assert!(
            contract
                .output_contracts
                .iter()
                .any(|output| output.id == response.output && output.root == "api_response")
        );
        contract
            .validate(
                &checked,
                semantic.execution_graph(),
                semantic.resource_graph(),
                semantic.reactive_graph(),
                semantic.resolved_out_graph(),
            )
            .expect("fresh contract validates");
    }

    #[test]
    fn execution_output_roots_are_total_canonical_and_mutation_checked() {
        let (checked, semantic) = checked_and_semantic(
            "server-outputs.bn",
            include_str!("../../../examples/server_outputs.bn"),
        );
        let execution = semantic.execution_graph();
        let contract = contract_for(&checked, &semantic);
        assert!(!execution.roots.is_empty());
        assert_eq!(execution.roots.len(), contract.output_contracts.len());
        for (ordinal, (root, output)) in execution
            .roots
            .iter()
            .zip(&contract.output_contracts)
            .enumerate()
        {
            assert_eq!(root.ordinal, ordinal);
            assert_eq!(output.ordinal, ordinal);
            assert_eq!(root.declaration, output.declaration);
            assert_eq!(root.checked_statement, output.checked_statement);
            assert_eq!(root.statement, output.statement);
            assert_eq!(root.expression, output.expression);
            assert_eq!(root.value, output.value);
            assert_eq!(
                root.kind,
                match output.contract {
                    SemanticOutputContractKindV1::RetainedVisualDocument =>
                        SemanticRootKindV1::RetainedVisualDocument,
                    SemanticOutputContractKindV1::RetainedVisualScene =>
                        SemanticRootKindV1::RetainedVisualScene,
                    SemanticOutputContractKindV1::HostValue => SemanticRootKindV1::HostValue,
                }
            );
        }

        let mut reordered = execution.clone();
        reordered.roots.swap(0, 1);
        assert!(reordered.validate_checked_roots(&checked).is_err());

        let mut kind_mutation = execution.clone();
        kind_mutation.roots[0].kind = SemanticRootKindV1::RetainedVisualDocument;
        assert!(kind_mutation.validate_checked_roots(&checked).is_err());

        let mut statement_mutation = execution.clone();
        statement_mutation.roots[0].statement = SemanticStatementId(usize::MAX);
        assert!(statement_mutation.validate_checked_roots(&checked).is_err());

        let mut value_mutation = execution.clone();
        value_mutation.roots[0].value = SemanticValueId(usize::MAX);
        assert!(value_mutation.validate_checked_roots(&checked).is_err());

        let mut extra_non_output = execution.clone();
        let mut extra = extra_non_output.roots[0].clone();
        extra.ordinal = extra_non_output.roots.len();
        extra_non_output.roots.push(extra);
        assert!(extra_non_output.validate_checked_roots(&checked).is_err());
    }

    #[test]
    fn metadata_has_dense_total_expression_and_diagnostic_identity() {
        let (checked, semantic) = checked_and_semantic(
            "metadata.bn",
            r#"
FUNCTION increment(value) {
    value + 1
}
store: [
    value: 1 |> increment()
]
outputs: [
    answer: store.value
]
"#,
        );
        let contract = contract_for(&checked, &semantic);
        assert_eq!(
            contract.metadata.expression_types.len(),
            contract.metadata.original_source_expression_count
        );
        let mut occurrence_count = 0;
        for (index, expression) in contract.metadata.expression_types.iter().enumerate() {
            assert_eq!(expression.id, SemanticSourceExpressionId(index));
            for occurrence in &expression.occurrences {
                occurrence_count += 1;
                assert_eq!(
                    semantic.execution_graph().expressions[occurrence.expression.as_usize()]
                        .flow_type,
                    occurrence.flow_type
                );
            }
        }
        assert!(occurrence_count > 0);
        for (index, diagnostic) in contract.metadata.diagnostics.iter().enumerate() {
            assert_eq!(diagnostic.id, SemanticDiagnosticId(index));
        }
        assert_eq!(contract.metadata.function_types.len(), 1);
    }

    #[test]
    fn validation_rejects_metadata_output_and_host_identity_mutations() {
        let (checked, semantic) = checked_and_semantic(
            "server-outputs.bn",
            include_str!("../../../examples/server_outputs.bn"),
        );
        let fresh = contract_for(&checked, &semantic);

        let mut metadata_mutation = fresh.clone();
        metadata_mutation.metadata.original_source_expression_count += 1;
        assert!(
            metadata_mutation
                .validate(
                    &checked,
                    semantic.execution_graph(),
                    semantic.resource_graph(),
                    semantic.reactive_graph(),
                    semantic.resolved_out_graph(),
                )
                .is_err()
        );

        let mut output_mutation = fresh.clone();
        output_mutation.output_contracts[0].binding = SemanticBindingId(usize::MAX);
        assert!(
            output_mutation
                .validate(
                    &checked,
                    semantic.execution_graph(),
                    semantic.resource_graph(),
                    semantic.reactive_graph(),
                    semantic.resolved_out_graph(),
                )
                .is_err()
        );

        let mut host_mutation = fresh;
        let SemanticHostPortKindV1::HttpServer { request, .. } =
            &mut host_mutation.host_ports[0].kind
        else {
            panic!("expected HTTP host port");
        };
        request.source = SemanticSourceId(usize::MAX);
        assert!(
            host_mutation
                .validate(
                    &checked,
                    semantic.execution_graph(),
                    semantic.resource_graph(),
                    semantic.reactive_graph(),
                    semantic.resolved_out_graph(),
                )
                .is_err()
        );
    }

    #[test]
    fn source_payload_shapes_reject_stale_missing_and_duplicate_checked_identities() {
        let (checked, _) = checked_and_semantic(
            "server-outputs.bn",
            include_str!("../../../examples/server_outputs.bn"),
        );
        assert!(
            !checked
                .lowering_metadata
                .source_payload_shape_table
                .is_empty()
        );

        let mut stale = checked.lowering_metadata.source_payload_shape_table.clone();
        stale[0].checked_sources[0] = CheckedSourceId(u32::MAX);
        assert!(validate_source_payload_shape_identity_coverage(&checked, &stale).is_err());

        let mut missing = checked.lowering_metadata.source_payload_shape_table.clone();
        missing[0].checked_sources.remove(0);
        assert!(validate_source_payload_shape_identity_coverage(&checked, &missing).is_err());

        let mut duplicate = checked.lowering_metadata.source_payload_shape_table.clone();
        let repeated = duplicate[0].checked_sources[0];
        duplicate[0].checked_sources.insert(0, repeated);
        assert!(validate_source_payload_shape_identity_coverage(&checked, &duplicate).is_err());
    }
}
