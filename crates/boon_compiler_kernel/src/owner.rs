use crate::{
    ComponentProgram, ComponentProgramBuilder, KernelCollectionOperationKind, KernelPattern,
    KernelRecordEntry, KernelSelectArm, KernelSolveError, KernelSolveWork, KernelSummaryCallInput,
    KernelSummaryNode, KernelSummaryProgram, KernelSummaryProjectionStep, KernelSummaryRecordEntry,
    KernelSummarySelectArm, KernelSummaryValueId, OutputId, PublishMode, TypeTermId,
    TypeVariableId, solve_component,
};
use boon_checked::{
    BytesType, FlowMode, FlowType, ObjectShape, Type, Variant, type_is_recursively_closed,
};
use boon_data::ExactRoundingRule;
use boon_effect_schema::{
    BarrierSpec, DeliveryCardinalitySpec, ReplaySpec, ResultPolicySpec, ValueType, host_effect_spec,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KernelExpressionId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KernelOwnerId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct KernelInheritedFormal {
    pub target_ordinal: u32,
    pub caller_ordinal: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KernelCollectionKind {
    List,
    Bytes,
    Set,
    Map,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum KernelRenderConstructorKind {
    Fixed(Box<str>),
    StripeDirection,
}

/// A source-level pure ABI call compiled away before the work queue runs.
///
/// These variants describe result and requirement equations, not runtime
/// implementations. The residual program therefore contains no function-name
/// dispatch or generic ABI edge search.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KernelPureBuiltinKind {
    TextTransform,
    TextSlice,
    TextLength,
    TextConcat,
    TextPredicate,
    TextToNumber,
    NumberToText,
    NumberMath,
    NumberRound,
    NumberProjection,
    ListLength,
    ListPredicate,
    ListFilter,
    ListMap,
    ListFind,
    ListLatest,
    ListAppend,
    ListSort,
    ListChunk,
    TextJoin,
    FieldColor,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum KernelOwnerNodeKind {
    /// A closed ABI value supplied at the kernel boundary (for example a
    /// SOURCE payload contract). It is imported once into the type DAG.
    Known(Type),
    /// A source occurrence with its closed payload ABI. This solves exactly
    /// like a known value while remaining explicit in the checked artifact.
    Source(Type),
    Absent,
    Text,
    TextTemplate,
    Number,
    Byte,
    Bits(u32),
    Tag(Box<str>),
    Record {
        tag: Option<Box<str>>,
    },
    Block,
    Collection {
        kind: KernelCollectionKind,
        capacity: Option<usize>,
    },
    MapEntry,
    /// One detached occurrence read from an invocation-local formal.
    FormalRead {
        formal: u32,
        fields: Box<[Box<str>]>,
    },
    /// A same-owner lexical alias. Unlike a cross-owner read, its occurrence
    /// remains an equality participant and can carry requirements back to the
    /// BLOCK binding that declared it.
    LexicalRead {
        fields: Box<[Box<str>]>,
    },
    /// A detached occurrence read from another owner's public provider.
    /// The exact provider is carried by the node's single `ReadProvider` edge.
    ValueRead {
        fields: Box<[Box<str>]>,
        /// A local selector expression whose tag match proves this nested
        /// projection belongs to the selected variant. Its mode, rather than
        /// an aggregate projection through every retained branch, owns the
        /// occurrence mode inside that match arm.
        mode_narrowing: Option<KernelExpressionId>,
    },
    /// A detached occurrence projected from an owner-local derived authority,
    /// such as a match payload or contextual collection binding.
    DerivedRead {
        fields: Box<[Box<str>]>,
    },
    /// One contextual collection callback binding, projected directionally
    /// from the input collection without coalescing producer and consumer.
    CollectionItemRead,
    /// A user-call occurrence. Acyclic targets are composed into this
    /// component with a fresh formal frame during compilation.
    UserCall {
        target: KernelOwnerId,
        inherited_formal: Option<KernelInheritedFormal>,
    },
    RenderConstructor {
        kind: KernelRenderConstructorKind,
    },
    PureBuiltin {
        kind: KernelPureBuiltinKind,
    },
    /// A host call whose type and execution policies come from the stable
    /// lower-level effect-schema registry. The operation name is the ABI key;
    /// no legacy callable graph is consulted.
    HostEffect {
        operation: Box<str>,
    },
    Latest,
    When,
    Then,
    Infix {
        operation: Box<str>,
    },
    Draining,
    Hold,
    MatchArm {
        pattern: KernelPattern,
    },
    Arrow,
    Delimiter,
    Unknown,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum KernelOwnerEdgeRole {
    RecordField {
        name: Box<str>,
        spread: bool,
    },
    /// A dynamic interpolation consumed while producing a `TextTemplate`.
    ///
    /// The interpolation does not constrain the template's result type, but
    /// it is still an authored dependency and therefore remains in the dense
    /// owner graph for reachability, currentness, and artifact provenance.
    TextDynamic,
    BlockResult,
    CollectionItem,
    MapEntry,
    MapKey,
    MapValue,
    ReadProvider,
    CallArgument {
        ordinal: u32,
    },
    AbiArgument {
        name: Box<str>,
    },
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
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KernelOwnerInputEdge {
    pub role: KernelOwnerEdgeRole,
    pub expression: KernelExpressionId,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KernelOwnerNode {
    pub kind: KernelOwnerNodeKind,
    pub inputs: Box<[KernelOwnerInputEdge]>,
    pub mode: FlowMode,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KernelOwnerProgramInput {
    pub nodes: Box<[KernelOwnerNode]>,
    /// Invocation-local parameter/context slots consumed by `FormalRead`.
    pub formal_count: u32,
    /// Compact namespace suffix referenced after the local node range.
    pub external_expressions: Box<[KernelExternalExpression]>,
    pub result: KernelExpressionId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct KernelExternalExpression {
    pub owner: KernelOwnerId,
    pub target: KernelExternalTarget,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KernelExternalTarget {
    Expression(KernelExpressionId),
    Result,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelProjectProgramInput {
    pub owners: Box<[KernelOwnerProgramInput]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerBuildError {
    message: String,
}

impl KernelOwnerBuildError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for KernelOwnerBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for KernelOwnerBuildError {}

#[derive(Debug)]
pub struct KernelOwnerProgram {
    component: ComponentProgram,
    result_output: OutputId,
    expression_outputs: Box<[OutputId]>,
    expression_modes: Box<[FlowMode]>,
    expression_artifacts: Box<[PendingKernelExpressionArtifact]>,
    calls: Box<[PendingKernelCallArtifact]>,
    effects: Box<[KernelHostEffectArtifact]>,
}

#[derive(Debug)]
pub struct KernelProjectProgram {
    component: ComponentProgram,
    owners: Box<[KernelProjectOwnerOutputs]>,
    compile_work: KernelCompileWork,
}

pub const KERNEL_RESIDUAL_MODULE_RANKING_LEN: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelResidualModuleWork {
    pub owner: u32,
    pub operations: u32,
    pub frames: u32,
    pub linked_operations: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelCompileWork {
    pub definition_modules: u64,
    pub principal_expressions: u64,
    pub residual_type_modules: u64,
    pub residual_module_operations: u64,
    pub residual_module_terms: u64,
    pub residual_frames: u64,
    pub linked_operations: u64,
    pub scheduled_work_items: u64,
    pub acyclic_residual_frames: u64,
    pub dominant_module_owner: u64,
    pub dominant_module_operations: u64,
    pub dominant_module_frames: u64,
    pub dominant_module_linked_operations: u64,
    pub residual_module_ranking: [KernelResidualModuleWork; KERNEL_RESIDUAL_MODULE_RANKING_LEN],
    pub linked_terms: u64,
    pub acyclic_initial_operations: u64,
    pub compiled_call_sites: u64,
    pub invocation_frames: u64,
    pub reused_invocation_frames: u64,
    pub direct_result_summaries: u64,
    pub summary_definition_nodes: u64,
    pub summary_invoke_nodes: u64,
    pub principal_result_reuses: u64,
    pub principal_expression_reuses: u64,
    pub pruned_invocation_expressions: u64,
    pub specialization_plans: u64,
    pub reused_specialization_plans: u64,
    pub max_call_depth: u64,
}

#[derive(Clone, Debug)]
struct KernelProjectOwnerOutputs {
    result: OutputId,
    expressions: Box<[OutputId]>,
    expression_modes: Box<[FlowMode]>,
    expression_artifacts: Box<[PendingKernelExpressionArtifact]>,
    calls: Box<[PendingKernelCallArtifact]>,
    effects: Box<[KernelHostEffectArtifact]>,
}

#[derive(Clone, Debug)]
struct PendingKernelExpressionArtifact {
    id: KernelExpressionId,
    kind: KernelOwnerNodeKind,
    inputs: Box<[KernelExpressionInputArtifact]>,
}

#[derive(Clone, Debug)]
struct PendingKernelCallArtifact {
    expression: KernelExpressionId,
    target: KernelCallTarget,
    inputs: Box<[KernelCallInputArtifact]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelCallTarget {
    User {
        target: KernelOwnerId,
        inherited_formal: Option<KernelInheritedFormal>,
    },
    RenderConstructor {
        kind: KernelRenderConstructorKind,
    },
    PureBuiltin {
        kind: KernelPureBuiltinKind,
    },
    HostEffect {
        operation: Box<str>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelCallInputRole {
    Formal { ordinal: u32 },
    Abi { name: Box<str> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCallInputArtifact {
    pub role: KernelCallInputRole,
    pub value: KernelValueReference,
}

/// A call input names either an expression in the call's definition or an
/// explicit expression/result authority in another dense definition. Keeping
/// the namespaces distinct prevents linked external providers from masquerading
/// as out-of-range local expression IDs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KernelValueReference {
    Local(KernelExpressionId),
    External(KernelExternalExpression),
}

pub type KernelCallValueReference = KernelValueReference;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelExpressionInputArtifact {
    pub role: KernelOwnerEdgeRole,
    pub value: KernelValueReference,
}

/// One solved expression row in a definition artifact. The compact authored
/// kind and typed input edges survive solving, so downstream stages consume
/// immutable definition facts instead of reconstructing source graphs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelExpressionArtifact {
    pub id: KernelExpressionId,
    pub kind: KernelOwnerNodeKind,
    pub inputs: Box<[KernelExpressionInputArtifact]>,
    pub flow_type: FlowType,
}

/// One source-authored call occurrence with its compact input edges and solved
/// result. Downstream consumers no longer need to rediscover call structure by
/// walking the owner expression graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCallArtifact {
    pub expression: KernelExpressionId,
    pub target: KernelCallTarget,
    pub inputs: Box<[KernelCallInputArtifact]>,
    pub result: FlowType,
}

/// One source-authored host-effect occurrence in a definition artifact.
///
/// Policies are copied from the stable ABI registry so downstream stages do
/// not rediscover effects by walking expressions or dispatching on call names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelHostEffectArtifact {
    pub expression: KernelExpressionId,
    pub operation: Box<str>,
    pub replay: ReplaySpec,
    pub barrier: BarrierSpec,
    pub result_policy: ResultPolicySpec,
    pub delivery: DeliveryCardinalitySpec,
}

/// Immutable checked result surface for one definition.
///
/// This is deliberately free of solver cells, operation IDs, and work
/// counters. Later checked rows (calls, effects, state, lists, diagnostics)
/// extend this single artifact instead of creating parallel owner products.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionArtifact {
    pub result: FlowType,
    pub expressions: Box<[KernelExpressionArtifact]>,
    pub calls: Box<[KernelCallArtifact]>,
    pub effects: Box<[KernelHostEffectArtifact]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDefinitionSnapshot {
    pub definition: DefinitionArtifact,
    pub work: KernelSolveWork,
}

/// Immutable checked snapshot produced by one complete dense solve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedSnapshot {
    pub definitions: Box<[DefinitionArtifact]>,
    pub work: KernelSolveWork,
}

/// Returns whether the permanent kernel can resolve this operation entirely
/// from the lower-level host-effect ABI registry.
pub fn is_kernel_host_effect(operation: &str) -> bool {
    host_effect_spec(operation).is_some_and(|spec| {
        spec.result_policy == ResultPolicySpec::ReturnValue && spec.schema.is_some()
    })
}

impl KernelOwnerProgram {
    pub fn solve(self) -> Result<KernelDefinitionSnapshot, KernelSolveError> {
        let artifact = solve_component(self.component)?;
        let mut result = artifact
            .output(self.result_output)
            .expect("owner result output belongs to its component")
            .flow_type
            .clone();
        let expression_flows = self
            .expression_outputs
            .iter()
            .zip(self.expression_modes.iter().copied())
            .map(|(output, mode)| {
                let mut flow = artifact
                    .output(*output)
                    .expect("owner expression output belongs to its component")
                    .flow_type
                    .clone();
                flow.mode = mode;
                flow
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let result_index = self
            .expression_outputs
            .iter()
            .position(|output| *output == self.result_output)
            .expect("owner result belongs to its expression outputs");
        result.mode = self.expression_modes[result_index];
        let calls = materialize_call_artifacts(self.calls, &expression_flows);
        let expressions =
            materialize_expression_artifacts(self.expression_artifacts, expression_flows);
        Ok(KernelDefinitionSnapshot {
            definition: DefinitionArtifact {
                result,
                expressions,
                calls,
                effects: self.effects,
            },
            work: artifact.work,
        })
    }

    pub fn component(&self) -> &ComponentProgram {
        &self.component
    }
}

impl KernelProjectProgram {
    pub fn solve(self) -> Result<KernelCheckedSnapshot, KernelSolveError> {
        let artifact = solve_component(self.component)?;
        let definitions = self
            .owners
            .into_vec()
            .into_iter()
            .map(|owner| {
                let mut result = artifact
                    .output(owner.result)
                    .expect("project owner result belongs to its component")
                    .flow_type
                    .clone();
                let expression_flows = owner
                    .expressions
                    .iter()
                    .zip(owner.expression_modes.iter().copied())
                    .map(|(output, mode)| {
                        let mut flow = artifact
                            .output(*output)
                            .expect("project owner expression belongs to its component")
                            .flow_type
                            .clone();
                        flow.mode = mode;
                        flow
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let result_index = owner
                    .expressions
                    .iter()
                    .position(|output| *output == owner.result)
                    .expect("project owner result belongs to its expression outputs");
                result.mode = owner.expression_modes[result_index];
                let calls = materialize_call_artifacts(owner.calls, &expression_flows);
                let expressions =
                    materialize_expression_artifacts(owner.expression_artifacts, expression_flows);
                DefinitionArtifact {
                    result,
                    expressions,
                    calls,
                    effects: owner.effects,
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(KernelCheckedSnapshot {
            definitions,
            work: artifact.work,
        })
    }

    pub fn component(&self) -> &ComponentProgram {
        &self.component
    }

    pub const fn compile_work(&self) -> KernelCompileWork {
        self.compile_work
    }
}

pub fn compile_owner_program(
    input: &KernelOwnerProgramInput,
) -> Result<KernelOwnerProgram, KernelOwnerBuildError> {
    if !input.external_expressions.is_empty() {
        return Err(KernelOwnerBuildError::new(
            "standalone owner program cannot import external expressions",
        ));
    }
    let result = checked_expression_index(input.result, input.nodes.len(), "owner result")?;
    let mut builder = ComponentProgramBuilder::new();
    let mut mode_builder = ModeProgramBuilder::default();
    let formal_static_variants = vec![None; input.formal_count as usize];
    let formal_dependent_expressions = [owner_expressions_depend_on_formals(input)];
    let formal_dependent_results = [formal_dependent_expressions[0][result]];
    let principals = vec![allocate_owner_instance(
        &mut builder,
        &mut mode_builder,
        input,
        &formal_static_variants,
    )];
    let principal = &principals[0];
    let context = OwnerCompileContext {
        initial_state_surface: false,
        owner: KernelOwnerId(0),
        input,
        expressions: &principal.expressions,
        formals: &principal.formals,
        formal_requirements: &principal.formal_requirements,
        expression_modes: &principal.expression_modes,
        formal_modes: &principal.formal_modes,
        formal_mode_sources: &principal.formal_mode_sources,
        static_variants: &principal.static_variants,
        formal_static_variants: &principal.formal_static_variants,
        project: None,
        principals: &principals,
        formal_dependent_results: &formal_dependent_results,
        formal_dependent_expressions: &formal_dependent_expressions,
        external_variables: None,
        syntax_selected_calls: None,
        direct_summaries: &[],
    };
    let specialization = OwnerSpecialization {
        static_variants: principal.static_variants.clone(),
        reachable: (0..input.nodes.len())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        syntax_selected_calls: vec![false; input.nodes.len()].into_boxed_slice(),
        invocation_dependencies: formal_dependent_expressions[0].clone(),
        transparent_type_providers: vec![None; input.nodes.len()].into_boxed_slice(),
    };
    let module = compile_residual_type_module(
        KernelOwnerId(0),
        input,
        None,
        &principals,
        &formal_dependent_results,
        &formal_dependent_expressions,
        &formal_static_variants,
        &specialization,
        None,
        false,
    )?;
    if let Some(call) = module.calls.first() {
        return Err(KernelOwnerBuildError::new(format!(
            "standalone owner node {call} cannot contain a user call"
        )));
    }
    append_residual_type_frame(&mut builder, &module, principal, &[])?;
    for (index, node) in input.nodes.iter().enumerate() {
        let equation = node_mode_equation(&mut mode_builder, &context, index, node)?;
        mode_builder.set(principal.expression_modes[index], equation);
    }

    let expression_outputs = principal
        .expressions
        .iter()
        .zip(input.nodes.iter())
        .map(|(variable, node)| builder.add_output(*variable, node.mode))
        .collect::<Vec<_>>();
    let modes = mode_builder.solve();
    let expression_modes = principal
        .expression_modes
        .iter()
        .map(|mode| modes[mode.0 as usize])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let result_output = expression_outputs[result];
    Ok(KernelOwnerProgram {
        component: builder.finish(),
        result_output,
        expression_outputs: expression_outputs.into_boxed_slice(),
        expression_modes,
        expression_artifacts: collect_expression_artifacts(input)?,
        calls: collect_call_artifacts(input)?,
        effects: collect_host_effect_artifacts(input)?,
    })
}

fn materialize_expression_artifacts(
    pending: Box<[PendingKernelExpressionArtifact]>,
    flows: Box<[FlowType]>,
) -> Box<[KernelExpressionArtifact]> {
    assert_eq!(
        pending.len(),
        flows.len(),
        "every solved expression must retain one compact artifact row"
    );
    pending
        .into_vec()
        .into_iter()
        .zip(flows)
        .map(|(expression, flow_type)| KernelExpressionArtifact {
            id: expression.id,
            kind: expression.kind,
            inputs: expression.inputs,
            flow_type,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn collect_expression_artifacts(
    input: &KernelOwnerProgramInput,
) -> Result<Box<[PendingKernelExpressionArtifact]>, KernelOwnerBuildError> {
    input
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            Ok(PendingKernelExpressionArtifact {
                id: KernelExpressionId(
                    u32::try_from(index).expect("kernel owner expression count exceeds u32"),
                ),
                kind: node.kind.clone(),
                inputs: node
                    .inputs
                    .iter()
                    .map(|edge| {
                        Ok(KernelExpressionInputArtifact {
                            role: edge.role.clone(),
                            value: kernel_value_reference(input, edge.expression, index)?,
                        })
                    })
                    .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?
                    .into_boxed_slice(),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn kernel_value_reference(
    input: &KernelOwnerProgramInput,
    expression: KernelExpressionId,
    consumer: usize,
) -> Result<KernelValueReference, KernelOwnerBuildError> {
    let reference = expression.0 as usize;
    if reference < input.nodes.len() {
        return Ok(KernelValueReference::Local(expression));
    }
    input
        .external_expressions
        .get(reference - input.nodes.len())
        .copied()
        .map(KernelValueReference::External)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "kernel expression {consumer} input references expression {reference} outside the local and external namespaces"
            ))
        })
}

fn materialize_call_artifacts(
    pending: Box<[PendingKernelCallArtifact]>,
    expressions: &[FlowType],
) -> Box<[KernelCallArtifact]> {
    pending
        .into_vec()
        .into_iter()
        .map(|call| KernelCallArtifact {
            expression: call.expression,
            target: call.target,
            inputs: call.inputs,
            result: expressions[call.expression.0 as usize].clone(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn collect_call_artifacts(
    input: &KernelOwnerProgramInput,
) -> Result<Box<[PendingKernelCallArtifact]>, KernelOwnerBuildError> {
    input
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(expression, node)| {
            let (target, uses_abi_inputs) = match &node.kind {
                KernelOwnerNodeKind::UserCall {
                    target,
                    inherited_formal,
                } => (
                    KernelCallTarget::User {
                        target: *target,
                        inherited_formal: *inherited_formal,
                    },
                    false,
                ),
                KernelOwnerNodeKind::RenderConstructor { kind } => (
                    KernelCallTarget::RenderConstructor { kind: kind.clone() },
                    true,
                ),
                KernelOwnerNodeKind::PureBuiltin { kind } => {
                    (KernelCallTarget::PureBuiltin { kind: *kind }, true)
                }
                KernelOwnerNodeKind::HostEffect { operation } => (
                    KernelCallTarget::HostEffect {
                        operation: operation.clone(),
                    },
                    true,
                ),
                _ => return None,
            };
            Some((|| {
                let mut inputs = Vec::with_capacity(node.inputs.len());
                for edge in &node.inputs {
                    let role = match &edge.role {
                        KernelOwnerEdgeRole::CallArgument { ordinal } if !uses_abi_inputs => {
                            KernelCallInputRole::Formal { ordinal: *ordinal }
                        }
                        KernelOwnerEdgeRole::AbiArgument { name } if uses_abi_inputs => {
                            KernelCallInputRole::Abi { name: name.clone() }
                        }
                        role => {
                            return Err(KernelOwnerBuildError::new(format!(
                                "kernel call node {expression} has non-call input role {role:?}"
                            )));
                        }
                    };
                    let value = kernel_value_reference(input, edge.expression, expression)?;
                    inputs.push(KernelCallInputArtifact { role, value });
                }
                Ok(PendingKernelCallArtifact {
                    expression: KernelExpressionId(
                        u32::try_from(expression)
                            .expect("kernel owner expression count exceeds u32"),
                    ),
                    target,
                    inputs: inputs.into_boxed_slice(),
                })
            })())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn collect_host_effect_artifacts(
    input: &KernelOwnerProgramInput,
) -> Result<Box<[KernelHostEffectArtifact]>, KernelOwnerBuildError> {
    input
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(expression, node)| {
            let KernelOwnerNodeKind::HostEffect { operation } = &node.kind else {
                return None;
            };
            Some((|| {
                let spec = host_effect_spec(operation).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel host-effect node {expression} names unknown operation `{operation}`"
                    ))
                })?;
                if spec.result_policy != ResultPolicySpec::ReturnValue || spec.schema.is_none() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel host-effect node {expression} operation `{operation}` has no return-value schema"
                    )));
                }
                Ok(KernelHostEffectArtifact {
                    expression: KernelExpressionId(
                        u32::try_from(expression)
                            .expect("kernel owner expression count exceeds u32"),
                    ),
                    operation: spec.operation.into(),
                    replay: spec.replay,
                    barrier: spec.barrier,
                    result_policy: spec.result_policy,
                    delivery: spec.delivery,
                })
            })())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

pub fn compile_project_program(
    input: &KernelProjectProgramInput,
) -> Result<KernelProjectProgram, KernelOwnerBuildError> {
    let mut builder = ComponentProgramBuilder::new();
    let mut mode_builder = ModeProgramBuilder::default();
    let mut invocations = HashMap::new();
    let mut specializations = HashMap::new();
    let mut residual_modules = HashMap::new();
    let mut compile_work = KernelCompileWork {
        definition_modules: input.owners.len() as u64,
        principal_expressions: input
            .owners
            .iter()
            .map(|owner| owner.nodes.len() as u64)
            .sum(),
        ..KernelCompileWork::default()
    };
    let formal_dependent_expressions = input
        .owners
        .iter()
        .map(owner_expressions_depend_on_formals)
        .collect::<Vec<_>>();
    let formal_dependent_results = input
        .owners
        .iter()
        .zip(&formal_dependent_expressions)
        .map(|(owner, dependencies)| {
            let result = owner.result.0 as usize;
            dependencies.get(result).copied().unwrap_or(true)
        })
        .collect::<Vec<_>>();
    let principals = input
        .owners
        .iter()
        .map(|owner| {
            allocate_owner_instance(
                &mut builder,
                &mut mode_builder,
                owner,
                &vec![None; owner.formal_count as usize],
            )
        })
        .collect::<Vec<_>>();
    for (owner_index, owner) in input.owners.iter().enumerate() {
        let owner_id = KernelOwnerId(
            u32::try_from(owner_index).expect("kernel project owner count exceeds u32"),
        );
        validate_owner_input(owner_id, owner, &principals)?;
    }
    let direct_summaries = compile_direct_result_summaries(&mut builder, input);
    for summary in direct_summaries.iter().flatten() {
        compile_work.summary_definition_nodes = compile_work
            .summary_definition_nodes
            .saturating_add(summary.program.nodes.len() as u64);
        compile_work.summary_invoke_nodes = compile_work.summary_invoke_nodes.saturating_add(
            summary
                .program
                .nodes
                .iter()
                .filter(|node| matches!(node, KernelSummaryNode::Invoke { .. }))
                .count() as u64,
        );
    }
    for (owner_index, owner) in input.owners.iter().enumerate() {
        let owner_id = KernelOwnerId(
            u32::try_from(owner_index).expect("kernel project owner count exceeds u32"),
        );
        let instance = &principals[owner_index];
        let context = OwnerCompileContext {
            initial_state_surface: false,
            owner: owner_id,
            input: owner,
            expressions: &instance.expressions,
            formals: &instance.formals,
            formal_requirements: &instance.formal_requirements,
            expression_modes: &instance.expression_modes,
            formal_modes: &instance.formal_modes,
            formal_mode_sources: &instance.formal_mode_sources,
            static_variants: &instance.static_variants,
            formal_static_variants: &instance.formal_static_variants,
            project: Some(input),
            principals: &principals,
            formal_dependent_results: &formal_dependent_results,
            formal_dependent_expressions: &formal_dependent_expressions,
            external_variables: None,
            syntax_selected_calls: None,
            direct_summaries: &direct_summaries,
        };
        let mut stack = vec![owner_id];
        let specialization = OwnerSpecialization {
            static_variants: instance.static_variants.clone(),
            reachable: (0..owner.nodes.len())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            syntax_selected_calls: vec![false; owner.nodes.len()].into_boxed_slice(),
            invocation_dependencies: formal_dependent_expressions[owner_index].clone(),
            transparent_type_providers: vec![None; owner.nodes.len()].into_boxed_slice(),
        };
        let module = compile_residual_type_module(
            owner_id,
            owner,
            Some(input),
            &principals,
            &formal_dependent_results,
            &formal_dependent_expressions,
            &instance.formal_static_variants,
            &specialization,
            None,
            false,
        )?;
        compile_work.residual_type_modules = compile_work.residual_type_modules.saturating_add(1);
        compile_work.residual_module_operations = compile_work
            .residual_module_operations
            .saturating_add(module.component.operation_count() as u64);
        compile_work.residual_module_terms = compile_work
            .residual_module_terms
            .saturating_add(module.component.terms().len() as u64);
        let external_variables = principal_external_variables(owner, input, &principals)?;
        append_residual_type_frame(&mut builder, &module, instance, &external_variables)?;
        compile_work.residual_frames = compile_work.residual_frames.saturating_add(1);
        for (index, node) in owner.nodes.iter().enumerate() {
            if matches!(node.kind, KernelOwnerNodeKind::UserCall { .. }) {
                compile_node(
                    &mut builder,
                    &mut mode_builder,
                    &mut invocations,
                    &mut specializations,
                    &mut residual_modules,
                    &mut compile_work,
                    &context,
                    &mut stack,
                    index,
                    node,
                    instance.expressions[index],
                    instance.expression_modes[index],
                    true,
                )?;
            } else {
                let equation = node_mode_equation(&mut mode_builder, &context, index, node)?;
                mode_builder.set(instance.expression_modes[index], equation);
            }
        }
    }
    let modes = mode_builder.solve();
    let owners = input
        .owners
        .iter()
        .enumerate()
        .map(|(owner_index, owner)| {
            let result = checked_expression_index(
                owner.result,
                owner.nodes.len(),
                &format!("project owner {owner_index} result"),
            )?;
            let expressions = principals[owner_index]
                .expressions
                .iter()
                .zip(owner.nodes.iter())
                .map(|(variable, node)| builder.add_output(*variable, node.mode))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Ok(KernelProjectOwnerOutputs {
                result: expressions[result],
                expressions,
                expression_modes: principals[owner_index]
                    .expression_modes
                    .iter()
                    .map(|mode| modes[mode.0 as usize])
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                expression_artifacts: collect_expression_artifacts(owner)?,
                calls: collect_call_artifacts(owner)?,
                effects: collect_host_effect_artifacts(owner)?,
            })
        })
        .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
    let component = builder.finish();
    let mut frame_counts = HashMap::<*const ComponentProgram, u64>::new();
    for frame in component.residual_frames.iter() {
        *frame_counts.entry(Arc::as_ptr(&frame.module)).or_default() += 1;
        if frame.module.acyclic_initial_operation_count() == frame.module.operation_count() as u64 {
            compile_work.acyclic_residual_frames =
                compile_work.acyclic_residual_frames.saturating_add(1);
        }
    }
    for (key, module) in &residual_modules {
        let operations = module.component.operation_count() as u64;
        let frames = frame_counts
            .get(&Arc::as_ptr(&module.component))
            .copied()
            .unwrap_or_default();
        let linked = operations.saturating_mul(frames);
        if linked > compile_work.dominant_module_linked_operations {
            compile_work.dominant_module_owner = key.target.0 as u64;
            compile_work.dominant_module_operations = operations;
            compile_work.dominant_module_frames = frames;
            compile_work.dominant_module_linked_operations = linked;
        }
        let candidate = KernelResidualModuleWork {
            owner: key.target.0,
            operations: u32::try_from(operations)
                .expect("kernel residual module operation count exceeds u32"),
            frames: u32::try_from(frames).expect("kernel residual module frame count exceeds u32"),
            linked_operations: linked,
        };
        if let Some(position) = compile_work
            .residual_module_ranking
            .iter()
            .position(|current| candidate.linked_operations > current.linked_operations)
        {
            for index in (position + 1..KERNEL_RESIDUAL_MODULE_RANKING_LEN).rev() {
                compile_work.residual_module_ranking[index] =
                    compile_work.residual_module_ranking[index - 1];
            }
            compile_work.residual_module_ranking[position] = candidate;
        }
    }
    compile_work.linked_operations = component.operation_count() as u64;
    compile_work.scheduled_work_items = component.scheduled_work_item_count() as u64;
    compile_work.linked_terms = component.terms().len() as u64;
    compile_work.acyclic_initial_operations = component.acyclic_initial_operation_count();
    Ok(KernelProjectProgram {
        component,
        owners: owners.into_boxed_slice(),
        compile_work,
    })
}

#[derive(Clone, Debug)]
struct OwnerInstance {
    expressions: Vec<TypeVariableId>,
    formals: Vec<TypeVariableId>,
    formal_requirements: Vec<TypeVariableId>,
    expression_modes: Arc<[ModeVariableId]>,
    formal_modes: Vec<ModeVariableId>,
    formal_mode_sources: Arc<[ModeSource]>,
    static_variants: Vec<Option<StaticVariantSet>>,
    formal_static_variants: Vec<Option<StaticVariantSet>>,
}

/// One structural flow-mode authority.
///
/// Types already retain their record/list shape while crossing owner calls.
/// Flow modes need the same provenance: a continuous record may contain a
/// `PresentOrAbsent` SOURCE field, and projecting that field must not collapse
/// to the record's root mode. Expression sources retain the compact residual
/// frame needed to follow record fields, collection items, and call formals;
/// opaque roots are used only where no structural syntax exists.
#[derive(Clone, Debug)]
enum ModeSource {
    Root(ModeVariableId),
    Expression {
        owner: KernelOwnerId,
        expression: usize,
        expression_modes: Arc<[ModeVariableId]>,
        formal_sources: Arc<[ModeSource]>,
    },
}

impl ModeSource {
    fn root_mode(&self) -> ModeVariableId {
        match self {
            Self::Root(mode) => *mode,
            Self::Expression {
                expression,
                expression_modes,
                ..
            } => expression_modes[*expression],
        }
    }
}

type StaticVariantSet = BTreeSet<Box<str>>;

#[derive(Clone, Debug)]
struct CallActual {
    variable: TypeVariableId,
    mode: ModeVariableId,
    mode_source: ModeSource,
    static_variants: Option<StaticVariantSet>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct InvocationKey {
    target: KernelOwnerId,
    actuals: Box<[(TypeVariableId, ModeVariableId)]>,
    static_variants: Box<[Option<StaticVariantSet>]>,
    initial_state_surface: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SpecializationKey {
    target: KernelOwnerId,
    static_variants: Box<[Option<StaticVariantSet>]>,
    initial_state_surface: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct OwnerSpecialization {
    static_variants: Vec<Option<StaticVariantSet>>,
    reachable: Box<[usize]>,
    syntax_selected_calls: Box<[bool]>,
    invocation_dependencies: Box<[bool]>,
    transparent_type_providers: Box<[Option<usize>]>,
}

#[derive(Debug)]
struct ResidualTypeModule {
    component: Arc<ComponentProgram>,
    local: OwnerInstance,
    external_variables: Box<[TypeVariableId]>,
    calls: Box<[usize]>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ModeVariableId(u32);

#[derive(Clone, Debug)]
enum ModeEquation {
    Fixed(FlowMode),
    Copy(ModeVariableId),
    Eventful(ModeVariableId),
    Latest(Box<[ModeVariableId]>),
    Call {
        result: FlowMode,
        inputs: Box<[ModeVariableId]>,
    },
}

#[derive(Clone, Debug)]
struct ModeVariable {
    fallback: FlowMode,
    equation: Option<ModeEquation>,
}

#[derive(Default)]
struct ModeProgramBuilder {
    variables: Vec<ModeVariable>,
}

impl ModeProgramBuilder {
    fn new_variable(&mut self, fallback: FlowMode) -> ModeVariableId {
        let id = ModeVariableId(
            u32::try_from(self.variables.len()).expect("kernel mode variable count exceeds u32"),
        );
        self.variables.push(ModeVariable {
            fallback,
            equation: None,
        });
        id
    }

    fn set(&mut self, output: ModeVariableId, equation: ModeEquation) {
        let slot = &mut self.variables[output.0 as usize].equation;
        assert!(slot.is_none(), "kernel mode variable has multiple writers");
        *slot = Some(equation);
    }

    fn solve(self) -> Box<[FlowMode]> {
        let mut reverse = vec![Vec::<usize>::new(); self.variables.len()];
        for (output, variable) in self.variables.iter().enumerate() {
            let inputs: &[ModeVariableId] = match variable.equation.as_ref() {
                Some(ModeEquation::Copy(input)) => std::slice::from_ref(input),
                Some(ModeEquation::Eventful(input)) => std::slice::from_ref(input),
                Some(ModeEquation::Latest(inputs)) => inputs,
                Some(ModeEquation::Call { inputs, .. }) => inputs,
                Some(ModeEquation::Fixed(_)) | None => &[],
            };
            for input in inputs {
                reverse[input.0 as usize].push(output);
            }
        }
        let mut modes = vec![None; self.variables.len()];
        let mut pending = (0..self.variables.len()).collect::<std::collections::VecDeque<_>>();
        let mut queued = vec![true; self.variables.len()];
        while let Some(output) = pending.pop_front() {
            queued[output] = false;
            let next = match self.variables[output].equation.as_ref() {
                Some(ModeEquation::Fixed(mode)) => Some(*mode),
                Some(ModeEquation::Copy(input)) => modes[input.0 as usize],
                Some(ModeEquation::Eventful(input)) => modes[input.0 as usize].filter(|mode| {
                    matches!(mode, FlowMode::TickPresent | FlowMode::PresentOrAbsent)
                }),
                Some(ModeEquation::Latest(inputs)) => {
                    latest_mode(inputs.iter().filter_map(|input| modes[input.0 as usize]))
                }
                Some(ModeEquation::Call { result, inputs }) => inputs
                    .iter()
                    .filter_map(|input| modes[input.0 as usize])
                    .fold(Some(*result), merge_call_mode),
                None => None,
            };
            if modes[output] == next {
                continue;
            }
            modes[output] = next;
            for consumer in &reverse[output] {
                if !queued[*consumer] {
                    queued[*consumer] = true;
                    pending.push_back(*consumer);
                }
            }
        }
        self.variables
            .into_iter()
            .enumerate()
            .map(|(index, variable)| modes[index].unwrap_or(variable.fallback))
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }
}

fn latest_mode(modes: impl IntoIterator<Item = FlowMode>) -> Option<FlowMode> {
    let mut saw_mode = false;
    let mut saw_continuous = false;
    let mut saw_present = false;
    for mode in modes {
        saw_mode = true;
        match mode {
            FlowMode::Continuous => saw_continuous = true,
            FlowMode::TickPresent | FlowMode::PresentOrAbsent => saw_present = true,
            FlowMode::Absent => {}
        }
    }
    saw_mode.then_some(if saw_continuous {
        FlowMode::Continuous
    } else if saw_present {
        FlowMode::PresentOrAbsent
    } else {
        FlowMode::Absent
    })
}

fn merge_call_mode(left: Option<FlowMode>, right: FlowMode) -> Option<FlowMode> {
    match (left, right) {
        (None, mode) => Some(mode),
        (Some(FlowMode::Absent), _) | (_, FlowMode::Absent) => Some(FlowMode::Absent),
        (Some(FlowMode::PresentOrAbsent), _) | (_, FlowMode::PresentOrAbsent) => {
            Some(FlowMode::PresentOrAbsent)
        }
        (Some(FlowMode::TickPresent), _) | (_, FlowMode::TickPresent) => {
            Some(FlowMode::TickPresent)
        }
        (Some(FlowMode::Continuous), FlowMode::Continuous) => Some(FlowMode::Continuous),
    }
}

fn allocate_owner_instance(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    owner: &KernelOwnerProgramInput,
    formal_static_variants: &[Option<StaticVariantSet>],
) -> OwnerInstance {
    let static_variants = infer_static_variants(owner, formal_static_variants);
    allocate_owner_instance_with_static_variants(
        builder,
        mode_builder,
        owner,
        formal_static_variants,
        static_variants,
        None,
    )
}

fn allocate_owner_instance_with_static_variants(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    owner: &KernelOwnerProgramInput,
    formal_static_variants: &[Option<StaticVariantSet>],
    static_variants: Vec<Option<StaticVariantSet>>,
    transparent_type_providers: Option<&[Option<usize>]>,
) -> OwnerInstance {
    let mut expressions = Vec::with_capacity(owner.nodes.len());
    for (index, node) in owner.nodes.iter().enumerate() {
        if let Some(provider) = transparent_type_providers.and_then(|providers| providers[index]) {
            expressions.push(
                *expressions
                    .get(provider)
                    .expect("transparent type provider precedes its match arm"),
            );
        } else if matches!(
            node.kind,
            KernelOwnerNodeKind::FormalRead { .. } | KernelOwnerNodeKind::LexicalRead { .. }
        ) {
            expressions.push(builder.new_variable());
        } else {
            expressions.push(builder.new_authoritative_provider());
        }
    }
    assert_eq!(formal_static_variants.len(), owner.formal_count as usize);
    let expression_modes = owner
        .nodes
        .iter()
        .map(|node| mode_builder.new_variable(node.mode))
        .collect::<Vec<_>>()
        .into();
    let formal_modes = (0..owner.formal_count)
        .map(|_| mode_builder.new_variable(FlowMode::Continuous))
        .collect::<Vec<_>>();
    let formal_mode_sources = formal_modes
        .iter()
        .copied()
        .map(ModeSource::Root)
        .collect::<Vec<_>>()
        .into();
    OwnerInstance {
        expressions,
        formals: (0..owner.formal_count)
            .map(|_| builder.new_contextual_hole())
            .collect(),
        formal_requirements: (0..owner.formal_count)
            .map(|_| builder.new_contextual_hole())
            .collect(),
        expression_modes,
        formal_modes,
        formal_mode_sources,
        static_variants,
        formal_static_variants: formal_static_variants.to_vec(),
    }
}

fn allocate_invocation_owner_instance(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    owner: &KernelOwnerProgramInput,
    principal: &OwnerInstance,
    actuals: &[CallActual],
    formal_static_variants: &[Option<StaticVariantSet>],
    static_variants: Vec<Option<StaticVariantSet>>,
    expression_dependencies: &[bool],
    reachable: &[usize],
    transparent_type_providers: &[Option<usize>],
) -> (OwnerInstance, u64) {
    assert_eq!(actuals.len(), owner.formal_count as usize);
    assert_eq!(expression_dependencies.len(), owner.nodes.len());
    let mut reachable_expressions = vec![false; owner.nodes.len()];
    for index in reachable {
        reachable_expressions[*index] = true;
    }
    let pruned_expressions = reachable_expressions
        .iter()
        .zip(expression_dependencies)
        .filter(|(reachable, dependent)| !**reachable && **dependent)
        .count() as u64;
    let mut expressions = Vec::with_capacity(owner.nodes.len());
    for (index, node) in owner.nodes.iter().enumerate() {
        if !expression_dependencies[index] || !reachable_expressions[index] {
            expressions.push(principal.expressions[index]);
        } else if let Some(provider) = transparent_type_providers[index] {
            expressions.push(
                *expressions
                    .get(provider)
                    .expect("transparent type provider precedes its match arm"),
            );
        } else if matches!(
            node.kind,
            KernelOwnerNodeKind::FormalRead { .. } | KernelOwnerNodeKind::LexicalRead { .. }
        ) {
            expressions.push(builder.new_variable());
        } else {
            expressions.push(builder.new_authoritative_provider());
        }
    }
    let expression_modes = owner
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            if expression_dependencies[index] && reachable_expressions[index] {
                mode_builder.new_variable(node.mode)
            } else {
                principal.expression_modes[index]
            }
        })
        .collect::<Vec<_>>()
        .into();
    (
        OwnerInstance {
            expressions,
            formals: actuals.iter().map(|actual| actual.variable).collect(),
            formal_requirements: (0..owner.formal_count)
                .map(|_| builder.new_contextual_hole())
                .collect(),
            expression_modes,
            formal_modes: actuals.iter().map(|actual| actual.mode).collect(),
            formal_mode_sources: actuals
                .iter()
                .map(|actual| actual.mode_source.clone())
                .collect::<Vec<_>>()
                .into(),
            static_variants,
            formal_static_variants: formal_static_variants.to_vec(),
        },
        pruned_expressions,
    )
}

struct OwnerCompileContext<'a> {
    initial_state_surface: bool,
    owner: KernelOwnerId,
    input: &'a KernelOwnerProgramInput,
    expressions: &'a [TypeVariableId],
    formals: &'a [TypeVariableId],
    formal_requirements: &'a [TypeVariableId],
    expression_modes: &'a Arc<[ModeVariableId]>,
    formal_modes: &'a [ModeVariableId],
    formal_mode_sources: &'a Arc<[ModeSource]>,
    static_variants: &'a [Option<StaticVariantSet>],
    formal_static_variants: &'a [Option<StaticVariantSet>],
    project: Option<&'a KernelProjectProgramInput>,
    principals: &'a [OwnerInstance],
    formal_dependent_results: &'a [bool],
    formal_dependent_expressions: &'a [Box<[bool]>],
    external_variables: Option<&'a [TypeVariableId]>,
    syntax_selected_calls: Option<&'a [bool]>,
    direct_summaries: &'a [Option<Arc<CompiledDirectSummary>>],
}

fn validate_owner_input(
    owner_id: KernelOwnerId,
    owner: &KernelOwnerProgramInput,
    principals: &[OwnerInstance],
) -> Result<(), KernelOwnerBuildError> {
    checked_expression_index(owner.result, owner.nodes.len(), "project owner result")?;
    for (index, external) in owner.external_expressions.iter().enumerate() {
        let Some(target) = principals.get(external.owner.0 as usize) else {
            return Err(KernelOwnerBuildError::new(format!(
                "project owner {} external expression {index} targets missing owner {}",
                owner_id.0, external.owner.0
            )));
        };
        if let KernelExternalTarget::Expression(expression) = external.target {
            checked_expression_index(
                expression,
                target.expressions.len(),
                &format!("project owner {} external expression {index}", owner_id.0),
            )?;
        }
    }
    Ok(())
}

fn edge_static_variants(
    context: &OwnerCompileContext<'_>,
    edge: &KernelOwnerInputEdge,
) -> Option<StaticVariantSet> {
    let expression = edge.expression.0 as usize;
    if expression < context.input.nodes.len() {
        return context.static_variants[expression].clone();
    }
    let external = context
        .input
        .external_expressions
        .get(expression.checked_sub(context.input.nodes.len())?)?;
    let owner = context.principals.get(external.owner.0 as usize)?;
    let expression = match external.target {
        KernelExternalTarget::Expression(expression) => expression.0 as usize,
        KernelExternalTarget::Result => {
            context
                .project?
                .owners
                .get(external.owner.0 as usize)?
                .result
                .0 as usize
        }
    };
    owner.static_variants.get(expression)?.clone()
}

fn infer_static_variants(
    owner: &KernelOwnerProgramInput,
    formal_static_variants: &[Option<StaticVariantSet>],
) -> Vec<Option<StaticVariantSet>> {
    let mut variants = vec![None; owner.nodes.len()];
    for (index, node) in owner.nodes.iter().enumerate() {
        variants[index] = match &node.kind {
            KernelOwnerNodeKind::Known(Type::VariantSet(values))
            | KernelOwnerNodeKind::Source(Type::VariantSet(values)) => values
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) | Variant::Tagged { tag, .. } => {
                        Some(tag.clone().into_boxed_str())
                    }
                })
                .collect::<Option<StaticVariantSet>>(),
            KernelOwnerNodeKind::Tag(tag) => Some(BTreeSet::from([tag.clone()])),
            KernelOwnerNodeKind::FormalRead { formal, fields } if fields.is_empty() => {
                formal_static_variants
                    .get(*formal as usize)
                    .cloned()
                    .flatten()
            }
            KernelOwnerNodeKind::Infix { operation } if infix_returns_bool(operation) => {
                Some(BTreeSet::from(["False".into(), "True".into()]))
            }
            KernelOwnerNodeKind::PureBuiltin {
                kind: KernelPureBuiltinKind::TextPredicate | KernelPureBuiltinKind::ListPredicate,
            } => Some(BTreeSet::from(["False".into(), "True".into()])),
            _ => None,
        };
    }
    for _ in 0..=owner.nodes.len() {
        let mut changed = false;
        for (index, node) in owner.nodes.iter().enumerate() {
            let next = match &node.kind {
                KernelOwnerNodeKind::Block => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::BlockResult)
                    })
                }
                KernelOwnerNodeKind::LexicalRead { fields }
                | KernelOwnerNodeKind::ValueRead { fields, .. }
                | KernelOwnerNodeKind::DerivedRead { fields }
                    if fields.is_empty() =>
                {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::ReadProvider)
                    })
                }
                KernelOwnerNodeKind::Latest => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::LatestBranch)
                    })
                }
                KernelOwnerNodeKind::When => {
                    let arms = possible_when_arm_expressions(owner, index, &variants);
                    merge_static_expression_variants(&arms, &variants)
                }
                KernelOwnerNodeKind::Then => {
                    let has_output = node
                        .inputs
                        .iter()
                        .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ThenOutput));
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::ThenOutput)
                            || (!has_output && matches!(role, KernelOwnerEdgeRole::ThenInput))
                    })
                }
                KernelOwnerNodeKind::Draining => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::DrainingInput)
                    })
                }
                KernelOwnerNodeKind::MatchArm { .. } => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::MatchOutput)
                    })
                }
                KernelOwnerNodeKind::Arrow => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::ArrowOutput)
                    })
                }
                _ => variants[index].clone(),
            };
            if variants[index] != next {
                variants[index] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    variants
}

fn merge_static_edge_variants(
    owner: &KernelOwnerProgramInput,
    node: &KernelOwnerNode,
    variants: &[Option<StaticVariantSet>],
    selected: impl Fn(&KernelOwnerEdgeRole) -> bool,
) -> Option<StaticVariantSet> {
    let expressions = node
        .inputs
        .iter()
        .filter(|edge| selected(&edge.role))
        .map(|edge| edge.expression.0 as usize)
        .collect::<Vec<_>>();
    if expressions
        .iter()
        .any(|expression| *expression >= owner.nodes.len())
    {
        return None;
    }
    merge_static_expression_variants(&expressions, variants)
}

fn merge_static_expression_variants(
    expressions: &[usize],
    variants: &[Option<StaticVariantSet>],
) -> Option<StaticVariantSet> {
    if expressions.is_empty() {
        return None;
    }
    let mut merged = BTreeSet::new();
    for expression in expressions {
        merged.extend(variants.get(*expression)?.as_ref()?.iter().cloned());
    }
    Some(merged)
}

fn possible_when_arm_expressions(
    owner: &KernelOwnerProgramInput,
    when: usize,
    variants: &[Option<StaticVariantSet>],
) -> Vec<usize> {
    let node = &owner.nodes[when];
    let arms = node
        .inputs
        .iter()
        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenArm))
        .map(|edge| edge.expression.0 as usize)
        .filter(|arm| *arm < owner.nodes.len())
        .collect::<Vec<_>>();
    let selector = node
        .inputs
        .iter()
        .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
        .map(|edge| edge.expression.0 as usize)
        .filter(|selector| *selector < owner.nodes.len())
        .and_then(|selector| variants.get(selector))
        .and_then(Option::as_ref);
    let Some(selector) = selector else {
        return arms;
    };
    let mut selected = BTreeSet::new();
    for tag in selector {
        if let Some(arm) = arms.iter().copied().find(|arm| {
            matches!(
                &owner.nodes[*arm].kind,
                KernelOwnerNodeKind::MatchArm { pattern }
                    if static_pattern_accepts_tag(pattern, tag)
            )
        }) {
            selected.insert(arm);
        }
    }
    selected.into_iter().collect()
}

fn static_pattern_accepts_tag(pattern: &KernelPattern, tag: &str) -> bool {
    match pattern {
        KernelPattern::Wildcard | KernelPattern::Binding { .. } => true,
        KernelPattern::Tag { name, .. } => name.as_ref() == tag,
        KernelPattern::Number
        | KernelPattern::Text
        | KernelPattern::Bits { .. }
        | KernelPattern::Invalid => false,
    }
}

fn reachable_owner_nodes(
    owner: &KernelOwnerProgramInput,
    result: usize,
    variants: &[Option<StaticVariantSet>],
) -> BTreeSet<usize> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![result];
    while let Some(index) = pending.pop() {
        if !reachable.insert(index) {
            continue;
        }
        let node = &owner.nodes[index];
        if matches!(node.kind, KernelOwnerNodeKind::When) {
            pending.extend(
                node.inputs
                    .iter()
                    .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
                    .map(|edge| edge.expression.0 as usize)
                    .filter(|input| *input < owner.nodes.len()),
            );
            pending.extend(possible_when_arm_expressions(owner, index, variants));
        } else {
            pending.extend(
                node.inputs
                    .iter()
                    .map(|edge| edge.expression.0 as usize)
                    .filter(|input| *input < owner.nodes.len()),
            );
        }
    }
    reachable
}

/// Mark calls whose result is constructed under a formal-derived `WHEN` arm.
///
/// This is occurrence provenance, not ordinary reachability. A definition
/// retains every state update in its public result. An invocation evaluates a
/// branch as a construction site, so stateful callees below that constructor
/// expose their initializer surface; the separately owned state artifact keeps
/// the complete update domain. This does not require a parallel static-type
/// evaluator merely to rediscover a tag nested inside another call's record.
fn syntax_selected_call_nodes(
    owner: &KernelOwnerProgramInput,
    variants: &[Option<StaticVariantSet>],
    formal_dependencies: &[bool],
) -> Box<[bool]> {
    let mut selected_calls = vec![false; owner.nodes.len()];
    for (when, node) in owner.nodes.iter().enumerate() {
        if !matches!(node.kind, KernelOwnerNodeKind::When) {
            continue;
        }
        let Some(selector) = node
            .inputs
            .iter()
            .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
            .map(|edge| edge.expression.0 as usize)
            .filter(|selector| *selector < owner.nodes.len())
        else {
            continue;
        };
        if !formal_dependencies.get(selector).copied().unwrap_or(false) {
            continue;
        }
        let arms = possible_when_arm_expressions(owner, when, variants);
        for arm in arms {
            for expression in reachable_owner_nodes(owner, arm, variants) {
                if matches!(
                    owner.nodes[expression].kind,
                    KernelOwnerNodeKind::UserCall { .. }
                ) {
                    selected_calls[expression] = true;
                }
            }
        }
    }
    selected_calls.into_boxed_slice()
}

fn invocation_expression_dependencies(
    owner: &KernelOwnerProgramInput,
    formal_dependencies: &[bool],
    syntax_selected_calls: &[bool],
) -> Box<[bool]> {
    let mut dependent = formal_dependencies
        .iter()
        .zip(syntax_selected_calls)
        .map(|(formal, selected)| *formal || *selected)
        .collect::<Vec<_>>();
    loop {
        let mut changed = false;
        for (index, node) in owner.nodes.iter().enumerate() {
            if dependent[index] {
                continue;
            }
            if node.inputs.iter().any(|edge| {
                dependent
                    .get(edge.expression.0 as usize)
                    .copied()
                    .unwrap_or(false)
            }) {
                dependent[index] = true;
                changed = true;
            }
        }
        if !changed {
            return dependent.into_boxed_slice();
        }
    }
}

/// Type-only aliases that need no invocation-local publication.
///
/// Principal frames still materialize every authored expression for checked
/// artifacts. Specialized invocation frames may point a transparent match arm
/// directly at its sole output, because the enclosing SELECT owns detachment
/// and publication. Field-backed delimiter arms remain real record producers.
fn transparent_type_providers(owner: &KernelOwnerProgramInput) -> Box<[Option<usize>]> {
    let mut has_use = vec![false; owner.nodes.len()];
    let mut select_only = vec![true; owner.nodes.len()];
    for node in &owner.nodes {
        for edge in &node.inputs {
            let expression = edge.expression.0 as usize;
            if expression >= owner.nodes.len() {
                continue;
            }
            has_use[expression] = true;
            select_only[expression] &= matches!(edge.role, KernelOwnerEdgeRole::WhenArm);
        }
    }
    owner
        .nodes
        .iter()
        .enumerate()
        .map(|(expression, node)| {
            let KernelOwnerNodeKind::MatchArm { .. } = node.kind else {
                return None;
            };
            let mut outputs = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput));
            let output = outputs.next()?;
            let provider = output.expression.0 as usize;
            (outputs.next().is_none()
                && node.inputs.len() == 1
                && provider < expression
                && has_use[expression]
                && select_only[expression]
                && owner.result.0 as usize != expression)
                .then_some(provider)
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

/// Return whether each expression requires an invocation-local cell.
///
/// This intentionally walks the unspecialized local graph rather than the
/// statically sliced occurrence graph. In addition to formal-dependent values,
/// every HOLD is occurrence-owned even when its initializer and updates are
/// syntactically formal-independent. A principal result may be reused only
/// when every invocation computes the same non-stateful value and imposes no
/// formal requirement. User-call inherited formals are implicit inputs, so
/// they are treated as dependencies even though they do not have an ordinary
/// edge.
fn owner_expressions_depend_on_formals(owner: &KernelOwnerProgramInput) -> Box<[bool]> {
    let mut dependent = owner
        .nodes
        .iter()
        .map(|node| {
            matches!(
                node.kind,
                KernelOwnerNodeKind::FormalRead { .. }
                    | KernelOwnerNodeKind::Hold
                    | KernelOwnerNodeKind::UserCall {
                        inherited_formal: Some(_),
                        ..
                    }
            )
        })
        .collect::<Vec<_>>();
    loop {
        let mut changed = false;
        for (index, node) in owner.nodes.iter().enumerate() {
            if dependent[index] {
                continue;
            }
            if node.inputs.iter().any(|edge| {
                dependent
                    .get(edge.expression.0 as usize)
                    .copied()
                    .unwrap_or(false)
            }) {
                dependent[index] = true;
                changed = true;
            }
        }
        if !changed {
            return dependent.into_boxed_slice();
        }
    }
}

fn compile_residual_type_module(
    owner_id: KernelOwnerId,
    owner: &KernelOwnerProgramInput,
    project: Option<&KernelProjectProgramInput>,
    principals: &[OwnerInstance],
    formal_dependent_results: &[bool],
    formal_dependent_expressions: &[Box<[bool]>],
    formal_static_variants: &[Option<StaticVariantSet>],
    specialization: &OwnerSpecialization,
    invocation_dependencies: Option<&[bool]>,
    initial_state_surface: bool,
) -> Result<Arc<ResidualTypeModule>, KernelOwnerBuildError> {
    let mut builder = ComponentProgramBuilder::new();
    let mut mode_builder = ModeProgramBuilder::default();
    let residual_transparent_type_providers = invocation_dependencies.map(|dependencies| {
        specialization
            .transparent_type_providers
            .iter()
            .copied()
            .enumerate()
            .map(|(expression, provider)| {
                (dependencies[expression]
                    && specialization.reachable.binary_search(&expression).is_ok())
                .then_some(provider)
                .flatten()
            })
            .collect::<Vec<_>>()
    });
    let local = allocate_owner_instance_with_static_variants(
        &mut builder,
        &mut mode_builder,
        owner,
        formal_static_variants,
        specialization.static_variants.clone(),
        residual_transparent_type_providers.as_deref(),
    );
    let external_variables = owner
        .external_expressions
        .iter()
        .map(|_| builder.new_contextual_hole())
        .collect::<Vec<_>>();
    let context = OwnerCompileContext {
        initial_state_surface,
        owner: owner_id,
        input: owner,
        expressions: &local.expressions,
        formals: &local.formals,
        formal_requirements: &local.formal_requirements,
        expression_modes: &local.expression_modes,
        formal_modes: &local.formal_modes,
        formal_mode_sources: &local.formal_mode_sources,
        static_variants: &local.static_variants,
        formal_static_variants: &local.formal_static_variants,
        project,
        principals,
        formal_dependent_results,
        formal_dependent_expressions,
        external_variables: Some(&external_variables),
        syntax_selected_calls: Some(&specialization.syntax_selected_calls),
        direct_summaries: &[],
    };
    let mut invocations = HashMap::new();
    let mut specializations = HashMap::new();
    let mut residual_modules = HashMap::new();
    let mut compile_work = KernelCompileWork::default();
    let mut stack = vec![owner_id];
    let mut calls = Vec::new();
    for index in specialization.reachable.iter().copied() {
        if invocation_dependencies.is_some_and(|dependencies| !dependencies[index]) {
            continue;
        }
        if residual_transparent_type_providers
            .as_ref()
            .is_some_and(|providers| providers[index].is_some())
        {
            continue;
        }
        let node = owner.nodes.get(index).ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "residual module owner {} references missing node {index}",
                owner_id.0
            ))
        })?;
        if matches!(node.kind, KernelOwnerNodeKind::UserCall { .. }) {
            calls.push(index);
            continue;
        }
        compile_node(
            &mut builder,
            &mut mode_builder,
            &mut invocations,
            &mut specializations,
            &mut residual_modules,
            &mut compile_work,
            &context,
            &mut stack,
            index,
            node,
            local.expressions[index],
            local.expression_modes[index],
            false,
        )?;
    }
    Ok(Arc::new(ResidualTypeModule {
        component: Arc::new(builder.finish()),
        local,
        external_variables: external_variables.into_boxed_slice(),
        calls: calls.into_boxed_slice(),
    }))
}

fn append_residual_type_frame(
    builder: &mut ComponentProgramBuilder,
    module: &ResidualTypeModule,
    instance: &OwnerInstance,
    external_variables: &[TypeVariableId],
) -> Result<(), KernelOwnerBuildError> {
    if external_variables.len() != module.external_variables.len() {
        return Err(KernelOwnerBuildError::new(format!(
            "residual frame supplies {} external variables for {} module imports",
            external_variables.len(),
            module.external_variables.len()
        )));
    }
    let mut variables = vec![None; module.component.variable_count()];
    let mut map = |local: TypeVariableId,
                   global: TypeVariableId,
                   role: &str|
     -> Result<(), KernelOwnerBuildError> {
        let slot = variables.get_mut(local.0 as usize).ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "residual module {role} variable {} is outside its frame",
                local.0
            ))
        })?;
        if slot
            .replace(global)
            .is_some_and(|previous| previous != global)
        {
            return Err(KernelOwnerBuildError::new(format!(
                "residual module {role} variable {} has conflicting frame mappings",
                local.0
            )));
        }
        Ok(())
    };
    for (local, global) in module.local.expressions.iter().zip(&instance.expressions) {
        map(*local, *global, "expression")?;
    }
    for (local, global) in module.local.formals.iter().zip(&instance.formals) {
        map(*local, *global, "formal")?;
    }
    for (local, global) in module
        .local
        .formal_requirements
        .iter()
        .zip(&instance.formal_requirements)
    {
        map(*local, *global, "formal requirement")?;
    }
    for (local, global) in module.external_variables.iter().zip(external_variables) {
        map(*local, *global, "external")?;
    }
    for (index, spec) in module
        .component
        .variable_specs()
        .iter()
        .copied()
        .enumerate()
    {
        if variables[index].is_none() {
            variables[index] = Some(builder.new_variable_with(spec));
        }
    }
    let variables = variables
        .into_iter()
        .map(|variable| variable.expect("every residual frame variable is mapped"))
        .collect::<Vec<_>>();
    builder.add_residual_frame(Arc::clone(&module.component), variables);
    Ok(())
}

fn principal_external_variables(
    owner: &KernelOwnerProgramInput,
    project: &KernelProjectProgramInput,
    principals: &[OwnerInstance],
) -> Result<Vec<TypeVariableId>, KernelOwnerBuildError> {
    owner
        .external_expressions
        .iter()
        .map(|external| {
            let target_instance = principals.get(external.owner.0 as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "residual module imports missing owner {}",
                    external.owner.0
                ))
            })?;
            let target_owner = project
                .owners
                .get(external.owner.0 as usize)
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "residual module imports missing owner input {}",
                        external.owner.0
                    ))
                })?;
            let expression = match external.target {
                KernelExternalTarget::Expression(expression) => checked_expression_index(
                    expression,
                    target_owner.nodes.len(),
                    "residual external expression",
                )?,
                KernelExternalTarget::Result => checked_expression_index(
                    target_owner.result,
                    target_owner.nodes.len(),
                    "residual external result",
                )?,
            };
            target_instance
                .expressions
                .get(expression)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "residual module imports missing owner {} expression {expression}",
                        external.owner.0
                    ))
                })
        })
        .collect()
}

fn instantiate_owner(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    invocations: &mut HashMap<InvocationKey, OwnerInstance>,
    specializations: &mut HashMap<SpecializationKey, OwnerSpecialization>,
    residual_modules: &mut HashMap<SpecializationKey, Arc<ResidualTypeModule>>,
    compile_work: &mut KernelCompileWork,
    project: &KernelProjectProgramInput,
    principals: &[OwnerInstance],
    formal_dependent_results: &[bool],
    formal_dependent_expressions: &[Box<[bool]>],
    direct_summaries: &[Option<Arc<CompiledDirectSummary>>],
    target: KernelOwnerId,
    actuals: &[CallActual],
    initial_state_surface: bool,
    stack: &mut Vec<KernelOwnerId>,
) -> Result<OwnerInstance, KernelOwnerBuildError> {
    compile_work.max_call_depth = compile_work.max_call_depth.max(stack.len() as u64);
    let owner = project.owners.get(target.0 as usize).ok_or_else(|| {
        KernelOwnerBuildError::new(format!("user call targets missing owner {}", target.0))
    })?;
    if actuals.len() != owner.formal_count as usize {
        return Err(KernelOwnerBuildError::new(format!(
            "user call to owner {} supplies {} actuals for {} formals",
            target.0,
            actuals.len(),
            owner.formal_count
        )));
    }
    if stack.contains(&target) {
        return principals.get(target.0 as usize).cloned().ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "recursive user call targets missing principal owner {}",
                target.0
            ))
        });
    }

    let formal_static_variants = actuals
        .iter()
        .map(|actual| actual.static_variants.clone())
        .collect::<Vec<_>>();
    let specialization_key = SpecializationKey {
        target,
        static_variants: formal_static_variants.clone().into_boxed_slice(),
        initial_state_surface,
    };
    let specialization = if let Some(specialization) = specializations.get(&specialization_key) {
        compile_work.reused_specialization_plans =
            compile_work.reused_specialization_plans.saturating_add(1);
        specialization.clone()
    } else {
        let static_variants = infer_static_variants(owner, &formal_static_variants);
        let result =
            checked_expression_index(owner.result, owner.nodes.len(), "specialized owner result")?;
        let syntax_selected_calls = syntax_selected_call_nodes(
            owner,
            &static_variants,
            &formal_dependent_expressions[target.0 as usize],
        );
        let specialization = OwnerSpecialization {
            reachable: reachable_owner_nodes(owner, result, &static_variants)
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            invocation_dependencies: invocation_expression_dependencies(
                owner,
                &formal_dependent_expressions[target.0 as usize],
                &syntax_selected_calls,
            ),
            syntax_selected_calls,
            static_variants,
            transparent_type_providers: transparent_type_providers(owner),
        };
        specializations.insert(specialization_key.clone(), specialization.clone());
        compile_work.specialization_plans = compile_work.specialization_plans.saturating_add(1);
        specialization
    };
    let key = InvocationKey {
        target,
        actuals: actuals
            .iter()
            .map(|actual| (actual.variable, actual.mode))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        static_variants: formal_static_variants.clone().into_boxed_slice(),
        initial_state_surface,
    };
    let owns_state = owner
        .nodes
        .iter()
        .any(|node| matches!(node.kind, KernelOwnerNodeKind::Hold));
    if !owns_state {
        if let Some(instance) = invocations.get(&key) {
            compile_work.reused_invocation_frames =
                compile_work.reused_invocation_frames.saturating_add(1);
            return Ok(instance.clone());
        }
    }
    compile_work.invocation_frames = compile_work.invocation_frames.saturating_add(1);
    // Invocation frames borrow the caller's provider/mode roots directly.
    // The detached FormalRead occurrences and private requirement roots below
    // still isolate consumers; the redundant actual -> fresh-formal publish
    // layer only obscured identical applications from composition.
    let expression_dependencies = &specialization.invocation_dependencies;
    let principal = &principals[target.0 as usize];
    let (instance, pruned_expressions) = allocate_invocation_owner_instance(
        builder,
        mode_builder,
        owner,
        principal,
        actuals,
        &formal_static_variants,
        specialization.static_variants.clone(),
        expression_dependencies,
        &specialization.reachable,
        &specialization.transparent_type_providers,
    );
    compile_work.pruned_invocation_expressions = compile_work
        .pruned_invocation_expressions
        .saturating_add(pruned_expressions);
    compile_work.principal_expression_reuses =
        compile_work.principal_expression_reuses.saturating_add(
            specialization
                .reachable
                .iter()
                .filter(|index| !expression_dependencies[**index])
                .count() as u64,
        );
    let module = if let Some(module) = residual_modules.get(&specialization_key) {
        Arc::clone(module)
    } else {
        let module = compile_residual_type_module(
            target,
            owner,
            Some(project),
            principals,
            formal_dependent_results,
            formal_dependent_expressions,
            &formal_static_variants,
            &specialization,
            Some(expression_dependencies),
            initial_state_surface,
        )?;
        compile_work.residual_type_modules = compile_work.residual_type_modules.saturating_add(1);
        compile_work.residual_module_operations = compile_work
            .residual_module_operations
            .saturating_add(module.component.operation_count() as u64);
        compile_work.residual_module_terms = compile_work
            .residual_module_terms
            .saturating_add(module.component.terms().len() as u64);
        residual_modules.insert(specialization_key, Arc::clone(&module));
        module
    };
    let external_variables = principal_external_variables(owner, project, principals)?;
    stack.push(target);
    let result = (|| {
        let context = OwnerCompileContext {
            initial_state_surface,
            owner: target,
            input: owner,
            expressions: &instance.expressions,
            formals: &instance.formals,
            formal_requirements: &instance.formal_requirements,
            expression_modes: &instance.expression_modes,
            formal_modes: &instance.formal_modes,
            formal_mode_sources: &instance.formal_mode_sources,
            static_variants: &instance.static_variants,
            formal_static_variants: &instance.formal_static_variants,
            project: Some(project),
            principals,
            formal_dependent_results,
            formal_dependent_expressions,
            external_variables: None,
            syntax_selected_calls: Some(&specialization.syntax_selected_calls),
            direct_summaries,
        };
        append_residual_type_frame(builder, &module, &instance, &external_variables)?;
        compile_work.residual_frames = compile_work.residual_frames.saturating_add(1);
        for index in specialization.reachable.iter().copied() {
            if !expression_dependencies[index] {
                continue;
            }
            let node = &owner.nodes[index];
            if matches!(node.kind, KernelOwnerNodeKind::UserCall { .. }) {
                compile_node(
                    builder,
                    mode_builder,
                    invocations,
                    specializations,
                    residual_modules,
                    compile_work,
                    &context,
                    stack,
                    index,
                    node,
                    instance.expressions[index],
                    instance.expression_modes[index],
                    true,
                )?;
            } else {
                let equation = node_mode_equation(mode_builder, &context, index, node)?;
                mode_builder.set(instance.expression_modes[index], equation);
            }
        }
        Ok::<(), KernelOwnerBuildError>(())
    })();
    let popped = stack.pop();
    debug_assert_eq!(popped, Some(target));
    result?;
    for (actual, requirement) in actuals.iter().zip(&instance.formal_requirements) {
        // A closed or directionally derived actual is provider authority, not
        // a writable formal scaffold. Callee requirements remain useful for
        // open caller formals, but must never widen a concrete occurrence.
        if builder.is_authoritative(actual.variable) {
            continue;
        }
        let actual = builder.variable_term(actual.variable);
        let requirement = builder.variable_term(*requirement);
        builder.add_unify(actual, requirement);
    }
    if !owns_state {
        invocations.insert(key, instance.clone());
    }
    Ok(instance)
}

fn direct_result_summary_supported(
    project: &KernelProjectProgramInput,
    owner_id: KernelOwnerId,
    expression: usize,
    active: &mut BTreeSet<(KernelOwnerId, usize)>,
) -> bool {
    let Some(owner) = project.owners.get(owner_id.0 as usize) else {
        return false;
    };
    let Some(node) = owner.nodes.get(expression) else {
        return false;
    };
    if !active.insert((owner_id, expression)) {
        return false;
    }
    let child = |edge: &KernelOwnerInputEdge, active: &mut BTreeSet<(KernelOwnerId, usize)>| {
        direct_result_summary_supported(project, owner_id, edge.expression.0 as usize, active)
    };
    let supported = match &node.kind {
        KernelOwnerNodeKind::Known(ty) | KernelOwnerNodeKind::Source(ty) => {
            type_is_recursively_closed(ty)
        }
        KernelOwnerNodeKind::Absent
        | KernelOwnerNodeKind::Text
        | KernelOwnerNodeKind::Number
        | KernelOwnerNodeKind::Byte
        | KernelOwnerNodeKind::Bits(_)
        | KernelOwnerNodeKind::Tag(_)
        | KernelOwnerNodeKind::FormalRead { .. } => node.inputs.is_empty(),
        KernelOwnerNodeKind::TextTemplate => node.inputs.iter().all(|edge| {
            matches!(edge.role, KernelOwnerEdgeRole::TextDynamic) && child(edge, active)
        }),
        KernelOwnerNodeKind::Record { .. } => node.inputs.iter().all(|edge| {
            matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. }) && child(edge, active)
        }),
        KernelOwnerNodeKind::Collection { kind, .. } => match kind {
            KernelCollectionKind::List
            | KernelCollectionKind::Bytes
            | KernelCollectionKind::Set => node.inputs.iter().all(|edge| {
                matches!(edge.role, KernelOwnerEdgeRole::CollectionItem) && child(edge, active)
            }),
            KernelCollectionKind::Map => node.inputs.iter().all(|edge| {
                matches!(edge.role, KernelOwnerEdgeRole::MapEntry) && child(edge, active)
            }),
        },
        KernelOwnerNodeKind::MapEntry => node.inputs.iter().all(|edge| {
            matches!(
                edge.role,
                KernelOwnerEdgeRole::MapKey | KernelOwnerEdgeRole::MapValue
            ) && child(edge, active)
        }),
        KernelOwnerNodeKind::Block => {
            let results = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::BlockResult))
                .collect::<Vec<_>>();
            matches!(results.as_slice(), [result] if child(result, active))
        }
        KernelOwnerNodeKind::Draining => {
            let inputs = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::DrainingInput))
                .collect::<Vec<_>>();
            matches!(inputs.as_slice(), [input] if child(input, active))
        }
        KernelOwnerNodeKind::LexicalRead { .. }
        | KernelOwnerNodeKind::ValueRead { .. }
        | KernelOwnerNodeKind::DerivedRead { .. } => {
            let providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
                .collect::<Vec<_>>();
            matches!(providers.as_slice(), [provider] if {
                let reference = provider.expression.0 as usize;
                if reference < owner.nodes.len() {
                    direct_result_summary_supported(project, owner_id, reference, active)
                } else {
                    owner
                        .external_expressions
                        .get(reference - owner.nodes.len())
                        .is_some()
                }
            })
        }
        KernelOwnerNodeKind::UserCall { target, .. } => {
            let Some(target_owner) = project.owners.get(target.0 as usize) else {
                active.remove(&(owner_id, expression));
                return false;
            };
            node.inputs.iter().all(|edge| {
                matches!(edge.role, KernelOwnerEdgeRole::CallArgument { .. }) && child(edge, active)
            }) && direct_result_summary_supported(
                project,
                *target,
                target_owner.result.0 as usize,
                active,
            )
        }
        KernelOwnerNodeKind::RenderConstructor { .. } => node.inputs.iter().all(|edge| {
            matches!(edge.role, KernelOwnerEdgeRole::AbiArgument { .. }) && child(edge, active)
        }),
        KernelOwnerNodeKind::PureBuiltin { kind }
            if direct_summary_fixed_builtin_supported(*kind) =>
        {
            node.inputs.iter().all(|edge| {
                matches!(edge.role, KernelOwnerEdgeRole::AbiArgument { .. }) && child(edge, active)
            })
        }
        KernelOwnerNodeKind::When => {
            let selectors = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
                .collect::<Vec<_>>();
            matches!(selectors.as_slice(), [selector] if child(selector, active))
                && node.inputs.iter().all(|edge| match edge.role {
                    KernelOwnerEdgeRole::WhenInput => true,
                    KernelOwnerEdgeRole::WhenArm => child(edge, active),
                    _ => false,
                })
        }
        KernelOwnerNodeKind::MatchArm { .. } => {
            let outputs = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
                .collect::<Vec<_>>();
            if !outputs.is_empty() {
                matches!(outputs.as_slice(), [output] if node.inputs.len() == 1 && child(output, active))
            } else if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. }))
            {
                node.inputs.iter().all(|edge| {
                    matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. })
                        && child(edge, active)
                })
            } else {
                node.inputs.is_empty()
            }
        }
        KernelOwnerNodeKind::Then => {
            let has_output = node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ThenOutput));
            let selected = node
                .inputs
                .iter()
                .filter(|edge| {
                    matches!(edge.role, KernelOwnerEdgeRole::ThenOutput)
                        || (!has_output && matches!(edge.role, KernelOwnerEdgeRole::ThenInput))
                })
                .collect::<Vec<_>>();
            matches!(selected.as_slice(), [value] if child(value, active))
                && node.inputs.iter().all(|edge| {
                    matches!(
                        edge.role,
                        KernelOwnerEdgeRole::ThenInput | KernelOwnerEdgeRole::ThenOutput
                    ) && child(edge, active)
                })
        }
        KernelOwnerNodeKind::Infix { .. } => {
            let left = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::InfixLeft))
                .collect::<Vec<_>>();
            let right = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::InfixRight))
                .collect::<Vec<_>>();
            matches!(left.as_slice(), [left] if child(left, active))
                && matches!(right.as_slice(), [right] if child(right, active))
                && node.inputs.len() == 2
        }
        KernelOwnerNodeKind::Arrow => {
            let outputs = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ArrowOutput))
                .collect::<Vec<_>>();
            matches!(outputs.as_slice(), [output] if node.inputs.len() == 1 && child(output, active))
        }
        KernelOwnerNodeKind::Delimiter | KernelOwnerNodeKind::Unknown => node.inputs.is_empty(),
        _ => false,
    };
    active.remove(&(owner_id, expression));
    supported
}

fn direct_summary_fixed_builtin_supported(kind: KernelPureBuiltinKind) -> bool {
    matches!(
        kind,
        KernelPureBuiltinKind::TextTransform
            | KernelPureBuiltinKind::TextSlice
            | KernelPureBuiltinKind::TextLength
            | KernelPureBuiltinKind::TextConcat
            | KernelPureBuiltinKind::TextPredicate
            | KernelPureBuiltinKind::TextToNumber
            | KernelPureBuiltinKind::NumberToText
            | KernelPureBuiltinKind::NumberMath
            | KernelPureBuiltinKind::NumberRound
            | KernelPureBuiltinKind::NumberProjection
            | KernelPureBuiltinKind::TextJoin
            | KernelPureBuiltinKind::FieldColor
    )
}

/// Tiny summary definitions cost less to inline than to enter through a
/// nested scratch frame. Larger definitions remain shared so call-heavy
/// projects do not duplicate their bytecode at every occurrence.
const SHARED_SUMMARY_MIN_NODES: usize = 128;

#[derive(Clone, Debug)]
enum DirectSummaryInput {
    FormalProjection {
        formal: u32,
        fields: Box<[crate::NameId]>,
    },
    External {
        owner: KernelOwnerId,
        expression: usize,
    },
}

#[derive(Clone, Copy, Debug)]
enum DirectSummaryMode {
    Fixed {
        owner: KernelOwnerId,
        expression: usize,
    },
    Input(u32),
}

#[derive(Clone, Debug)]
struct CompiledDirectSummary {
    program: Arc<KernelSummaryProgram>,
    inputs: Box<[DirectSummaryInput]>,
    result_mode: DirectSummaryMode,
    formal_count: usize,
}

#[derive(Clone, Debug)]
enum PlannedSummaryActual {
    Formal(u32),
    Value(PlannedSummaryValue),
}

#[derive(Clone, Copy, Debug)]
struct PlannedSummaryValue {
    value: KernelSummaryValueId,
    mode: DirectSummaryMode,
    formal_projection_input: Option<u32>,
}

struct DirectSummaryPlanCompiler<'a> {
    builder: &'a mut ComponentProgramBuilder,
    project: &'a KernelProjectProgramInput,
    summaries: &'a [Option<Arc<CompiledDirectSummary>>],
    nodes: Vec<KernelSummaryNode>,
    inputs: Vec<DirectSummaryInput>,
    formal_projection_inputs: HashMap<(u32, Box<[crate::NameId]>), (u32, KernelSummaryValueId)>,
}

impl DirectSummaryPlanCompiler<'_> {
    fn push_node(&mut self, node: KernelSummaryNode) -> KernelSummaryValueId {
        let id = KernelSummaryValueId(
            u32::try_from(self.nodes.len()).expect("kernel summary value count exceeds u32"),
        );
        self.nodes.push(node);
        id
    }

    fn push_formal_projection(
        &mut self,
        formal: u32,
        fields: Box<[crate::NameId]>,
    ) -> PlannedSummaryValue {
        let key = (formal, fields.clone());
        if let Some((input, value)) = self.formal_projection_inputs.get(&key).copied() {
            return PlannedSummaryValue {
                value,
                mode: DirectSummaryMode::Input(input),
                formal_projection_input: Some(input),
            };
        }
        let input =
            u32::try_from(self.inputs.len()).expect("kernel summary input count exceeds u32");
        self.inputs
            .push(DirectSummaryInput::FormalProjection { formal, fields });
        let value = self.push_node(KernelSummaryNode::Input(input));
        self.formal_projection_inputs.insert(key, (input, value));
        PlannedSummaryValue {
            value,
            mode: DirectSummaryMode::Input(input),
            formal_projection_input: Some(input),
        }
    }

    fn project_value(
        &mut self,
        value: PlannedSummaryValue,
        fields: &[Box<str>],
        mode: DirectSummaryMode,
    ) -> Option<PlannedSummaryValue> {
        if fields.is_empty() {
            return Some(value);
        }
        let fields = fields
            .iter()
            .map(|field| self.builder.terms_mut().intern_name(field))
            .collect::<Vec<_>>();
        if let Some(input) = value.formal_projection_input {
            let DirectSummaryInput::FormalProjection {
                formal,
                fields: prefix,
            } = self.inputs.get(input as usize)?.clone()
            else {
                return None;
            };
            let mut projection = prefix.into_vec();
            projection.extend(fields);
            return Some(self.push_formal_projection(formal, projection.into_boxed_slice()));
        }
        Some(PlannedSummaryValue {
            value: self.push_node(KernelSummaryNode::Projection {
                provider: value.value,
                fields: fields.into_boxed_slice(),
            }),
            mode,
            formal_projection_input: None,
        })
    }

    fn project_interned_formal_value(
        &mut self,
        value: PlannedSummaryValue,
        fields: &[crate::NameId],
    ) -> Option<PlannedSummaryValue> {
        if fields.is_empty() {
            return Some(value);
        }
        let input = value.formal_projection_input?;
        let DirectSummaryInput::FormalProjection {
            formal,
            fields: prefix,
        } = self.inputs.get(input as usize)?.clone()
        else {
            return None;
        };
        let mut projection = prefix.into_vec();
        projection.extend_from_slice(fields);
        Some(self.push_formal_projection(formal, projection.into_boxed_slice()))
    }

    fn compile_shared_invoke(
        &mut self,
        summary: &CompiledDirectSummary,
        actuals: &[PlannedSummaryActual],
    ) -> Option<PlannedSummaryValue> {
        if actuals.len() != summary.formal_count {
            return None;
        }
        let mut values = Vec::with_capacity(summary.inputs.len());
        let mut modes = Vec::with_capacity(summary.inputs.len());
        for input in summary.inputs.iter() {
            let value = match input {
                DirectSummaryInput::FormalProjection { formal, fields } => {
                    match actuals.get(*formal as usize)? {
                        PlannedSummaryActual::Formal(formal) => {
                            self.push_formal_projection(*formal, fields.clone())
                        }
                        PlannedSummaryActual::Value(value) => {
                            self.project_interned_formal_value(*value, fields)?
                        }
                    }
                }
                DirectSummaryInput::External { owner, expression } => {
                    let input = u32::try_from(self.inputs.len())
                        .expect("kernel summary input count exceeds u32");
                    self.inputs.push(DirectSummaryInput::External {
                        owner: *owner,
                        expression: *expression,
                    });
                    PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Input(input)),
                        mode: DirectSummaryMode::Fixed {
                            owner: *owner,
                            expression: *expression,
                        },
                        formal_projection_input: None,
                    }
                }
            };
            values.push(value.value);
            modes.push(value.mode);
        }
        let mode = match summary.result_mode {
            DirectSummaryMode::Fixed { owner, expression } => {
                DirectSummaryMode::Fixed { owner, expression }
            }
            DirectSummaryMode::Input(input) => *modes.get(input as usize)?,
        };
        Some(PlannedSummaryValue {
            value: self.push_node(KernelSummaryNode::Invoke {
                program: Arc::clone(&summary.program),
                inputs: values.into_boxed_slice(),
            }),
            mode,
            formal_projection_input: None,
        })
    }

    fn compile_expression(
        &mut self,
        owner_id: KernelOwnerId,
        expression: usize,
        actuals: &[PlannedSummaryActual],
        active: &mut BTreeSet<(KernelOwnerId, usize)>,
    ) -> Option<PlannedSummaryValue> {
        if !active.insert((owner_id, expression)) {
            return None;
        }
        let result = (|| {
            let owner = self.project.owners.get(owner_id.0 as usize)?;
            let node = owner.nodes.get(expression)?;
            let fixed_mode = DirectSummaryMode::Fixed {
                owner: owner_id,
                expression,
            };
            let term_value = |compiler: &mut Self, term| PlannedSummaryValue {
                value: compiler.push_node(KernelSummaryNode::Term(term)),
                mode: fixed_mode,
                formal_projection_input: None,
            };
            match &node.kind {
                KernelOwnerNodeKind::Known(ty) | KernelOwnerNodeKind::Source(ty)
                    if type_is_recursively_closed(ty) =>
                {
                    let value = self.builder.terms_mut().import_checked_type(ty, &mut |_| {
                        unreachable!("compiled direct-summary ABI type is closed")
                    });
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Absent => {
                    let value = self.builder.terms().absent();
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Text => {
                    let value = self.builder.terms().text();
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::TextTemplate => {
                    let mut dependencies = Vec::with_capacity(node.inputs.len());
                    for edge in &node.inputs {
                        if !matches!(edge.role, KernelOwnerEdgeRole::TextDynamic) {
                            return None;
                        }
                        dependencies.push(
                            self.compile_expression(
                                owner_id,
                                edge.expression.0 as usize,
                                actuals,
                                active,
                            )?
                            .value,
                        );
                    }
                    let text = self.builder.terms().text();
                    let result = self.push_node(KernelSummaryNode::Term(text));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: dependencies.into_boxed_slice(),
                            result,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Number => {
                    let value = self.builder.terms().number();
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Byte => {
                    let value = self.builder.terms_mut().bytes(crate::BytesTerm::Fixed(1));
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Bits(width) => {
                    let value = self.builder.terms_mut().bits(*width);
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Tag(tag) => {
                    let tag = self.builder.terms_mut().variant_tag(tag);
                    let value = self.builder.terms_mut().variant_set([tag]);
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::FormalRead { formal, fields } => {
                    let actual = actuals.get(*formal as usize)?;
                    match actual {
                        PlannedSummaryActual::Formal(formal) => {
                            let fields = fields
                                .iter()
                                .map(|field| self.builder.terms_mut().intern_name(field))
                                .collect::<Vec<_>>()
                                .into_boxed_slice();
                            Some(self.push_formal_projection(*formal, fields))
                        }
                        PlannedSummaryActual::Value(value) => {
                            self.project_value(*value, fields, fixed_mode)
                        }
                    }
                }
                KernelOwnerNodeKind::Record { tag } => {
                    let mut entries = Vec::with_capacity(node.inputs.len());
                    for edge in &node.inputs {
                        let KernelOwnerEdgeRole::RecordField { name, spread } = &edge.role else {
                            return None;
                        };
                        let value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        if *spread {
                            entries.push(KernelSummaryRecordEntry::Spread { value: value.value });
                        } else {
                            let name = self.builder.terms_mut().intern_name(name);
                            entries.push(KernelSummaryRecordEntry::Field {
                                name,
                                value: value.value,
                            });
                        }
                    }
                    let tag = tag
                        .as_ref()
                        .map(|tag| self.builder.terms_mut().intern_name(tag));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Record {
                            tag,
                            entries: entries.into_boxed_slice(),
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Collection { kind, .. } => match kind {
                    KernelCollectionKind::List | KernelCollectionKind::Set => {
                        let mut inputs = Vec::with_capacity(node.inputs.len());
                        for edge in &node.inputs {
                            if !matches!(edge.role, KernelOwnerEdgeRole::CollectionItem) {
                                return None;
                            }
                            inputs.push(
                                self.compile_expression(
                                    owner_id,
                                    edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?
                                .value,
                            );
                        }
                        let kind = match kind {
                            KernelCollectionKind::List => KernelCollectionOperationKind::List,
                            KernelCollectionKind::Set => KernelCollectionOperationKind::Set,
                            _ => unreachable!(),
                        };
                        Some(PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Collection {
                                kind,
                                inputs: inputs.into_boxed_slice(),
                                values: Box::new([]),
                            }),
                            mode: fixed_mode,
                            formal_projection_input: None,
                        })
                    }
                    KernelCollectionKind::Bytes => {
                        let mut dependencies = Vec::with_capacity(node.inputs.len());
                        for edge in &node.inputs {
                            if !matches!(edge.role, KernelOwnerEdgeRole::CollectionItem) {
                                return None;
                            }
                            dependencies.push(
                                self.compile_expression(
                                    owner_id,
                                    edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?
                                .value,
                            );
                        }
                        let bytes = self
                            .builder
                            .terms_mut()
                            .bytes(crate::BytesTerm::Fixed(node.inputs.len()));
                        let result = self.push_node(KernelSummaryNode::Term(bytes));
                        Some(PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Sequence {
                                inputs: dependencies.into_boxed_slice(),
                                result,
                            }),
                            mode: fixed_mode,
                            formal_projection_input: None,
                        })
                    }
                    KernelCollectionKind::Map => {
                        let mut keys = Vec::new();
                        let mut values = Vec::new();
                        for edge in &node.inputs {
                            if !matches!(edge.role, KernelOwnerEdgeRole::MapEntry) {
                                return None;
                            }
                            let entry = owner.nodes.get(edge.expression.0 as usize)?;
                            if !matches!(entry.kind, KernelOwnerNodeKind::MapEntry) {
                                return None;
                            }
                            for entry_edge in &entry.inputs {
                                let value = self.compile_expression(
                                    owner_id,
                                    entry_edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?;
                                match entry_edge.role {
                                    KernelOwnerEdgeRole::MapKey => keys.push(value.value),
                                    KernelOwnerEdgeRole::MapValue => values.push(value.value),
                                    _ => return None,
                                }
                            }
                        }
                        Some(PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Collection {
                                kind: KernelCollectionOperationKind::Map,
                                inputs: keys.into_boxed_slice(),
                                values: values.into_boxed_slice(),
                            }),
                            mode: fixed_mode,
                            formal_projection_input: None,
                        })
                    }
                },
                KernelOwnerNodeKind::MapEntry => {
                    let mut dependencies = Vec::with_capacity(node.inputs.len());
                    for edge in &node.inputs {
                        if !matches!(
                            edge.role,
                            KernelOwnerEdgeRole::MapKey | KernelOwnerEdgeRole::MapValue
                        ) {
                            return None;
                        }
                        dependencies.push(
                            self.compile_expression(
                                owner_id,
                                edge.expression.0 as usize,
                                actuals,
                                active,
                            )?
                            .value,
                        );
                    }
                    let absent = self.builder.terms().absent();
                    let result = self.push_node(KernelSummaryNode::Term(absent));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: dependencies.into_boxed_slice(),
                            result,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Block => {
                    let results = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::BlockResult))
                        .collect::<Vec<_>>();
                    let [result] = results.as_slice() else {
                        return None;
                    };
                    self.compile_expression(owner_id, result.expression.0 as usize, actuals, active)
                }
                KernelOwnerNodeKind::Draining => {
                    let inputs = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::DrainingInput))
                        .collect::<Vec<_>>();
                    let [input] = inputs.as_slice() else {
                        return None;
                    };
                    self.compile_expression(owner_id, input.expression.0 as usize, actuals, active)
                }
                KernelOwnerNodeKind::LexicalRead { fields }
                | KernelOwnerNodeKind::ValueRead { fields, .. }
                | KernelOwnerNodeKind::DerivedRead { fields } => {
                    let providers = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
                        .collect::<Vec<_>>();
                    let [provider] = providers.as_slice() else {
                        return None;
                    };
                    let reference = provider.expression.0 as usize;
                    let provider = if reference < owner.nodes.len() {
                        self.compile_expression(owner_id, reference, actuals, active)?
                    } else {
                        let external = owner
                            .external_expressions
                            .get(reference - owner.nodes.len())?;
                        let target_owner = self.project.owners.get(external.owner.0 as usize)?;
                        let expression = match external.target {
                            KernelExternalTarget::Expression(expression) => expression.0 as usize,
                            KernelExternalTarget::Result => target_owner.result.0 as usize,
                        };
                        let input = u32::try_from(self.inputs.len())
                            .expect("kernel summary input count exceeds u32");
                        self.inputs.push(DirectSummaryInput::External {
                            owner: external.owner,
                            expression,
                        });
                        PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Input(input)),
                            mode: DirectSummaryMode::Fixed {
                                owner: external.owner,
                                expression,
                            },
                            formal_projection_input: None,
                        }
                    };
                    self.project_value(provider, fields, fixed_mode)
                }
                KernelOwnerNodeKind::UserCall {
                    target,
                    inherited_formal,
                } => {
                    let target_owner = self.project.owners.get(target.0 as usize)?;
                    let mut target_actuals = vec![None; target_owner.formal_count as usize];
                    for edge in &node.inputs {
                        let KernelOwnerEdgeRole::CallArgument { ordinal } = edge.role else {
                            return None;
                        };
                        let value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        let slot = target_actuals.get_mut(ordinal as usize)?;
                        if slot.replace(PlannedSummaryActual::Value(value)).is_some() {
                            return None;
                        }
                    }
                    if let Some(inherited) = inherited_formal {
                        let actual = actuals.get(inherited.caller_ordinal as usize)?.clone();
                        let slot = target_actuals.get_mut(inherited.target_ordinal as usize)?;
                        if slot.replace(actual).is_some() {
                            return None;
                        }
                    }
                    let target_actuals = target_actuals.into_iter().collect::<Option<Vec<_>>>()?;
                    let shared = self
                        .summaries
                        .get(target.0 as usize)
                        .and_then(Clone::clone)
                        .filter(|summary| summary.program.nodes.len() >= SHARED_SUMMARY_MIN_NODES);
                    if let Some(shared) = shared
                        && let Some(result) =
                            self.compile_shared_invoke(shared.as_ref(), &target_actuals)
                    {
                        return Some(result);
                    }
                    self.compile_expression(
                        *target,
                        target_owner.result.0 as usize,
                        &target_actuals,
                        active,
                    )
                }
                KernelOwnerNodeKind::RenderConstructor { kind } => {
                    let mut entries = Vec::with_capacity(node.inputs.len() + 1);
                    let mut direction = None;
                    for edge in &node.inputs {
                        let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                            return None;
                        };
                        let mut value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        if let Some(expected) =
                            render_argument_requirement(self.builder, name.as_ref())
                        {
                            value.value = self.push_node(KernelSummaryNode::Constrain {
                                value: value.value,
                                expected,
                            });
                        }
                        if name.as_ref() == "direction" {
                            direction = Some(value.value);
                        }
                        let name = self.builder.terms_mut().intern_name(name);
                        entries.push(KernelSummaryRecordEntry::Field {
                            name,
                            value: value.value,
                        });
                    }
                    let kind = match kind {
                        KernelRenderConstructorKind::Fixed(tag) => {
                            let tag = self.builder.terms_mut().variant_tag(tag);
                            let term = self.builder.terms_mut().variant_set([tag]);
                            self.push_node(KernelSummaryNode::Term(term))
                        }
                        KernelRenderConstructorKind::StripeDirection => {
                            let row_tag = self.builder.terms_mut().variant_tag("Row");
                            let row = self.builder.terms_mut().variant_set([row_tag]);
                            let stack_tag = self.builder.terms_mut().variant_tag("Stack");
                            let stack = self.builder.terms_mut().variant_set([stack_tag]);
                            let fallback = self.builder.terms_mut().union([row, stack]);
                            let fallback_value = self.push_node(KernelSummaryNode::Term(fallback));
                            if let Some(direction) = direction {
                                let row_value = self.push_node(KernelSummaryNode::Term(row));
                                let stack_value = self.push_node(KernelSummaryNode::Term(stack));
                                self.push_node(KernelSummaryNode::Select {
                                    selector: direction,
                                    arms: vec![
                                        KernelSummarySelectArm {
                                            pattern: KernelPattern::Tag {
                                                name: "Row".into(),
                                                fields: Box::new([]),
                                            },
                                            output: row_value,
                                        },
                                        KernelSummarySelectArm {
                                            pattern: KernelPattern::Tag {
                                                name: "Column".into(),
                                                fields: Box::new([]),
                                            },
                                            output: stack_value,
                                        },
                                        KernelSummarySelectArm {
                                            pattern: KernelPattern::Wildcard,
                                            output: fallback_value,
                                        },
                                    ]
                                    .into_boxed_slice(),
                                })
                            } else {
                                fallback_value
                            }
                        }
                    };
                    let kind_name = self.builder.terms_mut().intern_name("kind");
                    entries.push(KernelSummaryRecordEntry::Field {
                        name: kind_name,
                        value: kind,
                    });
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Record {
                            tag: None,
                            entries: entries.into_boxed_slice(),
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::PureBuiltin { kind }
                    if direct_summary_fixed_builtin_supported(*kind) =>
                {
                    let mut dependencies = Vec::with_capacity(node.inputs.len());
                    let mut names = BTreeSet::new();
                    for edge in &node.inputs {
                        let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                            return None;
                        };
                        if !names.insert(name.as_ref()) {
                            return None;
                        }
                        let mut value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        if let Some(expected) =
                            pure_builtin_argument_requirement(self.builder, *kind, name.as_ref())
                        {
                            value.value = self.push_node(KernelSummaryNode::Constrain {
                                value: value.value,
                                expected,
                            });
                        }
                        dependencies.push(value.value);
                    }
                    let result = match kind {
                        KernelPureBuiltinKind::TextTransform
                        | KernelPureBuiltinKind::TextSlice
                        | KernelPureBuiltinKind::TextConcat
                        | KernelPureBuiltinKind::NumberToText
                        | KernelPureBuiltinKind::TextJoin
                        | KernelPureBuiltinKind::FieldColor => self.builder.terms().text(),
                        KernelPureBuiltinKind::NumberMath
                        | KernelPureBuiltinKind::NumberRound
                        | KernelPureBuiltinKind::NumberProjection
                        | KernelPureBuiltinKind::TextLength => self.builder.terms().number(),
                        KernelPureBuiltinKind::TextPredicate => boolean_type(self.builder),
                        KernelPureBuiltinKind::TextToNumber => parsed_number_type(self.builder),
                        _ => return None,
                    };
                    let result = self.push_node(KernelSummaryNode::Term(result));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: dependencies.into_boxed_slice(),
                            result,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::When => {
                    let mut selector = None;
                    let mut arms = Vec::new();
                    for edge in &node.inputs {
                        match edge.role {
                            KernelOwnerEdgeRole::WhenInput => {
                                if selector.is_some() {
                                    return None;
                                }
                                selector = Some(self.compile_expression(
                                    owner_id,
                                    edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?);
                            }
                            KernelOwnerEdgeRole::WhenArm => {
                                let arm = owner.nodes.get(edge.expression.0 as usize)?;
                                let KernelOwnerNodeKind::MatchArm { pattern } = &arm.kind else {
                                    return None;
                                };
                                let output = self.compile_expression(
                                    owner_id,
                                    edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?;
                                arms.push(KernelSummarySelectArm {
                                    pattern: pattern.clone(),
                                    output: output.value,
                                });
                            }
                            _ => return None,
                        }
                    }
                    let selector = selector?;
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Select {
                            selector: selector.value,
                            arms: arms.into_boxed_slice(),
                        }),
                        mode: selector.mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::MatchArm { .. } => {
                    let outputs = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
                        .collect::<Vec<_>>();
                    if let [output] = outputs.as_slice() {
                        if node.inputs.len() != 1 {
                            return None;
                        }
                        self.compile_expression(
                            owner_id,
                            output.expression.0 as usize,
                            actuals,
                            active,
                        )
                    } else if outputs.is_empty()
                        && node.inputs.iter().all(|edge| {
                            matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. })
                        })
                        && !node.inputs.is_empty()
                    {
                        let mut entries = Vec::with_capacity(node.inputs.len());
                        for edge in &node.inputs {
                            let KernelOwnerEdgeRole::RecordField { name, spread } = &edge.role
                            else {
                                return None;
                            };
                            let value = self.compile_expression(
                                owner_id,
                                edge.expression.0 as usize,
                                actuals,
                                active,
                            )?;
                            if *spread {
                                entries
                                    .push(KernelSummaryRecordEntry::Spread { value: value.value });
                            } else {
                                let name = self.builder.terms_mut().intern_name(name);
                                entries.push(KernelSummaryRecordEntry::Field {
                                    name,
                                    value: value.value,
                                });
                            }
                        }
                        Some(PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Record {
                                tag: None,
                                entries: entries.into_boxed_slice(),
                            }),
                            mode: fixed_mode,
                            formal_projection_input: None,
                        })
                    } else if outputs.is_empty() && node.inputs.is_empty() {
                        let absent = self.builder.terms().absent();
                        Some(term_value(self, absent))
                    } else {
                        None
                    }
                }
                KernelOwnerNodeKind::Then => {
                    let has_output = node
                        .inputs
                        .iter()
                        .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ThenOutput));
                    let mut dependencies = Vec::with_capacity(node.inputs.len());
                    let mut selected = None;
                    for edge in &node.inputs {
                        if !matches!(
                            edge.role,
                            KernelOwnerEdgeRole::ThenInput | KernelOwnerEdgeRole::ThenOutput
                        ) {
                            return None;
                        }
                        let value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        if matches!(edge.role, KernelOwnerEdgeRole::ThenOutput)
                            || (!has_output && matches!(edge.role, KernelOwnerEdgeRole::ThenInput))
                        {
                            if selected.replace(value.value).is_some() {
                                return None;
                            }
                        }
                        dependencies.push(value.value);
                    }
                    let selected = selected?;
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: dependencies.into_boxed_slice(),
                            result: selected,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Infix { operation } => {
                    let mut left = None;
                    let mut right = None;
                    for edge in &node.inputs {
                        let slot = match edge.role {
                            KernelOwnerEdgeRole::InfixLeft => &mut left,
                            KernelOwnerEdgeRole::InfixRight => &mut right,
                            _ => return None,
                        };
                        if slot
                            .replace(self.compile_expression(
                                owner_id,
                                edge.expression.0 as usize,
                                actuals,
                                active,
                            )?)
                            .is_some()
                        {
                            return None;
                        }
                    }
                    let (Some(mut left), Some(mut right)) = (left, right) else {
                        return None;
                    };
                    if infix_requires_number_operands(operation) {
                        let number = self.builder.terms().number();
                        left.value = self.push_node(KernelSummaryNode::Constrain {
                            value: left.value,
                            expected: number,
                        });
                        right.value = self.push_node(KernelSummaryNode::Constrain {
                            value: right.value,
                            expected: number,
                        });
                    }
                    let result = if infix_returns_bool(operation) {
                        boolean_type(self.builder)
                    } else {
                        self.builder.terms().number()
                    };
                    let result = self.push_node(KernelSummaryNode::Term(result));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: vec![left.value, right.value].into_boxed_slice(),
                            result,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Arrow => {
                    let outputs = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ArrowOutput))
                        .collect::<Vec<_>>();
                    let [output] = outputs.as_slice() else {
                        return None;
                    };
                    if node.inputs.len() != 1 {
                        return None;
                    }
                    self.compile_expression(owner_id, output.expression.0 as usize, actuals, active)
                }
                KernelOwnerNodeKind::Delimiter | KernelOwnerNodeKind::Unknown
                    if node.inputs.is_empty() =>
                {
                    let unknown = self.builder.terms().unknown();
                    Some(term_value(self, unknown))
                }
                _ => None,
            }
        })();
        active.remove(&(owner_id, expression));
        result
    }
}

fn compile_direct_result_summaries(
    builder: &mut ComponentProgramBuilder,
    project: &KernelProjectProgramInput,
) -> Vec<Option<Arc<CompiledDirectSummary>>> {
    let targets = project
        .owners
        .iter()
        .flat_map(|owner| owner.nodes.iter())
        .filter_map(|node| match node.kind {
            KernelOwnerNodeKind::UserCall { target, .. } => Some(target),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let supported = targets
        .into_iter()
        .filter(|target| {
            let Some(owner) = project.owners.get(target.0 as usize) else {
                return false;
            };
            direct_result_summary_supported(
                project,
                *target,
                owner.result.0 as usize,
                &mut BTreeSet::new(),
            )
        })
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(supported.len());
    let mut states = vec![0_u8; project.owners.len()];
    for target in supported.iter().copied() {
        append_direct_summary_order(project, target, &supported, &mut states, &mut order);
    }
    let mut summaries = vec![None; project.owners.len()];
    for target in order {
        let Some(owner) = project.owners.get(target.0 as usize) else {
            continue;
        };
        let result = owner.result.0 as usize;
        let actuals = (0..owner.formal_count)
            .map(PlannedSummaryActual::Formal)
            .collect::<Vec<_>>();
        let mut compiler = DirectSummaryPlanCompiler {
            builder,
            project,
            summaries: &summaries,
            nodes: Vec::new(),
            inputs: Vec::new(),
            formal_projection_inputs: HashMap::new(),
        };
        let Some(result) =
            compiler.compile_expression(target, result, &actuals, &mut BTreeSet::new())
        else {
            continue;
        };
        summaries[target.0 as usize] = Some(Arc::new(CompiledDirectSummary {
            program: Arc::new(KernelSummaryProgram {
                definition: target.0,
                nodes: compiler.nodes.into_boxed_slice(),
                result: result.value,
            }),
            inputs: compiler.inputs.into_boxed_slice(),
            result_mode: result.mode,
            formal_count: owner.formal_count as usize,
        }));
    }
    summaries
}

fn append_direct_summary_order(
    project: &KernelProjectProgramInput,
    target: KernelOwnerId,
    supported: &BTreeSet<KernelOwnerId>,
    states: &mut [u8],
    order: &mut Vec<KernelOwnerId>,
) {
    let index = target.0 as usize;
    match states.get(index).copied() {
        Some(2) => return,
        Some(1) | None => return,
        Some(_) => {}
    }
    states[index] = 1;
    if let Some(owner) = project.owners.get(index) {
        let mut dependencies = BTreeSet::new();
        collect_direct_summary_call_targets(
            owner,
            owner.result.0 as usize,
            &mut BTreeSet::new(),
            &mut dependencies,
        );
        for dependency in dependencies {
            if supported.contains(&dependency) {
                append_direct_summary_order(project, dependency, supported, states, order);
            }
        }
    }
    states[index] = 2;
    order.push(target);
}

fn collect_direct_summary_call_targets(
    owner: &KernelOwnerProgramInput,
    expression: usize,
    active: &mut BTreeSet<usize>,
    targets: &mut BTreeSet<KernelOwnerId>,
) {
    if !active.insert(expression) {
        return;
    }
    let Some(node) = owner.nodes.get(expression) else {
        active.remove(&expression);
        return;
    };
    if let KernelOwnerNodeKind::UserCall { target, .. } = node.kind {
        targets.insert(target);
    }
    for edge in node.inputs.iter() {
        let dependency = edge.expression.0 as usize;
        if dependency < owner.nodes.len() {
            collect_direct_summary_call_targets(owner, dependency, active, targets);
        }
    }
    active.remove(&expression);
}

fn emit_compiled_direct_summary(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    output: TypeVariableId,
    output_mode: ModeVariableId,
    actuals: &[CallActual],
    summary: &CompiledDirectSummary,
) -> Result<(), KernelOwnerBuildError> {
    if actuals.len() != summary.formal_count {
        return Err(KernelOwnerBuildError::new(format!(
            "compiled direct summary receives {} actuals for {} formals",
            actuals.len(),
            summary.formal_count
        )));
    }
    let mut call_inputs = Vec::with_capacity(summary.inputs.len());
    let mut input_modes = Vec::with_capacity(summary.inputs.len());
    for input in summary.inputs.iter() {
        match input {
            DirectSummaryInput::FormalProjection { formal, fields } => {
                let actual = actuals.get(*formal as usize).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "compiled direct summary reads missing formal {formal}"
                    ))
                })?;
                let mut steps = Vec::with_capacity(fields.len().max(1));
                if fields.is_empty() {
                    steps.push(KernelSummaryProjectionStep {
                        field: None,
                        consumer: builder.new_variable(),
                    });
                } else {
                    for field in fields.iter().copied() {
                        steps.push(KernelSummaryProjectionStep {
                            field: Some(field),
                            consumer: builder.new_variable(),
                        });
                    }
                }
                call_inputs.push(KernelSummaryCallInput::Projection {
                    provider: actual.variable,
                    steps: steps.into_boxed_slice(),
                });
                input_modes.push(projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &actual.mode_source,
                    &fields
                        .iter()
                        .map(|field| builder.terms().name(*field).into())
                        .collect::<Vec<Box<str>>>(),
                    &mut BTreeSet::new(),
                )?);
            }
            DirectSummaryInput::External { owner, expression } => {
                let instance = context.principals.get(owner.0 as usize).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "compiled direct summary imports missing owner {}",
                        owner.0
                    ))
                })?;
                let variable = instance.expressions.get(*expression).copied().ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "compiled direct summary imports missing owner {} expression {expression}",
                        owner.0
                    ))
                })?;
                call_inputs.push(KernelSummaryCallInput::Term(
                    builder.variable_term(variable),
                ));
                input_modes.push(instance.expression_modes[*expression]);
            }
        }
    }
    builder.add_summary_call(output, Arc::clone(&summary.program), call_inputs);
    let mode = match summary.result_mode {
        DirectSummaryMode::Fixed { owner, expression } => context
            .principals
            .get(owner.0 as usize)
            .and_then(|instance| instance.expression_modes.get(expression))
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "compiled direct summary mode references missing owner {} expression {expression}",
                    owner.0
                ))
            })?,
        DirectSummaryMode::Input(input) => input_modes
            .get(input as usize)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "compiled direct summary mode references missing input {input}"
                ))
            })?,
    };
    mode_builder.set(output_mode, ModeEquation::Copy(mode));
    Ok(())
}

fn compile_node(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    invocations: &mut HashMap<InvocationKey, OwnerInstance>,
    specializations: &mut HashMap<SpecializationKey, OwnerSpecialization>,
    residual_modules: &mut HashMap<SpecializationKey, Arc<ResidualTypeModule>>,
    compile_work: &mut KernelCompileWork,
    context: &OwnerCompileContext<'_>,
    stack: &mut Vec<KernelOwnerId>,
    index: usize,
    node: &KernelOwnerNode,
    output: TypeVariableId,
    output_mode: ModeVariableId,
    compile_mode: bool,
) -> Result<(), KernelOwnerBuildError> {
    match &node.kind {
        KernelOwnerNodeKind::Known(ty) | KernelOwnerNodeKind::Source(ty) => {
            if !type_is_recursively_closed(ty) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} imports a non-closed ABI type {ty:?}"
                )));
            }
            let provider = builder
                .terms_mut()
                .import_checked_type(ty, &mut |_| unreachable!("closed ABI type has no variable"));
            builder.add_publish(output, [provider], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Absent => {
            let absent = builder.terms().absent();
            builder.add_publish(output, [absent], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Text | KernelOwnerNodeKind::TextTemplate => {
            let text = builder.terms().text();
            builder.add_publish(output, [text], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Number => {
            let number = builder.terms().number();
            builder.add_publish(output, [number], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Byte => {
            let byte = builder.terms_mut().bytes(crate::BytesTerm::Fixed(1));
            builder.add_publish(output, [byte], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Bits(width) => {
            let bits = builder.terms_mut().bits(*width);
            builder.add_publish(output, [bits], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Tag(tag) => {
            let tag = builder.terms_mut().variant_tag(tag);
            let variants = builder.terms_mut().variant_set([tag]);
            builder.add_publish(output, [variants], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Record { tag } => {
            compile_record(builder, context, index, node, output, tag.as_deref())?;
        }
        KernelOwnerNodeKind::Block => {
            let mut results = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::BlockResult));
            let result = results.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} BLOCK has no result edge"
                ))
            })?;
            if results.next().is_some() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} BLOCK has multiple result edges"
                )));
            }
            let provider = edge_variable(context, index, result)?;
            builder.add_projection_into(provider, [], output);
        }
        KernelOwnerNodeKind::Collection { kind, .. } => match kind {
            KernelCollectionKind::List | KernelCollectionKind::Set => {
                let items = selected_edge_terms(builder, context, index, node, |role| {
                    matches!(role, KernelOwnerEdgeRole::CollectionItem)
                })?;
                let kind = match kind {
                    KernelCollectionKind::List => KernelCollectionOperationKind::List,
                    KernelCollectionKind::Set => KernelCollectionOperationKind::Set,
                    _ => unreachable!(),
                };
                builder.add_collection(output, kind, items, []);
            }
            KernelCollectionKind::Bytes => {
                let size = node
                    .inputs
                    .iter()
                    .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::CollectionItem))
                    .count();
                let bytes = builder.terms_mut().bytes(crate::BytesTerm::Fixed(size));
                builder.add_publish(output, [bytes], PublishMode::Replace);
            }
            KernelCollectionKind::Map => {
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for edge in &node.inputs {
                    if !matches!(edge.role, KernelOwnerEdgeRole::MapEntry) {
                        continue;
                    }
                    let entry_index = checked_expression_index(
                        edge.expression,
                        context.input.nodes.len(),
                        "map entry",
                    )?;
                    let entry = &context.input.nodes[entry_index];
                    if !matches!(entry.kind, KernelOwnerNodeKind::MapEntry) {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} map edge targets non-entry node {entry_index}"
                        )));
                    }
                    keys.extend(selected_edge_terms(
                        builder,
                        context,
                        entry_index,
                        entry,
                        |role| matches!(role, KernelOwnerEdgeRole::MapKey),
                    )?);
                    values.extend(selected_edge_terms(
                        builder,
                        context,
                        entry_index,
                        entry,
                        |role| matches!(role, KernelOwnerEdgeRole::MapValue),
                    )?);
                }
                builder.add_collection(output, KernelCollectionOperationKind::Map, keys, values);
            }
        },
        KernelOwnerNodeKind::MapEntry => {
            // The enclosing MAP consumes its exact key/value edges. The entry
            // itself remains an internal delimiter rather than a value type.
            let absent = builder.terms().absent();
            builder.add_publish(output, [absent], PublishMode::Replace);
        }
        KernelOwnerNodeKind::FormalRead { formal, fields } => {
            if !node.inputs.is_empty() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} formal read has explicit inputs"
                )));
            }
            let provider = context
                .formals
                .get(*formal as usize)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} reads missing formal {formal}"
                    ))
                })?;
            let path = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            builder.add_projection_into(provider, path.iter().copied(), output);
            let requirement = context
                .formal_requirements
                .get(*formal as usize)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} reads missing formal requirement {formal}"
                    ))
                })?;
            let requirement = requirement_projection(builder, requirement, &path);
            let requirement = builder.variable_term(requirement);
            let output_term = builder.variable_term(output);
            builder.add_unify(output_term, requirement);
        }
        KernelOwnerNodeKind::LexicalRead { fields } => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} lexical read has no provider"
                ))
            })?;
            if providers.next().is_some() || node.inputs.len() != 1 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} lexical read must have exactly one provider"
                )));
            }
            let provider = edge_variable(context, index, provider)?;
            let path = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            if path.is_empty() {
                let provider = builder.variable_term(provider);
                let output_term = builder.variable_term(output);
                builder.add_unify(output_term, provider);
            } else {
                builder.add_projection_into(provider, path, output);
            }
        }
        KernelOwnerNodeKind::ValueRead { fields, .. }
        | KernelOwnerNodeKind::DerivedRead { fields } => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} value read has no provider"
                ))
            })?;
            if providers.next().is_some() || node.inputs.len() != 1 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} value read must have exactly one provider"
                )));
            }
            let provider = edge_variable(context, index, provider)?;
            let path = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            builder.add_projection_into(provider, path, output);
        }
        KernelOwnerNodeKind::CollectionItemRead => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} collection item read has no provider"
                ))
            })?;
            if providers.next().is_some() || node.inputs.len() != 1 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} collection item read must have exactly one provider"
                )));
            }
            let provider = edge_variable(context, index, provider)?;
            builder.add_collection_item_projection(provider, output);
        }
        KernelOwnerNodeKind::UserCall {
            target,
            inherited_formal,
        } => {
            compile_work.compiled_call_sites = compile_work.compiled_call_sites.saturating_add(1);
            let project = context.project.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "standalone owner node {index} cannot call owner {}",
                    target.0
                ))
            })?;
            let target_owner = project.owners.get(target.0 as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} calls missing owner {}",
                    target.0
                ))
            })?;
            let mut actuals = vec![None; target_owner.formal_count as usize];
            for edge in &node.inputs {
                let KernelOwnerEdgeRole::CallArgument { ordinal } = edge.role else {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} user call has non-argument edge {:?}",
                        edge.role
                    )));
                };
                let slot = actuals.get_mut(ordinal as usize).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} supplies out-of-range argument {ordinal}"
                    ))
                })?;
                if slot.is_some() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} repeats argument {ordinal}"
                    )));
                }
                *slot = Some(CallActual {
                    variable: edge_variable(context, index, edge)?,
                    mode: edge_mode_variable(context, index, edge)?,
                    mode_source: mode_source_for_edge(
                        context,
                        context.owner,
                        context.expression_modes,
                        context.formal_mode_sources,
                        edge,
                    )?,
                    static_variants: edge_static_variants(context, edge),
                });
            }
            if let Some(inherited) = inherited_formal {
                let actual = context
                    .formals
                    .get(inherited.caller_ordinal as usize)
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits missing caller formal {}",
                            inherited.caller_ordinal
                        ))
                    })?;
                let actual_mode = context
                    .formal_modes
                    .get(inherited.caller_ordinal as usize)
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits missing caller mode {}",
                            inherited.caller_ordinal
                        ))
                    })?;
                let actual_mode_source = context
                    .formal_mode_sources
                    .get(inherited.caller_ordinal as usize)
                    .cloned()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits missing caller mode source {}",
                            inherited.caller_ordinal
                        ))
                    })?;
                let slot = actuals
                    .get_mut(inherited.target_ordinal as usize)
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits out-of-range target formal {}",
                            inherited.target_ordinal
                        ))
                    })?;
                if slot
                    .replace(CallActual {
                        variable: actual,
                        mode: actual_mode,
                        mode_source: actual_mode_source,
                        static_variants: context
                            .formal_static_variants
                            .get(inherited.caller_ordinal as usize)
                            .cloned()
                            .flatten(),
                    })
                    .is_some()
                {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} supplies and inherits target formal {}",
                        inherited.target_ordinal
                    )));
                }
            }
            let actuals = actuals
                .into_iter()
                .enumerate()
                .map(|(ordinal, actual)| {
                    actual.ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} omits argument {ordinal}"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let result = checked_expression_index(
                target_owner.result,
                target_owner.nodes.len(),
                "user-call result",
            )?;
            // A formal-independent definition is already represented by its
            // principal frame. Reusing that frame is exact: no actual can
            // alter the result, select a different branch, or receive a
            // requirement from the result cone. This is the first flag-day
            // step away from cloning a complete callee residual for every
            // call occurrence.
            if !context.formal_dependent_results[target.0 as usize] {
                compile_work.principal_result_reuses =
                    compile_work.principal_result_reuses.saturating_add(1);
                let provider = builder
                    .variable_term(context.principals[target.0 as usize].expressions[result]);
                builder.add_publish(output, [provider], PublishMode::Replace);
                mode_builder.set(
                    output_mode,
                    ModeEquation::Copy(
                        context.principals[target.0 as usize].expression_modes[result],
                    ),
                );
                return Ok(());
            }
            if let KernelOwnerNodeKind::FormalRead { formal, fields } =
                &target_owner.nodes[result].kind
            {
                if !target_owner.nodes[result].inputs.is_empty() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner {} direct-result formal read has explicit inputs",
                        target.0
                    )));
                }
                let actual = actuals.get(*formal as usize).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner {} direct-result summary reads missing formal {formal}",
                        target.0
                    ))
                })?;
                let path = fields
                    .iter()
                    .map(|field| builder.terms_mut().intern_name(field))
                    .collect::<Vec<_>>();
                builder.add_projection_into(actual.variable, path.iter().copied(), output);
                if !builder.is_authoritative(actual.variable) {
                    let requirement_root = builder.new_contextual_hole();
                    let requirement = requirement_projection(builder, requirement_root, &path);
                    let output_term = builder.variable_term(output);
                    let requirement = builder.variable_term(requirement);
                    builder.add_unify(output_term, requirement);
                    let actual_term = builder.variable_term(actual.variable);
                    let requirement_root = builder.variable_term(requirement_root);
                    builder.add_unify(actual_term, requirement_root);
                }
                let projected_mode = projected_mode_variable(
                    mode_builder,
                    context,
                    index,
                    &actual.mode_source,
                    fields,
                    &mut BTreeSet::new(),
                )?;
                mode_builder.set(output_mode, ModeEquation::Copy(projected_mode));
                compile_work.direct_result_summaries =
                    compile_work.direct_result_summaries.saturating_add(1);
                return Ok(());
            }
            if let Some(Some(summary)) = context.direct_summaries.get(target.0 as usize) {
                emit_compiled_direct_summary(
                    builder,
                    mode_builder,
                    context,
                    index,
                    output,
                    output_mode,
                    &actuals,
                    summary,
                )?;
                compile_work.direct_result_summaries =
                    compile_work.direct_result_summaries.saturating_add(1);
                return Ok(());
            }
            let instance = instantiate_owner(
                builder,
                mode_builder,
                invocations,
                specializations,
                residual_modules,
                compile_work,
                project,
                context.principals,
                context.formal_dependent_results,
                context.formal_dependent_expressions,
                context.direct_summaries,
                *target,
                &actuals,
                context.initial_state_surface
                    || context
                        .syntax_selected_calls
                        .and_then(|calls| calls.get(index))
                        .copied()
                        .unwrap_or(false),
                stack,
            )?;
            let provider = builder.variable_term(instance.expressions[result]);
            builder.add_publish(output, [provider], PublishMode::Replace);
            mode_builder.set(
                output_mode,
                ModeEquation::Copy(instance.expression_modes[result]),
            );
        }
        KernelOwnerNodeKind::RenderConstructor { kind } => {
            let mut fields = Vec::with_capacity(node.inputs.len() + 1);
            let mut direction = None;
            for edge in &node.inputs {
                let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} render constructor has invalid edge {:?}",
                        edge.role
                    )));
                };
                let value = edge_variable(context, index, edge)?;
                if name.as_ref() == "direction" {
                    direction = Some(value);
                }
                let value_term = builder.variable_term(value);
                constrain_render_argument(builder, name, value_term);
                let name = builder.terms_mut().intern_name(name);
                fields.push((name, value_term));
            }
            let kind = match kind {
                KernelRenderConstructorKind::Fixed(tag) => {
                    let tag = builder.terms_mut().variant_tag(tag);
                    builder.terms_mut().variant_set([tag])
                }
                KernelRenderConstructorKind::StripeDirection => {
                    let kind = builder.new_authoritative_provider();
                    let row_name = builder.terms_mut().variant_tag("Row");
                    let row = builder.terms_mut().variant_set([row_name]);
                    let stack_name = builder.terms_mut().variant_tag("Stack");
                    let stack = builder.terms_mut().variant_set([stack_name]);
                    let fallback = builder.terms_mut().union([row, stack]);
                    match direction {
                        Some(direction) => {
                            builder.add_select(
                                kind,
                                direction,
                                [
                                    KernelSelectArm {
                                        pattern: KernelPattern::Tag {
                                            name: "Row".into(),
                                            fields: Box::new([]),
                                        },
                                        output: row,
                                    },
                                    KernelSelectArm {
                                        pattern: KernelPattern::Tag {
                                            name: "Column".into(),
                                            fields: Box::new([]),
                                        },
                                        output: stack,
                                    },
                                    KernelSelectArm {
                                        pattern: KernelPattern::Wildcard,
                                        output: fallback,
                                    },
                                ],
                            );
                        }
                        None => {
                            builder.add_publish(kind, [fallback], PublishMode::Replace);
                        }
                    }
                    builder.variable_term(kind)
                }
            };
            let kind_name = builder.terms_mut().intern_name("kind");
            fields.push((kind_name, kind));
            let result = builder.terms_mut().object(fields, false);
            builder.add_publish(output, [result], PublishMode::Replace);
        }
        KernelOwnerNodeKind::PureBuiltin { kind } => {
            let mut arguments = BTreeMap::new();
            for edge in &node.inputs {
                let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} pure builtin has invalid edge {:?}",
                        edge.role
                    )));
                };
                let value = edge_variable(context, index, edge)?;
                if arguments.insert(name.as_ref(), value).is_some() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} pure builtin repeats argument `{name}`"
                    )));
                }
                let value = builder.variable_term(value);
                constrain_pure_builtin_argument(builder, *kind, name, value);
            }
            let argument = |name: &str| {
                arguments.get(name).copied().ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner {} node {index} pure builtin omits argument `{name}`",
                        context.owner.0
                    ))
                })
            };
            let list_argument = || {
                arguments
                    .get("$pipe")
                    .or_else(|| arguments.get("list"))
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner {} node {index} list builtin omits its `$pipe`/`list` input",
                            context.owner.0
                        ))
                    })
            };
            let result = match kind {
                KernelPureBuiltinKind::TextTransform
                | KernelPureBuiltinKind::TextSlice
                | KernelPureBuiltinKind::TextConcat
                | KernelPureBuiltinKind::NumberToText
                | KernelPureBuiltinKind::TextJoin
                | KernelPureBuiltinKind::FieldColor => builder.terms().text(),
                KernelPureBuiltinKind::NumberMath
                | KernelPureBuiltinKind::NumberRound
                | KernelPureBuiltinKind::NumberProjection
                | KernelPureBuiltinKind::TextLength
                | KernelPureBuiltinKind::ListLength => builder.terms().number(),
                KernelPureBuiltinKind::TextPredicate | KernelPureBuiltinKind::ListPredicate => {
                    boolean_type(builder)
                }
                KernelPureBuiltinKind::TextToNumber => parsed_number_type(builder),
                KernelPureBuiltinKind::ListFilter | KernelPureBuiltinKind::ListSort => {
                    builder.variable_term(list_argument()?)
                }
                KernelPureBuiltinKind::ListMap => {
                    let item = builder.variable_term(argument("new")?);
                    builder.terms_mut().list(item)
                }
                KernelPureBuiltinKind::ListFind => {
                    let item = builder.variable_term(argument("item")?);
                    let value = builder.terms_mut().intern_name("value");
                    let fields = builder.terms_mut().object([(value, item)], false);
                    let found = builder.terms_mut().tagged_variant("Found", fields);
                    let not_found = builder.terms_mut().variant_tag("NotFound");
                    builder
                        .terms_mut()
                        .variant_set_preserving_order([found, not_found])
                }
                KernelPureBuiltinKind::ListLatest => {
                    let item = builder.new_authoritative_provider();
                    builder.add_collection_item_projection(list_argument()?, item);
                    builder.variable_term(item)
                }
                KernelPureBuiltinKind::ListAppend => {
                    let item = builder.variable_term(argument("item")?);
                    builder.terms_mut().list(item)
                }
                KernelPureBuiltinKind::ListChunk => {
                    let item = builder.new_authoritative_provider();
                    builder.add_collection_item_projection(list_argument()?, item);
                    let item = builder.variable_term(item);
                    let label_name = builder.terms_mut().intern_name("label");
                    let items_name = builder.terms_mut().intern_name("items");
                    let label = builder.terms().text();
                    let items = builder.terms_mut().list(item);
                    let chunk = builder
                        .terms_mut()
                        .object([(label_name, label), (items_name, items)], false);
                    builder.terms_mut().list(chunk)
                }
            };
            builder.add_publish(output, [result], PublishMode::Replace);
        }
        KernelOwnerNodeKind::HostEffect { operation } => {
            compile_host_effect(builder, context, index, node, output, operation)?;
        }
        KernelOwnerNodeKind::Latest => {
            let branches = selected_edge_terms(builder, context, index, node, |role| {
                matches!(role, KernelOwnerEdgeRole::LatestBranch)
            })?;
            if branches.is_empty() {
                // An empty LATEST carries no value-shape evidence. `Absent`
                // remains a runtime value/type used by explicit absent
                // providers, while the empty selector is a contextual hole.
                // HOLD widening consequently ignores it without claiming that
                // the LATEST expression itself has the Absent value type.
                let unknown = builder.terms().unknown();
                builder.add_publish(output, [unknown], PublishMode::Replace);
            } else {
                builder.add_publish(output, branches, PublishMode::Union);
            }
        }
        KernelOwnerNodeKind::When => {
            let mut selector = None;
            let mut arms = Vec::new();
            let possible_arms = possible_when_arm_references(context, index, node)?;
            for edge in &node.inputs {
                match edge.role {
                    KernelOwnerEdgeRole::WhenInput => {
                        if selector
                            .replace(edge_variable(context, index, edge)?)
                            .is_some()
                        {
                            return Err(KernelOwnerBuildError::new(format!(
                                "kernel owner node {index} WHEN repeats its selector"
                            )));
                        }
                    }
                    KernelOwnerEdgeRole::WhenArm => {
                        if !possible_arms.contains(&(edge.expression.0 as usize)) {
                            continue;
                        }
                        let arm = referenced_node(context, index, edge)?;
                        let KernelOwnerNodeKind::MatchArm { pattern } = &arm.kind else {
                            return Err(KernelOwnerBuildError::new(format!(
                                "kernel owner node {index} WHEN targets a non-arm expression"
                            )));
                        };
                        let arm = edge_variable(context, index, edge)?;
                        arms.push(KernelSelectArm {
                            pattern: pattern.clone(),
                            output: builder.variable_term(arm),
                        });
                    }
                    _ => {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} WHEN has invalid edge {:?}",
                            edge.role
                        )));
                    }
                }
            }
            let selector = selector.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} WHEN has no selector"
                ))
            })?;
            builder.add_select(output, selector, arms);
        }
        KernelOwnerNodeKind::Then => {
            let has_output = node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ThenOutput));
            publish_selected_edges(
                builder,
                context,
                index,
                node,
                output,
                |role| {
                    matches!(role, KernelOwnerEdgeRole::ThenOutput)
                        || (!has_output && matches!(role, KernelOwnerEdgeRole::ThenInput))
                },
                PublishMode::Union,
            )?;
        }
        KernelOwnerNodeKind::Infix { operation } => {
            let mut left = None;
            let mut right = None;
            for edge in &node.inputs {
                let slot = match edge.role {
                    KernelOwnerEdgeRole::InfixLeft => &mut left,
                    KernelOwnerEdgeRole::InfixRight => &mut right,
                    _ => {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} infix `{operation}` has invalid edge {:?}",
                            edge.role
                        )));
                    }
                };
                if slot.replace(edge_variable(context, index, edge)?).is_some() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} infix `{operation}` repeats an operand"
                    )));
                }
            }
            let (Some(left), Some(right)) = (left, right) else {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} infix `{operation}` requires two operands"
                )));
            };
            if infix_requires_number_operands(operation) {
                let number = builder.terms().number();
                let left = builder.variable_term(left);
                builder.add_unify(left, number);
                let right = builder.variable_term(right);
                builder.add_unify(right, number);
            }
            let result = if infix_returns_bool(operation) {
                let false_tag = builder.terms_mut().variant_tag("False");
                let true_tag = builder.terms_mut().variant_tag("True");
                builder.terms_mut().variant_set([false_tag, true_tag])
            } else {
                builder.terms().number()
            };
            builder.add_publish(output, [result], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Draining => {
            publish_selected_edges(
                builder,
                context,
                index,
                node,
                output,
                |role| matches!(role, KernelOwnerEdgeRole::DrainingInput),
                PublishMode::Replace,
            )?;
        }
        KernelOwnerNodeKind::Hold => {
            if context.initial_state_surface {
                publish_selected_edges(
                    builder,
                    context,
                    index,
                    node,
                    output,
                    |role| matches!(role, KernelOwnerEdgeRole::HoldInitial),
                    PublishMode::Replace,
                )?;
            } else {
                // A normal definition or direct call denotes the complete
                // state domain. Only a proven syntax-selected construction
                // occurrence uses the initializer-only surface above.
                publish_selected_edges(
                    builder,
                    context,
                    index,
                    node,
                    output,
                    |role| {
                        matches!(
                            role,
                            KernelOwnerEdgeRole::HoldInitial | KernelOwnerEdgeRole::HoldUpdate
                        )
                    },
                    PublishMode::StructuralWiden,
                )?;
            }
        }
        KernelOwnerNodeKind::MatchArm { .. } => {
            if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
            {
                publish_selected_edges(
                    builder,
                    context,
                    index,
                    node,
                    output,
                    |role| matches!(role, KernelOwnerEdgeRole::MatchOutput),
                    PublishMode::Replace,
                )?;
            } else if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. }))
            {
                compile_record(builder, context, index, node, output, None)?;
            } else {
                let absent = builder.terms().absent();
                builder.add_publish(output, [absent], PublishMode::Replace);
            }
        }
        KernelOwnerNodeKind::Arrow => {
            publish_selected_edges(
                builder,
                context,
                index,
                node,
                output,
                |role| matches!(role, KernelOwnerEdgeRole::ArrowOutput),
                PublishMode::Replace,
            )?;
        }
        KernelOwnerNodeKind::Delimiter | KernelOwnerNodeKind::Unknown => {
            let unknown = builder.terms().unknown();
            builder.add_publish(output, [unknown], PublishMode::Replace);
        }
    }
    if compile_mode && !matches!(node.kind, KernelOwnerNodeKind::UserCall { .. }) {
        let equation = node_mode_equation(mode_builder, context, index, node)?;
        mode_builder.set(output_mode, equation);
    }
    Ok(())
}

fn compile_record(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
    output: TypeVariableId,
    tag: Option<&str>,
) -> Result<(), KernelOwnerBuildError> {
    let mut entries = Vec::new();
    for edge in &node.inputs {
        let KernelOwnerEdgeRole::RecordField { name, spread } = &edge.role else {
            continue;
        };
        let value = edge_variable(context, node_index, edge)?;
        let value = builder.variable_term(value);
        if *spread {
            entries.push(KernelRecordEntry::Spread { value });
        } else {
            let name = builder.terms_mut().intern_name(name);
            entries.push(KernelRecordEntry::Field { name, value });
        }
    }
    let tag = tag.map(|tag| builder.terms_mut().intern_name(tag));
    builder.add_record(output, tag, entries);
    Ok(())
}

fn publish_selected_edges(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
    output: TypeVariableId,
    selected: impl Fn(&KernelOwnerEdgeRole) -> bool,
    mode: PublishMode,
) -> Result<(), KernelOwnerBuildError> {
    let terms = selected_edge_terms(builder, context, node_index, node, selected)?;
    if terms.is_empty() && mode == PublishMode::Replace {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {node_index} has no selected provider"
        )));
    }
    builder.add_publish(output, terms, mode);
    Ok(())
}

/// Build the invocation-private requirement surface for one formal read.
///
/// Provider projections remain directional and detached. Requirements instead
/// form ordinary equations, so a use such as `width + 1` constrains the fresh
/// call frame without specializing the callable's principal frame or another
/// invocation.
fn requirement_projection(
    builder: &mut ComponentProgramBuilder,
    root: TypeVariableId,
    fields: &[crate::NameId],
) -> TypeVariableId {
    let mut provider = root;
    for field in fields {
        let consumer = builder.new_variable();
        let consumer_term = builder.variable_term(consumer);
        let scaffold = builder.terms_mut().object([(*field, consumer_term)], true);
        let provider_term = builder.variable_term(provider);
        builder.add_unify(provider_term, scaffold);
        provider = consumer;
    }
    provider
}

fn selected_edge_terms(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
    selected: impl Fn(&KernelOwnerEdgeRole) -> bool,
) -> Result<Vec<crate::TypeTermId>, KernelOwnerBuildError> {
    node.inputs
        .iter()
        .filter(|edge| selected(&edge.role))
        .map(|edge| {
            let variable = edge_variable(context, node_index, edge)?;
            Ok(builder.variable_term(variable))
        })
        .collect()
}

fn node_mode_equation(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
) -> Result<ModeEquation, KernelOwnerBuildError> {
    let copy = |selected: fn(&KernelOwnerEdgeRole) -> bool| {
        let mut inputs = node.inputs.iter().filter(|edge| selected(&edge.role));
        let input = inputs.next().ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "kernel owner node {node_index} has no provider for its flow mode"
            ))
        })?;
        if inputs.next().is_some() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {node_index} has multiple providers for one flow mode"
            )));
        }
        edge_mode_variable(context, node_index, input).map(ModeEquation::Copy)
    };
    match &node.kind {
        KernelOwnerNodeKind::FormalRead { formal, fields } => {
            let source = context
                .formal_mode_sources
                .get(*formal as usize)
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {node_index} reads missing formal mode source {formal}"
                    ))
                })?;
            projected_mode_variable(
                mode_builder,
                context,
                node_index,
                source,
                fields,
                &mut BTreeSet::new(),
            )
            .map(ModeEquation::Copy)
        }
        KernelOwnerNodeKind::ValueRead {
            fields,
            mode_narrowing,
        } => {
            if let Some(selector) = mode_narrowing {
                let selector = context
                    .expression_modes
                    .get(selector.0 as usize)
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {node_index} has an out-of-range mode-narrowing selector {}",
                            selector.0
                        ))
                    })?;
                return Ok(ModeEquation::Copy(selector));
            }
            // A cross-owner value read consumes the provider declaration's
            // public mode. Its field path affects the value type, but it does
            // not turn the ordinary checked occurrence into a projection of
            // the provider's private expression-mode tree. Contextual user
            // call inference deliberately performs that deeper projection in
            // `projected_mode_variable` below.
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} value read has no mode provider"
                ))
            })?;
            if providers.next().is_some() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} value read has multiple mode providers"
                )));
            }
            let reference = provider.expression.0 as usize;
            let public_result = reference
                .checked_sub(context.input.nodes.len())
                .and_then(|external| context.input.external_expressions.get(external))
                .is_some_and(|external| matches!(external.target, KernelExternalTarget::Result));
            if fields.is_empty() || public_result {
                edge_mode_variable(context, node_index, provider).map(ModeEquation::Copy)
            } else {
                projected_edge_mode_variable(mode_builder, context, node_index, provider, fields)
                    .map(ModeEquation::Copy)
            }
        }
        KernelOwnerNodeKind::LexicalRead { fields }
        | KernelOwnerNodeKind::DerivedRead { fields } => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} value read has no mode provider"
                ))
            })?;
            if providers.next().is_some() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} value read has multiple mode providers"
                )));
            }
            if fields.is_empty() {
                return edge_mode_variable(context, node_index, provider).map(ModeEquation::Copy);
            }
            projected_edge_mode_variable(mode_builder, context, node_index, provider, fields)
                .map(ModeEquation::Copy)
        }
        KernelOwnerNodeKind::CollectionItemRead => {
            copy(|role| matches!(role, KernelOwnerEdgeRole::ReadProvider))
        }
        KernelOwnerNodeKind::Block => copy(|role| matches!(role, KernelOwnerEdgeRole::BlockResult)),
        KernelOwnerNodeKind::Latest => node
            .inputs
            .iter()
            .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::LatestBranch))
            .map(|edge| edge_mode_variable(context, node_index, edge))
            .collect::<Result<Vec<_>, _>>()
            .map(|inputs| ModeEquation::Latest(inputs.into_boxed_slice())),
        KernelOwnerNodeKind::When => copy(|role| matches!(role, KernelOwnerEdgeRole::WhenInput)),
        KernelOwnerNodeKind::Draining => {
            copy(|role| matches!(role, KernelOwnerEdgeRole::DrainingInput))
        }
        KernelOwnerNodeKind::MatchArm { .. } => {
            if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
            {
                copy(|role| matches!(role, KernelOwnerEdgeRole::MatchOutput))
            } else if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. }))
            {
                Ok(ModeEquation::Fixed(node.mode))
            } else {
                Ok(ModeEquation::Fixed(FlowMode::Absent))
            }
        }
        KernelOwnerNodeKind::Arrow => {
            if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ArrowOutput))
            {
                copy(|role| matches!(role, KernelOwnerEdgeRole::ArrowOutput))
            } else {
                Ok(ModeEquation::Fixed(FlowMode::Absent))
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListMap,
        } => copy(
            |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "new"),
        ),
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListLatest,
        } => copy(
            |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if matches!(name.as_ref(), "$pipe" | "list")),
        ),
        KernelOwnerNodeKind::UserCall { .. } => Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {node_index} user-call mode must come from its invocation"
        ))),
        KernelOwnerNodeKind::Known(_)
        | KernelOwnerNodeKind::Source(_)
        | KernelOwnerNodeKind::Absent
        | KernelOwnerNodeKind::Text
        | KernelOwnerNodeKind::TextTemplate
        | KernelOwnerNodeKind::Number
        | KernelOwnerNodeKind::Byte
        | KernelOwnerNodeKind::Bits(_)
        | KernelOwnerNodeKind::Tag(_)
        | KernelOwnerNodeKind::Record { .. }
        | KernelOwnerNodeKind::Collection { .. }
        | KernelOwnerNodeKind::MapEntry
        | KernelOwnerNodeKind::RenderConstructor { .. }
        | KernelOwnerNodeKind::PureBuiltin { .. }
        | KernelOwnerNodeKind::HostEffect { .. }
        | KernelOwnerNodeKind::Then
        | KernelOwnerNodeKind::Infix { .. }
        | KernelOwnerNodeKind::Hold
        | KernelOwnerNodeKind::Delimiter
        | KernelOwnerNodeKind::Unknown => Ok(ModeEquation::Fixed(node.mode)),
    }
}

fn possible_when_arm_references(
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
) -> Result<BTreeSet<usize>, KernelOwnerBuildError> {
    let arm_edges = node
        .inputs
        .iter()
        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenArm))
        .collect::<Vec<_>>();
    let selector = node
        .inputs
        .iter()
        .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
        .and_then(|edge| edge_static_variants(context, edge));
    let Some(selector) = selector else {
        return Ok(arm_edges
            .into_iter()
            .map(|edge| edge.expression.0 as usize)
            .collect());
    };
    let mut selected = BTreeSet::new();
    for tag in selector {
        for edge in &arm_edges {
            let arm = referenced_node(context, node_index, edge)?;
            let KernelOwnerNodeKind::MatchArm { pattern } = &arm.kind else {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} WHEN targets a non-arm expression"
                )));
            };
            if static_pattern_accepts_tag(pattern, &tag) {
                selected.insert(edge.expression.0 as usize);
                break;
            }
        }
    }
    Ok(selected)
}

fn infix_returns_bool(operation: &str) -> bool {
    matches!(operation, "==" | "!=" | ">" | "<" | ">=" | "<=")
}

fn boolean_type(builder: &mut ComponentProgramBuilder) -> crate::TypeTermId {
    let false_tag = builder.terms_mut().variant_tag("False");
    let true_tag = builder.terms_mut().variant_tag("True");
    builder.terms_mut().variant_set([false_tag, true_tag])
}

fn parsed_number_type(builder: &mut ComponentProgramBuilder) -> crate::TypeTermId {
    let value = builder.terms_mut().intern_name("value");
    let number = builder.terms().number();
    let parsed_fields = builder.terms_mut().object([(value, number)], false);
    let parsed = builder.terms_mut().tagged_variant("Parsed", parsed_fields);

    let reason = builder.terms_mut().intern_name("reason");
    let position = builder.terms_mut().intern_name("position");
    let text = builder.terms().text();
    let number = builder.terms().number();
    let invalid_fields = builder
        .terms_mut()
        .object([(reason, text), (position, number)], false);
    let invalid = builder
        .terms_mut()
        .tagged_variant("InvalidNumber", invalid_fields);
    builder
        .terms_mut()
        .variant_set_preserving_order([parsed, invalid])
}

fn rounding_rule_type(builder: &mut ComponentProgramBuilder) -> crate::TypeTermId {
    let variants = ExactRoundingRule::ALL
        .into_iter()
        .map(|rule| builder.terms_mut().variant_tag(rule.as_tag()))
        .collect::<Vec<_>>();
    builder.terms_mut().variant_set(variants)
}

fn compile_host_effect(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    index: usize,
    node: &KernelOwnerNode,
    output: TypeVariableId,
    operation: &str,
) -> Result<(), KernelOwnerBuildError> {
    let spec = host_effect_spec(operation).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {index} names unknown host effect `{operation}`"
        ))
    })?;
    if spec.result_policy != ResultPolicySpec::ReturnValue {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {index} host effect `{operation}` has no return value"
        )));
    }
    let schema = spec.schema.ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {index} host effect `{operation}` has no type schema"
        ))
    })?;
    let ValueType::Record {
        fields,
        open: false,
    } = &schema.intent
    else {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {index} host effect `{operation}` requires a closed record intent"
        )));
    };

    let pipe_field = fields.first().map(|field| field.name);
    let mut arguments = BTreeMap::<&str, TypeVariableId>::new();
    for edge in &node.inputs {
        let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {index} host effect has invalid edge {:?}",
                edge.role
            )));
        };
        let name = if name.as_ref() == "$pipe" {
            pipe_field.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} cannot pipe into argument-free host effect `{operation}`"
                ))
            })?
        } else {
            name
        };
        if !fields.iter().any(|field| field.name == name) {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {index} host effect `{operation}` has no argument `{name}`"
            )));
        }
        let value = edge_variable(context, index, edge)?;
        if arguments.insert(name, value).is_some() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {index} host effect `{operation}` repeats argument `{name}`"
            )));
        }
    }
    for field in fields {
        let Some(actual) = arguments.get(field.name).copied() else {
            if schema
                .intent_defaults
                .iter()
                .any(|default| default.field_name == field.name)
            {
                continue;
            }
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {index} host effect `{operation}` omits required argument `{}`",
                field.name
            )));
        };
        let expected = effect_schema_type_to_checked(&field.value_type);
        let expected = builder
            .terms_mut()
            .import_checked_type(&expected, &mut |_| {
                unreachable!("host ABI types are closed")
            });
        let actual = builder.variable_term(actual);
        builder.add_unify(actual, expected);
    }

    let result = effect_schema_type_to_checked(&schema.result);
    let result = builder
        .terms_mut()
        .import_checked_type(&result, &mut |_| unreachable!("host ABI types are closed"));
    builder.add_publish(output, [result], PublishMode::Replace);
    Ok(())
}

fn effect_schema_type_to_checked(value_type: &ValueType) -> Type {
    match value_type {
        ValueType::Number => Type::Number,
        ValueType::Text => Type::Text,
        ValueType::Bytes { fixed_len } => {
            Type::Bytes(fixed_len.map_or(BytesType::Dynamic, |length| {
                BytesType::Fixed(
                    usize::try_from(length).expect("host ABI fixed byte length fits usize"),
                )
            }))
        }
        ValueType::List { item } => Type::List(Type::shared(effect_schema_type_to_checked(item))),
        ValueType::Record { fields, open } => Type::object(ObjectShape::from_ordered_fields(
            fields.iter().map(|field| {
                (
                    field.name.to_owned(),
                    effect_schema_type_to_checked(&field.value_type),
                )
            }),
            *open,
        )),
        ValueType::Variant { variants } => Type::VariantSet(
            variants
                .iter()
                .map(|variant| {
                    if variant.fields.is_empty() {
                        Variant::Tag(variant.tag.to_owned())
                    } else {
                        Variant::Tagged {
                            tag: variant.tag.to_owned(),
                            fields: ObjectShape::from_ordered_fields(
                                variant.fields.iter().map(|field| {
                                    (
                                        field.name.to_owned(),
                                        effect_schema_type_to_checked(&field.value_type),
                                    )
                                }),
                                false,
                            ),
                        }
                    }
                })
                .collect(),
        ),
    }
}

fn constrain_pure_builtin_argument(
    builder: &mut ComponentProgramBuilder,
    kind: KernelPureBuiltinKind,
    name: &str,
    value: TypeTermId,
) {
    if let Some(expected) = pure_builtin_argument_requirement(builder, kind, name) {
        builder.add_unify(value, expected);
    }
}

fn pure_builtin_argument_requirement(
    builder: &mut ComponentProgramBuilder,
    kind: KernelPureBuiltinKind,
    name: &str,
) -> Option<TypeTermId> {
    match kind {
        KernelPureBuiltinKind::TextTransform
        | KernelPureBuiltinKind::TextLength
        | KernelPureBuiltinKind::TextPredicate => Some(builder.terms().text()),
        KernelPureBuiltinKind::TextToNumber if matches!(name, "$pipe" | "input" | "text") => {
            Some(builder.terms().text())
        }
        KernelPureBuiltinKind::TextToNumber if name == "radix" => Some(builder.terms().number()),
        KernelPureBuiltinKind::TextToNumber => None,
        KernelPureBuiltinKind::TextSlice if matches!(name, "$pipe" | "input") => {
            Some(builder.terms().text())
        }
        KernelPureBuiltinKind::TextSlice => Some(builder.terms().number()),
        // Text/concat accepts the runtime's text-formattable scalar family.
        // That is a validation contract, not an equality constraint.
        KernelPureBuiltinKind::TextConcat | KernelPureBuiltinKind::FieldColor => None,
        KernelPureBuiltinKind::TextJoin if name == "$pipe" => {
            let item = builder.terms().text();
            Some(builder.terms_mut().list(item))
        }
        KernelPureBuiltinKind::TextJoin => Some(builder.terms().text()),
        KernelPureBuiltinKind::NumberToText if name == "prefix" => Some(boolean_type(builder)),
        KernelPureBuiltinKind::NumberToText | KernelPureBuiltinKind::NumberMath => {
            Some(builder.terms().number())
        }
        KernelPureBuiltinKind::NumberRound if name == "using" => Some(rounding_rule_type(builder)),
        KernelPureBuiltinKind::NumberRound => Some(builder.terms().number()),
        KernelPureBuiltinKind::NumberProjection if name == "zoom" => None,
        KernelPureBuiltinKind::NumberProjection => Some(builder.terms().number()),
        KernelPureBuiltinKind::ListLength
        | KernelPureBuiltinKind::ListPredicate
        | KernelPureBuiltinKind::ListFilter
        | KernelPureBuiltinKind::ListMap
        | KernelPureBuiltinKind::ListFind
        | KernelPureBuiltinKind::ListLatest
        | KernelPureBuiltinKind::ListAppend
        | KernelPureBuiltinKind::ListSort
        | KernelPureBuiltinKind::ListChunk
            if matches!(name, "$pipe" | "list") =>
        {
            let item = builder.new_variable();
            let item = builder.variable_term(item);
            Some(builder.terms_mut().list(item))
        }
        KernelPureBuiltinKind::ListFilter | KernelPureBuiltinKind::ListFind if name == "if" => {
            Some(boolean_type(builder))
        }
        KernelPureBuiltinKind::ListChunk if name == "size" => Some(builder.terms().number()),
        KernelPureBuiltinKind::ListLength
        | KernelPureBuiltinKind::ListPredicate
        | KernelPureBuiltinKind::ListFilter
        | KernelPureBuiltinKind::ListMap
        | KernelPureBuiltinKind::ListFind
        | KernelPureBuiltinKind::ListLatest
        | KernelPureBuiltinKind::ListAppend
        | KernelPureBuiltinKind::ListSort
        | KernelPureBuiltinKind::ListChunk => None,
    }
}

fn constrain_render_argument(builder: &mut ComponentProgramBuilder, name: &str, value: TypeTermId) {
    if let Some(expected) = render_argument_requirement(builder, name) {
        builder.add_unify(value, expected);
    }
}

fn render_argument_requirement(
    builder: &mut ComponentProgramBuilder,
    name: &str,
) -> Option<TypeTermId> {
    match name {
        "text"
        | "label"
        | "target"
        | "input_id"
        | "source"
        | "artifact_id"
        | "bootstrap_source"
        | "bootstrap_artifact_id" => Some(builder.terms().text()),
        "gap" | "revision" => Some(builder.terms().number()),
        "visible" | "selected" | "checked" | "focus" => {
            let false_tag = builder.terms_mut().variant_tag("False");
            let true_tag = builder.terms_mut().variant_tag("True");
            Some(builder.terms_mut().variant_set([false_tag, true_tag]))
        }
        "root" | "child" => Some(builder.terms().render_contract()),
        "items" | "contents" => {
            let item = builder.terms().render_contract();
            Some(builder.terms_mut().list(item))
        }
        "element" | "style" | "placeholder" | "lights" | "geometry" | "activate_focus" => {
            Some(builder.terms().open_object())
        }
        // Stripe direction is a Row/Column tag even though the legacy ABI
        // models the validation slot as an open object.
        "direction" => None,
        _ => None,
    }
}

fn infix_requires_number_operands(operation: &str) -> bool {
    matches!(
        operation,
        "+" | "-" | "*" | "/" | "%" | ">" | "<" | ">=" | "<="
    )
}

type ActiveModeProjection = BTreeSet<(KernelOwnerId, usize, usize, bool, bool)>;

fn owner_mode_input<'a>(
    context: &'a OwnerCompileContext<'a>,
    owner: KernelOwnerId,
) -> Result<&'a KernelOwnerProgramInput, KernelOwnerBuildError> {
    if owner == context.owner {
        return Ok(context.input);
    }
    context
        .project
        .and_then(|project| project.owners.get(owner.0 as usize))
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} mode projection references missing owner {}",
                context.owner.0, owner.0
            ))
        })
}

fn mode_source_for_edge(
    context: &OwnerCompileContext<'_>,
    owner: KernelOwnerId,
    expression_modes: &Arc<[ModeVariableId]>,
    formal_sources: &Arc<[ModeSource]>,
    edge: &KernelOwnerInputEdge,
) -> Result<ModeSource, KernelOwnerBuildError> {
    let input = owner_mode_input(context, owner)?;
    let reference = edge.expression.0 as usize;
    if reference < input.nodes.len() {
        return Ok(ModeSource::Expression {
            owner,
            expression: reference,
            expression_modes: Arc::clone(expression_modes),
            formal_sources: Arc::clone(formal_sources),
        });
    }
    let external_index = reference - input.nodes.len();
    let external = input.external_expressions.get(external_index).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "owner {} mode projection references external expression {external_index} outside 0..{}",
            owner.0,
            input.external_expressions.len()
        ))
    })?;
    let project = context.project.ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "owner {} mode projection references an external owner outside a project",
            owner.0
        ))
    })?;
    let target = project
        .owners
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} mode projection references missing owner {}",
                owner.0, external.owner.0
            ))
        })?;
    let target_instance = context
        .principals
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} mode projection references missing principal owner {}",
                owner.0, external.owner.0
            ))
        })?;
    let expression = match external.target {
        KernelExternalTarget::Expression(expression) => {
            checked_expression_index(expression, target.nodes.len(), "external mode projection")?
        }
        KernelExternalTarget::Result => checked_expression_index(
            target.result,
            target.nodes.len(),
            "external result mode projection",
        )?,
    };
    Ok(ModeSource::Expression {
        owner: external.owner,
        expression,
        expression_modes: Arc::clone(&target_instance.expression_modes),
        formal_sources: Arc::clone(&target_instance.formal_mode_sources),
    })
}

fn selected_mode_edge<'a>(
    node: &'a KernelOwnerNode,
    selected: impl Fn(&KernelOwnerEdgeRole) -> bool,
) -> Option<&'a KernelOwnerInputEdge> {
    node.inputs.iter().find(|edge| selected(&edge.role))
}

fn merge_projected_modes(
    mode_builder: &mut ModeProgramBuilder,
    mut inputs: Vec<ModeVariableId>,
    fallback: ModeVariableId,
) -> ModeVariableId {
    inputs.sort_unstable();
    inputs.dedup();
    match inputs.as_slice() {
        [] => fallback,
        [input] => *input,
        _ => {
            let output = mode_builder.new_variable(FlowMode::Continuous);
            mode_builder.set(output, ModeEquation::Latest(inputs.into_boxed_slice()));
            output
        }
    }
}

fn merge_contextual_call_modes(
    mode_builder: &mut ModeProgramBuilder,
    mut inputs: Vec<ModeVariableId>,
    result: FlowMode,
    fallback: ModeVariableId,
) -> ModeVariableId {
    inputs.sort_unstable();
    inputs.dedup();
    if inputs.is_empty() {
        return fallback;
    }
    let output = mode_builder.new_variable(result);
    mode_builder.set(
        output,
        ModeEquation::Call {
            result,
            inputs: inputs.into_boxed_slice(),
        },
    );
    output
}

fn eventful_projected_mode(
    mode_builder: &mut ModeProgramBuilder,
    input: ModeVariableId,
) -> ModeVariableId {
    let output = mode_builder.new_variable(FlowMode::Continuous);
    mode_builder.set(output, ModeEquation::Eventful(input));
    output
}

fn user_call_result_mode_source(
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    owner: KernelOwnerId,
    expression_modes: &Arc<[ModeVariableId]>,
    formal_sources: &Arc<[ModeSource]>,
    node: &KernelOwnerNode,
    target: KernelOwnerId,
    inherited_formal: Option<KernelInheritedFormal>,
) -> Result<ModeSource, KernelOwnerBuildError> {
    let project = context.project.ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects through a user call outside a project"
        ))
    })?;
    let target_input = project.owners.get(target.0 as usize).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects through missing owner {}",
            target.0
        ))
    })?;
    let target_instance = context.principals.get(target.0 as usize).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects through missing principal owner {}",
            target.0
        ))
    })?;
    let mut actuals = vec![None; target_input.formal_count as usize];
    for edge in &node.inputs {
        let KernelOwnerEdgeRole::CallArgument { ordinal } = edge.role else {
            continue;
        };
        let actual = mode_source_for_edge(context, owner, expression_modes, formal_sources, edge)?;
        let slot = actuals.get_mut(ordinal as usize).ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "kernel owner node {source_node} projects out-of-range argument {ordinal}"
            ))
        })?;
        if slot.replace(actual).is_some() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {source_node} projects repeated argument {ordinal}"
            )));
        }
    }
    if let Some(inherited) = inherited_formal {
        let actual = formal_sources
            .get(inherited.caller_ordinal as usize)
            .cloned()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {source_node} projects missing inherited formal {}",
                    inherited.caller_ordinal
                ))
            })?;
        let slot = actuals
            .get_mut(inherited.target_ordinal as usize)
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {source_node} projects out-of-range inherited formal {}",
                    inherited.target_ordinal
                ))
            })?;
        if slot.replace(actual).is_some() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {source_node} projects repeated inherited formal {}",
                inherited.target_ordinal
            )));
        }
    }
    let actuals = actuals
        .into_iter()
        .enumerate()
        .map(|(ordinal, actual)| {
            actual.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {source_node} projects omitted argument {ordinal}"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = checked_expression_index(
        target_input.result,
        target_input.nodes.len(),
        "projected user-call result",
    )?;
    Ok(ModeSource::Expression {
        owner: target,
        expression: result,
        expression_modes: Arc::clone(&target_instance.expression_modes),
        formal_sources: actuals.into(),
    })
}

fn projected_edge_mode_variable(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
    fields: &[Box<str>],
) -> Result<ModeVariableId, KernelOwnerBuildError> {
    let source = mode_source_for_edge(
        context,
        context.owner,
        context.expression_modes,
        context.formal_mode_sources,
        edge,
    )?;
    projected_mode_variable(
        mode_builder,
        context,
        node_index,
        &source,
        fields,
        &mut BTreeSet::new(),
    )
}

fn projected_mode_variable(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    source: &ModeSource,
    fields: &[Box<str>],
    active: &mut ActiveModeProjection,
) -> Result<ModeVariableId, KernelOwnerBuildError> {
    let ModeSource::Expression {
        owner,
        expression,
        expression_modes,
        formal_sources,
    } = source
    else {
        return Ok(source.root_mode());
    };
    let input = owner_mode_input(context, *owner)?;
    let node = input.nodes.get(*expression).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects missing owner {} expression {expression}",
            owner.0
        ))
    })?;
    let mode = source.root_mode();
    let unproven_branch = active.iter().any(|key| key.4)
        || matches!(
            node.kind,
            KernelOwnerNodeKind::Latest | KernelOwnerNodeKind::When
        );
    let active_key = (*owner, *expression, fields.len(), false, unproven_branch);
    if !active.insert(active_key) {
        return Ok(mode);
    }
    let follow = |edge: &KernelOwnerInputEdge| {
        mode_source_for_edge(context, *owner, expression_modes, formal_sources, edge)
    };
    let result = match &node.kind {
        KernelOwnerNodeKind::FormalRead {
            formal,
            fields: provider_fields,
        } => {
            let mut combined = provider_fields.to_vec();
            combined.extend_from_slice(fields);
            let provider = formal_sources.get(*formal as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {source_node} projects missing formal {formal}"
                ))
            })?;
            projected_mode_variable(
                mode_builder,
                context,
                source_node,
                provider,
                &combined,
                active,
            )?
        }
        KernelOwnerNodeKind::LexicalRead {
            fields: provider_fields,
        }
        | KernelOwnerNodeKind::ValueRead {
            fields: provider_fields,
            ..
        }
        | KernelOwnerNodeKind::DerivedRead {
            fields: provider_fields,
        } => {
            let provider = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::ReadProvider)
            });
            if let Some(provider) = provider {
                let mut combined = provider_fields.to_vec();
                combined.extend_from_slice(fields);
                let provider = follow(provider)?;
                if combined.is_empty() {
                    provider.root_mode()
                } else {
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        &combined,
                        active,
                    )?
                }
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::UserCall {
            target,
            inherited_formal,
        } => {
            let result = user_call_result_mode_source(
                context,
                source_node,
                *owner,
                expression_modes,
                formal_sources,
                node,
                *target,
                *inherited_formal,
            )?;
            projected_mode_variable(mode_builder, context, source_node, &result, fields, active)?
        }
        KernelOwnerNodeKind::Block => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::BlockResult)
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Draining => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::DrainingInput)
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::MatchArm { .. } => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::MatchOutput)
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Arrow => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::ArrowOutput)
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::CollectionItemRead => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::ReadProvider)
            }) {
                let provider = follow(edge)?;
                if fields.is_empty() {
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )?
                } else {
                    projected_collection_item_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )?
                }
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListMap,
        } if fields.is_empty() => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "$pipe"),
            ) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                let inputs = node
                    .inputs
                    .iter()
                    .map(|edge| {
                        let provider = follow(edge)?;
                        projected_mode_variable(
                            mode_builder,
                            context,
                            source_node,
                            &provider,
                            fields,
                            active,
                        )
                    })
                    .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
                merge_contextual_call_modes(mode_builder, inputs, node.mode, mode)
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListMap,
        } => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "new"),
            ) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListLatest,
        } if fields.is_empty() => {
            let inputs = node
                .inputs
                .iter()
                .map(|edge| {
                    let provider = follow(edge)?;
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )
                })
                .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
            merge_contextual_call_modes(mode_builder, inputs, node.mode, mode)
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListLatest,
        } => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if matches!(name.as_ref(), "$pipe" | "list")),
            ) {
                let provider = follow(edge)?;
                if fields.is_empty() {
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )?
                } else {
                    projected_collection_item_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )?
                }
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::When if fields.is_empty() => {
            if let Some(edge) =
                selected_mode_edge(node, |role| matches!(role, KernelOwnerEdgeRole::WhenInput))
            {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Latest | KernelOwnerNodeKind::When => {
            let selected = |role: &KernelOwnerEdgeRole| match &node.kind {
                KernelOwnerNodeKind::Latest => {
                    matches!(role, KernelOwnerEdgeRole::LatestBranch)
                }
                KernelOwnerNodeKind::When => matches!(role, KernelOwnerEdgeRole::WhenArm),
                _ => false,
            };
            let projected = node
                .inputs
                .iter()
                .filter(|edge| selected(&edge.role))
                .map(|edge| {
                    let provider = follow(edge)?;
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )
                })
                .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
            merge_projected_modes(mode_builder, projected, mode)
        }
        KernelOwnerNodeKind::PureBuiltin { .. }
        | KernelOwnerNodeKind::RenderConstructor { .. }
        | KernelOwnerNodeKind::HostEffect { .. }
            if fields.is_empty() =>
        {
            let inputs = node
                .inputs
                .iter()
                .map(|edge| {
                    let provider = follow(edge)?;
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )
                })
                .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
            merge_contextual_call_modes(mode_builder, inputs, node.mode, mode)
        }
        KernelOwnerNodeKind::Record { .. } if !fields.is_empty() => {
            let field = &fields[0];
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(
                    role,
                    KernelOwnerEdgeRole::RecordField { name, spread: false }
                        if name.as_ref() == field.as_ref()
                )
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    &fields[1..],
                    active,
                )?
            } else {
                let projected_spreads = node
                    .inputs
                    .iter()
                    .filter(|edge| {
                        matches!(
                            edge.role,
                            KernelOwnerEdgeRole::RecordField { spread: true, .. }
                        )
                    })
                    .map(|edge| {
                        let provider = follow(edge)?;
                        projected_mode_variable(
                            mode_builder,
                            context,
                            source_node,
                            &provider,
                            fields,
                            active,
                        )
                    })
                    .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
                if projected_spreads.is_empty() && unproven_branch {
                    // A closed branch which does not define this field is not
                    // a continuous provider for the projection. Keep it
                    // unresolved here so a real eventful provider in another
                    // WHEN/LATEST branch determines the merged occurrence.
                    eventful_projected_mode(mode_builder, mode)
                } else if projected_spreads.is_empty() {
                    mode
                } else {
                    merge_projected_modes(mode_builder, projected_spreads, mode)
                }
            }
        }
        _ if !fields.is_empty() => eventful_projected_mode(mode_builder, mode),
        _ => mode,
    };
    active.remove(&active_key);
    Ok(result)
}

fn projected_collection_item_mode_variable(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    source: &ModeSource,
    fields: &[Box<str>],
    active: &mut ActiveModeProjection,
) -> Result<ModeVariableId, KernelOwnerBuildError> {
    let ModeSource::Expression {
        owner,
        expression,
        expression_modes,
        formal_sources,
    } = source
    else {
        return Ok(source.root_mode());
    };
    let input = owner_mode_input(context, *owner)?;
    let node = input.nodes.get(*expression).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects an item from missing owner {} expression {expression}",
            owner.0
        ))
    })?;
    let mode = source.root_mode();
    let active_key = (*owner, *expression, fields.len(), true, true);
    if !active.insert(active_key) {
        return Ok(mode);
    }
    let follow = |edge: &KernelOwnerInputEdge| {
        mode_source_for_edge(context, *owner, expression_modes, formal_sources, edge)
    };
    let result = match &node.kind {
        KernelOwnerNodeKind::Collection {
            kind: KernelCollectionKind::List | KernelCollectionKind::Set,
            ..
        } => {
            let projected = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::CollectionItem))
                .map(|edge| {
                    let provider = follow(edge)?;
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )
                })
                .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
            merge_projected_modes(mode_builder, projected, mode)
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListMap,
        } => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "new"),
            ) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListFilter | KernelPureBuiltinKind::ListSort,
        } => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if matches!(name.as_ref(), "$pipe" | "list")),
            ) {
                let provider = follow(edge)?;
                projected_collection_item_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::LexicalRead {
            fields: provider_fields,
        }
        | KernelOwnerNodeKind::ValueRead {
            fields: provider_fields,
            ..
        }
        | KernelOwnerNodeKind::DerivedRead {
            fields: provider_fields,
        } if provider_fields.is_empty() => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::ReadProvider)
            }) {
                let provider = follow(edge)?;
                projected_collection_item_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Block => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::BlockResult)
            }) {
                let provider = follow(edge)?;
                projected_collection_item_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Draining => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::DrainingInput)
            }) {
                let provider = follow(edge)?;
                projected_collection_item_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::UserCall {
            target,
            inherited_formal,
        } => {
            let provider = user_call_result_mode_source(
                context,
                source_node,
                *owner,
                expression_modes,
                formal_sources,
                node,
                *target,
                *inherited_formal,
            )?;
            projected_collection_item_mode_variable(
                mode_builder,
                context,
                source_node,
                &provider,
                fields,
                active,
            )?
        }
        _ => mode,
    };
    active.remove(&active_key);
    Ok(result)
}

fn edge_variable(
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
) -> Result<TypeVariableId, KernelOwnerBuildError> {
    let reference = edge.expression.0 as usize;
    if reference < context.input.nodes.len() {
        return Ok(context.expressions[reference]);
    }
    let external_index = reference - context.input.nodes.len();
    if let Some(external_variables) = context.external_variables {
        return external_variables
            .get(external_index)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "input of owner {} node {node_index} references external frame variable {external_index} outside 0..{}",
                    context.owner.0,
                    external_variables.len()
                ))
            });
    }
    let external = context
        .input
        .external_expressions
        .get(external_index)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "input of owner {} node {node_index} references external expression {external_index} outside 0..{}",
                context.owner.0,
                context.input.external_expressions.len()
            ))
        })?;
    let target_owner = context
        .principals
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "input of owner {} node {node_index} references missing owner {}",
                context.owner.0, external.owner.0
            ))
        })?;
    match external.target {
        KernelExternalTarget::Expression(expression) => target_owner
            .expressions
            .get(expression.0 as usize)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "input of owner {} node {node_index} references missing owner {} expression {}",
                    context.owner.0, external.owner.0, expression.0
                ))
            }),
        KernelExternalTarget::Result => {
            let project = context.project.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "input of owner {} node {node_index} references an owner result outside a project",
                    context.owner.0
                ))
            })?;
            let target_input = project
                .owners
                .get(external.owner.0 as usize)
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "input of owner {} node {node_index} references missing owner {} result",
                        context.owner.0, external.owner.0
                    ))
                })?;
            let result = checked_expression_index(
                target_input.result,
                target_input.nodes.len(),
                "external owner result",
            )?;
            Ok(target_owner.expressions[result])
        }
    }
}

fn referenced_node<'a>(
    context: &'a OwnerCompileContext<'a>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
) -> Result<&'a KernelOwnerNode, KernelOwnerBuildError> {
    let reference = edge.expression.0 as usize;
    if reference < context.input.nodes.len() {
        return context.input.nodes.get(reference).ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} node {node_index} references missing local node {reference}",
                context.owner.0
            ))
        });
    }
    let external_index = reference - context.input.nodes.len();
    let external = context
        .input
        .external_expressions
        .get(external_index)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} node {node_index} references missing external node {external_index}",
                context.owner.0
            ))
        })?;
    let project = context.project.ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "owner {} node {node_index} references an external node outside a project",
            context.owner.0
        ))
    })?;
    let target = project
        .owners
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} node {node_index} references missing owner {}",
                context.owner.0, external.owner.0
            ))
        })?;
    let expression = match external.target {
        KernelExternalTarget::Expression(expression) => {
            checked_expression_index(expression, target.nodes.len(), "external node reference")?
        }
        KernelExternalTarget::Result => checked_expression_index(
            target.result,
            target.nodes.len(),
            "external result node reference",
        )?,
    };
    target.nodes.get(expression).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "owner {} node {node_index} references missing owner {} node {expression}",
            context.owner.0, external.owner.0
        ))
    })
}

fn edge_mode_variable(
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
) -> Result<ModeVariableId, KernelOwnerBuildError> {
    let reference = edge.expression.0 as usize;
    if reference < context.input.nodes.len() {
        return Ok(context.expression_modes[reference]);
    }
    let external_index = reference - context.input.nodes.len();
    let external = context
        .input
        .external_expressions
        .get(external_index)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "mode input of owner {} node {node_index} references external expression {external_index} outside 0..{}",
                context.owner.0,
                context.input.external_expressions.len()
            ))
        })?;
    let target_owner = context
        .principals
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "mode input of owner {} node {node_index} references missing owner {}",
                context.owner.0, external.owner.0
            ))
        })?;
    match external.target {
        KernelExternalTarget::Expression(expression) => target_owner
            .expression_modes
            .get(expression.0 as usize)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "mode input of owner {} node {node_index} references missing owner {} expression {}",
                    context.owner.0, external.owner.0, expression.0
                ))
            }),
        KernelExternalTarget::Result => {
            let project = context.project.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "mode input of owner {} node {node_index} references an owner result outside a project",
                    context.owner.0
                ))
            })?;
            let target_input = project
                .owners
                .get(external.owner.0 as usize)
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "mode input of owner {} node {node_index} references missing owner {} result",
                        context.owner.0, external.owner.0
                    ))
                })?;
            let result = checked_expression_index(
                target_input.result,
                target_input.nodes.len(),
                "external owner result mode",
            )?;
            Ok(target_owner.expression_modes[result])
        }
    }
}

fn checked_expression_index(
    expression: KernelExpressionId,
    len: usize,
    context: &str,
) -> Result<usize, KernelOwnerBuildError> {
    let index = expression.0 as usize;
    if index >= len {
        return Err(KernelOwnerBuildError::new(format!(
            "{context} references expression {} outside 0..{len}",
            expression.0
        )));
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_checked::{ObjectShape, Type, Variant};

    fn edge(role: KernelOwnerEdgeRole, expression: u32) -> KernelOwnerInputEdge {
        KernelOwnerInputEdge {
            role,
            expression: KernelExpressionId(expression),
        }
    }

    #[test]
    fn user_call_modes_follow_fixed_builtin_inputs_contextually() {
        let input = KernelProjectProgramInput {
            owners: vec![
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::FormalRead {
                            formal: 0,
                            fields: Box::new([]),
                        },
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 1,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(0),
                },
                KernelOwnerProgramInput {
                    nodes: vec![
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Known(Type::Text),
                            inputs: Box::new([]),
                            mode: FlowMode::PresentOrAbsent,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::PureBuiltin {
                                kind: KernelPureBuiltinKind::TextTransform,
                            },
                            inputs: vec![edge(
                                KernelOwnerEdgeRole::AbiArgument {
                                    name: "$pipe".into(),
                                },
                                0,
                            )]
                            .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::UserCall {
                                target: KernelOwnerId(0),
                                inherited_formal: None,
                            },
                            inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                                .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                    ]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(2),
                },
            ]
            .into_boxed_slice(),
        };

        let artifact = compile_project_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definitions[1].expressions[1].flow_type.mode,
            FlowMode::Continuous,
            "the builtin's ordinary checked surface remains fixed"
        );
        assert_eq!(
            artifact.definitions[1].result.mode,
            FlowMode::PresentOrAbsent,
            "the user-call frame follows the builtin's eventful actual"
        );
    }

    #[test]
    fn projected_mode_ignores_unproven_continuous_branches() {
        let mut builder = ModeProgramBuilder::default();
        let continuous = builder.new_variable(FlowMode::Continuous);
        let present = builder.new_variable(FlowMode::PresentOrAbsent);
        builder.set(continuous, ModeEquation::Fixed(FlowMode::Continuous));
        builder.set(present, ModeEquation::Fixed(FlowMode::PresentOrAbsent));
        let continuous_projection = eventful_projected_mode(&mut builder, continuous);
        let present_projection = eventful_projected_mode(&mut builder, present);
        let merged = builder.new_variable(FlowMode::Continuous);
        builder.set(
            merged,
            ModeEquation::Latest(
                vec![continuous_projection, present_projection].into_boxed_slice(),
            ),
        );

        let modes = builder.solve();

        assert_eq!(modes[merged.0 as usize], FlowMode::PresentOrAbsent);
    }

    #[test]
    fn missing_record_field_does_not_mask_an_eventful_projection() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Number),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "other".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Text),
                    inputs: Box::new([]),
                    mode: FlowMode::PresentOrAbsent,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "wanted".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Latest,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::LatestBranch, 1),
                        edge(KernelOwnerEdgeRole::LatestBranch, 3),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::DerivedRead {
                        fields: vec!["wanted".into()].into_boxed_slice(),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 4)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::DerivedRead {
                        fields: vec!["wanted".into()].into_boxed_slice(),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(5),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definition.result.mode,
            FlowMode::PresentOrAbsent,
            "a closed branch without the projected field contributes no continuous provider",
        );
        assert_eq!(
            artifact.definition.expressions[6].flow_type.mode,
            FlowMode::Continuous,
            "a direct missing-field occurrence retains its declared/root mode",
        );
    }

    #[test]
    fn tag_matched_value_read_uses_the_selector_mode() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Initial".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Text),
                    inputs: Box::new([]),
                    mode: FlowMode::PresentOrAbsent,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record {
                        tag: Some("Ready".into()),
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "wanted".into(),
                            spread: false,
                        },
                        1,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Latest,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::LatestBranch, 0),
                        edge(KernelOwnerEdgeRole::LatestBranch, 2),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::ValueRead {
                        fields: Box::new([]),
                        mode_narrowing: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::ValueRead {
                        fields: vec!["wanted".into()].into_boxed_slice(),
                        mode_narrowing: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::ValueRead {
                        fields: vec!["wanted".into()].into_boxed_slice(),
                        mode_narrowing: Some(KernelExpressionId(4)),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "Ready".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 6)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Wildcard,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 8)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 4),
                        edge(KernelOwnerEdgeRole::WhenArm, 7),
                        edge(KernelOwnerEdgeRole::WhenArm, 9),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(10),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definition.expressions[5].flow_type.mode,
            FlowMode::PresentOrAbsent,
            "an unguarded projection retains the eventful update surface",
        );
        assert_eq!(
            artifact.definition.expressions[6].flow_type.mode,
            FlowMode::Continuous,
            "the matching tag makes the retained selector mode authoritative",
        );
    }

    #[test]
    fn invocation_match_arms_alias_their_outputs_without_a_publish_cell() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "True".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 0),
                        edge(KernelOwnerEdgeRole::WhenArm, 2),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(3),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("True".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, wrapper, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().invocation_frames, 0);
        assert!(program.component().operations.iter().any(|operation| {
            matches!(
                operation.as_ref(),
                crate::KernelOperation::SummaryCall { program, .. }
                    if program.nodes.iter().any(|node| matches!(node, KernelSummaryNode::Select { .. }))
                        && program.nodes.iter().all(|node| !matches!(node, KernelSummaryNode::Invoke { .. }))
            )
        }), "the tiny selector callee should remain inline below the sharing threshold");
        assert_eq!(
            program.solve().unwrap().definitions[2].result.ty,
            Type::Number
        );
    }

    #[test]
    fn owner_program_compiles_a_widened_record_list() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Header".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "kind".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Empty".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "kind".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Collection {
                        kind: KernelCollectionKind::List,
                        capacity: None,
                    },
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::CollectionItem, 1),
                        edge(KernelOwnerEdgeRole::CollectionItem, 3),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();
        let Type::List(item) = artifact.definition.result.ty else {
            panic!("owner result must be a list")
        };
        let Type::Object(item) = item.as_ref() else {
            panic!("list item must be a record")
        };
        assert_eq!(
            item.fields["kind"],
            Type::VariantSet(
                vec![
                    Variant::Tag("Empty".to_owned()),
                    Variant::Tag("Header".to_owned())
                ]
                .into()
            )
        );
        assert_eq!(
            artifact.definition.expressions[1].flow_type.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "kind".to_owned(),
                    Type::VariantSet(vec![Variant::Tag("Header".to_owned())].into()),
                )],
                false,
            )),
            "directional collection widening must not backflow into producers"
        );
    }

    #[test]
    fn empty_latest_is_an_unknown_shape_and_not_an_absent_hold_update() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Latest,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 0),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 1),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definition.expressions[1].flow_type.ty,
            Type::Unknown
        );
        assert_eq!(
            artifact.definition.expressions[1].flow_type.mode,
            FlowMode::Continuous
        );
        assert_eq!(artifact.definition.result.ty, Type::Number);
    }

    #[test]
    fn host_effect_call_and_policy_are_published_once_in_the_definition_artifact() {
        let input = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::HostEffect {
                    operation: "Clock/wall".into(),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();
        let [call] = artifact.definition.calls.as_ref() else {
            panic!("host effect must publish one call artifact")
        };
        let [effect] = artifact.definition.effects.as_ref() else {
            panic!("host effect must publish one policy artifact")
        };
        let spec = host_effect_spec("Clock/wall").expect("wall-clock effect ABI");

        assert_eq!(call.expression, KernelExpressionId(0));
        assert_eq!(
            call.target,
            KernelCallTarget::HostEffect {
                operation: "Clock/wall".into(),
            }
        );
        assert!(call.inputs.is_empty());
        assert_eq!(call.result, artifact.definition.expressions[0].flow_type);
        assert_eq!(effect.expression, KernelExpressionId(0));
        assert_eq!(effect.operation.as_ref(), spec.operation);
        assert_eq!(effect.replay, spec.replay);
        assert_eq!(effect.barrier, spec.barrier);
        assert_eq!(effect.result_policy, spec.result_policy);
        assert_eq!(effect.delivery, spec.delivery);
    }

    #[test]
    fn empty_collections_use_language_neutral_item_authorities() {
        let solve = |kind| {
            compile_owner_program(&KernelOwnerProgramInput {
                nodes: vec![KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Collection {
                        kind,
                        capacity: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                }]
                .into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
                result: KernelExpressionId(0),
            })
            .unwrap()
            .solve()
            .unwrap()
            .definition
            .result
            .ty
        };

        assert_eq!(
            solve(KernelCollectionKind::List),
            Type::List(Type::shared(Type::object(ObjectShape::new(
                std::collections::BTreeMap::new(),
                true,
            ))))
        );
        assert_eq!(
            solve(KernelCollectionKind::Set),
            Type::Set(Type::shared(Type::Unknown))
        );
        assert_eq!(
            solve(KernelCollectionKind::Map),
            Type::Map {
                key: Box::new(Type::Unknown),
                value: Box::new(Type::Unknown),
            }
        );
    }

    #[test]
    fn stripe_constructor_compiles_direction_to_one_exact_render_kind() {
        let solve = |direction: &str| {
            compile_owner_program(&KernelOwnerProgramInput {
                nodes: vec![
                    KernelOwnerNode {
                        kind: KernelOwnerNodeKind::Tag(direction.into()),
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    },
                    KernelOwnerNode {
                        kind: KernelOwnerNodeKind::RenderConstructor {
                            kind: KernelRenderConstructorKind::StripeDirection,
                        },
                        inputs: vec![edge(
                            KernelOwnerEdgeRole::AbiArgument {
                                name: "direction".into(),
                            },
                            0,
                        )]
                        .into_boxed_slice(),
                        mode: FlowMode::Continuous,
                    },
                ]
                .into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
                result: KernelExpressionId(1),
            })
            .unwrap()
            .solve()
            .unwrap()
            .definition
            .result
            .ty
        };
        let expected = |direction: &str, kind: &str| {
            Type::object(ObjectShape::from_ordered_fields(
                [
                    (
                        "direction".to_owned(),
                        Type::VariantSet(vec![Variant::Tag(direction.to_owned())].into()),
                    ),
                    (
                        "kind".to_owned(),
                        Type::VariantSet(vec![Variant::Tag(kind.to_owned())].into()),
                    ),
                ],
                false,
            ))
        };
        assert_eq!(solve("Row"), expected("Row", "Row"));
        assert_eq!(solve("Column"), expected("Column", "Stack"));
    }

    #[test]
    fn record_spreads_overlay_fields_in_authored_order() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "family".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "size".into(),
                                spread: false,
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "base".into(),
                                spread: true,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "family".into(),
                                spread: false,
                            },
                            1,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "color".into(),
                                spread: false,
                            },
                            3,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };
        let result = compile_owner_program(&input)
            .unwrap()
            .solve()
            .unwrap()
            .definition
            .result;
        assert_eq!(
            result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("family".to_owned(), Type::Number),
                    ("size".to_owned(), Type::Number),
                    ("color".to_owned(), Type::Text),
                ],
                false,
            ))
        );
    }

    #[test]
    fn acyclic_user_calls_compose_fresh_formal_frames_into_one_component() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "number".into(),
                                spread: false,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "text".into(),
                                spread: false,
                            },
                            3,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().direct_result_summaries, 2);
        assert_eq!(
            program.compile_work().invocation_frames,
            0,
            "direct formal-result summaries must not allocate callee frames",
        );
        let artifact = program.solve().unwrap();
        assert_eq!(
            artifact.definitions[1].result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("number".to_owned(), Type::Number),
                    ("text".to_owned(), Type::Text),
                ],
                false,
            )),
            "repeated calls must not share or specialize one formal frame"
        );
        assert_eq!(
            artifact.definitions[1].expressions[2].flow_type.ty,
            Type::Number
        );
        assert_eq!(
            artifact.definitions[1].expressions[3].flow_type.ty,
            Type::Text
        );
        let [number_call, text_call] = artifact.definitions[1].calls.as_ref() else {
            panic!("caller definition must publish both call occurrences")
        };
        assert_eq!(number_call.expression, KernelExpressionId(2));
        assert_eq!(text_call.expression, KernelExpressionId(3));
        assert_eq!(
            number_call.target,
            KernelCallTarget::User {
                target: KernelOwnerId(0),
                inherited_formal: None,
            }
        );
        assert_eq!(
            number_call.inputs.as_ref(),
            [KernelCallInputArtifact {
                role: KernelCallInputRole::Formal { ordinal: 0 },
                value: KernelCallValueReference::Local(KernelExpressionId(0)),
            }]
        );
        assert_eq!(number_call.result.ty, Type::Number);
        assert_eq!(text_call.result.ty, Type::Text);
        let number_expression = &artifact.definitions[1].expressions[2];
        assert_eq!(number_expression.id, KernelExpressionId(2));
        assert_eq!(
            number_expression.kind,
            KernelOwnerNodeKind::UserCall {
                target: KernelOwnerId(0),
                inherited_formal: None,
            }
        );
        assert_eq!(
            number_expression.inputs.as_ref(),
            [KernelExpressionInputArtifact {
                role: KernelOwnerEdgeRole::CallArgument { ordinal: 0 },
                value: KernelValueReference::Local(KernelExpressionId(0)),
            }]
        );
        assert_eq!(number_expression.flow_type, number_call.result);
        assert!(artifact.definitions[0].calls.is_empty());
    }

    #[test]
    fn structural_result_summary_inlines_trivial_nested_bytecode() {
        let identity = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "value".into(),
                            spread: false,
                        },
                        1,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "number".into(),
                                spread: false,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "text".into(),
                                spread: false,
                            },
                            3,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![identity, wrapper, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().invocation_frames, 0);
        assert_eq!(
            program.compile_work().direct_result_summaries,
            3,
            "the wrapper principal plus both caller occurrences use summaries",
        );
        let summary_programs = program
            .component()
            .operations
            .iter()
            .filter_map(|operation| match operation.as_ref() {
                crate::KernelOperation::SummaryCall { program, .. } => Some(program),
                _ => None,
            })
            .collect::<Vec<_>>();
        let [number_summary, text_summary] = summary_programs.as_slice() else {
            panic!("two caller occurrences must use the compiled wrapper summary")
        };
        assert!(
            Arc::ptr_eq(number_summary, text_summary),
            "compatible calls must share one immutable result-summary program",
        );
        assert!(
            number_summary
                .nodes
                .iter()
                .all(|node| !matches!(node, KernelSummaryNode::Invoke { .. })),
            "a one-node identity summary must stay inline",
        );
        let artifact = program.solve().unwrap();
        let Type::Object(result) = &artifact.definitions[2].result.ty else {
            panic!("caller result must be a record")
        };
        let Type::Object(number) = &result.fields["number"] else {
            panic!("number result must be a record")
        };
        let Type::Object(text) = &result.fields["text"] else {
            panic!("text result must be a record")
        };
        assert_eq!(number.fields["value"], Type::Number);
        assert_eq!(text.fields["value"], Type::Text);
    }

    #[test]
    fn structural_result_summary_shares_identical_formal_projection_inputs() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "left".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "right".into(),
                                spread: false,
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        let calls = program
            .component()
            .operations
            .iter()
            .filter_map(|operation| match operation.as_ref() {
                crate::KernelOperation::SummaryCall {
                    program, inputs, ..
                } if program.definition == 0 => Some((program, inputs)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!calls.is_empty(), "the callee must use its direct summary");
        for (summary, inputs) in calls {
            assert_eq!(
                summary
                    .nodes
                    .iter()
                    .filter(|node| matches!(node, KernelSummaryNode::Input(_)))
                    .count(),
                1,
                "one formal path is one immutable summary value",
            );
            assert_eq!(
                inputs.len(),
                1,
                "one formal path allocates one occurrence-local projection equation",
            );
        }

        let artifact = program.solve().unwrap();
        let Type::Object(result) = &artifact.definitions[1].result.ty else {
            panic!("caller result must be a record")
        };
        assert_eq!(result.fields["left"], Type::Number);
        assert_eq!(result.fields["right"], Type::Number);
    }

    #[test]
    fn structural_result_summary_invokes_large_shared_nested_bytecode() {
        let field_count = SHARED_SUMMARY_MIN_NODES - 1;
        let mut callee_nodes = (0..field_count)
            .map(|_| KernelOwnerNode {
                kind: KernelOwnerNodeKind::Number,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            })
            .collect::<Vec<_>>();
        callee_nodes.push(KernelOwnerNode {
            kind: KernelOwnerNodeKind::Record { tag: None },
            inputs: (0..field_count)
                .map(|index| {
                    edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: format!("value_{index}").into(),
                            spread: false,
                        },
                        u32::try_from(index).expect("test field index exceeds u32"),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            mode: FlowMode::Continuous,
        });
        let callee = KernelOwnerProgramInput {
            nodes: callee_nodes.into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(field_count as u32),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::UserCall {
                    target: KernelOwnerId(0),
                    inherited_formal: None,
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::UserCall {
                    target: KernelOwnerId(1),
                    inherited_formal: None,
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, wrapper, caller].into_boxed_slice(),
        })
        .unwrap();
        assert!(program.compile_work().summary_invoke_nodes >= 1);

        let artifact = program.solve().unwrap();
        let Type::Object(result) = &artifact.definitions[2].result.ty else {
            panic!("caller result must be the shared record")
        };
        assert_eq!(result.fields.len(), field_count);
        assert!(result.fields.values().all(|field| *field == Type::Number));
    }

    #[test]
    fn structural_result_summary_composes_nested_formal_projections() {
        let projector = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: vec!["value".into()].into_boxed_slice(),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "value".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![projector, wrapper, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(
            program.compile_work().invocation_frames,
            0,
            "a nested projection of a forwarded formal must stay in summary bytecode",
        );
        let summary_inputs = program
            .component()
            .operations
            .iter()
            .find_map(|operation| match operation.as_ref() {
                crate::KernelOperation::SummaryCall { inputs, .. }
                    if inputs.iter().any(|input| {
                        matches!(
                            input,
                            KernelSummaryCallInput::Projection { steps, .. }
                                if steps.iter().any(|step| step.field.is_some())
                        )
                    }) =>
                {
                    Some(inputs)
                }
                _ => None,
            })
            .expect("the wrapper call must use a composed formal projection input");
        assert!(summary_inputs.len() >= 2);
        assert_eq!(
            program.solve().unwrap().definitions[2].result.ty,
            Type::Number
        );
    }

    #[test]
    fn structural_result_summary_projects_a_computed_value() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "value".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::DerivedRead {
                        fields: vec!["value".into()].into_boxed_slice(),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().invocation_frames, 0);
        assert!(program.component().operations.iter().any(|operation| {
            matches!(
                operation.as_ref(),
                crate::KernelOperation::SummaryCall { program, .. }
                    if program.nodes.iter().any(|node| {
                        matches!(node, KernelSummaryNode::Projection { .. })
                    })
            )
        }));
        assert_eq!(
            program.solve().unwrap().definitions[1].result.ty,
            Type::Number
        );
    }

    #[test]
    fn formal_independent_calls_share_the_compiled_principal_residual() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let mut caller_nodes = vec![KernelOwnerNode {
            kind: KernelOwnerNodeKind::Text,
            inputs: Box::new([]),
            mode: FlowMode::Continuous,
        }];
        let mut items = Vec::new();
        for _ in 0..32 {
            let call = u32::try_from(caller_nodes.len()).unwrap();
            caller_nodes.push(KernelOwnerNode {
                kind: KernelOwnerNodeKind::UserCall {
                    target: KernelOwnerId(0),
                    inherited_formal: None,
                },
                inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                    .into_boxed_slice(),
                mode: FlowMode::Continuous,
            });
            items.push(edge(KernelOwnerEdgeRole::CollectionItem, call));
        }
        let result = u32::try_from(caller_nodes.len()).unwrap();
        caller_nodes.push(KernelOwnerNode {
            kind: KernelOwnerNodeKind::Collection {
                kind: KernelCollectionKind::List,
                capacity: None,
            },
            inputs: items.into_boxed_slice(),
            mode: FlowMode::Continuous,
        });
        let caller = KernelOwnerProgramInput {
            nodes: caller_nodes.into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(result),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert!(
            program.component().operation_count() < 80,
            "formal-independent calls must not clone their callee: {} operations",
            program.component().operation_count()
        );
        let artifact = program.solve().unwrap();
        assert_eq!(
            artifact.definitions[1].result.ty,
            Type::List(Type::shared(Type::Number))
        );
    }

    #[test]
    fn formal_independent_subexpressions_share_principal_cells_across_calls() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "value".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "constant".into(),
                                spread: false,
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "number".into(),
                                spread: false,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "text".into(),
                                spread: false,
                            },
                            3,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().direct_result_summaries, 2);
        assert_eq!(program.compile_work().invocation_frames, 0);
        assert!(
            program.component().scheduled_work_item_count() < program.component().operation_count(),
            "acyclic residual instructions must execute through compact frame work items: {} scheduled for {} instructions",
            program.component().scheduled_work_item_count(),
            program.component().operation_count(),
        );
        let artifact = program.solve().unwrap();
        let Type::Object(result) = &artifact.definitions[1].result.ty else {
            panic!("caller result must be a record")
        };
        let Type::Object(number) = &result.fields["number"] else {
            panic!("number call result must be a record")
        };
        let Type::Object(text) = &result.fields["text"] else {
            panic!("text call result must be a record")
        };
        assert_eq!(number.fields["value"], Type::Number);
        assert_eq!(text.fields["value"], Type::Text);
        assert_eq!(number.fields["constant"], Type::Number);
        assert_eq!(text.fields["constant"], Type::Number);
    }

    #[test]
    fn static_call_selectors_slice_unreachable_residual_arms() {
        let mut callee_nodes = vec![KernelOwnerNode {
            kind: KernelOwnerNodeKind::FormalRead {
                formal: 0,
                fields: Box::new([]),
            },
            inputs: Box::new([]),
            mode: FlowMode::Continuous,
        }];
        let mut arms = Vec::new();
        for ordinal in 0..8 {
            let output = u32::try_from(callee_nodes.len()).unwrap();
            callee_nodes.push(KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 1,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            });
            let arm = u32::try_from(callee_nodes.len()).unwrap();
            callee_nodes.push(KernelOwnerNode {
                kind: KernelOwnerNodeKind::MatchArm {
                    pattern: KernelPattern::Tag {
                        name: format!("Arm{ordinal}").into_boxed_str(),
                        fields: Box::new([]),
                    },
                },
                inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, output)].into_boxed_slice(),
                mode: FlowMode::Continuous,
            });
            arms.push(edge(KernelOwnerEdgeRole::WhenArm, arm));
        }
        let result = u32::try_from(callee_nodes.len()).unwrap();
        callee_nodes.push(KernelOwnerNode {
            kind: KernelOwnerNodeKind::When,
            inputs: std::iter::once(edge(KernelOwnerEdgeRole::WhenInput, 0))
                .chain(arms)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            mode: FlowMode::Continuous,
        });
        let callee = KernelOwnerProgramInput {
            nodes: callee_nodes.into_boxed_slice(),
            formal_count: 2,
            external_expressions: Box::new([]),
            result: KernelExpressionId(result),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Arm3".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0),
                        edge(KernelOwnerEdgeRole::CallArgument { ordinal: 1 }, 1),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert!(
            program.component().operation_count() < 40,
            "one static call must not clone all eight residual arms: {} operations",
            program.component().operation_count()
        );
        assert_eq!(
            program.compile_work().invocation_frames,
            0,
            "the definition-owned SELECT summary replaces the complete occurrence frame",
        );
        assert!(program.component().operations.iter().any(|operation| {
            matches!(
                operation.as_ref(),
                crate::KernelOperation::SummaryCall { program, .. }
                    if program.nodes.iter().any(|node| matches!(node, KernelSummaryNode::Select { .. }))
            )
        }));
        let artifact = program.solve().unwrap();
        assert_eq!(artifact.definitions[1].result.ty, Type::Text);
    }

    #[test]
    fn project_program_propagates_a_child_owner_expression_without_reconstruction() {
        let input = KernelProjectProgramInput {
            owners: vec![
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::Tag("Ready".into()),
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(0),
                },
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::Record { tag: None },
                        inputs: vec![edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "status".into(),
                                spread: false,
                            },
                            1,
                        )]
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
                },
            ]
            .into_boxed_slice(),
        };

        let artifact = compile_project_program(&input).unwrap().solve().unwrap();
        assert_eq!(artifact.definitions.len(), 2);
        assert_eq!(
            artifact.definitions[1].result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "status".to_owned(),
                    Type::VariantSet(vec![Variant::Tag("Ready".to_owned())].into()),
                )],
                false,
            ))
        );
        assert!(artifact.work.activations < 16);
    }

    #[test]
    fn stateful_calls_use_fresh_occurrences_without_losing_the_state_domain() {
        let stateful = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Closed".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Open".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 0),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 1),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "state".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(3),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "first".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "second".into(),
                                spread: false,
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![stateful, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().invocation_frames, 2);
        assert_eq!(program.compile_work().reused_invocation_frames, 0);
        let artifact = program.solve().unwrap();
        let definition = &artifact.definitions[0].result.ty;
        let first_call = &artifact.definitions[1].expressions[0].flow_type.ty;
        let second_call = &artifact.definitions[1].expressions[1].flow_type.ty;
        let state_type = |ty: &Type| {
            let Type::Object(shape) = ty else {
                panic!("stateful call result is not an object: {ty:?}");
            };
            shape
                .fields
                .get("state")
                .cloned()
                .expect("stateful call result has a state field")
        };
        let definition_state = state_type(definition);
        let first_state = state_type(first_call);
        let second_state = state_type(second_call);
        assert_eq!(
            definition_state,
            Type::VariantSet(
                vec![
                    Variant::Tag("Closed".to_owned()),
                    Variant::Tag("Open".to_owned()),
                ]
                .into(),
            )
        );
        assert_eq!(first_state, definition_state);
        assert_eq!(second_state, first_state);
    }

    #[test]
    fn singleton_syntax_selection_exposes_only_the_nested_state_initializer() {
        let stateful = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Closed".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Open".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 0),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 1),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "state".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(3),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "UseState".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Fallback".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Wildcard,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 0),
                        edge(KernelOwnerEdgeRole::WhenArm, 2),
                        edge(KernelOwnerEdgeRole::WhenArm, 4),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(5),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("UseState".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "selected".into(),
                                spread: false,
                            },
                            1,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "direct".into(),
                                spread: false,
                            },
                            2,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(3),
        };

        let wrapper_dependencies = owner_expressions_depend_on_formals(&wrapper);
        let wrapper_variants =
            infer_static_variants(&wrapper, &[Some(BTreeSet::from(["UseState".into()]))]);
        assert!(
            syntax_selected_call_nodes(&wrapper, &wrapper_variants, &wrapper_dependencies)[1],
            "the nested stateful call must retain syntax-selection provenance",
        );

        let artifact = compile_project_program(&KernelProjectProgramInput {
            owners: vec![stateful, wrapper, caller].into_boxed_slice(),
        })
        .unwrap()
        .solve()
        .unwrap();
        let state_type = |ty: &Type| {
            let Type::Object(shape) = ty else {
                panic!("stateful result is not an object: {ty:?}");
            };
            shape.fields.get("state").cloned().expect("state field")
        };
        let full = Type::VariantSet(
            vec![
                Variant::Tag("Closed".to_owned()),
                Variant::Tag("Open".to_owned()),
            ]
            .into(),
        );
        assert_eq!(state_type(&artifact.definitions[0].result.ty), full);
        assert_eq!(
            state_type(&artifact.definitions[1].expressions[1].flow_type.ty),
            full
        );
        assert_eq!(
            state_type(&artifact.definitions[2].expressions[1].flow_type.ty),
            Type::VariantSet(vec![Variant::Tag("Closed".to_owned())].into()),
        );
        assert_eq!(
            state_type(&artifact.definitions[2].expressions[2].flow_type.ty),
            full
        );
    }

    #[test]
    fn project_result_alias_replays_after_a_cyclic_child_reaches_quiescence() {
        let child = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Initial".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Updated".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "Initial".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 5),
                        edge(KernelOwnerEdgeRole::WhenArm, 2),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 0),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 3),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: vec![KernelExternalExpression {
                owner: KernelOwnerId(0),
                target: KernelExternalTarget::Result,
            }]
            .into_boxed_slice(),
            result: KernelExpressionId(4),
        };
        let parent = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Block,
                inputs: vec![edge(KernelOwnerEdgeRole::BlockResult, 1)].into_boxed_slice(),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: vec![KernelExternalExpression {
                owner: KernelOwnerId(1),
                target: KernelExternalTarget::Result,
            }]
            .into_boxed_slice(),
            result: KernelExpressionId(0),
        };

        let artifact = compile_project_program(&KernelProjectProgramInput {
            owners: vec![parent, child].into_boxed_slice(),
        })
        .unwrap()
        .solve()
        .unwrap();
        let expected = Type::VariantSet(
            vec![
                Variant::Tag("Initial".to_owned()),
                Variant::Tag("Updated".to_owned()),
            ]
            .into(),
        );
        assert_eq!(artifact.definitions[1].result.ty, expected);
        assert_eq!(
            artifact.definitions[0].result.ty, expected,
            "a cross-owner result alias must observe the child's final epoch"
        );
    }

    #[test]
    fn public_reads_keep_the_declaration_mode_but_calls_project_the_actual_mode() {
        let input = KernelProjectProgramInput {
            owners: vec![
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::FormalRead {
                            formal: 0,
                            fields: Box::new([]),
                        },
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 1,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(0),
                },
                KernelOwnerProgramInput {
                    nodes: vec![
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Known(Type::Text),
                            inputs: Box::new([]),
                            mode: FlowMode::PresentOrAbsent,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Record { tag: None },
                            inputs: vec![edge(
                                KernelOwnerEdgeRole::RecordField {
                                    name: "event".into(),
                                    spread: false,
                                },
                                0,
                            )]
                            .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                    ]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(1),
                },
                KernelOwnerProgramInput {
                    nodes: vec![
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::ValueRead {
                                fields: vec!["event".into()].into_boxed_slice(),
                                mode_narrowing: None,
                            },
                            inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 2)]
                                .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::UserCall {
                                target: KernelOwnerId(0),
                                inherited_formal: None,
                            },
                            inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                                .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                    ]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: vec![KernelExternalExpression {
                        owner: KernelOwnerId(1),
                        target: KernelExternalTarget::Result,
                    }]
                    .into_boxed_slice(),
                    result: KernelExpressionId(1),
                },
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::ValueRead {
                            fields: vec!["event".into()].into_boxed_slice(),
                            mode_narrowing: None,
                        },
                        inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 1)].into_boxed_slice(),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: vec![KernelExternalExpression {
                        owner: KernelOwnerId(1),
                        target: KernelExternalTarget::Expression(KernelExpressionId(1)),
                    }]
                    .into_boxed_slice(),
                    result: KernelExpressionId(0),
                },
            ]
            .into_boxed_slice(),
        };

        let artifact = compile_project_program(&input).unwrap().solve().unwrap();
        assert_eq!(artifact.definitions[1].result.mode, FlowMode::Continuous);
        assert_eq!(
            artifact.definitions[2].expressions[0].flow_type.ty,
            Type::Text
        );
        assert_eq!(
            artifact.definitions[2].expressions[0].flow_type.mode,
            FlowMode::Continuous,
            "an ordinary cross-owner read retains the public declaration mode"
        );
        assert_eq!(
            artifact.definitions[2].result.mode,
            FlowMode::PresentOrAbsent,
            "contextual call inference follows the eventful projected actual"
        );
        assert_eq!(
            artifact.definitions[3].result.mode,
            FlowMode::PresentOrAbsent,
            "an exact external expression boundary retains structural mode projection"
        );
    }
}
