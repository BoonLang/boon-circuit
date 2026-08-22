#[cfg(debug_assertions)]
use crate::TypeTerm;
use crate::{
    FrozenTypeStore, KernelArtifactFlowTermV1, KernelOwnerId, KernelSolveError, TypeTermId,
    TypeVariableId, alpha_normalize_flow_type,
};
use boon_checked::{FlowMode, FlowType, Type, TypeVar};
use boon_contract::SymbolId;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Checked range into one immutable `DefinitionCodeStore` column.
///
/// Raw coordinates never cross the store boundary. Construction validates
/// every start, length, and end once; consumers receive only borrowed slices.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Span32 {
    start: u32,
    len: u32,
}

impl Span32 {
    fn from_bounds(start: usize, end: usize, label: &str) -> Result<Self, KernelSolveError> {
        let start = u32::try_from(start).map_err(|_| {
            KernelSolveError::new(format!(
                "kernel definition-code {label} column start exceeds u32"
            ))
        })?;
        let end = u32::try_from(end).map_err(|_| {
            KernelSolveError::new(format!(
                "kernel definition-code {label} column end exceeds u32"
            ))
        })?;
        Ok(Self {
            start,
            len: end.checked_sub(start).ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel definition-code {label} column is not monotonic"
                ))
            })?,
        })
    }

    fn append<T>(
        column: &mut Vec<T>,
        rows: impl IntoIterator<Item = T>,
    ) -> Result<Self, KernelSolveError> {
        let start = u32::try_from(column.len()).map_err(|_| {
            KernelSolveError::new("kernel definition-code column start exceeds u32")
        })?;
        column.extend(rows);
        let end = u32::try_from(column.len())
            .map_err(|_| KernelSolveError::new("kernel definition-code column end exceeds u32"))?;
        let len = end.checked_sub(start).ok_or_else(|| {
            KernelSolveError::new("kernel definition-code column range is not monotonic")
        })?;
        start
            .checked_add(len)
            .filter(|candidate| *candidate == end)
            .ok_or_else(|| {
                KernelSolveError::new("kernel definition-code column range overflows u32")
            })?;
        Ok(Self { start, len })
    }

    fn get<'a, T>(self, column: &'a [T]) -> Option<&'a [T]> {
        let start = self.start as usize;
        let end = start.checked_add(self.len as usize)?;
        column.get(start..end)
    }

    fn contains(self, index: usize) -> bool {
        let start = self.start as usize;
        start
            .checked_add(self.len as usize)
            .is_some_and(|end| index >= start && index < end)
    }
}

const MISSING_EXECUTION_ROW: u32 = u32::MAX;

/// One expression in a definition-local namespace.
///
/// The pair remains valid before checked-image linking and does not make a
/// reusable definition module depend on a project-global expression offset.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct PackedExpressionRef {
    owner: KernelOwnerId,
    expression: crate::KernelExpressionId,
}

impl PackedExpressionRef {
    pub(crate) const fn new(owner: KernelOwnerId, expression: crate::KernelExpressionId) -> Self {
        Self { owner, expression }
    }

    pub(crate) const fn owner(self) -> KernelOwnerId {
        self.owner
    }

    pub(crate) const fn expression(self) -> crate::KernelExpressionId {
        self.expression
    }
}

/// One call in a definition-local namespace.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct PackedCallRef {
    owner: KernelOwnerId,
    ordinal: u32,
}

impl PackedCallRef {
    const NONE: Self = Self {
        owner: KernelOwnerId(u32::MAX),
        ordinal: u32::MAX,
    };

    pub(crate) const fn new(owner: KernelOwnerId, ordinal: u32) -> Self {
        Self { owner, ordinal }
    }

    pub(crate) const fn owner(self) -> KernelOwnerId {
        self.owner
    }

    pub(crate) const fn ordinal(self) -> u32 {
        self.ordinal
    }

    const fn is_none(self) -> bool {
        self.ordinal == u32::MAX
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedExecutionSelector {
    input: PackedExpressionRef,
    arms: Span32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedExecutionNode {
    expression: PackedExpressionRef,
    dependencies: Span32,
    call: PackedCallRef,
    selector: u32,
    template_owner: KernelOwnerId,
}

/// One definition header in the permanent packed compiler output.
///
/// This first consumed slice centralizes all solver-owned flow roots and their
/// definition-local alpha namespace. Subsequent M1 cuts add the remaining row
/// families to the same store; they must not create another parallel sidecar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DefinitionCode {
    effect_summary: crate::KernelEffectSummary,
    basis_fingerprint_v14: [u8; 32],
    result: KernelArtifactFlowTermV1,
    formals: Span32,
    expressions: Span32,
    expression_flush_types: Span32,
    expression_kind_types: Span32,
    declaration_flows: Span32,
    calls: Span32,
    diagnostic_types: Span32,
    source_payload_types: Span32,
    states: Span32,
    list_item_types: Span32,
    resource_projection_requirements: Span32,
    alpha_variables: Span32,
    /// Dense reverse index for this definition's local expressions. Values
    /// address `DefinitionCodeStore::execution_nodes`; `u32::MAX` means the
    /// expression is not retained by any callable execution template.
    execution_node_by_expression: Span32,
    /// `u32::MAX` means this definition is not a callable execution template.
    execution_result: u32,
    execution_nodes: Span32,
    execution_calls: Span32,
    stable_digest: [u8; 32],
}

/// Immutable definition column authority shared by checked and semantic
/// consumers.
///
/// The rich `DefinitionArtifact` no longer owns a compact flow sidecar. This
/// store is the single retained authority for result/formal/expression roots;
/// the checked linker consumes it directly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionCodeStore {
    types: Arc<FrozenTypeStore>,
    definitions: Box<[DefinitionCode]>,
    flows: Box<[KernelArtifactFlowTermV1]>,
    expression_flush_types: Box<[Option<crate::TypeTermId>]>,
    expression_kind_types: Box<[Option<crate::TypeTermId>]>,
    declaration_flows: Box<[Option<PackedFlow>]>,
    calls: Box<[PackedCallFacts]>,
    call_substitutions: Box<[PackedCallTypeSubstitution]>,
    diagnostic_types: Box<[Option<PackedDiagnosticTypes>]>,
    source_payload_types: Box<[crate::TypeTermId]>,
    states: Box<[PackedPublishedState]>,
    list_item_types: Box<[crate::TypeTermId]>,
    resource_projection_requirements: Box<[PackedResourceProjectionRequirement]>,
    resource_projection_origins: Box<[PackedSourceRead]>,
    resource_projection_symbols: Box<[SymbolId]>,
    alpha_variables: Box<[TypeVariableId]>,
    execution_nodes: Box<[PackedExecutionNode]>,
    execution_dependencies: Box<[PackedExpressionRef]>,
    execution_selectors: Box<[PackedExecutionSelector]>,
    execution_selector_arms: Box<[PackedExpressionRef]>,
    execution_calls: Box<[PackedCallRef]>,
    execution_node_by_expression: Box<[u32]>,
}

impl DefinitionCodeStore {
    pub(crate) fn type_store(&self) -> &Arc<FrozenTypeStore> {
        &self.types
    }

    pub fn definition_count(&self) -> usize {
        self.definitions.len()
    }

    pub(crate) fn definitions(&self) -> impl ExactSizeIterator<Item = DefinitionCodeRef<'_>> + '_ {
        self.definitions
            .iter()
            .enumerate()
            .map(|(owner, code)| DefinitionCodeRef {
                store: self,
                owner: KernelOwnerId(
                    u32::try_from(owner).expect("kernel definition count exceeds u32"),
                ),
                code,
            })
    }

    pub fn definition(&self, owner: KernelOwnerId) -> Option<DefinitionCodeRef<'_>> {
        self.definitions
            .get(owner.0 as usize)
            .map(|code| DefinitionCodeRef {
                store: self,
                owner,
                code,
            })
    }

    pub(crate) fn execution_node(
        &self,
        expression: PackedExpressionRef,
    ) -> Option<PackedExecutionNodeRef<'_>> {
        let node = self.expression_node_slot(expression).copied()?;
        (node != MISSING_EXECUTION_ROW).then(|| PackedExecutionNodeRef {
            store: self,
            node: &self.execution_nodes[node as usize],
        })
    }

    pub(crate) fn symbol(&self, symbol: SymbolId) -> Option<&str> {
        self.types.as_arena().text_snapshot().symbol(symbol)
    }

    pub(crate) fn materialization_cache(&self) -> DefinitionTypeMaterializationCache {
        DefinitionTypeMaterializationCache {
            // Most compatibility projections already own the exact rich type
            // they need. Allocate the dense recursive cache only on the first
            // actual packed-type export, not merely because a linker phase
            // may need one exceptional type.
            types: Vec::new(),
        }
    }

    fn expression_node_slot(&self, expression: PackedExpressionRef) -> Option<&u32> {
        let definition = self.definitions.get(expression.owner.0 as usize)?;
        if expression.expression.0 >= definition.expressions.len {
            return None;
        }
        definition
            .execution_node_by_expression
            .get(&self.execution_node_by_expression)?
            .get(expression.expression.0 as usize)
    }

    fn call_exists(&self, call: PackedCallRef) -> bool {
        !call.is_none()
            && self
                .definitions
                .get(call.owner.0 as usize)
                .is_some_and(|definition| call.ordinal < definition.calls.len)
    }

    /// Validate the permanent execution columns in every build profile.
    ///
    /// Unlike recursive type validation, these checks are inexpensive and
    /// protect borrowed semantic accessors from malformed raw coordinates.
    fn validate_execution(&self) -> Result<(), KernelSolveError> {
        for (owner_index, definition) in self.definitions.iter().enumerate() {
            let owner = KernelOwnerId(u32::try_from(owner_index).map_err(|_| {
                KernelSolveError::new("kernel definition-code execution owner exceeds u32")
            })?);
            let local_nodes = definition
                .execution_node_by_expression
                .get(&self.execution_node_by_expression)
                .filter(|rows| rows.len() == definition.expressions.len as usize)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} has an invalid execution-node index span"
                    ))
                })?;
            let template_nodes = definition
                .execution_nodes
                .get(&self.execution_nodes)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} has an invalid execution-node span"
                    ))
                })?;
            let template_calls = definition
                .execution_calls
                .get(&self.execution_calls)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} has an invalid execution-call span"
                    ))
                })?;
            if template_calls.windows(2).any(|calls| calls[0] >= calls[1]) {
                return Err(KernelSolveError::new(format!(
                    "kernel definition-code owner {owner_index} execution calls are not strictly ordered"
                )));
            }
            for call in template_calls {
                if !self.call_exists(*call) {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} execution template references a missing call {}:{}",
                        call.owner.0, call.ordinal,
                    )));
                }
            }

            let has_template = definition.execution_result != MISSING_EXECUTION_ROW;
            if !has_template {
                if !template_nodes.is_empty() || !template_calls.is_empty() {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} has execution rows without a template result"
                    )));
                }
            } else {
                if definition.execution_result >= definition.expressions.len {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} execution result {} is outside its local expressions",
                        definition.execution_result,
                    )));
                }
                let result_node = local_nodes[definition.execution_result as usize];
                if result_node == MISSING_EXECUTION_ROW
                    || !definition.execution_nodes.contains(result_node as usize)
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} execution result has no node in its template"
                    )));
                }
                if template_nodes.is_empty() {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} execution template is empty"
                    )));
                }
            }

            for (local, node_index) in local_nodes.iter().copied().enumerate() {
                if node_index == MISSING_EXECUTION_ROW {
                    continue;
                }
                let node = self
                    .execution_nodes
                    .get(node_index as usize)
                    .ok_or_else(|| {
                        KernelSolveError::new(format!(
                            "kernel definition-code owner {owner_index} local expression {local} references a missing execution node {node_index}"
                        ))
                    })?;
                if node.expression
                    != PackedExpressionRef::new(
                        owner,
                        crate::KernelExpressionId(u32::try_from(local).map_err(|_| {
                            KernelSolveError::new(
                                "kernel definition-code local expression ordinal exceeds u32",
                            )
                        })?),
                    )
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner_index} local expression {local} points to the wrong execution node"
                    )));
                }
            }
        }

        for (node_index, node) in self.execution_nodes.iter().enumerate() {
            let template = self
                .definitions
                .get(node.template_owner.0 as usize)
                .filter(|definition| definition.execution_result != MISSING_EXECUTION_ROW)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel execution node {node_index} references missing template owner {}",
                        node.template_owner.0,
                    ))
                })?;
            if !template.execution_nodes.contains(node_index) {
                return Err(KernelSolveError::new(format!(
                    "kernel execution node {node_index} is outside template owner {}",
                    node.template_owner.0,
                )));
            }
            if self.expression_node_slot(node.expression).copied() != Some(node_index as u32) {
                return Err(KernelSolveError::new(format!(
                    "kernel execution node {node_index} has no unique reverse expression index"
                )));
            }
            let dependencies = node
                .dependencies
                .get(&self.execution_dependencies)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel execution node {node_index} has an invalid dependency span"
                    ))
                })?;
            if dependencies
                .windows(2)
                .any(|dependencies| dependencies[0] >= dependencies[1])
            {
                return Err(KernelSolveError::new(format!(
                    "kernel execution node {node_index} dependencies are not strictly ordered"
                )));
            }
            for dependency in dependencies {
                let dependency_node = self
                    .expression_node_slot(*dependency)
                    .copied()
                    .filter(|node| *node != MISSING_EXECUTION_ROW)
                    .and_then(|node| self.execution_nodes.get(node as usize))
                    .ok_or_else(|| {
                        KernelSolveError::new(format!(
                            "kernel execution node {node_index} references a missing dependency {}:{}",
                            dependency.owner.0, dependency.expression.0,
                        ))
                    })?;
                if dependency_node.template_owner != node.template_owner {
                    return Err(KernelSolveError::new(format!(
                        "kernel execution node {node_index} crosses execution templates through dependency {}:{}",
                        dependency.owner.0, dependency.expression.0,
                    )));
                }
            }
            if !node.call.is_none() {
                if !self.call_exists(node.call)
                    || template
                        .execution_calls
                        .get(&self.execution_calls)
                        .is_none_or(|calls| calls.binary_search(&node.call).is_err())
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel execution node {node_index} references call {}:{} outside its template",
                        node.call.owner.0, node.call.ordinal,
                    )));
                }
            }
            if node.selector != MISSING_EXECUTION_ROW {
                let selector = self
                    .execution_selectors
                    .get(node.selector as usize)
                    .ok_or_else(|| {
                        KernelSolveError::new(format!(
                            "kernel execution node {node_index} references missing selector {}",
                            node.selector,
                        ))
                    })?;
                let arms = selector
                    .arms
                    .get(&self.execution_selector_arms)
                    .ok_or_else(|| {
                        KernelSolveError::new(format!(
                            "kernel execution node {node_index} has an invalid selector-arm span"
                        ))
                    })?;
                for (label, expression) in std::iter::once(("input", selector.input))
                    .chain(arms.iter().copied().map(|arm| ("arm", arm)))
                {
                    let selector_node = self
                        .expression_node_slot(expression)
                        .copied()
                        .filter(|node| *node != MISSING_EXECUTION_ROW)
                        .and_then(|node| self.execution_nodes.get(node as usize))
                        .ok_or_else(|| {
                            KernelSolveError::new(format!(
                                "kernel execution node {node_index} references a missing selector {label} {}:{}",
                                expression.owner.0, expression.expression.0,
                            ))
                        })?;
                    if selector_node.template_owner != node.template_owner {
                        return Err(KernelSolveError::new(format!(
                            "kernel execution node {node_index} crosses execution templates through selector {label} {}:{}",
                            expression.owner.0, expression.expression.0,
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
    fn validate(&self) -> Result<(), KernelSolveError> {
        let mut term_generations = vec![0_u32; self.types.len()];
        let mut variable_generations = Vec::<u32>::new();
        let mut generation = 0_u32;
        let mut stack = Vec::<TypeTermId>::new();
        for (owner, definition) in self.definitions.iter().enumerate() {
            generation = generation.wrapping_add(1);
            if generation == 0 {
                term_generations.fill(0);
                variable_generations.fill(0);
                generation = 1;
            }
            definition.formals.get(&self.flows).ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel definition-code owner {owner} has an invalid formal span"
                ))
            })?;
            definition.expressions.get(&self.flows).ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel definition-code owner {owner} has an invalid expression span"
                ))
            })?;
            definition
                .expression_flush_types
                .get(&self.expression_flush_types)
                .filter(|rows| rows.len() == definition.expressions.len as usize)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition-code owner {owner} has an invalid expression-FLUSH span"
                    ))
                })?;
            definition
                .expression_kind_types
                .get(&self.expression_kind_types)
                .filter(|rows| rows.len() == definition.expressions.len as usize)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition-code owner {owner} has an invalid expression-kind-type span"
                    ))
                })?;
            for (label, span, len) in [
                (
                    "declaration-flow",
                    definition.declaration_flows,
                    self.declaration_flows.len(),
                ),
                ("call", definition.calls, self.calls.len()),
                (
                    "diagnostic-type",
                    definition.diagnostic_types,
                    self.diagnostic_types.len(),
                ),
                (
                    "SOURCE-payload",
                    definition.source_payload_types,
                    self.source_payload_types.len(),
                ),
                ("published-state", definition.states, self.states.len()),
                (
                    "LIST-item",
                    definition.list_item_types,
                    self.list_item_types.len(),
                ),
                (
                    "resource-projection",
                    definition.resource_projection_requirements,
                    self.resource_projection_requirements.len(),
                ),
            ] {
                let start = span.start as usize;
                let end = start.checked_add(span.len as usize).ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition-code owner {owner} {label} span overflows usize"
                    ))
                })?;
                if end > len {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner} has an invalid {label} span"
                    )));
                }
            }
            let calls = definition
                .calls
                .get(&self.calls)
                .expect("validated call span");
            if calls
                .windows(2)
                .any(|calls| calls[0].expression >= calls[1].expression)
            {
                return Err(KernelSolveError::new(format!(
                    "kernel definition-code owner {owner} calls are not in unique expression order"
                )));
            }
            for call in calls {
                if call.expression.0 >= definition.expressions.len {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner} call expression {} is outside its expression rows",
                        call.expression.0,
                    )));
                }
                call.substitutions
                    .get(&self.call_substitutions)
                    .ok_or_else(|| {
                        KernelSolveError::new(format!(
                            "kernel definition-code owner {owner} has an invalid call-substitution span"
                        ))
                    })?;
            }
            let states = definition
                .states
                .get(&self.states)
                .expect("validated published-state span");
            if states
                .windows(2)
                .any(|states| states[0].input_ordinal >= states[1].input_ordinal)
            {
                return Err(KernelSolveError::new(format!(
                    "kernel definition-code owner {owner} states are not in unique input order"
                )));
            }
            for requirement in definition
                .resource_projection_requirements
                .get(&self.resource_projection_requirements)
                .expect("validated resource-projection span")
            {
                requirement
                    .projection
                    .get(&self.resource_projection_symbols)
                    .ok_or_else(|| {
                        KernelSolveError::new(format!(
                            "kernel definition-code owner {owner} has an invalid resource-projection path"
                        ))
                    })?;
                for origin in requirement.origins.get(&self.resource_projection_origins).ok_or_else(
                    || {
                        KernelSolveError::new(format!(
                            "kernel definition-code owner {owner} has an invalid resource-projection origin span"
                        ))
                    },
                )? {
                    origin
                        .payload_projection
                        .get(&self.resource_projection_symbols)
                        .ok_or_else(|| {
                            KernelSolveError::new(format!(
                                "kernel definition-code owner {owner} has an invalid resource-origin path"
                            ))
                        })?;
                }
            }
            let alpha_variables = definition
                .alpha_variables
                .get(&self.alpha_variables)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition-code owner {owner} has an invalid alpha-variable span"
                    ))
                })?;
            for variable in alpha_variables.iter().copied() {
                let index = variable.0 as usize;
                if variable_generations.len() <= index {
                    variable_generations.resize(index + 1, 0);
                }
                if variable_generations[index] == generation {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner} repeats alpha variable {}",
                        variable.0
                    )));
                }
                variable_generations[index] = generation;
            }

            stack.clear();
            stack.push(definition.result.term);
            stack.extend(
                definition
                    .formals
                    .get(&self.flows)
                    .expect("validated formal span")
                    .iter()
                    .map(|flow| flow.term),
            );
            stack.extend(
                definition
                    .expressions
                    .get(&self.flows)
                    .expect("validated expression span")
                    .iter()
                    .map(|flow| flow.term),
            );
            stack.extend(
                definition
                    .expression_flush_types
                    .get(&self.expression_flush_types)
                    .expect("validated expression-FLUSH span")
                    .iter()
                    .flatten()
                    .copied(),
            );
            stack.extend(
                definition
                    .resource_projection_requirements
                    .get(&self.resource_projection_requirements)
                    .expect("validated resource-projection span")
                    .iter()
                    .map(|requirement| requirement.required_term),
            );
            stack.extend(
                definition
                    .expression_kind_types
                    .get(&self.expression_kind_types)
                    .expect("validated expression-kind span")
                    .iter()
                    .flatten()
                    .copied(),
            );
            stack.extend(
                definition
                    .declaration_flows
                    .get(&self.declaration_flows)
                    .expect("validated declaration span")
                    .iter()
                    .flatten()
                    .map(|flow| flow.term),
            );
            for call in definition
                .calls
                .get(&self.calls)
                .expect("validated call span")
            {
                stack.extend(
                    call.substitutions
                        .get(&self.call_substitutions)
                        .expect("validated call-substitution span")
                        .iter()
                        .map(|substitution| substitution.term),
                );
            }
            stack.extend(
                definition
                    .source_payload_types
                    .get(&self.source_payload_types)
                    .expect("validated SOURCE span")
                    .iter()
                    .copied(),
            );
            for diagnostic in definition
                .diagnostic_types
                .get(&self.diagnostic_types)
                .expect("validated diagnostic-type span")
                .iter()
                .flatten()
            {
                stack.push(diagnostic.actual);
                stack.push(diagnostic.expected);
            }
            stack.extend(
                definition
                    .states
                    .get(&self.states)
                    .expect("validated state span")
                    .iter()
                    .map(|state| state.flow.term),
            );
            stack.extend(
                definition
                    .list_item_types
                    .get(&self.list_item_types)
                    .expect("validated LIST span")
                    .iter()
                    .copied(),
            );
            while let Some(term) = stack.pop() {
                let index = term.0 as usize;
                let Some(seen) = term_generations.get_mut(index) else {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition-code owner {owner} references missing type term {}",
                        term.0
                    )));
                };
                if *seen == generation {
                    continue;
                }
                *seen = generation;
                match self.types.as_arena().term(term) {
                    TypeTerm::Variable(variable) => {
                        if variable_generations.get(variable.0 as usize).copied()
                            != Some(generation)
                        {
                            return Err(KernelSolveError::new(format!(
                                "kernel definition-code owner {owner} omits type variable {} from its alpha map",
                                variable.0
                            )));
                        }
                    }
                    TypeTerm::VariantSet(variants) => {
                        stack.extend(variants.iter().filter_map(|variant| match variant {
                            crate::VariantTerm::Tag(_) => None,
                            crate::VariantTerm::Tagged { fields, .. } => Some(*fields),
                        }));
                    }
                    TypeTerm::Object { fields, .. } => {
                        stack.extend(fields.canonical_iter().map(|field| field.ty));
                    }
                    TypeTerm::List(item) | TypeTerm::Set(item) => stack.push(item),
                    TypeTerm::Function { args, result, .. } => {
                        stack.extend(args.iter().copied());
                        stack.push(result);
                    }
                    TypeTerm::Union(members) => stack.extend(members.iter().copied()),
                    TypeTerm::Map { key, value } => {
                        stack.push(key);
                        stack.push(value);
                    }
                    TypeTerm::Text
                    | TypeTerm::Number
                    | TypeTerm::Bytes(_)
                    | TypeTerm::Absent
                    | TypeTerm::OpenObjectPlaceholder
                    | TypeTerm::RenderContract
                    | TypeTerm::UnresolvedShape(_)
                    | TypeTerm::Unknown
                    | TypeTerm::Bits(_) => {}
                }
            }
        }
        Ok(())
    }
}

/// Borrowed, store-qualified view of one definition's packed flow rows.
#[derive(Clone, Copy, Debug)]
pub struct DefinitionCodeRef<'a> {
    store: &'a DefinitionCodeStore,
    owner: KernelOwnerId,
    code: &'a DefinitionCode,
}

/// Store-qualified borrowed view of one callable execution template.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedDefinitionExecutionRef<'a> {
    code: DefinitionCodeRef<'a>,
}

/// Store-qualified borrowed view of one execution node.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedExecutionNodeRef<'a> {
    store: &'a DefinitionCodeStore,
    node: &'a PackedExecutionNode,
}

/// Store-qualified borrowed view of one sparse selector row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedExecutionSelectorRef<'a> {
    store: &'a DefinitionCodeStore,
    selector: &'a PackedExecutionSelector,
}

pub(crate) struct PackedExecutionNodeIter<'a> {
    store: &'a DefinitionCodeStore,
    nodes: std::slice::Iter<'a, PackedExecutionNode>,
}

impl<'a> Iterator for PackedExecutionNodeIter<'a> {
    type Item = PackedExecutionNodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.nodes.next().map(|node| PackedExecutionNodeRef {
            store: self.store,
            node,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.nodes.size_hint()
    }
}

impl DoubleEndedIterator for PackedExecutionNodeIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.nodes.next_back().map(|node| PackedExecutionNodeRef {
            store: self.store,
            node,
        })
    }
}

impl ExactSizeIterator for PackedExecutionNodeIter<'_> {}

impl<'a> DefinitionCodeRef<'a> {
    pub const fn owner(self) -> KernelOwnerId {
        self.owner
    }

    pub(crate) const fn has_execution_template(self) -> bool {
        self.code.execution_result != MISSING_EXECUTION_ROW
    }

    pub(crate) fn execution_template(self) -> Option<PackedDefinitionExecutionRef<'a>> {
        self.has_execution_template()
            .then_some(PackedDefinitionExecutionRef { code: self })
    }

    pub(crate) fn execution_result(self) -> Option<PackedExpressionRef> {
        self.has_execution_template().then(|| {
            PackedExpressionRef::new(
                self.owner,
                crate::KernelExpressionId(self.code.execution_result),
            )
        })
    }

    pub(crate) fn execution_nodes(self) -> PackedExecutionNodeIter<'a> {
        PackedExecutionNodeIter {
            store: self.store,
            nodes: self
                .code
                .execution_nodes
                .get(&self.store.execution_nodes)
                .expect("sealed definition-code execution-node span is valid")
                .iter(),
        }
    }

    pub(crate) fn execution_calls(self) -> &'a [PackedCallRef] {
        self.code
            .execution_calls
            .get(&self.store.execution_calls)
            .expect("sealed definition-code execution-call span is valid")
    }

    #[cfg(test)]
    pub(crate) fn execution_node_for_local_expression(
        self,
        expression: crate::KernelExpressionId,
    ) -> Option<PackedExecutionNodeRef<'a>> {
        if expression.0 >= self.code.expressions.len {
            return None;
        }
        self.store
            .execution_node(PackedExpressionRef::new(self.owner, expression))
    }

    pub(crate) fn formals(self) -> &'a [KernelArtifactFlowTermV1] {
        self.code
            .formals
            .get(&self.store.flows)
            .expect("sealed definition-code formal span is valid")
    }

    pub(crate) fn expressions(self) -> &'a [KernelArtifactFlowTermV1] {
        self.code
            .expressions
            .get(&self.store.flows)
            .expect("sealed definition-code expression span is valid")
    }

    pub(crate) fn expression_count(self) -> usize {
        self.code.expressions.len as usize
    }

    pub(crate) fn call_count(self) -> usize {
        self.code.calls.len as usize
    }

    pub(crate) fn call_expression(self, ordinal: usize) -> Option<crate::KernelExpressionId> {
        self.code
            .calls
            .get(&self.store.calls)
            .expect("sealed definition-code call span is valid")
            .get(ordinal)
            .map(|call| call.expression)
    }

    pub(crate) const fn effect_summary(self) -> crate::KernelEffectSummary {
        self.code.effect_summary
    }

    pub(crate) fn effect_summary_for(
        self,
        owner: KernelOwnerId,
    ) -> Option<crate::KernelEffectSummary> {
        self.store
            .definition(owner)
            .map(DefinitionCodeRef::effect_summary)
    }

    pub(crate) const fn basis_fingerprint_v14(self) -> [u8; 32] {
        self.code.basis_fingerprint_v14
    }

    pub(crate) fn source_count(self) -> usize {
        self.code.source_payload_types.len as usize
    }

    pub(crate) fn state_count(self) -> usize {
        self.code.states.len as usize
    }

    pub(crate) fn states(self) -> &'a [PackedPublishedState] {
        self.code
            .states
            .get(&self.store.states)
            .expect("sealed definition-code state span is valid")
    }

    pub(crate) fn list_count(self) -> usize {
        self.code.list_item_types.len as usize
    }

    pub(crate) fn published_expression(self, ordinal: usize) -> Option<KernelArtifactFlowTermV1> {
        let base = self.expressions().get(ordinal).copied()?;
        let expression = crate::KernelExpressionId(u32::try_from(ordinal).ok()?);
        let requirement = self
            .resource_projection_requirements()
            .binary_search_by_key(&expression, |requirement| requirement.expression)
            .ok()
            .and_then(|index| self.resource_projection_requirements().get(index));
        requirement
            .and_then(|requirement| requirement.published_expression)
            .or(Some(base))
    }

    pub(crate) fn resource_projection_requirement_count(self) -> usize {
        self.code.resource_projection_requirements.len as usize
    }

    pub(crate) fn resource_projection_requirements(
        self,
    ) -> &'a [PackedResourceProjectionRequirement] {
        self.code
            .resource_projection_requirements
            .get(&self.store.resource_projection_requirements)
            .expect("sealed definition-code resource-projection span is valid")
    }

    pub(crate) fn resource_projection_origins(
        self,
        requirement: &PackedResourceProjectionRequirement,
    ) -> &'a [PackedSourceRead] {
        requirement
            .origins
            .get(&self.store.resource_projection_origins)
            .expect("sealed definition-code resource-projection origin span is valid")
    }

    pub(crate) fn resource_projection_path_symbols(self, ordinal: usize) -> Option<&'a [SymbolId]> {
        let requirement = self.resource_projection_requirements().get(ordinal)?;
        requirement
            .projection
            .get(&self.store.resource_projection_symbols)
    }

    pub(crate) fn resource_projection_origin_count(self, ordinal: usize) -> Option<usize> {
        let requirement = self.resource_projection_requirements().get(ordinal)?;
        Some(self.resource_projection_origins(requirement).len())
    }

    pub(crate) fn resource_projection_origin(
        self,
        requirement_ordinal: usize,
        origin_ordinal: usize,
    ) -> Option<(KernelOwnerId, crate::KernelSourceId, &'a [SymbolId])> {
        let requirement = self
            .resource_projection_requirements()
            .get(requirement_ordinal)?;
        let origin = self
            .resource_projection_origins(requirement)
            .get(origin_ordinal)?;
        Some((
            origin.owner,
            origin.source,
            origin
                .payload_projection
                .get(&self.store.resource_projection_symbols)?,
        ))
    }

    pub(crate) fn resource_projection_required_is_published(self, ordinal: usize) -> Option<bool> {
        let requirement = self.resource_projection_requirements().get(ordinal)?;
        Some(
            self.published_expression(requirement.expression.0 as usize)?
                .term
                == requirement.required_term,
        )
    }

    pub(crate) fn alpha_variables(self) -> &'a [TypeVariableId] {
        self.code
            .alpha_variables
            .get(&self.store.alpha_variables)
            .expect("sealed definition-code alpha-variable span is valid")
    }

    pub(crate) fn alpha_variable_count(self) -> usize {
        self.code.alpha_variables.len as usize
    }

    pub(crate) const fn stable_digest(self) -> [u8; 32] {
        self.code.stable_digest
    }

    pub(crate) fn expression_surface_digest(self, ordinal: usize) -> Option<[u8; 32]> {
        self.published_expression(ordinal)
            .map(|flow| flow.stable_digest)
    }

    /// Create one compatibility materializer whose alpha variables already use
    /// their final linked namespace.
    ///
    /// The old linker first materialized every recursive type with variables
    /// numbered from zero and then recursively cloned any variable-bearing
    /// object merely to add the definition's global base. Supplying that base
    /// here makes the compatibility alpha-normalization map directly into the
    /// final namespace, so the linker does not clone the type a second time.
    /// Closed terms continue to share the project-wide export cache.
    pub(crate) fn linked_materializer<'cache>(
        self,
        cache: &'cache mut DefinitionTypeMaterializationCache,
        alpha_start: u32,
    ) -> DefinitionCodeMaterializer<'a, 'cache> {
        let mut variables = BTreeMap::new();
        for (ordinal, source) in self.alpha_variables().iter().copied().enumerate() {
            let ordinal =
                u32::try_from(ordinal).expect("sealed definition alpha-variable count exceeds u32");
            variables.insert(
                TypeVar(source.0),
                TypeVar(
                    alpha_start
                        .checked_add(ordinal)
                        .expect("linked definition alpha-variable namespace overflows u32"),
                ),
            );
        }
        let next = alpha_start
            .checked_add(
                u32::try_from(variables.len())
                    .expect("sealed definition alpha-variable count exceeds u32"),
            )
            .expect("linked definition alpha-variable namespace overflows u32");
        DefinitionCodeMaterializer {
            code: self,
            cache,
            variables,
            next,
            alpha_end: next,
        }
    }

    pub fn materialize_result(self) -> FlowType {
        self.materialize_flow(self.code.result)
    }

    pub fn materialize_formal(self, ordinal: usize) -> Option<FlowType> {
        self.formals()
            .get(ordinal)
            .copied()
            .map(|flow| self.materialize_flow(flow))
    }

    pub fn materialize_expression(self, ordinal: usize) -> Option<FlowType> {
        self.expressions()
            .get(ordinal)
            .copied()
            .map(|flow| self.materialize_flow(flow))
    }

    pub fn materialize_published_expression(self, ordinal: usize) -> Option<FlowType> {
        self.published_expression(ordinal)
            .map(|flow| self.materialize_flow(flow))
    }

    pub(crate) fn materialize_resource_projection_requirement(
        self,
        ordinal: usize,
    ) -> Option<MaterializedResourceProjectionRequirement> {
        let requirement = *self.resource_projection_requirements().get(ordinal)?;
        let text = self.store.types.as_arena().text_snapshot();
        let materialize_path = |span: Span32| {
            span.get(&self.store.resource_projection_symbols)
                .expect("sealed resource-projection symbol span is valid")
                .iter()
                .map(|symbol| {
                    text.symbol(*symbol)
                        .expect("resource-projection symbol belongs to the text authority")
                        .to_owned()
                })
                .collect::<Vec<_>>()
        };
        let origins = self
            .resource_projection_origins(&requirement)
            .iter()
            .map(|origin| MaterializedSourceRead {
                owner: origin.owner,
                source: origin.source,
                payload_projection: materialize_path(origin.payload_projection),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let required_type = self
            .published_expression(requirement.expression.0 as usize)
            .filter(|published| published.term == requirement.required_term)
            .is_none()
            .then(|| self.materialize_type(requirement.required_term));
        Some(MaterializedResourceProjectionRequirement {
            expression: requirement.expression,
            target: requirement.target,
            projection: materialize_path(requirement.projection).into_boxed_slice(),
            origins,
            required_type,
        })
    }

    pub fn materialize_expression_flush(self, ordinal: usize) -> Option<Type> {
        let term = self
            .code
            .expression_flush_types
            .get(&self.store.expression_flush_types)
            .expect("sealed definition-code expression-FLUSH span is valid")
            .get(ordinal)
            .copied()
            .flatten()?;
        Some(self.materialize_type(term))
    }

    pub fn materialize_expression_kind_type(self, ordinal: usize) -> Option<Type> {
        let term = self
            .code
            .expression_kind_types
            .get(&self.store.expression_kind_types)
            .expect("sealed definition-code expression-kind-type span is valid")
            .get(ordinal)
            .copied()
            .flatten()?;
        Some(self.materialize_type(term))
    }

    pub fn materialize_declaration_flow(self, ordinal: usize) -> Option<FlowType> {
        let flow = self
            .code
            .declaration_flows
            .get(&self.store.declaration_flows)
            .expect("sealed definition-code declaration-flow span is valid")
            .get(ordinal)
            .copied()
            .flatten()?;
        Some(self.materialize_packed_flow(flow))
    }

    pub(crate) fn diagnostic_count(self) -> usize {
        self.code.diagnostic_types.len as usize
    }

    pub fn materialize_call_facts(self, ordinal: usize) -> Option<MaterializedCallFacts> {
        let call = self
            .code
            .calls
            .get(&self.store.calls)
            .expect("sealed definition-code call span is valid")
            .get(ordinal)?;
        let substitutions = call
            .substitutions
            .get(&self.store.call_substitutions)
            .expect("sealed definition-code call-substitution span is valid")
            .iter()
            .map(|substitution| crate::KernelCallTypeSubstitution {
                variable: substitution.variable,
                value: self.materialize_type(substitution.term),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Some(MaterializedCallFacts {
            substitutions,
            syntax_discriminated_result: call.syntax_discriminated_result,
        })
    }

    pub fn materialize_source_payload_type(self, ordinal: usize) -> Option<Type> {
        self.code
            .source_payload_types
            .get(&self.store.source_payload_types)
            .expect("sealed definition-code SOURCE-payload span is valid")
            .get(ordinal)
            .copied()
            .map(|term| self.materialize_type(term))
    }

    pub fn materialize_state_flow(self, ordinal: usize) -> Option<FlowType> {
        self.code
            .states
            .get(&self.store.states)
            .expect("sealed definition-code state-flow span is valid")
            .get(ordinal)
            .copied()
            .map(|state| self.materialize_packed_flow(state.flow))
    }

    pub fn materialize_list_item_type(self, ordinal: usize) -> Option<Type> {
        self.code
            .list_item_types
            .get(&self.store.list_item_types)
            .expect("sealed definition-code LIST-item span is valid")
            .get(ordinal)
            .copied()
            .map(|term| self.materialize_type(term))
    }

    fn materialize_flow(self, flow: KernelArtifactFlowTermV1) -> FlowType {
        let raw = FlowType {
            mode: flow.mode,
            ty: self.store.types.as_arena().export_checked_type(flow.term),
        };
        self.alpha_normalize_flow(raw)
    }

    fn materialize_type(self, term: crate::TypeTermId) -> Type {
        self.alpha_normalize_flow(FlowType {
            mode: FlowMode::Continuous,
            ty: self.store.types.as_arena().export_checked_type(term),
        })
        .ty
    }

    fn materialize_packed_flow(self, flow: PackedFlow) -> FlowType {
        self.alpha_normalize_flow(FlowType {
            mode: flow.mode,
            ty: self.store.types.as_arena().export_checked_type(flow.term),
        })
    }

    fn alpha_normalize_flow(self, raw: FlowType) -> FlowType {
        let mut variables = BTreeMap::new();
        for (ordinal, source) in self.alpha_variables().iter().copied().enumerate() {
            variables.insert(
                TypeVar(source.0),
                TypeVar(
                    u32::try_from(ordinal)
                        .expect("sealed definition alpha-variable count exceeds u32"),
                ),
            );
        }
        let mut next = u32::try_from(variables.len())
            .expect("sealed definition alpha-variable count exceeds u32");
        let normalized = alpha_normalize_flow_type(&raw, &mut variables, &mut next);
        assert_eq!(
            next as usize,
            self.alpha_variables().len(),
            "sealed definition-code alpha map omitted a materialized variable"
        );
        normalized
    }
}

impl<'a> PackedDefinitionExecutionRef<'a> {
    pub(crate) const fn owner(self) -> KernelOwnerId {
        self.code.owner
    }

    pub(crate) fn result(self) -> PackedExpressionRef {
        self.code
            .execution_result()
            .expect("packed execution view always has a result")
    }

    pub(crate) fn nodes(self) -> PackedExecutionNodeIter<'a> {
        self.code.execution_nodes()
    }

    pub(crate) fn calls(self) -> &'a [PackedCallRef] {
        self.code.execution_calls()
    }

    pub(crate) const fn source_count(self) -> usize {
        self.code.code.source_payload_types.len as usize
    }

    pub(crate) const fn state_count(self) -> usize {
        self.code.code.states.len as usize
    }

    pub(crate) const fn list_count(self) -> usize {
        self.code.code.list_item_types.len as usize
    }
}

impl<'a> PackedExecutionNodeRef<'a> {
    pub(crate) const fn template_owner(self) -> KernelOwnerId {
        self.node.template_owner
    }

    pub(crate) const fn expression(self) -> PackedExpressionRef {
        self.node.expression
    }

    pub(crate) fn dependencies(self) -> &'a [PackedExpressionRef] {
        self.node
            .dependencies
            .get(&self.store.execution_dependencies)
            .expect("sealed definition-code execution-dependency span is valid")
    }

    pub(crate) const fn call(self) -> Option<PackedCallRef> {
        if self.node.call.is_none() {
            None
        } else {
            Some(self.node.call)
        }
    }

    pub(crate) fn selector(self) -> Option<PackedExecutionSelectorRef<'a>> {
        (self.node.selector != MISSING_EXECUTION_ROW).then(|| PackedExecutionSelectorRef {
            store: self.store,
            selector: &self.store.execution_selectors[self.node.selector as usize],
        })
    }
}

impl<'a> PackedExecutionSelectorRef<'a> {
    pub(crate) const fn input(self) -> PackedExpressionRef {
        self.selector.input
    }

    pub(crate) fn arms(self) -> &'a [PackedExpressionRef] {
        self.selector
            .arms
            .get(&self.store.execution_selector_arms)
            .expect("sealed definition-code execution-selector-arm span is valid")
    }
}

/// Phase-local recursive export cache shared by every definition materialized
/// into the compatibility checked image. It is deliberately external to the
/// frozen store and is dropped with the linker phase.
pub(crate) struct DefinitionTypeMaterializationCache {
    types: Vec<Option<Type>>,
}

/// One definition-local alpha authority over a project-wide recursive export
/// cache. Repeated roots clone shared `Arc` subtrees instead of rebuilding the
/// same strings, object maps, vectors, and boxes.
pub(crate) struct DefinitionCodeMaterializer<'code, 'cache> {
    code: DefinitionCodeRef<'code>,
    cache: &'cache mut DefinitionTypeMaterializationCache,
    variables: BTreeMap<TypeVar, TypeVar>,
    next: u32,
    alpha_end: u32,
}

impl DefinitionCodeMaterializer<'_, '_> {
    pub(crate) fn materialize_result(&mut self) -> FlowType {
        self.materialize_flow(self.code.code.result)
    }

    pub(crate) fn materialize_formal(&mut self, ordinal: usize) -> Option<FlowType> {
        self.code
            .formals()
            .get(ordinal)
            .copied()
            .map(|flow| self.materialize_flow(flow))
    }

    pub(crate) fn materialize_expression(&mut self, ordinal: usize) -> Option<FlowType> {
        self.code
            .expressions()
            .get(ordinal)
            .copied()
            .map(|flow| self.materialize_flow(flow))
    }

    pub(crate) fn materialize_published_expression(&mut self, ordinal: usize) -> Option<FlowType> {
        self.code
            .published_expression(ordinal)
            .map(|flow| self.materialize_flow(flow))
    }

    pub(crate) fn materialize_declaration_flow(&mut self, ordinal: usize) -> Option<FlowType> {
        let flow = self
            .code
            .code
            .declaration_flows
            .get(&self.code.store.declaration_flows)
            .expect("sealed definition-code declaration-flow span is valid")
            .get(ordinal)
            .copied()
            .flatten()?;
        Some(self.materialize_packed_flow(flow))
    }

    pub(crate) fn materialize_call_facts(
        &mut self,
        ordinal: usize,
    ) -> Option<MaterializedCallFacts> {
        let call = *self
            .code
            .code
            .calls
            .get(&self.code.store.calls)
            .expect("sealed definition-code call span is valid")
            .get(ordinal)?;
        let substitutions = call
            .substitutions
            .get(&self.code.store.call_substitutions)
            .expect("sealed definition-code call-substitution span is valid")
            .to_vec();
        Some(MaterializedCallFacts {
            substitutions: substitutions
                .into_iter()
                .map(|substitution| crate::KernelCallTypeSubstitution {
                    variable: substitution.variable,
                    value: self.materialize_type(substitution.term),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            syntax_discriminated_result: call.syntax_discriminated_result,
        })
    }

    pub(crate) fn materialize_source_payload_type(&mut self, ordinal: usize) -> Option<Type> {
        let term = self
            .code
            .code
            .source_payload_types
            .get(&self.code.store.source_payload_types)
            .expect("sealed definition-code SOURCE-payload span is valid")
            .get(ordinal)
            .copied()?;
        Some(self.materialize_type(term))
    }

    pub(crate) fn materialize_state_flow(&mut self, ordinal: usize) -> Option<FlowType> {
        let flow = self
            .code
            .code
            .states
            .get(&self.code.store.states)
            .expect("sealed definition-code state-flow span is valid")
            .get(ordinal)
            .copied()?;
        Some(self.materialize_packed_flow(flow.flow))
    }

    pub(crate) fn materialize_list_item_type(&mut self, ordinal: usize) -> Option<Type> {
        let term = self
            .code
            .code
            .list_item_types
            .get(&self.code.store.list_item_types)
            .expect("sealed definition-code LIST-item span is valid")
            .get(ordinal)
            .copied()?;
        Some(self.materialize_type(term))
    }

    pub(crate) fn materialize_expression_flush(&mut self, ordinal: usize) -> Option<Type> {
        let term = self
            .code
            .code
            .expression_flush_types
            .get(&self.code.store.expression_flush_types)
            .expect("sealed definition-code expression-FLUSH span is valid")
            .get(ordinal)
            .copied()
            .flatten()?;
        Some(self.materialize_type(term))
    }

    fn materialize_flow(&mut self, flow: KernelArtifactFlowTermV1) -> FlowType {
        FlowType {
            mode: flow.mode,
            ty: self.materialize_type(flow.term),
        }
    }

    fn materialize_packed_flow(&mut self, flow: PackedFlow) -> FlowType {
        FlowType {
            mode: flow.mode,
            ty: self.materialize_type(flow.term),
        }
    }

    fn materialize_type(&mut self, term: TypeTermId) -> Type {
        let arena = self.code.store.types.as_arena();
        if self.cache.types.len() != arena.len() {
            self.cache.types.resize(arena.len(), None);
        }
        let raw = arena.export_checked_type_cached(term, &mut self.cache.types);
        if !arena.has_variable(term) {
            return raw;
        }
        let normalized = alpha_normalize_flow_type(
            &FlowType {
                mode: FlowMode::Continuous,
                ty: raw,
            },
            &mut self.variables,
            &mut self.next,
        )
        .ty;
        assert_eq!(
            self.next, self.alpha_end,
            "sealed definition-code alpha map omitted a materialized variable"
        );
        normalized
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedFlow {
    pub(crate) mode: FlowMode,
    pub(crate) term: crate::TypeTermId,
}

/// One source payload origin retained in definition-local coordinates.
///
/// The payload path span addresses the definition-local symbol input while
/// building and the store-global symbol column after sealing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedSourceReadInput {
    pub(crate) owner: KernelOwnerId,
    pub(crate) source: crate::KernelSourceId,
    pub(crate) payload_projection_start: u32,
    pub(crate) payload_projection_len: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedSourceRead {
    owner: KernelOwnerId,
    source: crate::KernelSourceId,
    payload_projection: Span32,
}

impl PackedSourceRead {
    pub(crate) const fn owner(self) -> KernelOwnerId {
        self.owner
    }

    pub(crate) const fn source(self) -> crate::KernelSourceId {
        self.source
    }
}

/// Definition-local input for one exact SOURCE/resource projection fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedResourceProjectionRequirementInput {
    pub(crate) expression: crate::KernelExpressionId,
    pub(crate) target: crate::KernelDeclarationReference,
    pub(crate) projection_start: u32,
    pub(crate) projection_len: u32,
    pub(crate) origin_start: u32,
    pub(crate) origin_len: u32,
    pub(crate) required_term: TypeTermId,
    pub(crate) published_expression: Option<KernelArtifactFlowTermV1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedResourceProjectionRequirement {
    expression: crate::KernelExpressionId,
    target: crate::KernelDeclarationReference,
    projection: Span32,
    origins: Span32,
    required_term: TypeTermId,
    published_expression: Option<KernelArtifactFlowTermV1>,
}

impl PackedResourceProjectionRequirement {
    pub(crate) const fn expression(self) -> crate::KernelExpressionId {
        self.expression
    }

    pub(crate) const fn target(self) -> crate::KernelDeclarationReference {
        self.target
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedCallFacts {
    expression: crate::KernelExpressionId,
    substitutions: Span32,
    syntax_discriminated_result: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedCallFactsInput {
    pub(crate) expression: crate::KernelExpressionId,
    pub(crate) substitution_start: u32,
    pub(crate) substitution_len: u32,
    pub(crate) syntax_discriminated_result: bool,
}

const MISSING_SYNTHETIC_STATE_ORDINAL: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedPublishedState {
    pub(crate) input_ordinal: u32,
    pub(crate) synthetic_ordinal: u32,
    pub(crate) flow: PackedFlow,
}

impl PackedPublishedState {
    pub(crate) const fn new(
        input_ordinal: u32,
        synthetic_ordinal: Option<u32>,
        flow: PackedFlow,
    ) -> Self {
        Self {
            input_ordinal,
            synthetic_ordinal: match synthetic_ordinal {
                Some(ordinal) => ordinal,
                None => MISSING_SYNTHETIC_STATE_ORDINAL,
            },
            flow,
        }
    }

    pub(crate) const fn synthetic_ordinal(self) -> Option<u32> {
        if self.synthetic_ordinal == MISSING_SYNTHETIC_STATE_ORDINAL {
            None
        } else {
            Some(self.synthetic_ordinal)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedCallTypeSubstitution {
    pub(crate) variable: crate::KernelTypeParameterId,
    pub(crate) term: TypeTermId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedDiagnosticTypes {
    pub(crate) actual: TypeTermId,
    pub(crate) expected: TypeTermId,
}

pub struct MaterializedCallFacts {
    pub substitutions: Box<[crate::KernelCallTypeSubstitution]>,
    pub syntax_discriminated_result: bool,
}

pub(crate) struct MaterializedSourceRead {
    pub(crate) owner: KernelOwnerId,
    pub(crate) source: crate::KernelSourceId,
    pub(crate) payload_projection: Vec<String>,
}

pub(crate) struct MaterializedResourceProjectionRequirement {
    pub(crate) expression: crate::KernelExpressionId,
    pub(crate) target: crate::KernelDeclarationReference,
    pub(crate) projection: Box<[String]>,
    pub(crate) origins: Box<[MaterializedSourceRead]>,
    /// `None` means the already-materialized published expression owns this
    /// exact rich type. Exceptional no-origin requirements retain a distinct
    /// packed fallback and materialize only that term.
    pub(crate) required_type: Option<Type>,
}

pub(crate) struct DefinitionAdditionalTypeRoots<'a> {
    pub(crate) effect_summary: crate::KernelEffectSummary,
    pub(crate) basis_fingerprint_v14: [u8; 32],
    pub(crate) expression_flush_types: &'a [Option<crate::TypeTermId>],
    pub(crate) expression_kind_types: &'a [Option<crate::TypeTermId>],
    pub(crate) declaration_flows: &'a [Option<PackedFlow>],
    pub(crate) calls: &'a [PackedCallFactsInput],
    pub(crate) call_substitutions: &'a [PackedCallTypeSubstitution],
    pub(crate) diagnostic_types: &'a [Option<PackedDiagnosticTypes>],
    pub(crate) source_payload_types: &'a [crate::TypeTermId],
    /// Number of authored state candidates in the immutable definition facts.
    /// Published rows retain dense input ordinals into that authority.
    pub(crate) state_input_count: usize,
    pub(crate) states: &'a [PackedPublishedState],
    pub(crate) list_item_types: &'a [crate::TypeTermId],
    pub(crate) resource_projection_requirements: &'a [PackedResourceProjectionRequirementInput],
    pub(crate) resource_projection_origins: &'a [PackedSourceReadInput],
    pub(crate) resource_projection_symbols: &'a [SymbolId],
    pub(crate) alpha_variables: &'a [TypeVariableId],
    pub(crate) stable_digest: [u8; 32],
}

/// Capability for the one execution template currently being appended.
///
/// Tokens are deliberately opaque and single-use. They prevent an owner-side
/// traversal from accidentally appending rows to a template that has already
/// been sealed or to a different definition's active template.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DefinitionExecutionTemplateToken {
    owner: KernelOwnerId,
    serial: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveExecutionTemplate {
    token: DefinitionExecutionTemplateToken,
    result: PackedExpressionRef,
    node_start: usize,
}

#[derive(Debug)]
pub(crate) struct DefinitionCodeBuilder {
    definitions: Vec<DefinitionCode>,
    flows: Vec<KernelArtifactFlowTermV1>,
    expression_flush_types: Vec<Option<crate::TypeTermId>>,
    expression_kind_types: Vec<Option<crate::TypeTermId>>,
    declaration_flows: Vec<Option<PackedFlow>>,
    calls: Vec<PackedCallFacts>,
    call_substitutions: Vec<PackedCallTypeSubstitution>,
    diagnostic_types: Vec<Option<PackedDiagnosticTypes>>,
    source_payload_types: Vec<crate::TypeTermId>,
    states: Vec<PackedPublishedState>,
    list_item_types: Vec<crate::TypeTermId>,
    resource_projection_requirements: Vec<PackedResourceProjectionRequirement>,
    resource_projection_origins: Vec<PackedSourceRead>,
    resource_projection_symbols: Vec<SymbolId>,
    alpha_variables: Vec<TypeVariableId>,
    execution_nodes: Vec<PackedExecutionNode>,
    execution_dependencies: Vec<PackedExpressionRef>,
    execution_selectors: Vec<PackedExecutionSelector>,
    execution_selector_arms: Vec<PackedExpressionRef>,
    execution_calls: Vec<PackedCallRef>,
    execution_node_by_expression: Vec<u32>,
    /// Phase-local duplicate/exact-membership check. It is deliberately not
    /// retained in the immutable store.
    execution_node_by_call: Vec<u32>,
    active_execution: Option<ActiveExecutionTemplate>,
    next_execution_serial: u32,
}

impl DefinitionCodeBuilder {
    pub(crate) fn with_capacity(definitions: usize, flows: usize, alpha_variables: usize) -> Self {
        Self {
            definitions: Vec::with_capacity(definitions),
            flows: Vec::with_capacity(flows),
            expression_flush_types: Vec::with_capacity(flows.saturating_sub(definitions)),
            expression_kind_types: Vec::with_capacity(flows.saturating_sub(definitions)),
            declaration_flows: Vec::new(),
            calls: Vec::new(),
            call_substitutions: Vec::new(),
            diagnostic_types: Vec::new(),
            source_payload_types: Vec::new(),
            states: Vec::new(),
            list_item_types: Vec::new(),
            resource_projection_requirements: Vec::new(),
            resource_projection_origins: Vec::new(),
            resource_projection_symbols: Vec::new(),
            alpha_variables: Vec::with_capacity(alpha_variables),
            execution_nodes: Vec::new(),
            execution_dependencies: Vec::new(),
            execution_selectors: Vec::new(),
            execution_selector_arms: Vec::new(),
            execution_calls: Vec::new(),
            execution_node_by_expression: Vec::with_capacity(flows.saturating_sub(definitions)),
            execution_node_by_call: Vec::new(),
            active_execution: None,
            next_execution_serial: 0,
        }
    }

    /// Reserve the project-wide execution columns once before walking any
    /// template. Passing exact counts yields zero column growth during node
    /// publication; safe upper bounds are also accepted.
    pub(crate) fn reserve_execution(
        &mut self,
        nodes: usize,
        dependencies: usize,
        selectors: usize,
        selector_arms: usize,
        calls: usize,
    ) {
        self.execution_nodes.reserve(nodes);
        self.execution_dependencies.reserve(dependencies);
        self.execution_selectors.reserve(selectors);
        self.execution_selector_arms.reserve(selector_arms);
        self.execution_calls.reserve(calls);
    }

    fn execution_expression_slot_index(
        &self,
        expression: PackedExpressionRef,
    ) -> Result<usize, KernelSolveError> {
        let definition = self
            .definitions
            .get(expression.owner.0 as usize)
            .ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel execution references missing definition {}",
                    expression.owner.0,
                ))
            })?;
        if expression.expression.0 >= definition.expressions.len {
            return Err(KernelSolveError::new(format!(
                "kernel execution references expression {}:{} outside local range 0..{}",
                expression.owner.0, expression.expression.0, definition.expressions.len,
            )));
        }
        (definition.execution_node_by_expression.start as usize)
            .checked_add(expression.expression.0 as usize)
            .filter(|index| *index < self.execution_node_by_expression.len())
            .ok_or_else(|| {
                KernelSolveError::new(
                    "kernel execution expression reverse-index coordinate overflows its column",
                )
            })
    }

    fn execution_call_slot_index(&self, call: PackedCallRef) -> Result<usize, KernelSolveError> {
        if call.is_none() {
            return Err(KernelSolveError::new(
                "kernel execution cannot publish the reserved missing-call reference",
            ));
        }
        let definition = self.definitions.get(call.owner.0 as usize).ok_or_else(|| {
            KernelSolveError::new(format!(
                "kernel execution references missing call owner {}",
                call.owner.0,
            ))
        })?;
        if call.ordinal >= definition.calls.len {
            return Err(KernelSolveError::new(format!(
                "kernel execution references call {}:{} outside local range 0..{}",
                call.owner.0, call.ordinal, definition.calls.len,
            )));
        }
        (definition.calls.start as usize)
            .checked_add(call.ordinal as usize)
            .filter(|index| *index < self.execution_node_by_call.len())
            .ok_or_else(|| {
                KernelSolveError::new(
                    "kernel execution call reverse-index coordinate overflows its column",
                )
            })
    }

    fn require_active_execution(
        &self,
        token: DefinitionExecutionTemplateToken,
    ) -> Result<ActiveExecutionTemplate, KernelSolveError> {
        self.active_execution
            .filter(|active| active.token == token)
            .ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel execution token {}:{} is stale or belongs to another active template",
                    token.owner.0, token.serial,
                ))
            })
    }

    /// Begin one callable template after all definition type headers exist.
    pub(crate) fn begin_execution_template(
        &mut self,
        owner: KernelOwnerId,
        result: PackedExpressionRef,
    ) -> Result<DefinitionExecutionTemplateToken, KernelSolveError> {
        if self.active_execution.is_some() {
            return Err(KernelSolveError::new(
                "kernel definition-code cannot begin a second execution template while one is active",
            ));
        }
        if result.owner != owner {
            return Err(KernelSolveError::new(format!(
                "kernel execution template owner {} has result in definition {}",
                owner.0, result.owner.0,
            )));
        }
        let _ = self.execution_expression_slot_index(result)?;
        let definition = self.definitions.get(owner.0 as usize).ok_or_else(|| {
            KernelSolveError::new(format!(
                "kernel execution template references missing owner {}",
                owner.0,
            ))
        })?;
        if definition.execution_result != MISSING_EXECUTION_ROW {
            return Err(KernelSolveError::new(format!(
                "kernel definition-code owner {} already has an execution template",
                owner.0,
            )));
        }
        let serial = self.next_execution_serial;
        self.next_execution_serial =
            self.next_execution_serial.checked_add(1).ok_or_else(|| {
                KernelSolveError::new(
                    "kernel definition-code execution token namespace exceeds u32",
                )
            })?;
        let token = DefinitionExecutionTemplateToken { owner, serial };
        self.active_execution = Some(ActiveExecutionTemplate {
            token,
            result,
            node_start: self.execution_nodes.len(),
        });
        Ok(token)
    }

    /// Append one already-normalized execution node without allocating an
    /// owned dependency or selector vector.
    pub(crate) fn push_execution_node(
        &mut self,
        token: DefinitionExecutionTemplateToken,
        expression: PackedExpressionRef,
        dependencies: &[PackedExpressionRef],
        call: Option<PackedCallRef>,
        selector: Option<(PackedExpressionRef, &[PackedExpressionRef])>,
    ) -> Result<(), KernelSolveError> {
        let active = self.require_active_execution(token)?;
        let expression_slot = self.execution_expression_slot_index(expression)?;
        if self.execution_node_by_expression[expression_slot] != MISSING_EXECUTION_ROW {
            return Err(KernelSolveError::new(format!(
                "kernel execution repeats expression {}:{} across templates",
                expression.owner.0, expression.expression.0,
            )));
        }
        if dependencies
            .windows(2)
            .any(|dependencies| dependencies[0] >= dependencies[1])
        {
            return Err(KernelSolveError::new(format!(
                "kernel execution node {}:{} dependencies are not strictly ordered",
                expression.owner.0, expression.expression.0,
            )));
        }
        for dependency in dependencies {
            let _ = self.execution_expression_slot_index(*dependency)?;
        }
        let call_slot = call
            .map(|call| self.execution_call_slot_index(call))
            .transpose()?;
        if call_slot.is_some_and(|slot| self.execution_node_by_call[slot] != MISSING_EXECUTION_ROW)
        {
            let call = call.expect("call slot exists only for a call");
            return Err(KernelSolveError::new(format!(
                "kernel execution repeats call {}:{} across nodes",
                call.owner.0, call.ordinal,
            )));
        }
        if let Some((input, arms)) = selector {
            let _ = self.execution_expression_slot_index(input)?;
            for (index, arm) in arms.iter().copied().enumerate() {
                let _ = self.execution_expression_slot_index(arm)?;
                if arms[..index].contains(&arm) {
                    return Err(KernelSolveError::new(format!(
                        "kernel execution node {}:{} repeats selector arm {}:{}",
                        expression.owner.0, expression.expression.0, arm.owner.0, arm.expression.0,
                    )));
                }
            }
        }

        let dependency_span = Span32::append(
            &mut self.execution_dependencies,
            dependencies.iter().copied(),
        )?;
        let selector = if let Some((input, arms)) = selector {
            let arms = Span32::append(&mut self.execution_selector_arms, arms.iter().copied())?;
            let index = u32::try_from(self.execution_selectors.len()).map_err(|_| {
                KernelSolveError::new("kernel definition-code execution selector count exceeds u32")
            })?;
            self.execution_selectors
                .push(PackedExecutionSelector { input, arms });
            index
        } else {
            MISSING_EXECUTION_ROW
        };
        let node_index = u32::try_from(self.execution_nodes.len()).map_err(|_| {
            KernelSolveError::new("kernel definition-code execution node count exceeds u32")
        })?;
        self.execution_nodes.push(PackedExecutionNode {
            expression,
            dependencies: dependency_span,
            call: call.unwrap_or(PackedCallRef::NONE),
            selector,
            template_owner: active.token.owner,
        });
        self.execution_node_by_expression[expression_slot] = node_index;
        if let Some(call_slot) = call_slot {
            self.execution_node_by_call[call_slot] = node_index;
        }
        Ok(())
    }

    /// Seal the active template after validating its complete membership.
    pub(crate) fn finish_execution_template(
        &mut self,
        token: DefinitionExecutionTemplateToken,
        calls: &[PackedCallRef],
    ) -> Result<(), KernelSolveError> {
        let active = self.require_active_execution(token)?;
        if calls.windows(2).any(|calls| calls[0] >= calls[1]) {
            return Err(KernelSolveError::new(format!(
                "kernel execution template owner {} calls are not strictly ordered",
                token.owner.0,
            )));
        }
        let node_end = self.execution_nodes.len();
        if active.node_start == node_end {
            return Err(KernelSolveError::new(format!(
                "kernel execution template owner {} has no nodes",
                token.owner.0,
            )));
        }
        let result_slot = self.execution_expression_slot_index(active.result)?;
        let result_node = self.execution_node_by_expression[result_slot];
        if result_node == MISSING_EXECUTION_ROW
            || (result_node as usize) < active.node_start
            || (result_node as usize) >= node_end
        {
            return Err(KernelSolveError::new(format!(
                "kernel execution template owner {} has no node for result expression {}",
                token.owner.0, active.result.expression.0,
            )));
        }

        for call in calls {
            let slot = self.execution_call_slot_index(*call)?;
            let node = self.execution_node_by_call[slot];
            if node == MISSING_EXECUTION_ROW
                || (node as usize) < active.node_start
                || (node as usize) >= node_end
            {
                return Err(KernelSolveError::new(format!(
                    "kernel execution template owner {} lists call {}:{} without a node",
                    token.owner.0, call.owner.0, call.ordinal,
                )));
            }
        }
        for (offset, node) in self.execution_nodes[active.node_start..node_end]
            .iter()
            .enumerate()
        {
            let node_index = active.node_start + offset;
            if node.template_owner != token.owner {
                return Err(KernelSolveError::new(format!(
                    "kernel execution node {node_index} belongs to template {} instead of {}",
                    node.template_owner.0, token.owner.0,
                )));
            }
            for dependency in node
                .dependencies
                .get(&self.execution_dependencies)
                .expect("builder-created execution dependency span is valid")
            {
                let slot = self.execution_expression_slot_index(*dependency)?;
                let dependency_node = self.execution_node_by_expression[slot];
                if dependency_node == MISSING_EXECUTION_ROW
                    || (dependency_node as usize) < active.node_start
                    || (dependency_node as usize) >= node_end
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel execution node {node_index} has dependency {}:{} outside template owner {}",
                        dependency.owner.0, dependency.expression.0, token.owner.0,
                    )));
                }
            }
            if !node.call.is_none() && calls.binary_search(&node.call).is_err() {
                return Err(KernelSolveError::new(format!(
                    "kernel execution node {node_index} call {}:{} is absent from template owner {}",
                    node.call.owner.0, node.call.ordinal, token.owner.0,
                )));
            }
            if node.selector != MISSING_EXECUTION_ROW {
                let selector = &self.execution_selectors[node.selector as usize];
                for expression in std::iter::once(selector.input).chain(
                    selector
                        .arms
                        .get(&self.execution_selector_arms)
                        .expect("builder-created selector-arm span is valid")
                        .iter()
                        .copied(),
                ) {
                    let slot = self.execution_expression_slot_index(expression)?;
                    let selector_node = self.execution_node_by_expression[slot];
                    if selector_node == MISSING_EXECUTION_ROW
                        || (selector_node as usize) < active.node_start
                        || (selector_node as usize) >= node_end
                    {
                        return Err(KernelSolveError::new(format!(
                            "kernel execution node {node_index} has selector expression {}:{} outside template owner {}",
                            expression.owner.0, expression.expression.0, token.owner.0,
                        )));
                    }
                }
            }
        }

        let execution_nodes = Span32::from_bounds(active.node_start, node_end, "execution-node")?;
        let execution_calls = Span32::append(&mut self.execution_calls, calls.iter().copied())?;
        let definition = self
            .definitions
            .get_mut(token.owner.0 as usize)
            .expect("active execution template owner was validated at begin");
        definition.execution_result = active.result.expression.0;
        definition.execution_nodes = execution_nodes;
        definition.execution_calls = execution_calls;
        self.active_execution = None;
        Ok(())
    }

    pub(crate) fn push(
        &mut self,
        owner: KernelOwnerId,
        result: KernelArtifactFlowTermV1,
        formal_flows: &[KernelArtifactFlowTermV1],
        expression_flows: &[KernelArtifactFlowTermV1],
        additional: DefinitionAdditionalTypeRoots<'_>,
    ) -> Result<(), KernelSolveError> {
        if owner.0 as usize != self.definitions.len() {
            return Err(KernelSolveError::new(format!(
                "kernel definition-code expected owner {} but received {}",
                self.definitions.len(),
                owner.0
            )));
        }
        let formals = Span32::append(&mut self.flows, formal_flows.iter().copied())?;
        let expressions = Span32::append(&mut self.flows, expression_flows.iter().copied())?;
        let execution_node_by_expression = Span32::append(
            &mut self.execution_node_by_expression,
            (0..expression_flows.len()).map(|_| MISSING_EXECUTION_ROW),
        )?;
        if additional.expression_flush_types.len() != expressions.len as usize
            || additional.expression_kind_types.len() != expressions.len as usize
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition-code owner {} has {} expression flows, {} FLUSH rows, and {} kind-type rows",
                owner.0,
                expressions.len,
                additional.expression_flush_types.len(),
                additional.expression_kind_types.len(),
            )));
        }
        if additional
            .resource_projection_requirements
            .windows(2)
            .any(|rows| rows[0].expression >= rows[1].expression)
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition-code owner {} resource projections are not in unique dense-expression order",
                owner.0,
            )));
        }
        let expression_flush_types = Span32::append(
            &mut self.expression_flush_types,
            additional.expression_flush_types.iter().copied(),
        )?;
        let expression_kind_types = Span32::append(
            &mut self.expression_kind_types,
            additional.expression_kind_types.iter().copied(),
        )?;
        let declaration_flows = Span32::append(
            &mut self.declaration_flows,
            additional.declaration_flows.iter().copied(),
        )?;
        let call_substitution_base =
            u32::try_from(self.call_substitutions.len()).map_err(|_| {
                KernelSolveError::new("kernel definition-code call-substitution start exceeds u32")
            })?;
        let _call_substitutions = Span32::append(
            &mut self.call_substitutions,
            additional.call_substitutions.iter().copied(),
        )?;
        let mut packed_calls = Vec::with_capacity(additional.calls.len());
        if additional
            .calls
            .windows(2)
            .any(|calls| calls[0].expression >= calls[1].expression)
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition-code owner {} calls are not in unique expression order",
                owner.0,
            )));
        }
        for call in additional.calls {
            if call.expression.0 >= expressions.len {
                return Err(KernelSolveError::new(format!(
                    "kernel definition-code owner {} call expression {} is outside its expression rows",
                    owner.0, call.expression.0,
                )));
            }
            let local_end = call
                .substitution_start
                .checked_add(call.substitution_len)
                .ok_or_else(|| {
                    KernelSolveError::new(
                        "kernel definition-code local call-substitution span overflows u32",
                    )
                })?;
            if local_end as usize > additional.call_substitutions.len() {
                return Err(KernelSolveError::new(
                    "kernel definition-code call-substitution span is outside its definition",
                ));
            }
            packed_calls.push(PackedCallFacts {
                expression: call.expression,
                substitutions: Span32 {
                    start: call_substitution_base
                        .checked_add(call.substitution_start)
                        .ok_or_else(|| {
                            KernelSolveError::new(
                                "kernel definition-code call-substitution span overflows u32",
                            )
                        })?,
                    len: call.substitution_len,
                },
                syntax_discriminated_result: call.syntax_discriminated_result,
            });
        }
        let calls = Span32::append(&mut self.calls, packed_calls)?;
        if self.execution_node_by_call.len() != calls.start as usize {
            return Err(KernelSolveError::new(
                "kernel definition-code call and execution-call indexes lost alignment",
            ));
        }
        self.execution_node_by_call
            .extend((0..calls.len).map(|_| MISSING_EXECUTION_ROW));
        let diagnostic_types = Span32::append(
            &mut self.diagnostic_types,
            additional.diagnostic_types.iter().copied(),
        )?;
        let source_payload_types = Span32::append(
            &mut self.source_payload_types,
            additional.source_payload_types.iter().copied(),
        )?;
        if additional
            .states
            .windows(2)
            .any(|states| states[0].input_ordinal >= states[1].input_ordinal)
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition-code owner {} states are not in unique input order",
                owner.0,
            )));
        }
        for state in additional.states {
            if state.input_ordinal as usize >= additional.state_input_count {
                return Err(KernelSolveError::new(format!(
                    "kernel definition-code owner {} state input {} is outside its {} authored state rows",
                    owner.0, state.input_ordinal, additional.state_input_count,
                )));
            }
        }
        let states = Span32::append(&mut self.states, additional.states.iter().copied())?;
        let list_item_types = Span32::append(
            &mut self.list_item_types,
            additional.list_item_types.iter().copied(),
        )?;
        let symbol_base = u32::try_from(self.resource_projection_symbols.len()).map_err(|_| {
            KernelSolveError::new(
                "kernel definition-code resource-projection symbol start exceeds u32",
            )
        })?;
        let _resource_projection_symbols = Span32::append(
            &mut self.resource_projection_symbols,
            additional.resource_projection_symbols.iter().copied(),
        )?;
        let origin_base = u32::try_from(self.resource_projection_origins.len()).map_err(|_| {
            KernelSolveError::new(
                "kernel definition-code resource-projection origin start exceeds u32",
            )
        })?;
        let mut packed_origins = Vec::with_capacity(additional.resource_projection_origins.len());
        for origin in additional.resource_projection_origins {
            let local_end = origin
                .payload_projection_start
                .checked_add(origin.payload_projection_len)
                .ok_or_else(|| {
                    KernelSolveError::new(
                        "kernel definition-code resource-origin path span overflows u32",
                    )
                })?;
            if local_end as usize > additional.resource_projection_symbols.len() {
                return Err(KernelSolveError::new(
                    "kernel definition-code resource-origin path is outside its definition",
                ));
            }
            packed_origins.push(PackedSourceRead {
                owner: origin.owner,
                source: origin.source,
                payload_projection: Span32 {
                    start: symbol_base
                        .checked_add(origin.payload_projection_start)
                        .ok_or_else(|| {
                            KernelSolveError::new(
                                "kernel definition-code resource-origin path start overflows u32",
                            )
                        })?,
                    len: origin.payload_projection_len,
                },
            });
        }
        let _resource_projection_origins =
            Span32::append(&mut self.resource_projection_origins, packed_origins)?;
        let mut packed_requirements =
            Vec::with_capacity(additional.resource_projection_requirements.len());
        for requirement in additional.resource_projection_requirements {
            if let Some(published) = requirement.published_expression {
                let base = expression_flows
                    .get(requirement.expression.0 as usize)
                    .ok_or_else(|| {
                        KernelSolveError::new(
                            "kernel definition-code published resource projection references a missing expression",
                        )
                    })?;
                if published.term != requirement.required_term || published.mode != base.mode {
                    return Err(KernelSolveError::new(
                        "kernel definition-code published resource projection disagrees with its required term or expression mode",
                    ));
                }
            }
            let projection_end = requirement
                .projection_start
                .checked_add(requirement.projection_len)
                .ok_or_else(|| {
                    KernelSolveError::new(
                        "kernel definition-code resource-requirement path span overflows u32",
                    )
                })?;
            if projection_end as usize > additional.resource_projection_symbols.len() {
                return Err(KernelSolveError::new(
                    "kernel definition-code resource-requirement path is outside its definition",
                ));
            }
            let origin_end = requirement
                .origin_start
                .checked_add(requirement.origin_len)
                .ok_or_else(|| {
                    KernelSolveError::new(
                        "kernel definition-code resource-requirement origin span overflows u32",
                    )
                })?;
            if origin_end as usize > additional.resource_projection_origins.len() {
                return Err(KernelSolveError::new(
                    "kernel definition-code resource-requirement origins are outside its definition",
                ));
            }
            packed_requirements.push(PackedResourceProjectionRequirement {
                expression: requirement.expression,
                target: requirement.target,
                projection: Span32 {
                    start: symbol_base.checked_add(requirement.projection_start).ok_or_else(
                        || {
                            KernelSolveError::new(
                                "kernel definition-code resource-requirement path start overflows u32",
                            )
                        },
                    )?,
                    len: requirement.projection_len,
                },
                origins: Span32 {
                    start: origin_base.checked_add(requirement.origin_start).ok_or_else(|| {
                        KernelSolveError::new(
                            "kernel definition-code resource-requirement origin start overflows u32",
                        )
                    })?,
                    len: requirement.origin_len,
                },
                required_term: requirement.required_term,
                published_expression: requirement.published_expression,
            });
        }
        let resource_projection_requirements = Span32::append(
            &mut self.resource_projection_requirements,
            packed_requirements,
        )?;
        let alpha_variables = Span32::append(
            &mut self.alpha_variables,
            additional.alpha_variables.iter().copied(),
        )?;
        self.definitions.push(DefinitionCode {
            effect_summary: additional.effect_summary,
            basis_fingerprint_v14: additional.basis_fingerprint_v14,
            result,
            formals,
            expressions,
            expression_flush_types,
            expression_kind_types,
            declaration_flows,
            calls,
            diagnostic_types,
            source_payload_types,
            states,
            list_item_types,
            resource_projection_requirements,
            alpha_variables,
            execution_node_by_expression,
            execution_result: MISSING_EXECUTION_ROW,
            execution_nodes: Span32::default(),
            execution_calls: Span32::default(),
            stable_digest: additional.stable_digest,
        });
        Ok(())
    }

    pub(crate) fn finish(
        self,
        types: Arc<FrozenTypeStore>,
    ) -> Result<DefinitionCodeStore, KernelSolveError> {
        if let Some(active) = self.active_execution {
            return Err(KernelSolveError::new(format!(
                "kernel definition-code execution template owner {} was not finished",
                active.token.owner.0,
            )));
        }
        let store = DefinitionCodeStore {
            types,
            definitions: self.definitions.into_boxed_slice(),
            flows: self.flows.into_boxed_slice(),
            expression_flush_types: self.expression_flush_types.into_boxed_slice(),
            expression_kind_types: self.expression_kind_types.into_boxed_slice(),
            declaration_flows: self.declaration_flows.into_boxed_slice(),
            calls: self.calls.into_boxed_slice(),
            call_substitutions: self.call_substitutions.into_boxed_slice(),
            diagnostic_types: self.diagnostic_types.into_boxed_slice(),
            source_payload_types: self.source_payload_types.into_boxed_slice(),
            states: self.states.into_boxed_slice(),
            list_item_types: self.list_item_types.into_boxed_slice(),
            resource_projection_requirements: self
                .resource_projection_requirements
                .into_boxed_slice(),
            resource_projection_origins: self.resource_projection_origins.into_boxed_slice(),
            resource_projection_symbols: self.resource_projection_symbols.into_boxed_slice(),
            alpha_variables: self.alpha_variables.into_boxed_slice(),
            execution_nodes: self.execution_nodes.into_boxed_slice(),
            execution_dependencies: self.execution_dependencies.into_boxed_slice(),
            execution_selectors: self.execution_selectors.into_boxed_slice(),
            execution_selector_arms: self.execution_selector_arms.into_boxed_slice(),
            execution_calls: self.execution_calls.into_boxed_slice(),
            execution_node_by_expression: self.execution_node_by_expression.into_boxed_slice(),
        };
        store.validate_execution()?;
        #[cfg(debug_assertions)]
        store.validate()?;
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypeTermArena;

    fn push_test_definition(
        builder: &mut DefinitionCodeBuilder,
        owner: u32,
        flow: KernelArtifactFlowTermV1,
        expression_count: usize,
        call_count: usize,
        source_count: usize,
        state_count: usize,
        list_count: usize,
    ) {
        let expressions = vec![flow; expression_count];
        let calls = (0..call_count)
            .map(|ordinal| PackedCallFactsInput {
                expression: crate::KernelExpressionId(
                    u32::try_from(ordinal).expect("test call count fits u32"),
                ),
                substitution_start: 0,
                substitution_len: 0,
                syntax_discriminated_result: false,
            })
            .collect::<Vec<_>>();
        let expression_flush_types = vec![None; expression_count];
        let expression_kind_types = vec![None; expression_count];
        let source_payload_types = vec![flow.term; source_count];
        let states = (0..state_count)
            .map(|ordinal| {
                PackedPublishedState::new(
                    u32::try_from(ordinal).expect("test state count fits u32"),
                    None,
                    PackedFlow {
                        mode: flow.mode,
                        term: flow.term,
                    },
                )
            })
            .collect::<Vec<_>>();
        let list_item_types = vec![flow.term; list_count];
        builder
            .push(
                KernelOwnerId(owner),
                flow,
                &[],
                &expressions,
                DefinitionAdditionalTypeRoots {
                    effect_summary: crate::KernelEffectSummary::default(),
                    basis_fingerprint_v14: [owner as u8; 32],
                    expression_flush_types: &expression_flush_types,
                    expression_kind_types: &expression_kind_types,
                    declaration_flows: &[],
                    calls: &calls,
                    call_substitutions: &[],
                    diagnostic_types: &[],
                    source_payload_types: &source_payload_types,
                    state_input_count: state_count,
                    states: &states,
                    list_item_types: &list_item_types,
                    resource_projection_requirements: &[],
                    resource_projection_origins: &[],
                    resource_projection_symbols: &[],
                    alpha_variables: &[],
                    stable_digest: [owner as u8; 32],
                },
            )
            .unwrap();
    }

    #[test]
    fn span32_appends_and_borrows_exact_rows() {
        let mut rows = vec![1_u32];
        let span = Span32::append(&mut rows, [2, 3]).expect("small rows fit");
        assert_eq!(span.get(&rows), Some(&[2, 3][..]));
    }

    #[test]
    fn span32_rejects_out_of_bounds_borrow() {
        let span = Span32 { start: 2, len: 2 };
        assert_eq!(span.get(&[1_u32, 2, 3]), None);
    }

    #[test]
    fn packed_execution_rows_borrow_cross_definition_facts_without_rich_dtos() {
        let arena = TypeTermArena::new();
        let unknown = arena.unknown();
        let flow = KernelArtifactFlowTermV1 {
            mode: FlowMode::Continuous,
            term: unknown,
            stable_digest: [0; 32],
            runtime_erased_digest: [0; 32],
        };
        let mut builder = DefinitionCodeBuilder::with_capacity(2, 4, 0);
        push_test_definition(&mut builder, 0, flow, 3, 0, 1, 1, 1);
        push_test_definition(&mut builder, 1, flow, 1, 1, 0, 0, 0);
        builder.reserve_execution(3, 2, 1, 1, 1);

        let root = PackedExpressionRef::new(KernelOwnerId(0), crate::KernelExpressionId(0));
        let arm = PackedExpressionRef::new(KernelOwnerId(0), crate::KernelExpressionId(1));
        let external = PackedExpressionRef::new(KernelOwnerId(1), crate::KernelExpressionId(0));
        let call = PackedCallRef::new(KernelOwnerId(1), 0);
        let token = builder
            .begin_execution_template(KernelOwnerId(0), root)
            .unwrap();
        builder
            .push_execution_node(token, arm, &[], None, None)
            .unwrap();
        builder
            .push_execution_node(token, external, &[], Some(call), None)
            .unwrap();
        builder
            .push_execution_node(
                token,
                root,
                &[arm, external],
                None,
                Some((external, &[arm])),
            )
            .unwrap();
        builder.finish_execution_template(token, &[call]).unwrap();

        let store = builder.finish(Arc::new(arena.freeze())).unwrap();
        let definition = store.definition(KernelOwnerId(0)).unwrap();
        assert!(definition.has_execution_template());
        assert_eq!(definition.execution_result(), Some(root));
        assert_eq!(definition.execution_calls(), &[call]);

        let template = definition.execution_template().unwrap();
        assert_eq!(template.owner(), KernelOwnerId(0));
        assert_eq!(template.result(), root);
        assert_eq!(template.nodes().len(), 3);
        assert_eq!(template.calls(), &[call]);
        assert_eq!(template.source_count(), 1);
        assert_eq!(template.state_count(), 1);
        assert_eq!(template.list_count(), 1);

        let root_node = definition
            .execution_node_for_local_expression(crate::KernelExpressionId(0))
            .unwrap();
        assert_eq!(root_node.template_owner(), KernelOwnerId(0));
        assert_eq!(root_node.expression(), root);
        assert_eq!(root_node.dependencies(), &[arm, external]);
        assert_eq!(root_node.call(), None);
        let selector = root_node.selector().unwrap();
        assert_eq!(selector.input(), external);
        assert_eq!(selector.arms(), &[arm]);

        let external_node = store.execution_node(external).unwrap();
        assert_eq!(external_node.template_owner(), KernelOwnerId(0));
        assert_eq!(external_node.call(), Some(call));
        let external_definition = store.definition(KernelOwnerId(1)).unwrap();
        assert!(!external_definition.has_execution_template());
        assert!(
            external_definition
                .execution_node_for_local_expression(crate::KernelExpressionId(0))
                .is_some()
        );
        assert!(
            definition
                .execution_node_for_local_expression(crate::KernelExpressionId(2))
                .is_none()
        );
    }

    #[test]
    fn execution_template_rejects_dangling_dependencies_at_finish() {
        let arena = TypeTermArena::new();
        let unknown = arena.unknown();
        let flow = KernelArtifactFlowTermV1 {
            mode: FlowMode::Continuous,
            term: unknown,
            stable_digest: [0; 32],
            runtime_erased_digest: [0; 32],
        };
        let mut builder = DefinitionCodeBuilder::with_capacity(1, 2, 0);
        push_test_definition(&mut builder, 0, flow, 2, 0, 0, 0, 0);
        let root = PackedExpressionRef::new(KernelOwnerId(0), crate::KernelExpressionId(0));
        let missing = PackedExpressionRef::new(KernelOwnerId(0), crate::KernelExpressionId(1));
        let token = builder
            .begin_execution_template(KernelOwnerId(0), root)
            .unwrap();
        builder
            .push_execution_node(token, root, &[missing], None, None)
            .unwrap();
        let error = builder.finish_execution_template(token, &[]).unwrap_err();
        assert!(error.to_string().contains("outside template owner 0"));
    }

    #[test]
    fn execution_builder_rejects_duplicate_nodes_arms_and_calls() {
        let arena = TypeTermArena::new();
        let unknown = arena.unknown();
        let flow = KernelArtifactFlowTermV1 {
            mode: FlowMode::Continuous,
            term: unknown,
            stable_digest: [0; 32],
            runtime_erased_digest: [0; 32],
        };
        let mut builder = DefinitionCodeBuilder::with_capacity(1, 2, 0);
        push_test_definition(&mut builder, 0, flow, 2, 1, 0, 0, 0);
        let root = PackedExpressionRef::new(KernelOwnerId(0), crate::KernelExpressionId(0));
        let arm = PackedExpressionRef::new(KernelOwnerId(0), crate::KernelExpressionId(1));
        let call = PackedCallRef::new(KernelOwnerId(0), 0);
        let token = builder
            .begin_execution_template(KernelOwnerId(0), root)
            .unwrap();
        let duplicate_arm = builder
            .push_execution_node(token, arm, &[], None, Some((root, &[root, root])))
            .unwrap_err();
        assert!(duplicate_arm.to_string().contains("repeats selector arm"));
        builder
            .push_execution_node(token, arm, &[], Some(call), None)
            .unwrap();
        let duplicate_node = builder
            .push_execution_node(token, arm, &[], None, None)
            .unwrap_err();
        assert!(duplicate_node.to_string().contains("repeats expression"));
        builder
            .push_execution_node(token, root, &[arm], None, None)
            .unwrap();
        let duplicate_call = builder
            .finish_execution_template(token, &[call, call])
            .unwrap_err();
        assert!(duplicate_call.to_string().contains("not strictly ordered"));
    }

    #[test]
    fn hidden_type_roots_share_one_definition_alpha_authority() {
        let mut arena = TypeTermArena::new();
        let unknown = arena.unknown();
        let declaration_variable = arena.variable(TypeVariableId(7));
        let state_variable = arena.variable(TypeVariableId(9));
        let flow = KernelArtifactFlowTermV1 {
            mode: FlowMode::Continuous,
            term: unknown,
            stable_digest: [0; 32],
            runtime_erased_digest: [0; 32],
        };
        let expressions = [flow];
        let flushes = [None];
        let kind_types = [None];
        let declarations = [Some(PackedFlow {
            mode: FlowMode::Continuous,
            term: declaration_variable,
        })];
        let states = [PackedPublishedState::new(
            0,
            None,
            PackedFlow {
                mode: FlowMode::TickPresent,
                term: state_variable,
            },
        )];
        let alpha = [TypeVariableId(7), TypeVariableId(9)];
        let mut builder = DefinitionCodeBuilder::with_capacity(1, 1, 2);
        builder
            .push(
                KernelOwnerId(0),
                flow,
                &[],
                &expressions,
                DefinitionAdditionalTypeRoots {
                    effect_summary: crate::KernelEffectSummary::default(),
                    basis_fingerprint_v14: [3; 32],
                    expression_flush_types: &flushes,
                    expression_kind_types: &kind_types,
                    declaration_flows: &declarations,
                    calls: &[],
                    call_substitutions: &[],
                    diagnostic_types: &[],
                    source_payload_types: &[],
                    state_input_count: states.len(),
                    states: &states,
                    list_item_types: &[],
                    resource_projection_requirements: &[],
                    resource_projection_origins: &[],
                    resource_projection_symbols: &[],
                    alpha_variables: &alpha,
                    stable_digest: [3; 32],
                },
            )
            .unwrap();
        let store = builder.finish(Arc::new(arena.freeze())).unwrap();
        let definition = store.definition(KernelOwnerId(0)).unwrap();
        assert_eq!(
            definition.materialize_declaration_flow(0),
            Some(FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(TypeVar(0)),
            })
        );
        assert_eq!(
            definition.materialize_state_flow(0),
            Some(FlowType {
                mode: FlowMode::TickPresent,
                ty: Type::Var(TypeVar(1)),
            })
        );

        let mut cache = store.materialization_cache();
        let mut linked = definition.linked_materializer(&mut cache, 17);
        assert_eq!(
            linked.materialize_declaration_flow(0),
            Some(FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(TypeVar(17)),
            })
        );
        assert_eq!(
            linked.materialize_state_flow(0),
            Some(FlowType {
                mode: FlowMode::TickPresent,
                ty: Type::Var(TypeVar(18)),
            })
        );
    }

    #[test]
    fn definition_builder_rejects_invalid_call_and_state_coordinates() {
        let arena = TypeTermArena::new();
        let unknown = arena.unknown();
        let flow = KernelArtifactFlowTermV1 {
            mode: FlowMode::Continuous,
            term: unknown,
            stable_digest: [0; 32],
            runtime_erased_digest: [0; 32],
        };
        let invalid_call = [PackedCallFactsInput {
            expression: crate::KernelExpressionId(1),
            substitution_start: 0,
            substitution_len: 0,
            syntax_discriminated_result: false,
        }];
        let mut builder = DefinitionCodeBuilder::with_capacity(1, 1, 0);
        let error = builder
            .push(
                KernelOwnerId(0),
                flow,
                &[],
                &[flow],
                DefinitionAdditionalTypeRoots {
                    effect_summary: crate::KernelEffectSummary::default(),
                    basis_fingerprint_v14: [0; 32],
                    expression_flush_types: &[None],
                    expression_kind_types: &[None],
                    declaration_flows: &[],
                    calls: &invalid_call,
                    call_substitutions: &[],
                    diagnostic_types: &[],
                    source_payload_types: &[],
                    state_input_count: 0,
                    states: &[],
                    list_item_types: &[],
                    resource_projection_requirements: &[],
                    resource_projection_origins: &[],
                    resource_projection_symbols: &[],
                    alpha_variables: &[],
                    stable_digest: [0; 32],
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("outside its expression rows"));

        let invalid_state = [PackedPublishedState::new(
            1,
            None,
            PackedFlow {
                mode: FlowMode::Continuous,
                term: unknown,
            },
        )];
        let mut builder = DefinitionCodeBuilder::with_capacity(1, 1, 0);
        let error = builder
            .push(
                KernelOwnerId(0),
                flow,
                &[],
                &[flow],
                DefinitionAdditionalTypeRoots {
                    effect_summary: crate::KernelEffectSummary::default(),
                    basis_fingerprint_v14: [0; 32],
                    expression_flush_types: &[None],
                    expression_kind_types: &[None],
                    declaration_flows: &[],
                    calls: &[],
                    call_substitutions: &[],
                    diagnostic_types: &[],
                    source_payload_types: &[],
                    state_input_count: 1,
                    states: &invalid_state,
                    list_item_types: &[],
                    resource_projection_requirements: &[],
                    resource_projection_origins: &[],
                    resource_projection_symbols: &[],
                    alpha_variables: &[],
                    stable_digest: [0; 32],
                },
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside its 1 authored state rows")
        );
    }

    #[test]
    fn resource_projection_rows_require_dense_expression_order() {
        let arena = TypeTermArena::new();
        let unknown = arena.unknown();
        let flow = KernelArtifactFlowTermV1 {
            mode: FlowMode::Continuous,
            term: unknown,
            stable_digest: [0; 32],
            runtime_erased_digest: [0; 32],
        };
        let requirements = [
            PackedResourceProjectionRequirementInput {
                expression: crate::KernelExpressionId(1),
                target: crate::KernelDeclarationReference::Local(crate::KernelDeclarationId(0)),
                projection_start: 0,
                projection_len: 0,
                origin_start: 0,
                origin_len: 0,
                required_term: unknown,
                published_expression: None,
            },
            PackedResourceProjectionRequirementInput {
                expression: crate::KernelExpressionId(0),
                target: crate::KernelDeclarationReference::Local(crate::KernelDeclarationId(0)),
                projection_start: 0,
                projection_len: 0,
                origin_start: 0,
                origin_len: 0,
                required_term: unknown,
                published_expression: None,
            },
        ];
        let mut builder = DefinitionCodeBuilder::with_capacity(1, 2, 0);
        let error = builder
            .push(
                KernelOwnerId(0),
                flow,
                &[],
                &[flow, flow],
                DefinitionAdditionalTypeRoots {
                    effect_summary: crate::KernelEffectSummary::default(),
                    basis_fingerprint_v14: [0; 32],
                    expression_flush_types: &[None, None],
                    expression_kind_types: &[None, None],
                    declaration_flows: &[],
                    calls: &[],
                    call_substitutions: &[],
                    diagnostic_types: &[],
                    source_payload_types: &[],
                    state_input_count: 0,
                    states: &[],
                    list_item_types: &[],
                    resource_projection_requirements: &requirements,
                    resource_projection_origins: &[],
                    resource_projection_symbols: &[],
                    alpha_variables: &[],
                    stable_digest: [0; 32],
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("unique dense-expression order"));
    }
}
