#![forbid(unsafe_code)]

mod contextual_expansion;
mod core_lowering;
mod dependency_manifest;
mod execution;
mod lowering_contract;
mod memory_contract;
pub mod program_core;
mod reactive;
mod resource;
mod semantic_image;
mod storage_contract;
mod verified_intent;
mod view_contract;

#[doc(hidden)]
pub mod out_net;

pub use dependency_manifest::*;
pub use execution::*;
pub use lowering_contract::*;
pub use memory_contract::*;
pub use out_net::{
    DistributedCallOccurrenceRoot, OutCallInstanceId, OutInputValue, OutNetId, ProducerFunctionId,
    ProducerParameterId, ProducerResultStatementId, ScopedCheckedExpr, StaticOwnerDef,
    StaticOwnerId,
};
pub use reactive::*;
pub use resource::*;
pub use semantic_image::*;
pub use storage_contract::*;
pub use view_contract::*;

use boon_checked::{
    CheckedExternalDeclarationIdentityV1, CheckedProgram, CheckedProgramFields, DeclId,
};
use boon_contract::SourceBundleDigestV1;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

pub const SEMANTIC_PROGRAM_SCHEMA_V1: &str = "boon.semantic-program.v1";
pub const BUNDLE_SEMANTIC_PROGRAM_SCHEMA_V1: &str = "boon.bundle-semantic-program.v1";
pub const DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1: [u8; 32] = [
    0x5d, 0x6e, 0x99, 0xcf, 0x20, 0xa7, 0x9b, 0x22, 0xcb, 0x5d, 0xb2, 0x01, 0x50, 0x3c, 0x77, 0xa5,
    0xe5, 0xc6, 0x3d, 0x36, 0xbd, 0x7e, 0xa0, 0xec, 0x8d, 0xb2, 0x14, 0x0c, 0x3b, 0xe8, 0x64, 0xbe,
];
pub const MAX_BUNDLE_SEMANTIC_PRODUCER_REQUESTS_V1: usize = 4_096;
pub const MAX_BUNDLE_SEMANTIC_PRODUCER_REQUEST_BYTES_V1: usize = 4 * 1024 * 1024;
pub const MAX_BUNDLE_SEMANTIC_CALL_CROSSINGS_V1: usize = 4_096;
pub const MAX_BUNDLE_SEMANTIC_CALL_CROSSING_BYTES_V1: usize = 32 * 1024 * 1024;
pub const MAX_BUNDLE_SEMANTIC_VALUE_CROSSINGS_V1: usize = 16_384;
pub const MAX_BUNDLE_SEMANTIC_VALUE_CROSSING_BYTES_V1: usize = 32 * 1024 * 1024;
const SEMANTIC_PROGRAM_DIGEST_DOMAIN: &[u8] = b"boon.semantic-program.v1\0";
const CANONICAL_PROGRAM_CORE_DIGEST_DOMAIN: &[u8] = b"boon.canonical-program-core.v2\0";
const BUNDLE_SEMANTIC_PROGRAM_DIGEST_DOMAIN: &[u8] = b"boon.bundle-semantic-program.v1\0";
const OUT_PORT_SHAPE_DIGEST_DOMAIN: &[u8] = b"boon.out-port-shape.v1\0";
const PRODUCER_MATERIALIZATION_IDENTITY_DOMAIN: &[u8] =
    b"boon.producer-materialization-identity.v1\0";
const DISTRIBUTED_VALUE_OCCURRENCE_IDENTITY_DOMAIN: &[u8] =
    b"boon.distributed-value-occurrence-identity.v1\0";

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

digest_type!(SemanticProgramDigestV1);
digest_type!(BundleSemanticProgramDigestV1);
digest_type!(CheckedProgramDigestV1);
digest_type!(CallableDependencyManifestDigestV1);
digest_type!(DependencyClassifierSchemaDigestV1);
digest_type!(DistributedValueOccurrenceIdentityV1);

pub type ResolvedOutGraph = out_net::OutNet<OutPortContractV1>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutPortContractV1 {
    pub flow_type: boon_checked::FlowType,
    pub resolved_type: boon_checked::Type,
    pub shape_digest: [u8; 32],
    pub lexical_scope: boon_checked::LexicalScopeId,
    pub output_scope: boon_checked::LexicalScopeId,
    pub role: boon_checked::ProgramRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_identity: Option<OutGenerationIdentityV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_identity: Option<OutCorrelationIdentityV1>,
    pub presence: OutPresenceCompatibilityV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutGenerationIdentityV1 {
    pub owner: StaticOwnerId,
    pub output_scope: boon_checked::LexicalScopeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutCorrelationIdentityV1 {
    pub net: OutNetId,
    pub owner: StaticOwnerId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutPresenceCompatibilityV1 {
    pub mode: boon_checked::FlowMode,
    pub may_be_present: bool,
    pub may_be_absent: bool,
}

impl OutPresenceCompatibilityV1 {
    const fn from_mode(mode: boon_checked::FlowMode) -> Self {
        match mode {
            boon_checked::FlowMode::Continuous => Self {
                mode,
                may_be_present: true,
                may_be_absent: false,
            },
            boon_checked::FlowMode::TickPresent | boon_checked::FlowMode::PresentOrAbsent => Self {
                mode,
                may_be_present: true,
                may_be_absent: true,
            },
            boon_checked::FlowMode::Absent => Self {
                mode,
                may_be_present: false,
                may_be_absent: true,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerMaterializationMode {
    Current,
    Invocation,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ProducerMaterializationRequest {
    pub identity: [u8; 32],
    pub callable: SemanticCallableId,
    pub local_function: String,
    pub mode: ProducerMaterializationMode,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DistributedCallOccurrence {
    pub root: DistributedCallOccurrenceRoot,
    pub call: SemanticCallId,
    pub callable: SemanticCallableId,
    pub call_path: Vec<SemanticCallId>,
    pub occurrence_path: String,
    pub canonical_function: String,
    pub producer_role: boon_checked::ProgramRole,
    pub mode: ProducerMaterializationMode,
    pub producer_materialization_identity: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedValueOccurrence {
    pub identity: DistributedValueOccurrenceIdentityV1,
    pub root: DistributedCallOccurrenceRoot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub call_path: Vec<SemanticCallId>,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub checked_expression: boon_checked::CheckedExprId,
    pub external_identity: boon_checked::CheckedExternalDeclarationIdentityV1,
    pub producer_role: boon_checked::ProgramRole,
    /// Diagnostic only; excluded from structural occurrence identity.
    pub occurrence_path: String,
    /// Diagnostic only; excluded from structural occurrence identity.
    pub canonical_path: String,
}

#[derive(Clone)]
struct DistributedCallAnalysisFrame {
    owner_callable: Option<SemanticCallableId>,
    root: DistributedCallOccurrenceRoot,
    path: String,
    call_path: Vec<SemanticCallId>,
    active_callables: Vec<SemanticCallableId>,
}

fn producer_materialization_identity(
    program: &SemanticProgram,
    root: DistributedCallOccurrenceRoot,
    call_path: &[SemanticCallId],
    external_identity: boon_checked::CheckedExternalDeclarationIdentityV1,
    mode: ProducerMaterializationMode,
) -> Result<[u8; 32], SemanticError> {
    canonical_hash(
        PRODUCER_MATERIALIZATION_IDENTITY_DOMAIN,
        &(
            program.source_bundle_digest_v1,
            program.role(),
            root,
            call_path,
            external_identity,
            mode,
        ),
    )
}

/// Resolves the complete fixed-point input for distributed producer discovery
/// exclusively from the validated semantic call/callable inventory.
pub fn distributed_call_occurrences(
    program: &SemanticProgram,
) -> Result<Vec<DistributedCallOccurrence>, SemanticError> {
    program.validate()?;
    let execution = program.execution_graph();
    let mut calls_by_owner = BTreeMap::<Option<SemanticCallableId>, Vec<&SemanticCall>>::new();
    for call in &execution.calls {
        calls_by_owner
            .entry(call.owner_callable)
            .or_default()
            .push(call);
    }
    for calls in calls_by_owner.values_mut() {
        calls.sort_by_key(|call| call.id);
    }

    let mut frames = vec![DistributedCallAnalysisFrame {
        owner_callable: None,
        root: DistributedCallOccurrenceRoot::Program,
        path: "program".to_owned(),
        call_path: Vec::new(),
        active_callables: Vec::new(),
    }];
    for request in program.producer_materializations.iter().rev() {
        let callable = execution
            .callables
            .get(request.callable.as_usize())
            .filter(|callable| {
                callable.id == request.callable
                    && callable.kind == boon_checked::CheckedCallableKind::User
                    && callable.name == request.local_function
            })
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "producer request callable {} does not exactly identify `{}`",
                    request.callable, request.local_function
                ))
            })?;
        frames.push(DistributedCallAnalysisFrame {
            owner_callable: Some(callable.id),
            root: DistributedCallOccurrenceRoot::Producer(request.identity),
            path: format!("producer:{}", digest_hex(&request.identity)),
            call_path: Vec::new(),
            active_callables: vec![callable.id],
        });
    }

    let mut occurrences = BTreeMap::<String, DistributedCallOccurrence>::new();
    while let Some(frame) = frames.pop() {
        let calls = calls_by_owner
            .get(&frame.owner_callable)
            .cloned()
            .unwrap_or_default();
        for call in calls.into_iter().rev() {
            let callable = execution
                .callables
                .get(call.callable.as_usize())
                .filter(|callable| callable.id == call.callable)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "semantic call {} references missing callable {}",
                        call.id, call.callable
                    ))
                })?;
            let path = format!("{}/{}", frame.path, call.occurrence_segment);
            let mut call_path = frame.call_path.clone();
            call_path.push(call.id);
            match callable.kind {
                boon_checked::CheckedCallableKind::User => {
                    if frame.active_callables.contains(&callable.id) {
                        return Err(SemanticError::new(format!(
                            "distributed call analysis encountered recursive callable `{}`",
                            callable.name
                        )));
                    }
                    let mut active_callables = frame.active_callables.clone();
                    active_callables.push(callable.id);
                    frames.push(DistributedCallAnalysisFrame {
                        owner_callable: Some(callable.id),
                        root: frame.root,
                        path,
                        call_path,
                        active_callables,
                    });
                }
                boon_checked::CheckedCallableKind::External => {
                    let Some(producer_role) = distributed_function_role(&call.function) else {
                        continue;
                    };
                    let external_identity = call.external_identity.ok_or_else(|| {
                        SemanticError::new(format!(
                            "distributed semantic call {} has no sealed external declaration identity",
                            call.id
                        ))
                    })?;
                    if external_identity.kind
                        != boon_checked::CheckedExternalDeclarationKind::Callable
                        || external_identity.producer_role != producer_role
                        || callable.external_identity != Some(external_identity)
                    {
                        return Err(SemanticError::new(format!(
                            "distributed semantic call {} has an incompatible sealed external declaration identity",
                            call.id
                        )));
                    }
                    let structural_occurrence = DistributedCallOccurrence {
                        root: frame.root,
                        call: call.id,
                        callable: callable.id,
                        call_path: call_path.clone(),
                        occurrence_path: path.clone(),
                        canonical_function: call.function.clone(),
                        producer_role,
                        mode: ProducerMaterializationMode::Current,
                        producer_materialization_identity: [0; 32],
                    };
                    let (call_expression, _, _) =
                        exact_call_expression_for_occurrence(program, &structural_occurrence)?;
                    let schedules = program
                        .reactive_graph()
                        .call_invocations
                        .iter()
                        .filter(|schedule| schedule.expression == call_expression.id)
                        .collect::<Vec<_>>();
                    let [schedule] = schedules.as_slice() else {
                        return Err(SemanticError::new(format!(
                            "distributed semantic call {} expression {} maps to {} invocation schedules",
                            call.id,
                            call_expression.id,
                            schedules.len()
                        )));
                    };
                    if schedule.call != call.id || schedule.value != call_expression.value_id {
                        return Err(SemanticError::new(format!(
                            "distributed semantic call {} invocation schedule differs from its concrete semantic expression",
                            call.id
                        )));
                    }
                    let invocation_arms = program
                        .reactive_graph()
                        .invocation_arms_for_call_expression(call_expression.id)
                        .map_err(|error| SemanticError::new(error.to_string()))?;
                    let mode = if schedule.current_capable && invocation_arms.is_empty() {
                        ProducerMaterializationMode::Current
                    } else {
                        ProducerMaterializationMode::Invocation
                    };
                    let producer_materialization_identity = producer_materialization_identity(
                        program,
                        frame.root,
                        &call_path,
                        external_identity,
                        mode,
                    )?;
                    if producer_materialization_identity
                        .iter()
                        .all(|byte| *byte == 0)
                    {
                        return Err(SemanticError::new(format!(
                            "distributed occurrence `{path}` produced a zero materialization identity"
                        )));
                    }
                    let occurrence = DistributedCallOccurrence {
                        root: frame.root,
                        call: call.id,
                        callable: callable.id,
                        call_path,
                        occurrence_path: path.clone(),
                        canonical_function: call.function.clone(),
                        producer_role,
                        mode,
                        producer_materialization_identity,
                    };
                    if let Some(previous) = occurrences.insert(path.clone(), occurrence.clone())
                        && previous != occurrence
                    {
                        return Err(SemanticError::new(format!(
                            "distributed occurrence `{path}` resolves to conflicting call contracts"
                        )));
                    }
                }
                boon_checked::CheckedCallableKind::Builtin => {}
            }
        }
    }
    Ok(occurrences.into_values().collect())
}

fn distributed_value_structural_root(
    program: &SemanticProgram,
    frame: Option<OutCallInstanceId>,
) -> Result<(DistributedCallOccurrenceRoot, Vec<SemanticCallId>), SemanticError> {
    let Some(frame) = frame else {
        return Ok((DistributedCallOccurrenceRoot::Program, Vec::new()));
    };
    let out_net = program.resolved_out_graph();
    let mut ancestry = Vec::new();
    let mut next = Some(frame);
    let mut remaining = out_net.call_instances.len().saturating_add(1);
    while let Some(call) = next {
        if remaining == 0 {
            return Err(SemanticError::new(format!(
                "distributed value frame {frame} has cyclic OUT ancestry"
            )));
        }
        remaining -= 1;
        let instance = out_net
            .call_instances
            .get(call.as_usize())
            .filter(|instance| instance.id == call)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "distributed value frame ancestry references missing OUT call {call}"
                ))
            })?;
        ancestry.push(call);
        next = instance.parent;
    }
    ancestry.reverse();

    let producer_root = ancestry.first().and_then(|root| {
        out_net
            .producer_roots()
            .iter()
            .find(|producer| producer.call == *root)
    });
    let root = producer_root
        .map(|producer| DistributedCallOccurrenceRoot::Producer(producer.spec.identity))
        .unwrap_or(DistributedCallOccurrenceRoot::Program);
    let first_static = usize::from(producer_root.is_some());
    let mut call_path = Vec::with_capacity(ancestry.len().saturating_sub(first_static));
    for instance in ancestry.into_iter().skip(first_static) {
        let checked_call = out_net.call_instances[instance.as_usize()]
            .provenance
            .call_id
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "distributed value non-root OUT call {instance} has no checked call identity"
                ))
            })?;
        let matches = program
            .execution_graph()
            .calls
            .iter()
            .filter(|call| call.checked_call == checked_call)
            .map(|call| call.id)
            .collect::<Vec<_>>();
        let [call] = matches.as_slice() else {
            return Err(SemanticError::new(format!(
                "distributed value OUT call {instance} checked identity {} maps to {} semantic calls",
                checked_call.0,
                matches.len()
            )));
        };
        call_path.push(*call);
    }
    Ok((root, call_path))
}

fn distributed_value_occurrence_identity(
    program: &SemanticProgram,
    root: DistributedCallOccurrenceRoot,
    call_path: &[SemanticCallId],
    checked_expression: boon_checked::CheckedExprId,
    external_identity: boon_checked::CheckedExternalDeclarationIdentityV1,
) -> Result<DistributedValueOccurrenceIdentityV1, SemanticError> {
    // Global SemanticExprId/SemanticValueId coordinates are intentionally not
    // part of this cross-round key: canonically inserting an earlier producer
    // root may rebase dense IDs without changing this occurrence.
    Ok(DistributedValueOccurrenceIdentityV1(canonical_hash(
        DISTRIBUTED_VALUE_OCCURRENCE_IDENTITY_DOMAIN,
        &(
            program.source_bundle_digest_v1,
            program.role(),
            root,
            call_path,
            checked_expression,
            external_identity,
        ),
    )?))
}

/// Resolve every concrete cross-role value read from semantic identity alone.
///
/// Producer roots are part of the structural key, so newly materialized
/// producer bodies can reveal additional value crossings monotonically during
/// distributed fixed-point construction.
pub fn distributed_value_occurrences(
    program: &SemanticProgram,
) -> Result<Vec<DistributedValueOccurrence>, SemanticError> {
    program.validate()?;
    let execution = program.execution_graph();
    let mut occurrences = BTreeMap::new();
    for expression in &execution.expressions {
        let SemanticExpressionKind::ExternalRead {
            canonical_path,
            external_identity,
        } = &expression.kind
        else {
            continue;
        };
        let external_identity = external_identity.ok_or_else(|| {
            SemanticError::new(format!(
                "{} external value expression {} has no sealed declaration identity",
                program.role().namespace(),
                expression.id
            ))
        })?;
        if external_identity.kind != boon_checked::CheckedExternalDeclarationKind::Value
            || external_identity.producer_role == program.role()
        {
            return Err(SemanticError::new(format!(
                "{} external value expression {} has an incompatible sealed declaration identity",
                program.role().namespace(),
                expression.id
            )));
        }
        let origin = execution
            .checked_expression_origins
            .get(expression.id.as_usize())
            .filter(|origin| {
                origin.expression == expression.id
                    && origin.checked_expression == expression.checked_expr_id
            })
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "external value expression {} has no exact checked origin",
                    expression.id
                ))
            })?;
        let (root, call_path) = distributed_value_structural_root(program, origin.call_instance)?;
        let identity = distributed_value_occurrence_identity(
            program,
            root,
            &call_path,
            origin.checked_expression,
            external_identity,
        )?;
        if identity.as_bytes().iter().all(|byte| *byte == 0) {
            return Err(SemanticError::new(format!(
                "external value expression {} produced a zero structural occurrence identity",
                expression.id
            )));
        }
        let mut occurrence_path = match root {
            DistributedCallOccurrenceRoot::Program => "program".to_owned(),
            DistributedCallOccurrenceRoot::Producer(identity) => {
                format!("producer:{}", digest_hex(&identity))
            }
        };
        for call in &call_path {
            occurrence_path.push_str(&format!("/call:{}", call.0));
        }
        occurrence_path.push_str(&format!(
            "/value:{}:{}",
            origin.checked_expression.0, expression.id.0
        ));
        let occurrence = DistributedValueOccurrence {
            identity,
            root,
            call_path,
            expression: expression.id,
            value: expression.value_id,
            checked_expression: origin.checked_expression,
            external_identity,
            producer_role: external_identity.producer_role,
            occurrence_path,
            canonical_path: canonical_path.clone(),
        };
        if let Some(previous) = occurrences.insert(identity, occurrence.clone())
            && previous != occurrence
        {
            return Err(SemanticError::new(format!(
                "distributed value occurrence {} resolves to conflicting semantic reads",
                identity
            )));
        }
    }
    Ok(occurrences.into_values().collect())
}

/// The single pre-backend semantic artifact.
///
/// Checked and execution construction state is consumed into one sealed image.
/// Remaining domain graphs are migrated into that image in dependency order;
/// none may recreate or retain the rich checked program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticProgram {
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: boon_checked::ProgramRole,
    semantic_image: SealedSemanticImageV3,
    #[cfg(test)]
    checked_program: CheckedProgramFields,
    #[cfg(test)]
    execution_graph: SemanticExecutionImageColumnsV1,
    producer_materializations: Vec<ProducerMaterializationRequest>,
    resolved_out_graph: ResolvedOutGraph,
    resource_graph: SemanticResourceGraphV2,
    reactive_graph: SemanticReactiveGraphV1,
    lowering_contract: SemanticLoweringContractV2,
    view_binding_graph: SemanticViewBindingGraphV1,
    scope_storage_graph: SemanticScopeStorageGraphV1,
    memory_graph: SemanticMemoryGraphV1,
    canonical_core: program_core::CanonicalProgramCoreV2,
    dependency_manifest: CallableDependencyManifestV7,
    request_graph: Arc<boon_compilation_db::SealedRequestGraphSnapshot>,
    digest: SemanticProgramDigestV1,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct BundleSemanticRoleDigestV1 {
    pub role: boon_checked::ProgramRole,
    pub semantic_program_digest: SemanticProgramDigestV1,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct BundleProducerMaterializationRequestV1 {
    pub role: boon_checked::ProgramRole,
    pub request: ProducerMaterializationRequest,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleSemanticRouteScopeV1 {
    SessionLocal,
    OriginScoped,
    SharedSubscription,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BundleSemanticCallArgumentBindingV1 {
    Explicit {
        checked_value: boon_checked::CheckedExprId,
        expression: SemanticExprId,
        value: SemanticValueId,
        flow_type: boon_checked::FlowType,
        from_pipe: bool,
    },
    Omitted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BundleSemanticCallArgumentV1 {
    pub ordinal: usize,
    pub name: String,
    pub consumer_parameter: SemanticParameterId,
    pub consumer_formal: DeclId,
    pub producer_parameter: SemanticParameterId,
    pub producer_formal: DeclId,
    pub producer_flow_type: boon_checked::FlowType,
    pub requirement: boon_checked::CheckedParameterRequirement,
    pub evaluation_scope: boon_checked::CheckedEvaluationScope,
    pub binding: BundleSemanticCallArgumentBindingV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BundleSemanticCallCrossingV1 {
    pub consumer_role: boon_checked::ProgramRole,
    pub consumer_call: SemanticCallId,
    pub consumer_callable: SemanticCallableId,
    pub consumer_expression: SemanticExprId,
    pub consumer_value: SemanticValueId,
    pub consumer_instance: OutCallInstanceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_callable: Option<SemanticCallableId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub consumer_scope: SemanticScopeId,
    pub producer_role: boon_checked::ProgramRole,
    pub producer_callable: SemanticCallableId,
    pub external_identity: boon_checked::CheckedExternalDeclarationIdentityV1,
    pub producer_materialization_identity: [u8; 32],
    pub root: DistributedCallOccurrenceRoot,
    pub call_path: Vec<SemanticCallId>,
    pub occurrence_path: String,
    pub canonical_function: String,
    pub local_function: String,
    pub result: boon_checked::FlowType,
    pub effect: boon_checked::CheckedEffectSummary,
    pub arguments: Vec<BundleSemanticCallArgumentV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invocation_arms: Vec<SemanticTriggerOwnedArmV1>,
    pub mode: ProducerMaterializationMode,
    pub route_scope: BundleSemanticRouteScopeV1,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BundleSemanticValueDeliveryV1 {
    Current,
    Event {
        source: SemanticSourceId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        payload_projection: Vec<String>,
    },
    RelayedEvent {
        read: SemanticExprId,
        external_identity: boon_checked::CheckedExternalDeclarationIdentityV1,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        payload_projection: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BundleSemanticValueCrossingV1 {
    pub occurrence_identity: DistributedValueOccurrenceIdentityV1,
    pub root: DistributedCallOccurrenceRoot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub call_path: Vec<SemanticCallId>,
    pub checked_expression: boon_checked::CheckedExprId,
    /// Diagnostic only; excluded from structural occurrence identity.
    pub occurrence_path: String,
    pub consumer_role: boon_checked::ProgramRole,
    pub consumer_expression: SemanticExprId,
    pub consumer_value: SemanticValueId,
    pub consumer_scope: SemanticScopeId,
    pub producer_role: boon_checked::ProgramRole,
    pub producer_declaration: DeclId,
    pub producer_expression: SemanticExprId,
    pub producer_value: SemanticValueId,
    pub external_identity: boon_checked::CheckedExternalDeclarationIdentityV1,
    pub canonical_path: String,
    pub flow_type: boon_checked::FlowType,
    pub delivery: BundleSemanticValueDeliveryV1,
    pub route_scope: BundleSemanticRouteScopeV1,
}

/// Atomic, pre-verification ownership of the exact final Client, Session, and
/// Server semantic programs and their frozen distributed call closure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleSemanticProgramV1 {
    schema: String,
    programs: [SemanticProgram; 3],
    role_digests: Vec<BundleSemanticRoleDigestV1>,
    producer_requests: Vec<BundleProducerMaterializationRequestV1>,
    call_crossings: Vec<BundleSemanticCallCrossingV1>,
    value_crossings: Vec<BundleSemanticValueCrossingV1>,
    digest: BundleSemanticProgramDigestV1,
}

impl SemanticProgram {
    pub fn role(&self) -> boon_checked::ProgramRole {
        self.role
    }

    pub const fn source_bundle_digest_v1(&self) -> SourceBundleDigestV1 {
        self.source_bundle_digest_v1
    }

    pub const fn digest(&self) -> SemanticProgramDigestV1 {
        self.digest
    }

    pub const fn dependency_manifest(&self) -> &CallableDependencyManifestV7 {
        &self.dependency_manifest
    }

    pub fn request_graph_snapshot(&self) -> Arc<boon_compilation_db::SealedRequestGraphSnapshot> {
        Arc::clone(&self.request_graph)
    }

    pub const fn checked_program_digest(&self) -> CheckedProgramDigestV1 {
        self.dependency_manifest.checked_program_digest
    }

    pub const fn resolved_out_graph(&self) -> &ResolvedOutGraph {
        &self.resolved_out_graph
    }

    pub const fn semantic_image(&self) -> &SealedSemanticImageV3 {
        &self.semantic_image
    }

    pub const fn execution_graph(&self) -> &SemanticExecutionImageColumnsV1 {
        #[cfg(test)]
        {
            &self.execution_graph
        }
        #[cfg(not(test))]
        {
            self.semantic_image.execution()
        }
    }

    pub const fn resource_graph(&self) -> &SemanticResourceGraphV2 {
        &self.resource_graph
    }

    pub const fn reactive_graph(&self) -> &SemanticReactiveGraphV1 {
        &self.reactive_graph
    }

    pub const fn lowering_contract(&self) -> &SemanticLoweringContractV2 {
        &self.lowering_contract
    }

    pub const fn scope_storage_graph(&self) -> &SemanticScopeStorageGraphV1 {
        &self.scope_storage_graph
    }

    pub const fn view_binding_graph(&self) -> &SemanticViewBindingGraphV1 {
        &self.view_binding_graph
    }

    pub const fn memory_graph(&self) -> &SemanticMemoryGraphV1 {
        &self.memory_graph
    }

    pub fn producer_materialization_requests(&self) -> &[ProducerMaterializationRequest] {
        &self.producer_materializations
    }

    pub fn producer_callable(
        &self,
        local_function: &str,
    ) -> Result<SemanticCallableId, SemanticError> {
        let matches = self
            .execution_graph()
            .callables
            .iter()
            .filter(|callable| {
                callable.kind == boon_checked::CheckedCallableKind::User
                    && callable.name == local_function
            })
            .map(|callable| callable.id)
            .collect::<Vec<_>>();
        let [callable] = matches.as_slice() else {
            return Err(SemanticError::new(format!(
                "producer function `{local_function}` resolves to {} semantic user callables",
                matches.len()
            )));
        };
        Ok(*callable)
    }

    pub fn validate(&self) -> Result<(), SemanticError> {
        self.semantic_image
            .validate_identity(self.source_bundle_digest_v1, self.role)
            .map_err(SemanticError::new)?;
        let execution = self.execution_graph();
        #[cfg(test)]
        {
            if self.source_bundle_digest_v1 != self.checked_program.source_bundle_digest_v1 {
                return Err(SemanticError::new(
                    "semantic source bundle digest does not match its checked oracle",
                ));
            }
            validate_contextual_bindings(&self.checked_program)?;
            validate_out_contracts(&self.checked_program, &self.resolved_out_graph)?;
            contextual_expansion::validate_checked_callable_and_call_inventory(
                &self.checked_program,
                execution,
            )
            .map_err(SemanticError::new)?;
            execution
                .validate_checked_roots(&self.checked_program)
                .map_err(SemanticError::new)?;
        }
        execution
            .validate(&self.resolved_out_graph)
            .map_err(SemanticError::new)?;
        self.resource_graph
            .validate(execution, &self.resolved_out_graph)
            .map_err(SemanticError::new)?;
        self.reactive_graph
            .validate(execution, &self.resource_graph, &self.resolved_out_graph)
            .map_err(|error| SemanticError::new(error.to_string()))?;
        #[cfg(test)]
        {
            self.lowering_contract
                .validate(
                    &self.checked_program,
                    execution,
                    &self.resource_graph,
                    &self.reactive_graph,
                    &self.resolved_out_graph,
                )
                .map_err(|error| SemanticError::new(error.to_string()))?;
            self.scope_storage_graph
                .validate(
                    &self.checked_program,
                    execution,
                    &self.resource_graph,
                    &self.reactive_graph,
                    &self.lowering_contract,
                    &self.resolved_out_graph,
                )
                .map_err(|error| SemanticError::new(error.to_string()))?;
        }
        self.view_binding_graph
            .validate(
                execution,
                &self.resource_graph,
                &self.reactive_graph,
                &self.scope_storage_graph,
                &self.lowering_contract,
            )
            .map_err(|error| SemanticError::new(error.to_string()))?;
        #[cfg(test)]
        {
            self.memory_graph
                .validate(
                    &self.checked_program,
                    execution,
                    &self.resource_graph,
                    &self.reactive_graph,
                    &self.scope_storage_graph,
                    &self.lowering_contract,
                )
                .map_err(|error| SemanticError::new(error.to_string()))?;
            resource::validate_checked_list_classification(
                &self.checked_program,
                execution,
                &self.resource_graph,
            )
            .map_err(SemanticError::new)?;
            resource::validate_checked_resource_provenance(
                &self.checked_program,
                execution,
                &self.resource_graph,
            )
            .map_err(SemanticError::new)?;
        }
        self.validate_integrity_handoff()
    }

    fn validate_freshly_constructed(&self) -> Result<(), SemanticError> {
        self.semantic_image
            .validate_identity(self.source_bundle_digest_v1, self.role)
            .map_err(SemanticError::new)?;
        // Every component builder has just validated its inputs and exact
        // output shape, and the manifest/digest builders consumed those same
        // immutable values directly. Preserve the independent public deep
        // validator above, but do not immediately serialize and hash the whole
        // artifact a second time before returning it.
        validate_canonical_core_handoff(self)
    }

    fn validate_integrity_handoff(&self) -> Result<(), SemanticError> {
        self.dependency_manifest
            .validate_integrity(
                DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
                &self.semantic_image,
                self.execution_graph(),
            )
            .map_err(|error| SemanticError::new(error.to_string()))?;
        let request_graph_stats = self.request_graph.graph().stats();
        let expected_request_count = self
            .dependency_manifest
            .proof_digests
            .projection_count
            .checked_add(self.dependency_manifest.callable_entries.len())
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| SemanticError::new("semantic request graph count overflow"))?;
        if self.request_graph.revision() != boon_compilation_db::Revision(0)
            || self.request_graph.request_count() != expected_request_count
            || request_graph_stats.nodes != expected_request_count
            || request_graph_stats.edges
                != self.dependency_manifest.proof_digests.projection_edge_count
        {
            return Err(SemanticError::new(
                "semantic request graph differs from its compact dependency proof",
            ));
        }
        validate_canonical_core_handoff(self)?;
        let expected = semantic_program_digest(self)?;
        if self.digest != expected {
            return Err(SemanticError::new(
                "semantic program digest does not match its canonical payload",
            ));
        }
        Ok(())
    }

    /// Consumed only after `boon_verify` has wrapped this artifact in a
    /// `ContractVerifiedProgram`.
    #[doc(hidden)]
    pub fn into_lowering_parts(
        self,
    ) -> (
        SourceBundleDigestV1,
        program_core::CanonicalProgramCoreV2,
        SemanticProgramDigestV1,
        CallableDependencyManifestDigestV1,
    ) {
        (
            self.source_bundle_digest_v1,
            self.canonical_core,
            self.digest,
            self.dependency_manifest.manifest_digest,
        )
    }
}

fn validate_canonical_core_handoff(program: &SemanticProgram) -> Result<(), SemanticError> {
    let core = &program.canonical_core;
    let execution = program.execution_graph();
    let external_event_identities = program
        .reactive_graph
        .external_event_identities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let expected_external_event_paths = execution
        .expressions
        .iter()
        .filter_map(|expression| {
            let SemanticExpressionKind::ExternalRead {
                canonical_path,
                external_identity: Some(identity),
            } = &expression.kind
            else {
                return None;
            };
            (matches!(
                expression.flow_type.mode,
                boon_checked::FlowMode::TickPresent | boon_checked::FlowMode::PresentOrAbsent
            ) && external_event_identities.contains(identity))
            .then(|| program_core::distributed_event_source_path(canonical_path))
        })
        .collect::<BTreeSet<_>>();
    if core.graph_node_count != core.executable.expressions.len()
        || core.executable.expressions.len() != execution.expressions.len()
        || core.executable.statements.len() != execution.statements.len()
        || core.executable.sources.len() != execution.sources.len()
        || core.executable.states.len() != execution.states.len()
        || core.executable.roots.len() != execution.roots.len()
        || core.executable.functions.len() != execution.functions.len()
        || core.executable.ordinary_functions.len()
            != execution
                .callables
                .iter()
                .filter(|callable| callable.semantic_root.is_some())
                .count()
        || core.materializations.len() != execution.materializations.len()
        || core.sources.len()
            != program.resource_graph.sources.len() + expected_external_event_paths.len()
        || core.state_cells.len() != program.resource_graph.states.len()
        || core.lists.len() != program.resource_graph.lists.len()
        || core.activations.len() != program.reactive_graph.activations.len()
        || core.pulse_batches.len() != program.reactive_graph.pulse_batches.len()
        || core.semantic_memory.len() != program.memory_graph.memories.len()
        || core.migration_edges.len() != program.memory_graph.migration_edges.len()
        || core.expression_count
            != program
                .lowering_contract
                .metadata
                .original_source_expression_count
    {
        return Err(SemanticError::new(
            "canonical program core inventory differs from its semantic graphs",
        ));
    }
    let mapped_external_event_paths = core
        .sources
        .iter()
        .skip(program.resource_graph.sources.len())
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    if mapped_external_event_paths != expected_external_event_paths {
        return Err(SemanticError::new(
            "canonical program core distributed event sources differ from semantic external-event reads",
        ));
    }
    for (semantic, executable) in execution
        .expressions
        .iter()
        .zip(&core.executable.expressions)
    {
        if semantic.id.as_usize() != executable.id.as_usize()
            || semantic.checked_expr_id != executable.checked_expr_id
            || semantic.flow_type != executable.flow_type
            || semantic.effect != executable.effect
            || semantic.owner != executable.owner
            || semantic.resource_binding_path != executable.resource_binding_path
        {
            return Err(SemanticError::new(format!(
                "canonical executable expression {} differs from semantic expression {}",
                executable.id, semantic.id
            )));
        }
    }
    for (semantic, executable) in execution.statements.iter().zip(&core.executable.statements) {
        if semantic.id.as_usize() != executable.id.as_usize()
            || semantic.declaration != executable.declaration
            || semantic.flow_type != executable.flow_type
            || semantic.value.map(SemanticExprId::as_usize)
                != executable
                    .value
                    .map(program_core::ExecutableExprId::as_usize)
        {
            return Err(SemanticError::new(format!(
                "canonical executable statement {} differs from semantic statement {}",
                executable.id, semantic.id
            )));
        }
    }
    for (semantic, executable) in program
        .reactive_graph
        .pulse_batches
        .iter()
        .zip(&core.pulse_batches)
    {
        if semantic.id.as_usize() != executable.id.as_usize()
            || semantic.slice_digest.0 != executable.semantic_slice_digest
            || executable.fusion != program_core::PulseFusionEligibility::PendingVerification
        {
            return Err(SemanticError::new(format!(
                "canonical pulse batch {} is not the pending semantic pulse batch {}",
                executable.id, semantic.id
            )));
        }
    }
    Ok(())
}

impl BundleSemanticProgramV1 {
    pub fn freeze(programs: [SemanticProgram; 3]) -> Result<Self, SemanticError> {
        let programs = canonical_bundle_programs(programs)?;
        let (role_digests, producer_requests, call_crossings, value_crossings) =
            derive_bundle_call_closure(&programs)?;
        let mut bundle = Self {
            schema: BUNDLE_SEMANTIC_PROGRAM_SCHEMA_V1.to_owned(),
            programs,
            role_digests,
            producer_requests,
            call_crossings,
            value_crossings,
            digest: BundleSemanticProgramDigestV1([0; 32]),
        };
        bundle.digest = bundle_semantic_program_digest(&bundle)?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub const fn digest(&self) -> BundleSemanticProgramDigestV1 {
        self.digest
    }

    pub fn role_program(&self, role: boon_checked::ProgramRole) -> Option<&SemanticProgram> {
        self.programs.iter().find(|program| program.role() == role)
    }

    pub fn role_programs(
        &self,
    ) -> impl ExactSizeIterator<Item = (boon_checked::ProgramRole, &SemanticProgram)> {
        self.programs
            .iter()
            .map(|program| (program.role(), program))
    }

    pub fn role_digests(&self) -> &[BundleSemanticRoleDigestV1] {
        &self.role_digests
    }

    pub fn producer_requests(&self) -> &[BundleProducerMaterializationRequestV1] {
        &self.producer_requests
    }

    pub fn call_crossings(&self) -> &[BundleSemanticCallCrossingV1] {
        &self.call_crossings
    }

    pub fn value_crossings(&self) -> &[BundleSemanticValueCrossingV1] {
        &self.value_crossings
    }

    pub fn validate(&self) -> Result<(), SemanticError> {
        if self.schema != BUNDLE_SEMANTIC_PROGRAM_SCHEMA_V1 {
            return Err(SemanticError::new(format!(
                "unsupported bundle semantic schema `{}`",
                self.schema
            )));
        }
        validate_bundle_closure_bounds(
            &self.producer_requests,
            &self.call_crossings,
            &self.value_crossings,
        )?;
        for program in &self.programs {
            program.validate()?;
        }
        let expected_roles = [
            boon_checked::ProgramRole::Client,
            boon_checked::ProgramRole::Session,
            boon_checked::ProgramRole::Server,
        ];
        for (program, expected_role) in self.programs.iter().zip(expected_roles) {
            if program.role() != expected_role {
                return Err(SemanticError::new(
                    "bundle semantic programs are not in canonical role order",
                ));
            }
        }
        let (role_digests, producer_requests, call_crossings, value_crossings) =
            derive_bundle_call_closure(&self.programs)?;
        if self.role_digests != role_digests {
            return Err(SemanticError::new(
                "bundle semantic role digests are stale or incomplete",
            ));
        }
        if self.producer_requests != producer_requests {
            return Err(SemanticError::new(
                "bundle semantic producer requests are stale or incomplete",
            ));
        }
        if self.call_crossings != call_crossings {
            return Err(SemanticError::new(
                "bundle semantic call crossings are stale or incomplete",
            ));
        }
        if self.value_crossings != value_crossings {
            return Err(SemanticError::new(
                "bundle semantic value crossings are stale or incomplete",
            ));
        }
        let expected_digest = bundle_semantic_program_digest(self)?;
        if self.digest != expected_digest {
            return Err(SemanticError::new(
                "bundle semantic digest does not match its canonical payload",
            ));
        }
        Ok(())
    }

    /// Consumed only by the verified-bundle handoff after bundle verification.
    #[doc(hidden)]
    pub fn into_role_programs(self) -> [SemanticProgram; 3] {
        self.programs
    }
}

fn canonical_bundle_programs(
    programs: [SemanticProgram; 3],
) -> Result<[SemanticProgram; 3], SemanticError> {
    let mut by_role = BTreeMap::new();
    for program in programs {
        let role = program.role();
        if by_role.insert(role, program).is_some() {
            return Err(SemanticError::new(format!(
                "bundle semantic programs contain duplicate {} role",
                role.namespace()
            )));
        }
    }
    let mut take = |role| {
        by_role.remove(&role).ok_or_else(|| {
            SemanticError::new(format!(
                "bundle semantic programs are missing {} role",
                role.namespace()
            ))
        })
    };
    let programs = [
        take(boon_checked::ProgramRole::Client)?,
        take(boon_checked::ProgramRole::Session)?,
        take(boon_checked::ProgramRole::Server)?,
    ];
    if !by_role.is_empty() {
        return Err(SemanticError::new(
            "bundle semantic programs contain an unsupported role",
        ));
    }
    Ok(programs)
}

fn exact_semantic_scope(
    program: &SemanticProgram,
    checked_scope: boon_checked::LexicalScopeId,
    label: &str,
) -> Result<SemanticScopeId, SemanticError> {
    let matches = program
        .execution_graph()
        .scopes
        .iter()
        .filter(|scope| scope.checked_scope == checked_scope)
        .map(|scope| scope.id)
        .collect::<Vec<_>>();
    let [scope] = matches.as_slice() else {
        return Err(SemanticError::new(format!(
            "{label} checked scope {} maps to {} semantic scopes",
            checked_scope.0,
            matches.len()
        )));
    };
    Ok(*scope)
}

fn bundle_call_route_scope(
    consumer: boon_checked::ProgramRole,
    producer: boon_checked::ProgramRole,
) -> Result<BundleSemanticRouteScopeV1, SemanticError> {
    match (consumer, producer) {
        (boon_checked::ProgramRole::Client, boon_checked::ProgramRole::Session)
        | (boon_checked::ProgramRole::Session, boon_checked::ProgramRole::Client) => {
            Ok(BundleSemanticRouteScopeV1::SessionLocal)
        }
        (boon_checked::ProgramRole::Session, boon_checked::ProgramRole::Server)
        | (boon_checked::ProgramRole::Server, boon_checked::ProgramRole::Session) => {
            Ok(BundleSemanticRouteScopeV1::OriginScoped)
        }
        _ => Err(SemanticError::new(format!(
            "distributed call route from {} to {} is not an adjacent role edge",
            consumer.namespace(),
            producer.namespace()
        ))),
    }
}

fn bundle_value_route_scope(
    consumer: boon_checked::ProgramRole,
    producer: boon_checked::ProgramRole,
    delivery: &BundleSemanticValueDeliveryV1,
    producer_origin_scoped: bool,
) -> Result<BundleSemanticRouteScopeV1, SemanticError> {
    match (consumer, producer) {
        (boon_checked::ProgramRole::Client, boon_checked::ProgramRole::Session)
        | (boon_checked::ProgramRole::Session, boon_checked::ProgramRole::Client) => {
            Ok(BundleSemanticRouteScopeV1::SessionLocal)
        }
        (boon_checked::ProgramRole::Server, boon_checked::ProgramRole::Session) => {
            Ok(BundleSemanticRouteScopeV1::OriginScoped)
        }
        (boon_checked::ProgramRole::Session, boon_checked::ProgramRole::Server) => {
            Ok(match delivery {
                BundleSemanticValueDeliveryV1::Event { .. }
                | BundleSemanticValueDeliveryV1::RelayedEvent { .. } => {
                    BundleSemanticRouteScopeV1::OriginScoped
                }
                BundleSemanticValueDeliveryV1::Current if producer_origin_scoped => {
                    BundleSemanticRouteScopeV1::OriginScoped
                }
                BundleSemanticValueDeliveryV1::Current => {
                    BundleSemanticRouteScopeV1::SharedSubscription
                }
            })
        }
        _ => Err(SemanticError::new(format!(
            "distributed value route from {} to {} is not an adjacent role edge",
            consumer.namespace(),
            producer.namespace()
        ))),
    }
}

fn exact_call_expression_for_occurrence<'a>(
    program: &'a SemanticProgram,
    occurrence: &DistributedCallOccurrence,
) -> Result<
    (
        &'a SemanticExpression,
        &'a SemanticExpressionOrigin,
        OutCallInstanceId,
    ),
    SemanticError,
> {
    let mut matches = Vec::new();
    for expression in &program.execution_graph().expressions {
        let SemanticExpressionKind::Call {
            call,
            callable,
            function,
            instance,
            ..
        } = &expression.kind
        else {
            continue;
        };
        if *call != occurrence.call
            || *callable != occurrence.callable
            || function != &occurrence.canonical_function
        {
            continue;
        }
        let Some(instance) = *instance else {
            continue;
        };
        let (root, occurrence_path) = semantic_distributed_call_occurrence(program, instance)?;
        if root == occurrence.root && occurrence_path == occurrence.occurrence_path {
            let origin = program
                .execution_graph()
                .checked_expression_origins
                .get(expression.id.as_usize())
                .filter(|origin| origin.expression == expression.id)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "distributed occurrence `{}` expression {} has no exact origin",
                        occurrence.occurrence_path, expression.id
                    ))
                })?;
            matches.push((expression, origin, instance));
        }
    }
    let [matched] = matches.as_slice() else {
        return Err(SemanticError::new(format!(
            "distributed occurrence `{}` maps to {} concrete semantic call expressions",
            occurrence.occurrence_path,
            matches.len()
        )));
    };
    Ok(*matched)
}

fn semantic_distributed_call_occurrence(
    program: &SemanticProgram,
    frame: OutCallInstanceId,
) -> Result<(DistributedCallOccurrenceRoot, String), SemanticError> {
    let out = program.resolved_out_graph();
    let mut ancestry = Vec::new();
    let mut next = Some(frame);
    let mut remaining = out.call_instances.len().saturating_add(1);
    while let Some(call) = next {
        if remaining == 0 {
            return Err(SemanticError::new(format!(
                "distributed call frame {frame} has cyclic OUT ancestry"
            )));
        }
        remaining -= 1;
        let instance = out
            .call_instances
            .get(call.as_usize())
            .filter(|candidate| candidate.id == call)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "distributed call frame ancestry references missing OUT call {call}"
                ))
            })?;
        ancestry.push(call);
        next = instance.parent;
    }
    ancestry.reverse();

    let producer_root = ancestry.first().and_then(|root| {
        out.producer_roots()
            .iter()
            .find(|producer| producer.call == *root)
    });
    let root = producer_root
        .map(|producer| DistributedCallOccurrenceRoot::Producer(producer.spec.identity))
        .unwrap_or(DistributedCallOccurrenceRoot::Program);
    let mut path = match root {
        DistributedCallOccurrenceRoot::Program => "program".to_owned(),
        DistributedCallOccurrenceRoot::Producer(identity) => {
            format!("producer:{}", digest_hex(&identity))
        }
    };
    let first_static = usize::from(producer_root.is_some());
    for call in ancestry.into_iter().skip(first_static) {
        let checked_call = out.call_instances[call.as_usize()]
            .provenance
            .call_id
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "distributed non-root OUT call {call} has no checked call identity"
                ))
            })?;
        let semantic_call = program
            .execution_graph()
            .calls
            .get(checked_call.0 as usize)
            .filter(|candidate| candidate.checked_call == checked_call)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "distributed OUT call {call} references missing checked call {}",
                    checked_call.0
                ))
            })?;
        path.push('/');
        path.push_str(&semantic_call.occurrence_segment);
    }
    Ok((root, path))
}

fn exact_bundle_call_arguments(
    consumer_program: &SemanticProgram,
    occurrence: &DistributedCallOccurrence,
    call_definition: &SemanticCall,
    call_expression: &SemanticExpression,
    consumer_callable: &SemanticCallable,
    producer_callable: &SemanticCallable,
) -> Result<Vec<BundleSemanticCallArgumentV1>, SemanticError> {
    let SemanticExpressionKind::Call {
        arguments,
        parameter_bindings,
        contexts,
        result,
        effect,
        instance,
        ..
    } = &call_expression.kind
    else {
        return Err(SemanticError::new(format!(
            "distributed occurrence `{}` concrete expression is not a call",
            occurrence.occurrence_path
        )));
    };
    let instance = instance.ok_or_else(|| {
        SemanticError::new(format!(
            "distributed occurrence `{}` has no concrete OUT call frame",
            occurrence.occurrence_path
        ))
    })?;
    if !contexts.is_empty()
        || !call_definition.contexts.is_empty()
        || !matches!(
            call_definition.context_binding,
            boon_checked::CheckedContextBinding::None
        )
    {
        return Err(SemanticError::new(format!(
            "distributed occurrence `{}` carries contextual PASS state across a role boundary",
            occurrence.occurrence_path
        )));
    }
    if call_expression.flow_type != *result
        || call_definition.result != *result
        || producer_callable.result != *result
        || call_expression.effect != *effect
        || call_definition.effect != *effect
        || consumer_callable.effect != *effect
        || producer_callable.effect != *effect
    {
        return Err(SemanticError::new(format!(
            "distributed occurrence `{}` result/effect differs across its consumer and producer semantic definitions",
            occurrence.occurrence_path
        )));
    }
    if call_definition
        .entries
        .iter()
        .any(|entry| !matches!(entry, SemanticCallEntry::Input { .. }))
    {
        return Err(SemanticError::new(format!(
            "distributed occurrence `{}` exposes OUT across a role boundary",
            occurrence.occurrence_path
        )));
    }
    if consumer_callable.parameters.len() != producer_callable.parameters.len()
        || parameter_bindings.len() != consumer_callable.parameters.len()
    {
        return Err(SemanticError::new(format!(
            "distributed occurrence `{}` does not have exact consumer/producer parameter coverage",
            occurrence.occurrence_path
        )));
    }

    let mut consumer_parameters = consumer_callable.parameters.iter().collect::<Vec<_>>();
    consumer_parameters.sort_by_key(|parameter| parameter.ordinal);
    let mut producer_parameters = producer_callable.parameters.iter().collect::<Vec<_>>();
    producer_parameters.sort_by_key(|parameter| parameter.ordinal);
    let mut bindings = parameter_bindings.iter().collect::<Vec<_>>();
    bindings.sort_by_key(|binding| binding.ordinal);
    let mut result_arguments = Vec::with_capacity(bindings.len());
    for ((consumer_parameter, producer_parameter), binding) in consumer_parameters
        .into_iter()
        .zip(producer_parameters)
        .zip(bindings)
    {
        if consumer_parameter.ordinal != producer_parameter.ordinal
            || consumer_parameter.ordinal != binding.ordinal
            || consumer_parameter.name != producer_parameter.name
            || consumer_parameter.name != binding.name
            || consumer_parameter.formal != binding.formal
            || consumer_parameter.kind != boon_checked::CheckedParameterKind::Value
            || producer_parameter.kind != boon_checked::CheckedParameterKind::Value
            || consumer_parameter.flow_type != producer_parameter.flow_type
            || consumer_parameter.requirement != producer_parameter.requirement
            || consumer_parameter.requirement != binding.requirement
            || consumer_parameter.evaluation_scope != producer_parameter.evaluation_scope
        {
            return Err(SemanticError::new(format!(
                "distributed occurrence `{}` parameter ordinal {} differs between consumer binding and sealed producer formal",
                occurrence.occurrence_path, binding.ordinal
            )));
        }
        let matching_arguments = arguments
            .iter()
            .filter(|argument| {
                argument.ordinal == binding.ordinal
                    && argument.formal == binding.formal
                    && argument.name == binding.name
            })
            .collect::<Vec<_>>();
        let static_entry = call_definition.entries.iter().find(|entry| {
            matches!(
                entry,
                SemanticCallEntry::Input {
                    formal,
                    ordinal,
                    name,
                    ..
                } if *formal == binding.formal
                    && *ordinal == binding.ordinal
                    && name == &binding.name
            )
        });
        let binding = match &binding.kind {
            SemanticCallParameterBindingKind::Explicit {
                checked_value,
                value,
                from_pipe,
            } => {
                let [argument] = matching_arguments.as_slice() else {
                    return Err(SemanticError::new(format!(
                        "distributed occurrence `{}` explicit parameter ordinal {} maps to {} concrete arguments",
                        occurrence.occurrence_path,
                        consumer_parameter.ordinal,
                        matching_arguments.len()
                    )));
                };
                let Some(SemanticCallEntry::Input {
                    checked_value: static_checked_value,
                    value_flow_type,
                    from_pipe: static_from_pipe,
                    ..
                }) = static_entry
                else {
                    return Err(SemanticError::new(format!(
                        "distributed occurrence `{}` explicit parameter ordinal {} has no static input entry",
                        occurrence.occurrence_path, consumer_parameter.ordinal
                    )));
                };
                let value_definition = consumer_program
                    .execution_graph()
                    .expressions
                    .get(value.as_usize())
                    .filter(|expression| expression.id == *value)
                    .ok_or_else(|| {
                        SemanticError::new(format!(
                            "distributed occurrence `{}` argument references missing semantic expression {}",
                            occurrence.occurrence_path, value
                        ))
                    })?;
                let call_instance = consumer_program
                    .resolved_out_graph()
                    .call_instances
                    .get(instance.as_usize())
                    .filter(|candidate| candidate.id == instance)
                    .ok_or_else(|| {
                        SemanticError::new(format!(
                            "distributed occurrence `{}` references missing concrete call frame {}",
                            occurrence.occurrence_path, instance
                        ))
                    })?;
                let instantiated_value_flow_type = boon_checked::FlowType {
                    mode: value_flow_type.mode,
                    ty: consumer_program
                        .resolved_out_graph()
                        .apply_type_substitutions(call_instance.id, &value_flow_type.ty),
                };
                if argument.checked_value != *checked_value
                    || argument.value != *value
                    || argument.from_pipe != *from_pipe
                    || static_checked_value != checked_value
                    || static_from_pipe != from_pipe
                    || instantiated_value_flow_type != value_definition.flow_type
                {
                    return Err(SemanticError::new(format!(
                        "distributed occurrence `{}` explicit parameter ordinal {} has inconsistent value provenance: argument checked/value/from-pipe {:?}/{}/{}; binding {:?}/{}/{}; static checked/from-pipe {:?}/{}; instantiated/value flow {:?}/{:?}",
                        occurrence.occurrence_path,
                        consumer_parameter.ordinal,
                        argument.checked_value,
                        argument.value,
                        argument.from_pipe,
                        checked_value,
                        value,
                        from_pipe,
                        static_checked_value,
                        static_from_pipe,
                        instantiated_value_flow_type,
                        value_definition.flow_type,
                    )));
                }
                BundleSemanticCallArgumentBindingV1::Explicit {
                    checked_value: *checked_value,
                    expression: *value,
                    value: value_definition.value_id,
                    flow_type: value_definition.flow_type.clone(),
                    from_pipe: *from_pipe,
                }
            }
            SemanticCallParameterBindingKind::Omitted => {
                if !matching_arguments.is_empty()
                    || static_entry.is_some()
                    || matches!(
                        consumer_parameter.requirement,
                        boon_checked::CheckedParameterRequirement::Required
                    )
                {
                    return Err(SemanticError::new(format!(
                        "distributed occurrence `{}` omitted parameter ordinal {} is not exactly optional and absent",
                        occurrence.occurrence_path, consumer_parameter.ordinal
                    )));
                }
                BundleSemanticCallArgumentBindingV1::Omitted
            }
        };
        result_arguments.push(BundleSemanticCallArgumentV1 {
            ordinal: consumer_parameter.ordinal,
            name: consumer_parameter.name.clone(),
            consumer_parameter: consumer_parameter.id,
            consumer_formal: consumer_parameter.formal,
            producer_parameter: producer_parameter.id,
            producer_formal: producer_parameter.formal,
            producer_flow_type: producer_parameter.flow_type.clone(),
            requirement: producer_parameter.requirement.clone(),
            evaluation_scope: producer_parameter.evaluation_scope,
            binding,
        });
    }
    if result_arguments.len() != parameter_bindings.len()
        || arguments.len()
            != result_arguments
                .iter()
                .filter(|argument| {
                    matches!(
                        &argument.binding,
                        BundleSemanticCallArgumentBindingV1::Explicit { .. }
                    )
                })
                .count()
    {
        return Err(SemanticError::new(format!(
            "distributed occurrence `{}` has extra or missing concrete arguments",
            occurrence.occurrence_path
        )));
    }
    Ok(result_arguments)
}

fn append_bundle_event_projection(
    delivery: BundleSemanticValueDeliveryV1,
    projection: &[String],
) -> BundleSemanticValueDeliveryV1 {
    match delivery {
        BundleSemanticValueDeliveryV1::Current => BundleSemanticValueDeliveryV1::Current,
        BundleSemanticValueDeliveryV1::Event {
            source,
            mut payload_projection,
        } => {
            payload_projection.extend_from_slice(projection);
            BundleSemanticValueDeliveryV1::Event {
                source,
                payload_projection,
            }
        }
        BundleSemanticValueDeliveryV1::RelayedEvent {
            read,
            external_identity,
            mut payload_projection,
        } => {
            payload_projection.extend_from_slice(projection);
            BundleSemanticValueDeliveryV1::RelayedEvent {
                read,
                external_identity,
                payload_projection,
            }
        }
    }
}

fn exact_bundle_value_delivery(
    program: &SemanticProgram,
    root: SemanticExprId,
) -> Result<BundleSemanticValueDeliveryV1, SemanticError> {
    fn resolve(
        program: &SemanticProgram,
        expression_id: SemanticExprId,
        visited: &mut BTreeSet<SemanticExprId>,
    ) -> Result<BundleSemanticValueDeliveryV1, SemanticError> {
        if !visited.insert(expression_id) {
            return Err(SemanticError::new(format!(
                "semantic value delivery has an expression cycle at {expression_id}"
            )));
        }
        let execution = program.execution_graph();
        let expression = execution
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "semantic value delivery references missing expression {expression_id}"
                ))
            })?;
        let direct_sources = execution
            .sources
            .iter()
            .filter(|source| source.expression == expression_id)
            .map(|source| source.id)
            .collect::<Vec<_>>();
        if !direct_sources.is_empty() {
            let [source] = direct_sources.as_slice() else {
                return Err(SemanticError::new(format!(
                    "semantic value expression {expression_id} owns {} source identities",
                    direct_sources.len()
                )));
            };
            return Ok(BundleSemanticValueDeliveryV1::Event {
                source: *source,
                payload_projection: Vec::new(),
            });
        }

        let delivery = match &expression.kind {
            SemanticExpressionKind::CanonicalRead {
                source: Some(source),
                ..
            } => {
                if !execution
                    .sources
                    .iter()
                    .any(|candidate| candidate.id == source.source)
                {
                    return Err(SemanticError::new(format!(
                        "semantic value expression {expression_id} references missing source {}",
                        source.source
                    )));
                }
                BundleSemanticValueDeliveryV1::Event {
                    source: source.source,
                    payload_projection: source.payload_projection.clone(),
                }
            }
            SemanticExpressionKind::CanonicalRead {
                target,
                projection,
                source: None,
                ..
            } => {
                let origin = execution
                    .checked_expression_origins
                    .get(expression_id.as_usize())
                    .filter(|origin| origin.expression == expression_id)
                    .ok_or_else(|| {
                        SemanticError::new(format!(
                            "semantic value expression {expression_id} has no exact origin"
                        ))
                    })?;
                let producers = execution
                    .statements
                    .iter()
                    .filter(|statement| {
                        statement.declaration == Some(*target)
                            && statement.call_instance == origin.call_instance
                    })
                    .filter_map(|statement| statement.value)
                    .collect::<Vec<_>>();
                match producers.as_slice() {
                    [producer] if *producer != expression_id => append_bundle_event_projection(
                        resolve(program, *producer, visited)?,
                        projection,
                    ),
                    [] => BundleSemanticValueDeliveryV1::Current,
                    _ => {
                        return Err(SemanticError::new(format!(
                            "semantic value expression {expression_id} target declaration {} maps to {} exact producer expressions",
                            target.0,
                            producers.len()
                        )));
                    }
                }
            }
            SemanticExpressionKind::LocalRead {
                binding,
                projection,
                ..
            } => {
                let producers = execution
                    .expressions
                    .iter()
                    .flat_map(|expression| match &expression.kind {
                        SemanticExpressionKind::Block { bindings, .. } => bindings
                            .iter()
                            .filter(|candidate| candidate.id == *binding)
                            .map(|candidate| candidate.value)
                            .collect::<Vec<_>>(),
                        _ => Vec::new(),
                    })
                    .collect::<Vec<_>>();
                let [producer] = producers.as_slice() else {
                    return Err(SemanticError::new(format!(
                        "semantic local binding {} maps to {} producer expressions",
                        binding,
                        producers.len()
                    )));
                };
                append_bundle_event_projection(resolve(program, *producer, visited)?, projection)
            }
            SemanticExpressionKind::Project { input, fields } => {
                append_bundle_event_projection(resolve(program, *input, visited)?, fields)
            }
            SemanticExpressionKind::Block { result, .. } => resolve(program, *result, visited)?,
            SemanticExpressionKind::Then {
                output: Some(output),
                ..
            }
            | SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => resolve(program, *output, visited)?,
            SemanticExpressionKind::ExternalRead {
                canonical_path,
                external_identity,
            } if expression.flow_type.mode != boon_checked::FlowMode::Continuous => {
                let external_identity = external_identity.ok_or_else(|| {
                    SemanticError::new(format!(
                        "event-valued semantic alias `{canonical_path}` has no sealed declaration identity"
                    ))
                })?;
                BundleSemanticValueDeliveryV1::RelayedEvent {
                    read: expression_id,
                    external_identity,
                    payload_projection: Vec::new(),
                }
            }
            _ => {
                let direct_source_members = expression
                    .provenance
                    .members
                    .iter()
                    .filter_map(|member| match member {
                        SemanticValueMember {
                            path,
                            origin: SemanticValueOrigin::Source { source, .. },
                        } if path.is_empty() => Some(*source),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>();
                match direct_source_members.len() {
                    0 => BundleSemanticValueDeliveryV1::Current,
                    1 => {
                        let source = direct_source_members.first().copied().ok_or_else(|| {
                            SemanticError::new(format!(
                                "semantic value expression {expression_id} lost its sole direct event source"
                            ))
                        })?;
                        BundleSemanticValueDeliveryV1::Event {
                            source,
                            payload_projection: Vec::new(),
                        }
                    }
                    count => {
                        return Err(SemanticError::new(format!(
                            "semantic value expression {expression_id} has {count} direct event sources"
                        )));
                    }
                }
            }
        };
        Ok(delivery)
    }

    resolve(program, root, &mut BTreeSet::new())
}

fn semantic_expression_depends_on_role(
    program: &SemanticProgram,
    root: SemanticExprId,
    producer_role: boon_checked::ProgramRole,
) -> Result<bool, SemanticError> {
    let execution = program.execution_graph();
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = execution
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "semantic dependency closure references missing expression {expression_id}"
                ))
            })?;
        match &expression.kind {
            SemanticExpressionKind::ExternalRead {
                external_identity: Some(identity),
                ..
            } if identity.producer_role == producer_role => return Ok(true),
            SemanticExpressionKind::Call {
                call, arguments, ..
            } => {
                let definition = execution
                    .calls
                    .get(call.as_usize())
                    .filter(|definition| definition.id == *call)
                    .ok_or_else(|| {
                        SemanticError::new(format!(
                            "semantic dependency closure references missing call {call}"
                        ))
                    })?;
                if definition
                    .external_identity
                    .is_some_and(|identity| identity.producer_role == producer_role)
                {
                    return Ok(true);
                }
                pending.extend(arguments.iter().map(|argument| argument.value));
            }
            SemanticExpressionKind::CanonicalRead { target, .. }
            | SemanticExpressionKind::Drain { target, .. } => {
                pending.extend(
                    execution
                        .statements
                        .iter()
                        .filter(|statement| statement.declaration == Some(*target))
                        .filter_map(|statement| statement.value),
                );
            }
            SemanticExpressionKind::LocalRead { binding, .. } => {
                pending.extend(execution.expressions.iter().flat_map(|expression| {
                    match &expression.kind {
                        SemanticExpressionKind::Block { bindings, .. } => bindings
                            .iter()
                            .filter(|candidate| candidate.id == *binding)
                            .map(|candidate| candidate.value)
                            .collect::<Vec<_>>(),
                        _ => Vec::new(),
                    }
                }));
            }
            SemanticExpressionKind::TextTemplate { segments } => {
                pending.extend(segments.iter().filter_map(|segment| match segment {
                    SemanticTextSegment::Dynamic { value } => Some(*value),
                    SemanticTextSegment::Static { .. } => None,
                }));
            }
            SemanticExpressionKind::TaggedObject { fields, .. }
            | SemanticExpressionKind::Object(fields) => {
                pending.extend(fields.iter().map(|field| field.value));
            }
            SemanticExpressionKind::Block { bindings, result } => {
                pending.extend(bindings.iter().map(|binding| binding.value));
                pending.push(*result);
            }
            SemanticExpressionKind::Materialize { materialization } => {
                let definition = execution
                    .materializations
                    .get(materialization.as_usize())
                    .filter(|definition| definition.id == *materialization)
                    .ok_or_else(|| {
                        SemanticError::new(format!(
                            "semantic dependency closure references missing materialization {materialization}"
                        ))
                    })?;
                pending.extend(definition.expression_roots());
            }
            SemanticExpressionKind::Flush { payload: input }
            | SemanticExpressionKind::FlushBoundary { input }
            | SemanticExpressionKind::Draining { input }
            | SemanticExpressionKind::Project { input, .. } => pending.push(*input),
            SemanticExpressionKind::Hold {
                initial, updates, ..
            } => {
                pending.push(*initial);
                pending.extend(updates.iter().copied());
            }
            SemanticExpressionKind::Latest { branches } => {
                pending.extend(branches.iter().copied());
            }
            SemanticExpressionKind::When { input, arms, .. } => {
                pending.push(*input);
                pending.extend(arms.iter().map(|arm| arm.output));
            }
            SemanticExpressionKind::Then { input, output } => {
                pending.push(*input);
                pending.extend(*output);
            }
            SemanticExpressionKind::Infix { left, right, .. } => {
                pending.push(*left);
                pending.push(*right);
            }
            SemanticExpressionKind::MapEntry { key, value } => {
                pending.push(*key);
                pending.push(*value);
            }
            SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => pending.push(*output),
            SemanticExpressionKind::List { items, .. }
            | SemanticExpressionKind::Bytes { items, .. }
            | SemanticExpressionKind::Map { entries: items }
            | SemanticExpressionKind::Set { items } => {
                pending.extend(items.iter().copied());
            }
            SemanticExpressionKind::ExternalRead { .. }
            | SemanticExpressionKind::ElementState { .. }
            | SemanticExpressionKind::Text(_)
            | SemanticExpressionKind::Number(_)
            | SemanticExpressionKind::Bits(_)
            | SemanticExpressionKind::BytesByte(_)
            | SemanticExpressionKind::Absent
            | SemanticExpressionKind::Tag(_)
            | SemanticExpressionKind::Source { .. }
            | SemanticExpressionKind::Delimiter
            | SemanticExpressionKind::MaterializationLocal { .. }
            | SemanticExpressionKind::FunctionParameter { .. }
            | SemanticExpressionKind::MatchArm { output: None, .. } => {}
        }
    }
    Ok(false)
}

type BundleSemanticClosureV1 = (
    Vec<BundleSemanticRoleDigestV1>,
    Vec<BundleProducerMaterializationRequestV1>,
    Vec<BundleSemanticCallCrossingV1>,
    Vec<BundleSemanticValueCrossingV1>,
);

fn exact_bundle_producer_expression(
    producer: &SemanticProgram,
    declaration: DeclId,
    canonical_path: &str,
) -> Result<SemanticExprId, SemanticError> {
    let candidates = producer
        .reactive_graph()
        .bindings
        .iter()
        .filter(|binding| binding.declaration == declaration && binding.call_instance.is_none())
        .map(|binding| {
            let priority = match binding.target {
                SemanticBindingTargetV1::State { .. } | SemanticBindingTargetV1::List { .. } => {
                    0_u8
                }
                SemanticBindingTargetV1::Field { .. } => 1,
                SemanticBindingTargetV1::Source { .. } => 2,
            };
            (priority, binding.producer)
        })
        .collect::<Vec<_>>();
    let Some(priority) = candidates.iter().map(|(priority, _)| *priority).min() else {
        return Err(SemanticError::new(format!(
            "external value read `{canonical_path}` target declaration {} has no root semantic binding",
            declaration.0
        )));
    };
    let producers = candidates
        .into_iter()
        .filter(|(candidate, _)| *candidate == priority)
        .map(|(_, producer)| producer)
        .collect::<BTreeSet<_>>();
    let mut exact = producers.iter().copied();
    let Some(producer) = exact.next() else {
        return Err(SemanticError::new(format!(
            "external value read `{canonical_path}` target declaration {} maps to {} equally authoritative producer semantic values",
            declaration.0,
            producers.len()
        )));
    };
    if exact.next().is_some() {
        return Err(SemanticError::new(format!(
            "external value read `{canonical_path}` target declaration {} maps to {} equally authoritative producer semantic values",
            declaration.0,
            producers.len()
        )));
    }
    Ok(producer)
}

fn derive_bundle_call_closure(
    programs: &[SemanticProgram; 3],
) -> Result<BundleSemanticClosureV1, SemanticError> {
    let role_digests = programs
        .iter()
        .map(|program| BundleSemanticRoleDigestV1 {
            role: program.role(),
            semantic_program_digest: program.digest(),
        })
        .collect::<Vec<_>>();
    let mut actual_requests = programs
        .iter()
        .flat_map(|program| {
            program
                .producer_materializations
                .iter()
                .cloned()
                .map(|request| BundleProducerMaterializationRequestV1 {
                    role: program.role(),
                    request,
                })
        })
        .collect::<Vec<_>>();
    actual_requests.sort();
    validate_bundle_collection_count(
        "producer requests",
        actual_requests.len(),
        MAX_BUNDLE_SEMANTIC_PRODUCER_REQUESTS_V1,
    )?;
    if actual_requests.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(SemanticError::new(
            "bundle semantic producer request set contains duplicates",
        ));
    }

    let mut crossings = Vec::new();
    let mut expected_requests = Vec::new();
    let mut crossing_identities = BTreeSet::new();
    for consumer in programs {
        let owned_request_identities = consumer
            .producer_materializations
            .iter()
            .map(|request| request.identity)
            .collect::<BTreeSet<_>>();
        for occurrence in distributed_call_occurrences(consumer)? {
            if let DistributedCallOccurrenceRoot::Producer(identity) = occurrence.root
                && !owned_request_identities.contains(&identity)
            {
                return Err(SemanticError::new(format!(
                    "distributed occurrence `{}` names producer root {} not owned by its {} semantic program",
                    occurrence.occurrence_path,
                    digest_hex(&identity),
                    consumer.role().namespace()
                )));
            }
            if !crossing_identities.insert(occurrence.producer_materialization_identity) {
                return Err(SemanticError::new(format!(
                    "distributed occurrence `{}` duplicates producer materialization identity {}",
                    occurrence.occurrence_path,
                    digest_hex(&occurrence.producer_materialization_identity)
                )));
            }
            let local_function = occurrence
                .canonical_function
                .strip_prefix(&format!("{}/", occurrence.producer_role.namespace()))
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "qualified function `{}` has the wrong producer role prefix",
                        occurrence.canonical_function
                    ))
                })?
                .to_owned();
            let producer = programs
                .iter()
                .find(|program| program.role() == occurrence.producer_role)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "distributed occurrence `{}` references missing {} producer role",
                        occurrence.occurrence_path,
                        occurrence.producer_role.namespace()
                    ))
                })?;
            let call_definition = consumer
                .execution_graph()
                .calls
                .get(occurrence.call.as_usize())
                .filter(|call| call.id == occurrence.call)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "distributed occurrence `{}` references missing semantic call {}",
                        occurrence.occurrence_path, occurrence.call
                    ))
                })?;
            let external_identity = call_definition.external_identity.ok_or_else(|| {
                SemanticError::new(format!(
                    "distributed call occurrence `{}` has no sealed external identity",
                    occurrence.occurrence_path
                ))
            })?;
            if external_identity.kind != boon_checked::CheckedExternalDeclarationKind::Callable
                || external_identity.producer_role != occurrence.producer_role
                || external_identity.producer_source_bundle_digest_v1
                    != producer.source_bundle_digest_v1()
            {
                return Err(SemanticError::new(format!(
                    "distributed call occurrence `{}` has an incompatible sealed external identity",
                    occurrence.occurrence_path
                )));
            }
            let producer_callable = producer
                .execution_graph()
                .callables
                .iter()
                .find(|callable| {
                    callable.checked_callable == external_identity.producer_declaration
                })
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "distributed call occurrence `{}` target declaration {} has no producer semantic callable",
                        occurrence.occurrence_path,
                        external_identity.producer_declaration.0
                    ))
                })?
                .id;
            let producer_callable_definition =
                &producer.execution_graph().callables[producer_callable.as_usize()];
            if producer_callable_definition.kind != boon_checked::CheckedCallableKind::User
                || producer_callable_definition.name != local_function
            {
                return Err(SemanticError::new(format!(
                    "distributed call occurrence `{}` sealed target {} differs from diagnostic function `{local_function}`",
                    occurrence.occurrence_path, producer_callable
                )));
            }
            let consumer_callable_definition = consumer
                .execution_graph()
                .callables
                .get(occurrence.callable.as_usize())
                .filter(|callable| callable.id == occurrence.callable)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "distributed occurrence `{}` references missing consumer callable {}",
                        occurrence.occurrence_path, occurrence.callable
                    ))
                })?;
            if consumer_callable_definition.kind != boon_checked::CheckedCallableKind::External
                || consumer_callable_definition.external_identity != Some(external_identity)
            {
                return Err(SemanticError::new(format!(
                    "distributed occurrence `{}` consumer callable is not the sealed external target",
                    occurrence.occurrence_path
                )));
            }
            let (call_expression, call_origin, call_instance) =
                exact_call_expression_for_occurrence(consumer, &occurrence)?;
            let arguments = exact_bundle_call_arguments(
                consumer,
                &occurrence,
                call_definition,
                call_expression,
                consumer_callable_definition,
                producer_callable_definition,
            )?;
            let consumer_scope = exact_semantic_scope(
                consumer,
                call_origin.checked_scope,
                &format!("distributed occurrence `{}`", occurrence.occurrence_path),
            )?;
            let route_scope = bundle_call_route_scope(consumer.role(), occurrence.producer_role)?;
            let invocation_schedules = consumer
                .reactive_graph()
                .call_invocations
                .iter()
                .filter(|schedule| schedule.expression == call_expression.id)
                .collect::<Vec<_>>();
            let [invocation_schedule] = invocation_schedules.as_slice() else {
                return Err(SemanticError::new(format!(
                    "distributed occurrence `{}` maps to {} semantic invocation schedules",
                    occurrence.occurrence_path,
                    invocation_schedules.len()
                )));
            };
            let invocation_arms = consumer
                .reactive_graph()
                .invocation_arms_for_call_expression(call_expression.id)
                .map_err(|error| SemanticError::new(error.to_string()))?;
            let derived_mode = if invocation_schedule.current_capable && invocation_arms.is_empty()
            {
                ProducerMaterializationMode::Current
            } else {
                ProducerMaterializationMode::Invocation
            };
            if invocation_schedule.call != occurrence.call
                || invocation_schedule.value != call_expression.value_id
                || derived_mode != occurrence.mode
            {
                return Err(SemanticError::new(format!(
                    "distributed occurrence `{}` mode or invocation schedule differs from its exact semantic dependencies",
                    occurrence.occurrence_path
                )));
            }
            let request = ProducerMaterializationRequest {
                identity: occurrence.producer_materialization_identity,
                callable: producer_callable,
                local_function: local_function.clone(),
                mode: derived_mode,
            };
            expected_requests.push(BundleProducerMaterializationRequestV1 {
                role: occurrence.producer_role,
                request,
            });
            validate_bundle_collection_count(
                "call crossings",
                crossings.len().saturating_add(1),
                MAX_BUNDLE_SEMANTIC_CALL_CROSSINGS_V1,
            )?;
            crossings.push(BundleSemanticCallCrossingV1 {
                consumer_role: consumer.role(),
                consumer_call: occurrence.call,
                consumer_callable: occurrence.callable,
                consumer_expression: call_expression.id,
                consumer_value: call_expression.value_id,
                consumer_instance: call_instance,
                owner_callable: call_definition.owner_callable,
                owner: call_expression.owner,
                consumer_scope,
                producer_role: occurrence.producer_role,
                producer_callable,
                external_identity,
                producer_materialization_identity: occurrence.producer_materialization_identity,
                root: occurrence.root,
                call_path: occurrence.call_path,
                occurrence_path: occurrence.occurrence_path,
                canonical_function: occurrence.canonical_function,
                local_function,
                result: call_definition.result.clone(),
                effect: call_definition.effect,
                arguments,
                invocation_arms,
                mode: derived_mode,
                route_scope,
            });
        }
    }
    expected_requests.sort();
    crossings.sort_by_key(|crossing| {
        (
            crossing.consumer_role,
            crossing.occurrence_path.clone(),
            crossing.consumer_call,
            crossing.consumer_expression,
            crossing.producer_role,
            crossing.producer_callable,
        )
    });
    if expected_requests != actual_requests {
        return Err(SemanticError::new(
            "bundle semantic call crossings and producer requests are not in exact 1:1 correspondence",
        ));
    }

    let mut value_crossings = Vec::new();
    for consumer in programs {
        for occurrence in distributed_value_occurrences(consumer)? {
            let expression = consumer
                .execution_graph()
                .expressions
                .get(occurrence.expression.as_usize())
                .filter(|expression| {
                    expression.id == occurrence.expression
                        && expression.value_id == occurrence.value
                        && expression.checked_expr_id == occurrence.checked_expression
                })
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "distributed value occurrence {} references a missing or inconsistent consumer expression",
                        occurrence.identity
                    ))
                })?;
            let canonical_path = occurrence.canonical_path.clone();
            let external_identity = occurrence.external_identity;
            if external_identity.kind != boon_checked::CheckedExternalDeclarationKind::Value
                || external_identity.producer_role != occurrence.producer_role
            {
                return Err(SemanticError::new(format!(
                    "{} external value read `{canonical_path}` has a non-value or mismatched identity",
                    consumer.role().namespace()
                )));
            }
            let SemanticExpressionKind::ExternalRead {
                canonical_path: expression_path,
                external_identity: expression_identity,
            } = &expression.kind
            else {
                return Err(SemanticError::new(format!(
                    "distributed value occurrence {} consumer expression is not an external read",
                    occurrence.identity
                )));
            };
            if expression_path != &canonical_path || *expression_identity != Some(external_identity)
            {
                return Err(SemanticError::new(format!(
                    "{} external value read `{canonical_path}` differs from its structural occurrence",
                    consumer.role().namespace()
                )));
            }
            let producer = programs
                .iter()
                .find(|program| program.role() == external_identity.producer_role)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "external value read `{canonical_path}` references missing {} producer role",
                        external_identity.producer_role.namespace()
                    ))
                })?;
            if producer.source_bundle_digest_v1()
                != external_identity.producer_source_bundle_digest_v1
            {
                return Err(SemanticError::new(format!(
                    "external value read `{canonical_path}` producer source digest is stale"
                )));
            }
            let producer_expression = exact_bundle_producer_expression(
                producer,
                external_identity.producer_declaration,
                &canonical_path,
            )?;
            let producer_expression_definition = producer
                .execution_graph()
                .expressions
                .get(producer_expression.as_usize())
                .filter(|candidate| candidate.id == producer_expression)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "external value read `{canonical_path}` target expression {producer_expression} is missing"
                    ))
                })?;
            // A producer-owned current is Continuous inside its authority, but
            // its consumer-side import is PresentOrAbsent until the first
            // transport snapshot arrives (and again across reconnect). The
            // sealed delivery contract below owns that presence distinction;
            // the public data type itself must still match exactly.
            if producer_expression_definition.flow_type.ty != expression.flow_type.ty {
                return Err(SemanticError::new(format!(
                    "external value read `{canonical_path}` data type {:?} differs from sealed producer value {:?}",
                    expression.flow_type.ty, producer_expression_definition.flow_type.ty
                )));
            }
            let consumer_origin = consumer
                .execution_graph()
                .checked_expression_origins
                .get(expression.id.as_usize())
                .filter(|origin| {
                    origin.expression == expression.id
                        && origin.checked_expression == occurrence.checked_expression
                })
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "external value read `{canonical_path}` has no exact semantic origin"
                    ))
                })?;
            let consumer_scope = exact_semantic_scope(
                consumer,
                consumer_origin.checked_scope,
                &format!("external value read `{canonical_path}`"),
            )?;
            let delivery = exact_bundle_value_delivery(producer, producer_expression)?;
            let producer_origin_scoped = producer.role() == boon_checked::ProgramRole::Server
                && matches!(delivery, BundleSemanticValueDeliveryV1::Current)
                && semantic_expression_depends_on_role(
                    producer,
                    producer_expression,
                    boon_checked::ProgramRole::Session,
                )?;
            let route_scope = bundle_value_route_scope(
                consumer.role(),
                external_identity.producer_role,
                &delivery,
                producer_origin_scoped,
            )?;
            validate_bundle_collection_count(
                "value crossings",
                value_crossings.len().saturating_add(1),
                MAX_BUNDLE_SEMANTIC_VALUE_CROSSINGS_V1,
            )?;
            value_crossings.push(BundleSemanticValueCrossingV1 {
                occurrence_identity: occurrence.identity,
                root: occurrence.root,
                call_path: occurrence.call_path,
                checked_expression: occurrence.checked_expression,
                occurrence_path: occurrence.occurrence_path,
                consumer_role: consumer.role(),
                consumer_expression: expression.id,
                consumer_value: expression.value_id,
                consumer_scope,
                producer_role: external_identity.producer_role,
                producer_declaration: external_identity.producer_declaration,
                producer_expression,
                producer_value: producer_expression_definition.value_id,
                external_identity,
                canonical_path,
                flow_type: expression.flow_type.clone(),
                delivery,
                route_scope,
            });
        }
    }
    value_crossings.sort_by_key(|crossing| {
        (
            crossing.occurrence_identity,
            crossing.consumer_role,
            crossing.consumer_expression,
            crossing.producer_role,
            crossing.producer_declaration,
            crossing.producer_expression,
            crossing.canonical_path.clone(),
        )
    });
    validate_bundle_closure_bounds(&actual_requests, &crossings, &value_crossings)?;
    Ok((role_digests, actual_requests, crossings, value_crossings))
}

fn validate_bundle_closure_bounds(
    producer_requests: &[BundleProducerMaterializationRequestV1],
    call_crossings: &[BundleSemanticCallCrossingV1],
    value_crossings: &[BundleSemanticValueCrossingV1],
) -> Result<(), SemanticError> {
    validate_bundle_collection(
        "producer requests",
        producer_requests,
        MAX_BUNDLE_SEMANTIC_PRODUCER_REQUESTS_V1,
        MAX_BUNDLE_SEMANTIC_PRODUCER_REQUEST_BYTES_V1,
    )?;
    validate_bundle_collection(
        "call crossings",
        call_crossings,
        MAX_BUNDLE_SEMANTIC_CALL_CROSSINGS_V1,
        MAX_BUNDLE_SEMANTIC_CALL_CROSSING_BYTES_V1,
    )?;
    validate_bundle_collection(
        "value crossings",
        value_crossings,
        MAX_BUNDLE_SEMANTIC_VALUE_CROSSINGS_V1,
        MAX_BUNDLE_SEMANTIC_VALUE_CROSSING_BYTES_V1,
    )
}

fn validate_bundle_collection<T: Serialize>(
    kind: &str,
    values: &[T],
    max_count: usize,
    max_encoded_bytes: usize,
) -> Result<(), SemanticError> {
    validate_bundle_collection_count(kind, values.len(), max_count)?;
    let encoded = boon_contract::canonical_serde_cbor_v1(values).map_err(|error| {
        SemanticError::new(format!(
            "canonical bundle semantic {kind} encoding failed: {error}"
        ))
    })?;
    validate_bundle_collection_encoded_size(kind, encoded.len(), max_encoded_bytes)
}

fn validate_bundle_collection_count(
    kind: &str,
    count: usize,
    max_count: usize,
) -> Result<(), SemanticError> {
    if count > max_count {
        return Err(SemanticError::new(format!(
            "bundle semantic {kind} count {count} exceeds V1 limit {max_count}"
        )));
    }
    Ok(())
}

fn validate_bundle_collection_encoded_size(
    kind: &str,
    encoded_bytes: usize,
    max_encoded_bytes: usize,
) -> Result<(), SemanticError> {
    if encoded_bytes > max_encoded_bytes {
        return Err(SemanticError::new(format!(
            "bundle semantic {kind} canonical encoding is {encoded_bytes} bytes; V1 limit is {max_encoded_bytes} bytes"
        )));
    }
    Ok(())
}

fn bundle_semantic_program_digest(
    bundle: &BundleSemanticProgramV1,
) -> Result<BundleSemanticProgramDigestV1, SemanticError> {
    Ok(BundleSemanticProgramDigestV1(canonical_hash(
        BUNDLE_SEMANTIC_PROGRAM_DIGEST_DOMAIN,
        &(
            &bundle.schema,
            &bundle.role_digests,
            &bundle.producer_requests,
            &bundle.call_crossings,
            &bundle.value_crossings,
        ),
    )?))
}

pub fn elaborate(
    checked_program: CheckedProgram,
    producer_materializations: &[ProducerMaterializationRequest],
) -> Result<SemanticProgram, SemanticError> {
    elaborate_with_external_event_identities(checked_program, producer_materializations, &[])
}

/// Elaborate one role with the exact sealed external declarations whose
/// producer deliveries were proven event-backed by the atomic bundle.
///
/// The ordinary single-program boundary passes an empty set. Distributed
/// compilation first freezes producer delivery, then re-elaborates all roles
/// through this boundary so current crossings cannot masquerade as SOURCE
/// triggers.
pub fn elaborate_with_external_event_identities(
    checked_program: CheckedProgram,
    producer_materializations: &[ProducerMaterializationRequest],
    external_event_identities: &[CheckedExternalDeclarationIdentityV1],
) -> Result<SemanticProgram, SemanticError> {
    elaborate_with_representation(
        checked_program,
        producer_materializations,
        external_event_identities,
        true,
    )
}

/// Builds the historical occurrence-specialized semantic projection for a
/// differential test oracle.
///
/// This entrypoint does not exist in ordinary builds. It must never be used as
/// a compiler/runtime fallback or as a production performance path.
#[cfg(feature = "test-flat-oracle")]
#[doc(hidden)]
pub fn elaborate_flat_test_oracle(
    checked_program: CheckedProgram,
    producer_materializations: &[ProducerMaterializationRequest],
) -> Result<SemanticProgram, SemanticError> {
    elaborate_with_representation(checked_program, producer_materializations, &[], false)
}

fn elaborate_with_representation(
    checked_program: CheckedProgram,
    producer_materializations: &[ProducerMaterializationRequest],
    external_event_identities: &[CheckedExternalDeclarationIdentityV1],
    retain_ordinary_calls: bool,
) -> Result<SemanticProgram, SemanticError> {
    let trace_elaboration = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
    macro_rules! elaboration_phase {
        ($name:literal, $expression:expr) => {{
            let started = trace_elaboration.then(std::time::Instant::now);
            if trace_elaboration {
                eprintln!(concat!("boon_semantic phase ", $name, ":start"));
            }
            let result = $expression;
            if let Some(started) = started {
                eprintln!(
                    concat!("boon_semantic phase ", $name, ":done elapsed_ms={:.3}"),
                    started.elapsed().as_secs_f64() * 1000.0,
                );
            }
            result
        }};
    }

    let (checked_program, checked_handoff) = checked_program.into_parts();
    let source_bundle_digest_v1 = checked_program.source_bundle_digest_v1;
    let role = checked_program.role;
    if trace_elaboration {
        eprintln!(
            "boon_semantic artifact checked_program scopes={} declarations={} statements={} expressions={} callables={} calls={} sources={} states={} lists={} handoff_projections={}",
            checked_program.scopes.len(),
            checked_program.declarations.len(),
            checked_program.statements.len(),
            checked_program.expressions.len(),
            checked_program.callables.len(),
            checked_program.calls.len(),
            checked_program.sources.len(),
            checked_program.states.len(),
            checked_program.lists.len(),
            checked_handoff.projections.len(),
        );
    }
    #[cfg(test)]
    let checked_program_oracle = checked_program.clone();
    let producer_materializations = elaboration_phase!(
        "canonical_producer_requests",
        canonical_producer_requests(producer_materializations)
    )?;
    elaboration_phase!(
        "validate_contextual_bindings",
        validate_contextual_bindings(&checked_program)
    )?;
    let producer_roots = elaboration_phase!(
        "resolve_producer_roots",
        resolve_producer_roots(&checked_program, &producer_materializations)
    )?;
    let verified_intent = elaboration_phase!("verified_semantic_intent", {
        let retained_definitions = retain_ordinary_calls
            .then(|| contextual_expansion::ordinary_callable_declarations(&checked_program))
            .unwrap_or_default();
        verified_intent::VerifiedSemanticIntentV1::build(
            &checked_program,
            &producer_roots,
            retained_definitions,
        )
    })
    .map_err(SemanticError::new)?;
    verified_intent.trace();
    let out_net = elaboration_phase!(
        "out_net",
        out_net::OutNet::<OutPortContractV1>::try_build_with_intent(
            &checked_program,
            producer_roots,
            &verified_intent,
            |call, _, entry| provisional_out_port_contract(&checked_program, call, entry),
            |kind, _, _, _, _| kind == boon_checked::CheckedCallableKind::Builtin,
        )
    )?;
    if out_net.has_errors() {
        return Err(SemanticError::new(
            out_net
                .diagnostics
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    let mut resolved_out_graph = out_net.graph;
    if trace_elaboration {
        let cumulative_substitution_count = resolved_out_graph
            .call_instances
            .iter()
            .map(|call| resolved_out_graph.type_substitution_count(call.id))
            .sum::<usize>();
        let local_substitution_count = resolved_out_graph
            .call_instances
            .iter()
            .map(|call| call.local_type_substitutions.len())
            .sum::<usize>();
        let maximum_substitution_count = resolved_out_graph
            .call_instances
            .iter()
            .map(|call| resolved_out_graph.type_substitution_count(call.id))
            .max()
            .unwrap_or(0);
        eprintln!(
            "boon_semantic artifact out_graph calls={} ports={} nets={} owners={} cumulative_substitutions={} local_substitutions={} max_substitutions_per_call={}",
            resolved_out_graph.call_instances.len(),
            resolved_out_graph.ports.len(),
            resolved_out_graph.nets.len(),
            resolved_out_graph.static_owners.len(),
            cumulative_substitution_count,
            local_substitution_count,
            maximum_substitution_count,
        );
    }
    elaboration_phase!(
        "resolve_out_contracts",
        resolve_out_contracts(&checked_program, &mut resolved_out_graph)
    )?;
    elaboration_phase!(
        "validate_out_contracts",
        validate_out_contracts(&checked_program, &resolved_out_graph)
    )?;
    let (
        materializations,
        materialization_expressions,
        expression_builder_indexes,
        required_ordinary_definitions,
    ) = elaboration_phase!(
        "derive_contextual_materializations",
        contextual_expansion::derive_contextual_materializations(
            &checked_program,
            &resolved_out_graph,
            verified_intent.retained_definitions(),
            retain_ordinary_calls,
        )
    )
    .map_err(|error| SemanticError::new(error.to_string()))?;
    let mut semantic_image_builder = elaboration_phase!(
        "derive_semantic_execution_graph",
        contextual_expansion::derive_semantic_execution_graph(
            &checked_program,
            checked_handoff,
            &resolved_out_graph,
            &materializations,
            materialization_expressions,
            &expression_builder_indexes,
            &required_ordinary_definitions,
            retain_ordinary_calls,
        )
    )
    .map_err(|error| SemanticError::new(error.to_string()))?;
    let prepared_resource_inputs = elaboration_phase!(
        "normalize_execution_resource_authorities",
        semantic_image_builder.normalize_resource_authorities(&checked_program)
    )
    .map_err(SemanticError::new)?;
    let execution_graph = semantic_image_builder.execution();
    if trace_elaboration {
        eprintln!(
            "boon_semantic artifact execution_graph scopes={} expressions={} origins={} statements={} callables={} calls={} call_occurrences={} sources={} states={} roots={} functions={} materializations={} static_owners={}",
            execution_graph.scopes.len(),
            execution_graph.expressions.len(),
            execution_graph.checked_expression_origins.len(),
            execution_graph.statements.len(),
            execution_graph.callables.len(),
            execution_graph.calls.len(),
            execution_graph.call_occurrences.len(),
            execution_graph.sources.len(),
            execution_graph.states.len(),
            execution_graph.roots.len(),
            execution_graph.functions.len(),
            execution_graph.materializations.len(),
            execution_graph.static_owners.len(),
        );
    }
    elaboration_phase!(
        "validate_checked_callable_and_call_inventory",
        contextual_expansion::validate_checked_callable_and_call_inventory(
            &checked_program,
            &execution_graph,
        )
    )
    .map_err(SemanticError::new)?;
    elaboration_phase!(
        "validate_checked_roots",
        execution_graph.validate_checked_roots(&checked_program)
    )
    .map_err(SemanticError::new)?;
    let semantic_image_builder = elaboration_phase!(
        "finalize_execution_image",
        semantic_image_builder.finalize_execution(&resolved_out_graph)
    )
    .map_err(SemanticError::new)?;
    let execution_graph = semantic_image_builder.execution();
    let resource_build = elaboration_phase!(
        "build_semantic_resource_graph",
        resource::build_semantic_resource_graph(
            &checked_program,
            &resolved_out_graph,
            execution_graph,
            prepared_resource_inputs,
        )
    )
    .map_err(SemanticError::new)?;
    let resource_graph = resource_build.graph;
    let resource_dependency_rows = resource_build.dependency_rows;
    if trace_elaboration {
        eprintln!(
            "boon_semantic construction_rows domain=resource rows={}",
            resource_dependency_rows.len()
        );
    }
    let reactive_graph = elaboration_phase!(
        "build_semantic_reactive_graph",
        reactive::build_semantic_reactive_graph_from_validated_inputs(
            &execution_graph,
            &resource_graph,
            &resolved_out_graph,
            external_event_identities,
        )
    )
    .map_err(|error| SemanticError::new(error.to_string()))?;
    let lowering_build = elaboration_phase!(
        "build_semantic_lowering_contract",
        lowering_contract::build_semantic_lowering_contract_with_dependency_rows(
            &checked_program,
            &execution_graph,
            &resource_graph,
            &reactive_graph,
        )
    )
    .map_err(|error| SemanticError::new(error.to_string()))?;
    let lowering_contract = lowering_build.contract;
    let lowering_dependency_rows = lowering_build.dependency_rows;
    if trace_elaboration {
        eprintln!(
            "boon_semantic construction_rows domain=lowering rows={}",
            lowering_dependency_rows.len()
        );
    }
    let scope_storage_graph = elaboration_phase!(
        "build_semantic_scope_storage_graph",
        storage_contract::build_semantic_scope_storage_graph_from_validated_inputs(
            &checked_program,
            &execution_graph,
            &resource_graph,
            &reactive_graph,
            &lowering_contract,
        )
    )
    .map_err(|error| SemanticError::new(error.to_string()))?;
    let view_binding_graph = elaboration_phase!(
        "build_semantic_view_binding_graph",
        build_semantic_view_binding_graph(
            &execution_graph,
            &resource_graph,
            &reactive_graph,
            &scope_storage_graph,
            &lowering_contract,
        )
    )
    .map_err(|error| SemanticError::new(error.to_string()))?;
    let memory_graph = elaboration_phase!(
        "build_semantic_memory_graph",
        build_semantic_memory_graph(
            &checked_program,
            &execution_graph,
            &resource_graph,
            &reactive_graph,
            &scope_storage_graph,
            &lowering_contract,
        )
    )
    .map_err(|error| SemanticError::new(error.to_string()))?;
    let canonical_core = elaboration_phase!(
        "build_canonical_program_core",
        core_lowering::build_canonical_program_core(
            &execution_graph,
            &resource_graph,
            &reactive_graph,
            &lowering_contract,
            &view_binding_graph,
            &scope_storage_graph,
            &memory_graph,
        )
    )
    .map_err(SemanticError::new)?;
    let mut semantic_image_builder = semantic_image_builder;
    elaboration_phase!(
        "finalize_executable_receipts",
        semantic_image_builder.finalize_executable_receipts(&canonical_core)
    )
    .map_err(SemanticError::new)?;
    let execution_graph = semantic_image_builder.execution();
    let dependency_build = elaboration_phase!(
        "build_callable_dependency_manifest",
        build_callable_dependency_manifest_v7(
            DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
            &checked_program,
            semantic_image_builder.checked_handoff(),
            semantic_image_builder.execution_handoff(),
            &producer_materializations,
            &resolved_out_graph,
            execution_graph,
            &resource_graph,
            &resource_dependency_rows,
            &reactive_graph,
            &lowering_contract,
            &lowering_dependency_rows,
            &view_binding_graph,
            &scope_storage_graph,
            &memory_graph,
        )
    )
    .map_err(|error| SemanticError::new(error.to_string()))?;
    let dependency_manifest = dependency_build.manifest;
    let request_graph = Arc::new(dependency_build.request_graph);
    #[cfg(test)]
    let execution_graph_oracle = (*execution_graph).clone();
    let semantic_image = elaboration_phase!("seal_semantic_image", semantic_image_builder.seal())
        .map_err(SemanticError::new)?;
    let mut semantic = SemanticProgram {
        source_bundle_digest_v1,
        role,
        semantic_image,
        #[cfg(test)]
        checked_program: checked_program_oracle,
        #[cfg(test)]
        execution_graph: execution_graph_oracle,
        producer_materializations,
        resolved_out_graph,
        resource_graph,
        reactive_graph,
        lowering_contract,
        view_binding_graph,
        scope_storage_graph,
        memory_graph,
        canonical_core,
        dependency_manifest,
        request_graph,
        digest: SemanticProgramDigestV1([0; 32]),
    };
    semantic.digest = elaboration_phase!(
        "semantic_program_digest",
        semantic_program_digest(&semantic)
    )?;
    elaboration_phase!(
        "semantic_validate_freshly_constructed",
        semantic.validate_freshly_constructed()
    )?;
    Ok(semantic)
}

fn provisional_out_port_contract(
    program: &CheckedProgramFields,
    call: &boon_checked::CheckedCall,
    entry: &boon_checked::CheckedCallEntry,
) -> Result<OutPortContractV1, SemanticError> {
    let formal = match entry {
        boon_checked::CheckedCallEntry::Input { formal, .. }
        | boon_checked::CheckedCallEntry::FreshOut { formal, .. }
        | boon_checked::CheckedCallEntry::ForwardOut { formal, .. } => *formal,
    };
    let callable = program
        .callables
        .iter()
        .find(|callable| callable.decl_id == call.callable)
        .ok_or_else(|| {
            SemanticError::new(format!(
                "checked call {} references missing callable {} while constructing its OUT contract",
                call.id.0, call.callable.0
            ))
        })?;
    let parameter = callable
        .parameters
        .iter()
        .find(|parameter| parameter.decl_id == formal)
        .ok_or_else(|| {
            SemanticError::new(format!(
                "checked call {} references missing formal {} while constructing its OUT contract",
                call.id.0, formal.0
            ))
        })?;
    let flow_type = boon_checked::FlowType {
        mode: parameter.flow_type.mode,
        ty: boon_checked::apply_checked_type_substitutions(
            &parameter.flow_type.ty,
            &call.type_substitutions,
        ),
    };
    let lexical_scope = program
        .expressions
        .iter()
        .find(|expression| expression.id == call.expression)
        .map(|expression| expression.scope_id)
        .ok_or_else(|| {
            SemanticError::new(format!(
                "checked call {} references missing expression {} while constructing its OUT contract",
                call.id.0, call.expression.0
            ))
        })?;
    let output_scope = match entry {
        boon_checked::CheckedCallEntry::FreshOut { scope_id, .. } => *scope_id,
        boon_checked::CheckedCallEntry::ForwardOut { target, .. } => {
            let declaration = program
                .declarations
                .iter()
                .find(|declaration| declaration.id == *target)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "checked call {} forwards OUT formal {} to missing declaration {}",
                        call.id.0, formal.0, target.0
                    ))
                })?;
            declaration.body_scope.ok_or_else(|| {
                SemanticError::new(format!(
                    "checked call {} forwards OUT formal {} to declaration {} without an output scope",
                    call.id.0, formal.0, target.0
                ))
            })?
        }
        boon_checked::CheckedCallEntry::Input { .. } => lexical_scope,
    };
    Ok(OutPortContractV1 {
        resolved_type: flow_type.ty.clone(),
        shape_digest: [0; 32],
        lexical_scope,
        output_scope,
        role: call.role,
        generation_identity: None,
        correlation_identity: None,
        presence: OutPresenceCompatibilityV1::from_mode(flow_type.mode),
        flow_type,
    })
}

fn out_contract_resolution_order(graph: &ResolvedOutGraph) -> Result<Vec<usize>, SemanticError> {
    // Nested contextual bodies can be allocated before the enclosing output
    // port whose item contract they project. Resolve those exact port
    // dependencies first instead of relying on allocation order.
    fn visit(
        graph: &ResolvedOutGraph,
        port_index: usize,
        states: &mut [u8],
        order: &mut Vec<usize>,
    ) -> Result<(), SemanticError> {
        match states.get(port_index).copied() {
            Some(2) => return Ok(()),
            Some(1) => {
                return Err(SemanticError::new(format!(
                    "OUT contract evaluation-port dependencies contain a cycle at port {port_index}"
                )));
            }
            Some(0) => {}
            _ => {
                return Err(SemanticError::new(format!(
                    "OUT contract resolution references missing port {port_index}"
                )));
            }
        }
        states[port_index] = 1;

        let port = graph.ports.get(port_index).ok_or_else(|| {
            SemanticError::new(format!(
                "OUT contract resolution references missing port {port_index}"
            ))
        })?;
        let call = graph
            .call_instances
            .get(port.call.as_usize())
            .filter(|call| call.id == port.call)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "OUT port {port_index} references missing call {}",
                    port.call
                ))
            })?;
        let dependencies = call
            .inputs
            .iter()
            .filter_map(|input| match input.value {
                out_net::OutInputValue::Checked(ScopedCheckedExpr {
                    evaluation_port: Some(dependency),
                    ..
                }) => Some(dependency),
                _ => None,
            })
            .filter(|dependency| {
                graph
                    .ports
                    .get(dependency.as_usize())
                    .is_none_or(|dependency_port| dependency_port.call != call.id)
            })
            .collect::<BTreeSet<_>>();
        for dependency in dependencies {
            let dependency_index = dependency.as_usize();
            if graph
                .ports
                .get(dependency_index)
                .is_none_or(|port| port.id != dependency)
            {
                return Err(SemanticError::new(format!(
                    "OUT port {port_index} references missing evaluation port {dependency}"
                )));
            }
            visit(graph, dependency_index, states, order)?;
        }

        states[port_index] = 2;
        order.push(port_index);
        Ok(())
    }

    let mut states = vec![0; graph.ports.len()];
    let mut order = Vec::with_capacity(graph.ports.len());
    for port_index in 0..graph.ports.len() {
        visit(graph, port_index, &mut states, &mut order)?;
    }
    Ok(order)
}

fn resolve_out_contracts(
    program: &CheckedProgramFields,
    graph: &mut ResolvedOutGraph,
) -> Result<(), SemanticError> {
    let resolution_order = out_contract_resolution_order(graph)?;
    for port_index in resolution_order {
        let (call_id, formal, net_id) = {
            let port = &graph.ports[port_index];
            (port.call, port.formal, port.net)
        };
        let instance = graph
            .call_instances
            .get(call_id.as_usize())
            .filter(|instance| instance.id == call_id)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "OUT port {port_index} references missing call instance {call_id}"
                ))
            })?;
        let callable = program
            .callables
            .iter()
            .find(|callable| callable.decl_id == instance.provenance.callable)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "OUT port {port_index} references missing callable {}",
                    instance.provenance.callable.0
                ))
            })?;
        let parameter = callable
            .parameters
            .iter()
            .find(|parameter| parameter.decl_id == formal)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "OUT port {port_index} references missing formal {}",
                    formal.0
                ))
            })?;
        let mut substitutions = graph.type_substitution_environment(call_id);
        let mut provisional_variables = instance
            .local_type_substitutions
            .iter()
            .map(|substitution| substitution.variable)
            .collect::<BTreeSet<_>>();
        let parent_substitutions = instance
            .parent
            .map(|parent| graph.type_substitution_environment(parent))
            .unwrap_or_default();
        let ordered_inputs = instance
            .inputs
            .iter()
            .filter(|input| {
                !matches!(
                    input.value,
                    out_net::OutInputValue::Checked(ScopedCheckedExpr {
                        evaluation_port: Some(_),
                        ..
                    })
                )
            })
            .chain(instance.inputs.iter().filter(|input| {
                matches!(
                    input.value,
                    out_net::OutInputValue::Checked(ScopedCheckedExpr {
                        evaluation_port: Some(_),
                        ..
                    })
                )
            }))
            .collect::<Vec<_>>();
        for input in ordered_inputs {
            let input_parameter = callable
                .parameters
                .iter()
                .find(|parameter| parameter.decl_id == input.formal)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "OUT call instance {call_id} references missing input formal {}",
                        input.formal.0
                    ))
                })?;
            let actual = match &input.value {
                out_net::OutInputValue::Checked(scoped) => {
                    let mut input_substitutions = parent_substitutions.clone();
                    if let Some(evaluation_call) = scoped
                        .evaluation_port
                        .and_then(|port| graph.ports.get(port.as_usize()))
                        .map(|port| port.call)
                    {
                        if evaluation_call == call_id {
                            input_substitutions = substitutions.clone();
                        } else if graph
                            .call_instances
                            .get(evaluation_call.as_usize())
                            .is_some_and(|instance| instance.id == evaluation_call)
                        {
                            let evaluation_substitutions =
                                graph.type_substitution_environment(evaluation_call);
                            merge_out_contract_substitutions(
                                &mut input_substitutions,
                                evaluation_substitutions,
                            );
                        }
                    }
                    concrete_checked_expression_type(
                        program,
                        graph,
                        *scoped,
                        &input_substitutions,
                        &mut BTreeSet::new(),
                    )
                    .map_err(|error| {
                        SemanticError::new(format!(
                            "OUT call instance {call_id} `{}` input `{}` with {} local and {} parent substitution(s): {error}",
                            callable.name,
                            input_parameter.name,
                            substitutions.len(),
                            parent_substitutions.len(),
                        ))
                    })?
                }
                out_net::OutInputValue::ProducerParameter { flow_type, .. } => {
                    apply_out_contract_substitutions(&flow_type.ty, &parent_substitutions)
                }
            };
            release_provisional_out_contract_bindings(
                &input_parameter.flow_type.ty,
                &actual,
                &mut substitutions,
                &mut provisional_variables,
            );
            unify_out_contract_type(&input_parameter.flow_type.ty, &actual, &mut substitutions)
                .map_err(|error| {
                    let provenance_line = program
                        .expressions
                        .iter()
                        .find(|expression| expression.id == instance.provenance.expression)
                        .map_or(0, |expression| expression.span.line);
                    SemanticError::new(format!(
                        "OUT call instance {call_id} `{}` at checked expression {} line {provenance_line} input `{}` pattern {:?} with {} substitution(s): {error}",
                        callable.name,
                        instance.provenance.expression.0,
                        input_parameter.name,
                        input_parameter.flow_type.ty,
                        substitutions.len(),
                    ))
                })?;
        }
        let substitutions = substitutions
            .into_iter()
            .map(|(variable, value)| boon_checked::CheckedTypeSubstitution { variable, value })
            .collect::<Vec<_>>();
        let resolved_type =
            boon_checked::apply_checked_type_substitutions(&parameter.flow_type.ty, &substitutions);
        if !out_contract_type_is_resolved(&resolved_type) {
            return Err(SemanticError::new(format!(
                "OUT port {port_index} has unresolved type {resolved_type:?}"
            )));
        }
        let flow_type = boon_checked::FlowType {
            mode: parameter.flow_type.mode,
            ty: resolved_type.clone(),
        };
        let lexical_scope = program
            .expressions
            .iter()
            .find(|expression| expression.id == instance.provenance.expression)
            .map(|expression| expression.scope_id)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "OUT call instance {call_id} references missing provenance expression {}",
                    instance.provenance.expression.0
                ))
            })?;
        let output_scope = graph.owner_scope_for_net(net_id).ok_or_else(|| {
            SemanticError::new(format!("OUT net {net_id} has no canonical output scope"))
        })?;
        let owner = graph.owner_for_net(net_id).ok_or_else(|| {
            SemanticError::new(format!(
                "OUT net {net_id} has no canonical generation owner"
            ))
        })?;
        let role = match instance.provenance.call_id {
            Some(checked_call) => program
                .calls
                .iter()
                .find(|call| call.id == checked_call)
                .map(|call| call.role)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "OUT call instance {call_id} references missing checked call {}",
                        checked_call.0
                    ))
                })?,
            None => callable.role,
        };
        graph.ports[port_index].contract = OutPortContractV1 {
            flow_type: flow_type.clone(),
            resolved_type: resolved_type.clone(),
            shape_digest: canonical_hash(OUT_PORT_SHAPE_DIGEST_DOMAIN, &resolved_type)?,
            lexical_scope,
            output_scope,
            role,
            generation_identity: Some(OutGenerationIdentityV1 {
                owner,
                output_scope,
            }),
            correlation_identity: Some(OutCorrelationIdentityV1 { net: net_id, owner }),
            presence: OutPresenceCompatibilityV1::from_mode(flow_type.mode),
        };
    }
    Ok(())
}

fn concrete_checked_expression_type(
    program: &CheckedProgramFields,
    graph: &ResolvedOutGraph,
    scoped: ScopedCheckedExpr,
    active_substitutions: &BTreeMap<boon_checked::TypeVar, boon_checked::Type>,
    visiting: &mut BTreeSet<(boon_checked::CheckedExprId, Option<OutCallInstanceId>)>,
) -> Result<boon_checked::Type, SemanticError> {
    let key = (scoped.expression, scoped.frame);
    if !visiting.insert(key) {
        return Err(SemanticError::new(format!(
            "OUT type resolution contains a cycle at checked expression {} in frame {:?}",
            scoped.expression.0, scoped.frame
        )));
    }
    let resolved = (|| {
        let expression = program
            .expressions
            .iter()
            .find(|expression| expression.id == scoped.expression)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "OUT type resolution references missing checked expression {}",
                    scoped.expression.0
                ))
            })?;
        match &expression.kind {
            boon_checked::CheckedExpressionKind::Call { call } => {
                let checked_call = program.calls.iter().find(|candidate| candidate.id == *call);
                let callable = checked_call.and_then(|call| {
                    program
                        .callables
                        .iter()
                        .find(|callable| callable.decl_id == call.callable)
                });
                let instance_id = match graph.call_instance_for_checked_call(*call, scoped.frame) {
                    Some(instance) => instance,
                    None => {
                        // Retained ordinary definitions deliberately omit
                        // pure direct calls from each concrete OUT frame. The
                        // checked expression still owns its exact occurrence
                        // type; use it when the active frame substitutions
                        // close every variable. Calls with OUT/context/effects
                        // are never eligible and must retain a concrete
                        // instance.
                        let instance_less_is_pure =
                            checked_call.zip(callable).is_some_and(|(call, callable)| {
                                graph.intentionally_elided_call(
                                    call.id,
                                    call.owner_callable,
                                    scoped.frame,
                                ) && call.expression == scoped.expression
                                    && callable.kind == boon_checked::CheckedCallableKind::User
                                    && call.entries.iter().all(|entry| {
                                        matches!(
                                            entry,
                                            boon_checked::CheckedCallEntry::Input { .. }
                                        )
                                    })
                                    && call.contexts.is_empty()
                                    && matches!(
                                        call.context_binding,
                                        boon_checked::CheckedContextBinding::None
                                    )
                                    && call.contextual_substitutions.is_empty()
                                    && callable.contexts.is_empty()
                                    && callable.context_formal.is_none()
                                    && callable.contextual_operation.is_none()
                                    && callable.effect
                                        == boon_checked::CheckedEffectSummary::default()
                            });
                        let occurrence = instance_less_is_pure.then(|| {
                            let mut substitutions = active_substitutions.clone();
                            if let Some(frame) = scoped.frame {
                                merge_out_contract_substitutions(
                                    &mut substitutions,
                                    graph.type_substitution_environment(frame),
                                );
                            }
                            let local_substitutions = checked_call
                                .into_iter()
                                .flat_map(|call| &call.type_substitutions)
                                .map(|substitution| {
                                    (
                                        substitution.variable,
                                        apply_out_contract_substitutions(
                                            &substitution.value,
                                            &substitutions,
                                        ),
                                    )
                                })
                                .collect::<Vec<_>>();
                            substitutions.extend(local_substitutions);
                            let expression_ty = apply_out_contract_substitutions(
                                &expression.flow_type.ty,
                                &substitutions,
                            );
                            let call_ty = checked_call.map(|call| {
                                apply_out_contract_substitutions(&call.result.ty, &substitutions)
                            });
                            (call_ty.as_ref() == Some(&expression_ty)).then_some(expression_ty)
                        });
                        let occurrence = occurrence.flatten();
                        if let Some(resolved) = occurrence.as_ref().filter(|occurrence| {
                            boon_checked::type_is_recursively_closed(occurrence)
                        }) {
                            return Ok(resolved.clone());
                        }
                        return Err(SemanticError::new(format!(
                            "CALL expression {} references missing OUT call instance for checked call {} function {:?} in frame {:?}; checked occurrence after {} substitution(s) is {:?}",
                            scoped.expression.0,
                            call.0,
                            checked_call.map(|call| call.function.as_str()),
                            scoped.frame,
                            active_substitutions.len(),
                            occurrence,
                        )));
                    }
                };
                let instance = graph
                    .call_instances
                    .get(instance_id.as_usize())
                    .filter(|instance| instance.id == instance_id)
                    .ok_or_else(|| {
                        SemanticError::new(format!(
                            "CALL expression {} references missing OUT call instance {instance_id}",
                            scoped.expression.0
                        ))
                    })?;
                if out_contract_type_is_resolved(&instance.result.ty) {
                    return Ok(instance.result.ty.clone());
                }
                let callable = program
                    .callables
                    .iter()
                    .find(|callable| callable.decl_id == instance.provenance.callable)
                    .ok_or_else(|| {
                        SemanticError::new(format!(
                            "CALL expression {} references missing callable {}",
                            scoped.expression.0, instance.provenance.callable.0
                        ))
                    })?;
                let mut substitutions = active_substitutions.clone();
                let instance_type_environment = graph.type_substitution_environment(instance_id);
                merge_out_contract_substitutions(&mut substitutions, instance_type_environment);
                let mut provisional_variables = instance
                    .local_type_substitutions
                    .iter()
                    .map(|substitution| substitution.variable)
                    .collect::<BTreeSet<_>>();
                let checked_result =
                    apply_out_contract_substitutions(&callable.result.ty, &substitutions);
                if out_contract_type_is_resolved(&checked_result) {
                    // The checked call already owns a concrete result contract.
                    // Re-walking unrelated monomorphic inputs here would
                    // duplicate typechecker-specific argument rules (for
                    // example render metadata syntax) without contributing any
                    // result substitutions.
                    return Ok(checked_result);
                }
                let ordered_inputs = instance
                    .inputs
                    .iter()
                    .filter(|input| {
                        !matches!(
                            input.value,
                            out_net::OutInputValue::Checked(ScopedCheckedExpr {
                                evaluation_port: Some(_),
                                ..
                            })
                        )
                    })
                    .chain(instance.inputs.iter().filter(|input| {
                        matches!(
                            input.value,
                            out_net::OutInputValue::Checked(ScopedCheckedExpr {
                                evaluation_port: Some(_),
                                ..
                            })
                        )
                    }))
                    .collect::<Vec<_>>();
                let mut deferred_inputs = Vec::new();
                for input in ordered_inputs {
                    let parameter = callable
                        .parameters
                        .iter()
                        .find(|parameter| parameter.decl_id == input.formal)
                        .ok_or_else(|| {
                            SemanticError::new(format!(
                                "CALL expression {} references missing input formal {} on `{}`",
                                scoped.expression.0, input.formal.0, callable.name
                            ))
                        })?;
                    let actual = match &input.value {
                        out_net::OutInputValue::Checked(input) => {
                            let mut input_substitutions = active_substitutions.clone();
                            if let Some(evaluation_call) = input
                                .evaluation_port
                                .and_then(|port| graph.ports.get(port.as_usize()))
                                .map(|port| port.call)
                            {
                                if evaluation_call == instance_id {
                                    input_substitutions = substitutions.clone();
                                } else if graph
                                    .call_instances
                                    .get(evaluation_call.as_usize())
                                    .is_some_and(|instance| instance.id == evaluation_call)
                                {
                                    let evaluation_substitutions =
                                        graph.type_substitution_environment(evaluation_call);
                                    merge_out_contract_substitutions(
                                        &mut input_substitutions,
                                        evaluation_substitutions,
                                    );
                                }
                            }
                            concrete_checked_expression_type(
                                program,
                                graph,
                                *input,
                                &input_substitutions,
                                visiting,
                            )?
                        }
                        out_net::OutInputValue::ProducerParameter { flow_type, .. } => {
                            apply_out_contract_substitutions(&flow_type.ty, active_substitutions)
                        }
                    };
                    if out_contract_type_contains_empty_list_placeholder(&actual) {
                        deferred_inputs.push((
                            parameter.name.clone(),
                            parameter.flow_type.ty.clone(),
                            actual,
                        ));
                        continue;
                    }
                    release_provisional_out_contract_bindings(
                        &parameter.flow_type.ty,
                        &actual,
                        &mut substitutions,
                        &mut provisional_variables,
                    );
                    unify_out_contract_type(&parameter.flow_type.ty, &actual, &mut substitutions)
                        .map_err(|error| {
                        SemanticError::new(format!(
                            "CALL expression {} `{}` input `{}`: {error}",
                            scoped.expression.0, callable.name, parameter.name
                        ))
                    })?;
                }
                for (parameter_name, parameter_type, actual) in deferred_inputs {
                    let expected =
                        apply_out_contract_substitutions(&parameter_type, &substitutions);
                    let contextual_actual =
                        contextualize_empty_list_placeholders(&actual, &expected).unwrap_or(actual);
                    release_provisional_out_contract_bindings(
                        &parameter_type,
                        &contextual_actual,
                        &mut substitutions,
                        &mut provisional_variables,
                    );
                    unify_out_contract_type(
                        &parameter_type,
                        &contextual_actual,
                        &mut substitutions,
                    )
                    .map_err(|error| {
                        SemanticError::new(format!(
                            "CALL expression {} `{}` input `{parameter_name}`: {error}",
                            scoped.expression.0, callable.name
                        ))
                    })?;
                }
                let result = apply_out_contract_substitutions(&callable.result.ty, &substitutions);
                if out_contract_type_is_resolved(&result) {
                    return Ok(result);
                }
                if callable.kind == boon_checked::CheckedCallableKind::User
                    && let Some(result_expression) = callable.result_expression
                {
                    return concrete_checked_expression_type(
                        program,
                        graph,
                        ScopedCheckedExpr {
                            expression: result_expression,
                            frame: Some(instance_id),
                            evaluation_port: None,
                            value_frame: scoped.value_frame,
                        },
                        &substitutions,
                        visiting,
                    );
                }
                Ok(result)
            }
            boon_checked::CheckedExpressionKind::Block {
                result: Some(result),
                ..
            }
            | boon_checked::CheckedExpressionKind::MatchArm {
                output: Some(result),
                ..
            }
            | boon_checked::CheckedExpressionKind::Then {
                output: Some(result),
                ..
            } => concrete_checked_expression_type(
                program,
                graph,
                ScopedCheckedExpr {
                    expression: *result,
                    frame: scoped.frame,
                    evaluation_port: scoped.evaluation_port,
                    value_frame: scoped.value_frame,
                },
                active_substitutions,
                visiting,
            ),
            boon_checked::CheckedExpressionKind::Object { fields } => {
                let mut concrete_fields = BTreeMap::new();
                let mut field_order = Vec::new();
                let mut open = false;
                for field in fields {
                    let field_type = concrete_checked_expression_type(
                        program,
                        graph,
                        ScopedCheckedExpr {
                            expression: field.value,
                            frame: scoped.frame,
                            evaluation_port: scoped.evaluation_port,
                            value_frame: scoped.value_frame,
                        },
                        active_substitutions,
                        visiting,
                    )?;
                    if field.spread {
                        let boon_checked::Type::Object(shape) = field_type else {
                            return Err(SemanticError::new(format!(
                                "OBJECT expression {} spread field `{}` has non-object concrete type {field_type:?}",
                                scoped.expression.0, field.name
                            )));
                        };
                        open |= shape.open;
                        for name in &shape.field_order {
                            let Some(ty) = shape.fields.get(name).cloned() else {
                                continue;
                            };
                            if !concrete_fields.contains_key(name) {
                                field_order.push(name.clone());
                            }
                            concrete_fields.insert(name.clone(), ty);
                        }
                    } else {
                        if !concrete_fields.contains_key(&field.name) {
                            field_order.push(field.name.clone());
                        }
                        concrete_fields.insert(field.name.clone(), field_type);
                    }
                }
                Ok(boon_checked::Type::object(boon_checked::ObjectShape {
                    fields: concrete_fields,
                    field_order,
                    open,
                }))
            }
            boon_checked::CheckedExpressionKind::TaggedObject { tag, fields } => {
                let mut concrete_fields = BTreeMap::new();
                let mut field_order = Vec::new();
                let mut open = false;
                for field in fields {
                    let field_type = concrete_checked_expression_type(
                        program,
                        graph,
                        ScopedCheckedExpr {
                            expression: field.value,
                            frame: scoped.frame,
                            evaluation_port: scoped.evaluation_port,
                            value_frame: scoped.value_frame,
                        },
                        active_substitutions,
                        visiting,
                    )?;
                    if field.spread {
                        let boon_checked::Type::Object(shape) = field_type else {
                            return Err(SemanticError::new(format!(
                                "tagged OBJECT expression {} spread field `{}` has non-object concrete type {field_type:?}",
                                scoped.expression.0, field.name
                            )));
                        };
                        open |= shape.open;
                        for name in &shape.field_order {
                            let Some(ty) = shape.fields.get(name).cloned() else {
                                continue;
                            };
                            if !concrete_fields.contains_key(name) {
                                field_order.push(name.clone());
                            }
                            concrete_fields.insert(name.clone(), ty);
                        }
                    } else {
                        if !concrete_fields.contains_key(&field.name) {
                            field_order.push(field.name.clone());
                        }
                        concrete_fields.insert(field.name.clone(), field_type);
                    }
                }
                Ok(boon_checked::Type::VariantSet(
                    vec![boon_checked::Variant::Tagged {
                        tag: tag.clone(),
                        fields: boon_checked::ObjectShape {
                            fields: concrete_fields,
                            field_order,
                            open,
                        }
                        .into(),
                    }]
                    .into(),
                ))
            }
            boon_checked::CheckedExpressionKind::When { arms, .. }
            | boon_checked::CheckedExpressionKind::While { arms, .. } => {
                concrete_checked_branch_expression_type(
                    program,
                    graph,
                    scoped,
                    arms,
                    active_substitutions,
                    visiting,
                )
            }
            boon_checked::CheckedExpressionKind::Latest { branches } => {
                concrete_checked_branch_expression_type(
                    program,
                    graph,
                    scoped,
                    branches,
                    active_substitutions,
                    visiting,
                )
            }
            boon_checked::CheckedExpressionKind::Passed { projection, .. } => {
                let frame = scoped.frame.ok_or_else(|| {
                    SemanticError::new(format!(
                        "PASSED expression {} has no concrete OUT call frame",
                        scoped.expression.0
                    ))
                })?;
                let instance = graph
                    .call_instances
                    .get(frame.as_usize())
                    .filter(|instance| instance.id == frame)
                    .ok_or_else(|| {
                        SemanticError::new(format!(
                            "PASSED expression {} references missing OUT call frame {frame}",
                            scoped.expression.0
                        ))
                    })?;
                let passed = instance.passed.ok_or_else(|| {
                    SemanticError::new(format!(
                        "PASSED expression {} has no exact contextual binding in OUT call frame {frame}",
                        scoped.expression.0
                    ))
                })?;
                let base = concrete_checked_expression_type(
                    program,
                    graph,
                    passed.value,
                    active_substitutions,
                    visiting,
                )?;
                project_out_contract_type(base, projection).map_err(|error| {
                    SemanticError::new(format!(
                        "PASSED expression {} in frame {frame}: {error}",
                        scoped.expression.0
                    ))
                })
            }
            boon_checked::CheckedExpressionKind::Read {
                target, projection, ..
            } => {
                if let Some(payload_type) = exact_checked_resource_projection_type(
                    program,
                    scoped.expression,
                    active_substitutions,
                )? {
                    return Ok(payload_type);
                }
                let mut expression_substitutions = active_substitutions.clone();
                let frame_type_environment = scoped
                    .frame
                    .map(|frame| {
                        graph
                            .call_instances
                            .get(frame.as_usize())
                            .filter(|instance| instance.id == frame)
                            .ok_or_else(|| {
                                SemanticError::new(format!(
                                    "READ expression {} references missing OUT call frame {frame}",
                                    scoped.expression.0
                                ))
                            })?;
                        Ok(graph.type_substitution_environment(frame))
                    })
                    .transpose()?
                    .unwrap_or_default();
                merge_out_contract_substitutions(
                    &mut expression_substitutions,
                    frame_type_environment,
                );
                let expression_substitutions = expression_substitutions
                    .into_iter()
                    .map(|(variable, value)| boon_checked::CheckedTypeSubstitution {
                        variable,
                        value,
                    })
                    .collect::<Vec<_>>();
                let expression_actual = boon_checked::apply_checked_type_substitutions(
                    &expression.flow_type.ty,
                    &expression_substitutions,
                );
                let output_actual = scoped
                    .evaluation_port
                    .map(|port_id| {
                        let port = graph
                            .ports
                            .get(port_id.as_usize())
                            .filter(|port| port.id == port_id)
                            .ok_or_else(|| {
                                SemanticError::new(format!(
                                    "READ expression {} references missing evaluation port {port_id}",
                                    scoped.expression.0
                                ))
                            })?;
                        let output = match port.binding {
                            out_net::OutPortBinding::Fresh { output, .. } => output,
                            out_net::OutPortBinding::Forward { target } => target,
                        };
                        if output != *target {
                            return Ok(None);
                        }
                        if out_contract_type_is_resolved(&port.contract.resolved_type) {
                            return Ok(Some(port.contract.resolved_type.clone()));
                        }
                        let instance = graph
                            .call_instances
                            .get(port.call.as_usize())
                            .filter(|instance| instance.id == port.call)
                            .ok_or_else(|| {
                                SemanticError::new(format!(
                                    "evaluation port {port_id} references missing call {}",
                                    port.call
                                ))
                            })?;
                        let callable = program
                            .callables
                            .iter()
                            .find(|callable| callable.decl_id == instance.provenance.callable)
                            .ok_or_else(|| {
                                SemanticError::new(format!(
                                    "evaluation port {port_id} references missing callable {}",
                                    instance.provenance.callable.0
                                ))
                            })?;
                        let parameter = callable
                            .parameters
                            .iter()
                            .find(|parameter| parameter.decl_id == port.formal)
                            .ok_or_else(|| {
                                SemanticError::new(format!(
                                    "evaluation port {port_id} references missing OUT formal {}",
                                    port.formal.0
                                ))
                        })?;
                        let mut output_substitutions = active_substitutions.clone();
                        let instance_type_environment =
                            graph.type_substitution_environment(port.call);
                        merge_out_contract_substitutions(
                            &mut output_substitutions,
                            instance_type_environment,
                        );
                        Ok(Some(apply_out_contract_substitutions(
                            &parameter.flow_type.ty,
                            &output_substitutions,
                        )))
                    })
                    .transpose()?
                    .flatten();
                if let Some(output_actual) = output_actual {
                    let projected_output = project_out_contract_type(output_actual, projection)
                        .map_err(|error| {
                            SemanticError::new(format!(
                                "READ expression {} evaluation-port projection: {error}",
                                scoped.expression.0
                            ))
                        })?;
                    if out_contract_type_is_resolved(&expression_actual)
                        && unify_out_contract_type(
                            &projected_output,
                            &expression_actual,
                            &mut BTreeMap::new(),
                        )
                        .is_ok()
                    {
                        // The checked expression may carry a stricter
                        // arm-local refinement than the port's concrete item
                        // contract.
                        return Ok(expression_actual);
                    }
                    // Builtin type variables are reused across call frames.
                    // The exact evaluation port owns this read; a flat nested
                    // substitution snapshot must not reinterpret its item.
                    return Ok(projected_output);
                }
                if out_contract_type_is_resolved(&expression_actual) {
                    // Checked read types are expression-local.  In particular,
                    // a read inside a tagged WHEN arm already carries the
                    // structurally narrowed payload type, while its declaration
                    // still owns the complete variant set.  Re-projecting the
                    // declaration here would discard that arm-local proof.
                    return Ok(expression_actual);
                }
                let frame_actual = scoped
                    .frame
                    .map(|frame| {
                        graph
                            .call_instances
                            .get(frame.as_usize())
                            .filter(|instance| instance.id == frame)
                            .ok_or_else(|| {
                                SemanticError::new(format!(
                                    "READ expression {} references missing OUT call frame {frame}",
                                    scoped.expression.0
                                ))
                            })
                    })
                    .transpose()?
                    .and_then(|instance| {
                        instance.inputs.iter().find(|input| input.formal == *target)
                    })
                    .map(|input| match &input.value {
                        out_net::OutInputValue::Checked(actual) => {
                            concrete_checked_expression_type(
                                program,
                                graph,
                                *actual,
                                active_substitutions,
                                visiting,
                            )
                        }
                        out_net::OutInputValue::ProducerParameter { flow_type, .. } => {
                            Ok(flow_type.ty.clone())
                        }
                    })
                    .transpose()?;
                let base = match frame_actual {
                    Some(actual) => actual,
                    None => program
                        .declarations
                        .iter()
                        .find(|declaration| declaration.id == *target)
                        .map(|declaration| declaration.flow_type.ty.clone())
                        .ok_or_else(|| {
                            SemanticError::new(format!(
                                "READ expression {} references missing declaration {}",
                                scoped.expression.0, target.0
                            ))
                        })?,
                };
                let target_detail = program
                    .declarations
                    .iter()
                    .find(|declaration| declaration.id == *target)
                    .map(|declaration| {
                        format!(
                            "`{}` ({:?}, scope {}, line {})",
                            declaration.name,
                            declaration.kind,
                            declaration.scope_id.0,
                            declaration.span.line
                        )
                    })
                    .unwrap_or_else(|| format!("<missing declaration {}>", target.0));
                let evaluation_detail = scoped
                    .evaluation_port
                    .and_then(|port| graph.ports.get(port.as_usize()))
                    .map(|port| {
                        let formal_type = graph
                            .call_instances
                            .get(port.call.as_usize())
                            .and_then(|instance| {
                                program.callables.iter().find(|callable| {
                                    callable.decl_id == instance.provenance.callable
                                })
                            })
                            .and_then(|callable| {
                                callable
                                    .parameters
                                    .iter()
                                    .find(|parameter| parameter.decl_id == port.formal)
                            })
                            .map(|parameter| format!("{:?}", parameter.flow_type.ty))
                            .unwrap_or_else(|| "<missing formal type>".to_owned());
                        format!(
                            "{} {:?} formal {} type {formal_type}",
                            port.id, port.binding, port.formal.0
                        )
                    })
                    .unwrap_or_else(|| "none".to_owned());
                let frame_detail = scoped
                    .frame
                    .and_then(|frame| graph.call_instances.get(frame.as_usize()))
                    .map(|instance| {
                        let callable = program
                            .callables
                            .iter()
                            .find(|callable| callable.decl_id == instance.provenance.callable)
                            .map(|callable| callable.name.as_str())
                            .unwrap_or("<missing callable>");
                        let input = instance
                            .inputs
                            .iter()
                            .find(|input| input.formal == *target)
                            .map(|input| match &input.value {
                                out_net::OutInputValue::Checked(actual) => format!(
                                    "checked {} frame {:?} evaluation {:?}",
                                    actual.expression.0, actual.frame, actual.evaluation_port
                                ),
                                out_net::OutInputValue::ProducerParameter { parameter, .. } => {
                                    format!("producer parameter {parameter:?}")
                                }
                            })
                            .unwrap_or_else(|| "missing target input".to_owned());
                        format!(
                            "{} `{callable}` parent {:?} checked expression {} with {} substitution(s), target input {input}",
                            instance.id,
                            instance.parent,
                            instance.provenance.expression.0,
                            graph.type_substitution_count(instance.id),
                        )
                    })
                    .unwrap_or_else(|| "none".to_owned());
                project_out_contract_type(base, projection).map_err(|error| {
                    SemanticError::new(format!(
                        "READ expression {} target {} {target_detail} in frame {frame_detail} under evaluation port {evaluation_detail}: {error}",
                        scoped.expression.0, target.0,
                    ))
                })
            }
            _ => {
                let mut substitutions = active_substitutions.clone();
                let frame_type_environment = scoped
                    .frame
                    .map(|frame| {
                        graph
                            .call_instances
                            .get(frame.as_usize())
                            .filter(|instance| instance.id == frame)
                            .ok_or_else(|| {
                                SemanticError::new(format!(
                                    "expression {} references missing OUT call frame {frame}",
                                    scoped.expression.0
                                ))
                            })?;
                        Ok(graph.type_substitution_environment(frame))
                    })
                    .transpose()?
                    .unwrap_or_default();
                merge_out_contract_substitutions(&mut substitutions, frame_type_environment);
                let substitutions = substitutions
                    .into_iter()
                    .map(|(variable, value)| boon_checked::CheckedTypeSubstitution {
                        variable,
                        value,
                    })
                    .collect::<Vec<_>>();
                Ok(boon_checked::apply_checked_type_substitutions(
                    &expression.flow_type.ty,
                    &substitutions,
                ))
            }
        }
    })();
    visiting.remove(&key);
    resolved
}

fn concrete_checked_branch_expression_type(
    program: &CheckedProgramFields,
    graph: &ResolvedOutGraph,
    scoped: ScopedCheckedExpr,
    branches: &[boon_checked::CheckedExprId],
    active_substitutions: &BTreeMap<boon_checked::TypeVar, boon_checked::Type>,
    visiting: &mut BTreeSet<(boon_checked::CheckedExprId, Option<OutCallInstanceId>)>,
) -> Result<boon_checked::Type, SemanticError> {
    let expression = program
        .expressions
        .iter()
        .find(|expression| expression.id == scoped.expression)
        .ok_or_else(|| {
            SemanticError::new(format!(
                "branch type resolution references missing checked expression {}",
                scoped.expression.0
            ))
        })?;
    // The checked occurrence is the authority for an already-closed branch
    // container. Descending into its arms cannot contribute any result type
    // substitutions, and child-owner expressions may carry independently
    // alpha-normalized variables that do not belong to this OUT frame. Only
    // trust the raw occurrence here: treating a type made closed by the flat
    // frame environment as authoritative could capture an unrelated alpha
    // with the same ordinal.
    if boon_checked::type_is_recursively_closed(&expression.flow_type.ty) {
        return Ok(expression.flow_type.ty.clone());
    }
    let mut substitutions = active_substitutions.clone();
    let frame_type_environment = scoped
        .frame
        .map(|frame| {
            graph
                .call_instances
                .get(frame.as_usize())
                .filter(|instance| instance.id == frame)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "branch expression {} references missing OUT call frame {frame}",
                        scoped.expression.0
                    ))
                })?;
            Ok(graph.type_substitution_environment(frame))
        })
        .transpose()?
        .unwrap_or_default();
    merge_out_contract_substitutions(&mut substitutions, frame_type_environment);

    let mut concrete_branches = Vec::new();
    for branch in branches {
        let branch_expression = program
            .expressions
            .iter()
            .find(|expression| expression.id == *branch)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "branch expression {} references missing branch {}",
                    scoped.expression.0, branch.0
                ))
            })?;
        let concrete = concrete_checked_expression_type(
            program,
            graph,
            ScopedCheckedExpr {
                expression: *branch,
                frame: scoped.frame,
                evaluation_port: scoped.evaluation_port,
                value_frame: scoped.value_frame,
            },
            &substitutions,
            visiting,
        )?;
        unify_out_contract_type(
            &branch_expression.flow_type.ty,
            &concrete,
            &mut substitutions,
        )
        .map_err(|error| {
            SemanticError::new(format!(
                "branch expression {} cannot resolve branch {}: {error}",
                scoped.expression.0, branch.0
            ))
        })?;
        concrete_branches.push(concrete);
    }

    let result = apply_out_contract_substitutions(&expression.flow_type.ty, &substitutions);
    if out_contract_type_is_resolved(&result) {
        return Ok(result);
    }
    if let Some(first) = concrete_branches.first()
        && concrete_branches
            .iter()
            .skip(1)
            .all(|candidate| candidate == first)
    {
        return Ok(first.clone());
    }
    Ok(result)
}

fn exact_checked_resource_projection_type(
    program: &CheckedProgramFields,
    expression: boon_checked::CheckedExprId,
    substitutions: &BTreeMap<boon_checked::TypeVar, boon_checked::Type>,
) -> Result<Option<boon_checked::Type>, SemanticError> {
    let requirements = program
        .resource_projection_requirements
        .iter()
        .filter(|requirement| requirement.expression == expression)
        .collect::<Vec<_>>();
    let requirement = match requirements.as_slice() {
        [] => return Ok(None),
        [requirement] => *requirement,
        _ => {
            return Err(SemanticError::new(format!(
                "checked expression {} has {} resource projection requirements",
                expression.0,
                requirements.len()
            )));
        }
    };
    if requirement.source_origins.is_empty() {
        return Ok(None);
    }

    let required_type = apply_out_contract_substitutions(&requirement.required_type, substitutions);
    let mut exact_type = None;
    for origin in &requirement.source_origins {
        let source = program
            .sources
            .get(origin.source.0 as usize)
            .filter(|source| source.id == origin.source)
            .ok_or_else(|| {
                SemanticError::new(format!(
                    "checked expression {} resource projection references missing source {}",
                    expression.0, origin.source.0
                ))
            })?;
        let source_type = apply_out_contract_substitutions(&source.payload_type, substitutions);
        let projected = project_out_contract_type(source_type, &origin.payload_projection)
            .map_err(|error| {
                SemanticError::new(format!(
                    "checked expression {} source {} payload projection {:?}: {error}",
                    expression.0, origin.source.0, origin.payload_projection
                ))
            })?;
        if !out_contract_type_is_resolved(&projected) {
            return Err(SemanticError::new(format!(
                "checked expression {} source {} payload projection has unresolved type {projected:?}",
                expression.0, origin.source.0
            )));
        }
        match &exact_type {
            Some(existing) if existing != &projected => {
                return Err(SemanticError::new(format!(
                    "checked expression {} source payload origins disagree: {existing:?} versus {projected:?}",
                    expression.0
                )));
            }
            Some(_) => {}
            None => exact_type = Some(projected),
        }
    }
    let exact_type = exact_type.expect("nonempty checked source origins");
    if out_contract_type_is_resolved(&required_type) && required_type != exact_type {
        let expression_detail = program
            .expressions
            .iter()
            .find(|candidate| candidate.id == expression)
            .map(|candidate| format!(" line {} kind {:?}", candidate.span.line, candidate.kind))
            .unwrap_or_default();
        let origins = requirement
            .source_origins
            .iter()
            .filter_map(|origin| {
                program
                    .sources
                    .get(origin.source.0 as usize)
                    .filter(|source| source.id == origin.source)
                    .map(|source| {
                        format!(
                            "source {} path {:?} line {} projection {:?}",
                            source.id.0, source.path, source.span.line, origin.payload_projection
                        )
                    })
            })
            .collect::<Vec<_>>();
        return Err(SemanticError::new(format!(
            "checked expression {}{expression_detail} resource requirement type {required_type:?} (raw {:?}) differs from exact source payload type {exact_type:?}; origins: {origins:?}",
            expression.0, requirement.required_type,
        )));
    }
    Ok(Some(exact_type))
}

fn apply_out_contract_substitutions(
    ty: &boon_checked::Type,
    substitutions: &BTreeMap<boon_checked::TypeVar, boon_checked::Type>,
) -> boon_checked::Type {
    let substitutions = substitutions
        .iter()
        .map(|(variable, value)| boon_checked::CheckedTypeSubstitution {
            variable: *variable,
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    boon_checked::apply_checked_type_substitutions(ty, &substitutions)
}

fn project_out_contract_type(
    mut ty: boon_checked::Type,
    fields: &[String],
) -> Result<boon_checked::Type, SemanticError> {
    for field in fields {
        let boon_checked::Type::Object(shape) = ty else {
            return Err(SemanticError::new(format!(
                "OUT type projection `{field}` requires an object, got {ty:?}"
            )));
        };
        ty = shape.fields.get(field).cloned().ok_or_else(|| {
            SemanticError::new(format!(
                "OUT type projection references missing object field `{field}`; available fields are {:?}",
                shape.field_order
            ))
        })?;
    }
    Ok(ty)
}

fn out_contract_type_contains_empty_list_placeholder(ty: &boon_checked::Type) -> bool {
    match ty {
        boon_checked::Type::UnresolvedShape { reason } => reason == "empty list item",
        boon_checked::Type::List(item) => out_contract_type_contains_empty_list_placeholder(item),
        boon_checked::Type::Map { key, value } => {
            out_contract_type_contains_empty_list_placeholder(key)
                || out_contract_type_contains_empty_list_placeholder(value)
        }
        boon_checked::Type::Set(item) => out_contract_type_contains_empty_list_placeholder(item),
        boon_checked::Type::Function { args, result } => {
            args.iter()
                .any(out_contract_type_contains_empty_list_placeholder)
                || out_contract_type_contains_empty_list_placeholder(&result.ty)
        }
        boon_checked::Type::Object(shape) => shape
            .fields
            .values()
            .any(out_contract_type_contains_empty_list_placeholder),
        boon_checked::Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            boon_checked::Variant::Tag(_) => false,
            boon_checked::Variant::Tagged { fields, .. } => fields
                .fields
                .values()
                .any(out_contract_type_contains_empty_list_placeholder),
        }),
        boon_checked::Type::Union(members) => members
            .iter()
            .any(out_contract_type_contains_empty_list_placeholder),
        boon_checked::Type::Var(_)
        | boon_checked::Type::Unknown
        | boon_checked::Type::Bits { .. }
        | boon_checked::Type::Text
        | boon_checked::Type::Number
        | boon_checked::Type::Bytes(_)
        | boon_checked::Type::Absent
        | boon_checked::Type::RenderContract => false,
    }
}

fn contextualize_empty_list_placeholders(
    actual: &boon_checked::Type,
    expected: &boon_checked::Type,
) -> Option<boon_checked::Type> {
    match (actual, expected) {
        (boon_checked::Type::UnresolvedShape { reason }, expected)
            if reason == "empty list item" && out_contract_type_is_resolved(expected) =>
        {
            Some(expected.clone())
        }
        (boon_checked::Type::List(actual), boon_checked::Type::List(expected)) => {
            Some(boon_checked::Type::List(boon_checked::Type::shared(
                contextualize_empty_list_placeholders(actual, expected)?,
            )))
        }
        (boon_checked::Type::Object(actual), boon_checked::Type::Object(expected)) => {
            let mut contextual = actual.as_ref().clone();
            for (name, actual_field) in &actual.fields {
                if !out_contract_type_contains_empty_list_placeholder(actual_field) {
                    continue;
                }
                let expected_field = expected.fields.get(name)?;
                contextual.fields.insert(
                    name.clone(),
                    contextualize_empty_list_placeholders(actual_field, expected_field)?,
                );
            }
            Some(boon_checked::Type::object(contextual))
        }
        (
            boon_checked::Type::Function {
                args: actual_args,
                result: actual_result,
            },
            boon_checked::Type::Function {
                args: expected_args,
                result: expected_result,
            },
        ) if actual_args.len() == expected_args.len() => Some(boon_checked::Type::Function {
            args: actual_args
                .iter()
                .zip(expected_args)
                .map(|(actual, expected)| contextualize_empty_list_placeholders(actual, expected))
                .collect::<Option<Vec<_>>>()?,
            result: Box::new(boon_checked::FlowType {
                mode: actual_result.mode,
                ty: contextualize_empty_list_placeholders(&actual_result.ty, &expected_result.ty)?,
            }),
        }),
        (boon_checked::Type::Union(actual), boon_checked::Type::Union(expected))
            if actual.len() == expected.len() =>
        {
            Some(boon_checked::Type::Union(
                actual
                    .iter()
                    .zip(expected)
                    .map(|(actual, expected)| {
                        contextualize_empty_list_placeholders(actual, expected)
                    })
                    .collect::<Option<Vec<_>>>()?,
            ))
        }
        (actual, _) if !out_contract_type_contains_empty_list_placeholder(actual) => {
            Some(actual.clone())
        }
        _ => None,
    }
}

/// Drops a definition-site call-frame binding immediately before the first
/// concrete occurrence input binds the same formal alpha.
///
/// Checked call substitutions are useful provisional schemes while an OUT
/// expression is being evaluated, but they are not co-authoritative with the
/// concrete invocation. In particular, a contextual output can shape an open
/// item scaffold or retain a broad branch-result union before its parent input
/// is known. The first concrete input for each scheme variable owns that
/// occurrence; subsequent inputs retain ordinary compatibility checking.
/// Traverse only pattern/actual pairs that the contract matcher itself can
/// align so an absent union branch cannot accidentally discard a needed seed.
fn release_provisional_out_contract_bindings(
    pattern: &boon_checked::Type,
    actual: &boon_checked::Type,
    substitutions: &mut BTreeMap<boon_checked::TypeVar, boon_checked::Type>,
    provisional_variables: &mut BTreeSet<boon_checked::TypeVar>,
) {
    match (pattern, actual) {
        (boon_checked::Type::Var(variable), _) => {
            if provisional_variables.remove(variable) {
                substitutions.remove(variable);
            }
        }
        (boon_checked::Type::List(pattern), boon_checked::Type::List(actual))
        | (boon_checked::Type::Set(pattern), boon_checked::Type::Set(actual)) => {
            release_provisional_out_contract_bindings(
                pattern,
                actual,
                substitutions,
                provisional_variables,
            );
        }
        (
            boon_checked::Type::Map {
                key: pattern_key,
                value: pattern_value,
            },
            boon_checked::Type::Map {
                key: actual_key,
                value: actual_value,
            },
        ) => {
            release_provisional_out_contract_bindings(
                pattern_key,
                actual_key,
                substitutions,
                provisional_variables,
            );
            release_provisional_out_contract_bindings(
                pattern_value,
                actual_value,
                substitutions,
                provisional_variables,
            );
        }
        (boon_checked::Type::Union(pattern), boon_checked::Type::Union(actual))
            if pattern.len() == actual.len() =>
        {
            for (pattern, actual) in pattern.iter().zip(actual) {
                release_provisional_out_contract_bindings(
                    pattern,
                    actual,
                    substitutions,
                    provisional_variables,
                );
            }
        }
        (boon_checked::Type::Object(pattern), boon_checked::Type::Object(actual)) => {
            for (name, pattern) in &pattern.fields {
                if let Some(actual) = actual.fields.get(name) {
                    release_provisional_out_contract_bindings(
                        pattern,
                        actual,
                        substitutions,
                        provisional_variables,
                    );
                }
            }
        }
        (boon_checked::Type::VariantSet(pattern), boon_checked::Type::VariantSet(actual)) => {
            for actual_variant in actual.iter() {
                let Some(boon_checked::Variant::Tagged {
                    fields: pattern_fields,
                    ..
                }) = pattern.iter().find(|pattern_variant| {
                    matches!(
                        (pattern_variant, actual_variant),
                        (
                            boon_checked::Variant::Tagged { tag: pattern, .. },
                            boon_checked::Variant::Tagged { tag: actual, .. }
                        ) if pattern == actual
                    )
                })
                else {
                    continue;
                };
                let boon_checked::Variant::Tagged {
                    fields: actual_fields,
                    ..
                } = actual_variant
                else {
                    continue;
                };
                for (name, pattern) in &pattern_fields.fields {
                    if let Some(actual) = actual_fields.fields.get(name) {
                        release_provisional_out_contract_bindings(
                            pattern,
                            actual,
                            substitutions,
                            provisional_variables,
                        );
                    }
                }
            }
        }
        (
            boon_checked::Type::Function {
                args: pattern_args,
                result: pattern_result,
            },
            boon_checked::Type::Function {
                args: actual_args,
                result: actual_result,
            },
        ) if pattern_args.len() == actual_args.len() => {
            for (pattern, actual) in pattern_args.iter().zip(actual_args) {
                release_provisional_out_contract_bindings(
                    pattern,
                    actual,
                    substitutions,
                    provisional_variables,
                );
            }
            release_provisional_out_contract_bindings(
                &pattern_result.ty,
                &actual_result.ty,
                substitutions,
                provisional_variables,
            );
        }
        _ => {}
    }
}

fn unify_out_contract_type(
    pattern: &boon_checked::Type,
    actual: &boon_checked::Type,
    substitutions: &mut BTreeMap<boon_checked::TypeVar, boon_checked::Type>,
) -> Result<(), SemanticError> {
    if !out_contract_type_is_resolved(actual) {
        return Err(SemanticError::new(format!(
            "OUT input has unresolved concrete type {actual:?}"
        )));
    }
    match (pattern, actual) {
        (boon_checked::Type::Var(variable), actual) => match substitutions.get(variable) {
            Some(existing) if out_contract_type_is_resolved(existing) && existing != actual => {
                if boon_checked::resolved_type_is_assignable_to(actual, existing) {
                    // Preserve the already-known wider bound. The new
                    // occurrence is a valid specialization of it.
                } else if boon_checked::resolved_type_is_assignable_to(existing, actual) {
                    // Input order must not decide a generic OUT contract. If
                    // an earlier occurrence installed a narrower closed
                    // variant/object and a later occurrence supplies its
                    // closed supertype, retain the supertype that admits both.
                    substitutions.insert(*variable, actual.clone());
                } else {
                    return Err(SemanticError::new(format!(
                        "OUT type variable {:?} has conflicting concrete types {existing:?} and {actual:?}",
                        variable
                    )));
                }
            }
            _ => {
                substitutions.insert(*variable, actual.clone());
            }
        },
        (boon_checked::Type::List(pattern), boon_checked::Type::List(actual)) => {
            unify_out_contract_type(pattern, actual, substitutions)
                .map_err(|error| SemanticError::new(format!("list item: {error}")))?;
        }
        (boon_checked::Type::Union(pattern), boon_checked::Type::Union(actual))
            if pattern.len() == actual.len() =>
        {
            // Branch result unions retain their checked member order while
            // frame substitutions close individual members. Recurse through
            // that structure so an existing alpha binding (for example
            // `T = True` in `T | False`) is applied at the leaf instead of
            // comparing the raw and substituted unions as unrelated concrete
            // types.
            for (index, (pattern, actual)) in pattern.iter().zip(actual).enumerate() {
                unify_out_contract_type(pattern, actual, substitutions).map_err(|error| {
                    SemanticError::new(format!("union member {index}: {error}"))
                })?;
            }
        }
        (boon_checked::Type::Object(pattern), boon_checked::Type::Object(actual)) => {
            for (name, pattern) in &pattern.fields {
                let Some(actual_field) = actual.fields.get(name) else {
                    if actual.open {
                        continue;
                    }
                    return Err(SemanticError::new(format!(
                        "OUT input object is missing required field `{name}`"
                    )));
                };
                unify_out_contract_type(pattern, actual_field, substitutions).map_err(|error| {
                    SemanticError::new(format!("object field `{name}`: {error}"))
                })?;
            }
        }
        (
            boon_checked::Type::VariantSet(pattern_variants),
            boon_checked::Type::VariantSet(actual_variants),
        ) => {
            for actual_variant in actual_variants {
                let matching = pattern_variants.iter().find(|pattern_variant| {
                    matches!(
                        (pattern_variant, actual_variant),
                        (
                            boon_checked::Variant::Tag(pattern),
                            boon_checked::Variant::Tag(actual)
                        ) if pattern == actual
                    ) || matches!(
                        (pattern_variant, actual_variant),
                        (
                            boon_checked::Variant::Tagged {
                                tag: pattern,
                                ..
                            },
                            boon_checked::Variant::Tagged { tag: actual, .. }
                        ) if pattern == actual
                    )
                });
                let Some(matching) = matching else {
                    return Err(SemanticError::new(format!(
                        "OUT input variant {actual_variant:?} is not admitted by expected type {pattern:?}"
                    )));
                };
                let (
                    boon_checked::Variant::Tagged {
                        fields: pattern_fields,
                        ..
                    },
                    boon_checked::Variant::Tagged {
                        fields: actual_fields,
                        ..
                    },
                ) = (matching, actual_variant)
                else {
                    continue;
                };
                for (name, pattern) in &pattern_fields.fields {
                    let Some(actual) = actual_fields.fields.get(name) else {
                        if actual_fields.open {
                            continue;
                        }
                        return Err(SemanticError::new(format!(
                            "OUT input tagged variant is missing required field `{name}`"
                        )));
                    };
                    unify_out_contract_type(pattern, actual, substitutions).map_err(|error| {
                        SemanticError::new(format!("tagged variant field `{name}`: {error}"))
                    })?;
                }
            }
        }
        (
            boon_checked::Type::Bytes(boon_checked::BytesType::Dynamic),
            boon_checked::Type::Bytes(_),
        ) => {}
        (
            boon_checked::Type::Function {
                args: pattern_args,
                result: pattern_result,
            },
            boon_checked::Type::Function {
                args: actual_args,
                result: actual_result,
            },
        ) => {
            if pattern_args.len() != actual_args.len() {
                return Err(SemanticError::new(format!(
                    "OUT input function has {} arguments; expected {}",
                    actual_args.len(),
                    pattern_args.len()
                )));
            }
            if pattern_result.mode != actual_result.mode {
                return Err(SemanticError::new(format!(
                    "OUT input function result mode {:?} differs from expected {:?}",
                    actual_result.mode, pattern_result.mode
                )));
            }
            for (index, (pattern, actual)) in pattern_args.iter().zip(actual_args).enumerate() {
                unify_out_contract_type(pattern, actual, substitutions).map_err(|error| {
                    SemanticError::new(format!("function argument {index}: {error}"))
                })?;
            }
            unify_out_contract_type(&pattern_result.ty, &actual_result.ty, substitutions)
                .map_err(|error| SemanticError::new(format!("function result: {error}")))?;
        }
        (pattern, actual) if pattern == actual => {}
        (pattern, actual) => {
            return Err(SemanticError::new(format!(
                "OUT input type {actual:?} is incompatible with expected type {pattern:?}"
            )));
        }
    }
    Ok(())
}

fn merge_out_contract_substitutions(
    substitutions: &mut BTreeMap<boon_checked::TypeVar, boon_checked::Type>,
    additions: impl IntoIterator<Item = (boon_checked::TypeVar, boon_checked::Type)>,
) {
    for (variable, value) in additions {
        match substitutions.get(&variable) {
            Some(existing)
                if out_contract_type_is_resolved(existing)
                    && !out_contract_type_is_resolved(&value) => {}
            _ => {
                substitutions.insert(variable, value);
            }
        }
    }
}

fn validate_contextual_bindings(program: &CheckedProgramFields) -> Result<(), SemanticError> {
    let callables = program
        .callables
        .iter()
        .map(|callable| (callable.decl_id, callable))
        .collect::<BTreeMap<_, _>>();
    if callables.len() != program.callables.len() {
        return Err(SemanticError::new(
            "checked callable table contains duplicate declaration IDs",
        ));
    }

    let mut formals_by_id = BTreeMap::new();
    let mut formals_by_callable = BTreeMap::new();
    for formal in &program.context_formals {
        if formals_by_id.insert(formal.id, formal).is_some() {
            return Err(SemanticError::new(format!(
                "checked contextual formal table contains duplicate formal {}",
                formal.id.0
            )));
        }
        if formals_by_callable
            .insert(formal.callable, formal)
            .is_some()
        {
            return Err(SemanticError::new(format!(
                "checked callable {} owns more than one contextual formal",
                formal.callable.0
            )));
        }
        let callable = callables.get(&formal.callable).ok_or_else(|| {
            SemanticError::new(format!(
                "contextual formal {} references missing callable {}",
                formal.id.0, formal.callable.0
            ))
        })?;
        if callable.kind != boon_checked::CheckedCallableKind::User {
            return Err(SemanticError::new(format!(
                "non-user callable {} owns contextual formal {}",
                formal.callable.0, formal.id.0
            )));
        }
        if callable.context_formal != Some(formal.id) {
            return Err(SemanticError::new(format!(
                "contextual formal {} is not the declared formal of callable {}",
                formal.id.0, formal.callable.0
            )));
        }
    }
    for callable in &program.callables {
        match callable.context_formal {
            Some(formal) => {
                let definition = formals_by_id.get(&formal).ok_or_else(|| {
                    SemanticError::new(format!(
                        "callable {} references missing contextual formal {}",
                        callable.decl_id.0, formal.0
                    ))
                })?;
                if definition.callable != callable.decl_id {
                    return Err(SemanticError::new(format!(
                        "callable {} references contextual formal {} owned by callable {}",
                        callable.decl_id.0, formal.0, definition.callable.0
                    )));
                }
            }
            None if formals_by_callable.contains_key(&callable.decl_id) => {
                return Err(SemanticError::new(format!(
                    "callable {} owns a contextual formal but does not declare it",
                    callable.decl_id.0
                )));
            }
            None => {}
        }
    }

    let expressions = program
        .expressions
        .iter()
        .map(|expression| expression.id)
        .collect::<BTreeSet<_>>();
    for call in &program.calls {
        let callable = callables.get(&call.callable).ok_or_else(|| {
            SemanticError::new(format!(
                "checked call {} references missing callable {}",
                call.id.0, call.callable.0
            ))
        })?;
        let target_formal = callable.context_formal;
        match call.context_binding {
            boon_checked::CheckedContextBinding::Explicit { value, .. } => {
                if target_formal.is_none() {
                    return Err(SemanticError::new(format!(
                        "checked call {} has explicit PASS context for noncontextual callable {}",
                        call.id.0, call.callable.0
                    )));
                }
                if !expressions.contains(&value) {
                    return Err(SemanticError::new(format!(
                        "checked call {} explicit PASS context references missing expression {}",
                        call.id.0, value.0
                    )));
                }
            }
            boon_checked::CheckedContextBinding::Inherited { formal } => {
                if target_formal.is_none() {
                    return Err(SemanticError::new(format!(
                        "checked call {} inherits PASS context for noncontextual callable {}",
                        call.id.0, call.callable.0
                    )));
                }
                let owner = call.owner_callable.ok_or_else(|| {
                    SemanticError::new(format!(
                        "root checked call {} cannot inherit contextual formal {}",
                        call.id.0, formal.0
                    ))
                })?;
                let owner_callable = callables.get(&owner).ok_or_else(|| {
                    SemanticError::new(format!(
                        "checked call {} inherits from missing owner callable {}",
                        call.id.0, owner.0
                    ))
                })?;
                if owner_callable.context_formal != Some(formal)
                    || formals_by_id
                        .get(&formal)
                        .is_none_or(|definition| definition.callable != owner)
                {
                    return Err(SemanticError::new(format!(
                        "checked call {} inherits contextual formal {} outside owner callable {}",
                        call.id.0, formal.0, owner.0
                    )));
                }
            }
            boon_checked::CheckedContextBinding::None => {
                if let Some(formal) = target_formal {
                    return Err(SemanticError::new(format!(
                        "checked call {} to contextual callable {} has no explicit or inherited binding for formal {}",
                        call.id.0, call.callable.0, formal.0
                    )));
                }
            }
        }

        let mut substitutions = BTreeSet::new();
        for substitution in &call.contextual_substitutions {
            let Some(formal) = target_formal else {
                return Err(SemanticError::new(format!(
                    "checked call {} has contextual substitutions for noncontextual callable {}",
                    call.id.0, call.callable.0
                )));
            };
            if substitution.formal != formal
                || formals_by_id
                    .get(&substitution.formal)
                    .is_none_or(|definition| definition.callable != call.callable)
            {
                return Err(SemanticError::new(format!(
                    "checked call {} contextual substitution formal {} is not owned by callable {}",
                    call.id.0, substitution.formal.0, call.callable.0
                )));
            }
            if !substitutions.insert((substitution.formal, substitution.variable)) {
                return Err(SemanticError::new(format!(
                    "checked call {} repeats contextual substitution formal {} variable {:?}",
                    call.id.0, substitution.formal.0, substitution.variable
                )));
            }
        }
    }
    Ok(())
}

fn validate_out_contracts(
    program: &CheckedProgramFields,
    graph: &ResolvedOutGraph,
) -> Result<(), SemanticError> {
    for net in &graph.nets {
        let owner = graph.owner_for_net(net.id).ok_or_else(|| {
            SemanticError::new(format!("OUT net {} has no generation owner", net.id))
        })?;
        let output_scope = graph
            .owner_scope_for_net(net.id)
            .ok_or_else(|| SemanticError::new(format!("OUT net {} has no output scope", net.id)))?;
        let expected_generation = Some(OutGenerationIdentityV1 {
            owner,
            output_scope,
        });
        let expected_correlation = Some(OutCorrelationIdentityV1 { net: net.id, owner });
        let Some(first_port_id) = net.ports.first().copied() else {
            return Err(SemanticError::new(format!(
                "OUT net {} has no ports",
                net.id
            )));
        };
        let baseline = &graph.ports[first_port_id.as_usize()].contract;
        for port_id in &net.ports {
            let port = graph
                .ports
                .get(port_id.as_usize())
                .filter(|port| port.id == *port_id && port.net == net.id)
                .ok_or_else(|| {
                    SemanticError::new(format!(
                        "OUT net {} references noncanonical port {}",
                        net.id, port_id
                    ))
                })?;
            let contract = &port.contract;
            if contract.flow_type.ty != contract.resolved_type
                || contract.resolved_type != baseline.resolved_type
            {
                return Err(out_contract_mismatch(net.id, *port_id, "type"));
            }
            let expected_shape =
                canonical_hash(OUT_PORT_SHAPE_DIGEST_DOMAIN, &contract.resolved_type)?;
            if contract.shape_digest != expected_shape
                || contract.shape_digest != baseline.shape_digest
            {
                return Err(out_contract_mismatch(net.id, *port_id, "shape"));
            }
            if contract.output_scope != output_scope
                || contract.output_scope != baseline.output_scope
                || !program
                    .scopes
                    .iter()
                    .any(|scope| scope.id == contract.lexical_scope)
                || !program.scopes.iter().any(|scope| {
                    scope.id == contract.output_scope
                        && scope.kind == boon_checked::CheckedScopeKind::RepeatedOutput
                })
            {
                return Err(out_contract_mismatch(net.id, *port_id, "scope"));
            }
            if contract.role != baseline.role {
                return Err(out_contract_mismatch(net.id, *port_id, "role"));
            }
            if contract.generation_identity != expected_generation
                || contract.generation_identity != baseline.generation_identity
            {
                return Err(out_contract_mismatch(net.id, *port_id, "generation"));
            }
            if contract.correlation_identity != expected_correlation
                || contract.correlation_identity != baseline.correlation_identity
            {
                return Err(out_contract_mismatch(net.id, *port_id, "correlation"));
            }
            let expected_presence = OutPresenceCompatibilityV1::from_mode(contract.flow_type.mode);
            if contract.presence != expected_presence || contract.presence != baseline.presence {
                return Err(out_contract_mismatch(net.id, *port_id, "presence"));
            }
        }
    }
    Ok(())
}

fn out_contract_mismatch(
    net: OutNetId,
    port: out_net::OutPortId,
    dimension: &str,
) -> SemanticError {
    SemanticError::new(format!(
        "OUT net {net} port {port} has incompatible {dimension} contract"
    ))
}

fn out_contract_type_is_resolved(ty: &boon_checked::Type) -> bool {
    match ty {
        boon_checked::Type::Var(_)
        | boon_checked::Type::Unknown
        | boon_checked::Type::UnresolvedShape { .. } => false,
        boon_checked::Type::List(item) => out_contract_type_is_resolved(item),
        boon_checked::Type::Map { key, value } => {
            out_contract_type_is_resolved(key) && out_contract_type_is_resolved(value)
        }
        boon_checked::Type::Set(item) => out_contract_type_is_resolved(item),
        boon_checked::Type::Union(members) => {
            !members.is_empty() && members.iter().all(out_contract_type_is_resolved)
        }
        boon_checked::Type::Function { args, result } => {
            args.iter().all(out_contract_type_is_resolved)
                && out_contract_type_is_resolved(&result.ty)
        }
        boon_checked::Type::Object(shape) => {
            shape.fields.values().all(out_contract_type_is_resolved)
        }
        boon_checked::Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            boon_checked::Variant::Tag(_) => true,
            boon_checked::Variant::Tagged { fields, .. } => {
                fields.fields.values().all(out_contract_type_is_resolved)
            }
        }),
        boon_checked::Type::Text
        | boon_checked::Type::Number
        | boon_checked::Type::Bytes(_)
        | boon_checked::Type::Bits { .. }
        | boon_checked::Type::Absent
        | boon_checked::Type::RenderContract => true,
    }
}

fn canonical_producer_requests(
    requests: &[ProducerMaterializationRequest],
) -> Result<Vec<ProducerMaterializationRequest>, SemanticError> {
    let mut requests = requests.to_vec();
    requests.sort();
    requests.dedup();
    for request in &requests {
        if request.identity.iter().all(|byte| *byte == 0) {
            return Err(SemanticError::new(
                "producer materialization identity must be nonzero",
            ));
        }
        if request.local_function.is_empty() {
            return Err(SemanticError::new(
                "producer materialization function must be nonempty",
            ));
        }
    }
    for pair in requests.windows(2) {
        if pair[0].identity == pair[1].identity {
            return Err(SemanticError::new(format!(
                "producer materialization identity {} names both `{}` and `{}`",
                digest_hex(&pair[0].identity),
                pair[0].local_function,
                pair[1].local_function
            )));
        }
    }
    Ok(requests)
}

fn resolve_producer_roots(
    program: &CheckedProgramFields,
    requests: &[ProducerMaterializationRequest],
) -> Result<Vec<out_net::ProducerRootSpec>, SemanticError> {
    let first_statement = program
        .statements
        .iter()
        .map(|statement| statement.id.0 as usize)
        .max()
        .map_or(0, |id| id.saturating_add(1));
    requests
        .iter()
        .cloned()
        .enumerate()
        .map(|(ordinal, request)| {
            let callable = exact_producer_callable(program, &request)?;
            if callable.result.mode != boon_checked::FlowMode::Continuous {
                return Err(SemanticError::new(format!(
                    "producer function `{}` result must be continuous, found {:?}",
                    request.local_function, callable.result.mode
                )));
            }
            let out_parameters = callable
                .parameters
                .iter()
                .filter(|parameter| parameter.kind != boon_checked::CheckedParameterKind::Value)
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>();
            if !out_parameters.is_empty() {
                return Err(SemanticError::new(format!(
                    "producer function `{}` has unsupported OUT parameter(s): {}",
                    request.local_function,
                    out_parameters.join(", ")
                )));
            }
            if callable.requires_pass() {
                return Err(SemanticError::new(format!(
                    "producer function `{}` has an unsupported PASS-in-signature requirement",
                    request.local_function
                )));
            }
            if callable.result_expression.is_none() {
                return Err(SemanticError::new(format!(
                    "producer function `{}` has no checked result expression",
                    request.local_function
                )));
            }
            if callable
                .parameters
                .iter()
                .any(|parameter| runtime_type_contains_var(&parameter.flow_type.ty))
                || runtime_type_contains_var(&callable.result.ty)
            {
                return Err(SemanticError::new(format!(
                    "producer function `{}` has no concrete distributed boundary specialization",
                    request.local_function
                )));
            }
            let function = ProducerFunctionId(ordinal);
            let mut parameters = callable.parameters.clone();
            parameters.sort_by_key(|parameter| parameter.ordinal);
            let parameters = parameters
                .into_iter()
                .map(|parameter| out_net::ProducerRootParameter {
                    formal: parameter.decl_id,
                    parameter: ProducerParameterId {
                        function,
                        ordinal: parameter.ordinal,
                    },
                    name: parameter.name,
                    flow_type: parameter.flow_type,
                })
                .collect();
            let invocation = request.mode == ProducerMaterializationMode::Invocation;
            Ok(out_net::ProducerRootSpec {
                identity: request.identity,
                mode: request.mode,
                callable: callable.decl_id,
                function,
                function_name: callable.name.clone(),
                result_statement: ProducerResultStatementId(
                    first_statement.saturating_add(ordinal),
                ),
                result_declaration: callable.decl_id,
                result_path: format!("@producer/{}/result", digest_hex(&request.identity)),
                result_type: if invocation {
                    boon_checked::FlowType {
                        mode: boon_checked::FlowMode::PresentOrAbsent,
                        ty: callable.result.ty.clone(),
                    }
                } else {
                    callable.result.clone()
                },
                parameters,
            })
        })
        .collect()
}

fn exact_producer_callable<'a>(
    program: &'a CheckedProgramFields,
    request: &ProducerMaterializationRequest,
) -> Result<&'a boon_checked::CheckedCallableSignature, SemanticError> {
    let Some(callable) = program.callables.get(request.callable.as_usize()) else {
        return Err(SemanticError::new(format!(
            "producer request references missing semantic callable {}",
            request.callable
        )));
    };
    if callable.kind != boon_checked::CheckedCallableKind::User
        || callable.name != request.local_function
    {
        return Err(SemanticError::new(format!(
            "producer request callable {} does not exactly identify user function `{}`",
            request.callable, request.local_function
        )));
    }
    Ok(callable)
}

pub(crate) fn temporally_gated_checked_expressions(
    program: &CheckedProgramFields,
) -> BTreeSet<boon_checked::CheckedExprId> {
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, expression))
        .collect::<BTreeMap<_, _>>();
    let calls = program
        .calls
        .iter()
        .map(|call| (call.id, call))
        .collect::<BTreeMap<_, _>>();
    let mut pending = Vec::new();
    for expression in &program.expressions {
        match &expression.kind {
            boon_checked::CheckedExpressionKind::Then {
                output: Some(output),
                ..
            } => pending.push(*output),
            boon_checked::CheckedExpressionKind::When { input, arms }
            | boon_checked::CheckedExpressionKind::While { input, arms }
                if expressions.get(input).is_some_and(|input| {
                    input.flow_type.mode != boon_checked::FlowMode::Continuous
                }) =>
            {
                pending.extend(arms.iter().copied());
            }
            _ => {}
        }
    }
    let mut gated = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if !gated.insert(expression) {
            continue;
        }
        let Some(expression) = expressions.get(&expression) else {
            continue;
        };
        pending.extend(checked_expression_children_for_call_analysis(
            &expression.kind,
            &calls,
        ));
    }
    gated
}

fn checked_expression_children_for_call_analysis(
    kind: &boon_checked::CheckedExpressionKind,
    calls: &BTreeMap<boon_checked::CheckedCallId, &boon_checked::CheckedCall>,
) -> Vec<boon_checked::CheckedExprId> {
    use boon_checked::CheckedExpressionKind as Kind;
    match kind {
        Kind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                boon_checked::CheckedTextSegment::Dynamic { value } => Some(*value),
                boon_checked::CheckedTextSegment::Static { .. } => None,
            })
            .collect(),
        Kind::TaggedObject { fields, .. } | Kind::Object { fields } => {
            fields.iter().map(|field| field.value).collect()
        }
        Kind::Call { call } => calls
            .get(call)
            .into_iter()
            .flat_map(|call| {
                call.entries
                    .iter()
                    .filter_map(|entry| match entry {
                        boon_checked::CheckedCallEntry::Input { value, .. } => Some(*value),
                        _ => None,
                    })
                    .chain(call.context_binding.explicit().map(|(value, _)| value))
            })
            .collect(),
        Kind::Flush { payload: input } | Kind::Draining { input } => vec![*input],
        Kind::Hold { initial, .. } => vec![*initial],
        Kind::Latest { branches } => branches.clone(),
        Kind::When { input, arms } | Kind::While { input, arms } => {
            let mut children = vec![*input];
            children.extend(arms.iter().copied());
            children
        }
        Kind::Then { input, output } => {
            let mut children = vec![*input];
            children.extend(*output);
            children
        }
        Kind::Infix { left, right, .. } => vec![*left, *right],
        Kind::MapEntry { key, value } => vec![*key, *value],
        Kind::MatchArm { output, .. } => output.iter().copied().collect(),
        Kind::Block { bindings, result } => bindings
            .iter()
            .map(|binding| binding.value)
            .chain(result.iter().copied())
            .collect(),
        Kind::List { items, .. }
        | Kind::Bytes { items, .. }
        | Kind::Map { entries: items }
        | Kind::Set { items } => items.clone(),
        Kind::Read { .. }
        | Kind::Passed { .. }
        | Kind::ExternalRead { .. }
        | Kind::Drain { .. }
        | Kind::Text { .. }
        | Kind::Number { .. }
        | Kind::Bits { .. }
        | Kind::BytesByte { .. }
        | Kind::Absent
        | Kind::Tag { .. }
        | Kind::Source
        | Kind::Delimiter
        | Kind::Invalid { .. } => Vec::new(),
    }
}

fn distributed_function_role(function: &str) -> Option<boon_checked::ProgramRole> {
    match function.split_once('/')?.0 {
        "Client" => Some(boon_checked::ProgramRole::Client),
        "Session" => Some(boon_checked::ProgramRole::Session),
        "Server" => Some(boon_checked::ProgramRole::Server),
        _ => None,
    }
}

fn runtime_type_contains_var(ty: &boon_checked::Type) -> bool {
    match ty {
        boon_checked::Type::Var(_) => true,
        boon_checked::Type::List(item) => runtime_type_contains_var(item),
        boon_checked::Type::Map { key, value } => {
            runtime_type_contains_var(key) || runtime_type_contains_var(value)
        }
        boon_checked::Type::Set(item) => runtime_type_contains_var(item),
        boon_checked::Type::Union(members) => members.iter().any(runtime_type_contains_var),
        boon_checked::Type::Function { args, result } => {
            args.iter().any(runtime_type_contains_var) || runtime_type_contains_var(&result.ty)
        }
        boon_checked::Type::Object(shape) => shape.fields.values().any(runtime_type_contains_var),
        boon_checked::Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            boon_checked::Variant::Tag(_) => false,
            boon_checked::Variant::Tagged { fields, .. } => {
                fields.fields.values().any(runtime_type_contains_var)
            }
        }),
        boon_checked::Type::Text
        | boon_checked::Type::Number
        | boon_checked::Type::Bytes(_)
        | boon_checked::Type::Bits { .. }
        | boon_checked::Type::Absent
        | boon_checked::Type::RenderContract
        | boon_checked::Type::UnresolvedShape { .. }
        | boon_checked::Type::Unknown => false,
    }
}

fn semantic_program_digest(
    program: &SemanticProgram,
) -> Result<SemanticProgramDigestV1, SemanticError> {
    #[derive(Serialize)]
    struct Payload<'a> {
        schema: &'static str,
        source_bundle_digest_v1: SourceBundleDigestV1,
        checked_program_digest: CheckedProgramDigestV1,
        component_digests: &'a CallableDependencyComponentDigestsV1,
        canonical_core_digest: [u8; 32],
        dependency_manifest_digest: CallableDependencyManifestDigestV1,
    }
    let canonical_core_digest = canonical_hash(
        CANONICAL_PROGRAM_CORE_DIGEST_DOMAIN,
        &program.canonical_core,
    )?;
    Ok(SemanticProgramDigestV1(canonical_hash(
        SEMANTIC_PROGRAM_DIGEST_DOMAIN,
        &Payload {
            schema: SEMANTIC_PROGRAM_SCHEMA_V1,
            source_bundle_digest_v1: program.source_bundle_digest_v1,
            checked_program_digest: program.dependency_manifest.checked_program_digest,
            component_digests: &program.dependency_manifest.component_digests,
            canonical_core_digest,
            dependency_manifest_digest: program.dependency_manifest.manifest_digest,
        },
    )?))
}

fn canonical_hash<T: Serialize + ?Sized>(
    domain: &[u8],
    value: &T,
) -> Result<[u8; 32], SemanticError> {
    boon_contract::canonical_serde_hash_v1(domain, value)
        .map_err(|error| SemanticError::new(format!("canonical semantic encoding failed: {error}")))
}

fn digest_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticError {
    message: String,
}

impl SemanticError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SemanticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for SemanticError {}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_checked::{
        CheckedEffectSummary, CheckedExternalDeclarationIdentityV1, CheckedExternalDeclarationKind,
        CheckedProgram, ExternalFunctionArgument, ExternalFunctionType, ExternalTypeEnvironment,
        FlowMode, FlowType, ProgramRole, Type,
    };

    fn continuous(ty: Type) -> FlowType {
        FlowType {
            mode: FlowMode::Continuous,
            ty,
        }
    }

    #[test]
    fn out_contract_type_variable_widens_independently_of_input_order() {
        let variable = boon_checked::TypeVar(7);
        let narrow = Type::VariantSet(vec![boon_checked::Variant::Tag("Closed".to_owned())].into());
        let wide = Type::VariantSet(
            vec![
                boon_checked::Variant::Tag("Closed".to_owned()),
                boon_checked::Variant::Tag("Open".to_owned()),
            ]
            .into(),
        );

        for (first, second) in [(&narrow, &wide), (&wide, &narrow)] {
            let mut substitutions = BTreeMap::new();
            unify_out_contract_type(&Type::Var(variable), first, &mut substitutions)
                .expect("first closed type binds the variable");
            unify_out_contract_type(&Type::Var(variable), second, &mut substitutions)
                .expect("assignable closed types have a common wider contract");
            assert_eq!(substitutions.get(&variable), Some(&wide));
        }
    }

    #[test]
    fn first_concrete_out_input_replaces_an_initial_consumer_scaffold_once() {
        let variable = boon_checked::TypeVar(7);
        let provisional = Type::object(boon_checked::ObjectShape {
            fields: BTreeMap::from([
                (
                    "item_kind".to_owned(),
                    Type::VariantSet(
                        vec![boon_checked::Variant::Tag("GroupHeader".to_owned())].into(),
                    ),
                ),
                ("name".to_owned(), Type::Text),
            ]),
            field_order: vec!["item_kind".to_owned(), "name".to_owned()],
            open: true,
        });
        let exact = Type::object(boon_checked::ObjectShape {
            fields: BTreeMap::from([
                (
                    "item_kind".to_owned(),
                    Type::VariantSet(
                        vec![boon_checked::Variant::Tag("VariableRow".to_owned())].into(),
                    ),
                ),
                ("id".to_owned(), Type::Text),
                ("payload".to_owned(), Type::Number),
            ]),
            field_order: vec![
                "item_kind".to_owned(),
                "id".to_owned(),
                "payload".to_owned(),
            ],
            open: false,
        });
        let mut substitutions = BTreeMap::from([(variable, provisional)]);
        let mut provisional_variables = BTreeSet::from([variable]);
        let pattern = Type::List(Type::shared(Type::Var(variable)));
        let actual = Type::List(Type::shared(exact.clone()));

        release_provisional_out_contract_bindings(
            &pattern,
            &actual,
            &mut substitutions,
            &mut provisional_variables,
        );
        unify_out_contract_type(&pattern, &actual, &mut substitutions)
            .expect("a closed parent input must own a provisionally shaped OUT formal");
        assert_eq!(substitutions.get(&variable), Some(&exact));

        let result_variable = boon_checked::TypeVar(8);
        let generic_result =
            Type::VariantSet(vec![boon_checked::Variant::Tag("GenericBranch".to_owned())].into());
        let occurrence_result =
            Type::VariantSet(vec![boon_checked::Variant::Tag("SelectedBranch".to_owned())].into());
        substitutions.insert(result_variable, generic_result);
        provisional_variables.insert(result_variable);
        release_provisional_out_contract_bindings(
            &Type::Var(result_variable),
            &occurrence_result,
            &mut substitutions,
            &mut provisional_variables,
        );
        unify_out_contract_type(
            &Type::Var(result_variable),
            &occurrence_result,
            &mut substitutions,
        )
        .expect("the first concrete output occurrence must replace its broad checked principal");
        assert_eq!(
            substitutions.get(&result_variable),
            Some(&occurrence_result),
        );

        release_provisional_out_contract_bindings(
            &Type::Var(variable),
            &Type::Text,
            &mut substitutions,
            &mut provisional_variables,
        );
        let error = unify_out_contract_type(&Type::Var(variable), &Type::Text, &mut substitutions)
            .expect_err("a second incompatible closed provider must remain fail-closed");
        assert!(
            error.to_string().contains("conflicting concrete types"),
            "unexpected second-provider error: {error}",
        );
    }

    #[test]
    fn out_contract_union_applies_an_existing_frame_binding_at_the_member_leaf() {
        let variable = boon_checked::TypeVar(46);
        let true_type =
            Type::VariantSet(vec![boon_checked::Variant::Tag("True".to_owned())].into());
        let false_type =
            Type::VariantSet(vec![boon_checked::Variant::Tag("False".to_owned())].into());
        let pattern = Type::Union(vec![Type::Var(variable), false_type.clone()]);
        let actual = Type::Union(vec![true_type.clone(), false_type]);
        let mut substitutions = BTreeMap::from([(variable, true_type)]);

        unify_out_contract_type(&pattern, &actual, &mut substitutions)
            .expect("a structural branch union must reuse its existing frame alpha binding");
    }

    fn checked_branch_container_fixture() -> (
        CheckedProgramFields,
        ResolvedOutGraph,
        boon_checked::CheckedExprId,
        Vec<boon_checked::CheckedExprId>,
        boon_checked::CheckedExprId,
    ) {
        let parsed = boon_parser::parse_source(
            "semantic-closed-branch-container.bn",
            r#"
result:
    True |> WHEN {
        True => True
        __ => False
    }
"#,
        )
        .expect("closed branch-container fixture parses");
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("closed branch-container fixture typechecks");
        let (fields, _) = checked.into_parts();
        let (container, branches) = fields
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                boon_checked::CheckedExpressionKind::When { arms, .. } => {
                    Some((expression.id, arms.clone()))
                }
                _ => None,
            })
            .expect("fixture has one WHEN container");
        let unresolved_leaf = branches
            .iter()
            .rev()
            .find_map(|branch| {
                fields
                    .expressions
                    .iter()
                    .find(|expression| expression.id == *branch)
                    .and_then(|expression| match expression.kind {
                        boon_checked::CheckedExpressionKind::MatchArm {
                            output: Some(output),
                            ..
                        } => Some(output),
                        _ => None,
                    })
            })
            .expect("fixture wildcard arm has one output expression");
        let producer_roots = resolve_producer_roots(&fields, &[]).unwrap();
        let graph = out_net::OutNet::<OutPortContractV1>::try_build_with(
            &fields,
            producer_roots,
            |call, _, entry| provisional_out_port_contract(&fields, call, entry),
            |kind, _, _, _, _| kind == boon_checked::CheckedCallableKind::Builtin,
        )
        .expect("closed branch-container fixture builds one OUT graph")
        .graph;
        (fields, graph, container, branches, unresolved_leaf)
    }

    #[test]
    fn closed_checked_branch_container_does_not_reinterpret_child_owner_alphas() {
        let (mut fields, graph, container, branches, unresolved_leaf) =
            checked_branch_container_fixture();
        let expected = fields
            .expressions
            .iter()
            .find(|expression| expression.id == container)
            .expect("WHEN container expression")
            .flow_type
            .ty
            .clone();
        assert!(boon_checked::type_is_recursively_closed(&expected));
        fields
            .expressions
            .iter_mut()
            .find(|expression| expression.id == unresolved_leaf)
            .expect("wildcard output expression")
            .flow_type
            .ty = Type::Var(boon_checked::TypeVar(10));

        let actual = concrete_checked_branch_expression_type(
            &fields,
            &graph,
            ScopedCheckedExpr {
                expression: container,
                frame: None,
                evaluation_port: None,
                value_frame: None,
            },
            &branches,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
        )
        .expect("a closed checked branch container owns its occurrence result");

        assert_eq!(actual, expected);
    }

    #[test]
    fn substituted_only_branch_container_closure_still_checks_unresolved_arms() {
        let (mut fields, graph, container, branches, unresolved_leaf) =
            checked_branch_container_fixture();
        fields
            .expressions
            .iter_mut()
            .find(|expression| expression.id == container)
            .expect("WHEN container expression")
            .flow_type
            .ty = Type::Var(boon_checked::TypeVar(0));
        fields
            .expressions
            .iter_mut()
            .find(|expression| expression.id == unresolved_leaf)
            .expect("wildcard output expression")
            .flow_type
            .ty = Type::Var(boon_checked::TypeVar(10));
        let substitutions = BTreeMap::from([(boon_checked::TypeVar(0), Type::Number)]);

        let error = concrete_checked_branch_expression_type(
            &fields,
            &graph,
            ScopedCheckedExpr {
                expression: container,
                frame: None,
                evaluation_port: None,
                value_frame: None,
            },
            &branches,
            &substitutions,
            &mut BTreeSet::new(),
        )
        .expect_err("a frame substitution cannot bless an unresolved child-owner branch");

        assert!(
            error.to_string().contains("Var(TypeVar(10))"),
            "unexpected branch-container error: {error}",
        );
    }

    #[test]
    #[ignore = "reads the temporary current owner-checked NovyWave artifact"]
    fn inspect_current_owner_checked_novywave_out_frames() {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let encoded =
            std::fs::read_to_string(workspace.join("target/novywave-owner-checked-current.toml"))
                .expect("temporary NovyWave checked artifact");
        let fields: boon_checked::CheckedProgramFields =
            toml::from_str(&encoded).expect("temporary checked artifact decodes");
        let producer_roots = resolve_producer_roots(&fields, &[]).unwrap();
        let retained = contextual_expansion::ordinary_callable_declarations(&fields);
        let intent =
            verified_intent::VerifiedSemanticIntentV1::build(&fields, &producer_roots, retained)
                .expect("cached NovyWave semantic intent");
        let out_net = out_net::OutNet::<OutPortContractV1>::try_build_with_intent(
            &fields,
            producer_roots,
            &intent,
            |call, _, entry| provisional_out_port_contract(&fields, call, entry),
            |kind, _, _, _, _| kind == boon_checked::CheckedCallableKind::Builtin,
        )
        .expect("cached NovyWave OUT graph builds");
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let mut graph = out_net.graph.clone();
        resolve_out_contracts(&fields, &mut graph)
            .expect("cached artifact resolves every NovyWave OUT contract");
        let compact_expression = fields
            .expressions
            .iter()
            .find(|expression| expression.id.0 == 16545)
            .expect("compact PASSED expression");
        eprintln!(
            "compact PASSED checked expression {} flow={:?} kind={:?}",
            compact_expression.id.0,
            compact_expression.flow_type,
            std::mem::discriminant(&compact_expression.kind),
        );
        let compact_path = [
            "store".to_owned(),
            "bridge_request_compact_label".to_owned(),
        ];
        let compact_formal = fields
            .context_formal(boon_checked::ContextFormalId(104))
            .expect("compact PASSED context formal");
        eprintln!(
            "compact PASSED formal path={:?}",
            project_out_contract_type(compact_formal.scheme.flow_type.ty.clone(), &compact_path)
        );
        let store = fields
            .declarations
            .iter()
            .find(|declaration| declaration.id.0 == 1429)
            .expect("compact PASSED target declaration");
        eprintln!(
            "compact PASSED target declaration name={} kind={:?} value={:?}",
            store.name, store.kind, store.value,
        );
        let store_path = ["bridge_request_compact_label".to_owned()];
        eprintln!(
            "compact PASSED store path={:?}",
            project_out_contract_type(store.flow_type.ty.clone(), &store_path)
        );
        for declaration in fields
            .declarations
            .iter()
            .filter(|declaration| declaration.name == "bridge_request_compact_label")
        {
            eprintln!(
                "compact PASSED field declaration id={} kind={:?} flow={:?} value={:?}",
                declaration.id.0, declaration.kind, declaration.flow_type, declaration.value,
            );
        }
        for expression_id in [1230, 9596, 9597] {
            let expression = fields
                .expressions
                .iter()
                .find(|expression| expression.id.0 == expression_id)
                .expect("compact PASSED construction expression");
            eprintln!(
                "compact PASSED construction expression {expression_id} declaration={:?} projected={:?} kind={:?}",
                expression.declaration,
                project_out_contract_type(
                    expression.flow_type.ty.clone(),
                    if expression_id == 9597 {
                        &compact_path
                    } else {
                        &store_path
                    },
                ),
                std::mem::discriminant(&expression.kind),
            );
            if let boon_checked::CheckedExpressionKind::Object { fields } = &expression.kind {
                eprintln!(
                    "compact PASSED object fields={:?}",
                    fields
                        .iter()
                        .map(|field| (&field.name, field.value, field.declaration))
                        .collect::<Vec<_>>()
                );
            }
        }
        let mut frame = Some(out_net::OutCallInstanceId::from_usize(2127));
        while let Some(current) = frame {
            let instance = &graph.call_instances[current.as_usize()];
            eprintln!(
                "compact PASSED frame {} parent={:?} provenance={:?} passed={:?} substitutions={}",
                current,
                instance.parent,
                instance.provenance,
                instance.passed,
                instance.local_type_substitutions.len(),
            );
            if let Some(passed) = instance.passed {
                let value = fields
                    .expressions
                    .iter()
                    .find(|expression| expression.id == passed.value.expression)
                    .expect("compact PASSED value expression");
                eprintln!(
                    "compact PASSED value expression {} mode={:?} projected={:?} kind={:?} scoped={:?}",
                    value.id.0,
                    value.flow_type.mode,
                    project_out_contract_type(value.flow_type.ty.clone(), &compact_path),
                    std::mem::discriminant(&value.kind),
                    passed.value,
                );
            }
            frame = instance.parent;
        }
        let retained = contextual_expansion::ordinary_callable_declarations(&fields);
        contextual_expansion::derive_contextual_materializations(&fields, &graph, &retained, true)
            .expect("cached artifact derives every NovyWave contextual materialization");
    }

    fn checked_role(
        label: &str,
        source: &str,
        environment: &ExternalTypeEnvironment,
    ) -> CheckedProgram {
        let parsed = boon_parser::parse_source(label, source).expect("bundle fixture parses");
        let (output, _) = boon_typecheck::check_runtime_program_profiled_with_external_types(
            &parsed,
            environment,
        );
        assert!(
            !output.report.has_errors(),
            "bundle fixture diagnostics: {:#?}",
            output.report.diagnostics
        );
        output.program.expect("bundle fixture checks")
    }

    fn frozen_call_bundle() -> BundleSemanticProgramV1 {
        let session_checked = checked_role(
            "session-bundle.bn",
            r#"
store: [
    count: 1
]

FUNCTION add(value) {
    value + 1
}
"#,
            &ExternalTypeEnvironment::empty(ProgramRole::Session),
        );
        let session_add = session_checked
            .callables
            .iter()
            .find(|callable| callable.name == "add")
            .expect("session add callable");
        let session_count = session_checked
            .declarations
            .iter()
            .find(|declaration| {
                session_checked.declaration_path(declaration.id).as_deref() == Some("store.count")
            })
            .expect("session count declaration");
        let mut client_environment = ExternalTypeEnvironment::sealed(ProgramRole::Client);
        client_environment
            .values
            .insert("Session/store.count".to_owned(), continuous(Type::Number));
        client_environment.functions.insert(
            "Session/add".to_owned(),
            ExternalFunctionType {
                args: vec![ExternalFunctionArgument {
                    name: "value".to_owned(),
                    flow_type: continuous(Type::Number),
                }],
                result: continuous(Type::Number),
                effect: CheckedEffectSummary::default(),
            },
        );
        client_environment.external_identities.insert(
            "Session/store.count".to_owned(),
            CheckedExternalDeclarationIdentityV1 {
                producer_role: ProgramRole::Session,
                producer_source_bundle_digest_v1: session_checked.source_bundle_digest_v1,
                producer_declaration: session_count.id,
                kind: CheckedExternalDeclarationKind::Value,
            },
        );
        client_environment.external_identities.insert(
            "Session/add".to_owned(),
            CheckedExternalDeclarationIdentityV1 {
                producer_role: ProgramRole::Session,
                producer_source_bundle_digest_v1: session_checked.source_bundle_digest_v1,
                producer_declaration: session_add.decl_id,
                kind: CheckedExternalDeclarationKind::Callable,
            },
        );
        let client_checked = checked_role(
            "client-bundle.bn",
            "count: Session/store.count\nresult: Session/add(value: count)\n",
            &client_environment,
        );
        let server_checked = checked_role(
            "server-bundle.bn",
            "",
            &ExternalTypeEnvironment::empty(ProgramRole::Server),
        );

        let client = elaborate(client_checked, &[]).expect("client semantic program");
        let session_without_request =
            elaborate(session_checked.clone(), &[]).expect("session semantic program");
        let occurrences =
            distributed_call_occurrences(&client).expect("client distributed occurrence");
        let [occurrence] = occurrences.as_slice() else {
            panic!("expected exactly one distributed call occurrence");
        };
        let request = ProducerMaterializationRequest {
            identity: occurrence.producer_materialization_identity,
            callable: session_without_request
                .producer_callable("add")
                .expect("session producer callable"),
            local_function: "add".to_owned(),
            mode: occurrence.mode,
        };
        let session = elaborate(session_checked, &[request]).expect("requested session semantic");
        let server = elaborate(server_checked, &[]).expect("server semantic program");
        BundleSemanticProgramV1::freeze([server, client, session]).expect("bundle freezes")
    }

    fn frozen_invocation_bundle() -> BundleSemanticProgramV1 {
        let server_checked = checked_role(
            "server-invocation-bundle.bn",
            r#"
FUNCTION add(value) {
    value + 1
}
"#,
            &ExternalTypeEnvironment::empty(ProgramRole::Server),
        );
        let server_add = server_checked
            .callables
            .iter()
            .find(|callable| callable.name == "add")
            .expect("server add callable");
        let mut session_environment = ExternalTypeEnvironment::sealed(ProgramRole::Session);
        session_environment.functions.insert(
            "Server/add".to_owned(),
            ExternalFunctionType {
                args: vec![ExternalFunctionArgument {
                    name: "value".to_owned(),
                    flow_type: continuous(Type::Number),
                }],
                result: continuous(Type::Number),
                effect: CheckedEffectSummary::default(),
            },
        );
        session_environment.external_identities.insert(
            "Server/add".to_owned(),
            CheckedExternalDeclarationIdentityV1 {
                producer_role: ProgramRole::Server,
                producer_source_bundle_digest_v1: server_checked.source_bundle_digest_v1,
                producer_declaration: server_add.decl_id,
                kind: CheckedExternalDeclarationKind::Callable,
            },
        );
        let session_checked = checked_role(
            "session-invocation-bundle.bn",
            r#"
store: [
    invoke: SOURCE
    result:
        invoke |> THEN { Server/add(value: 7) }
]
"#,
            &session_environment,
        );
        let client_checked = checked_role(
            "client-invocation-bundle.bn",
            "",
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        );

        let session = elaborate(session_checked, &[]).expect("session semantic program");
        let server_without_request =
            elaborate(server_checked.clone(), &[]).expect("server semantic program");
        let occurrences =
            distributed_call_occurrences(&session).expect("session distributed occurrence");
        let [occurrence] = occurrences.as_slice() else {
            panic!("expected exactly one invocation occurrence");
        };
        assert_eq!(occurrence.mode, ProducerMaterializationMode::Invocation);
        let request = ProducerMaterializationRequest {
            identity: occurrence.producer_materialization_identity,
            callable: server_without_request
                .producer_callable("add")
                .expect("server producer callable"),
            local_function: "add".to_owned(),
            mode: occurrence.mode,
        };
        let server =
            elaborate(server_checked, &[request]).expect("requested server semantic program");
        let client = elaborate(client_checked, &[]).expect("client semantic program");
        BundleSemanticProgramV1::freeze([server, session, client])
            .expect("invocation bundle freezes")
    }

    fn frozen_event_value_bundle() -> BundleSemanticProgramV1 {
        let client_checked = checked_role(
            "client-event-value-bundle.bn",
            "store: [\n    submit: SOURCE\n]\n",
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        );
        let client_submit = client_checked
            .declarations
            .iter()
            .find(|declaration| {
                client_checked.declaration_path(declaration.id).as_deref() == Some("store.submit")
            })
            .expect("client submit declaration");
        let submit_flow = client_submit.flow_type.clone();
        let mut session_environment = ExternalTypeEnvironment::sealed(ProgramRole::Session);
        session_environment
            .values
            .insert("Client/store.submit".to_owned(), submit_flow);
        session_environment.external_identities.insert(
            "Client/store.submit".to_owned(),
            CheckedExternalDeclarationIdentityV1 {
                producer_role: ProgramRole::Client,
                producer_source_bundle_digest_v1: client_checked.source_bundle_digest_v1,
                producer_declaration: client_submit.id,
                kind: CheckedExternalDeclarationKind::Value,
            },
        );
        let session_checked = checked_role(
            "session-event-value-bundle.bn",
            "submit: Client/store.submit\n",
            &session_environment,
        );
        let server_checked = checked_role(
            "server-event-value-bundle.bn",
            "",
            &ExternalTypeEnvironment::empty(ProgramRole::Server),
        );
        let client = elaborate(client_checked, &[]).expect("client semantic program");
        let session = elaborate(session_checked, &[]).expect("session semantic program");
        let server = elaborate(server_checked, &[]).expect("server semantic program");
        BundleSemanticProgramV1::freeze([session, server, client])
            .expect("event value bundle freezes")
    }

    fn frozen_relayed_event_value_bundle() -> BundleSemanticProgramV1 {
        let client_checked = checked_role(
            "client-relayed-event-value-bundle.bn",
            "store: [\n    submit: SOURCE\n]\n",
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        );
        let client_submit = client_checked
            .declarations
            .iter()
            .find(|declaration| {
                client_checked.declaration_path(declaration.id).as_deref() == Some("store.submit")
            })
            .expect("client submit declaration");
        let client_submit_identity = CheckedExternalDeclarationIdentityV1 {
            producer_role: ProgramRole::Client,
            producer_source_bundle_digest_v1: client_checked.source_bundle_digest_v1,
            producer_declaration: client_submit.id,
            kind: CheckedExternalDeclarationKind::Value,
        };
        let mut session_environment = ExternalTypeEnvironment::sealed(ProgramRole::Session);
        session_environment.values.insert(
            "Client/store.submit".to_owned(),
            client_submit.flow_type.clone(),
        );
        session_environment
            .external_identities
            .insert("Client/store.submit".to_owned(), client_submit_identity);
        let session_checked = checked_role(
            "session-relayed-event-value-bundle.bn",
            "store: [\n    session_submit: Client/store.submit\n]\n",
            &session_environment,
        );
        let session_submit = session_checked
            .declarations
            .iter()
            .find(|declaration| {
                session_checked.declaration_path(declaration.id).as_deref()
                    == Some("store.session_submit")
            })
            .expect("session submit declaration");
        let session_submit_identity = CheckedExternalDeclarationIdentityV1 {
            producer_role: ProgramRole::Session,
            producer_source_bundle_digest_v1: session_checked.source_bundle_digest_v1,
            producer_declaration: session_submit.id,
            kind: CheckedExternalDeclarationKind::Value,
        };
        let mut server_environment = ExternalTypeEnvironment::sealed(ProgramRole::Server);
        server_environment.values.insert(
            "Session/store.session_submit".to_owned(),
            session_submit.flow_type.clone(),
        );
        server_environment.external_identities.insert(
            "Session/store.session_submit".to_owned(),
            session_submit_identity,
        );
        let server_checked = checked_role(
            "server-relayed-event-value-bundle.bn",
            "request: Session/store.session_submit\n",
            &server_environment,
        );

        let client = elaborate(client_checked, &[]).expect("client semantic program");
        let session = elaborate(session_checked, &[]).expect("session semantic program");
        let server = elaborate(server_checked, &[]).expect("server semantic program");
        BundleSemanticProgramV1::freeze([server, client, session])
            .expect("relayed event value bundle freezes")
    }

    #[test]
    fn semantic_callable_and_call_inventories_are_exact_and_fail_closed() {
        let bundle = frozen_call_bundle();
        for (_, program) in bundle.role_programs() {
            assert_eq!(
                program.execution_graph().callables.len(),
                program.checked_program.callables.len()
            );
            assert_eq!(
                program.execution_graph().calls.len(),
                program.checked_program.calls.len()
            );
            for (index, callable) in program.execution_graph().callables.iter().enumerate() {
                assert_eq!(callable.id, SemanticCallableId(index));
            }
            for (index, call) in program.execution_graph().calls.iter().enumerate() {
                assert_eq!(call.id, SemanticCallId(index));
            }
        }

        let mut client = bundle
            .role_program(ProgramRole::Client)
            .expect("client role")
            .clone();
        client.execution_graph.callables.pop();
        client
            .validate()
            .expect_err("missing semantic callable coverage must fail");

        let mut client = bundle
            .role_program(ProgramRole::Client)
            .expect("client role")
            .clone();
        client.execution_graph.calls[0]
            .function
            .push_str("/mutated");
        client
            .validate()
            .expect_err("mutated semantic call provenance must fail");

        let mut client = bundle
            .role_program(ProgramRole::Client)
            .expect("client role")
            .clone();
        let external = client
            .execution_graph
            .callables
            .iter()
            .position(|callable| callable.kind == boon_checked::CheckedCallableKind::External)
            .expect("external callable");
        client.execution_graph.callables[external].external_identity = None;
        client
            .validate()
            .expect_err("erased external callable identity must fail");
    }

    #[test]
    fn distributed_materialization_identity_is_structural_and_diagnostic_invariant() {
        let bundle = frozen_call_bundle();
        let client = bundle
            .role_program(ProgramRole::Client)
            .expect("client role");
        let occurrences = distributed_call_occurrences(client).expect("distributed occurrences");
        let [occurrence] = occurrences.as_slice() else {
            panic!("expected one distributed occurrence");
        };
        let call = &client.execution_graph().calls[occurrence.call.as_usize()];
        let external_identity = call.external_identity.expect("sealed call identity");
        let baseline = producer_materialization_identity(
            client,
            occurrence.root,
            &occurrence.call_path,
            external_identity,
            occurrence.mode,
        )
        .unwrap();
        assert_eq!(baseline, occurrence.producer_materialization_identity);
        assert_eq!(
            call.occurrence_segment,
            format!("call:{}", call.checked_call.0)
        );

        let mut diagnostics_mutated = client.clone();
        let diagnostic_call =
            &mut diagnostics_mutated.execution_graph.calls[occurrence.call.as_usize()];
        diagnostic_call.function = "Session/renamed_diagnostic".to_owned();
        diagnostic_call.span.line = diagnostic_call.span.line.saturating_add(100);
        diagnostic_call.span.start = diagnostic_call.span.start.saturating_add(1000);
        diagnostic_call.span.end = diagnostic_call.span.end.saturating_add(1000);
        diagnostics_mutated.execution_graph.callables[occurrence.callable.as_usize()].name =
            "renamed_diagnostic".to_owned();
        assert_eq!(
            producer_materialization_identity(
                &diagnostics_mutated,
                occurrence.root,
                &occurrence.call_path,
                external_identity,
                occurrence.mode,
            )
            .unwrap(),
            baseline
        );

        let first_checked = boon_typecheck::check_program(
            &boon_parser::parse_source(
                "structural-call-first.bn",
                "FUNCTION first(value) {\n    value\n}\nresult: first(value: 1)\n",
            )
            .unwrap(),
        )
        .program
        .expect("first structural call fixture");
        let renamed_checked = boon_typecheck::check_program(
            &boon_parser::parse_source(
                "structural-call-renamed.bn",
                "\n\nFUNCTION renamed(value) {\n    value\n}\n\nresult:\n    renamed(value: 1)\n",
            )
            .unwrap(),
        )
        .program
        .expect("renamed structural call fixture");
        let [first_call] = first_checked.calls.as_slice() else {
            panic!("expected one first call");
        };
        let [renamed_call] = renamed_checked.calls.as_slice() else {
            panic!("expected one renamed call");
        };
        assert_ne!(first_call.function, renamed_call.function);
        assert_ne!(first_call.span, renamed_call.span);
        assert_eq!(first_call.id, renamed_call.id);
        assert_eq!(
            crate::out_net::checked_call_occurrence_segment(&first_checked, first_call.id).unwrap(),
            crate::out_net::checked_call_occurrence_segment(&renamed_checked, renamed_call.id,)
                .unwrap()
        );

        let mut changed_path = occurrence.call_path.clone();
        changed_path.push(SemanticCallId(usize::MAX));
        assert_ne!(
            producer_materialization_identity(
                client,
                occurrence.root,
                &changed_path,
                external_identity,
                occurrence.mode,
            )
            .unwrap(),
            baseline
        );
        let mut changed_identity = external_identity;
        changed_identity.producer_declaration =
            DeclId(changed_identity.producer_declaration.0.saturating_add(1));
        assert_ne!(
            producer_materialization_identity(
                client,
                occurrence.root,
                &occurrence.call_path,
                changed_identity,
                occurrence.mode,
            )
            .unwrap(),
            baseline
        );
        let changed_mode = match occurrence.mode {
            ProducerMaterializationMode::Current => ProducerMaterializationMode::Invocation,
            ProducerMaterializationMode::Invocation => ProducerMaterializationMode::Current,
        };
        assert_ne!(
            producer_materialization_identity(
                client,
                occurrence.root,
                &occurrence.call_path,
                external_identity,
                changed_mode,
            )
            .unwrap(),
            baseline
        );
    }

    #[test]
    fn distributed_value_occurrences_include_producer_roots_and_are_structural() {
        let session_checked = checked_role(
            "session-value-occurrence.bn",
            "store: [\n    count: 1\n]\n",
            &ExternalTypeEnvironment::empty(ProgramRole::Session),
        );
        let session_count = session_checked
            .declarations
            .iter()
            .find(|declaration| {
                session_checked.declaration_path(declaration.id).as_deref() == Some("store.count")
            })
            .expect("session count declaration");
        let external_identity = CheckedExternalDeclarationIdentityV1 {
            producer_role: ProgramRole::Session,
            producer_source_bundle_digest_v1: session_checked.source_bundle_digest_v1,
            producer_declaration: session_count.id,
            kind: CheckedExternalDeclarationKind::Value,
        };
        let mut server_environment = ExternalTypeEnvironment::sealed(ProgramRole::Server);
        server_environment
            .values
            .insert("Session/store.count".to_owned(), continuous(Type::Number));
        server_environment
            .external_identities
            .insert("Session/store.count".to_owned(), external_identity);
        let server_checked = checked_role(
            "server-value-occurrence.bn",
            r#"
FUNCTION add_session(value) {
    value + Session/store.count
}

FUNCTION add_session_twice(value) {
    value + Session/store.count + Session/store.count
}
"#,
            &server_environment,
        );
        let without_request =
            elaborate(server_checked.clone(), &[]).expect("server semantic program");
        assert!(
            distributed_value_occurrences(&without_request)
                .unwrap()
                .is_empty(),
            "unmaterialized callable body must not invent a value crossing"
        );
        let request = ProducerMaterializationRequest {
            identity: [200; 32],
            callable: without_request
                .producer_callable("add_session")
                .expect("producer callable"),
            local_function: "add_session".to_owned(),
            mode: ProducerMaterializationMode::Current,
        };
        let earlier_request = ProducerMaterializationRequest {
            identity: [1; 32],
            callable: without_request
                .producer_callable("add_session_twice")
                .expect("second producer callable"),
            local_function: "add_session_twice".to_owned(),
            mode: ProducerMaterializationMode::Current,
        };
        let with_request = elaborate(server_checked.clone(), std::slice::from_ref(&request))
            .expect("materialized server semantic program");
        let occurrences =
            distributed_value_occurrences(&with_request).expect("distributed value occurrences");
        let [occurrence] = occurrences.as_slice() else {
            panic!("expected one producer-root value occurrence");
        };
        assert_eq!(
            occurrence.root,
            DistributedCallOccurrenceRoot::Producer([200; 32])
        );
        assert!(occurrence.call_path.is_empty());
        assert_eq!(occurrence.external_identity, external_identity);
        assert_eq!(occurrence.producer_role, ProgramRole::Session);
        assert_eq!(
            distributed_value_occurrence_identity(
                &with_request,
                occurrence.root,
                &occurrence.call_path,
                occurrence.checked_expression,
                occurrence.external_identity,
            )
            .unwrap(),
            occurrence.identity
        );

        let with_earlier_request = elaborate(server_checked, &[request, earlier_request])
            .expect("two materialized server producers");
        let rebased_occurrences = distributed_value_occurrences(&with_earlier_request)
            .expect("rebased distributed value occurrences");
        let rebased = rebased_occurrences
            .iter()
            .find(|candidate| candidate.root == DistributedCallOccurrenceRoot::Producer([200; 32]))
            .expect("existing producer occurrence survives canonical insertion");
        assert_ne!(
            occurrence.expression, rebased.expression,
            "earlier canonical producer insertion should rebase dense expression IDs"
        );
        assert_eq!(occurrence.checked_expression, rebased.checked_expression);
        assert_eq!(occurrence.identity, rebased.identity);

        let mut diagnostics_mutated = with_request.clone();
        let expression =
            &mut diagnostics_mutated.execution_graph.expressions[occurrence.expression.as_usize()];
        let SemanticExpressionKind::ExternalRead { canonical_path, .. } = &mut expression.kind
        else {
            panic!("occurrence expression is external");
        };
        canonical_path.push_str(".renamed_diagnostic");
        diagnostics_mutated
            .execution_graph
            .checked_expression_origins[occurrence.expression.as_usize()]
        .checked_span
        .line += 100;
        assert_eq!(
            distributed_value_occurrence_identity(
                &diagnostics_mutated,
                occurrence.root,
                &occurrence.call_path,
                occurrence.checked_expression,
                occurrence.external_identity,
            )
            .unwrap(),
            occurrence.identity
        );
        assert_ne!(
            distributed_value_occurrence_identity(
                &with_request,
                occurrence.root,
                &occurrence.call_path,
                boon_checked::CheckedExprId(u32::MAX),
                occurrence.external_identity,
            )
            .unwrap(),
            occurrence.identity
        );
    }

    #[test]
    fn bundle_invocation_crossing_binds_exact_semantic_trigger_arms() {
        let bundle = frozen_invocation_bundle();
        bundle.validate().unwrap();
        let [crossing] = bundle.call_crossings() else {
            panic!("expected one invocation crossing");
        };
        assert_eq!(crossing.mode, ProducerMaterializationMode::Invocation);
        assert_eq!(
            crossing.route_scope,
            BundleSemanticRouteScopeV1::OriginScoped
        );
        let [arm] = crossing.invocation_arms.as_slice() else {
            panic!("expected one exact invocation arm");
        };
        assert!(matches!(arm.cause, SemanticEventCauseV1::Source(_)));
        let session = bundle
            .role_program(ProgramRole::Session)
            .expect("session role");
        assert_eq!(
            session
                .reactive_graph()
                .invocation_arms_for_call_expression(crossing.consumer_expression)
                .unwrap(),
            crossing.invocation_arms
        );

        macro_rules! reject_invocation_mutation {
            ($mutation:expr) => {{
                let mut mutated = bundle.clone();
                ($mutation)(&mut mutated.call_crossings[0].invocation_arms[0]);
                mutated
                    .validate()
                    .expect_err("invocation-arm mutation must fail");
            }};
        }
        reject_invocation_mutation!(|arm: &mut SemanticTriggerOwnedArmV1| {
            arm.id = SemanticTriggerArmId(usize::MAX);
        });
        reject_invocation_mutation!(|arm: &mut SemanticTriggerOwnedArmV1| {
            arm.gate_expression = SemanticExprId(usize::MAX);
        });
        reject_invocation_mutation!(|arm: &mut SemanticTriggerOwnedArmV1| {
            arm.gate_value = SemanticValueId(usize::MAX);
        });
        reject_invocation_mutation!(|arm: &mut SemanticTriggerOwnedArmV1| {
            arm.owner = Some(StaticOwnerId(usize::MAX));
        });
        reject_invocation_mutation!(|arm: &mut SemanticTriggerOwnedArmV1| {
            arm.route_scope = SemanticScopeId(usize::MAX);
        });
        reject_invocation_mutation!(|arm: &mut SemanticTriggerOwnedArmV1| {
            arm.row_scope = Some(SemanticRowScopeId(usize::MAX));
        });
        reject_invocation_mutation!(|arm: &mut SemanticTriggerOwnedArmV1| {
            arm.output_expression = SemanticExprId(usize::MAX);
        });
        reject_invocation_mutation!(|arm: &mut SemanticTriggerOwnedArmV1| {
            arm.output_value = SemanticValueId(usize::MAX);
        });
    }

    #[test]
    fn bundle_event_value_crossing_binds_exact_source_and_projection() {
        let bundle = frozen_event_value_bundle();
        bundle.validate().unwrap();
        assert!(bundle.call_crossings().is_empty());
        let [crossing] = bundle.value_crossings() else {
            panic!("expected one event value crossing");
        };
        let BundleSemanticValueDeliveryV1::Event {
            source,
            payload_projection,
        } = &crossing.delivery
        else {
            panic!("SOURCE crossing must use event delivery");
        };
        assert!(payload_projection.is_empty());
        assert_eq!(
            crossing.route_scope,
            BundleSemanticRouteScopeV1::SessionLocal
        );
        let client = bundle
            .role_program(ProgramRole::Client)
            .expect("client role");
        assert!(client.resource_graph().sources.iter().any(|candidate| {
            candidate.id == *source
                && candidate.declaration == crossing.producer_declaration
                && candidate.expression == crossing.producer_expression
        }));

        let mut mutated = bundle.clone();
        let BundleSemanticValueDeliveryV1::Event {
            source,
            payload_projection,
        } = &mut mutated.value_crossings[0].delivery
        else {
            panic!("SOURCE crossing must use event delivery");
        };
        *source = SemanticSourceId(usize::MAX);
        payload_projection.push("mutated".to_owned());
        mutated
            .validate()
            .expect_err("event source/projection mutation must fail");
    }

    #[test]
    fn imported_event_drives_the_exact_local_hold_update() {
        let client_checked = checked_role(
            "client-imported-hold-event.bn",
            "store: [\n    increment: SOURCE\n]\n",
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        );
        let client_increment = client_checked
            .declarations
            .iter()
            .find(|declaration| {
                client_checked.declaration_path(declaration.id).as_deref()
                    == Some("store.increment")
            })
            .expect("client increment declaration");
        let mut session_environment = ExternalTypeEnvironment::sealed(ProgramRole::Session);
        session_environment.values.insert(
            "Client/store.increment".to_owned(),
            client_increment.flow_type.clone(),
        );
        let increment_identity = CheckedExternalDeclarationIdentityV1 {
            producer_role: ProgramRole::Client,
            producer_source_bundle_digest_v1: client_checked.source_bundle_digest_v1,
            producer_declaration: client_increment.id,
            kind: CheckedExternalDeclarationKind::Value,
        };
        session_environment
            .external_identities
            .insert("Client/store.increment".to_owned(), increment_identity);
        let session_checked = checked_role(
            "session-imported-hold-event.bn",
            r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
]
"#,
            &session_environment,
        );
        let session =
            elaborate_with_external_event_identities(session_checked, &[], &[increment_identity])
                .expect("session semantic program");
        let graph = session.reactive_graph();
        let [arm] = graph.state_update_arms.as_slice() else {
            panic!(
                "imported event must own one state update arm: {:#?}",
                graph.state_update_arms
            );
        };
        let trigger = graph
            .trigger_arms
            .get(arm.trigger.as_usize())
            .filter(|trigger| trigger.id == arm.trigger)
            .expect("state update trigger");
        assert!(
            matches!(
                trigger.cause,
                SemanticEventCauseV1::ExternalRead(SemanticExprId(0))
            ),
            "imported event update must retain its exact pre-bundle read cause: {trigger:#?}"
        );
    }

    #[test]
    fn bundle_relayed_event_value_crossing_binds_exact_upstream_read() {
        let bundle = frozen_relayed_event_value_bundle();
        bundle.validate().unwrap();
        assert!(bundle.call_crossings().is_empty());
        assert_eq!(bundle.value_crossings().len(), 2);

        let direct = bundle
            .value_crossings()
            .iter()
            .find(|crossing| crossing.consumer_role == ProgramRole::Session)
            .expect("Client event crosses into Session");
        assert!(matches!(
            direct.delivery,
            BundleSemanticValueDeliveryV1::Event { .. }
        ));

        let relayed = bundle
            .value_crossings()
            .iter()
            .find(|crossing| crossing.consumer_role == ProgramRole::Server)
            .expect("Session event crosses into Server");
        let BundleSemanticValueDeliveryV1::RelayedEvent {
            read,
            external_identity,
            payload_projection,
        } = &relayed.delivery
        else {
            panic!("Session alias must preserve imported event authority");
        };
        assert!(payload_projection.is_empty());
        assert_eq!(
            relayed.route_scope,
            BundleSemanticRouteScopeV1::OriginScoped
        );
        let session = bundle
            .role_program(ProgramRole::Session)
            .expect("session role");
        let upstream = session
            .execution_graph()
            .expressions
            .get(read.as_usize())
            .filter(|expression| expression.id == *read)
            .expect("exact upstream read");
        let SemanticExpressionKind::ExternalRead {
            external_identity: Some(upstream_identity),
            ..
        } = upstream.kind
        else {
            panic!("relayed event authority must be an external read");
        };
        assert_eq!(upstream_identity, *external_identity);
        assert_eq!(external_identity.producer_role, ProgramRole::Client);

        let mut mutated = bundle.clone();
        let delivery = &mut mutated
            .value_crossings
            .iter_mut()
            .find(|crossing| crossing.consumer_role == ProgramRole::Server)
            .expect("mutated Server crossing")
            .delivery;
        let BundleSemanticValueDeliveryV1::RelayedEvent { read, .. } = delivery else {
            panic!("Server crossing must be relayed");
        };
        *read = SemanticExprId(usize::MAX);
        mutated
            .validate()
            .expect_err("relayed event read mutation must fail");
    }

    #[test]
    fn bundle_semantic_freeze_owns_three_roles_and_rejects_mutations() {
        let bundle = frozen_call_bundle();
        bundle.validate().unwrap();
        assert_eq!(
            bundle
                .role_programs()
                .map(|(role, _)| role)
                .collect::<Vec<_>>(),
            vec![
                ProgramRole::Client,
                ProgramRole::Session,
                ProgramRole::Server
            ]
        );
        assert_eq!(bundle.call_crossings().len(), 1);
        assert_eq!(bundle.value_crossings().len(), 1);
        assert_eq!(bundle.producer_requests().len(), 1);
        let crossing = &bundle.call_crossings()[0];
        let request = &bundle.producer_requests()[0];
        assert_eq!(
            crossing.producer_materialization_identity,
            request.request.identity
        );
        assert_eq!(crossing.producer_callable, request.request.callable);
        assert_eq!(
            crossing.route_scope,
            BundleSemanticRouteScopeV1::SessionLocal
        );
        assert_eq!(crossing.mode, ProducerMaterializationMode::Current);
        assert!(crossing.invocation_arms.is_empty());
        let [argument] = crossing.arguments.as_slice() else {
            panic!("expected one exact call argument");
        };
        assert_eq!(argument.ordinal, 0);
        assert_eq!(argument.name, "value");
        assert!(matches!(
            argument.binding,
            BundleSemanticCallArgumentBindingV1::Explicit { .. }
        ));
        let value_crossing = &bundle.value_crossings()[0];
        assert_eq!(
            value_crossing.delivery,
            BundleSemanticValueDeliveryV1::Current
        );
        assert_eq!(
            value_crossing.route_scope,
            BundleSemanticRouteScopeV1::SessionLocal
        );

        let client = bundle
            .role_program(ProgramRole::Client)
            .expect("client role")
            .clone();
        let server = bundle
            .role_program(ProgramRole::Server)
            .expect("server role")
            .clone();
        BundleSemanticProgramV1::freeze([client.clone(), client, server])
            .expect_err("duplicate Client and missing Session must fail");

        macro_rules! reject_bundle_mutation {
            ($mutation:expr) => {{
                let mut mutated = bundle.clone();
                ($mutation)(&mut mutated);
                mutated
                    .validate()
                    .expect_err("bundle semantic mutation must fail");
            }};
        }

        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.role_digests[0].role = ProgramRole::Server;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.role_digests[0].semantic_program_digest =
                bundle.role_digests[1].semantic_program_digest;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.producer_requests[0].request.identity[0] ^= 0xff;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.producer_requests[0].request.callable = SemanticCallableId(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0]
                .occurrence_path
                .push_str("/mutated");
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].canonical_function = "Server/other".to_owned();
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].producer_role = ProgramRole::Server;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].producer_callable = SemanticCallableId(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].consumer_expression = SemanticExprId(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].consumer_value = SemanticValueId(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].consumer_instance = OutCallInstanceId::from_usize(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].owner_callable = Some(SemanticCallableId(usize::MAX));
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].owner = Some(StaticOwnerId(usize::MAX));
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].consumer_scope = SemanticScopeId(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].result.mode = FlowMode::PresentOrAbsent;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].effect.invokes_host =
                !bundle.call_crossings[0].effect.invokes_host;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].arguments[0].producer_formal = DeclId(u32::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].arguments[0]
                .producer_parameter
                .ordinal = usize::MAX;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            let BundleSemanticCallArgumentBindingV1::Explicit {
                expression,
                value,
                flow_type,
                from_pipe,
                ..
            } = &mut bundle.call_crossings[0].arguments[0].binding
            else {
                panic!("fixture argument is explicit");
            };
            *expression = SemanticExprId(usize::MAX);
            *value = SemanticValueId(usize::MAX);
            flow_type.mode = FlowMode::PresentOrAbsent;
            *from_pipe = !*from_pipe;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].mode = match bundle.call_crossings[0].mode {
                ProducerMaterializationMode::Current => ProducerMaterializationMode::Invocation,
                ProducerMaterializationMode::Invocation => ProducerMaterializationMode::Current,
            };
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].root = DistributedCallOccurrenceRoot::Producer([9; 32]);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings[0].route_scope = BundleSemanticRouteScopeV1::OriginScoped;
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.call_crossings.clear();
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0]
                .canonical_path
                .push_str(".mutated");
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0].occurrence_identity =
                DistributedValueOccurrenceIdentityV1([9; 32]);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0].root = DistributedCallOccurrenceRoot::Producer([9; 32]);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0]
                .call_path
                .push(SemanticCallId(usize::MAX));
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0].checked_expression = boon_checked::CheckedExprId(u32::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0]
                .occurrence_path
                .push_str("/mutated");
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0].consumer_scope = SemanticScopeId(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0].producer_expression = SemanticExprId(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0].producer_value = SemanticValueId(usize::MAX);
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0].delivery = BundleSemanticValueDeliveryV1::Event {
                source: SemanticSourceId(usize::MAX),
                payload_projection: vec!["mutated".to_owned()],
            };
        });
        reject_bundle_mutation!(|bundle: &mut BundleSemanticProgramV1| {
            bundle.value_crossings[0].route_scope = BundleSemanticRouteScopeV1::OriginScoped;
        });
    }

    fn wrapped_out_contract_fixture() -> (CheckedProgram, ResolvedOutGraph, usize) {
        let parsed = boon_parser::parse_source(
            "semantic-out-contract.bn",
            r#"
FUNCTION wrapped(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new
    )
}

rows: LIST { [value: 1] }
result:
    rows
    |> wrapped(
        entry
        new: entry.value + 1
    )
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("valid wrapped OUT fixture has one checked program");
        let semantic = elaborate(checked.clone(), &[])
            .expect("valid wrapped OUT fixture has semantic contracts");
        let graph = semantic.resolved_out_graph.clone();
        let port = graph
            .nets
            .iter()
            .find(|net| net.ports.len() > 1)
            .and_then(|net| net.ports.get(1))
            .expect("wrapped OUT fixture has two unified ports")
            .as_usize();
        (checked, graph, port)
    }

    #[test]
    fn out_contract_uses_exact_mapped_source_payload_projection_type() {
        let parsed = boon_parser::parse_source(
            "semantic-mapped-source-out-contract.bn",
            r#"
store: [
    rows:
        LIST { [name: TEXT { one }] }
        |> List/map(item, new: selectable_row(row: item))
    selected_addresses:
        rows
        |> List/map(item, new: item.controls.select.address)
]

FUNCTION selectable_row(row) {
    [controls: [select: SOURCE], name: row.name]
}
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("mapped source OUT fixture checks");
        let address_read = checked
            .resource_projection_requirements
            .iter()
            .find(|requirement| {
                requirement
                    .source_origins
                    .iter()
                    .any(|origin| origin.payload_projection == ["address"])
            })
            .expect("mapped address read has exact checked source provenance");
        assert_eq!(
            exact_checked_resource_projection_type(
                &checked,
                address_read.expression,
                &BTreeMap::new()
            )
            .unwrap(),
            Some(Type::Text)
        );
        let producer_roots = resolve_producer_roots(&checked, &[]).unwrap();
        let out_net = out_net::OutNet::<OutPortContractV1>::try_build_with(
            &checked,
            producer_roots,
            |call, _, entry| provisional_out_port_contract(&checked, call, entry),
            |kind, _, _, _, _| kind == boon_checked::CheckedCallableKind::Builtin,
        )
        .unwrap();
        assert!(!out_net.has_errors());
        let mut graph = out_net.graph;
        resolve_out_contracts(&checked, &mut graph)
            .expect("mapped source payload projection resolves its OUT contract");
        assert!(
            graph
                .ports
                .iter()
                .all(|port| out_contract_type_is_resolved(&port.contract.resolved_type)),
            "all mapped-source OUT ports must be concrete: {:#?}",
            graph.ports
        );
    }

    #[test]
    fn out_contract_preserves_tagged_when_payload_narrowing() {
        let parsed = boon_parser::parse_source(
            "semantic-tagged-when-out-contract.bn",
            r#"
store: [
    responses: LIST {
        [name: TEXT { content-type }, value: BYTES {}]
    }
    response:
        responses |> List/page(size: 20, after: Start)
    visible_headers:
        response |> WHEN {
            Page =>
                response.items
                |> List/filter(item, if: True)
            __ => LIST {}
        }
]
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "tagged WHEN fixture must typecheck: {:#?}",
            checked.report.diagnostics
        );
        let checked = checked.program.expect("tagged WHEN checked program");
        let payload_read = checked
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    &expression.kind,
                    boon_checked::CheckedExpressionKind::Read {
                        projection,
                        ..
                    } if projection == &["items"]
                )
            })
            .expect("arm-local tagged payload read");
        assert!(
            matches!(payload_read.flow_type.ty, Type::List(_)),
            "checked read must retain its arm-local payload type: {payload_read:#?}"
        );
        let producer_roots = resolve_producer_roots(&checked, &[]).unwrap();
        let out_net = out_net::OutNet::<OutPortContractV1>::try_build_with(
            &checked,
            producer_roots,
            |call, _, entry| provisional_out_port_contract(&checked, call, entry),
            |kind, _, _, _, _| kind == boon_checked::CheckedCallableKind::Builtin,
        )
        .unwrap();
        assert!(!out_net.has_errors());
        let mut graph = out_net.graph;
        resolve_out_contracts(&checked, &mut graph)
            .expect("OUT resolution must preserve the checked arm-local payload type");
        assert!(
            graph
                .ports
                .iter()
                .all(|port| out_contract_type_is_resolved(&port.contract.resolved_type)),
            "all narrowed OUT contracts must be concrete: {:#?}",
            graph.ports
        );
        let semantic = elaborate(checked, &[])
            .expect("tagged payload list and fallback authority must elaborate end to end");
        assert!(
            semantic
                .resource_graph()
                .value_list_authorities
                .iter()
                .any(|authority| {
                    authority.semantic_path == "store.visible_headers"
                        && authority.role == SemanticValueListRoleV1::InlineValue
                }),
            "the fallback literal must retain a distinct inline-value authority"
        );
    }

    #[test]
    fn out_contract_keeps_captured_parameter_distinct_from_map_item() {
        let parsed = boon_parser::parse_source(
            "semantic-captured-map-parameter.bn",
            r#"
store: [
    rows:
        LIST {
            [
                bit_width: TEXT { 8 }
                formatter: Hexadecimal
                segments: LIST {
                    [value: TEXT { a }]
                }
            ]
        }
        |> List/map(item, new: render(signal: item))
]

FUNCTION render(signal) {
    signal.segments
    |> List/map(item, new:
        decorate(segment: item, signal: signal)
    )
}

FUNCTION decorate(segment, signal) {
    [
        value: segment.value
        label: signal.bit_width
        format: signal.formatter
    ]
}
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "captured map parameter fixture must typecheck: {:#?}",
            checked.report.diagnostics
        );
        let semantic = elaborate(
            checked
                .program
                .expect("captured map parameter fixture has a checked program"),
            &[],
        )
        .expect("captured parameter keeps its frame type beneath the map item port");
        assert!(
            semantic
                .resolved_out_graph()
                .ports
                .iter()
                .all(|port| out_contract_type_is_resolved(&port.contract.resolved_type))
        );
    }

    #[test]
    fn generic_out_substitution_accepts_a_narrower_appended_item() {
        let parsed = boon_parser::parse_source(
            "semantic-generic-variant-refinement.bn",
            r#"
store: [
    rows:
        LIST {
            [title: TEXT { first }, completed: False]
            [title: TEXT { second }, completed: True]
        }
        |> List/append(item:
            [title: TEXT { third }, completed: False]
        )
        |> List/map(item, new: item)
]
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "generic variant refinement fixture must typecheck: {:#?}",
            checked.report.diagnostics
        );
        let semantic = elaborate(
            checked
                .program
                .expect("generic variant refinement fixture has a checked program"),
            &[],
        )
        .expect("a singleton variant item must refine its list authority type");
        assert!(
            semantic
                .resolved_out_graph()
                .ports
                .iter()
                .all(|port| out_contract_type_is_resolved(&port.contract.resolved_type))
        );
        let completed_authority = semantic
            .scope_storage_graph()
            .fields
            .iter()
            .find(|field| {
                matches!(
                    &field.origin,
                    SemanticStorageFieldOriginV1::ListAuthority { item_path, .. }
                        if item_path == &["completed".to_owned()]
                )
            })
            .expect("completed list authority field");
        assert_eq!(
            completed_authority.flow_type.ty,
            Type::VariantSet(
                vec![
                    boon_checked::Variant::Tag("False".to_owned()),
                    boon_checked::Variant::Tag("True".to_owned()),
                ]
                .into(),
            ),
            "the storage schema must retain the full authority type"
        );
    }

    #[test]
    fn nested_generic_map_result_resolves_from_its_concrete_call_frames() {
        let parsed = boon_parser::parse_source(
            "semantic-nested-generic-map-result.bn",
            r#"
store: [
    rows:
        LIST {
            [
                kind: VariableRow
                id: TEXT { signal-1 }
                segments: LIST {
                    [
                        file: TEXT { waveform.vcd }
                        signal_id: TEXT { signal-1 }
                        label: TEXT { high }
                    ]
                }
            ]
        }
        |> List/map(item, new: lane_row(row: item))
]

FUNCTION segment_row(segment, row) {
    [
        file: segment.file
        signal_id: segment.signal_id
        lane_id: row.id
        label: segment.label
    ]
}

FUNCTION segment_rows(row) {
    row.segments
    |> List/retain(item, if: segment_is_visible(segment: item))
    |> List/map(item, new:
        segment_row(
            segment: normalized_segment(segment: item)
            row: row
        )
    )
}

FUNCTION segment_is_visible(segment) {
    segment.label == TEXT { high }
}

FUNCTION normalized_segment(segment) {
    [
        file: segment.file
        signal_id: segment.signal_id
        label: segment.label
    ]
}

FUNCTION variable_lane(row) {
    [
        kind: row.kind
        id: row.id
        segments: segment_rows(row: row)
    ]
}

FUNCTION group_lane(row) {
    [
        kind: row.kind
        id: row.id
        segments: segment_rows(row: row)
    ]
}

FUNCTION lane_row(row) {
    row.kind |> WHEN {
        VariableRow => variable_lane(row: row)
        __ => group_lane(row: row)
    }
}
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "nested generic map fixture must typecheck: {:#?}",
            checked.report.diagnostics
        );
        let semantic = elaborate(
            checked
                .program
                .expect("nested generic map fixture has a checked program"),
            &[],
        )
        .expect("nested generic map results resolve through concrete call frames");
        let out = semantic.resolved_out_graph();
        assert!(
            out.ports
                .iter()
                .all(|port| out_contract_type_is_resolved(&port.contract.resolved_type)),
            "every nested generic map OUT port must have a concrete contract"
        );
        let retained_substitutions = out
            .call_instances
            .iter()
            .map(|call| call.local_type_substitutions.len())
            .sum::<usize>();
        let logical_substitutions = out
            .call_instances
            .iter()
            .map(|call| out.type_substitution_count(call.id))
            .sum::<usize>();
        assert!(
            retained_substitutions < logical_substitutions,
            "nested generic frames must retain local deltas instead of inherited copies"
        );
        let inherited = out
            .call_instances
            .iter()
            .find(|call| out.type_substitution_count(call.id) > call.local_type_substitutions.len())
            .expect("nested generic fixture has an inherited type environment");
        let environment = out.type_substitution_environment(inherited.id);
        for variable in environment.keys() {
            assert_eq!(
                out.apply_type_substitutions(inherited.id, &Type::Var(*variable)),
                boon_checked::apply_checked_type_environment(&Type::Var(*variable), &environment,),
                "parent-linked lookup must match the flattened checked environment"
            );
        }
    }

    #[test]
    #[ignore = "large NovyWave semantic compiler gate"]
    fn novywave_checked_program_elaborates_without_occurrence_type_loss() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/novywave");
        let units = [
            "hold.bn",
            "Bridge/NovyBridge.bn",
            "Generated/Assets.bn",
            "Generated/NovyReference.bn",
            "Model/NovyModel.bn",
            "Theme/NovyTheme.bn",
            "RUN.bn",
            "View/NovyView.bn",
        ]
        .into_iter()
        .map(|path| {
            (
                path.to_owned(),
                std::fs::read_to_string(root.join(path)).expect("NovyWave source unit"),
            )
        })
        .collect::<Vec<_>>();
        let parsed = boon_parser::parse_project("RUN.bn", units).expect("NovyWave project parses");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "NovyWave diagnostics: {:#?}",
            checked.report.diagnostics
        );
        elaborate(
            checked.program.expect("NovyWave has a checked program"),
            &[],
        )
        .expect("NovyWave semantic elaboration preserves concrete call occurrences");
    }

    fn assert_contract_rejection(
        checked: &CheckedProgramFields,
        graph: &ResolvedOutGraph,
        port: usize,
        dimension: &str,
        mutate: impl FnOnce(&mut OutPortContractV1),
    ) {
        let mut invalid = graph.clone();
        mutate(&mut invalid.ports[port].contract);
        let error = validate_out_contracts(checked, &invalid)
            .expect_err("mutated OUT contract must be rejected");
        assert!(
            error.to_string().contains(dimension),
            "expected {dimension} rejection, got {error}"
        );
    }

    #[test]
    fn minimal_checked_program_has_stable_complete_manifest() {
        let parsed = boon_parser::parse_source("semantic-empty.bn", "").unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("valid source has one checked program");
        let source_bundle_digest_v1 = checked.source_bundle_digest_v1;
        let first = elaborate(checked.clone(), &[]).unwrap();
        let second = elaborate(checked, &[]).unwrap();
        assert_eq!(first.source_bundle_digest_v1(), source_bundle_digest_v1);
        assert_eq!(second.source_bundle_digest_v1(), source_bundle_digest_v1);
        assert_eq!(first.digest(), second.digest());
        assert_eq!(
            first.dependency_manifest().checked_program_digest,
            second.dependency_manifest().checked_program_digest
        );
        assert_eq!(
            first.dependency_manifest().callable_entries,
            second.dependency_manifest().callable_entries
        );
        first.validate().unwrap();
    }

    #[test]
    fn producer_identity_is_canonical_and_fail_closed() {
        let parsed = boon_parser::parse_source(
            "semantic-producer.bn",
            r#"
FUNCTION serve(value) {
    value + 0
}

seed: 0
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("valid producer fixture has one authoritative checked program");
        let callable = SemanticCallableId(
            checked
                .callables
                .iter()
                .position(|callable| callable.name == "serve")
                .expect("serve callable identity"),
        );
        let request = ProducerMaterializationRequest {
            identity: [7; 32],
            callable,
            local_function: "serve".to_owned(),
            mode: ProducerMaterializationMode::Current,
        };
        let first = elaborate(checked.clone(), std::slice::from_ref(&request)).unwrap();
        let duplicate = elaborate(checked.clone(), &[request.clone(), request]).unwrap();
        assert_eq!(first.digest(), duplicate.digest());
        assert!(
            elaborate(
                checked,
                &[ProducerMaterializationRequest {
                    identity: [0; 32],
                    callable,
                    local_function: "serve".to_owned(),
                    mode: ProducerMaterializationMode::Current,
                }],
            )
            .is_err()
        );
    }

    #[test]
    fn out_contract_uses_the_checked_type_of_an_instance_less_retained_call() {
        let parsed = boon_parser::parse_source(
            "semantic-retained-call-out-contract.bn",
            r#"
FUNCTION classify(value) {
    value == 0 |> WHEN {
        True => Low
        __ => High
    }
}

FUNCTION rows(value) {
    LIST {
        [state: classify(value: value)]
    }
}

FUNCTION mapped(value) {
    rows(value: value)
    |> List/map(item, new: item.state)
}

result: mapped(value: 0)
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "retained-call fixture must typecheck: {:#?}",
            checked.report.diagnostics,
        );
        let checked = checked.program.expect("retained-call checked program");
        let retained = contextual_expansion::ordinary_callable_declarations(&checked);
        for name in ["classify", "rows"] {
            let callable = checked
                .callables
                .iter()
                .find(|callable| callable.name == name)
                .unwrap_or_else(|| panic!("missing retained callable {name}"));
            assert!(
                retained.contains(&callable.decl_id),
                "{name} must use shared ordinary-body lowering",
            );
        }
        let producer_roots = resolve_producer_roots(&checked, &[]).unwrap();
        let out_net = out_net::OutNet::<OutPortContractV1>::try_build_with_retained_definitions(
            &checked,
            producer_roots,
            &retained,
            |call, _, entry| provisional_out_port_contract(&checked, call, entry),
            |kind, _, _, _, _| kind == boon_checked::CheckedCallableKind::Builtin,
        )
        .unwrap();
        assert!(!out_net.has_errors(), "{:#?}", out_net.diagnostics);
        let rows_call = checked
            .calls
            .iter()
            .find(|call| call.function == "rows")
            .expect("mapped fixture rows call");
        let rows_instance = out_net
            .graph
            .call_instances
            .iter()
            .find(|instance| instance.provenance.call_id == Some(rows_call.id))
            .expect("mapped fixture allocates one concrete rows frame")
            .id;
        let classify_call = checked
            .calls
            .iter()
            .find(|call| call.function == "classify")
            .expect("rows fixture classify call");
        assert!(
            out_net
                .graph
                .call_instance_for_checked_call(classify_call.id, Some(rows_instance))
                .is_none(),
            "the pure classify call must remain instance-less in the retained rows frame",
        );
        assert!(out_net.graph.intentionally_elided_call(
            classify_call.id,
            classify_call.owner_callable,
            Some(rows_instance),
        ));
        let frame_substitutions = out_net.graph.type_substitution_environment(rows_instance);
        let exact = concrete_checked_expression_type(
            &checked,
            &out_net.graph,
            ScopedCheckedExpr {
                expression: classify_call.expression,
                frame: Some(rows_instance),
                evaluation_port: None,
                value_frame: None,
            },
            &frame_substitutions,
            &mut BTreeSet::new(),
        )
        .expect("retained classify occurrence has an exact checked type");
        let checked_expression = checked
            .expressions
            .iter()
            .find(|expression| expression.id == classify_call.expression)
            .expect("classify checked expression");
        assert_eq!(exact, checked_expression.flow_type.ty);
        assert!(boon_checked::type_is_recursively_closed(&exact));
        let missing_root_frame = concrete_checked_expression_type(
            &checked,
            &out_net.graph,
            ScopedCheckedExpr {
                expression: classify_call.expression,
                frame: None,
                evaluation_port: None,
                value_frame: None,
            },
            &BTreeMap::new(),
            &mut BTreeSet::new(),
        )
        .expect_err("a missing call outside its retained owner frame must fail closed");
        assert!(
            missing_root_frame
                .to_string()
                .contains("references missing OUT call instance"),
        );
        let mut graph = out_net.graph;
        resolve_out_contracts(&checked, &mut graph)
            .expect("a resolved retained-call occurrence must supply the OUT input type");
        assert!(
            graph
                .ports
                .iter()
                .all(|port| { out_contract_type_is_resolved(&port.contract.resolved_type) })
        );
    }

    #[test]
    fn resolved_out_contract_carries_complete_compatibility_identity() {
        let (checked, graph, _) = wrapped_out_contract_fixture();
        validate_out_contracts(&checked, &graph).unwrap();
        for port in &graph.ports {
            assert_eq!(port.contract.flow_type.ty, port.contract.resolved_type);
            assert_ne!(port.contract.shape_digest, [0; 32]);
            assert!(port.contract.generation_identity.is_some());
            assert!(port.contract.correlation_identity.is_some());
            assert!(
                checked
                    .scopes
                    .iter()
                    .any(|scope| scope.id == port.contract.lexical_scope)
            );
            assert!(
                checked
                    .scopes
                    .iter()
                    .any(|scope| scope.id == port.contract.output_scope)
            );
        }
    }

    #[test]
    fn semantic_digest_binds_the_complete_resolved_out_graph() {
        let (checked, _, _) = wrapped_out_contract_fixture();
        let mut semantic = elaborate(checked, &[]).unwrap();
        semantic.resolved_out_graph.static_owners[0].child_ordinal =
            semantic.resolved_out_graph.static_owners[0]
                .child_ordinal
                .saturating_add(1);
        semantic.execution_graph.static_owners[0].child_ordinal =
            semantic.resolved_out_graph.static_owners[0].child_ordinal;
        semantic.scope_storage_graph = build_semantic_scope_storage_graph(
            &semantic.checked_program,
            &semantic.execution_graph,
            &semantic.resource_graph,
            &semantic.reactive_graph,
            &semantic.lowering_contract,
            &semantic.resolved_out_graph,
        )
        .expect("mutated OUT owner has a fresh deterministic storage graph");
        semantic.memory_graph = build_semantic_memory_graph(
            &semantic.checked_program,
            &semantic.execution_graph,
            &semantic.resource_graph,
            &semantic.reactive_graph,
            &semantic.scope_storage_graph,
            &semantic.lowering_contract,
        )
        .expect("mutated OUT owner has a fresh deterministic memory graph");
        let lowering_dependency_rows =
            lowering_contract::lowering_dependency_rows(&semantic.lowering_contract)
                .expect("mutated lowering contract has construction dependency rows");
        let resource_dependency_rows =
            resource::resource_dependency_rows_for_test(&semantic.resource_graph)
                .expect("mutated resource graph has construction dependency rows");
        let dependency_build = build_callable_dependency_manifest_v7(
            DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
            &semantic.checked_program,
            semantic.semantic_image.checked_handoff(),
            semantic.semantic_image.execution_handoff(),
            &semantic.producer_materializations,
            &semantic.resolved_out_graph,
            &semantic.execution_graph,
            &semantic.resource_graph,
            &resource_dependency_rows,
            &semantic.reactive_graph,
            &semantic.lowering_contract,
            &lowering_dependency_rows,
            &semantic.view_binding_graph,
            &semantic.scope_storage_graph,
            &semantic.memory_graph,
        )
        .expect("mutated OUT owner has a fresh dependency manifest");
        semantic.dependency_manifest = dependency_build.manifest;
        semantic.request_graph = Arc::new(dependency_build.request_graph);
        let error = semantic
            .validate()
            .expect_err("mutated resolved graph must invalidate semantic digest");
        assert!(
            error
                .to_string()
                .contains("semantic program digest does not match"),
            "{error}"
        );
    }

    #[test]
    fn resolved_out_contract_rejects_incompatible_type_and_shape() {
        let (checked, graph, port) = wrapped_out_contract_fixture();
        assert_contract_rejection(&checked, &graph, port, "type", |contract| {
            contract.flow_type.ty = boon_checked::Type::Text;
            contract.resolved_type = boon_checked::Type::Text;
            contract.shape_digest =
                canonical_hash(OUT_PORT_SHAPE_DIGEST_DOMAIN, &contract.resolved_type).unwrap();
        });
        assert_contract_rejection(&checked, &graph, port, "shape", |contract| {
            contract.shape_digest[0] ^= 0xff
        });
    }

    #[test]
    fn resolved_out_contract_rejects_incompatible_scope_and_role() {
        let (checked, graph, port) = wrapped_out_contract_fixture();
        assert_contract_rejection(&checked, &graph, port, "scope", |contract| {
            contract.output_scope = boon_checked::LexicalScopeId(u32::MAX);
        });
        assert_contract_rejection(&checked, &graph, port, "role", |contract| {
            contract.role = match contract.role {
                boon_checked::ProgramRole::Client => boon_checked::ProgramRole::Server,
                boon_checked::ProgramRole::Session | boon_checked::ProgramRole::Server => {
                    boon_checked::ProgramRole::Client
                }
            };
        });
    }

    #[test]
    fn resolved_out_contract_rejects_incompatible_generation_correlation_and_presence() {
        let (checked, graph, port) = wrapped_out_contract_fixture();
        assert_contract_rejection(&checked, &graph, port, "generation", |contract| {
            contract
                .generation_identity
                .as_mut()
                .expect("resolved generation identity")
                .owner = StaticOwnerId(usize::MAX);
        });
        assert_contract_rejection(&checked, &graph, port, "correlation", |contract| {
            contract
                .correlation_identity
                .as_mut()
                .expect("resolved correlation identity")
                .net = OutNetId(usize::MAX);
        });
        assert_contract_rejection(&checked, &graph, port, "presence", |contract| {
            contract.presence.may_be_absent = !contract.presence.may_be_absent;
        });
    }

    #[test]
    fn bundle_semantic_v1_count_and_encoded_size_limits_are_inclusive() {
        for (kind, limit) in [
            (
                "producer requests",
                MAX_BUNDLE_SEMANTIC_PRODUCER_REQUESTS_V1,
            ),
            ("call crossings", MAX_BUNDLE_SEMANTIC_CALL_CROSSINGS_V1),
            ("value crossings", MAX_BUNDLE_SEMANTIC_VALUE_CROSSINGS_V1),
        ] {
            validate_bundle_collection_count(kind, limit, limit)
                .expect("the exact V1 count limit is admitted");
            assert!(
                validate_bundle_collection_count(kind, limit.saturating_add(1), limit).is_err(),
                "{kind} must reject the first count over its V1 limit"
            );
        }
        for (kind, limit) in [
            (
                "producer requests",
                MAX_BUNDLE_SEMANTIC_PRODUCER_REQUEST_BYTES_V1,
            ),
            ("call crossings", MAX_BUNDLE_SEMANTIC_CALL_CROSSING_BYTES_V1),
            (
                "value crossings",
                MAX_BUNDLE_SEMANTIC_VALUE_CROSSING_BYTES_V1,
            ),
        ] {
            validate_bundle_collection_encoded_size(kind, limit, limit)
                .expect("the exact V1 encoded-byte limit is admitted");
            assert!(
                validate_bundle_collection_encoded_size(kind, limit.saturating_add(1), limit)
                    .is_err(),
                "{kind} must reject the first encoded byte over its V1 limit"
            );
        }

        let encoded_fixture = vec!["semantic-boundary".to_owned()];
        let encoded_len = boon_contract::canonical_serde_cbor_v1(&encoded_fixture)
            .expect("fixture has canonical bytes")
            .len();
        validate_bundle_collection(
            "encoded fixture",
            &encoded_fixture,
            encoded_fixture.len(),
            encoded_len,
        )
        .expect("canonical encoded size is admitted at the exact boundary");
        assert!(
            validate_bundle_collection(
                "encoded fixture",
                &encoded_fixture,
                encoded_fixture.len(),
                encoded_len.saturating_sub(1),
            )
            .is_err()
        );
    }

    #[test]
    fn bundle_validation_rejects_collections_over_v1_count_limits() {
        let mut requests = frozen_call_bundle();
        let request = requests.producer_requests[0].clone();
        requests.producer_requests = vec![request; MAX_BUNDLE_SEMANTIC_PRODUCER_REQUESTS_V1 + 1];
        assert!(requests.validate().is_err());

        let mut calls = frozen_call_bundle();
        let crossing = calls.call_crossings[0].clone();
        calls.call_crossings = vec![crossing; MAX_BUNDLE_SEMANTIC_CALL_CROSSINGS_V1 + 1];
        assert!(calls.validate().is_err());

        let mut values = frozen_event_value_bundle();
        let crossing = values.value_crossings[0].clone();
        values.value_crossings = vec![crossing; MAX_BUNDLE_SEMANTIC_VALUE_CROSSINGS_V1 + 1];
        assert!(values.validate().is_err());
    }

    #[test]
    fn direct_host_result_owns_its_schedule_before_downstream_hold_consumers() {
        let parsed = boon_parser::parse_source(
            "direct-host-result-schedule.bn",
            r#"
store: [
    start: SOURCE
    result:
        start |> THEN { Clock/wall() }
    observed:
        0 |> HOLD observed {
            result |> WHEN {
                WallClockRead => 1
                __ => SKIP
            }
        }
]
"#,
        )
        .expect("direct host-result fixture parses");
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("direct host-result fixture typechecks");
        let semantic = elaborate(checked, &[]).expect("direct host-result fixture elaborates");
        let [schedule] = semantic.reactive_graph().host_effect_schedules.as_slice() else {
            panic!("expected one direct host-effect schedule");
        };
        assert!(schedule.state_update_arms.is_empty());
        let derived = schedule
            .transient_result
            .expect("direct host effect owns a transient result");
        let result = semantic
            .reactive_graph()
            .derived_values
            .get(derived.as_usize())
            .expect("transient derived result exists");
        assert!(!result.trigger_arms.is_empty());
        assert!(result.state_backing.is_none());
    }
}
