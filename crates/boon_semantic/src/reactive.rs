//! Pre-backend reactive ownership and scheduling.
//!
//! This module deliberately consumes only the normalized semantic execution
//! graph, the semantic resource graph, and the resolved `OUT` graph.  It must
//! not reinterpret checked coordinates as executable identity.  Checked
//! expression IDs retained below are audit provenance for exact gates only.

use crate::{
    ProducerFunctionId, ProducerMaterializationMode, ResolvedOutGraph, SemanticBindingId,
    SemanticCallId, SemanticCaptureId, SemanticContextualOperationKind, SemanticDependencyUseId,
    SemanticDerivedValueId, SemanticExecutionGraphV1, SemanticExprId, SemanticExpression,
    SemanticExpressionKind, SemanticExternalDependencyId, SemanticFieldId,
    SemanticHostEffectScheduleId, SemanticListId, SemanticListMutationId,
    SemanticListResourceOriginV1, SemanticLocalBindingId, SemanticMaterializationId,
    SemanticMaterializationLocalId, SemanticParameterId, SemanticReadId, SemanticResourceGraphV1,
    SemanticRootKindV1, SemanticRowBinding, SemanticRowScopeId, SemanticScopeId, SemanticSourceId,
    SemanticStateId, SemanticStateUpdateArmId, SemanticStatementId, SemanticStatementKind,
    SemanticTriggerArmId, SemanticValueId, SemanticValueOrigin, StaticOwnerId,
};
use boon_typecheck::{
    CheckedExprId, CheckedExternalDeclarationIdentityV1, DeclId, FlowMode, FlowType, Type,
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
    execution
        .validate(out_net)
        .map_err(SemanticReactiveError::new)?;
    resources
        .validate(execution, out_net)
        .map_err(SemanticReactiveError::new)?;
    let graph = ReactiveBuilder::new(execution, resources, out_net)?.build()?;
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
        let expected = build_semantic_reactive_graph(execution, resources, out_net)?;
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

struct ReactiveBuilder<'a> {
    execution: &'a SemanticExecutionGraphV1,
    resources: &'a SemanticResourceGraphV1,
    out_net: &'a ResolvedOutGraph,
    expressions: ExpressionIndex<'a>,
    statement_values: BTreeMap<DeclId, Vec<SemanticStatementId>>,
    local_values: BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
    parameter_inputs: BTreeMap<SemanticParameterId, Vec<SemanticExprId>>,
}

impl<'a> ReactiveBuilder<'a> {
    fn new(
        execution: &'a SemanticExecutionGraphV1,
        resources: &'a SemanticResourceGraphV1,
        out_net: &'a ResolvedOutGraph,
    ) -> Result<Self, SemanticReactiveError> {
        let expressions = ExpressionIndex::new(execution)?;
        let mut statement_values = BTreeMap::<DeclId, Vec<SemanticStatementId>>::new();
        for statement in &execution.statements {
            if statement.value.is_some()
                && let Some(declaration) = statement.declaration
            {
                statement_values
                    .entry(declaration)
                    .or_default()
                    .push(statement.id);
            }
        }
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
            for parameter in &function.parameters {
                if parameter_inputs
                    .insert(parameter.id, parameter.input_expressions.clone())
                    .is_some()
                {
                    return Err(SemanticReactiveError::new(format!(
                        "semantic parameter {:?} has multiple producer-input inventories",
                        parameter.id
                    )));
                }
            }
        }
        Ok(Self {
            execution,
            resources,
            out_net,
            expressions,
            statement_values,
            local_values,
            parameter_inputs,
        })
    }

    fn build(self) -> Result<SemanticReactiveGraphV1, SemanticReactiveError> {
        let producer_instances = self.build_producer_instances()?;
        let fields = self.build_fields()?;
        let bindings = self.build_bindings(&fields)?;
        let reads = self.build_reads(&bindings)?;
        let mut triggers = TriggerResolver::new(
            self.execution,
            self.resources,
            &self.expressions,
            &bindings,
            &self.statement_values,
            &self.local_values,
            &self.parameter_inputs,
        )?;

        let raw_state_arms = self.build_state_update_arms(&mut triggers)?;
        let raw_mutations = self.build_list_mutations(&mut triggers)?;
        let raw_derived = self.build_raw_derived_values(&bindings, &mut triggers)?;
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

        let dependencies = self.build_dependencies(&state_update_arms, &trigger_arms)?;
        let possible_causes = self.build_possible_causes(&state_update_arms, &trigger_arms)?;
        let host_effect_schedules =
            self.build_host_effect_schedules(&state_update_arms, &trigger_arms)?;
        let dependency_uses = self.build_dependency_uses(&bindings, &reads, &mut triggers)?;
        let output_values = self.build_output_values(&fields)?;
        let view_captures =
            self.build_view_captures(&output_values, &fields, &reads, &mut triggers)?;
        let migration_inputs = self.build_migration_inputs()?;

        Ok(SemanticReactiveGraphV1 {
            schema: SEMANTIC_REACTIVE_GRAPH_SCHEMA_V1.to_owned(),
            producer_instances,
            fields,
            bindings,
            reads,
            dependency_uses,
            call_invocations,
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
                        .map(|expression| self.expressions.value(*expression))
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
                root_value: self.expressions.value(root_expression)?,
                mode: resource.mode,
                invocation_source,
                parameters,
            });
        }
        result.sort_by_key(|instance| (instance.function, instance.identity));
        Ok(result)
    }

    fn build_fields(&self) -> Result<Vec<SemanticFieldV1>, SemanticReactiveError> {
        let source_statements = self
            .resources
            .sources
            .iter()
            .map(|source| source.statement)
            .collect::<BTreeSet<_>>();
        let mut candidates = Vec::new();
        for statement in &self.execution.statements {
            let Some(declaration) = statement.declaration else {
                continue;
            };
            let Some(producer) = statement.value else {
                continue;
            };
            if source_statements.contains(&statement.id)
                && !matches!(
                    &statement.origin,
                    crate::SemanticStatementOrigin::ProducerResult { .. }
                )
            {
                continue;
            }
            let exact = self.exact_field_metadata(statement.id);
            let Some((name, path, row)) = exact else {
                continue;
            };
            let expression = self.expressions.expression(producer)?;
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
            .map(|field| (field.statement, field.id))
            .collect::<BTreeMap<_, _>>();
        let source_by_statement = self
            .resources
            .sources
            .iter()
            .map(|source| (source.statement, source))
            .collect::<BTreeMap<_, _>>();
        let state_by_statement = self
            .resources
            .states
            .iter()
            .map(|state| (state.statement, state))
            .collect::<BTreeMap<_, _>>();
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
            let expression = self.expressions.expression(source.expression)?;
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
            let Some(producer) = statement.value else {
                continue;
            };
            let expression = self.expressions.expression(producer)?;
            let target = if let Some(state) = state_by_statement.get(&statement.id) {
                SemanticBindingTargetV1::State { state: state.id }
            } else if let Some(list) = list_by_statement.get(&statement.id) {
                SemanticBindingTargetV1::List { list: list.id }
            } else if let Some(field) = fields_by_statement.get(&statement.id) {
                SemanticBindingTargetV1::Field { field: *field }
            } else if source_by_statement.contains_key(&statement.id) {
                // The exact source-expression candidate was emitted above.
                continue;
            } else {
                continue;
            };
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
                    } else if let Some(state) = unique_state_origin(expression)? {
                        let binding = unique_binding_for_target(
                            bindings,
                            SemanticBindingTargetV1::State { state },
                            expression,
                            self.execution,
                        )?;
                        SemanticReadTargetV1::StateProjection {
                            binding: binding.id,
                            state,
                            projection: projection.clone(),
                        }
                    } else {
                        let binding = self.resolve_decl_binding(*target, expression, bindings)?;
                        SemanticReadTargetV1::Binding {
                            binding: binding.id,
                            projection: projection.clone(),
                        }
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
                        producer_value: self.expressions.value(producer)?,
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
        let origin = self.expressions.origin(expression.id)?;
        let matches = bindings
            .iter()
            .filter(|binding| {
                binding.declaration == declaration
                    && binding.owner == expression.owner
                    && binding.call_instance == origin.call_instance
            })
            .collect::<Vec<_>>();
        let [binding] = matches.as_slice() else {
            return Err(SemanticReactiveError::new(format!(
                "semantic canonical read {} resolves declaration {} to {} exact owner/frame bindings",
                expression.id,
                declaration.0,
                matches.len()
            )));
        };
        Ok(*binding)
    }

    fn build_state_update_arms(
        &self,
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<Vec<RawStateUpdateArm>, SemanticReactiveError> {
        let mut result = BTreeSet::new();
        for state in &self.resources.states {
            for trigger in triggers.trigger_arms_for_expression(state.expression)? {
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
                    SemanticEventCauseV1::Source(_) => false,
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
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<Vec<RawListMutation>, SemanticReactiveError> {
        let mut mutations = BTreeSet::new();
        for list in &self.resources.lists {
            self.collect_list_mutations(
                list.id,
                list.producer,
                &mut BTreeSet::new(),
                triggers,
                &mut mutations,
            )?;
        }
        Ok(mutations.into_iter().collect())
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
        let expression = self.expressions.expression(root)?;
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
                if let Some(input) = exact_unique_list_argument(
                    self.execution,
                    &self.expressions,
                    *callable,
                    arguments,
                    root,
                )? {
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
        bindings: &[SemanticBindingV1],
        triggers: &mut TriggerResolver<'_>,
    ) -> Result<Vec<RawDerivedValue>, SemanticReactiveError> {
        let fields = bindings
            .iter()
            .filter_map(|binding| match binding.target {
                SemanticBindingTargetV1::Field { field } => Some((binding, field)),
                SemanticBindingTargetV1::Source { .. }
                | SemanticBindingTargetV1::State { .. }
                | SemanticBindingTargetV1::List { .. } => None,
            })
            .collect::<Vec<_>>();
        let mut result = Vec::new();
        for (binding, field) in fields {
            let triggers_for_value = triggers.trigger_arms_for_expression(binding.producer)?;
            let causes = triggers_for_value
                .iter()
                .map(|arm| arm.cause)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let materialized = self
                .resources
                .lists
                .iter()
                .find(|list| {
                    matches!(
                        list.origin,
                        SemanticListResourceOriginV1::Derived { statement, .. }
                            if statement == binding.statement
                    )
                })
                .map(|list| (list.id, list.row_scope));
            let kind = if materialized.is_some() {
                SemanticDerivedValueKindV1::ListView
            } else if self.expression_is_aggregate(binding.producer)? {
                SemanticDerivedValueKindV1::Aggregate
            } else if !causes.is_empty() {
                SemanticDerivedValueKindV1::SourceEventTransform
            } else {
                SemanticDerivedValueKindV1::Pure
            };
            let default_values = match &self.expressions.expression(binding.producer)?.kind {
                SemanticExpressionKind::Latest { branches } => branches
                    .iter()
                    .filter(|branch| {
                        triggers
                            .event_causes_for_expression(**branch)
                            .is_ok_and(|causes| causes.is_empty())
                    })
                    .map(|branch| self.expressions.value(*branch))
                    .collect::<Result<Vec<_>, _>>()?,
                SemanticExpressionKind::Hold { initial, .. } => {
                    vec![self.expressions.value(*initial)?]
                }
                _ => Vec::new(),
            };
            let startup_recompute = causes.is_empty();
            result.push(RawDerivedValue {
                binding: binding.id,
                field,
                statement: binding.statement,
                producer: binding.producer,
                value: binding.value,
                kind,
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

    fn expression_is_aggregate(
        &self,
        expression: SemanticExprId,
    ) -> Result<bool, SemanticReactiveError> {
        let expression = self.expressions.expression(expression)?;
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
        state_arms: &[SemanticStateUpdateArmV1],
        triggers: &[SemanticTriggerOwnedArmV1],
    ) -> Result<Vec<SemanticHostEffectScheduleV1>, SemanticReactiveError> {
        let mut schedules = Vec::new();
        for expression in &self.execution.expressions {
            let SemanticExpressionKind::Call { call, function, .. } = &expression.kind else {
                continue;
            };
            if !boon_typecheck::is_typed_host_effect(function) {
                continue;
            }
            let mut covering = Vec::new();
            for arm in state_arms {
                let trigger = require_trigger(triggers, arm.trigger)?;
                if trigger.owner == expression.owner
                    && self.expression_reaches(trigger.output_expression, expression.id)?
                {
                    covering.push(arm.id);
                }
            }
            if covering.is_empty() {
                return Err(SemanticReactiveError::new(format!(
                    "typed host effect `{function}` at semantic expression {} owner {:?} has no exact state update schedule",
                    expression.id, expression.owner
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
                arguments,
                result,
                effect,
                ..
            } = &expression.kind
            else {
                continue;
            };
            let call_definition = self
                .execution
                .calls
                .get(call.as_usize())
                .filter(|candidate| candidate.id == *call)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "call expression {} references missing semantic call {}",
                        expression.id, call
                    ))
                })?;
            if call_definition.external_identity.is_none() {
                continue;
            }
            if expression.flow_type != *result || expression.effect != *effect {
                return Err(SemanticReactiveError::new(format!(
                    "external call expression {} has inconsistent normalized result/effect metadata",
                    expression.id
                )));
            }
            let current_capable = result.mode == FlowMode::Continuous
                && arguments.iter().all(|argument| {
                    self.expressions
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
                    "external call expression {} is eventful, stateful, or host-effectful but has no exact source/state invocation arm",
                    expression.id
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
                .event_causes_for_expression(binding.producer)?
                .into_iter()
                .collect::<Vec<_>>();
            for expression_id in reachable {
                let expression = self.expressions.expression(expression_id)?;
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
                    let semantic_call = self
                        .execution
                        .calls
                        .get(call.as_usize())
                        .filter(|candidate| candidate.id == call)
                        .ok_or_else(|| {
                            SemanticReactiveError::new(format!(
                                "expression {} references missing semantic call {}",
                                expression_id, call
                            ))
                        })?;
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
                let expression = self.expressions.expression(root.expression)?;
                let origin = self.expressions.origin(root.expression)?;
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
                    route_scope: self.expressions.route_scope(root.expression)?,
                })
            })
            .collect()
    }

    fn build_view_captures(
        &self,
        outputs: &[SemanticOutputValueV1],
        fields: &[SemanticFieldV1],
        reads: &[SemanticReadBindingV1],
        triggers: &mut TriggerResolver<'_>,
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
                    let target = match read.target {
                        SemanticReadTargetV1::SourcePayload { source, .. } => {
                            SemanticViewCaptureTargetV1::Source { source }
                        }
                        _ => SemanticViewCaptureTargetV1::Read { read: read.id },
                    };
                    raw.insert((
                        output.ordinal,
                        expression,
                        read.value,
                        target,
                        trigger_row_scope(
                            &triggers.event_causes_for_expression(expression)?,
                            self.resources,
                        )?,
                    ));
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
                input_value: self.expressions.value(input)?,
                owner: expression.owner,
                route_scope: self.expressions.route_scope(expression.id)?,
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
            let expression = self.expressions.expression(expression)?;
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
}

#[derive(Clone, Debug)]
struct RawDerivedValue {
    binding: SemanticBindingId,
    field: SemanticFieldId,
    statement: SemanticStatementId,
    producer: SemanticExprId,
    value: SemanticValueId,
    kind: SemanticDerivedValueKindV1,
    materialized_list: Option<SemanticListId>,
    materialized_row_scope: Option<SemanticRowScopeId>,
    causes: Vec<SemanticEventCauseV1>,
    trigger_arms: Vec<RawTriggerArm>,
    default_values: Vec<SemanticValueId>,
    startup_recompute: bool,
}

#[derive(Debug)]
struct ExpressionIndex<'a> {
    execution: &'a SemanticExecutionGraphV1,
}

impl<'a> ExpressionIndex<'a> {
    fn new(execution: &'a SemanticExecutionGraphV1) -> Result<Self, SemanticReactiveError> {
        for (index, expression) in execution.expressions.iter().enumerate() {
            if expression.id != SemanticExprId(index)
                || expression.value_id != SemanticValueId(index)
            {
                return Err(SemanticReactiveError::new(format!(
                    "semantic expression at index {index} does not have exact dense expression/value identity"
                )));
            }
        }
        if execution.checked_expression_origins.len() != execution.expressions.len() {
            return Err(SemanticReactiveError::new(format!(
                "semantic checked-expression origins cover {}, expected {} expressions",
                execution.checked_expression_origins.len(),
                execution.expressions.len()
            )));
        }
        Ok(Self { execution })
    }

    fn expression(
        &self,
        id: SemanticExprId,
    ) -> Result<&'a SemanticExpression, SemanticReactiveError> {
        self.execution
            .expressions
            .get(id.as_usize())
            .filter(|expression| expression.id == id)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "reactive derivation references missing semantic expression {id}"
                ))
            })
    }

    fn value(&self, id: SemanticExprId) -> Result<SemanticValueId, SemanticReactiveError> {
        Ok(self.expression(id)?.value_id)
    }

    fn origin(
        &self,
        id: SemanticExprId,
    ) -> Result<&'a crate::SemanticExpressionOrigin, SemanticReactiveError> {
        self.execution
            .checked_expression_origins
            .get(id.as_usize())
            .filter(|origin| origin.expression == id)
            .ok_or_else(|| {
                SemanticReactiveError::new(format!(
                    "semantic expression {id} has no exact checked-origin entry"
                ))
            })
    }

    fn route_scope(&self, id: SemanticExprId) -> Result<SemanticScopeId, SemanticReactiveError> {
        let origin = self.origin(id)?;
        let matches = self
            .execution
            .scopes
            .iter()
            .filter(|scope| scope.checked_scope == origin.checked_scope)
            .map(|scope| scope.id)
            .collect::<Vec<_>>();
        let [scope] = matches.as_slice() else {
            return Err(SemanticReactiveError::new(format!(
                "semantic expression {id} checked scope {} resolves to {} semantic scopes",
                origin.checked_scope.0,
                matches.len()
            )));
        };
        Ok(*scope)
    }
}

struct TriggerResolver<'a> {
    execution: &'a SemanticExecutionGraphV1,
    resources: &'a SemanticResourceGraphV1,
    expressions: &'a ExpressionIndex<'a>,
    bindings: &'a [SemanticBindingV1],
    statement_values: &'a BTreeMap<DeclId, Vec<SemanticStatementId>>,
    local_values: &'a BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
    parameter_inputs: &'a BTreeMap<SemanticParameterId, Vec<SemanticExprId>>,
    causes_cache: BTreeMap<SemanticExprId, BTreeSet<SemanticEventCauseV1>>,
}

impl<'a> TriggerResolver<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        execution: &'a SemanticExecutionGraphV1,
        resources: &'a SemanticResourceGraphV1,
        expressions: &'a ExpressionIndex<'a>,
        bindings: &'a [SemanticBindingV1],
        statement_values: &'a BTreeMap<DeclId, Vec<SemanticStatementId>>,
        local_values: &'a BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
        parameter_inputs: &'a BTreeMap<SemanticParameterId, Vec<SemanticExprId>>,
    ) -> Result<Self, SemanticReactiveError> {
        for source in &resources.sources {
            if execution
                .sources
                .get(source.id.as_usize())
                .filter(|candidate| candidate.id == source.id)
                .is_none()
            {
                return Err(SemanticReactiveError::new(format!(
                    "reactive source resource references missing semantic source {}",
                    source.id
                )));
            }
        }
        for state in &resources.states {
            if execution
                .states
                .get(state.id.as_usize())
                .filter(|candidate| candidate.id == state.id)
                .is_none()
            {
                return Err(SemanticReactiveError::new(format!(
                    "reactive state resource references missing semantic state {}",
                    state.id
                )));
            }
        }
        Ok(Self {
            execution,
            resources,
            expressions,
            bindings,
            statement_values,
            local_values,
            parameter_inputs,
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
        self.collect_event_causes(root, &mut BTreeSet::new(), &mut causes)?;
        self.causes_cache.insert(root, causes.clone());
        Ok(causes)
    }

    fn collect_event_causes(
        &mut self,
        id: SemanticExprId,
        visited: &mut BTreeSet<SemanticExprId>,
        causes: &mut BTreeSet<SemanticEventCauseV1>,
    ) -> Result<(), SemanticReactiveError> {
        if !visited.insert(id) {
            return Ok(());
        }
        let expression = self.expressions.expression(id)?;
        let direct = self.direct_causes(expression)?;
        if !direct.is_empty() {
            causes.extend(direct);
            return Ok(());
        }
        match &expression.kind {
            SemanticExpressionKind::CanonicalRead { target, .. } => {
                let binding = self.exact_binding_for_decl(*target, expression)?;
                self.collect_event_causes(binding.producer, visited, causes)?;
            }
            SemanticExpressionKind::LocalRead { binding, .. } => {
                let (_, producer) = self.local_values.get(binding).copied().ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic local read {} references missing binding {}",
                        id, binding
                    ))
                })?;
                self.collect_event_causes(producer, visited, causes)?;
            }
            SemanticExpressionKind::FunctionParameter { parameter, .. } => {
                let inputs = self.parameter_inputs.get(parameter).ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "semantic function parameter {:?} has no exact input inventory",
                        parameter
                    ))
                })?;
                for input in inputs {
                    self.collect_event_causes(*input, visited, causes)?;
                }
            }
            _ => {
                for child in semantic_expression_children(&expression.kind, self.execution)? {
                    self.collect_event_causes(child, visited, causes)?;
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
        if let SemanticExpressionKind::CanonicalRead {
            source: Some(source),
            ..
        } = &expression.kind
        {
            causes.insert(SemanticEventCauseV1::Source(source.source));
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
                SemanticValueOrigin::Runtime | SemanticValueOrigin::MaterializationLocal { .. } => {
                }
            }
        }
        if matches!(expression.kind, SemanticExpressionKind::Source { .. }) {
            let matches = self
                .resources
                .sources
                .iter()
                .filter(|source| source.expression == expression.id)
                .map(|source| source.id)
                .collect::<Vec<_>>();
            let [source] = matches.as_slice() else {
                return Err(SemanticReactiveError::new(format!(
                    "semantic SOURCE expression {} resolves to {} source resources",
                    expression.id,
                    matches.len()
                )));
            };
            causes.insert(SemanticEventCauseV1::Source(*source));
        }
        Ok(causes)
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
        let expression = self.expressions.expression(id)?;
        match &expression.kind {
            SemanticExpressionKind::When {
                input,
                arms: select_arms,
                ..
            } => {
                let causes = self.event_causes_for_expression(*input)?;
                if !causes.is_empty() {
                    for cause in causes {
                        arms.insert(self.arm(cause, *input, id)?);
                    }
                    return Ok(());
                }
                for arm in select_arms {
                    self.collect_trigger_arms(arm.output, terminal, visited, arms)?;
                }
            }
            SemanticExpressionKind::Then { input, output } => {
                let causes = self.event_causes_for_expression(*input)?;
                if !causes.is_empty() {
                    // Exact THEN identity rule: when no output expression is
                    // present, the gated input is itself the arm output.
                    let arm_output = output.unwrap_or(*input);
                    for cause in causes {
                        arms.insert(self.arm(cause, *input, arm_output)?);
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
                            self.expressions.expression(argument.value)?.flow_type.mode,
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
        let gate_expression = self.expressions.expression(gate)?;
        Ok(RawTriggerArm {
            cause,
            gate_checked_expression: gate_expression.checked_expr_id,
            gate_expression: gate,
            gate_value: gate_expression.value_id,
            owner: gate_expression.owner,
            route_scope: self.expressions.route_scope(gate)?,
            row_scope: self.row_scope(cause, gate_expression)?,
            output_expression: output,
            output_value: self.expressions.value(output)?,
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
        let origin = self.expressions.origin(expression.id)?;
        // Absence is represented as an empty candidate set solely so the
        // exact-cardinality check below reports zero; it never suppresses or
        // substitutes a binding.
        let statement_ids = self
            .statement_values
            .get(&declaration)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let matches = self
            .bindings
            .iter()
            .filter(|binding| {
                statement_ids.contains(&binding.statement)
                    && binding.owner == expression.owner
                    && binding.call_instance == origin.call_instance
            })
            .collect::<Vec<_>>();
        let [binding] = matches.as_slice() else {
            return Err(SemanticReactiveError::new(format!(
                "reactive expression {} resolves declaration {} to {} exact bindings",
                expression.id,
                declaration.0,
                matches.len()
            )));
        };
        Ok(*binding)
    }
}

fn semantic_expression_children(
    kind: &SemanticExpressionKind,
    execution: &SemanticExecutionGraphV1,
) -> Result<Vec<SemanticExprId>, SemanticReactiveError> {
    Ok(match kind {
        SemanticExpressionKind::CanonicalRead { .. }
        | SemanticExpressionKind::LocalRead { .. }
        | SemanticExpressionKind::ExternalRead { .. }
        | SemanticExpressionKind::ElementState { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Bool(_)
        | SemanticExpressionKind::Tag(_)
        | SemanticExpressionKind::Source { .. }
        | SemanticExpressionKind::Delimiter
        | SemanticExpressionKind::MaterializationLocal { .. }
        | SemanticExpressionKind::FunctionParameter { .. } => Vec::new(),
        SemanticExpressionKind::Materialize { materialization } => {
            let materialization = execution
                .materializations
                .get(materialization.as_usize())
                .filter(|candidate| candidate.id == *materialization)
                .ok_or_else(|| {
                    SemanticReactiveError::new(format!(
                        "expression references missing semantic materialization {}",
                        materialization
                    ))
                })?;
            materialization.expression_roots()
        }
        SemanticExpressionKind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                crate::SemanticTextSegment::Static { .. } => None,
                crate::SemanticTextSegment::Dynamic { value } => Some(*value),
            })
            .collect(),
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields)
        | SemanticExpressionKind::Record(fields) => {
            fields.iter().map(|field| field.value).collect()
        }
        SemanticExpressionKind::Block { bindings, result } => bindings
            .iter()
            .map(|binding| binding.value)
            .chain(std::iter::once(*result))
            .collect(),
        SemanticExpressionKind::Call { arguments, .. } => {
            arguments.iter().map(|argument| argument.value).collect()
        }
        SemanticExpressionKind::Draining { input }
        | SemanticExpressionKind::Project { input, .. } => vec![*input],
        SemanticExpressionKind::Hold {
            initial, updates, ..
        } => std::iter::once(*initial)
            .chain(updates.iter().copied())
            .collect(),
        SemanticExpressionKind::Latest { branches } => branches.clone(),
        SemanticExpressionKind::When { input, arms, .. } => std::iter::once(*input)
            .chain(arms.iter().map(|arm| arm.output))
            .collect(),
        SemanticExpressionKind::Then { input, output } => std::iter::once(*input)
            .chain(output.iter().copied())
            .collect(),
        SemanticExpressionKind::Infix { left, right, .. } => vec![*left, *right],
        SemanticExpressionKind::MatchArm { output, .. } => output.iter().copied().collect(),
        SemanticExpressionKind::List { items, .. }
        | SemanticExpressionKind::Bytes { items, .. } => items.clone(),
    })
}

fn exact_call_argument_at_ordinal(
    execution: &SemanticExecutionGraphV1,
    callable: crate::SemanticCallableId,
    arguments: &[crate::SemanticCallArgument],
    ordinal: usize,
    expression: SemanticExprId,
) -> Result<SemanticExprId, SemanticReactiveError> {
    let callable = execution
        .callables
        .get(callable.as_usize())
        .filter(|candidate| candidate.id == callable)
        .ok_or_else(|| {
            SemanticReactiveError::new(format!(
                "semantic call expression {expression} references missing callable {callable}"
            ))
        })?;
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
    expressions: &ExpressionIndex<'_>,
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
        if matches!(expressions.expression(exact)?.flow_type.ty, Type::List(_)) {
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

fn unique_state_origin(
    expression: &SemanticExpression,
) -> Result<Option<SemanticStateId>, SemanticReactiveError> {
    let states = expression
        .provenance
        .members
        .iter()
        .filter_map(|member| match member.origin {
            SemanticValueOrigin::State { state, .. } => Some(state),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    match states.len() {
        0 => Ok(None),
        1 => Ok(states.iter().next().copied()),
        count => Err(SemanticReactiveError::new(format!(
            "semantic read {} has {count} exact state origins",
            expression.id
        ))),
    }
}

fn unique_binding_for_target<'a>(
    bindings: &'a [SemanticBindingV1],
    target: SemanticBindingTargetV1,
    expression: &SemanticExpression,
    execution: &SemanticExecutionGraphV1,
) -> Result<&'a SemanticBindingV1, SemanticReactiveError> {
    let origin = execution
        .checked_expression_origins
        .get(expression.id.as_usize())
        .filter(|origin| origin.expression == expression.id)
        .ok_or_else(|| {
            SemanticReactiveError::new(format!(
                "semantic expression {} has no exact origin",
                expression.id
            ))
        })?;
    let matches = bindings
        .iter()
        .filter(|binding| {
            binding.target == target
                && binding.owner == expression.owner
                && binding.call_instance == origin.call_instance
        })
        .collect::<Vec<_>>();
    let [binding] = matches.as_slice() else {
        return Err(SemanticReactiveError::new(format!(
            "semantic read {} resolves target {:?} to {} exact bindings",
            expression.id,
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

fn trigger_row_scope(
    causes: &BTreeSet<SemanticEventCauseV1>,
    resources: &SemanticResourceGraphV1,
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
    fn expression_value_identity_is_total_and_dense() {
        let mut execution = graph(vec![source_expression(0, 0, None)]);
        execution.expressions[0].value_id = SemanticValueId(1);
        let error = ExpressionIndex::new(&execution).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("exact dense expression/value identity")
        );
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
        let index = ExpressionIndex::new(&execution).unwrap();
        let error = index.route_scope(SemanticExprId(0)).unwrap_err();
        assert!(error.to_string().contains("resolves to 2 semantic scopes"));
    }

    #[test]
    fn missing_child_expression_is_rejected_without_a_fallback() {
        let execution = graph(vec![SemanticExpression {
            id: SemanticExprId(0),
            value_id: SemanticValueId(0),
            checked_expr_id: CheckedExprId(0),
            flow_type: flow(FlowMode::Continuous),
            effect: CheckedEffectSummary::default(),
            owner: None,
            provenance: SemanticValueProvenance::default(),
            resource_binding_path: None,
            kind: SemanticExpressionKind::Project {
                input: SemanticExprId(1),
                fields: vec!["missing".to_owned()],
            },
        }]);
        let index = ExpressionIndex::new(&execution).unwrap();
        let error = index.expression(SemanticExprId(1)).unwrap_err();
        assert!(error.to_string().contains("missing semantic expression 1"));
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
