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
}

/// One definition header in the permanent packed compiler output.
///
/// This first consumed slice centralizes all solver-owned flow roots and their
/// definition-local alpha namespace. Subsequent M1 cuts add the remaining row
/// families to the same store; they must not create another parallel sidecar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DefinitionCode {
    result: KernelArtifactFlowTermV1,
    formals: Span32,
    expressions: Span32,
    expression_flush_types: Span32,
    expression_kind_types: Span32,
    declaration_flows: Span32,
    calls: Span32,
    diagnostic_types: Span32,
    source_payload_types: Span32,
    state_flows: Span32,
    list_item_types: Span32,
    resource_projection_requirements: Span32,
    alpha_variables: Span32,
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
    state_flows: Box<[PackedFlow]>,
    list_item_types: Box<[crate::TypeTermId]>,
    resource_projection_requirements: Box<[PackedResourceProjectionRequirement]>,
    resource_projection_origins: Box<[PackedSourceRead]>,
    resource_projection_symbols: Box<[SymbolId]>,
    alpha_variables: Box<[TypeVariableId]>,
}

impl DefinitionCodeStore {
    pub(crate) fn type_store(&self) -> &Arc<FrozenTypeStore> {
        &self.types
    }

    pub fn definition_count(&self) -> usize {
        self.definitions.len()
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
                ("state-flow", definition.state_flows, self.state_flows.len()),
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
            for call in definition
                .calls
                .get(&self.calls)
                .expect("validated call span")
            {
                call.substitutions
                    .get(&self.call_substitutions)
                    .ok_or_else(|| {
                        KernelSolveError::new(format!(
                            "kernel definition-code owner {owner} has an invalid call-substitution span"
                        ))
                    })?;
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
                    .state_flows
                    .get(&self.state_flows)
                    .expect("validated state span")
                    .iter()
                    .map(|flow| flow.term),
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

impl<'a> DefinitionCodeRef<'a> {
    pub const fn owner(self) -> KernelOwnerId {
        self.owner
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
            .state_flows
            .get(&self.store.state_flows)
            .expect("sealed definition-code state-flow span is valid")
            .get(ordinal)
            .copied()
            .map(|flow| self.materialize_packed_flow(flow))
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
            .state_flows
            .get(&self.code.store.state_flows)
            .expect("sealed definition-code state-flow span is valid")
            .get(ordinal)
            .copied()?;
        Some(self.materialize_packed_flow(flow))
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
    substitutions: Span32,
    syntax_discriminated_result: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackedCallFactsInput {
    pub(crate) substitution_start: u32,
    pub(crate) substitution_len: u32,
    pub(crate) syntax_discriminated_result: bool,
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
    pub(crate) expression_flush_types: &'a [Option<crate::TypeTermId>],
    pub(crate) expression_kind_types: &'a [Option<crate::TypeTermId>],
    pub(crate) declaration_flows: &'a [Option<PackedFlow>],
    pub(crate) calls: &'a [PackedCallFactsInput],
    pub(crate) call_substitutions: &'a [PackedCallTypeSubstitution],
    pub(crate) diagnostic_types: &'a [Option<PackedDiagnosticTypes>],
    pub(crate) source_payload_types: &'a [crate::TypeTermId],
    pub(crate) state_flows: &'a [PackedFlow],
    pub(crate) list_item_types: &'a [crate::TypeTermId],
    pub(crate) resource_projection_requirements: &'a [PackedResourceProjectionRequirementInput],
    pub(crate) resource_projection_origins: &'a [PackedSourceReadInput],
    pub(crate) resource_projection_symbols: &'a [SymbolId],
    pub(crate) alpha_variables: &'a [TypeVariableId],
    pub(crate) stable_digest: [u8; 32],
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
    state_flows: Vec<PackedFlow>,
    list_item_types: Vec<crate::TypeTermId>,
    resource_projection_requirements: Vec<PackedResourceProjectionRequirement>,
    resource_projection_origins: Vec<PackedSourceRead>,
    resource_projection_symbols: Vec<SymbolId>,
    alpha_variables: Vec<TypeVariableId>,
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
            state_flows: Vec::new(),
            list_item_types: Vec::new(),
            resource_projection_requirements: Vec::new(),
            resource_projection_origins: Vec::new(),
            resource_projection_symbols: Vec::new(),
            alpha_variables: Vec::with_capacity(alpha_variables),
        }
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
        for call in additional.calls {
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
        let diagnostic_types = Span32::append(
            &mut self.diagnostic_types,
            additional.diagnostic_types.iter().copied(),
        )?;
        let source_payload_types = Span32::append(
            &mut self.source_payload_types,
            additional.source_payload_types.iter().copied(),
        )?;
        let state_flows = Span32::append(
            &mut self.state_flows,
            additional.state_flows.iter().copied(),
        )?;
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
            result,
            formals,
            expressions,
            expression_flush_types,
            expression_kind_types,
            declaration_flows,
            calls,
            diagnostic_types,
            source_payload_types,
            state_flows,
            list_item_types,
            resource_projection_requirements,
            alpha_variables,
            stable_digest: additional.stable_digest,
        });
        Ok(())
    }

    pub(crate) fn finish(
        self,
        types: Arc<FrozenTypeStore>,
    ) -> Result<DefinitionCodeStore, KernelSolveError> {
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
            state_flows: self.state_flows.into_boxed_slice(),
            list_item_types: self.list_item_types.into_boxed_slice(),
            resource_projection_requirements: self
                .resource_projection_requirements
                .into_boxed_slice(),
            resource_projection_origins: self.resource_projection_origins.into_boxed_slice(),
            resource_projection_symbols: self.resource_projection_symbols.into_boxed_slice(),
            alpha_variables: self.alpha_variables.into_boxed_slice(),
        };
        #[cfg(debug_assertions)]
        store.validate()?;
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypeTermArena;

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
        let states = [PackedFlow {
            mode: FlowMode::TickPresent,
            term: state_variable,
        }];
        let alpha = [TypeVariableId(7), TypeVariableId(9)];
        let mut builder = DefinitionCodeBuilder::with_capacity(1, 1, 2);
        builder
            .push(
                KernelOwnerId(0),
                flow,
                &[],
                &expressions,
                DefinitionAdditionalTypeRoots {
                    expression_flush_types: &flushes,
                    expression_kind_types: &kind_types,
                    declaration_flows: &declarations,
                    calls: &[],
                    call_substitutions: &[],
                    diagnostic_types: &[],
                    source_payload_types: &[],
                    state_flows: &states,
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
                    expression_flush_types: &[None, None],
                    expression_kind_types: &[None, None],
                    declaration_flows: &[],
                    calls: &[],
                    call_substitutions: &[],
                    diagnostic_types: &[],
                    source_payload_types: &[],
                    state_flows: &[],
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
