//! Pre-backend reactive ownership and scheduling.
//!
//! This module deliberately consumes only the normalized semantic execution
//! graph, the semantic resource graph, and the resolved `OUT` graph.  It must
//! not reinterpret checked coordinates as executable identity.  Checked
//! expression IDs retained below are audit provenance for exact gates only.

use crate::{
    ProducerFunctionId, ProducerMaterializationMode, ResolvedOutGraph, SemanticActivationId,
    SemanticBindingId, SemanticCallId, SemanticCaptureId, SemanticContextualOperationKind,
    SemanticDependencyUseId, SemanticDerivedValueId, SemanticExecutionGraphV1, SemanticExprId,
    SemanticExpression, SemanticExpressionKind, SemanticExternalDependencyId, SemanticFieldId,
    SemanticHostEffectScheduleId, SemanticListId, SemanticListMutationId,
    SemanticListResourceOriginV1, SemanticLocalBindingId, SemanticMaterializationId,
    SemanticMaterializationLocalId, SemanticParameterId, SemanticPulseBatchId, SemanticReadId,
    SemanticResourceGraphV1, SemanticRootKindV1, SemanticRowBinding, SemanticRowScopeId,
    SemanticScopeId, SemanticSourceId, SemanticStateId, SemanticStateUpdateArmId,
    SemanticStatementId, SemanticStatementKind, SemanticTriggerArmId, SemanticValueId,
    SemanticValueOrigin, StaticOwnerId,
};
use boon_typecheck::{
    CheckedExprId, CheckedExternalDeclarationIdentityV1, CheckedExternalDeclarationKind,
    CheckedIntrinsicV1, DeclId, FlowMode, FlowType, Type,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SEMANTIC_REACTIVE_GRAPH_SCHEMA_V1: &str = "boon.semantic-reactive-graph.v1";

/// The complete part of the reactive graph that is already derivable from
/// normalized semantic identity.
///
/// Output contracts, render-node attributes, and migration transforms are not
/// guessed here.  [`SemanticOutputValueV1`] and [`SemanticViewCaptureV1`] own
/// the exact semantic roots and dependencies that those later contracts bind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticReactiveGraphV1 {
    pub schema: String,
    /// Sealed external declarations whose producer delivery was proven to be
    /// event-backed by the atomic distributed bundle. This input is retained
    /// so graph validation can deterministically rederive the same schedules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_event_identities: Vec<CheckedExternalDeclarationIdentityV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_instances: Vec<SemanticProducerInstanceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<SemanticFieldV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SemanticBindingV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<SemanticReadBindingV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependency_uses: Vec<SemanticDependencyUseV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub call_invocations: Vec<SemanticCallInvocationScheduleV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activations: Vec<SemanticActivationSiteV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pulse_batches: Vec<SemanticPulseBatchV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_values: Vec<SemanticDerivedValueV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_arms: Vec<SemanticTriggerOwnedArmV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_update_arms: Vec<SemanticStateUpdateArmV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_mutations: Vec<SemanticListMutationV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<SemanticDependencyEdgeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub possible_causes: Vec<SemanticPossibleCausesV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_effect_schedules: Vec<SemanticHostEffectScheduleV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_values: Vec<SemanticOutputValueV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub view_captures: Vec<SemanticViewCaptureV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migration_inputs: Vec<SemanticMigrationInputV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticProducerInstanceV1 {
    pub identity: [u8; 32],
    pub owner: StaticOwnerId,
    pub function: ProducerFunctionId,
    pub callable: crate::SemanticCallableId,
    pub root_call: crate::OutCallInstanceId,
    pub result_statement: SemanticStatementId,
    pub result_declaration: DeclId,
    pub result_path: String,
    pub root_expression: SemanticExprId,
    pub root_value: SemanticValueId,
    pub mode: ProducerMaterializationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_source: Option<SemanticSourceId>,
    pub parameters: Vec<SemanticProducerParameterV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticProducerParameterV1 {
    pub parameter: SemanticParameterId,
    pub formal: DeclId,
    pub name: String,
    pub flow_type: FlowType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_expressions: Vec<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_values: Vec<SemanticValueId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticFieldV1 {
    pub id: SemanticFieldId,
    pub statement: SemanticStatementId,
    pub declaration: DeclId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<SemanticRowBinding>,
    pub name: String,
    pub path: String,
    pub producer: SemanticExprId,
    pub value: SemanticValueId,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticBindingV1 {
    pub id: SemanticBindingId,
    pub declaration: DeclId,
    pub statement: SemanticStatementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_instance: Option<crate::OutCallInstanceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub producer: SemanticExprId,
    pub value: SemanticValueId,
    pub flow_type: FlowType,
    pub target: SemanticBindingTargetV1,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticBindingTargetV1 {
    Field { field: SemanticFieldId },
    Source { source: SemanticSourceId },
    State { state: SemanticStateId },
    List { list: SemanticListId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticReadBindingV1 {
    pub id: SemanticReadId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub target: SemanticReadTargetV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticReadTargetV1 {
    Binding {
        binding: SemanticBindingId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    SourcePayload {
        binding: SemanticBindingId,
        source: SemanticSourceId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        payload_projection: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    StateProjection {
        binding: SemanticBindingId,
        state: SemanticStateId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    Local {
        binding: SemanticLocalBindingId,
        declaration: DeclId,
        producer: SemanticExprId,
        producer_value: SemanticValueId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    External {
        canonical_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    },
    ElementState {
        context: crate::SemanticCallContextId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: SemanticMaterializationLocalId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    FunctionParameter {
        parameter: SemanticParameterId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencyUseV1 {
    pub id: SemanticDependencyUseId,
    pub dependent: SemanticBindingId,
    pub expression: SemanticExprId,
    pub target: SemanticDependencyTargetV1,
    pub timing: SemanticDependencyTimingV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallInvocationScheduleV1 {
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub call: SemanticCallId,
    pub current_capable: bool,
    pub dependent_bindings: Vec<SemanticBindingId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invocation_arms: Vec<SemanticTriggerArmId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyTargetV1 {
    ExternalRead {
        read: SemanticReadId,
    },
    ExternalCall {
        call: SemanticCallId,
        expression: SemanticExprId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyTimingV1 {
    Immediate,
    After {
        boundaries: Vec<SemanticEventCauseV1>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SemanticEventCauseV1 {
    Source(SemanticSourceId),
    State(SemanticStateId),
    Pulse(SemanticPulseBatchId),
    /// An event-valued read whose concrete ingress SOURCE is owned by the
    /// atomic distributed bundle boundary. Keeping the exact semantic
    /// expression here preserves its trigger schedule before executable
    /// source IDs exist.
    ExternalRead(SemanticExprId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticActivationSiteV1 {
    pub id: SemanticActivationId,
    pub then_expression: SemanticExprId,
    pub input_expression: SemanticExprId,
    pub input_value: SemanticValueId,
    pub output_expression: SemanticExprId,
    pub output_value: SemanticValueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub route_scope: SemanticScopeId,
    pub states: Vec<SemanticStateId>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticPulseBatchDigestV1(pub [u8; 32]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticPulseScheduleV1 {
    StageArbitrateCommitPublishBeforeNext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticPulseFlushPolicyV1 {
    DiscardCurrentStopRemainingKeepPriorCommits,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticPulseStartV1 {
    Startup,
    Triggered { arms: Vec<SemanticTriggerArmId> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticPulseBatchV1 {
    pub id: SemanticPulseBatchId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosing_activation: Option<SemanticActivationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<SemanticStateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_expression: Option<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_value: Option<SemanticValueId>,
    pub call: SemanticCallId,
    pub call_expression: SemanticExprId,
    pub call_value: SemanticValueId,
    pub count_expression: SemanticExprId,
    pub count_value: SemanticValueId,
    pub start: SemanticPulseStartV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_expression: Option<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_output: Option<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_arms: Vec<SemanticTriggerArmId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_update_arms: Vec<SemanticStateUpdateArmId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_mutations: Vec<SemanticListMutationId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_values: Vec<SemanticDerivedValueId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_effect_schedules: Vec<SemanticHostEffectScheduleId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flush_roots: Vec<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emission_routes: Vec<SemanticPulseEmissionRouteV1>,
    pub schedule: SemanticPulseScheduleV1,
    pub flush_policy: SemanticPulseFlushPolicyV1,
    pub slice_digest: SemanticPulseBatchDigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticPulseEmissionRouteV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<SemanticExprId>,
    pub filter: SemanticPulseEmissionFilterV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticPulseEmissionFilterV1 {
    Passthrough,
    Skip {
        call: SemanticCallId,
        expression: SemanticExprId,
        count_expression: SemanticExprId,
        count_value: SemanticValueId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticTriggerOwnedArmV1 {
    pub id: SemanticTriggerArmId,
    pub cause: SemanticEventCauseV1,
    pub gate_checked_expression: CheckedExprId,
    pub gate_expression: SemanticExprId,
    pub gate_value: SemanticValueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub route_scope: SemanticScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_scope: Option<SemanticRowScopeId>,
    pub output_expression: SemanticExprId,
    pub output_value: SemanticValueId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStateUpdateArmV1 {
    pub id: SemanticStateUpdateArmId,
    pub state: SemanticStateId,
    pub trigger: SemanticTriggerArmId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencyEdgeV1 {
    pub id: SemanticExternalDependencyId,
    pub from: SemanticEventCauseV1,
    pub to: SemanticStateId,
    pub indexed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticPossibleCausesV1 {
    pub state: SemanticStateId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<SemanticEventCauseV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticListMutationV1 {
    pub id: SemanticListMutationId,
    pub list: SemanticListId,
    pub site: SemanticExprId,
    pub site_value: SemanticValueId,
    pub cause: SemanticEventCauseV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub route_scope: SemanticScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_scope: Option<SemanticRowScopeId>,
    pub kind: SemanticListMutationKindV1,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticListMutationKindV1 {
    Append {
        gate: SemanticExprId,
        gate_value: SemanticValueId,
        item: SemanticExprId,
        item_value: SemanticValueId,
    },
    Remove {
        materialization: SemanticMaterializationId,
        gate: SemanticExprId,
        gate_value: SemanticValueId,
        owner: StaticOwnerId,
        row_local: SemanticMaterializationLocalId,
        predicate: SemanticExprId,
        predicate_value: SemanticValueId,
        remove_when: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDerivedValueV1 {
    pub id: SemanticDerivedValueId,
    pub binding: SemanticBindingId,
    pub field: SemanticFieldId,
    pub statement: SemanticStatementId,
    pub producer: SemanticExprId,
    pub value: SemanticValueId,
    pub kind: SemanticDerivedValueKindV1,
    /// Exact whole-value HOLD state backing a producer result.
    ///
    /// This is semantic state identity, not a diagnostic path or an
    /// executable-expression guess. A producer result may retain a
    /// context-free checked fallback expression even though its expanded
    /// value is the current value of a distinct state occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_backing: Option<SemanticStateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_list: Option<SemanticListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_row_scope: Option<SemanticRowScopeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<SemanticEventCauseV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_arms: Vec<SemanticTriggerArmId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_values: Vec<SemanticValueId>,
    pub startup_recompute: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDerivedValueKindV1 {
    SourceEventTransform,
    ListView,
    Aggregate,
    Pure,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticHostEffectScheduleV1 {
    pub id: SemanticHostEffectScheduleId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub call: SemanticCallId,
    pub checked_expression: CheckedExprId,
    pub owner: Option<StaticOwnerId>,
    pub operation: String,
    pub state_update_arms: Vec<SemanticStateUpdateArmId>,
    /// A host result may be retained by an explicit state update or by one
    /// compiler-owned transient derived-value lane, never both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transient_result: Option<SemanticDerivedValueId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticOutputValueV1 {
    pub ordinal: usize,
    pub checked_expression: CheckedExprId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub statement: SemanticStatementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<SemanticFieldId>,
    pub route_scope: SemanticScopeId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewCaptureV1 {
    pub id: SemanticCaptureId,
    pub output_ordinal: usize,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub target: SemanticViewCaptureTargetV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_scope: Option<SemanticRowScopeId>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticViewCaptureTargetV1 {
    Read { read: SemanticReadId },
    Source { source: SemanticSourceId },
    Field { field: SemanticFieldId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMigrationInputV1 {
    pub id: crate::SemanticMigrationId,
    pub marker: SemanticExprId,
    pub marker_value: SemanticValueId,
    pub input: SemanticExprId,
    pub input_value: SemanticValueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub route_scope: SemanticScopeId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticReactiveError {
    message: String,
}

impl SemanticReactiveError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SemanticReactiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SemanticReactiveError {}

impl From<String> for SemanticReactiveError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

/// Derive the semantic-owned reactive graph.
///
/// This function validates its three input graphs before deriving anything.
/// Every output ID is dense and every relationship is resolved by semantic or
/// resolved-OUT identity.  There is no name/path fallback.
pub fn build_semantic_reactive_graph(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    out_net: &ResolvedOutGraph,
) -> Result<SemanticReactiveGraphV1, SemanticReactiveError> {
    build_semantic_reactive_graph_with_external_events(execution, resources, out_net, &[])
}

pub fn build_semantic_reactive_graph_with_external_events(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    out_net: &ResolvedOutGraph,
    external_event_identities: &[CheckedExternalDeclarationIdentityV1],
) -> Result<SemanticReactiveGraphV1, SemanticReactiveError> {
    execution
        .validate(out_net)
        .map_err(SemanticReactiveError::new)?;
    resources
        .validate(execution, out_net)
        .map_err(SemanticReactiveError::new)?;
    let external_event_identities = external_event_identities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for identity in &external_event_identities {
        if identity.kind != CheckedExternalDeclarationKind::Value {
            return Err(SemanticReactiveError::new(
                "semantic external-event input carries a non-value declaration identity",
            ));
        }
        let matches = execution
            .expressions
            .iter()
            .filter(|expression| {
                matches!(
                    &expression.kind,
                    SemanticExpressionKind::ExternalRead {
                        external_identity: Some(candidate),
                        ..
                    } if candidate == identity
                ) && matches!(
                    expression.flow_type.mode,
                    FlowMode::TickPresent | FlowMode::PresentOrAbsent
                )
            })
            .count();
        if matches == 0 {
            return Err(SemanticReactiveError::new(format!(
                "semantic external-event identity for {} declaration {} has no event-flow read occurrence",
                identity.producer_role.namespace(),
                identity.producer_declaration.0
            )));
        }
    }
    let graph =
        ReactiveBuilder::new(execution, resources, out_net, external_event_identities)?.build()?;
    validate_semantic_reactive_shape(&graph, execution, resources)?;
    Ok(graph)
}

impl SemanticReactiveGraphV1 {
    /// Validate totality and canonicality by re-deriving the graph from its
    /// immutable semantic inputs.
    pub fn validate(
        &self,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        out_net: &ResolvedOutGraph,
    ) -> Result<(), SemanticReactiveError> {
        let expected = build_semantic_reactive_graph_with_external_events(
            execution,
            resources,
            out_net,
            &self.external_event_identities,
        )?;
        if self != &expected {
            return Err(SemanticReactiveError::new(
                "semantic reactive graph differs from its deterministic semantic derivation",
            ));
        }
        Ok(())
    }

    /// Return the exact trigger-owned invocation arms for one concrete
    /// semantic external-call expression.
    ///
    /// The schedule is derived from bindings whose external-call dependency
    /// target is `expression`.  Current-capable calls use arms before the
    /// terminal call; eventful, stateful, or host-effectful calls use the full
    /// dependent-root arms.  In both cases only arms whose semantic output
    /// reaches the call are retained.
    pub fn invocation_arms_for_call_expression(
        &self,
        expression: SemanticExprId,
    ) -> Result<Vec<SemanticTriggerOwnedArmV1>, SemanticReactiveError> {
        let matches = self
            .call_invocations
            .iter()
            .filter(|schedule| schedule.expression == expression)
            .collect::<Vec<_>>();
        let [schedule] = matches.as_slice() else {
            return Err(SemanticReactiveError::new(format!(
                "semantic external-call expression {expression} resolves to {} invocation schedules",
                matches.len()
            )));
        };
        schedule
            .invocation_arms
            .iter()
            .map(|arm| require_trigger(&self.trigger_arms, *arm).cloned())
            .collect()
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RawTriggerArm {
    cause: SemanticEventCauseV1,
    gate_checked_expression: CheckedExprId,
    gate_expression: SemanticExprId,
    gate_value: SemanticValueId,
    owner: Option<StaticOwnerId>,
    route_scope: SemanticScopeId,
    row_scope: Option<SemanticRowScopeId>,
    output_expression: SemanticExprId,
    output_value: SemanticValueId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RawStateUpdateArm {
    state: SemanticStateId,
    trigger: RawTriggerArm,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RawListMutation {
    list: SemanticListId,
    site: SemanticExprId,
    site_value: SemanticValueId,
    trigger: RawTriggerArm,
    kind: SemanticListMutationKindV1,
}

#[derive(Clone, Debug)]
struct RawCallInvocationSchedule {
    expression: SemanticExprId,
    value: SemanticValueId,
    call: SemanticCallId,
    current_capable: bool,
    dependent_bindings: Vec<SemanticBindingId>,
    invocation_arms: Vec<RawTriggerArm>,
}

#[derive(Clone, Debug)]
struct RawPulseBatch {
    id: SemanticPulseBatchId,
    enclosing_then: Option<SemanticExprId>,
    start_then: Option<SemanticExprId>,
    start_expression: Option<SemanticExprId>,
    state: Option<SemanticStateId>,
    hold_expression: Option<SemanticExprId>,
    hold_value: Option<SemanticValueId>,
    call: SemanticCallId,
    call_expression: SemanticExprId,
    call_value: SemanticValueId,
    count_expression: SemanticExprId,
    count_value: SemanticValueId,
    transition_expression: Option<SemanticExprId>,
    transition_output: Option<SemanticExprId>,
    flush_roots: Vec<SemanticExprId>,
    emission_routes: Vec<SemanticPulseEmissionRouteV1>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RawPulseTransition {
    batch_index: usize,
    transition: SemanticExprId,
    output: SemanticExprId,
    start_then: Option<SemanticExprId>,
    start_expression: Option<SemanticExprId>,
}

fn semantic_pulse_batch_digest_v1(
    batch: &SemanticPulseBatchV1,
) -> Result<SemanticPulseBatchDigestV1, SemanticReactiveError> {
    let mut payload = batch.clone();
    payload.slice_digest = SemanticPulseBatchDigestV1([0; 32]);
    boon_contract::canonical_serde_hash_v1(b"boon.semantic-pulse-batch.v1\0", &payload)
        .map(SemanticPulseBatchDigestV1)
        .map_err(|error| {
            SemanticReactiveError::new(format!(
                "semantic pulse batch {} canonical digest failed: {error}",
                batch.id
            ))
        })
}

fn reachable_reactive_expressions(
    execution: &SemanticExecutionGraphV1,
) -> Result<BTreeSet<SemanticExprId>, SemanticReactiveError> {
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
                    SemanticReactiveError::new(format!(
                        "reactive reachability references missing expression {expression_id}"
                    ))
                })?;
            pending.extend(semantic_expression_children(&expression.kind, execution)?);
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

struct ReactiveBuilder<'a> {
    execution: &'a SemanticExecutionGraphV1,
    resources: &'a SemanticResourceGraphV1,
    out_net: &'a ResolvedOutGraph,
    reachable_expressions: BTreeSet<SemanticExprId>,
    local_values: BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
    parameter_inputs: BTreeMap<SemanticExprId, Vec<SemanticExprId>>,
    external_event_identities: BTreeSet<CheckedExternalDeclarationIdentityV1>,
}

impl<'a> ReactiveBuilder<'a> {
    fn new(
        execution: &'a SemanticExecutionGraphV1,
        resources: &'a SemanticResourceGraphV1,
        out_net: &'a ResolvedOutGraph,
        external_event_identities: BTreeSet<CheckedExternalDeclarationIdentityV1>,
    ) -> Result<Self, SemanticReactiveError> {
        let reachable_expressions = reachable_reactive_expressions(execution)?;
        let mut local_values = BTreeMap::new();
        for expression in &execution.expressions {
            if let SemanticExpressionKind::Block { bindings, .. } = &expression.kind {
                for binding in bindings {
                    if local_values
                        .insert(binding.id, (binding.declaration, binding.value))
                        .is_some()
                    {
                        return Err(SemanticReactiveError::new(format!(
                            "semantic local binding {} has multiple producers",
                            binding.id
                        )));
                    }
                }
            }
        }
        let mut parameter_inputs = BTreeMap::new();
        for function in &execution.functions {
            let function_inputs = function
                .parameters
                .iter()
                .map(|parameter| (parameter.id, parameter.input_expressions.as_slice()))
                .collect::<BTreeMap<_, _>>();
            let mut pending = vec![function.root];
            let mut visited = BTreeSet::new();
            while let Some(expression_id) = pending.pop() {
                if !visited.insert(expression_id) {
                    continue;
                }
                let expression = execution.expression(expression_id)?;
                if let SemanticExpressionKind::FunctionParameter { parameter, .. } =
                    &expression.kind
                {
                    let inputs = function_inputs.get(parameter).ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "producer function {} expression {} references undeclared parameter {:?}",
                            function.producer, expression.id, parameter
                        ))
                    })?;
                    if let Some(previous) =
                        parameter_inputs.insert(expression.id, (*inputs).to_vec())
                        && previous != **inputs
                    {
                        return Err(SemanticReactiveError::new(format!(
                            "semantic parameter expression {} belongs to multiple producer-input inventories",
                            expression.id
                        )));
                    }
                }
                pending.extend(semantic_expression_children(&expression.kind, execution)?);
            }
        }
        Ok(Self {
            execution,
            resources,
            out_net,
            reachable_expressions,
            local_values,
            parameter_inputs,
            external_event_identities,
        })
    }

    fn build_raw_pulse_batches(&self) -> Result<Vec<RawPulseBatch>, SemanticReactiveError> {
        let mut batches = Vec::new();
        for expression in &self.execution.expressions {
            let SemanticExpressionKind::Call {
                call,
                intrinsic: Some(CheckedIntrinsicV1::StreamPulses),
                arguments,
                ..
            } = &expression.kind
            else {
                continue;
            };
            let counts = arguments
                .iter()
                .filter(|argument| argument.name == "count")
                .collect::<Vec<_>>();
            let [count] = counts.as_slice() else {
                return Err(SemanticReactiveError::new(format!(
                    "semantic Stream/pulses expression {} resolves to {} count arguments",
                    expression.id,
                    counts.len()
                )));
            };
            let count_expression = self.execution.expression(count.value)?;
            batches.push(RawPulseBatch {
                id: SemanticPulseBatchId(0),
                enclosing_then: None,
                start_then: None,
                start_expression: None,
                state: None,
                hold_expression: None,
                hold_value: None,
                call: *call,
                call_expression: expression.id,
                call_value: expression.value_id,
                count_expression: count.value,
                count_value: count_expression.value_id,
                transition_expression: None,
                transition_output: None,
                flush_roots: Vec::new(),
                emission_routes: Vec::new(),
            });
        }
        batches.sort_by_key(|batch| batch.call_expression);
        for (index, batch) in batches.iter_mut().enumerate() {
            batch.id = SemanticPulseBatchId(index);
        }
        let batch_by_expression = batches
            .iter()
            .enumerate()
            .map(|(index, batch)| (batch.call_expression, index))
            .collect::<BTreeMap<_, _>>();
        if batch_by_expression.len() != batches.len() {
            return Err(SemanticReactiveError::new(
                "semantic Stream/pulses expressions are not unique",
            ));
        }

        let mut parents = BTreeMap::<SemanticExprId, BTreeSet<SemanticExprId>>::new();
        for expression in &self.execution.expressions {
            for child in semantic_expression_children(&expression.kind, self.execution)? {
                parents.entry(child).or_default().insert(expression.id);
            }
        }

        for state in &self.resources.states {
            let hold = self.execution.expression(state.expression)?;
            let SemanticExpressionKind::Hold { updates, .. } = &hold.kind else {
                continue;
            };
            for update in updates {
                for transition in self.pulse_transitions_in_update(*update, &batch_by_expression)? {
                    let batch = &mut batches[transition.batch_index];
                    if batch.state.is_some() {
                        return Err(SemanticReactiveError::new(format!(
                            "semantic pulse batch {} is owned by multiple HOLD transitions",
                            batch.id
                        )));
                    }
                    batch.state = Some(state.id);
                    batch.hold_expression = Some(state.expression);
                    batch.hold_value = Some(hold.value_id);
                    batch.enclosing_then = match state.lifetime {
                        crate::SemanticStateLifetimeV1::Persistent => None,
                        crate::SemanticStateLifetimeV1::ActivationLocal { then_expression } => {
                            Some(then_expression)
                        }
                    };
                    batch.start_then = transition.start_then;
                    batch.start_expression = transition.start_expression;
                    batch.transition_expression = Some(transition.transition);
                    batch.transition_output = Some(transition.output);
                    batch.flush_roots = self.flush_roots(transition.output)?;
                    batch.emission_routes =
                        self.pulse_emission_routes(state.expression, &parents)?;
                }
            }
        }
        Ok(batches)
    }

    fn pulse_transitions_in_update(
        &self,
        root: SemanticExprId,
        batch_by_expression: &BTreeMap<SemanticExprId, usize>,
    ) -> Result<Vec<RawPulseTransition>, SemanticReactiveError> {
        let mut pending = vec![(root, None::<(SemanticExprId, SemanticExprId)>)];
        let mut visited = BTreeSet::new();
        let mut transitions = BTreeSet::new();
        while let Some((expression_id, start)) = pending.pop() {
            if !visited.insert((expression_id, start)) {
                continue;
            }
            let expression = self.execution.expression(expression_id)?;
            if let SemanticExpressionKind::Then { input, output } = &expression.kind {
                if let (Some(batch_index), Some(output)) =
                    (batch_by_expression.get(input).copied(), *output)
                {
                    transitions.insert(RawPulseTransition {
                        batch_index,
                        transition: expression_id,
                        output,
                        start_then: start.map(|value| value.0),
                        start_expression: start.map(|value| value.1),
                    });
                }
                pending.push((*input, start));
                if let Some(output) = output {
                    pending.push((*output, Some((expression_id, *input))));
                }
                continue;
            }
            for child in semantic_expression_children(&expression.kind, self.execution)? {
                pending.push((child, start));
            }
        }
        Ok(transitions.into_iter().collect())
    }

    fn pulse_emission_routes(
        &self,
        hold: SemanticExprId,
        parents: &BTreeMap<SemanticExprId, BTreeSet<SemanticExprId>>,
    ) -> Result<Vec<SemanticPulseEmissionRouteV1>, SemanticReactiveError> {
        let consumers = parents.get(&hold).cloned().unwrap_or_default();
        if consumers.is_empty() {
            return Ok(vec![SemanticPulseEmissionRouteV1 {
                consumer: None,
                filter: SemanticPulseEmissionFilterV1::Passthrough,
            }]);
        }
        consumers
            .into_iter()
            .map(|consumer| {
                let expression = self.execution.expression(consumer)?;
                let filter = match &expression.kind {
                    SemanticExpressionKind::Call {
                        call,
                        intrinsic: Some(CheckedIntrinsicV1::StreamSkip),
                        arguments,
                        ..
                    } => {
                        let stream_matches = arguments
                            .iter()
                            .filter(|argument| argument.name == "stream" && argument.value == hold)
                            .count();
                        let counts = arguments
                            .iter()
                            .filter(|argument| argument.name == "count")
                            .collect::<Vec<_>>();
                        let [count] = counts.as_slice() else {
                            return Err(SemanticReactiveError::new(format!(
                                "semantic Stream/skip expression {consumer} resolves to {} count arguments",
                                counts.len()
                            )));
                        };
                        if stream_matches != 1 {
                            return Err(SemanticReactiveError::new(format!(
                                "semantic Stream/skip expression {consumer} consumes HOLD {hold} through {stream_matches} exact stream arguments"
                            )));
                        }
                        SemanticPulseEmissionFilterV1::Skip {
                            call: *call,
                            expression: consumer,
                            count_expression: count.value,
                            count_value: self.execution.value(count.value)?,
                        }
                    }
                    _ => SemanticPulseEmissionFilterV1::Passthrough,
                };
                Ok(SemanticPulseEmissionRouteV1 {
                    consumer: Some(consumer),
                    filter,
                })
            })
            .collect()
    }

    fn flush_roots(
        &self,
        root: SemanticExprId,
    ) -> Result<Vec<SemanticExprId>, SemanticReactiveError> {
        let mut pending = vec![root];
        let mut visited = BTreeSet::new();
        let mut flushes = BTreeSet::new();
        while let Some(expression) = pending.pop() {
            if !visited.insert(expression) {
                continue;
            }
            let expression = self.execution.expression(expression)?;
            if matches!(&expression.kind, SemanticExpressionKind::Flush { .. }) {
                flushes.insert(expression.id);
            }
            pending.extend(semantic_expression_children(
                &expression.kind,
                self.execution,
            )?);
        }
        Ok(flushes.into_iter().collect())
    }

    fn build_activation_sites(
        &self,
    ) -> Result<
        (
            Vec<SemanticActivationSiteV1>,
            BTreeMap<SemanticExprId, SemanticActivationId>,
        ),
        SemanticReactiveError,
    > {
        let mut states_by_then = BTreeMap::<SemanticExprId, BTreeSet<SemanticStateId>>::new();
        for state in &self.resources.states {
            if let crate::SemanticStateLifetimeV1::ActivationLocal { then_expression } =
                state.lifetime
            {
                states_by_then
                    .entry(then_expression)
                    .or_default()
                    .insert(state.id);
            }
        }
        let activation_ids = states_by_then
            .keys()
            .copied()
            .enumerate()
            .map(|(index, expression)| (expression, SemanticActivationId(index)))
            .collect::<BTreeMap<_, _>>();
        let activations = states_by_then
            .into_iter()
            .map(|(then_expression, states)| {
                let expression = self.execution.expression(then_expression)?;
                let SemanticExpressionKind::Then {
                    input,
                    output: Some(output),
                } = &expression.kind
                else {
                    return Err(SemanticReactiveError::new(format!(
                        "activation site {then_expression} is not a THEN expression with an output"
                    )));
                };
                Ok(SemanticActivationSiteV1 {
                    id: activation_ids[&then_expression],
                    then_expression,
                    input_expression: *input,
                    input_value: self.execution.value(*input)?,
                    output_expression: *output,
                    output_value: self.execution.value(*output)?,
                    owner: expression.owner,
                    route_scope: self.execution.route_scope(then_expression)?,
                    states: states.into_iter().collect(),
                })
            })
            .collect::<Result<Vec<_>, SemanticReactiveError>>()?;
        Ok((activations, activation_ids))
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_pulse_batches(
        &self,
        batches: Vec<RawPulseBatch>,
        activation_ids: &BTreeMap<SemanticExprId, SemanticActivationId>,
        starts: &BTreeMap<SemanticPulseBatchId, SemanticPulseStartV1>,
        trigger_arms: &[SemanticTriggerOwnedArmV1],
        all_state_update_arms: &[SemanticStateUpdateArmV1],
        list_mutations: &[SemanticListMutationV1],
        derived_values: &[SemanticDerivedValueV1],
        host_effect_schedules: &[SemanticHostEffectScheduleV1],
    ) -> Result<Vec<SemanticPulseBatchV1>, SemanticReactiveError> {
        let trigger_causes = trigger_arms
            .iter()
            .map(|arm| (arm.id, arm.cause))
            .collect::<BTreeMap<_, _>>();
        let mut result = Vec::with_capacity(batches.len());
        for raw in batches {
            let cause = SemanticEventCauseV1::Pulse(raw.id);
            let trigger_arms = trigger_arms
                .iter()
                .filter(|arm| arm.cause == cause)
                .map(|arm| arm.id)
                .collect::<Vec<_>>();
            let state_update_arms = all_state_update_arms
                .iter()
                .filter(|arm| trigger_causes.get(&arm.trigger) == Some(&cause))
                .map(|arm| arm.id)
                .collect::<Vec<_>>();
            if let Some(state) = raw.state
                && !state_update_arms.iter().any(|arm| {
                    all_state_update_arms
                        .get(arm.as_usize())
                        .is_some_and(|candidate| candidate.state == state)
                })
            {
                return Err(SemanticReactiveError::new(format!(
                    "semantic pulse batch {} does not schedule its owning state {state}",
                    raw.id
                )));
            }
            let state_update_arm_set = state_update_arms.iter().copied().collect::<BTreeSet<_>>();
            let list_mutations = list_mutations
                .iter()
                .filter(|mutation| mutation.cause == cause)
                .map(|mutation| mutation.id)
                .collect::<Vec<_>>();
            let derived_values = derived_values
                .iter()
                .filter(|derived| derived.causes.contains(&cause))
                .map(|derived| derived.id)
                .collect::<Vec<_>>();
            let host_effect_schedules = host_effect_schedules
                .iter()
                .filter(|schedule| {
                    schedule
                        .state_update_arms
                        .iter()
                        .any(|arm| state_update_arm_set.contains(arm))
                })
                .map(|schedule| schedule.id)
                .collect::<Vec<_>>();
            let enclosing_activation =
                raw.enclosing_then
                    .map(|then_expression| {
                        activation_ids.get(&then_expression).copied().ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "semantic pulse batch {} lost activation site {then_expression}",
                            raw.id
                        ))
                    })
                    })
                    .transpose()?;
            let mut batch = SemanticPulseBatchV1 {
                id: raw.id,
                enclosing_activation,
                state: raw.state,
                hold_expression: raw.hold_expression,
                hold_value: raw.hold_value,
                call: raw.call,
                call_expression: raw.call_expression,
                call_value: raw.call_value,
                count_expression: raw.count_expression,
                count_value: raw.count_value,
                start: starts.get(&raw.id).cloned().ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic pulse batch {} lost its activation start",
                        raw.id
                    ))
                })?,
                transition_expression: raw.transition_expression,
                transition_output: raw.transition_output,
                trigger_arms,
                state_update_arms,
                list_mutations,
                derived_values,
                host_effect_schedules,
                flush_roots: raw.flush_roots,
                emission_routes: raw.emission_routes,
                schedule: SemanticPulseScheduleV1::StageArbitrateCommitPublishBeforeNext,
                flush_policy:
                    SemanticPulseFlushPolicyV1::DiscardCurrentStopRemainingKeepPriorCommits,
                slice_digest: SemanticPulseBatchDigestV1([0; 32]),
            };
            batch.slice_digest = semantic_pulse_batch_digest_v1(&batch)?;
            result.push(batch);
        }
        Ok(result)
    }

    fn build(self) -> Result<SemanticReactiveGraphV1, SemanticReactiveError> {
        let producer_instances = self.build_producer_instances()?;
        let fields = self.build_fields()?;
        let bindings = self.build_bindings(&fields)?;
        let reads = self.build_reads(&bindings)?;
        let mut raw_pulse_batches = self.build_raw_pulse_batches()?;
        let (activations, activation_ids) = self.build_activation_sites()?;
        let pulse_by_expression = raw_pulse_batches
            .iter()
            .map(|batch| (batch.call_expression, batch.id))
            .collect::<BTreeMap<_, _>>();
        if pulse_by_expression.len() != raw_pulse_batches.len() {
            return Err(SemanticReactiveError::new(
                "one semantic Stream/pulses expression belongs to multiple pulse batches",
            ));
        }
        let pulse_states = raw_pulse_batches
            .iter()
            .filter_map(|batch| batch.state.map(|state| (batch.id, state)))
            .collect::<BTreeMap<_, _>>();
        let pulse_activation_expressions = raw_pulse_batches
            .iter()
            .flat_map(|batch| [batch.enclosing_then, batch.start_then])
            .flatten()
            .collect::<BTreeSet<_>>();
        let mut triggers = TriggerResolver::new(
            self.execution,
            self.resources,
            self.out_net,
            &bindings,
            &self.local_values,
            &self.parameter_inputs,
            &pulse_by_expression,
            &pulse_states,
            &pulse_activation_expressions,
            &self.external_event_identities,
        )?;

        let mut raw_pulse_starts = BTreeMap::<SemanticPulseBatchId, Vec<RawTriggerArm>>::new();
        for batch in &raw_pulse_batches {
            let start_expression = match (batch.start_expression, batch.enclosing_then) {
                (Some(start_expression), _) => start_expression,
                (None, Some(then_expression)) => activations
                    .iter()
                    .find(|activation| activation.then_expression == then_expression)
                    .map(|activation| activation.input_expression)
                    .ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "semantic pulse batch {} lost activation input {}",
                            batch.id, then_expression
                        ))
                    })?,
                (None, None) => batch.count_expression,
            };
            raw_pulse_starts.insert(
                batch.id,
                triggers.trigger_arms_for_expression(start_expression)?,
            );
        }

        let raw_state_arms = self.build_state_update_arms(&mut triggers)?;
        let mut raw_mutations = self.build_list_mutations(&bindings, &reads, &mut triggers)?;
        self.bind_pulse_list_mutations(&raw_pulse_batches, &mut raw_mutations, &mut triggers)?;
        let mut raw_derived = self.build_raw_derived_values(&fields, &bindings, &mut triggers)?;
        self.bind_pulse_emission_derived_values(
            &mut raw_pulse_batches,
            &mut raw_derived,
            &mut triggers,
        )?;
        let raw_call_invocations =
            self.build_call_invocation_schedules(&bindings, &mut triggers)?;

        let trigger_keys = raw_state_arms
            .iter()
            .map(|arm| arm.trigger.clone())
            .chain(
                raw_mutations
                    .iter()
                    .map(|mutation| mutation.trigger.clone()),
            )
            .chain(
                raw_derived
                    .iter()
                    .flat_map(|derived| derived.trigger_arms.iter().cloned()),
            )
            .chain(
                raw_call_invocations
                    .iter()
                    .flat_map(|schedule| schedule.invocation_arms.iter().cloned()),
            )
            .chain(
                raw_pulse_starts
                    .values()
                    .flat_map(|arms| arms.iter().cloned()),
            )
            .collect::<BTreeSet<_>>();
        let trigger_ids = trigger_keys
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, arm)| (arm, SemanticTriggerArmId(index)))
            .collect::<BTreeMap<_, _>>();
        let trigger_arms = trigger_keys
            .into_iter()
            .enumerate()
            .map(|(index, arm)| SemanticTriggerOwnedArmV1 {
                id: SemanticTriggerArmId(index),
                cause: arm.cause,
                gate_checked_expression: arm.gate_checked_expression,
                gate_expression: arm.gate_expression,
                gate_value: arm.gate_value,
                owner: arm.owner,
                route_scope: arm.route_scope,
                row_scope: arm.row_scope,
                output_expression: arm.output_expression,
                output_value: arm.output_value,
            })
            .collect::<Vec<_>>();
        let pulse_starts = raw_pulse_starts
            .into_iter()
            .map(|(pulse, arms)| {
                let start = if arms.is_empty() {
                    SemanticPulseStartV1::Startup
                } else {
                    SemanticPulseStartV1::Triggered {
                        arms: arms
                            .iter()
                            .map(|arm| {
                                trigger_ids.get(arm).copied().ok_or_else(|| {
                                    SemanticReactiveError::new(format!(
                                        "semantic pulse batch {} lost a canonical start trigger",
                                        pulse
                                    ))
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    }
                };
                Ok((pulse, start))
            })
            .collect::<Result<BTreeMap<_, _>, SemanticReactiveError>>()?;

        let state_update_arms = raw_state_arms
            .into_iter()
            .enumerate()
            .map(|(index, arm)| {
                Ok(SemanticStateUpdateArmV1 {
                    id: SemanticStateUpdateArmId(index),
                    state: arm.state,
                    trigger: *trigger_ids.get(&arm.trigger).ok_or_else(|| {
                        SemanticReactiveError::new(
                            "state update arm lost its canonical trigger identity",
                        )
                    })?,
                })
            })
            .collect::<Result<Vec<_>, SemanticReactiveError>>()?;
        let list_mutations = raw_mutations
            .into_iter()
            .enumerate()
            .map(|(index, mutation)| SemanticListMutationV1 {
                id: SemanticListMutationId(index),
                list: mutation.list,
                site: mutation.site,
                site_value: mutation.site_value,
                cause: mutation.trigger.cause,
                owner: mutation.trigger.owner,
                route_scope: mutation.trigger.route_scope,
                row_scope: mutation.trigger.row_scope,
                kind: mutation.kind,
            })
            .collect::<Vec<_>>();
        let derived_values = raw_derived
            .into_iter()
            .enumerate()
            .map(|(index, derived)| {
                Ok(SemanticDerivedValueV1 {
                    id: SemanticDerivedValueId(index),
                    binding: derived.binding,
                    field: derived.field,
                    statement: derived.statement,
                    producer: derived.producer,
                    value: derived.value,
                    kind: derived.kind,
                    state_backing: derived.state_backing,
                    materialized_list: derived.materialized_list,
                    materialized_row_scope: derived.materialized_row_scope,
                    causes: derived.causes,
                    trigger_arms: derived
                        .trigger_arms
                        .iter()
                        .map(|arm| {
                            trigger_ids.get(arm).copied().ok_or_else(|| {
                                SemanticReactiveError::new(
                                    "derived value lost its canonical trigger identity",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    default_values: derived.default_values,
                    startup_recompute: derived.startup_recompute,
                })
            })
            .collect::<Result<Vec<_>, SemanticReactiveError>>()?;
        let call_invocations = raw_call_invocations
            .into_iter()
            .map(|schedule| {
                Ok(SemanticCallInvocationScheduleV1 {
                    expression: schedule.expression,
                    value: schedule.value,
                    call: schedule.call,
                    current_capable: schedule.current_capable,
                    dependent_bindings: schedule.dependent_bindings,
                    invocation_arms: schedule
                        .invocation_arms
                        .iter()
                        .map(|arm| {
                            trigger_ids.get(arm).copied().ok_or_else(|| {
                                SemanticReactiveError::new(
                                    "call invocation schedule lost its canonical trigger identity",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                })
            })
            .collect::<Result<Vec<_>, SemanticReactiveError>>()?;

        let dependencies =
            self.build_dependencies(&state_update_arms, &trigger_arms, &pulse_states)?;
        let possible_causes = self.build_possible_causes(&state_update_arms, &trigger_arms)?;
        let host_effect_schedules = self.build_host_effect_schedules(
            &fields,
            &bindings,
            &derived_values,
            &state_update_arms,
            &trigger_arms,
        )?;
        let dependency_uses = self.build_dependency_uses(&bindings, &reads, &mut triggers)?;
        let output_values = self.build_output_values(&fields)?;
        let view_captures = self.build_view_captures(
            &output_values,
            &fields,
            &bindings,
            &reads,
            &mut triggers,
            &pulse_states,
        )?;
        let migration_inputs = self.build_migration_inputs()?;
        let pulse_batches = self.finish_pulse_batches(
            raw_pulse_batches,
            &activation_ids,
            &pulse_starts,
            &trigger_arms,
            &state_update_arms,
            &list_mutations,
            &derived_values,
            &host_effect_schedules,
        )?;

        Ok(SemanticReactiveGraphV1 {
            schema: SEMANTIC_REACTIVE_GRAPH_SCHEMA_V1.to_owned(),
            external_event_identities: self.external_event_identities.into_iter().collect(),
            producer_instances,
            fields,
            bindings,
            reads,
            dependency_uses,
            call_invocations,
            activations,
            pulse_batches,
            derived_values,
            trigger_arms,
            state_update_arms,
            list_mutations,
            dependencies,
            possible_causes,
            host_effect_schedules,
            output_values,
            view_captures,
            migration_inputs,
        })
    }

    fn build_producer_instances(
        &self,
    ) -> Result<Vec<SemanticProducerInstanceV1>, SemanticReactiveError> {
        let roots_by_identity = self
            .out_net
            .producer_roots()
            .iter()
            .map(|root| (root.spec.identity, root))
            .collect::<BTreeMap<_, _>>();
        if roots_by_identity.len() != self.out_net.producer_roots().len() {
            return Err(SemanticReactiveError::new(
                "resolved OUT graph contains duplicate producer-root identities",
            ));
        }
        let functions = self
            .execution
            .functions
            .iter()
            .map(|function| (function.producer, function))
            .collect::<BTreeMap<_, _>>();
        let mut result = Vec::with_capacity(self.resources.producer_resources.len());
        for resource in &self.resources.producer_resources {
            let root = roots_by_identity.get(&resource.identity).ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "semantic producer resource for function {} has no exact resolved OUT root",
                    resource.function
                ))
            })?;
            let function = functions.get(&resource.function).ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "semantic producer resource references missing function {}",
                    resource.function
                ))
            })?;
            let statement = self
                .execution
                .statements
                .get(resource.result_statement.as_usize())
                .filter(|statement| statement.id == resource.result_statement)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic producer resource references missing statement {}",
                        resource.result_statement
                    ))
                })?;
            let root_expression = statement.value.ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "producer result statement {} has no semantic value",
                    statement.id
                ))
            })?;
            if function.identity != resource.identity
                || function.callable != resource.callable
                || function.root != root_expression
                || root.spec.function != resource.function
                || root.spec.identity != resource.identity
                || root.spec.callable
                    != self
                        .execution
                        .callables
                        .get(resource.callable.as_usize())
                        .filter(|callable| callable.id == resource.callable)
                        .ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "producer resource references missing callable {}",
                                resource.callable
                            ))
                        })?
                        .checked_callable
                || root.call != resource.root_call
                || root.spec.result_declaration != resource.result_declaration
                || root.spec.result_path != resource.result_path
                || root.spec.mode != resource.mode
            {
                return Err(SemanticReactiveError::new(format!(
                    "producer {} execution/resource/OUT provenance does not agree",
                    resource.function
                )));
            }
            match &statement.origin {
                crate::SemanticStatementOrigin::ProducerResult {
                    identity,
                    function: statement_function,
                    root_call,
                    result_statement,
                    ..
                } if *identity == resource.identity
                    && *statement_function == resource.function
                    && *root_call == resource.root_call
                    && *result_statement == root.spec.result_statement => {}
                _ => {
                    return Err(SemanticReactiveError::new(format!(
                        "producer result statement {} lacks exact producer provenance",
                        statement.id
                    )));
                }
            }
            let invocation_source = resource.invocation_source;
            if function.invocation_source.and_then(|expression| {
                self.execution
                    .sources
                    .iter()
                    .find_map(|source| (source.expression == expression).then_some(source.id))
            }) != invocation_source
            {
                return Err(SemanticReactiveError::new(format!(
                    "producer {} invocation source differs between execution and resource graphs",
                    resource.function
                )));
            }
            let parameters = function
                .parameters
                .iter()
                .map(|parameter| {
                    let input_values = parameter
                        .input_expressions
                        .iter()
                        .map(|expression| self.execution.value(*expression))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(SemanticProducerParameterV1 {
                        parameter: parameter.id,
                        formal: parameter.formal,
                        name: parameter.name.clone(),
                        flow_type: parameter.flow_type.clone(),
                        input_expressions: parameter.input_expressions.clone(),
                        input_values,
                    })
                })
                .collect::<Result<Vec<_>, SemanticReactiveError>>()?;
            result.push(SemanticProducerInstanceV1 {
                identity: resource.identity,
                owner: resource.owner,
                function: resource.function,
                callable: resource.callable,
                root_call: resource.root_call,
                result_statement: resource.result_statement,
                result_declaration: resource.result_declaration,
                result_path: resource.result_path.clone(),
                root_expression,
                root_value: self.execution.value(root_expression)?,
                mode: resource.mode,
                invocation_source,
                parameters,
            });
        }
        result.sort_by_key(|instance| (instance.function, instance.identity));
        Ok(result)
    }

    fn build_fields(&self) -> Result<Vec<SemanticFieldV1>, SemanticReactiveError> {
        let statement_parents = self
            .execution
            .statements
            .iter()
            .flat_map(|parent| parent.children.iter().map(move |child| (*child, parent.id)))
            .collect::<BTreeMap<_, _>>();
        let direct_storage_statements = self
            .execution
            .statements
            .iter()
            .filter(|statement| {
                let Some(parent) = statement_parents.get(&statement.id) else {
                    return true;
                };
                self.execution
                    .statements
                    .get(parent.as_usize())
                    .filter(|candidate| candidate.id == *parent)
                    .is_some_and(|parent| {
                        parent.declaration.is_some()
                            && matches!(parent.kind, SemanticStatementKind::Field { .. })
                    })
            })
            .map(|statement| statement.id)
            .collect::<BTreeSet<_>>();
        let mut candidates = Vec::new();
        for statement in &self.execution.statements {
            if !direct_storage_statements.contains(&statement.id) {
                continue;
            }
            let Some(declaration) = statement.declaration else {
                continue;
            };
            let Some(statement_producer) = statement.value else {
                continue;
            };
            if self.resources.sources.iter().any(|source| {
                source.statement == statement.id && source.expression == statement_producer
            }) && !matches!(
                &statement.origin,
                crate::SemanticStatementOrigin::ProducerResult { .. }
            ) {
                continue;
            }
            let producer = if let Some(parent_id) = statement_parents.get(&statement.id) {
                let parent = self
                    .execution
                    .statements
                    .get(parent_id.as_usize())
                    .filter(|candidate| candidate.id == *parent_id)
                    .ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "semantic field statement {} references missing parent {parent_id}",
                            statement.id
                        ))
                    })?;
                let mut parent_value = parent.value;
                'producer: loop {
                    let Some(value) = parent_value else {
                        break statement_producer;
                    };
                    let expression = self.execution.expression(value)?;
                    match &expression.kind {
                        SemanticExpressionKind::FlushBoundary { input } => {
                            parent_value = Some(*input);
                        }
                        SemanticExpressionKind::Object(fields)
                        | SemanticExpressionKind::TaggedObject { fields, .. } => {
                            let structural = fields
                                .iter()
                                .filter(|field| field.declaration == Some(declaration))
                                .map(|field| field.value)
                                .collect::<Vec<_>>();
                            match structural.as_slice() {
                                [value] => {
                                    if matches!(
                                        &self.execution.expression(statement_producer)?.kind,
                                        SemanticExpressionKind::Project { input, fields }
                                            if *input == *value && fields.is_empty()
                                    ) {
                                        break 'producer statement_producer;
                                    }
                                    let mut candidate = *value;
                                    loop {
                                        match &self.execution.expression(candidate)?.kind {
                                            SemanticExpressionKind::FlushBoundary { input } => {
                                                candidate = *input;
                                            }
                                            SemanticExpressionKind::Block { .. } => {
                                                break 'producer *value;
                                            }
                                            _ => break 'producer statement_producer,
                                        }
                                    }
                                }
                                [] => break statement_producer,
                                _ => {
                                    return Err(SemanticReactiveError::new(format!(
                                        "semantic parent field {} contains multiple structural values for declaration {}",
                                        parent.id, declaration.0
                                    )));
                                }
                            }
                        }
                        _ => break statement_producer,
                    }
                }
            } else {
                statement_producer
            };
            let exact = self.exact_field_metadata(statement.id);
            let Some((name, path, row)) = exact else {
                continue;
            };
            let expression = self.execution.expression(producer)?;
            candidates.push((
                statement.id,
                declaration,
                expression.owner,
                row,
                name,
                path,
                producer,
                expression.value_id,
                expression.flow_type.clone(),
            ));
        }
        candidates.sort_by_key(|candidate| candidate.0);
        Ok(candidates
            .into_iter()
            .enumerate()
            .map(
                |(
                    index,
                    (statement, declaration, owner, row, name, path, producer, value, flow_type),
                )| SemanticFieldV1 {
                    id: SemanticFieldId(index),
                    statement,
                    declaration,
                    owner,
                    row,
                    name,
                    path,
                    producer,
                    value,
                    flow_type,
                },
            )
            .collect())
    }

    fn exact_field_metadata(
        &self,
        statement: SemanticStatementId,
    ) -> Option<(String, String, Option<SemanticRowBinding>)> {
        if let Some(state) = self
            .resources
            .states
            .iter()
            .find(|state| state.statement == statement)
        {
            return Some((
                state.hold_name.clone(),
                state.path.clone(),
                state
                    .target_list
                    .zip(state.row_scope)
                    .map(|(list, scope)| SemanticRowBinding { list, scope }),
            ));
        }
        if let Some(list) = self
            .resources
            .lists
            .iter()
            .find(|list| list.statement == statement)
        {
            return Some((
                list.local_name.clone(),
                list.semantic_path.clone(),
                Some(SemanticRowBinding {
                    list: list.id,
                    scope: list.row_scope,
                }),
            ));
        }
        let statement = self.execution.statements.get(statement.as_usize())?;
        match &statement.kind {
            SemanticStatementKind::Field { name, path } => Some((name.clone(), path.clone(), None)),
            SemanticStatementKind::Hold {
                name: Some(name),
                path: Some(path),
                ..
            }
            | SemanticStatementKind::List {
                name: Some(name),
                path: Some(path),
                ..
            } => Some((name.clone(), path.clone(), None)),
            SemanticStatementKind::Source { .. }
            | SemanticStatementKind::Hold { .. }
            | SemanticStatementKind::List { .. }
            | SemanticStatementKind::Block
            | SemanticStatementKind::Spread
            | SemanticStatementKind::Expression => None,
        }
    }

    fn build_bindings(
        &self,
        fields: &[SemanticFieldV1],
    ) -> Result<Vec<SemanticBindingV1>, SemanticReactiveError> {
        let fields_by_statement = fields
            .iter()
            .map(|field| (field.statement, field))
            .collect::<BTreeMap<_, _>>();
        let source_by_statement = self
            .resources
            .sources
            .iter()
            .map(|source| (source.statement, source))
            .collect::<BTreeMap<_, _>>();
        let mut states_by_statement = BTreeMap::<_, Vec<_>>::new();
        for state in &self.resources.states {
            states_by_statement
                .entry(state.statement)
                .or_default()
                .push(state);
        }
        let list_by_statement = self
            .resources
            .lists
            .iter()
            .map(|list| (list.statement, list))
            .collect::<BTreeMap<_, _>>();
        let mut candidates = Vec::new();
        // Source authority is keyed by its exact source expression, not by the
        // enclosing statement value. Invocation-mode producer results
        // deliberately share one semantic statement with their invocation
        // SOURCE while retaining a distinct result field/value.
        for source in &self.resources.sources {
            let statement = self
                .execution
                .statements
                .get(source.statement.as_usize())
                .filter(|candidate| candidate.id == source.statement)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic source {} references missing statement {}",
                        source.id, source.statement
                    ))
                })?;
            let expression = self.execution.expression(source.expression)?;
            candidates.push((
                source.statement,
                source.declaration,
                statement.call_instance,
                source.owner,
                source.expression,
                expression.value_id,
                expression.flow_type.clone(),
                SemanticBindingTargetV1::Source { source: source.id },
            ));
        }
        for statement in &self.execution.statements {
            let Some(declaration) = statement.declaration else {
                continue;
            };
            let Some(statement_producer) = statement.value else {
                continue;
            };
            if let Some(states) = states_by_statement.get(&statement.id) {
                // One lexical statement can contain more than one concrete
                // state authority (for example nested initial LATEST/stateful
                // expressions). Preserve every exact state binding instead of
                // collapsing the statement to its final state.
                for state in states {
                    let expression = self.execution.expression(state.expression)?;
                    candidates.push((
                        statement.id,
                        declaration,
                        statement.call_instance,
                        expression.owner,
                        state.expression,
                        expression.value_id,
                        expression.flow_type.clone(),
                        SemanticBindingTargetV1::State { state: state.id },
                    ));
                }
                continue;
            }
            let (producer, target) = if let Some(list) = list_by_statement.get(&statement.id) {
                (
                    statement_producer,
                    SemanticBindingTargetV1::List { list: list.id },
                )
            } else if let Some(field) = fields_by_statement.get(&statement.id) {
                (
                    field.producer,
                    SemanticBindingTargetV1::Field { field: field.id },
                )
            } else if source_by_statement.contains_key(&statement.id) {
                // The exact source-expression candidate was emitted above.
                continue;
            } else {
                continue;
            };
            let expression = self.execution.expression(producer)?;
            candidates.push((
                statement.id,
                declaration,
                statement.call_instance,
                expression.owner,
                producer,
                expression.value_id,
                expression.flow_type.clone(),
                target,
            ));
        }
        candidates.sort_by_key(|candidate| {
            let target_ordinal = match candidate.7 {
                SemanticBindingTargetV1::Source { .. } => 0_u8,
                SemanticBindingTargetV1::State { .. } => 1,
                SemanticBindingTargetV1::List { .. } => 2,
                SemanticBindingTargetV1::Field { .. } => 3,
            };
            (candidate.0, target_ordinal, candidate.4)
        });
        Ok(candidates
            .into_iter()
            .enumerate()
            .map(
                |(
                    index,
                    (
                        statement,
                        declaration,
                        call_instance,
                        owner,
                        producer,
                        value,
                        flow_type,
                        target,
                    ),
                )| SemanticBindingV1 {
                    id: SemanticBindingId(index),
                    declaration,
                    statement,
                    call_instance,
                    owner,
                    producer,
                    value,
                    flow_type,
                    target,
                },
            )
            .collect())
    }

    fn build_reads(
        &self,
        bindings: &[SemanticBindingV1],
    ) -> Result<Vec<SemanticReadBindingV1>, SemanticReactiveError> {
        let mut reads = Vec::new();
        for expression in &self.execution.expressions {
            let target = match &expression.kind {
                SemanticExpressionKind::CanonicalRead {
                    target,
                    projection,
                    source,
                    ..
                } => {
                    if let Some(source_read) = source {
                        let binding = unique_binding_for_target(
                            bindings,
                            SemanticBindingTargetV1::Source {
                                source: source_read.source,
                            },
                            expression,
                            self.execution,
                        )?;
                        SemanticReadTargetV1::SourcePayload {
                            binding: binding.id,
                            source: source_read.source,
                            payload_projection: source_read.payload_projection.clone(),
                            projection: projection.clone(),
                        }
                    } else {
                        let binding = match self.resolve_decl_binding(*target, expression, bindings)
                        {
                            Ok(binding) => binding,
                            Err(_)
                                if !self.reachable_expressions.contains(&expression.id)
                                    && self.execution.expressions.iter().any(|candidate| {
                                        candidate.checked_expr_id == expression.checked_expr_id
                                            && self.reachable_expressions.contains(&candidate.id)
                                            && matches!(
                                                candidate.kind,
                                                SemanticExpressionKind::LocalRead { .. }
                                            )
                                    }) =>
                            {
                                // Context-free checked statement copies are retained for
                                // diagnostics, but only the structurally reachable BLOCK
                                // occurrence owns a lexical value frame.
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        match binding.target {
                            SemanticBindingTargetV1::State { state } => {
                                SemanticReadTargetV1::StateProjection {
                                    binding: binding.id,
                                    state,
                                    projection: projection.clone(),
                                }
                            }
                            SemanticBindingTargetV1::Field { .. }
                            | SemanticBindingTargetV1::Source { .. }
                            | SemanticBindingTargetV1::List { .. } => {
                                SemanticReadTargetV1::Binding {
                                    binding: binding.id,
                                    projection: projection.clone(),
                                }
                            }
                        }
                    }
                }
                SemanticExpressionKind::Drain {
                    target, projection, ..
                } => {
                    let binding = self.resolve_decl_binding(*target, expression, bindings)?;
                    match binding.target {
                        SemanticBindingTargetV1::State { state } => {
                            SemanticReadTargetV1::StateProjection {
                                binding: binding.id,
                                state,
                                projection: projection.clone(),
                            }
                        }
                        SemanticBindingTargetV1::Field { .. }
                        | SemanticBindingTargetV1::Source { .. }
                        | SemanticBindingTargetV1::List { .. } => SemanticReadTargetV1::Binding {
                            binding: binding.id,
                            projection: projection.clone(),
                        },
                    }
                }
                SemanticExpressionKind::LocalRead {
                    binding,
                    declaration,
                    projection,
                } => {
                    let (producer_declaration, producer) =
                        self.local_values.get(binding).copied().ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "semantic local read {} references missing binding {}",
                                expression.id, binding
                            ))
                        })?;
                    if producer_declaration != *declaration {
                        return Err(SemanticReactiveError::new(format!(
                            "semantic local read {} declaration differs from binding {}",
                            expression.id, binding
                        )));
                    }
                    SemanticReadTargetV1::Local {
                        binding: *binding,
                        declaration: *declaration,
                        producer,
                        producer_value: self.execution.value(producer)?,
                        projection: projection.clone(),
                    }
                }
                SemanticExpressionKind::ExternalRead {
                    canonical_path,
                    external_identity,
                } => SemanticReadTargetV1::External {
                    canonical_path: canonical_path.clone(),
                    external_identity: *external_identity,
                },
                SemanticExpressionKind::ElementState {
                    context,
                    projection,
                } => SemanticReadTargetV1::ElementState {
                    context: *context,
                    projection: projection.clone(),
                },
                SemanticExpressionKind::MaterializationLocal {
                    owner,
                    local,
                    projection,
                    ..
                } => SemanticReadTargetV1::MaterializationLocal {
                    owner: *owner,
                    local: *local,
                    projection: projection.clone(),
                },
                SemanticExpressionKind::FunctionParameter {
                    parameter,
                    projection,
                } => SemanticReadTargetV1::FunctionParameter {
                    parameter: *parameter,
                    projection: projection.clone(),
                },
                _ => continue,
            };
            reads.push(SemanticReadBindingV1 {
                id: SemanticReadId(reads.len()),
                expression: expression.id,
                value: expression.value_id,
                target,
            });
        }
        Ok(reads)
    }

    fn resolve_decl_binding<'b>(
        &self,
        declaration: DeclId,
        expression: &SemanticExpression,
        bindings: &'b [SemanticBindingV1],
    ) -> Result<&'b SemanticBindingV1, SemanticReactiveError> {
        let origin = self.execution.origin(expression.id)?;
        lexical_binding_for_decl(
            self.execution,
            self.resources,
            self.out_net,
            bindings,
            declaration,
            expression,
            origin.call_instance,
            "semantic canonical read",
        )
    }

    fn build_state_update_arms(
        &self,
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<Vec<RawStateUpdateArm>, SemanticReactiveError> {
        let mut result = BTreeSet::new();
        for state in &self.resources.states {
            let state_triggers = triggers.trigger_arms_for_expression(state.expression)?;
            for trigger in state_triggers {
                let same_state_family = match trigger.cause {
                    SemanticEventCauseV1::State(cause) => self
                        .resources
                        .states
                        .get(cause.as_usize())
                        .filter(|candidate| candidate.id == cause)
                        .is_some_and(|candidate| {
                            candidate.declaration == state.declaration
                                && candidate.owner == state.owner
                        }),
                    SemanticEventCauseV1::Source(_)
                    | SemanticEventCauseV1::Pulse(_)
                    | SemanticEventCauseV1::ExternalRead(_) => false,
                };
                if !same_state_family {
                    result.insert(RawStateUpdateArm {
                        state: state.id,
                        trigger,
                    });
                }
            }
        }
        Ok(result.into_iter().collect())
    }

    fn build_list_mutations(
        &self,
        bindings: &[SemanticBindingV1],
        reads: &[SemanticReadBindingV1],
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<Vec<RawListMutation>, SemanticReactiveError> {
        let mut mutations = BTreeSet::new();
        let mut visited_by_list = BTreeMap::<SemanticListId, BTreeSet<SemanticExprId>>::new();
        for list in &self.resources.lists {
            self.collect_list_mutations(
                list.id,
                list.producer,
                visited_by_list.entry(list.id).or_default(),
                triggers,
                &mut mutations,
            )?;
        }
        // Queue/worklist code mutates an authority through a read instead of
        // returning a List/append pipeline as the declaration's producer.
        // Discover those sites from their exact typed list input as well.
        let mut classified_sites = mutations
            .iter()
            .map(|mutation| mutation.site)
            .collect::<BTreeSet<_>>();
        for expression in &self.execution.expressions {
            let SemanticExpressionKind::Call {
                callable,
                callable_kind: crate::SemanticCallableKind::Builtin,
                function,
                arguments,
                ..
            } = &expression.kind
            else {
                continue;
            };
            if function != "List/append" {
                continue;
            }
            if classified_sites.contains(&expression.id) {
                continue;
            }
            let list_input = exact_call_argument_at_ordinal(
                self.execution,
                *callable,
                arguments,
                0,
                expression.id,
            )?;
            let Some(list) = self.list_authority_for_expression(
                list_input,
                bindings,
                reads,
                &mut BTreeSet::new(),
            )?
            else {
                continue;
            };
            self.collect_list_mutations(
                list,
                expression.id,
                visited_by_list.entry(list).or_default(),
                triggers,
                &mut mutations,
            )?;
            classified_sites.insert(expression.id);
        }
        Ok(mutations.into_iter().collect())
    }

    fn bind_pulse_list_mutations(
        &self,
        pulse_batches: &[RawPulseBatch],
        mutations: &mut [RawListMutation],
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<(), SemanticReactiveError> {
        for mutation in mutations {
            // A mutation structurally owned by a pulse transition executes in
            // that microturn. Its item may read recurrence state, but that
            // data dependency must not reschedule the mutation as a later
            // state event.
            let mut owners = Vec::new();
            for batch in pulse_batches {
                if let Some(output) = batch.transition_output
                    && self.expression_reaches(output, mutation.site)?
                {
                    owners.push(batch.id);
                }
            }
            match owners.as_slice() {
                [] => {}
                [pulse] => {
                    let output = match &mutation.kind {
                        SemanticListMutationKindV1::Append { .. } => {
                            let expression = self.execution.expression(mutation.site)?;
                            let SemanticExpressionKind::Call {
                                callable,
                                arguments,
                                ..
                            } = &expression.kind
                            else {
                                return Err(SemanticReactiveError::new(format!(
                                    "semantic list append site {} is not a call",
                                    mutation.site
                                )));
                            };
                            exact_call_argument_at_ordinal(
                                self.execution,
                                *callable,
                                arguments,
                                1,
                                mutation.site,
                            )?
                        }
                        SemanticListMutationKindV1::Remove { predicate, .. } => *predicate,
                    };
                    let trigger = triggers.arm(
                        SemanticEventCauseV1::Pulse(*pulse),
                        mutation.trigger.gate_expression,
                        output,
                    )?;
                    match &mut mutation.kind {
                        SemanticListMutationKindV1::Append {
                            gate,
                            gate_value,
                            item,
                            item_value,
                        } => {
                            *gate = trigger.gate_expression;
                            *gate_value = trigger.gate_value;
                            *item = output;
                            *item_value = trigger.output_value;
                        }
                        SemanticListMutationKindV1::Remove {
                            gate,
                            gate_value,
                            predicate,
                            predicate_value,
                            ..
                        } => {
                            *gate = trigger.gate_expression;
                            *gate_value = trigger.gate_value;
                            *predicate = output;
                            *predicate_value = trigger.output_value;
                        }
                    }
                    mutation.trigger = trigger;
                }
                _ => {
                    return Err(SemanticReactiveError::new(format!(
                        "semantic list mutation at {} belongs to {} pulse transition outputs",
                        mutation.site,
                        owners.len()
                    )));
                }
            }
        }
        Ok(())
    }

    fn list_authority_for_expression(
        &self,
        root: SemanticExprId,
        bindings: &[SemanticBindingV1],
        reads: &[SemanticReadBindingV1],
        visited: &mut BTreeSet<SemanticExprId>,
    ) -> Result<Option<SemanticListId>, SemanticReactiveError> {
        if !visited.insert(root) {
            return Ok(None);
        }
        if let Some(read) = reads.iter().find(|read| read.expression == root)
            && let SemanticReadTargetV1::Binding { binding, .. } = read.target
        {
            let binding = bindings
                .get(binding.as_usize())
                .filter(|candidate| candidate.id == binding)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic list authority read {root} references missing binding {binding}"
                    ))
                })?;
            if let SemanticBindingTargetV1::List { list } = binding.target {
                return Ok(Some(list));
            }
        }
        let expression = self.execution.expression(root)?;
        let input = match &expression.kind {
            SemanticExpressionKind::Materialize { materialization } => self
                .execution
                .materializations
                .get(materialization.as_usize())
                .filter(|candidate| candidate.id == *materialization)
                .map(|materialization| materialization.source),
            SemanticExpressionKind::Draining { input }
            | SemanticExpressionKind::Project { input, .. } => Some(*input),
            SemanticExpressionKind::Then {
                output: Some(output),
                ..
            }
            | SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => Some(*output),
            SemanticExpressionKind::Block { result, .. } => Some(*result),
            SemanticExpressionKind::Call {
                callable,
                arguments,
                ..
            } if matches!(expression.flow_type.ty, Type::List(_)) => {
                exact_unique_list_argument(self.execution, *callable, arguments, root)?
            }
            _ => None,
        };
        match input {
            Some(input) => self.list_authority_for_expression(input, bindings, reads, visited),
            None => Ok(None),
        }
    }

    fn collect_list_mutations(
        &self,
        list: SemanticListId,
        root: SemanticExprId,
        visited: &mut BTreeSet<SemanticExprId>,
        triggers: &mut TriggerResolver<'_>,
        mutations: &mut BTreeSet<RawListMutation>,
    ) -> Result<(), SemanticReactiveError> {
        if !visited.insert(root) {
            return Ok(());
        }
        let expression = self.execution.expression(root)?;
        match &expression.kind {
            SemanticExpressionKind::Materialize { materialization } => {
                let materialization = self
                    .execution
                    .materializations
                    .get(materialization.as_usize())
                    .filter(|candidate| candidate.id == *materialization)
                    .ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "list mutation pipeline references missing materialization {}",
                            materialization
                        ))
                    })?;
                self.collect_list_mutations(
                    list,
                    materialization.source,
                    visited,
                    triggers,
                    mutations,
                )?;
                let remove_when = match materialization.operation {
                    SemanticContextualOperationKind::Remove => Some(true),
                    SemanticContextualOperationKind::Retain => Some(false),
                    SemanticContextualOperationKind::Map
                    | SemanticContextualOperationKind::Filter
                    | SemanticContextualOperationKind::Every
                    | SemanticContextualOperationKind::Any
                    | SemanticContextualOperationKind::Find
                    | SemanticContextualOperationKind::SortBy
                    | SemanticContextualOperationKind::ThenBy => None,
                };
                if let Some(remove_when) = remove_when {
                    for trigger in triggers.trigger_arms_for_expression(materialization.body)? {
                        mutations.insert(RawListMutation {
                            list,
                            site: root,
                            site_value: expression.value_id,
                            kind: SemanticListMutationKindV1::Remove {
                                materialization: materialization.id,
                                gate: trigger.gate_expression,
                                gate_value: trigger.gate_value,
                                owner: materialization.owner,
                                row_local: materialization.row_local,
                                predicate: trigger.output_expression,
                                predicate_value: trigger.output_value,
                                remove_when,
                            },
                            trigger,
                        });
                    }
                }
            }
            SemanticExpressionKind::Call {
                callable,
                callable_kind: crate::SemanticCallableKind::Builtin,
                function,
                arguments,
                ..
            } if function == "List/append" => {
                let list_input =
                    exact_call_argument_at_ordinal(self.execution, *callable, arguments, 0, root)?;
                let item =
                    exact_call_argument_at_ordinal(self.execution, *callable, arguments, 1, root)?;
                self.collect_list_mutations(list, list_input, visited, triggers, mutations)?;
                for trigger in triggers.trigger_arms_for_expression(item)? {
                    mutations.insert(RawListMutation {
                        list,
                        site: root,
                        site_value: expression.value_id,
                        kind: SemanticListMutationKindV1::Append {
                            gate: trigger.gate_expression,
                            gate_value: trigger.gate_value,
                            item: trigger.output_expression,
                            item_value: trigger.output_value,
                        },
                        trigger,
                    });
                }
            }
            SemanticExpressionKind::Call {
                callable,
                arguments,
                ..
            } => {
                if let Some(input) =
                    exact_unique_list_argument(self.execution, *callable, arguments, root)?
                {
                    self.collect_list_mutations(list, input, visited, triggers, mutations)?;
                }
            }
            SemanticExpressionKind::Draining { input }
            | SemanticExpressionKind::Project { input, .. } => {
                self.collect_list_mutations(list, *input, visited, triggers, mutations)?
            }
            SemanticExpressionKind::Block { bindings, result } => {
                for value in bindings
                    .iter()
                    .map(|binding| binding.value)
                    .chain(std::iter::once(*result))
                {
                    self.collect_list_mutations(list, value, visited, triggers, mutations)?;
                }
            }
            SemanticExpressionKind::When { arms, .. } => {
                for arm in arms {
                    self.collect_list_mutations(list, arm.output, visited, triggers, mutations)?;
                }
            }
            SemanticExpressionKind::Latest { branches } => {
                for branch in branches {
                    self.collect_list_mutations(list, *branch, visited, triggers, mutations)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn build_raw_derived_values(
        &self,
        semantic_fields: &[SemanticFieldV1],
        bindings: &[SemanticBindingV1],
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<Vec<RawDerivedValue>, SemanticReactiveError> {
        let fields_by_statement = semantic_fields
            .iter()
            .map(|field| (field.statement, field.id))
            .collect::<BTreeMap<_, _>>();
        let mut fields = Vec::new();
        for binding in bindings {
            let (field, materialized) = match binding.target {
                SemanticBindingTargetV1::Field { field } => (field, None),
                SemanticBindingTargetV1::List { list } => {
                    let resource = self
                        .resources
                        .lists
                        .iter()
                        .find(|resource| resource.id == list)
                        .ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "list binding {} references missing semantic list {list}",
                                binding.id
                            ))
                        })?;
                    let producer = self.execution.expression(binding.producer)?;
                    if matches!(
                        resource.origin,
                        SemanticListResourceOriginV1::CheckedLiteral { .. }
                    ) && matches!(producer.kind, SemanticExpressionKind::List { .. })
                    {
                        continue;
                    }
                    let field = fields_by_statement
                        .get(&binding.statement)
                        .copied()
                        .ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "computed list binding {} statement {} has no semantic field identity",
                                binding.id, binding.statement
                            ))
                        })?;
                    (field, Some((resource.id, resource.row_scope)))
                }
                SemanticBindingTargetV1::Source { .. } | SemanticBindingTargetV1::State { .. } => {
                    continue;
                }
            };
            if materialized.is_none()
                && self.expression_is_directly_source_only(binding.producer)?
            {
                // A SOURCE-only value is routing metadata even when a pure
                // wrapper preserves several source leaves. It retains its
                // structural field/binding identity for semantic tools, but
                // it must not become an executable scalar derivation.
                continue;
            }
            fields.push((binding, field, materialized));
        }
        let hold_body_statements = self.hold_body_statement_ids();
        let retained_output_statements = self
            .execution
            .roots
            .iter()
            .filter(|root| {
                matches!(
                    root.kind,
                    SemanticRootKindV1::RetainedVisualDocument
                        | SemanticRootKindV1::RetainedVisualScene
                )
            })
            .map(|root| root.statement)
            .collect::<BTreeSet<_>>();
        let mut result = Vec::new();
        for (binding, field, materialized) in fields {
            if hold_body_statements.contains(&binding.statement)
                || retained_output_statements.contains(&binding.statement)
            {
                continue;
            }
            let structural_group = self.statement_is_structural_group(binding.statement)?;
            let structural_host_output = structural_group
                && self.execution.roots.iter().any(|root| {
                    root.statement == binding.statement
                        && root.kind == SemanticRootKindV1::HostValue
                });
            if structural_group && !structural_host_output {
                continue;
            }
            let state_backing =
                self.producer_result_state_backing(binding.statement, binding.producer)?;
            let state_backed_current = state_backing.is_some();
            let triggers_for_value = if structural_group
                || materialized.is_some()
                || state_backed_current
                || !self.expression_owns_event(binding.producer)?
            {
                Vec::new()
            } else {
                triggers.trigger_arms_for_expression(binding.producer)?
            };
            let causes = triggers_for_value
                .iter()
                .map(|arm| arm.cause)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let kind = if materialized.is_some() {
                SemanticDerivedValueKindV1::ListView
            } else if structural_group || state_backed_current {
                SemanticDerivedValueKindV1::Pure
            } else if !causes.is_empty() {
                SemanticDerivedValueKindV1::SourceEventTransform
            } else if self.expression_is_aggregate(binding.producer)? {
                SemanticDerivedValueKindV1::Aggregate
            } else {
                SemanticDerivedValueKindV1::Pure
            };
            let default_values = match &self.execution.expression(binding.producer)?.kind {
                SemanticExpressionKind::Latest { branches } => branches
                    .iter()
                    .filter(|branch| {
                        triggers
                            .event_causes_for_expression(**branch)
                            .is_ok_and(|causes| causes.is_empty())
                    })
                    .map(|branch| self.execution.value(*branch))
                    .collect::<Result<Vec<_>, _>>()?,
                SemanticExpressionKind::Hold { initial, .. } => {
                    vec![self.execution.value(*initial)?]
                }
                _ => Vec::new(),
            };
            let startup_recompute =
                matches!(kind, SemanticDerivedValueKindV1::SourceEventTransform);
            result.push(RawDerivedValue {
                binding: binding.id,
                field,
                statement: binding.statement,
                producer: binding.producer,
                value: binding.value,
                kind,
                state_backing,
                materialized_list: materialized.map(|value| value.0),
                materialized_row_scope: materialized.map(|value| value.1),
                causes,
                trigger_arms: triggers_for_value,
                default_values,
                startup_recompute,
            });
        }
        result.sort_by_key(|derived| derived.binding);
        Ok(result)
    }

    fn producer_result_state_backing(
        &self,
        statement: SemanticStatementId,
        expression: SemanticExprId,
    ) -> Result<Option<SemanticStateId>, SemanticReactiveError> {
        // A synthetic invocation wrapper can make a producer result appear
        // event-owned even when its actual value is only current HOLD state.
        // Preserve that result as a pure state read; the state update schedule
        // owns any nested SOURCE or host effect.
        let statement = self
            .execution
            .statements
            .get(statement.as_usize())
            .filter(|candidate| candidate.id == statement)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "derived producer result references missing semantic statement {statement}"
                ))
            })?;
        if !matches!(
            statement.origin,
            crate::SemanticStatementOrigin::ProducerResult { .. }
        ) {
            return Ok(None);
        }
        let expression = self.execution.expression(expression)?;
        let [member] = expression.provenance.members.as_slice() else {
            return Ok(None);
        };
        if !member.path.is_empty() {
            return Ok(None);
        }
        let SemanticValueOrigin::State { state, owner } = &member.origin else {
            return Ok(None);
        };
        let state = *state;
        let owner = *owner;
        let resource = self
            .resources
            .states
            .get(state.as_usize())
            .filter(|candidate| candidate.id == state)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "producer result expression {} references missing state {state}",
                    expression.id
                ))
            })?;
        if resource.owner != owner || resource.flow_type.ty != expression.flow_type.ty {
            return Err(SemanticReactiveError::new(format!(
                "producer result expression {} has stale owner/type provenance for state {state}",
                expression.id
            )));
        }
        Ok(Some(state))
    }

    fn expression_is_directly_source_only(
        &self,
        expression: SemanticExprId,
    ) -> Result<bool, SemanticReactiveError> {
        let provenance = &self.execution.expression(expression)?.provenance;
        if provenance.members.is_empty() {
            return Ok(false);
        }
        for member in &provenance.members {
            match &member.origin {
                SemanticValueOrigin::Source { source, .. } => {
                    if self
                        .resources
                        .sources
                        .get(source.as_usize())
                        .filter(|candidate| candidate.id == *source)
                        .is_none()
                    {
                        return Err(SemanticReactiveError::new(format!(
                            "semantic expression {expression} provenance references missing source {source}"
                        )));
                    }
                }
                SemanticValueOrigin::ProducerSource {
                    function,
                    producer,
                    identity,
                    owner,
                } => {
                    let matches = self
                        .resources
                        .sources
                        .iter()
                        .filter(|source| {
                            source.owner == Some(*owner)
                                && matches!(
                                    source.origin,
                                    crate::SemanticSourceOrigin::ProducerInvocation {
                                        function: candidate_function,
                                        producer: candidate_producer,
                                        identity: candidate_identity,
                                    } if candidate_function == *function
                                        && candidate_producer == *producer
                                        && candidate_identity == *identity
                                )
                        })
                        .count();
                    if matches != 1 {
                        return Err(SemanticReactiveError::new(format!(
                            "semantic expression {expression} producer-source provenance resolves to {matches} exact sources"
                        )));
                    }
                }
                SemanticValueOrigin::Runtime
                | SemanticValueOrigin::State { .. }
                | SemanticValueOrigin::MaterializationLocal { .. } => return Ok(false),
            }
        }
        Ok(true)
    }

    fn bind_pulse_emission_derived_values(
        &self,
        pulse_batches: &mut [RawPulseBatch],
        derived_values: &mut [RawDerivedValue],
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<(), SemanticReactiveError> {
        for batch in pulse_batches {
            let consumers = batch
                .emission_routes
                .iter()
                .filter_map(|route| route.consumer)
                .collect::<BTreeSet<_>>();
            let mut matched_existing_route = false;
            let mut inferred_skip_routes =
                BTreeMap::<SemanticExprId, SemanticPulseEmissionRouteV1>::new();
            for derived in derived_values.iter_mut() {
                let reachable = self.reachable_expressions(derived.producer)?;
                let mut owns_emission = consumers
                    .iter()
                    .any(|consumer| reachable.contains(consumer));
                matched_existing_route |= owns_emission;
                if let Some(state) = batch.state {
                    for expression in &reachable {
                        let SemanticExpressionKind::Call {
                            call,
                            intrinsic: Some(CheckedIntrinsicV1::StreamSkip),
                            arguments,
                            ..
                        } = &self.execution.expression(*expression)?.kind
                        else {
                            continue;
                        };
                        let streams = arguments
                            .iter()
                            .filter(|argument| argument.name == "stream")
                            .collect::<Vec<_>>();
                        let [stream] = streams.as_slice() else {
                            return Err(SemanticReactiveError::new(format!(
                                "semantic Stream/skip expression {expression} resolves to {} stream arguments",
                                streams.len()
                            )));
                        };
                        if !triggers
                            .event_causes_for_expression(stream.value)?
                            .contains(&SemanticEventCauseV1::State(state))
                        {
                            continue;
                        }
                        let counts = arguments
                            .iter()
                            .filter(|argument| argument.name == "count")
                            .collect::<Vec<_>>();
                        let [count] = counts.as_slice() else {
                            return Err(SemanticReactiveError::new(format!(
                                "semantic Stream/skip expression {expression} resolves to {} count arguments",
                                counts.len()
                            )));
                        };
                        inferred_skip_routes.insert(
                            *expression,
                            SemanticPulseEmissionRouteV1 {
                                consumer: Some(*expression),
                                filter: SemanticPulseEmissionFilterV1::Skip {
                                    call: *call,
                                    expression: *expression,
                                    count_expression: count.value,
                                    count_value: self.execution.value(count.value)?,
                                },
                            },
                        );
                        owns_emission = true;
                    }
                }
                if !owns_emission {
                    continue;
                }
                if let Some(state) = batch.state {
                    derived
                        .trigger_arms
                        .retain(|arm| arm.cause != SemanticEventCauseV1::State(state));
                }
                derived.trigger_arms.push(triggers.arm(
                    SemanticEventCauseV1::Pulse(batch.id),
                    batch.call_expression,
                    derived.producer,
                )?);
                derived.trigger_arms.sort();
                derived.trigger_arms.dedup();
                derived.causes = derived
                    .trigger_arms
                    .iter()
                    .map(|arm| arm.cause)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
            }
            if !matched_existing_route && !inferred_skip_routes.is_empty() {
                batch.emission_routes = inferred_skip_routes.into_values().collect();
            }
        }
        Ok(())
    }

    fn hold_body_statement_ids(&self) -> BTreeSet<SemanticStatementId> {
        let mut pending = self
            .execution
            .statements
            .iter()
            .filter(|statement| matches!(statement.kind, SemanticStatementKind::Hold { .. }))
            .flat_map(|statement| statement.children.iter().copied())
            .collect::<Vec<_>>();
        let mut result = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !result.insert(id) {
                continue;
            }
            if let Some(statement) = self
                .execution
                .statements
                .get(id.as_usize())
                .filter(|candidate| candidate.id == id)
            {
                pending.extend(statement.children.iter().copied());
            }
        }
        result
    }

    fn expression_owns_event(&self, id: SemanticExprId) -> Result<bool, SemanticReactiveError> {
        let expression = self.execution.expression(id)?;
        Ok(matches!(
            expression.flow_type.mode,
            FlowMode::TickPresent | FlowMode::PresentOrAbsent
        ) || matches!(
            expression.kind,
            SemanticExpressionKind::Latest { .. } | SemanticExpressionKind::Hold { .. }
        ) || matches!(
            &expression.kind,
            SemanticExpressionKind::Call { name, .. } if name == "List/latest"
        ))
    }

    fn statement_is_structural_group(
        &self,
        id: SemanticStatementId,
    ) -> Result<bool, SemanticReactiveError> {
        let statement = self
            .execution
            .statements
            .get(id.as_usize())
            .filter(|candidate| candidate.id == id)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "derived value references missing semantic statement {id}"
                ))
            })?;
        if !matches!(statement.kind, SemanticStatementKind::Field { .. }) {
            return Ok(false);
        }
        let Some(value) = statement.value else {
            return Ok(false);
        };
        let expression = self.execution.expression(value)?;
        if !matches!(
            expression.kind,
            SemanticExpressionKind::Object(_) | SemanticExpressionKind::TaggedObject { .. }
        ) || statement.children.is_empty()
        {
            return Ok(false);
        }
        Ok(statement.children.iter().all(|child| {
            self.execution
                .statements
                .get(child.as_usize())
                .filter(|candidate| candidate.id == *child)
                .is_some_and(|child| {
                    matches!(
                        child.kind,
                        SemanticStatementKind::Field { .. }
                            | SemanticStatementKind::Source { .. }
                            | SemanticStatementKind::Hold { .. }
                            | SemanticStatementKind::List { .. }
                            | SemanticStatementKind::Spread
                    )
                })
        }))
    }

    fn expression_is_aggregate(
        &self,
        expression: SemanticExprId,
    ) -> Result<bool, SemanticReactiveError> {
        let expression = self.execution.expression(expression)?;
        let SemanticExpressionKind::Materialize { materialization } = expression.kind else {
            return Ok(false);
        };
        let materialization = self
            .execution
            .materializations
            .get(materialization.as_usize())
            .filter(|candidate| candidate.id == materialization)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "derived value references missing materialization {}",
                    materialization
                ))
            })?;
        Ok(matches!(
            materialization.operation,
            SemanticContextualOperationKind::Every
                | SemanticContextualOperationKind::Any
                | SemanticContextualOperationKind::Find
        ))
    }

    fn build_dependencies(
        &self,
        state_arms: &[SemanticStateUpdateArmV1],
        triggers: &[SemanticTriggerOwnedArmV1],
        pulse_states: &BTreeMap<SemanticPulseBatchId, SemanticStateId>,
    ) -> Result<Vec<SemanticDependencyEdgeV1>, SemanticReactiveError> {
        let mut edges = BTreeSet::new();
        for arm in state_arms {
            let trigger = require_trigger(triggers, arm.trigger)?;
            let target = self
                .resources
                .states
                .get(arm.state.as_usize())
                .filter(|state| state.id == arm.state)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "state update arm references missing state {}",
                        arm.state
                    ))
                })?;
            let source_indexed = match trigger.cause {
                SemanticEventCauseV1::Source(source) => {
                    self.resources
                        .sources
                        .get(source.as_usize())
                        .filter(|candidate| candidate.id == source)
                        .ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "trigger references missing source {}",
                                source
                            ))
                        })?
                        .scoped
                }
                SemanticEventCauseV1::State(state) => {
                    self.resources
                        .states
                        .get(state.as_usize())
                        .filter(|candidate| candidate.id == state)
                        .ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "trigger references missing state {}",
                                state
                            ))
                        })?
                        .scoped
                }
                SemanticEventCauseV1::Pulse(pulse) => pulse_states
                    .get(&pulse)
                    .and_then(|state| {
                        self.resources
                            .states
                            .get(state.as_usize())
                            .filter(|candidate| candidate.id == *state)
                    })
                    .is_some_and(|state| state.scoped),
                SemanticEventCauseV1::ExternalRead(_) => false,
            };
            edges.insert((trigger.cause, arm.state, target.scoped || source_indexed));
        }
        Ok(edges
            .into_iter()
            .enumerate()
            .map(|(index, (from, to, indexed))| SemanticDependencyEdgeV1 {
                id: SemanticExternalDependencyId(index),
                from,
                to,
                indexed,
            })
            .collect())
    }

    fn build_possible_causes(
        &self,
        state_arms: &[SemanticStateUpdateArmV1],
        triggers: &[SemanticTriggerOwnedArmV1],
    ) -> Result<Vec<SemanticPossibleCausesV1>, SemanticReactiveError> {
        let mut by_state = self
            .resources
            .states
            .iter()
            .map(|state| (state.id, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        for arm in state_arms {
            let trigger = require_trigger(triggers, arm.trigger)?;
            by_state
                .get_mut(&arm.state)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "state update arm references missing state {}",
                        arm.state
                    ))
                })?
                .insert(trigger.cause);
        }
        self.resources
            .states
            .iter()
            .map(|state| {
                let causes = by_state
                    .remove(&state.id)
                    .ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "state {} has no initialized possible-cause inventory",
                            state.id
                        ))
                    })?
                    .into_iter()
                    .collect();
                Ok(SemanticPossibleCausesV1 {
                    state: state.id,
                    causes,
                })
            })
            .collect()
    }

    fn build_host_effect_schedules(
        &self,
        fields: &[SemanticFieldV1],
        bindings: &[SemanticBindingV1],
        derived_values: &[SemanticDerivedValueV1],
        state_arms: &[SemanticStateUpdateArmV1],
        triggers: &[SemanticTriggerOwnedArmV1],
    ) -> Result<Vec<SemanticHostEffectScheduleV1>, SemanticReactiveError> {
        let mut schedules = Vec::new();
        let runtime_roots = self
            .execution
            .roots
            .iter()
            .map(|root| root.expression)
            .chain(
                self.execution
                    .functions
                    .iter()
                    .map(|function| function.root),
            )
            .chain(
                self.execution
                    .sources
                    .iter()
                    .map(|source| source.expression),
            )
            .chain(self.resources.states.iter().map(|state| state.expression))
            .chain(self.resources.lists.iter().map(|list| list.producer))
            .chain(fields.iter().map(|field| field.producer))
            .collect::<BTreeSet<_>>();
        for expression in &self.execution.expressions {
            let SemanticExpressionKind::Call { call, function, .. } = &expression.kind else {
                continue;
            };
            if !boon_typecheck::is_typed_host_effect(function) {
                continue;
            }
            let mut runtime_reachable = false;
            for root in &runtime_roots {
                if self.expression_value_reaches(*root, expression.id, bindings)? {
                    runtime_reachable = true;
                    break;
                }
            }
            if !runtime_reachable {
                continue;
            }
            let mut transient_results = Vec::new();
            for derived in derived_values.iter().filter(|derived| {
                derived.state_backing.is_none() && !derived.trigger_arms.is_empty()
            }) {
                if self.expression_reaches(derived.producer, expression.id)? {
                    transient_results.push(derived.id);
                }
            }
            let transient_result = match transient_results.as_slice() {
                [derived] => {
                    let derived = derived_values
                        .get(derived.as_usize())
                        .filter(|candidate| candidate.id == *derived)
                        .ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "typed host effect `{function}` references missing transient derived value {derived}"
                            ))
                        })?;
                    if derived.trigger_arms.is_empty() {
                        return Err(SemanticReactiveError::new(format!(
                            "typed host effect `{function}` at semantic expression {} has a transient result without an exact event trigger",
                            expression.id
                        )));
                    }
                    Some(derived.id)
                }
                [] => None,
                _ => {
                    return Err(SemanticReactiveError::new(format!(
                        "typed host effect `{function}` at semantic expression {} reaches multiple transient result owners {transient_results:?}",
                        expression.id
                    )));
                }
            };
            if let Some(transient_result) = transient_result {
                schedules.push(SemanticHostEffectScheduleV1 {
                    id: SemanticHostEffectScheduleId(schedules.len()),
                    expression: expression.id,
                    value: expression.value_id,
                    call: *call,
                    checked_expression: expression.checked_expr_id,
                    owner: expression.owner,
                    operation: function.clone(),
                    state_update_arms: Vec::new(),
                    transient_result: Some(transient_result),
                });
                continue;
            }
            let mut covering = Vec::new();
            let mut candidates = Vec::new();
            for arm in state_arms {
                let trigger = require_trigger(triggers, arm.trigger)?;
                let reaches = self.expression_value_reaches(
                    trigger.output_expression,
                    expression.id,
                    bindings,
                )?;
                candidates.push((
                    arm.id,
                    arm.state,
                    trigger.id,
                    trigger.owner,
                    trigger.output_expression,
                    reaches,
                ));
                if trigger.owner == expression.owner && reaches {
                    covering.push(arm.id);
                }
            }
            if covering.is_empty() {
                let effect_occurrences = self
                    .execution
                    .expressions
                    .iter()
                    .filter_map(|candidate| {
                        let SemanticExpressionKind::Call {
                            call,
                            function: candidate_function,
                            ..
                        } = &candidate.kind
                        else {
                            return None;
                        };
                        (boon_typecheck::is_typed_host_effect(candidate_function)
                            && candidate.checked_expr_id == expression.checked_expr_id)
                            .then(|| {
                                let origin = self
                                    .execution
                                    .checked_expression_origins
                                    .get(candidate.id.as_usize())
                                    .filter(|origin| origin.expression == candidate.id);
                                (
                                    candidate.id,
                                    *call,
                                    candidate.owner,
                                    origin.map(|origin| {
                                        (origin.owning_statement, origin.call_instance)
                                    }),
                                )
                            })
                    })
                    .collect::<Vec<_>>();
                return Err(SemanticReactiveError::new(format!(
                    "typed host effect `{function}` at semantic expression {} checked {} owner {:?} has no exact state update schedule among {candidates:?}; exact checked occurrences: {effect_occurrences:?}",
                    expression.id, expression.checked_expr_id.0, expression.owner,
                )));
            }
            covering.sort();
            covering.dedup();
            schedules.push(SemanticHostEffectScheduleV1 {
                id: SemanticHostEffectScheduleId(schedules.len()),
                expression: expression.id,
                value: expression.value_id,
                call: *call,
                checked_expression: expression.checked_expr_id,
                owner: expression.owner,
                operation: function.clone(),
                state_update_arms: covering,
                transient_result: None,
            });
        }
        Ok(schedules)
    }

    fn build_call_invocation_schedules(
        &self,
        bindings: &[SemanticBindingV1],
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<Vec<RawCallInvocationSchedule>, SemanticReactiveError> {
        let mut schedules = Vec::new();
        for expression in &self.execution.expressions {
            let SemanticExpressionKind::Call {
                call,
                callable_kind,
                arguments,
                result,
                effect,
                ..
            } = &expression.kind
            else {
                continue;
            };
            let call_definition = self.execution.call(*call)?;
            if *callable_kind != crate::SemanticCallableKind::External {
                continue;
            }
            let callable = self.execution.callable(call_definition.callable)?;
            if callable.kind != boon_typecheck::CheckedCallableKind::External {
                return Err(SemanticReactiveError::new(format!(
                    "external call expression {} maps to non-external callable {}",
                    expression.id, callable.id
                )));
            }
            if expression.flow_type != *result || expression.effect != *effect {
                return Err(SemanticReactiveError::new(format!(
                    "external call expression {} has inconsistent normalized result/effect metadata",
                    expression.id
                )));
            }
            let current_capable = result.mode == FlowMode::Continuous
                && arguments.iter().all(|argument| {
                    self.execution
                        .expression(argument.value)
                        .is_ok_and(|value| value.flow_type.mode == FlowMode::Continuous)
                })
                && !effect.emits_source
                && !effect.invokes_host;
            let mut dependent_bindings = Vec::new();
            let mut invocation_arms = BTreeSet::new();
            for binding in bindings {
                if !self
                    .reachable_expressions(binding.producer)?
                    .contains(&expression.id)
                {
                    continue;
                }
                dependent_bindings.push(binding.id);
                let candidate_arms = if current_capable {
                    triggers.trigger_arms_before_expression(binding.producer, expression.id)?
                } else {
                    triggers.trigger_arms_for_expression(binding.producer)?
                };
                for arm in candidate_arms {
                    if self.expression_reaches(arm.output_expression, expression.id)? {
                        invocation_arms.insert(arm);
                    }
                }
            }
            dependent_bindings.sort();
            dependent_bindings.dedup();
            if dependent_bindings.is_empty() {
                return Err(SemanticReactiveError::new(format!(
                    "external call expression {} has no exact dependent semantic binding",
                    expression.id
                )));
            }
            if !current_capable && invocation_arms.is_empty() {
                return Err(SemanticReactiveError::new(format!(
                    "distributed call `{}` expression {} is eventful, stateful, or host-effectful but has no exact SOURCE or state trigger",
                    call_definition.function, expression.id
                )));
            }
            schedules.push(RawCallInvocationSchedule {
                expression: expression.id,
                value: expression.value_id,
                call: *call,
                current_capable,
                dependent_bindings,
                invocation_arms: invocation_arms.into_iter().collect(),
            });
        }
        schedules.sort_by_key(|schedule| schedule.expression);
        Ok(schedules)
    }

    fn build_dependency_uses(
        &self,
        bindings: &[SemanticBindingV1],
        reads: &[SemanticReadBindingV1],
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<Vec<SemanticDependencyUseV1>, SemanticReactiveError> {
        let reads_by_expression = reads
            .iter()
            .map(|read| (read.expression, read))
            .collect::<BTreeMap<_, _>>();
        let mut raw = BTreeSet::new();
        for binding in bindings {
            let reachable = self.reachable_expressions(binding.producer)?;
            let boundaries = triggers
                .event_causes_for_expression(binding.producer)
                .map_err(|error| {
                    SemanticReactiveError::new(format!(
                        "semantic binding {} declaration {} producer {} failed dependency analysis: {error}",
                        binding.id, binding.declaration.0, binding.producer
                    ))
                })?
                .into_iter()
                .collect::<Vec<_>>();
            for expression_id in reachable {
                let expression = self.execution.expression(expression_id)?;
                if let Some(read) = reads_by_expression.get(&expression_id)
                    && matches!(read.target, SemanticReadTargetV1::External { .. })
                {
                    raw.insert((
                        binding.id,
                        expression_id,
                        SemanticDependencyTargetV1::ExternalRead { read: read.id },
                        boundaries.clone(),
                    ));
                }
                if let SemanticExpressionKind::Call { call, .. } = expression.kind {
                    let semantic_call = self.execution.call(call)?;
                    if semantic_call.external_identity.is_some() {
                        raw.insert((
                            binding.id,
                            expression_id,
                            SemanticDependencyTargetV1::ExternalCall {
                                call,
                                expression: expression_id,
                            },
                            boundaries.clone(),
                        ));
                    }
                }
            }
        }
        Ok(raw
            .into_iter()
            .enumerate()
            .map(
                |(index, (dependent, expression, target, boundaries))| SemanticDependencyUseV1 {
                    id: SemanticDependencyUseId(index),
                    dependent,
                    expression,
                    target,
                    timing: if boundaries.is_empty() {
                        SemanticDependencyTimingV1::Immediate
                    } else {
                        SemanticDependencyTimingV1::After { boundaries }
                    },
                },
            )
            .collect())
    }

    fn build_output_values(
        &self,
        fields: &[SemanticFieldV1],
    ) -> Result<Vec<SemanticOutputValueV1>, SemanticReactiveError> {
        let fields_by_statement = fields
            .iter()
            .map(|field| (field.statement, field.id))
            .collect::<BTreeMap<_, _>>();
        self.execution
            .roots
            .iter()
            .filter(|root| {
                matches!(
                    root.kind,
                    SemanticRootKindV1::RetainedVisualDocument
                        | SemanticRootKindV1::RetainedVisualScene
                )
            })
            .enumerate()
            .map(|(ordinal, root)| {
                let expression = self.execution.expression(root.expression)?;
                let origin = self.execution.origin(root.expression)?;
                if expression.checked_expr_id != root.checked_expr_id
                    || expression.value_id != root.value
                    || origin.checked_expression != root.checked_expr_id
                    || origin.owning_statement != Some(root.statement)
                {
                    return Err(SemanticReactiveError::new(format!(
                        "retained execution root {} differs from exact semantic expression/statement provenance",
                        root.ordinal
                    )));
                }
                Ok(SemanticOutputValueV1 {
                    ordinal,
                    checked_expression: expression.checked_expr_id,
                    expression: root.expression,
                    value: root.value,
                    statement: root.statement,
                    field: fields_by_statement.get(&root.statement).copied(),
                    route_scope: self.execution.route_scope(root.expression)?,
                })
            })
            .collect()
    }

    fn build_view_captures(
        &self,
        outputs: &[SemanticOutputValueV1],
        fields: &[SemanticFieldV1],
        bindings: &[SemanticBindingV1],
        reads: &[SemanticReadBindingV1],
        triggers: &mut TriggerResolver<'_>,
        pulse_states: &BTreeMap<SemanticPulseBatchId, SemanticStateId>,
    ) -> Result<Vec<SemanticViewCaptureV1>, SemanticReactiveError> {
        let reads_by_expression = reads
            .iter()
            .map(|read| (read.expression, read))
            .collect::<BTreeMap<_, _>>();
        let fields_by_expression = fields
            .iter()
            .map(|field| (field.producer, field))
            .collect::<BTreeMap<_, _>>();
        let mut raw = BTreeSet::new();
        for output in outputs {
            for expression in self.reachable_expressions(output.expression)? {
                if let Some(read) = reads_by_expression.get(&expression) {
                    let target = match &read.target {
                        SemanticReadTargetV1::SourcePayload { source, .. } => {
                            SemanticViewCaptureTargetV1::Source { source: *source }
                        }
                        SemanticReadTargetV1::Binding { binding, .. } => {
                            let binding = bindings
                                .get(binding.as_usize())
                                .filter(|candidate| candidate.id == *binding)
                                .ok_or_else(|| {
                                    SemanticReactiveError::new(format!(
                                        "view capture read {} references missing binding {binding}",
                                        read.id
                                    ))
                                })?;
                            if let SemanticBindingTargetV1::Source { source } = binding.target {
                                SemanticViewCaptureTargetV1::Source { source }
                            } else {
                                SemanticViewCaptureTargetV1::Read { read: read.id }
                            }
                        }
                        _ => SemanticViewCaptureTargetV1::Read { read: read.id },
                    };
                    let trigger_scope = trigger_row_scope(
                        &triggers.event_causes_for_expression(expression)?,
                        self.resources,
                        pulse_states,
                    )?;
                    let read_scope = materialization_local_read_scope(read, self.execution)?;
                    let row_scope = match (trigger_scope, read_scope) {
                        (Some(trigger), Some(read)) if trigger != read => {
                            return Err(SemanticReactiveError::new(format!(
                                "view capture expression {expression} has trigger row scope {trigger} but reads materialization row scope {read}"
                            )));
                        }
                        (Some(scope), _) | (_, Some(scope)) => Some(scope),
                        (None, None) => None,
                    };
                    raw.insert((output.ordinal, expression, read.value, target, row_scope));
                } else if let Some(field) = fields_by_expression.get(&expression) {
                    raw.insert((
                        output.ordinal,
                        expression,
                        field.value,
                        SemanticViewCaptureTargetV1::Field { field: field.id },
                        field.row.map(|row| row.scope),
                    ));
                }
            }
        }
        Ok(raw
            .into_iter()
            .enumerate()
            .map(
                |(index, (output_ordinal, expression, value, target, row_scope))| {
                    SemanticViewCaptureV1 {
                        id: SemanticCaptureId(index),
                        output_ordinal,
                        expression,
                        value,
                        target,
                        row_scope,
                    }
                },
            )
            .collect())
    }

    fn build_migration_inputs(
        &self,
    ) -> Result<Vec<SemanticMigrationInputV1>, SemanticReactiveError> {
        let mut result = Vec::new();
        for expression in &self.execution.expressions {
            let SemanticExpressionKind::Draining { input } = expression.kind else {
                continue;
            };
            result.push(SemanticMigrationInputV1 {
                id: crate::SemanticMigrationId(result.len()),
                marker: expression.id,
                marker_value: expression.value_id,
                input,
                input_value: self.execution.value(input)?,
                owner: expression.owner,
                route_scope: self.execution.route_scope(expression.id)?,
            });
        }
        Ok(result)
    }

    fn reachable_expressions(
        &self,
        root: SemanticExprId,
    ) -> Result<BTreeSet<SemanticExprId>, SemanticReactiveError> {
        let mut result = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(expression) = pending.pop() {
            if !result.insert(expression) {
                continue;
            }
            let expression = self.execution.expression(expression)?;
            pending.extend(semantic_expression_children(
                &expression.kind,
                self.execution,
            )?);
        }
        Ok(result)
    }

    fn expression_reaches(
        &self,
        root: SemanticExprId,
        target: SemanticExprId,
    ) -> Result<bool, SemanticReactiveError> {
        Ok(self.reachable_expressions(root)?.contains(&target))
    }

    fn expression_value_reaches(
        &self,
        root: SemanticExprId,
        target: SemanticExprId,
        bindings: &[SemanticBindingV1],
    ) -> Result<bool, SemanticReactiveError> {
        let mut pending = vec![root];
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            if id == target {
                return Ok(true);
            }
            let expression = self.execution.expression(id)?;
            match &expression.kind {
                SemanticExpressionKind::CanonicalRead {
                    target: declaration,
                    ..
                } => {
                    let binding = self.resolve_decl_binding(*declaration, expression, bindings)?;
                    pending.push(binding.producer);
                }
                SemanticExpressionKind::LocalRead { binding, .. } => {
                    let (_, producer) =
                        self.local_values.get(binding).copied().ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "semantic value reachability read {id} references missing local binding {binding}"
                            ))
                        })?;
                    pending.push(producer);
                }
                SemanticExpressionKind::FunctionParameter { parameter, .. } => {
                    let inputs = self.parameter_inputs.get(&id).ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "semantic value reachability parameter expression {id} ({parameter:?}) has no exact producer inputs"
                        ))
                    })?;
                    pending.extend(inputs.iter().copied());
                }
                _ => pending.extend(semantic_expression_children(
                    &expression.kind,
                    self.execution,
                )?),
            }
        }
        Ok(false)
    }
}

#[derive(Clone, Debug)]
struct RawDerivedValue {
    binding: SemanticBindingId,
    field: SemanticFieldId,
    statement: SemanticStatementId,
    producer: SemanticExprId,
    value: SemanticValueId,
    kind: SemanticDerivedValueKindV1,
    state_backing: Option<SemanticStateId>,
    materialized_list: Option<SemanticListId>,
    materialized_row_scope: Option<SemanticRowScopeId>,
    causes: Vec<SemanticEventCauseV1>,
    trigger_arms: Vec<RawTriggerArm>,
    default_values: Vec<SemanticValueId>,
    startup_recompute: bool,
}

struct TriggerResolver<'a> {
    execution: &'a SemanticExecutionGraphV1,
    resources: &'a SemanticResourceGraphV1,
    out_net: &'a ResolvedOutGraph,
    bindings: &'a [SemanticBindingV1],
    local_values: &'a BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
    parameter_inputs: &'a BTreeMap<SemanticExprId, Vec<SemanticExprId>>,
    pulse_by_expression: &'a BTreeMap<SemanticExprId, SemanticPulseBatchId>,
    pulse_states: &'a BTreeMap<SemanticPulseBatchId, SemanticStateId>,
    pulse_activation_expressions: &'a BTreeSet<SemanticExprId>,
    external_event_identities: &'a BTreeSet<CheckedExternalDeclarationIdentityV1>,
    causes_cache: BTreeMap<SemanticExprId, BTreeSet<SemanticEventCauseV1>>,
}

fn lexical_owner_distance(
    execution: &SemanticExecutionGraphV1,
    descendant: Option<StaticOwnerId>,
    ancestor: Option<StaticOwnerId>,
) -> Result<Option<usize>, SemanticReactiveError> {
    let mut current = descendant;
    let mut distance = 0usize;
    let mut visited = BTreeSet::new();
    loop {
        if current == ancestor {
            return Ok(Some(distance));
        }
        let Some(owner) = current else {
            return Ok(None);
        };
        if !visited.insert(owner) {
            return Err(SemanticReactiveError::new(format!(
                "semantic static owner {owner} has cyclic ancestry"
            )));
        }
        let definition = execution
            .static_owners
            .get(owner.as_usize())
            .filter(|candidate| candidate.id == owner)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "semantic static owner ancestry references missing owner {owner}"
                ))
            })?;
        current = definition.parent;
        distance = distance.saturating_add(1);
    }
}

fn lexical_call_frame_distance(
    out_net: &ResolvedOutGraph,
    descendant: Option<crate::OutCallInstanceId>,
    ancestor: Option<crate::OutCallInstanceId>,
) -> Result<Option<usize>, SemanticReactiveError> {
    let mut current = descendant;
    let mut distance = 0usize;
    let mut visited = BTreeSet::new();
    loop {
        if current == ancestor {
            return Ok(Some(distance));
        }
        let Some(call) = current else {
            return Ok(None);
        };
        if !visited.insert(call) {
            return Err(SemanticReactiveError::new(format!(
                "semantic call frame {call} has cyclic ancestry"
            )));
        }
        let instance = out_net
            .call_instances
            .get(call.as_usize())
            .filter(|candidate| candidate.id == call)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "semantic call-frame ancestry references missing call {call}"
                ))
            })?;
        current = instance.parent;
        distance = distance.saturating_add(1);
    }
}

fn lexical_binding_for_decl<'a>(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    out_net: &ResolvedOutGraph,
    bindings: &'a [SemanticBindingV1],
    declaration: DeclId,
    expression: &SemanticExpression,
    call_instance: Option<crate::OutCallInstanceId>,
    diagnostic: &str,
) -> Result<&'a SemanticBindingV1, SemanticReactiveError> {
    let mut candidates = Vec::new();
    for binding in bindings
        .iter()
        .filter(|binding| binding.declaration == declaration)
    {
        let Some(frame_distance) =
            lexical_call_frame_distance(out_net, call_instance, binding.call_instance)?
        else {
            continue;
        };
        let Some(owner_distance) =
            lexical_owner_distance(execution, expression.owner, binding.owner)?
        else {
            continue;
        };
        let target_priority = match binding.target {
            SemanticBindingTargetV1::State { state } => {
                let state = resources
                    .states
                    .get(state.as_usize())
                    .filter(|candidate| candidate.id == state)
                    .ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "{diagnostic} {} binding {} references missing semantic state {state}",
                            expression.id, binding.id
                        ))
                    })?;
                if state.published { 0_u8 } else { 1 }
            }
            SemanticBindingTargetV1::List { .. } => 0_u8,
            SemanticBindingTargetV1::Field { .. } => 2,
            // Invocation-mode producer results may share one declaration with
            // their trigger SOURCE. The ordinary result field remains the
            // lexical value; the SOURCE is only event routing metadata.
            SemanticBindingTargetV1::Source { .. } => 3,
        };
        candidates.push(((frame_distance, owner_distance, target_priority), binding));
    }
    candidates.sort_by_key(|(distance, binding)| (*distance, binding.id));
    let Some((best_distance, _)) = candidates.first() else {
        return Err(SemanticReactiveError::new(format!(
            "{diagnostic} {} resolves declaration {} to no lexically visible owner/frame binding",
            expression.id, declaration.0
        )));
    };
    let best = candidates
        .iter()
        .filter(|(distance, _)| distance == best_distance)
        .map(|(_, binding)| *binding)
        .collect::<Vec<_>>();
    let [binding] = best.as_slice() else {
        return Err(SemanticReactiveError::new(format!(
            "{diagnostic} {} resolves declaration {} to {} equally-near lexical bindings: {:?}",
            expression.id,
            declaration.0,
            best.len(),
            best.iter()
                .map(|binding| (
                    binding.id,
                    binding.statement,
                    binding.call_instance,
                    binding.owner,
                    binding.producer,
                    binding.target,
                ))
                .collect::<Vec<_>>()
        )));
    };
    Ok(*binding)
}

impl<'a> TriggerResolver<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        execution: &'a SemanticExecutionGraphV1,
        resources: &'a SemanticResourceGraphV1,
        out_net: &'a ResolvedOutGraph,
        bindings: &'a [SemanticBindingV1],
        local_values: &'a BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
        parameter_inputs: &'a BTreeMap<SemanticExprId, Vec<SemanticExprId>>,
        pulse_by_expression: &'a BTreeMap<SemanticExprId, SemanticPulseBatchId>,
        pulse_states: &'a BTreeMap<SemanticPulseBatchId, SemanticStateId>,
        pulse_activation_expressions: &'a BTreeSet<SemanticExprId>,
        external_event_identities: &'a BTreeSet<CheckedExternalDeclarationIdentityV1>,
    ) -> Result<Self, SemanticReactiveError> {
        for source in &resources.sources {
            execution.source(source.id)?;
        }
        for state in &resources.states {
            execution.state(state.id)?;
        }
        Ok(Self {
            execution,
            resources,
            out_net,
            bindings,
            local_values,
            parameter_inputs,
            pulse_by_expression,
            pulse_states,
            pulse_activation_expressions,
            external_event_identities,
            causes_cache: BTreeMap::new(),
        })
    }

    fn event_causes_for_expression(
        &mut self,
        root: SemanticExprId,
    ) -> Result<BTreeSet<SemanticEventCauseV1>, SemanticReactiveError> {
        if let Some(cached) = self.causes_cache.get(&root) {
            return Ok(cached.clone());
        }
        let mut causes = BTreeSet::new();
        self.collect_event_causes(root, None, &mut BTreeSet::new(), &mut causes)?;
        self.causes_cache.insert(root, causes.clone());
        Ok(causes)
    }

    fn collect_event_causes(
        &mut self,
        id: SemanticExprId,
        terminal: Option<SemanticExprId>,
        visited: &mut BTreeSet<SemanticExprId>,
        causes: &mut BTreeSet<SemanticEventCauseV1>,
    ) -> Result<(), SemanticReactiveError> {
        if terminal == Some(id) || !visited.insert(id) {
            return Ok(());
        }
        let expression = self.execution.expression(id)?;
        let direct = self.direct_causes(expression)?;
        if !direct.is_empty() {
            causes.extend(direct);
            return Ok(());
        }
        match &expression.kind {
            // The input event owns activation of the bounded pulse batch.  It
            // is not also a direct emission from the activated expression:
            // initial HOLD publication and each admitted microturn are
            // represented by the Pulse cause inside the output.
            SemanticExpressionKind::Then {
                input,
                output: Some(output),
                ..
            } if self.pulse_activation_expressions.contains(&id) => {
                self.collect_event_causes(*output, Some(*input), visited, causes)?;
            }
            SemanticExpressionKind::CanonicalRead { target, .. } => {
                let binding = self.exact_binding_for_decl(*target, expression)?;
                self.collect_event_causes(binding.producer, terminal, visited, causes)?;
            }
            SemanticExpressionKind::LocalRead { binding, .. } => {
                let (_, producer) = self.local_values.get(binding).copied().ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic local read {} references missing binding {}",
                        id, binding
                    ))
                })?;
                self.collect_event_causes(producer, terminal, visited, causes)?;
            }
            SemanticExpressionKind::FunctionParameter { parameter, .. } => {
                let inputs = self.parameter_inputs.get(&id).ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic function parameter expression {} ({:?}) has no exact producer input inventory",
                        id, parameter
                    ))
                })?;
                for input in inputs {
                    self.collect_event_causes(*input, terminal, visited, causes)?;
                }
            }
            _ => {
                for child in semantic_expression_children(&expression.kind, self.execution)? {
                    self.collect_event_causes(child, terminal, visited, causes)?;
                }
            }
        }
        Ok(())
    }

    fn direct_causes(
        &self,
        expression: &SemanticExpression,
    ) -> Result<BTreeSet<SemanticEventCauseV1>, SemanticReactiveError> {
        let mut causes = BTreeSet::new();
        if let SemanticExpressionKind::ExternalRead {
            external_identity: Some(external_identity),
            ..
        } = &expression.kind
            && matches!(
                expression.flow_type.mode,
                FlowMode::TickPresent | FlowMode::PresentOrAbsent
            )
            && self.external_event_identities.contains(external_identity)
        {
            causes.insert(SemanticEventCauseV1::ExternalRead(expression.id));
            return Ok(causes);
        }
        if matches!(
            &expression.kind,
            SemanticExpressionKind::Call {
                intrinsic: Some(CheckedIntrinsicV1::StreamPulses),
                ..
            }
        ) {
            let pulse = self
                .pulse_by_expression
                .get(&expression.id)
                .copied()
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic Stream/pulses expression {} has no pulse batch identity",
                        expression.id
                    ))
                })?;
            causes.insert(SemanticEventCauseV1::Pulse(pulse));
            return Ok(causes);
        }
        if let SemanticExpressionKind::CanonicalRead {
            target,
            projection,
            source,
            ..
        } = &expression.kind
        {
            if let Some(source) = source {
                causes.insert(SemanticEventCauseV1::Source(source.source));
                return Ok(causes);
            }
            let binding = self.exact_binding_for_decl(*target, expression)?;
            match binding.target {
                SemanticBindingTargetV1::Source { source } => {
                    causes.insert(SemanticEventCauseV1::Source(source));
                }
                SemanticBindingTargetV1::State { state } => {
                    causes.insert(SemanticEventCauseV1::State(state));
                }
                SemanticBindingTargetV1::Field { .. } | SemanticBindingTargetV1::List { .. }
                    if projection.is_empty() =>
                {
                    return Ok(causes);
                }
                SemanticBindingTargetV1::Field { .. } | SemanticBindingTargetV1::List { .. } => {}
            }
            if !causes.is_empty() {
                return Ok(causes);
            }
        }
        for member in &expression.provenance.members {
            match member.origin {
                SemanticValueOrigin::Source { source, .. } => {
                    causes.insert(SemanticEventCauseV1::Source(source));
                }
                SemanticValueOrigin::State { state, .. } => {
                    causes.insert(SemanticEventCauseV1::State(state));
                }
                SemanticValueOrigin::ProducerSource {
                    identity, owner, ..
                } => {
                    let matches = self
                        .resources
                        .producer_resources
                        .iter()
                        .filter(|resource| resource.identity == identity && resource.owner == owner)
                        .filter_map(|resource| resource.invocation_source)
                        .collect::<BTreeSet<_>>();
                    let sources = matches.into_iter().collect::<Vec<_>>();
                    match sources.as_slice() {
                        [] => {}
                        [source] => {
                            causes.insert(SemanticEventCauseV1::Source(*source));
                        }
                        _ => {
                            return Err(SemanticReactiveError::new(format!(
                                "producer provenance at expression {} resolves to {} invocation sources",
                                expression.id,
                                sources.len()
                            )));
                        }
                    }
                }
                SemanticValueOrigin::MaterializationLocal {
                    owner,
                    local,
                    ref projection,
                } => {
                    if let Some(source) =
                        self.materialization_local_source(owner, local, projection)?
                    {
                        causes.insert(SemanticEventCauseV1::Source(source));
                    }
                }
                SemanticValueOrigin::Runtime => {}
            }
        }
        if matches!(expression.kind, SemanticExpressionKind::Source { .. }) {
            let mut matches = self
                .resources
                .sources
                .iter()
                .filter(|source| source.expression == expression.id)
                .map(|source| source.id)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                let origin = self.execution.origin(expression.id)?;
                matches = self
                    .resources
                    .sources
                    .iter()
                    .filter_map(|source| {
                        let definition = self
                            .execution
                            .sources
                            .get(source.id.as_usize())
                            .filter(|definition| definition.id == source.id)?;
                        let producer = self
                            .execution
                            .expressions
                            .get(definition.expression.as_usize())
                            .filter(|producer| producer.id == definition.expression)?;
                        (producer.checked_expr_id == expression.checked_expr_id
                            && definition.owner == expression.owner
                            && definition.call_instance == origin.call_instance)
                            .then_some(source.id)
                    })
                    .collect();
            }
            matches.sort();
            matches.dedup();
            let [source] = matches.as_slice() else {
                let details = matches
                    .iter()
                    .filter_map(|source| {
                        self.execution
                            .sources
                            .get(source.as_usize())
                            .filter(|definition| definition.id == *source)
                            .map(|definition| {
                                (
                                    definition.id,
                                    definition.expression,
                                    definition.owner,
                                    definition.call_instance,
                                    definition.binding_path.as_str(),
                                )
                            })
                    })
                    .collect::<Vec<_>>();
                return Err(SemanticReactiveError::new(format!(
                    "semantic SOURCE expression {} resolves to {} exact occurrence resources: {details:?}",
                    expression.id,
                    matches.len()
                )));
            };
            causes.insert(SemanticEventCauseV1::Source(*source));
        }
        Ok(causes)
    }

    fn materialization_local_source(
        &self,
        owner: StaticOwnerId,
        local: SemanticMaterializationLocalId,
        projection: &[String],
    ) -> Result<Option<SemanticSourceId>, SemanticReactiveError> {
        if projection.is_empty() {
            return Ok(None);
        }
        let source_lists = self
            .execution
            .materializations
            .iter()
            .filter(|materialization| {
                materialization.owner == owner && materialization.row_local == local
            })
            .filter_map(|materialization| materialization.source_list_id)
            .collect::<BTreeSet<_>>();
        let mut matches = BTreeSet::new();
        for list_id in source_lists {
            let list = self
                .resources
                .lists
                .get(list_id.as_usize())
                .filter(|list| list.id == list_id)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "materialization owner {owner} local {} references missing source list {list_id}",
                        local.0
                    ))
                })?;
            let path = format!("{}.{}", list.semantic_path, projection.join("."));
            matches.extend(
                self.resources
                    .sources
                    .iter()
                    .filter(|source| {
                        source.target_list == Some(list_id) && source.semantic_path == path
                    })
                    .map(|source| source.id),
            );
        }
        let matches = matches.into_iter().collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Ok(None),
            [source] => Ok(Some(*source)),
            _ => Err(SemanticReactiveError::new(format!(
                "materialization owner {owner} local {} projection `{}` resolves to {} sources",
                local.0,
                projection.join("."),
                matches.len()
            ))),
        }
    }

    fn trigger_arms_for_expression(
        &mut self,
        root: SemanticExprId,
    ) -> Result<Vec<RawTriggerArm>, SemanticReactiveError> {
        let mut arms = BTreeSet::new();
        self.collect_trigger_arms(root, None, &mut BTreeSet::new(), &mut arms)?;
        if arms.is_empty() {
            for cause in self.event_causes_for_expression(root)? {
                arms.insert(self.arm(cause, root, root)?);
            }
        }
        Ok(arms.into_iter().collect())
    }

    fn collect_trigger_arms(
        &mut self,
        id: SemanticExprId,
        terminal: Option<SemanticExprId>,
        visited: &mut BTreeSet<SemanticExprId>,
        arms: &mut BTreeSet<RawTriggerArm>,
    ) -> Result<(), SemanticReactiveError> {
        if terminal == Some(id) || !visited.insert(id) {
            return Ok(());
        }
        let expression = self.execution.expression(id)?;
        match &expression.kind {
            SemanticExpressionKind::When {
                input,
                arms: select_arms,
                ..
            } => {
                let input_arms = self.trigger_arms_before(*input, terminal)?;
                if !input_arms.is_empty() {
                    for input_arm in input_arms {
                        arms.insert(self.arm(input_arm.cause, *input, id)?);
                    }
                    return Ok(());
                }
                for arm in select_arms {
                    self.collect_trigger_arms(arm.output, terminal, visited, arms)?;
                }
            }
            SemanticExpressionKind::Then { input, output } => {
                if self.pulse_activation_expressions.contains(&id) {
                    if let Some(output) = output {
                        self.collect_trigger_arms(*output, Some(*input), visited, arms)?;
                    }
                    return Ok(());
                }
                let input_arms = self.trigger_arms_before(*input, terminal)?;
                if !input_arms.is_empty() {
                    // Exact THEN identity rule: when no output expression is
                    // present, the gated input is itself the arm output.
                    let arm_output = output.unwrap_or(*input);
                    for input_arm in input_arms {
                        arms.insert(self.arm(input_arm.cause, *input, arm_output)?);
                    }
                    return Ok(());
                }
                if let Some(output) = output {
                    self.collect_trigger_arms(*output, terminal, visited, arms)?;
                }
            }
            SemanticExpressionKind::Hold { updates, .. }
            | SemanticExpressionKind::Latest { branches: updates } => {
                for update in updates {
                    let mut update_arms = BTreeSet::new();
                    self.collect_trigger_arms(
                        *update,
                        terminal,
                        &mut BTreeSet::new(),
                        &mut update_arms,
                    )?;
                    if update_arms.is_empty() {
                        for cause in self.event_causes_for_expression(*update)? {
                            arms.insert(self.arm(cause, *update, *update)?);
                        }
                    } else {
                        arms.extend(update_arms);
                    }
                }
            }
            SemanticExpressionKind::Call {
                intrinsic: Some(CheckedIntrinsicV1::StreamPulses),
                ..
            } => {
                for cause in self.direct_causes(expression)? {
                    arms.insert(self.arm(cause, id, id)?);
                }
            }
            SemanticExpressionKind::Call { arguments, .. } => {
                for argument in arguments {
                    let mut argument_arms = BTreeSet::new();
                    self.collect_trigger_arms(
                        argument.value,
                        terminal,
                        &mut BTreeSet::new(),
                        &mut argument_arms,
                    )?;
                    if argument_arms.is_empty()
                        && matches!(
                            self.execution.expression(argument.value)?.flow_type.mode,
                            FlowMode::TickPresent | FlowMode::PresentOrAbsent
                        )
                    {
                        for cause in self.event_causes_for_expression(argument.value)? {
                            arms.insert(self.arm(cause, argument.value, id)?);
                        }
                    } else {
                        for argument_arm in argument_arms {
                            arms.insert(self.arm(
                                argument_arm.cause,
                                argument_arm.gate_expression,
                                id,
                            )?);
                        }
                    }
                }
            }
            SemanticExpressionKind::Materialize { materialization } => {
                let materialization = self
                    .execution
                    .materializations
                    .get(materialization.as_usize())
                    .filter(|candidate| candidate.id == *materialization)
                    .ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "trigger traversal references missing materialization {}",
                            materialization
                        ))
                    })?;
                self.collect_trigger_arms(materialization.body, terminal, visited, arms)?;
            }
            SemanticExpressionKind::CanonicalRead {
                target, projection, ..
            } => {
                if !self.direct_causes(expression)?.is_empty() {
                    for cause in self.direct_causes(expression)? {
                        arms.insert(self.arm(cause, id, id)?);
                    }
                    return Ok(());
                }
                let producer = self.exact_binding_for_decl(*target, expression)?.producer;
                let producer_arms = self.trigger_arms_before(producer, terminal)?;
                if producer_arms.is_empty() {
                    for cause in self.event_causes_for_expression(producer)? {
                        arms.insert(self.arm(
                            cause,
                            producer,
                            if projection.is_empty() { producer } else { id },
                        )?);
                    }
                } else {
                    for producer_arm in producer_arms {
                        arms.insert(self.arm(
                            producer_arm.cause,
                            producer_arm.gate_expression,
                            if projection.is_empty() {
                                producer_arm.output_expression
                            } else {
                                id
                            },
                        )?);
                    }
                }
            }
            SemanticExpressionKind::LocalRead {
                binding,
                projection,
                ..
            } => {
                let (_, producer) = self.local_values.get(binding).copied().ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "trigger traversal references missing local binding {}",
                        binding
                    ))
                })?;
                let producer_arms = self.trigger_arms_before(producer, terminal)?;
                if producer_arms.is_empty() {
                    for cause in self.event_causes_for_expression(producer)? {
                        arms.insert(self.arm(
                            cause,
                            producer,
                            if projection.is_empty() { producer } else { id },
                        )?);
                    }
                } else {
                    for producer_arm in producer_arms {
                        arms.insert(self.arm(
                            producer_arm.cause,
                            producer_arm.gate_expression,
                            if projection.is_empty() {
                                producer_arm.output_expression
                            } else {
                                id
                            },
                        )?);
                    }
                }
            }
            SemanticExpressionKind::FunctionParameter { parameter, .. } => {
                let causes = self.event_causes_for_expression(id)?;
                if causes.is_empty() && !self.parameter_inputs.contains_key(&id) {
                    return Err(SemanticReactiveError::new(format!(
                        "trigger traversal parameter expression {} ({:?}) has no exact producer input inventory",
                        id, parameter
                    )));
                }
                for cause in causes {
                    arms.insert(self.arm(cause, id, id)?);
                }
            }
            _ => {
                for child in semantic_expression_children(&expression.kind, self.execution)? {
                    self.collect_trigger_arms(child, terminal, visited, arms)?;
                }
            }
        }
        Ok(())
    }

    fn trigger_arms_before(
        &mut self,
        root: SemanticExprId,
        terminal: Option<SemanticExprId>,
    ) -> Result<Vec<RawTriggerArm>, SemanticReactiveError> {
        let mut arms = BTreeSet::new();
        self.collect_trigger_arms(root, terminal, &mut BTreeSet::new(), &mut arms)?;
        Ok(arms.into_iter().collect())
    }

    fn trigger_arms_before_expression(
        &mut self,
        root: SemanticExprId,
        terminal: SemanticExprId,
    ) -> Result<Vec<RawTriggerArm>, SemanticReactiveError> {
        self.trigger_arms_before(root, Some(terminal))
    }

    fn arm(
        &self,
        cause: SemanticEventCauseV1,
        gate: SemanticExprId,
        output: SemanticExprId,
    ) -> Result<RawTriggerArm, SemanticReactiveError> {
        let gate_expression = self.execution.expression(gate)?;
        Ok(RawTriggerArm {
            cause,
            gate_checked_expression: gate_expression.checked_expr_id,
            gate_expression: gate,
            gate_value: gate_expression.value_id,
            owner: gate_expression.owner,
            route_scope: self.execution.route_scope(gate)?,
            row_scope: self.row_scope(cause, gate_expression)?,
            output_expression: output,
            output_value: self.execution.value(output)?,
        })
    }

    fn row_scope(
        &self,
        cause: SemanticEventCauseV1,
        gate: &SemanticExpression,
    ) -> Result<Option<SemanticRowScopeId>, SemanticReactiveError> {
        let cause_scope = match cause {
            SemanticEventCauseV1::Source(source) => {
                self.resources
                    .sources
                    .get(source.as_usize())
                    .filter(|candidate| candidate.id == source)
                    .ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "trigger cause references missing source {}",
                            source
                        ))
                    })?
                    .row_scope
            }
            SemanticEventCauseV1::State(state) => {
                self.resources
                    .states
                    .get(state.as_usize())
                    .filter(|candidate| candidate.id == state)
                    .ok_or_else(|| {
                        SemanticReactiveError::new(format!(
                            "trigger cause references missing state {}",
                            state
                        ))
                    })?
                    .row_scope
            }
            SemanticEventCauseV1::Pulse(pulse) => self
                .pulse_states
                .get(&pulse)
                .and_then(|state| {
                    self.resources
                        .states
                        .get(state.as_usize())
                        .filter(|candidate| candidate.id == *state)
                })
                .and_then(|state| state.row_scope),
            SemanticEventCauseV1::ExternalRead(_) => None,
        };
        if cause_scope.is_some() {
            return Ok(cause_scope);
        }
        let local_scopes = gate
            .provenance
            .members
            .iter()
            .filter_map(|member| match member.origin {
                SemanticValueOrigin::MaterializationLocal { owner, local, .. } => {
                    Some((owner, local))
                }
                _ => None,
            })
            .flat_map(|(owner, local)| {
                self.execution
                    .materializations
                    .iter()
                    .filter(move |materialization| {
                        materialization.owner == owner && materialization.row_local == local
                    })
                    .filter_map(|materialization| {
                        materialization
                            .source_scope_id
                            .or(materialization.target_scope_id)
                    })
            })
            .collect::<BTreeSet<_>>();
        match local_scopes.len() {
            0 => Ok(None),
            1 => Ok(local_scopes.iter().next().copied()),
            count => Err(SemanticReactiveError::new(format!(
                "trigger gate {} has {count} exact materialization row scopes",
                gate.id
            ))),
        }
    }

    fn exact_binding_for_decl(
        &self,
        declaration: DeclId,
        expression: &SemanticExpression,
    ) -> Result<&SemanticBindingV1, SemanticReactiveError> {
        let origin = self.execution.origin(expression.id)?;
        lexical_binding_for_decl(
            self.execution,
            self.resources,
            self.out_net,
            self.bindings,
            declaration,
            expression,
            origin.call_instance,
            "reactive expression",
        )
    }
}

fn semantic_expression_children(
    kind: &SemanticExpressionKind,
    execution: &SemanticExecutionGraphV1,
) -> Result<Vec<SemanticExprId>, SemanticReactiveError> {
    execution.expression_children(kind).ok_or_else(|| {
        let SemanticExpressionKind::Materialize { materialization } = kind else {
            unreachable!("only invalid materialization references lack expression children");
        };
        SemanticReactiveError::new(format!(
            "expression references missing semantic materialization {materialization}"
        ))
    })
}

fn exact_call_argument_at_ordinal(
    execution: &SemanticExecutionGraphV1,
    callable: crate::SemanticCallableId,
    arguments: &[crate::SemanticCallArgument],
    ordinal: usize,
    expression: SemanticExprId,
) -> Result<SemanticExprId, SemanticReactiveError> {
    let callable = execution.callable(callable)?;
    let parameter = callable
        .parameters
        .get(ordinal)
        .filter(|parameter| {
            parameter.id.callable == callable.id && parameter.ordinal == ordinal
        })
        .ok_or_else(|| {
            SemanticReactiveError::new(format!(
                "semantic call expression {expression} callable {} has no exact parameter ordinal {ordinal}",
                callable.id
            ))
        })?;
    let matches = arguments
        .iter()
        .filter(|argument| argument.ordinal == ordinal && argument.formal == parameter.formal)
        .map(|argument| argument.value)
        .collect::<Vec<_>>();
    let [value] = matches.as_slice() else {
        return Err(SemanticReactiveError::new(format!(
            "semantic call expression {expression} has {} exact bindings for callable {} parameter ordinal {ordinal}",
            matches.len(),
            callable.id,
        )));
    };
    Ok(*value)
}

fn exact_unique_list_argument(
    execution: &SemanticExecutionGraphV1,
    callable: crate::SemanticCallableId,
    arguments: &[crate::SemanticCallArgument],
    expression: SemanticExprId,
) -> Result<Option<SemanticExprId>, SemanticReactiveError> {
    let mut list_arguments = Vec::new();
    for argument in arguments {
        let exact = exact_call_argument_at_ordinal(
            execution,
            callable,
            arguments,
            argument.ordinal,
            expression,
        )?;
        if matches!(execution.expression(exact)?.flow_type.ty, Type::List(_)) {
            list_arguments.push((argument.ordinal, exact));
        }
    }
    list_arguments.sort();
    match list_arguments.as_slice() {
        [] => Ok(None),
        [(_, list)] => Ok(Some(*list)),
        _ => Err(SemanticReactiveError::new(format!(
            "semantic call expression {expression} has {} exact list-typed input formals",
            list_arguments.len()
        ))),
    }
}

fn unique_binding_for_target<'a>(
    bindings: &'a [SemanticBindingV1],
    target: SemanticBindingTargetV1,
    expression: &SemanticExpression,
    execution: &SemanticExecutionGraphV1,
) -> Result<&'a SemanticBindingV1, SemanticReactiveError> {
    let origin = execution.origin(expression.id)?;
    let matches = bindings
        .iter()
        .filter(|binding| binding.target == target)
        .collect::<Vec<_>>();
    let [binding] = matches.as_slice() else {
        return Err(SemanticReactiveError::new(format!(
            "semantic read {} at owner {:?}, frame {:?} resolves occurrence target {:?} to {} exact bindings",
            expression.id,
            expression.owner,
            origin.call_instance,
            target,
            matches.len()
        )));
    };
    Ok(*binding)
}

fn require_trigger(
    triggers: &[SemanticTriggerOwnedArmV1],
    id: SemanticTriggerArmId,
) -> Result<&SemanticTriggerOwnedArmV1, SemanticReactiveError> {
    triggers
        .get(id.as_usize())
        .filter(|trigger| trigger.id == id)
        .ok_or_else(|| {
            SemanticReactiveError::new(format!(
                "reactive schedule references missing trigger arm {}",
                id
            ))
        })
}

fn materialization_local_read_scope(
    read: &SemanticReadBindingV1,
    execution: &SemanticExecutionGraphV1,
) -> Result<Option<SemanticRowScopeId>, SemanticReactiveError> {
    let SemanticReadTargetV1::MaterializationLocal { owner, local, .. } = read.target else {
        return Ok(None);
    };
    let materializations = execution
        .materializations
        .iter()
        .filter(|materialization| {
            materialization.owner == owner && materialization.row_local == local
        })
        .collect::<Vec<_>>();
    let [materialization] = materializations.as_slice() else {
        return Err(SemanticReactiveError::new(format!(
            "semantic read {} materialization local {owner}:{local:?} resolves to {} exact materializations",
            read.id,
            materializations.len()
        )));
    };
    Ok(materialization.source_scope_id)
}

fn trigger_row_scope(
    causes: &BTreeSet<SemanticEventCauseV1>,
    resources: &SemanticResourceGraphV1,
    pulse_states: &BTreeMap<SemanticPulseBatchId, SemanticStateId>,
) -> Result<Option<SemanticRowScopeId>, SemanticReactiveError> {
    let scopes = causes
        .iter()
        .filter_map(|cause| match cause {
            SemanticEventCauseV1::Source(source) => resources
                .sources
                .get(source.as_usize())
                .filter(|candidate| candidate.id == *source)
                .and_then(|source| source.row_scope),
            SemanticEventCauseV1::State(state) => resources
                .states
                .get(state.as_usize())
                .filter(|candidate| candidate.id == *state)
                .and_then(|state| state.row_scope),
            SemanticEventCauseV1::Pulse(pulse) => pulse_states
                .get(pulse)
                .and_then(|state| {
                    resources
                        .states
                        .get(state.as_usize())
                        .filter(|candidate| candidate.id == *state)
                })
                .and_then(|state| state.row_scope),
            SemanticEventCauseV1::ExternalRead(_) => None,
        })
        .collect::<BTreeSet<_>>();
    match scopes.len() {
        0 => Ok(None),
        1 => Ok(scopes.iter().next().copied()),
        count => Err(SemanticReactiveError::new(format!(
            "view capture has {count} exact row scopes"
        ))),
    }
}

fn validate_semantic_reactive_shape(
    graph: &SemanticReactiveGraphV1,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
) -> Result<(), SemanticReactiveError> {
    if graph.schema != SEMANTIC_REACTIVE_GRAPH_SCHEMA_V1 {
        return Err(SemanticReactiveError::new(format!(
            "unsupported semantic reactive graph schema `{}`",
            graph.schema
        )));
    }
    validate_dense("field", &graph.fields, |field| field.id.as_usize())?;
    validate_dense("binding", &graph.bindings, |binding| binding.id.as_usize())?;
    validate_dense("read", &graph.reads, |read| read.id.as_usize())?;
    validate_dense("dependency use", &graph.dependency_uses, |use_| {
        use_.id.as_usize()
    })?;
    validate_dense("derived value", &graph.derived_values, |derived| {
        derived.id.as_usize()
    })?;
    validate_dense("trigger arm", &graph.trigger_arms, |trigger| {
        trigger.id.as_usize()
    })?;
    validate_dense("state update arm", &graph.state_update_arms, |arm| {
        arm.id.as_usize()
    })?;
    validate_dense("list mutation", &graph.list_mutations, |mutation| {
        mutation.id.as_usize()
    })?;
    validate_dense("dependency", &graph.dependencies, |dependency| {
        dependency.id.as_usize()
    })?;
    validate_dense(
        "host effect schedule",
        &graph.host_effect_schedules,
        |schedule| schedule.id.as_usize(),
    )?;
    validate_dense("view capture", &graph.view_captures, |capture| {
        capture.id.as_usize()
    })?;
    validate_dense("migration input", &graph.migration_inputs, |input| {
        input.id.as_usize()
    })?;

    let mut previous_call = None;
    for schedule in &graph.call_invocations {
        if previous_call.is_some_and(|previous| previous >= schedule.expression) {
            return Err(SemanticReactiveError::new(
                "semantic call invocation schedules are not strictly expression-ordered",
            ));
        }
        previous_call = Some(schedule.expression);
        let expression = execution
            .expressions
            .get(schedule.expression.as_usize())
            .filter(|expression| expression.id == schedule.expression)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "call invocation schedule references missing expression {}",
                    schedule.expression
                ))
            })?;
        if expression.value_id != schedule.value
            || !matches!(
                expression.kind,
                SemanticExpressionKind::Call { call, .. } if call == schedule.call
            )
        {
            return Err(SemanticReactiveError::new(format!(
                "call invocation schedule for expression {} has stale call/value provenance",
                schedule.expression
            )));
        }
        for binding in &schedule.dependent_bindings {
            if graph
                .bindings
                .get(binding.as_usize())
                .filter(|candidate| candidate.id == *binding)
                .is_none()
            {
                return Err(SemanticReactiveError::new(format!(
                    "call invocation schedule for expression {} references missing binding {}",
                    schedule.expression, binding
                )));
            }
        }
        for trigger in &schedule.invocation_arms {
            require_trigger(&graph.trigger_arms, *trigger)?;
        }
    }

    for trigger in &graph.trigger_arms {
        let gate = execution
            .expressions
            .get(trigger.gate_expression.as_usize())
            .filter(|expression| expression.id == trigger.gate_expression)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "trigger {} references missing gate expression {}",
                    trigger.id, trigger.gate_expression
                ))
            })?;
        let output = execution
            .expressions
            .get(trigger.output_expression.as_usize())
            .filter(|expression| expression.id == trigger.output_expression)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "trigger {} references missing output expression {}",
                    trigger.id, trigger.output_expression
                ))
            })?;
        if trigger.gate_value != gate.value_id
            || trigger.gate_checked_expression != gate.checked_expr_id
            || trigger.output_value != output.value_id
            || trigger.owner != gate.owner
        {
            return Err(SemanticReactiveError::new(format!(
                "trigger {} has stale gate/output provenance",
                trigger.id
            )));
        }
    }
    for arm in &graph.state_update_arms {
        if resources
            .states
            .get(arm.state.as_usize())
            .filter(|state| state.id == arm.state)
            .is_none()
        {
            return Err(SemanticReactiveError::new(format!(
                "state update arm {} references missing state {}",
                arm.id, arm.state
            )));
        }
        require_trigger(&graph.trigger_arms, arm.trigger)?;
    }
    for schedule in &graph.host_effect_schedules {
        if schedule.state_update_arms.is_empty() == schedule.transient_result.is_none() {
            return Err(SemanticReactiveError::new(format!(
                "host effect schedule {} must have exactly one retained-state or transient-result owner",
                schedule.id
            )));
        }
        for arm in &schedule.state_update_arms {
            if graph
                .state_update_arms
                .get(arm.as_usize())
                .filter(|candidate| candidate.id == *arm)
                .is_none()
            {
                return Err(SemanticReactiveError::new(format!(
                    "host effect schedule {} references missing state update arm {}",
                    schedule.id, arm
                )));
            }
        }
        if let Some(derived) = schedule.transient_result {
            let derived = graph
                .derived_values
                .get(derived.as_usize())
                .filter(|candidate| candidate.id == derived)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "host effect schedule {} references missing transient derived value {}",
                        schedule.id, derived
                    ))
                })?;
            if derived.trigger_arms.is_empty() {
                return Err(SemanticReactiveError::new(format!(
                    "host effect schedule {} transient result has no exact trigger arms",
                    schedule.id
                )));
            }
        }
    }
    for (index, causes) in graph.possible_causes.iter().enumerate() {
        let expected = SemanticStateId(index);
        if causes.state != expected {
            return Err(SemanticReactiveError::new(format!(
                "possible-causes entry at index {index} covers {}, expected {expected}",
                causes.state
            )));
        }
    }
    if graph.possible_causes.len() != resources.states.len() {
        return Err(SemanticReactiveError::new(format!(
            "possible-causes table covers {} states, expected {}",
            graph.possible_causes.len(),
            resources.states.len()
        )));
    }
    Ok(())
}

fn validate_dense<T>(
    label: &str,
    values: &[T],
    id: impl Fn(&T) -> usize,
) -> Result<(), SemanticReactiveError> {
    for (index, value) in values.iter().enumerate() {
        if id(value) != index {
            return Err(SemanticReactiveError::new(format!(
                "semantic reactive {label} at index {index} has non-dense ID {}",
                id(value)
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        SemanticExpressionOrigin, SemanticScope, SemanticSourceRead, SemanticStaticOwner,
        SemanticValueMember, SemanticValueProvenance,
    };
    use boon_typecheck::{
        CheckedCallableKind, CheckedEffectSummary, CheckedEvaluationScope, CheckedExprId,
        CheckedParameterKind, CheckedParameterRequirement, CheckedScopeKind, CheckedSpan, FlowMode,
        LexicalScopeId, ProgramRole, Type,
    };

    fn flow(mode: FlowMode) -> FlowType {
        FlowType {
            ty: Type::Text,
            mode,
        }
    }

    fn source_expression(
        id: usize,
        source: usize,
        owner: Option<StaticOwnerId>,
    ) -> SemanticExpression {
        SemanticExpression {
            id: SemanticExprId(id),
            value_id: SemanticValueId(id),
            checked_expr_id: CheckedExprId(id as u32),
            flow_type: flow(FlowMode::TickPresent),
            effect: CheckedEffectSummary::default(),
            owner,
            provenance: SemanticValueProvenance {
                members: vec![SemanticValueMember {
                    path: Vec::new(),
                    origin: SemanticValueOrigin::Source {
                        source: SemanticSourceId(source),
                        owner,
                    },
                }],
            },
            resource_binding_path: None,
            kind: SemanticExpressionKind::CanonicalRead {
                target: DeclId(source as u32),
                path: format!("source_{source}"),
                projection: Vec::new(),
                source: Some(SemanticSourceRead {
                    source: SemanticSourceId(source),
                    payload_projection: Vec::new(),
                }),
            },
        }
    }

    fn graph(expressions: Vec<SemanticExpression>) -> SemanticExecutionGraphV1 {
        let origins = expressions
            .iter()
            .map(|expression| SemanticExpressionOrigin {
                expression: expression.id,
                checked_expression: expression.checked_expr_id,
                checked_scope: LexicalScopeId(0),
                checked_span: CheckedSpan {
                    line: 1,
                    start: expression.id.as_usize(),
                    end: expression.id.as_usize() + 1,
                },
                owning_statement: None,
                call_instance: None,
            })
            .collect();
        SemanticExecutionGraphV1 {
            expressions,
            scopes: vec![SemanticScope {
                id: SemanticScopeId(0),
                checked_scope: LexicalScopeId(0),
                parent: None,
                owner: None,
                kind: CheckedScopeKind::Root,
                span: CheckedSpan {
                    line: 1,
                    start: 0,
                    end: 1,
                },
            }],
            static_owners: Vec::<SemanticStaticOwner>::new(),
            checked_expression_origins: origins,
            ..SemanticExecutionGraphV1::default()
        }
    }

    fn append_callable() -> crate::SemanticCallable {
        crate::SemanticCallable {
            id: crate::SemanticCallableId(0),
            checked_callable: DeclId(20),
            scope: SemanticScopeId(0),
            kind: CheckedCallableKind::Builtin,
            name: "diagnostic-alias".to_owned(),
            external_identity: None,
            parameters: vec![
                crate::SemanticCallableParameter {
                    id: SemanticParameterId {
                        callable: crate::SemanticCallableId(0),
                        ordinal: 0,
                    },
                    formal: DeclId(10),
                    ordinal: 0,
                    name: "renamed-list".to_owned(),
                    kind: CheckedParameterKind::Value,
                    flow_type: FlowType {
                        ty: Type::List(Box::new(Type::Text)),
                        mode: FlowMode::Continuous,
                    },
                    requirement: CheckedParameterRequirement::Required,
                    evaluation_scope: CheckedEvaluationScope::Parent,
                    start: 0,
                    end: 1,
                },
                crate::SemanticCallableParameter {
                    id: SemanticParameterId {
                        callable: crate::SemanticCallableId(0),
                        ordinal: 1,
                    },
                    formal: DeclId(11),
                    ordinal: 1,
                    name: "renamed-item".to_owned(),
                    kind: CheckedParameterKind::Value,
                    flow_type: flow(FlowMode::Continuous),
                    requirement: CheckedParameterRequirement::Required,
                    evaluation_scope: CheckedEvaluationScope::Parent,
                    start: 2,
                    end: 3,
                },
            ],
            contexts: Vec::new(),
            context_formal: None,
            result: FlowType {
                ty: Type::List(Box::new(Type::Text)),
                mode: FlowMode::Continuous,
            },
            role: ProgramRole::Client,
            effect: CheckedEffectSummary::default(),
            body: None,
            result_expression: None,
            contextual_operation: None,
        }
    }

    #[test]
    fn route_scope_rejects_ambiguous_semantic_scope() {
        let mut execution = graph(vec![source_expression(0, 0, None)]);
        execution.scopes.push(SemanticScope {
            id: SemanticScopeId(1),
            checked_scope: LexicalScopeId(0),
            parent: None,
            owner: None,
            kind: CheckedScopeKind::Root,
            span: CheckedSpan {
                line: 1,
                start: 0,
                end: 1,
            },
        });
        let error = execution.route_scope(SemanticExprId(0)).unwrap_err();
        assert!(error.to_string().contains("resolves to 2 semantic scopes"));
    }

    #[test]
    fn call_argument_identity_uses_formal_and_ordinal_not_diagnostic_name() {
        let execution = SemanticExecutionGraphV1 {
            callables: vec![append_callable()],
            ..SemanticExecutionGraphV1::default()
        };
        let mut arguments = vec![
            crate::SemanticCallArgument {
                formal: DeclId(10),
                ordinal: 0,
                name: "not-list".to_owned(),
                checked_value: CheckedExprId(0),
                value: SemanticExprId(0),
                from_pipe: true,
            },
            crate::SemanticCallArgument {
                formal: DeclId(11),
                ordinal: 1,
                name: "not-item".to_owned(),
                checked_value: CheckedExprId(1),
                value: SemanticExprId(1),
                from_pipe: false,
            },
        ];
        assert_eq!(
            exact_call_argument_at_ordinal(
                &execution,
                crate::SemanticCallableId(0),
                &arguments,
                0,
                SemanticExprId(2),
            )
            .unwrap(),
            SemanticExprId(0)
        );

        arguments[0].formal = DeclId(99);
        let error = exact_call_argument_at_ordinal(
            &execution,
            crate::SemanticCallableId(0),
            &arguments,
            0,
            SemanticExprId(2),
        )
        .unwrap_err();
        assert!(error.to_string().contains("0 exact bindings"));
    }
}
