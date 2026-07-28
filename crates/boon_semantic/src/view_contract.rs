//! Exact semantic retained-view roots, constructor arguments, and bindings.
//!
//! This boundary consumes C's retained output contracts and the reactive
//! capture table. It never rediscovers roots from backend types and never uses
//! diagnostic node, attribute, or path strings as binding identity.

use crate::{
    SemanticBindingId, SemanticCallId, SemanticCallableId, SemanticCallableKind, SemanticCaptureId,
    SemanticExecutionGraphV1, SemanticExprId, SemanticExpression, SemanticExpressionKind,
    SemanticLoweringContractV1, SemanticOutputContractId, SemanticOutputContractKindV1,
    SemanticReactiveGraphV1, SemanticReadId, SemanticResourceGraphV1, SemanticRowBinding,
    SemanticScopeId, SemanticSourceId, SemanticValueId, SemanticViewCaptureTargetV1,
};
use boon_contract::SourceBundleDigestV1;
use boon_typecheck::{CheckedCallableKind, DeclId, Type};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

pub const SEMANTIC_VIEW_BINDING_GRAPH_SCHEMA_V1: &str = "boon.semantic-view-binding-graph.v1";
const SEMANTIC_VIEW_BINDING_GRAPH_DIGEST_DOMAIN: &[u8] = b"boon.semantic-view-binding-graph.v1\0";

macro_rules! view_id {
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

view_id!(
    SemanticViewRootId,
    SemanticViewNodeId,
    SemanticViewArgumentId,
    SemanticViewBindingId,
);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticViewBindingGraphDigestV1([u8; 32]);

impl SemanticViewBindingGraphDigestV1 {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for SemanticViewBindingGraphDigestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewBindingGraphV1 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<SemanticViewRootV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<SemanticViewNodeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<SemanticViewArgumentV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SemanticViewBindingV1>,
    pub digest: SemanticViewBindingGraphDigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewRootV1 {
    pub id: SemanticViewRootId,
    pub output: SemanticOutputContractId,
    pub reactive_output_ordinal: usize,
    pub statement: crate::SemanticStatementId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub binding: SemanticBindingId,
    pub route_scope: SemanticScopeId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewNodeV1 {
    pub id: SemanticViewNodeId,
    pub root: SemanticViewRootId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub call: SemanticCallId,
    pub callable: SemanticCallableId,
    /// Presentation only. Constructor identity is `callable` + `call`.
    pub diagnostic_kind: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticViewArgumentKindV1 {
    RenderTree,
    BindingInput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewArgumentV1 {
    pub id: SemanticViewArgumentId,
    pub root: SemanticViewRootId,
    pub node: SemanticViewNodeId,
    pub call: SemanticCallId,
    pub callable: SemanticCallableId,
    pub formal: DeclId,
    pub ordinal: usize,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub kind: SemanticViewArgumentKindV1,
    /// Presentation only. Formal identity is `callable` + `formal` + `ordinal`.
    pub diagnostic_attribute: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticViewBindingTargetV1 {
    Data { read: SemanticReadId },
    Event { source: SemanticSourceId },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticViewBindingKindV1 {
    Data,
    Source,
    Target,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewBindingV1 {
    pub id: SemanticViewBindingId,
    pub root: SemanticViewRootId,
    pub node: SemanticViewNodeId,
    pub argument: SemanticViewArgumentId,
    pub capture: SemanticCaptureId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub target: SemanticViewBindingTargetV1,
    pub kind: SemanticViewBindingKindV1,
    /// Exact retained-view attribute selected at the semantic leaf.
    pub canonical_attribute: String,
    /// Projection applied after the exact semantic read captured by `target`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_projection: Vec<String>,
    pub route_scope: SemanticScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<SemanticRowBinding>,
    /// Presentation only; target selection is the typed `target` above.
    pub diagnostic_node: String,
    /// Presentation only; argument selection is the typed `argument` above.
    pub diagnostic_attribute: String,
    /// Presentation only; read/source identity is the typed `target` above.
    pub diagnostic_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticViewBindingError {
    message: String,
}

impl SemanticViewBindingError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SemanticViewBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for SemanticViewBindingError {}

pub fn build_semantic_view_binding_graph(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
) -> Result<SemanticViewBindingGraphV1, SemanticViewBindingError> {
    let mut graph = derive_semantic_view_binding_graph(execution, resources, reactive, lowering)?;
    graph.digest = semantic_view_binding_graph_digest(&graph)?;
    Ok(graph)
}

impl SemanticViewBindingGraphV1 {
    pub fn validate(
        &self,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        lowering: &SemanticLoweringContractV1,
    ) -> Result<(), SemanticViewBindingError> {
        if self.schema != SEMANTIC_VIEW_BINDING_GRAPH_SCHEMA_V1
            || self.source_bundle_digest_v1 != lowering.metadata.source_bundle_digest_v1
        {
            return Err(SemanticViewBindingError::new(
                "semantic view-binding schema or source digest differs",
            ));
        }
        if self.digest != semantic_view_binding_graph_digest(self)? {
            return Err(SemanticViewBindingError::new(
                "semantic view-binding digest does not match its canonical payload",
            ));
        }
        let expected = build_semantic_view_binding_graph(execution, resources, reactive, lowering)?;
        if self != &expected {
            return Err(SemanticViewBindingError::new(
                "semantic view-binding graph differs from exact semantic roots and captures",
            ));
        }
        Ok(())
    }
}

fn derive_semantic_view_binding_graph(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
) -> Result<SemanticViewBindingGraphV1, SemanticViewBindingError> {
    let mut roots = Vec::new();
    let mut nodes = Vec::new();
    let mut arguments = Vec::new();
    let mut bindings = Vec::new();

    for output in lowering.output_contracts.iter().filter(|output| {
        matches!(
            output.contract,
            SemanticOutputContractKindV1::RetainedVisualDocument
                | SemanticOutputContractKindV1::RetainedVisualScene
        )
    }) {
        let output_expression = require_expression(execution, output.expression)?;
        let reactive_matches = reactive
            .output_values
            .iter()
            .filter(|candidate| {
                candidate.checked_expression == output_expression.checked_expr_id
                    && candidate.expression == output.expression
                    && candidate.value == output.value
                    && candidate.statement == output.statement
            })
            .collect::<Vec<_>>();
        let [reactive_output] = reactive_matches.as_slice() else {
            return Err(SemanticViewBindingError::new(format!(
                "retained output {} resolves to {} exact reactive output roots",
                output.id,
                reactive_matches.len()
            )));
        };

        let root_id = SemanticViewRootId(roots.len());
        roots.push(SemanticViewRootV1 {
            id: root_id,
            output: output.id,
            reactive_output_ordinal: reactive_output.ordinal,
            statement: output.statement,
            expression: output.expression,
            value: output.value,
            binding: output.binding,
            route_scope: reactive_output.route_scope,
        });

        let mut reachable = reachable_expressions(execution, output.expression)?;
        let record_node = reachable.iter().find_map(|expression| {
            let expression = require_expression(execution, *expression).ok()?;
            record_style_render_node(expression).then_some(expression.id)
        });
        if let Some(expression) = record_node {
            return Err(SemanticViewBindingError::new(format!(
                "retained output {} contains unsupported record-style retained node {expression}; V1 requires exact registered constructor calls",
                output.id
            )));
        }
        if !output.typed_contract_known {
            return Err(SemanticViewBindingError::new(format!(
                "retained output {} has no exact typed constructor contract",
                output.id
            )));
        }

        let mut constructor_expressions = reachable
            .iter()
            .copied()
            .filter_map(|expression| {
                let definition = require_expression(execution, expression).ok()?;
                let SemanticExpressionKind::Call { function, .. } = &definition.kind else {
                    return None;
                };
                exact_view_constructor(output.contract, function).then_some(expression)
            })
            .collect::<Vec<_>>();
        constructor_expressions.sort();
        if constructor_expressions.is_empty() {
            return Err(SemanticViewBindingError::new(format!(
                "retained output {} has no exact registered constructor node",
                output.id
            )));
        }

        for constructor_expression in constructor_expressions {
            let expression = require_expression(execution, constructor_expression)?;
            let SemanticExpressionKind::Call {
                call,
                callable,
                callable_kind,
                name,
                function,
                arguments: call_arguments,
                parameter_bindings,
                ..
            } = &expression.kind
            else {
                unreachable!("constructor expressions are exact calls")
            };
            validate_constructor_call(
                execution,
                ViewConstructorCallIdentity {
                    expression,
                    call: *call,
                    callable: *callable,
                    callable_kind: *callable_kind,
                    diagnostic_name: name,
                    function,
                    parameter_bindings,
                },
            )?;
            let node_id = SemanticViewNodeId(nodes.len());
            nodes.push(SemanticViewNodeV1 {
                id: node_id,
                root: root_id,
                expression: expression.id,
                value: expression.value_id,
                call: *call,
                callable: *callable,
                diagnostic_kind: function.clone(),
            });

            let callable_definition = require_callable(execution, *callable)?;
            let mut explicit_arguments = call_arguments.iter().collect::<Vec<_>>();
            explicit_arguments.sort_by_key(|argument| argument.ordinal);
            for call_argument in explicit_arguments {
                let parameter_matches = callable_definition
                    .parameters
                    .iter()
                    .filter(|parameter| {
                        parameter.formal == call_argument.formal
                            && parameter.ordinal == call_argument.ordinal
                            && parameter.id.callable == *callable
                    })
                    .collect::<Vec<_>>();
                let [parameter] = parameter_matches.as_slice() else {
                    return Err(SemanticViewBindingError::new(format!(
                        "view constructor node {node_id} argument {} resolves to {} exact callable formals",
                        call_argument.ordinal,
                        parameter_matches.len()
                    )));
                };
                if parameter.name != call_argument.name {
                    return Err(SemanticViewBindingError::new(format!(
                        "view constructor node {node_id} argument {} diagnostic name differs from exact formal",
                        call_argument.ordinal
                    )));
                }
                let kind = if render_tree_formal(&parameter.flow_type.ty) {
                    SemanticViewArgumentKindV1::RenderTree
                } else {
                    SemanticViewArgumentKindV1::BindingInput
                };
                let argument_expression = require_expression(execution, call_argument.value)?;
                let argument_id = SemanticViewArgumentId(arguments.len());
                arguments.push(SemanticViewArgumentV1 {
                    id: argument_id,
                    root: root_id,
                    node: node_id,
                    call: *call,
                    callable: *callable,
                    formal: call_argument.formal,
                    ordinal: call_argument.ordinal,
                    expression: call_argument.value,
                    value: argument_expression.value_id,
                    kind,
                    diagnostic_attribute: parameter.name.clone(),
                });
                if kind == SemanticViewArgumentKindV1::RenderTree {
                    continue;
                }

                let argument_reachable =
                    binding_input_expressions(execution, call_argument.value, output.contract)?;
                let mut captures = reactive
                    .view_captures
                    .iter()
                    .filter(|capture| {
                        capture.output_ordinal == reactive_output.ordinal
                            && argument_reachable.contains(&capture.expression)
                    })
                    .collect::<Vec<_>>();
                captures.sort_by_key(|capture| capture.id);
                for capture in captures {
                    let (target, diagnostic_path) = match capture.target {
                        SemanticViewCaptureTargetV1::Read { read } => {
                            let read_definition = reactive
                                .reads
                                .get(read.as_usize())
                                .filter(|candidate| candidate.id == read)
                                .ok_or_else(|| {
                                    SemanticViewBindingError::new(format!(
                                        "view capture {} references missing semantic read {read}",
                                        capture.id
                                    ))
                                })?;
                            if read_definition.expression != capture.expression
                                || read_definition.value != capture.value
                            {
                                return Err(SemanticViewBindingError::new(format!(
                                    "view capture {} differs from exact semantic read {read}",
                                    capture.id
                                )));
                            }
                            (
                                SemanticViewBindingTargetV1::Data { read },
                                format!("read:{read}"),
                            )
                        }
                        SemanticViewCaptureTargetV1::Source { source } => {
                            let source_definition = resources
                                .sources
                                .get(source.as_usize())
                                .filter(|candidate| candidate.id == source)
                                .ok_or_else(|| {
                                    SemanticViewBindingError::new(format!(
                                        "view capture {} references missing semantic source {source}",
                                        capture.id
                                    ))
                                })?;
                            (
                                SemanticViewBindingTargetV1::Event { source },
                                source_definition.semantic_path.clone(),
                            )
                        }
                        SemanticViewCaptureTargetV1::Field { .. } => continue,
                    };
                    let capture_expression = require_expression(execution, capture.expression)?;
                    if capture_expression.value_id != capture.value {
                        return Err(SemanticViewBindingError::new(format!(
                            "view capture {} expression/value identity differs",
                            capture.id
                        )));
                    }
                    let route_scope = expression_route_scope(execution, capture.expression)?;
                    let row = capture
                        .row_scope
                        .map(|scope| {
                            resources
                                .row_scopes
                                .get(scope.as_usize())
                                .filter(|candidate| candidate.id == scope)
                                .map(|row| SemanticRowBinding {
                                    list: row.list,
                                    scope,
                                })
                                .ok_or_else(|| {
                                    SemanticViewBindingError::new(format!(
                                        "view capture {} references missing row scope {scope}",
                                        capture.id
                                    ))
                                })
                        })
                        .transpose()?;
                    let leaf_bindings = binding_leaf_metadata(
                        execution,
                        call_argument.value,
                        capture.expression,
                        capture.target,
                        &parameter.name,
                        &diagnostic_path,
                        output.contract,
                    )?;
                    if leaf_bindings.is_empty() {
                        return Err(SemanticViewBindingError::new(format!(
                            "view capture {} has no exact retained-view leaf under argument {}",
                            capture.id, parameter.name
                        )));
                    }
                    for leaf in leaf_bindings {
                        bindings.push(SemanticViewBindingV1 {
                            id: SemanticViewBindingId(bindings.len()),
                            root: root_id,
                            node: node_id,
                            argument: argument_id,
                            capture: capture.id,
                            expression: capture.expression,
                            value: capture.value,
                            target,
                            kind: leaf.kind,
                            canonical_attribute: leaf.canonical_attribute,
                            additional_projection: leaf.additional_projection,
                            route_scope,
                            row,
                            diagnostic_node: function.clone(),
                            diagnostic_attribute: parameter.name.clone(),
                            diagnostic_path: diagnostic_path.clone(),
                        });
                    }
                }
            }
        }
        reachable.clear();
    }

    Ok(SemanticViewBindingGraphV1 {
        schema: SEMANTIC_VIEW_BINDING_GRAPH_SCHEMA_V1.to_owned(),
        source_bundle_digest_v1: lowering.metadata.source_bundle_digest_v1,
        roots,
        nodes,
        arguments,
        bindings,
        digest: SemanticViewBindingGraphDigestV1([0; 32]),
    })
}

struct ViewConstructorCallIdentity<'a> {
    expression: &'a SemanticExpression,
    call: SemanticCallId,
    callable: SemanticCallableId,
    callable_kind: SemanticCallableKind,
    diagnostic_name: &'a str,
    function: &'a str,
    parameter_bindings: &'a [crate::SemanticCallParameterBinding],
}

fn validate_constructor_call(
    execution: &SemanticExecutionGraphV1,
    identity: ViewConstructorCallIdentity<'_>,
) -> Result<(), SemanticViewBindingError> {
    let ViewConstructorCallIdentity {
        expression,
        call,
        callable,
        callable_kind,
        diagnostic_name,
        function,
        parameter_bindings,
    } = identity;
    let callable_definition = require_callable(execution, callable)?;
    let call_definition = execution
        .calls
        .get(call.as_usize())
        .filter(|candidate| candidate.id == call)
        .ok_or_else(|| {
            SemanticViewBindingError::new(format!(
                "view constructor expression {} references missing semantic call {call}",
                expression.id
            ))
        })?;
    let value_parameters = callable_definition
        .parameters
        .iter()
        .filter(|parameter| parameter.kind == boon_typecheck::CheckedParameterKind::Value)
        .collect::<Vec<_>>();
    if callable_kind != SemanticCallableKind::Builtin
        || callable_definition.kind != CheckedCallableKind::Builtin
        || call_definition.callable != callable
        || call_definition.checked_expression != expression.checked_expr_id
        || call_definition.function != function
        || callable_definition.name != diagnostic_name
        || parameter_bindings.len() != value_parameters.len()
        || parameter_bindings
            .iter()
            .zip(value_parameters)
            .any(|(binding, parameter)| {
                binding.formal != parameter.formal
                    || binding.ordinal != parameter.ordinal
                    || binding.name != parameter.name
            })
    {
        return Err(SemanticViewBindingError::new(format!(
            "view constructor expression {} lacks exact builtin callable/formal identity",
            expression.id
        )));
    }
    Ok(())
}

fn reachable_expressions(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
) -> Result<BTreeSet<SemanticExprId>, SemanticViewBindingError> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        if !reachable.insert(expression) {
            continue;
        }
        let expression = require_expression(execution, expression)?;
        pending.extend(expression_children(execution, &expression.kind)?);
    }
    Ok(reachable)
}

fn binding_input_expressions(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
    contract: SemanticOutputContractKindV1,
) -> Result<BTreeSet<SemanticExprId>, SemanticViewBindingError> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        if !reachable.insert(expression) {
            continue;
        }
        let expression = require_expression(execution, expression)?;
        if expression.id != root
            && matches!(
                &expression.kind,
                SemanticExpressionKind::Call { function, .. }
                    if exact_view_constructor(contract, function)
            )
        {
            continue;
        }
        pending.extend(expression_children(execution, &expression.kind)?);
    }
    Ok(reachable)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SemanticViewBindingLeaf {
    kind: SemanticViewBindingKindV1,
    canonical_attribute: String,
    additional_projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BindingLeafMode {
    Data {
        attribute: String,
        kind: SemanticViewBindingKindV1,
    },
    Element,
    Style {
        attribute: String,
    },
    Events {
        attribute: Option<String>,
    },
}

struct BindingLeafTraversal<'a> {
    execution: &'a SemanticExecutionGraphV1,
    capture: SemanticExprId,
    capture_target: SemanticViewCaptureTargetV1,
    source_fallback_attribute: &'a str,
    contract: SemanticOutputContractKindV1,
    result: BTreeSet<SemanticViewBindingLeaf>,
    visiting: BTreeSet<SemanticExprId>,
}

fn binding_leaf_metadata(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
    capture: SemanticExprId,
    capture_target: SemanticViewCaptureTargetV1,
    argument: &str,
    diagnostic_path: &str,
    contract: SemanticOutputContractKindV1,
) -> Result<Vec<SemanticViewBindingLeaf>, SemanticViewBindingError> {
    let mode = match argument {
        "element" => BindingLeafMode::Element,
        "style" => BindingLeafMode::Style {
            attribute: argument.to_owned(),
        },
        "target" => BindingLeafMode::Data {
            attribute: argument.to_owned(),
            kind: SemanticViewBindingKindV1::Target,
        },
        _ => BindingLeafMode::Data {
            attribute: argument.to_owned(),
            kind: SemanticViewBindingKindV1::Data,
        },
    };
    let source_fallback_attribute = diagnostic_path
        .rsplit('.')
        .next()
        .filter(|attribute| !attribute.is_empty())
        .unwrap_or("event");
    let mut traversal = BindingLeafTraversal {
        execution,
        capture,
        capture_target,
        source_fallback_attribute,
        contract,
        result: BTreeSet::new(),
        visiting: BTreeSet::new(),
    };
    traversal.visit(root, mode, Vec::new(), true)?;
    Ok(traversal.result.into_iter().collect())
}

impl BindingLeafTraversal<'_> {
    fn visit(
        &mut self,
        id: SemanticExprId,
        mode: BindingLeafMode,
        additional_projection: Vec<String>,
        is_root: bool,
    ) -> Result<(), SemanticViewBindingError> {
        if !self.visiting.insert(id) {
            return Ok(());
        }
        let expression = require_expression(self.execution, id)?;
        if !is_root
            && matches!(
                &expression.kind,
                SemanticExpressionKind::Call { function, .. }
                    if exact_view_constructor(self.contract, function)
            )
        {
            self.visiting.remove(&id);
            return Ok(());
        }
        if let SemanticExpressionKind::Project { input, fields } = &expression.kind {
            let mut projection = fields.clone();
            projection.extend(additional_projection);
            self.visit(*input, mode, projection, false)?;
            self.visiting.remove(&id);
            return Ok(());
        }
        if let SemanticExpressionKind::Call {
            function,
            arguments,
            ..
        } = &expression.kind
            && let Some(field_path) = function.strip_prefix("Field/")
        {
            let inputs = arguments
                .iter()
                .filter(|argument| argument.name == "input")
                .collect::<Vec<_>>();
            let [input] = inputs.as_slice() else {
                return Err(SemanticViewBindingError::new(format!(
                    "semantic field projection {id} resolves to {} exact input arguments",
                    inputs.len()
                )));
            };
            let mut projection = field_path
                .split('/')
                .filter(|field| !field.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if projection.is_empty() {
                return Err(SemanticViewBindingError::new(format!(
                    "semantic field projection {id} has no canonical fields"
                )));
            }
            projection.extend(additional_projection);
            self.visit(input.value, mode, projection, false)?;
            self.visiting.remove(&id);
            return Ok(());
        }
        if id == self.capture {
            let (attribute, read_kind) = match &mode {
                BindingLeafMode::Data { attribute, kind } => (attribute.as_str(), *kind),
                BindingLeafMode::Element => ("element", SemanticViewBindingKindV1::Data),
                BindingLeafMode::Style { attribute } => {
                    (attribute.as_str(), SemanticViewBindingKindV1::Data)
                }
                BindingLeafMode::Events { attribute } => (
                    attribute
                        .as_deref()
                        .unwrap_or(self.source_fallback_attribute),
                    SemanticViewBindingKindV1::Data,
                ),
            };
            let (kind, canonical_attribute) = match self.capture_target {
                SemanticViewCaptureTargetV1::Source { .. } => (
                    SemanticViewBindingKindV1::Source,
                    canonical_event_attribute(attribute).to_owned(),
                ),
                SemanticViewCaptureTargetV1::Read { .. } => (read_kind, attribute.to_owned()),
                SemanticViewCaptureTargetV1::Field { .. } => {
                    self.visiting.remove(&id);
                    return Ok(());
                }
            };
            self.result.insert(SemanticViewBindingLeaf {
                kind,
                canonical_attribute,
                additional_projection,
            });
            self.visiting.remove(&id);
            return Ok(());
        }

        match (&mode, &expression.kind) {
            (
                BindingLeafMode::Element,
                SemanticExpressionKind::Object(fields)
                | SemanticExpressionKind::TaggedObject { fields, .. },
            ) => {
                for field in fields {
                    let child_mode = if field.spread {
                        Some(BindingLeafMode::Element)
                    } else if field.name == "events" {
                        Some(BindingLeafMode::Events { attribute: None })
                    } else if field.name == "target" {
                        Some(BindingLeafMode::Data {
                            attribute: field.name.clone(),
                            kind: SemanticViewBindingKindV1::Target,
                        })
                    } else {
                        None
                    };
                    if let Some(child_mode) = child_mode {
                        self.visit(
                            field.value,
                            child_mode,
                            additional_projection.clone(),
                            false,
                        )?;
                    }
                }
            }
            (
                BindingLeafMode::Style { .. },
                SemanticExpressionKind::Object(fields)
                | SemanticExpressionKind::TaggedObject { fields, .. },
            ) => {
                for field in fields {
                    let child_mode = if field.spread {
                        mode.clone()
                    } else {
                        BindingLeafMode::Style {
                            attribute: field.name.clone(),
                        }
                    };
                    self.visit(
                        field.value,
                        child_mode,
                        additional_projection.clone(),
                        false,
                    )?;
                }
            }
            (
                BindingLeafMode::Events { attribute },
                SemanticExpressionKind::Object(fields)
                | SemanticExpressionKind::TaggedObject { fields, .. },
            ) => {
                for field in fields {
                    self.visit(
                        field.value,
                        BindingLeafMode::Events {
                            attribute: if field.spread {
                                attribute.clone()
                            } else {
                                Some(field.name.clone())
                            },
                        },
                        additional_projection.clone(),
                        false,
                    )?;
                }
            }
            _ => {
                for child in expression_children(self.execution, &expression.kind)? {
                    self.visit(child, mode.clone(), additional_projection.clone(), false)?;
                }
            }
        }
        self.visiting.remove(&id);
        Ok(())
    }
}

fn canonical_event_attribute(attribute: &str) -> &str {
    if attribute == "key_down" {
        "submit"
    } else {
        attribute
    }
}

fn expression_children(
    execution: &SemanticExecutionGraphV1,
    kind: &SemanticExpressionKind,
) -> Result<Vec<SemanticExprId>, SemanticViewBindingError> {
    Ok(match kind {
        SemanticExpressionKind::CanonicalRead { .. }
        | SemanticExpressionKind::LocalRead { .. }
        | SemanticExpressionKind::ExternalRead { .. }
        | SemanticExpressionKind::ElementState { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Absent
        | SemanticExpressionKind::Tag(_)
        | SemanticExpressionKind::Source { .. }
        | SemanticExpressionKind::Delimiter
        | SemanticExpressionKind::MaterializationLocal { .. }
        | SemanticExpressionKind::FunctionParameter { .. } => Vec::new(),
        SemanticExpressionKind::Materialize { materialization } => execution
            .materializations
            .get(materialization.as_usize())
            .filter(|candidate| candidate.id == *materialization)
            .ok_or_else(|| {
                SemanticViewBindingError::new(format!(
                    "view traversal references missing semantic materialization {materialization}"
                ))
            })?
            .expression_roots(),
        SemanticExpressionKind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                crate::SemanticTextSegment::Static { .. } => None,
                crate::SemanticTextSegment::Dynamic { value } => Some(*value),
            })
            .collect(),
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields) => {
            fields.iter().map(|field| field.value).collect()
        }
        SemanticExpressionKind::Call { arguments, .. } => {
            arguments.iter().map(|argument| argument.value).collect()
        }
        SemanticExpressionKind::Flush { payload: input }
        | SemanticExpressionKind::FlushBoundary { input }
        | SemanticExpressionKind::Draining { input }
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
        SemanticExpressionKind::Block { bindings, result } => bindings
            .iter()
            .map(|binding| binding.value)
            .chain(std::iter::once(*result))
            .collect(),
        SemanticExpressionKind::List { items, .. }
        | SemanticExpressionKind::Bytes { items, .. } => items.clone(),
    })
}

fn record_style_render_node(expression: &SemanticExpression) -> bool {
    if !matches!(
        expression.kind,
        SemanticExpressionKind::Object(_) | SemanticExpressionKind::TaggedObject { .. }
    ) {
        return false;
    }
    let Type::Object(shape) = &expression.flow_type.ty else {
        return false;
    };
    let Some(Type::VariantSet(variants)) = shape.fields.get("kind") else {
        return false;
    };
    variants.iter().any(|variant| {
        matches!(
            variant,
            boon_typecheck::Variant::Tag(tag) if matches!(
                tag.as_str(),
                "Document"
                    | "Scene"
                    | "Button"
                    | "Checkbox"
                    | "Row"
                    | "Stack"
                    | "Block"
                    | "Text"
                    | "TextInput"
                    | "Label"
                    | "Paragraph"
                    | "Link"
                    | "EmbeddedProgram"
                    | "EmbeddedMedia"
                    | "MapViewport"
            )
        )
    })
}

fn render_tree_formal(ty: &Type) -> bool {
    match ty {
        Type::RenderContract => true,
        Type::List(item) => render_tree_formal(item),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Object(_)
        | Type::VariantSet(_)
        | Type::Absent
        | Type::Function { .. }
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

fn exact_view_constructor(contract: SemanticOutputContractKindV1, function: &str) -> bool {
    const DOCUMENT: &[&str] = &[
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
    const SCENE: &[&str] = &[
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
        SemanticOutputContractKindV1::RetainedVisualDocument => DOCUMENT.contains(&function),
        SemanticOutputContractKindV1::RetainedVisualScene => SCENE.contains(&function),
        SemanticOutputContractKindV1::HostValue => false,
    }
}

fn expression_route_scope(
    execution: &SemanticExecutionGraphV1,
    expression: SemanticExprId,
) -> Result<SemanticScopeId, SemanticViewBindingError> {
    let origin = execution
        .checked_expression_origins
        .get(expression.as_usize())
        .filter(|candidate| candidate.expression == expression)
        .ok_or_else(|| {
            SemanticViewBindingError::new(format!(
                "view expression {expression} has no exact checked-origin identity"
            ))
        })?;
    let matches = execution
        .scopes
        .iter()
        .filter(|scope| scope.checked_scope == origin.checked_scope)
        .map(|scope| scope.id)
        .collect::<Vec<_>>();
    let [scope] = matches.as_slice() else {
        return Err(SemanticViewBindingError::new(format!(
            "view expression {expression} checked scope {} resolves to {} semantic scopes",
            origin.checked_scope.0,
            matches.len()
        )));
    };
    Ok(*scope)
}

fn require_expression(
    execution: &SemanticExecutionGraphV1,
    id: SemanticExprId,
) -> Result<&SemanticExpression, SemanticViewBindingError> {
    execution
        .expressions
        .get(id.as_usize())
        .filter(|candidate| candidate.id == id)
        .ok_or_else(|| {
            SemanticViewBindingError::new(format!(
                "semantic view-binding graph references missing expression {id}"
            ))
        })
}

fn require_callable(
    execution: &SemanticExecutionGraphV1,
    id: SemanticCallableId,
) -> Result<&crate::SemanticCallable, SemanticViewBindingError> {
    execution
        .callables
        .get(id.as_usize())
        .filter(|candidate| candidate.id == id)
        .ok_or_else(|| {
            SemanticViewBindingError::new(format!(
                "semantic view-binding graph references missing callable {id}"
            ))
        })
}

#[derive(Serialize)]
struct ViewBindingDigestPayload<'a> {
    schema: &'a str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    roots: &'a [SemanticViewRootV1],
    nodes: &'a [SemanticViewNodeV1],
    arguments: &'a [SemanticViewArgumentV1],
    bindings: &'a [SemanticViewBindingV1],
}

fn semantic_view_binding_graph_digest(
    graph: &SemanticViewBindingGraphV1,
) -> Result<SemanticViewBindingGraphDigestV1, SemanticViewBindingError> {
    boon_contract::canonical_serde_hash_v1(
        SEMANTIC_VIEW_BINDING_GRAPH_DIGEST_DOMAIN,
        &ViewBindingDigestPayload {
            schema: &graph.schema,
            source_bundle_digest_v1: graph.source_bundle_digest_v1,
            roots: &graph.roots,
            nodes: &graph.nodes,
            arguments: &graph.arguments,
            bindings: &graph.bindings,
        },
    )
    .map(SemanticViewBindingGraphDigestV1)
    .map_err(|error| {
        SemanticViewBindingError::new(format!(
            "failed to hash semantic view-binding graph: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(source: &str) -> boon_typecheck::CheckedProgram {
        let parsed = boon_parser::parse_source("view-contract.bn", source).expect("parse");
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "unexpected type diagnostics: {:#?}",
            output.report.diagnostics
        );
        output.program.expect("checked program")
    }

    fn document_fixture() -> crate::SemanticProgram {
        crate::elaborate(
            checked(
                r#"
store: [
    value: 1
]
document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: store.value
    )
)
"#,
            ),
            &[],
        )
        .expect("semantic document")
    }

    #[test]
    fn retained_root_constructor_formals_and_data_captures_are_exact() {
        let semantic = document_fixture();
        let graph = semantic.view_binding_graph();
        assert_eq!(graph.roots.len(), 1);
        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.arguments.iter().any(|argument| {
            argument.kind == SemanticViewArgumentKindV1::RenderTree
                && argument.diagnostic_attribute == "root"
        }));
        let data = graph
            .bindings
            .iter()
            .find(|binding| matches!(binding.target, SemanticViewBindingTargetV1::Data { .. }))
            .expect("label read is an exact data binding");
        let argument = &graph.arguments[data.argument.as_usize()];
        assert_eq!(data.kind, SemanticViewBindingKindV1::Data);
        assert_eq!(data.canonical_attribute, "label");
        assert!(data.additional_projection.is_empty());
        assert_eq!(
            argument.formal,
            semantic.execution_graph().callables[argument.callable.as_usize()].parameters
                [argument.ordinal]
                .formal
        );
        assert_eq!(
            data.capture,
            semantic.reactive_graph().view_captures[data.capture.as_usize()].id
        );
        graph
            .validate(
                semantic.execution_graph(),
                semantic.resource_graph(),
                semantic.reactive_graph(),
                semantic.lowering_contract(),
            )
            .expect("fresh view graph validates");
    }

    #[test]
    fn event_capture_targets_exact_semantic_source() {
        let semantic = crate::elaborate(
            checked(
                r#"
store: [
    clicked: SOURCE
]
document: Document/new(
    root: Element/button(
        element: [
            events: [
                click: store.clicked
            ]
        ]
        style: []
        label: TEXT { Click }
    )
)
"#,
            ),
            &[],
        )
        .expect("semantic event document");
        let graph = semantic.view_binding_graph();
        let event = graph
            .bindings
            .iter()
            .find_map(|binding| match binding.target {
                SemanticViewBindingTargetV1::Event { source } => Some((binding, source)),
                SemanticViewBindingTargetV1::Data { .. } => None,
            })
            .expect("button event is an exact source binding");
        assert_eq!(
            semantic.resource_graph().sources[event.1.as_usize()].id,
            event.1
        );
        assert_eq!(
            semantic.reactive_graph().view_captures[event.0.capture.as_usize()].target,
            SemanticViewCaptureTargetV1::Source { source: event.1 }
        );
        assert_eq!(event.0.kind, SemanticViewBindingKindV1::Source);
        assert_eq!(event.0.canonical_attribute, "click");
        assert!(event.0.additional_projection.is_empty());
    }

    #[test]
    fn nested_target_and_style_bindings_preserve_exact_leaf_kind_and_attribute() {
        let semantic = crate::elaborate(
            checked(
                r#"
store: [
    target: TEXT { target }
    size: 14
]
document: Document/new(
    root: Element/label(
        element: [
            target: store.target
        ]
        style: [
            font: [
                size: store.size
            ]
        ]
        label: TEXT { Label }
    )
)
"#,
            ),
            &[],
        )
        .expect("semantic target/style document");
        let graph = semantic.view_binding_graph();
        let target = graph
            .bindings
            .iter()
            .find(|binding| binding.canonical_attribute == "target")
            .expect("nested element target binding");
        assert_eq!(target.kind, SemanticViewBindingKindV1::Target);
        let size = graph
            .bindings
            .iter()
            .find(|binding| binding.canonical_attribute == "size")
            .expect("nested style leaf binding");
        assert_eq!(size.kind, SemanticViewBindingKindV1::Data);
    }

    #[test]
    fn projected_binding_preserves_projection_after_the_exact_semantic_read() {
        let semantic = crate::elaborate(
            checked(
                r#"
FUNCTION wrap(value) {
    [leaf: value]
}
store: [
    value: TEXT { projected }
]
document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: wrap(value: store.value).leaf
    )
)
"#,
            ),
            &[],
        )
        .expect("semantic projected document");
        let binding = semantic
            .view_binding_graph()
            .bindings
            .iter()
            .find(|binding| binding.canonical_attribute == "label")
            .expect("projected label binding");
        assert_eq!(binding.kind, SemanticViewBindingKindV1::Data);
        assert_eq!(binding.additional_projection, ["leaf"]);
    }

    #[test]
    fn validation_rejects_formal_target_scope_and_diagnostic_mutations() {
        let semantic = document_fixture();
        let fresh = semantic.view_binding_graph().clone();
        let rejects = |graph: &SemanticViewBindingGraphV1| {
            graph
                .validate(
                    semantic.execution_graph(),
                    semantic.resource_graph(),
                    semantic.reactive_graph(),
                    semantic.lowering_contract(),
                )
                .is_err()
        };

        let mut formal = fresh.clone();
        formal.arguments[0].formal = DeclId(usize::MAX as u32);
        formal.digest = semantic_view_binding_graph_digest(&formal).unwrap();
        assert!(rejects(&formal));

        let mut target = fresh.clone();
        let binding = target.bindings.first_mut().expect("data binding");
        binding.target = SemanticViewBindingTargetV1::Data {
            read: SemanticReadId(usize::MAX),
        };
        target.digest = semantic_view_binding_graph_digest(&target).unwrap();
        assert!(rejects(&target));

        let mut scope = fresh.clone();
        scope.bindings[0].route_scope = SemanticScopeId(usize::MAX);
        scope.digest = semantic_view_binding_graph_digest(&scope).unwrap();
        assert!(rejects(&scope));

        let mut diagnostic = fresh;
        diagnostic.bindings[0].diagnostic_path = "misleading.presentation.path".to_owned();
        diagnostic.digest = semantic_view_binding_graph_digest(&diagnostic).unwrap();
        assert!(rejects(&diagnostic));
    }

    #[test]
    fn record_style_retained_nodes_are_explicitly_rejected_in_v1() {
        let error = crate::elaborate(
            checked(
                r#"
document: [
    kind: Document
    root: [
        kind: Text
        text: TEXT { unsupported record node }
    ]
]
"#,
            ),
            &[],
        )
        .expect_err("record-style retained node must fail closed");
        assert!(
            error
                .to_string()
                .contains("unsupported record-style retained node"),
            "{error}"
        );
    }
}
