use crate::machine_plan_backend::{ValueIndex, lower_document_runtime_expression};
use boon_checked::{CheckedTypeSubstitution, Type, TypeVar, is_renderable_type};
use boon_ir::ErasedProgram;
use boon_plan::*;
use boon_semantic::program_core as ir;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn compile_document_plan(
    program: &ErasedProgram,
    value_index: &ValueIndex,
    root_computation_fields: &BTreeSet<FieldId>,
    row_expressions: &mut PlanRowExpressionArena,
    machine_constants: &mut Vec<PlanConstant>,
    distributed_expression_refs: &BTreeMap<ir::ExecutableExprId, ValueRef>,
    distributed_path_refs: &BTreeMap<String, ValueRef>,
) -> Result<Option<DocumentPlan>, PlanError> {
    let mut roots = program.output_values.iter().filter(|output| {
        matches!(
            output.contract,
            ir::SemanticOutputContractKind::RetainedVisual { .. }
        )
    });
    let Some(output) = roots.next() else {
        return Ok(None);
    };
    if roots.next().is_some() {
        return Err(PlanError::new(
            "MachinePlan can contain only one document or scene output root",
        ));
    }
    DocumentCompiler::new(
        program,
        value_index,
        root_computation_fields,
        row_expressions,
        machine_constants,
        distributed_expression_refs,
        distributed_path_refs,
    )?
    .compile(output)
    .map(Some)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GlobalValue {
    State(StateId),
    Field(FieldId),
    List(ListId),
    Source(SourceId),
    Inline(ir::ExecutableExprId),
}

#[derive(Clone, Copy)]
struct ContextualMaterializationInfo {
    id: usize,
    operation: ir::ContextualOperationKind,
    source: ir::ExecutableExprId,
    body: ir::ExecutableExprId,
    result_kind: ir::MaterializationResultKind,
    row_local: ir::MaterializationLocalId,
    owner: ir::StaticOwnerId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CompileContext {
    cache_scope: usize,
    call_instance: Option<usize>,
    stable_owner: Option<ir::StaticOwnerId>,
    owner_function: Option<DocumentFunctionId>,
    materialization_locals:
        BTreeMap<(ir::StaticOwnerId, ir::MaterializationLocalId), DocumentParameterId>,
    locals: BTreeMap<ir::ExecutableLocalBindingId, DocumentLocalId>,
    function_parameters: BTreeMap<ir::ExecutableParameterId, DocumentFunctionParameterBinding>,
    type_substitutions: BTreeMap<TypeVar, Type>,
    pattern_bindings: BTreeMap<String, PatternBindingContext>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DocumentFunctionParameterBinding {
    value: DocumentExprId,
    ty: Type,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PatternBindingContext {
    selector: usize,
    projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StaticDocumentSelector {
    Text(String),
    Number(boon_data::ExactNumber),
    Bits(boon_data::Bits),
    Tag(String),
}

impl StaticDocumentSelector {
    fn matches(&self, pattern: &boon_checked::CheckedMatchPattern) -> bool {
        use boon_checked::CheckedMatchPattern;
        match (self, pattern) {
            (_, CheckedMatchPattern::Wildcard | CheckedMatchPattern::Binding { .. }) => true,
            (Self::Text(actual), CheckedMatchPattern::Text { value }) => actual == value,
            (Self::Number(actual), CheckedMatchPattern::Number { value }) => actual == value,
            (Self::Bits(actual), CheckedMatchPattern::Bits { value }) => actual == value,
            (Self::Tag(actual), CheckedMatchPattern::Tag { name, .. }) => actual == name,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OrdinaryDocumentFunctionKey {
    function: ir::FunctionId,
    call_instance: Option<usize>,
    stable_owner: Option<ir::StaticOwnerId>,
    materialization_locals:
        BTreeMap<(ir::StaticOwnerId, ir::MaterializationLocalId), DocumentParameterId>,
    locals: BTreeMap<ir::ExecutableLocalBindingId, DocumentLocalId>,
    type_substitutions: BTreeMap<TypeVar, Type>,
    pattern_bindings: BTreeMap<String, PatternBindingContext>,
    parameter_types: Vec<Type>,
    static_parameter_selectors: Vec<Option<StaticDocumentSelector>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OrdinaryDocumentFunction {
    key: OrdinaryDocumentFunctionKey,
    id: DocumentFunctionId,
    parameters: Vec<DocumentParameterId>,
    parameter_values: Vec<DocumentExprId>,
    call_sites: Vec<(DocumentExprId, Option<DocumentFunctionId>)>,
}

struct ExecutableCall<'a> {
    expression: &'a ir::ExecutableExpression,
    checked_call: boon_checked::CheckedCallId,
    callable_kind: ir::ExecutableCallableKind,
    function: &'a str,
    instance: Option<usize>,
    arguments: &'a [ir::ExecutableCallArgument],
    contexts: &'a [ir::ExecutableCallContextId],
    context_ordinals: &'a [usize],
}

struct DocumentCompiler<'a> {
    program: &'a ErasedProgram,
    value_index: &'a ValueIndex,
    root_computation_fields: &'a BTreeSet<FieldId>,
    row_expressions: &'a mut PlanRowExpressionArena,
    machine_constants: &'a mut Vec<PlanConstant>,
    globals_by_storage: BTreeMap<ir::ErasedBindingId, GlobalValue>,
    distributed_by_expression: BTreeMap<ir::ExecutableExprId, ValueRef>,
    distributed_by_path: BTreeMap<String, ValueRef>,
    materializations_by_id: BTreeMap<usize, ContextualMaterializationInfo>,
    names: Vec<String>,
    name_ids: BTreeMap<String, DocumentNameId>,
    constants: Vec<DocumentConstant>,
    expressions: Vec<DocumentExpr>,
    expression_cache: BTreeMap<(usize, usize), DocumentExprId>,
    projected_expression_cache: BTreeMap<(usize, usize, Vec<String>), DocumentExprId>,
    static_expression_selectors: BTreeMap<DocumentExprId, StaticDocumentSelector>,
    functions: Vec<DocumentFunction>,
    function_ids: BTreeSet<DocumentFunctionId>,
    templates: Vec<DocumentTemplate>,
    template_ids: BTreeSet<DocumentTemplateId>,
    templates_by_node_expression:
        BTreeMap<(Option<usize>, ir::ExecutableExprId), DocumentTemplateId>,
    template_contexts: BTreeMap<DocumentTemplateId, CompileContext>,
    materializations: Vec<DocumentMaterialization>,
    materialization_ids: BTreeSet<DocumentMaterializationId>,
    materializations_in_progress: BTreeSet<usize>,
    compiled_materializations: BTreeSet<usize>,
    compiled_paths: BTreeMap<(Option<ScopeId>, String), DocumentExprId>,
    compile_stack: Vec<ir::ExecutableExprId>,
    active_ordinary_functions: BTreeSet<ir::FunctionId>,
    ordinary_function_overlay_requirements: BTreeMap<ir::FunctionId, bool>,
    ordinary_functions: Vec<OrdinaryDocumentFunction>,
    next_function_id: usize,
    next_cache_scope: usize,
    next_local: usize,
}

impl<'a> DocumentCompiler<'a> {
    fn materialization_resource_read(
        &self,
        owner: ir::StaticOwnerId,
        local: ir::MaterializationLocalId,
        projection: &[String],
    ) -> Result<Option<DocumentRead>, PlanError> {
        let definition = self
            .program
            .scope_index
            .locals
            .iter()
            .find(|definition| definition.owner == owner && definition.local == local)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "document expression references missing materialization local {owner}:{}",
                    local.0
                ))
            })?;
        let consumed = definition
            .members
            .iter()
            .filter(|member| projection.starts_with(&member.path))
            .map(|member| member.path.len())
            .max()
            .unwrap_or(0);
        let candidates = definition
            .members
            .iter()
            .filter(|member| member.path.len() == consumed && projection.starts_with(&member.path))
            .collect::<Vec<_>>();
        let [member] = candidates.as_slice() else {
            return Ok(None);
        };
        let rest = &projection[consumed..];
        match &member.target {
            ir::ErasedLocalMemberTarget::Sources(sources) if rest.is_empty() => {
                Ok(Some(DocumentRead::Sources {
                    sources: sources.iter().map(|source| SourceId(source.0)).collect(),
                }))
            }
            ir::ErasedLocalMemberTarget::State(state) if rest.is_empty() => {
                if self
                    .program
                    .state_cells
                    .get(state.as_usize())
                    .is_some_and(|candidate| candidate.id == *state && candidate.scope_id.is_some())
                {
                    return Ok(None);
                }
                Ok(Some(DocumentRead::State {
                    state: StateId(state.0),
                }))
            }
            ir::ErasedLocalMemberTarget::Sources(_) | ir::ErasedLocalMemberTarget::State(_) => {
                Err(PlanError::new(format!(
                    "document materialization resource `{}` cannot project `{}` directly",
                    member.path.join("."),
                    rest.join(".")
                )))
            }
            ir::ErasedLocalMemberTarget::Field(_) => Ok(None),
        }
    }

    fn new(
        program: &'a ErasedProgram,
        value_index: &'a ValueIndex,
        root_computation_fields: &'a BTreeSet<FieldId>,
        row_expressions: &'a mut PlanRowExpressionArena,
        machine_constants: &'a mut Vec<PlanConstant>,
        distributed_expression_refs: &'a BTreeMap<ir::ExecutableExprId, ValueRef>,
        distributed_path_refs: &'a BTreeMap<String, ValueRef>,
    ) -> Result<Self, PlanError> {
        let mut globals_by_storage = BTreeMap::new();
        for binding in &program.scope_index.bindings {
            let value = match binding.target {
                ir::ErasedBindingTarget::Value { row: Some(row), .. } => {
                    Some(GlobalValue::List(ListId(row.list.0)))
                }
                ir::ErasedBindingTarget::Value {
                    field: Some(field), ..
                } if !matches!(binding.flow_type.ty, Type::Object(_)) => {
                    Some(GlobalValue::Field(FieldId(field.0)))
                }
                ir::ErasedBindingTarget::Value { .. } => {
                    Some(GlobalValue::Inline(binding.producer))
                }
                ir::ErasedBindingTarget::Source { runtime, .. } => {
                    Some(GlobalValue::Source(SourceId(runtime.0)))
                }
                ir::ErasedBindingTarget::State { runtime, .. } => {
                    Some(GlobalValue::State(StateId(runtime.0)))
                }
            };
            if let Some(value) = value {
                globals_by_storage.insert(binding.id, value);
            }
        }

        let mut materializations_by_id = BTreeMap::new();
        for materialization in &program.materializations {
            let info = ContextualMaterializationInfo {
                id: materialization.id,
                operation: materialization.operation,
                source: materialization.source,
                body: materialization.body,
                result_kind: materialization.result_kind,
                row_local: materialization.row_local,
                owner: materialization.owner,
            };
            if materializations_by_id
                .insert(materialization.id, info)
                .is_some()
            {
                return Err(PlanError::new(format!(
                    "duplicate contextual materialization id {}",
                    materialization.id
                )));
            }
        }

        Ok(Self {
            program,
            value_index,
            root_computation_fields,
            row_expressions,
            machine_constants,
            globals_by_storage,
            distributed_by_expression: distributed_expression_refs.clone(),
            distributed_by_path: distributed_path_refs.clone(),
            materializations_by_id,
            names: Vec::new(),
            name_ids: BTreeMap::new(),
            constants: Vec::new(),
            expressions: Vec::new(),
            expression_cache: BTreeMap::new(),
            projected_expression_cache: BTreeMap::new(),
            static_expression_selectors: BTreeMap::new(),
            functions: Vec::new(),
            function_ids: BTreeSet::new(),
            templates: Vec::new(),
            template_ids: BTreeSet::new(),
            templates_by_node_expression: BTreeMap::new(),
            template_contexts: BTreeMap::new(),
            materializations: Vec::new(),
            materialization_ids: BTreeSet::new(),
            materializations_in_progress: BTreeSet::new(),
            compiled_materializations: BTreeSet::new(),
            compiled_paths: BTreeMap::new(),
            compile_stack: Vec::new(),
            active_ordinary_functions: BTreeSet::new(),
            ordinary_function_overlay_requirements: BTreeMap::new(),
            ordinary_functions: Vec::new(),
            next_function_id: program.scope_index.owners.len(),
            next_cache_scope: program.scope_index.owners.len().saturating_add(1),
            next_local: 0,
        })
    }

    fn compile(mut self, output: &ir::OutputRootValue) -> Result<DocumentPlan, PlanError> {
        let root_kind = match output.contract {
            ir::SemanticOutputContractKind::RetainedVisual {
                kind: ir::SemanticRetainedVisualKind::Document,
            } => DocumentRootKind::Document,
            ir::SemanticOutputContractKind::RetainedVisual {
                kind: ir::SemanticRetainedVisualKind::Scene,
            } => DocumentRootKind::Scene,
            ir::SemanticOutputContractKind::HostValue => {
                return Err(PlanError::new(format!(
                    "host-value output `{}` cannot be lowered as retained visual content",
                    output.root
                )));
            }
        };
        let root_expression =
            self.compile_expression(output.value_expression_id, &CompileContext::default(), None)?;

        self.functions.sort_by_key(|function| function.id);
        self.templates.sort_by_key(|template| template.id);
        self.materializations
            .sort_by_key(|materialization| materialization.id);

        let root_template =
            DocumentTemplateId(stable_compiler_identity(1, None, output.statement_id)?);
        let root_node = DocumentNodeId(stable_compiler_identity(2, None, output.statement_id)?);
        if self.template_ids.insert(root_template) {
            self.templates.push(DocumentTemplate {
                id: root_template,
                node: root_node,
                compiler_expr_id: output.statement_id,
                owner_function: None,
                constructor: match root_kind {
                    DocumentRootKind::Document => DocumentConstructor::DocumentNew,
                    DocumentRootKind::Scene => DocumentConstructor::SceneNew,
                },
                expression: root_expression,
            });
            self.templates.sort_by_key(|template| template.id);
        }
        let mut root = DocumentRoot {
            kind: root_kind,
            node: root_node,
            template: root_template,
            expression: root_expression,
        };
        let mut view_bindings = self.compile_view_bindings()?;
        let mut initial_patch_batch = DocumentPlan::build_initial_patch_batch(
            root,
            &self.templates,
            &view_bindings,
            &self.materializations,
        );
        let inlined_one_offs = self.inline_single_use_ordinary_functions(
            &mut root,
            &mut initial_patch_batch,
            &mut view_bindings,
        )?;
        if std::env::var_os("BOON_COMPILER_LOWER_TRACE").is_some() {
            let shared_ordinary_functions = self
                .ordinary_functions
                .iter()
                .filter(|variant| variant.call_sites.len() > 1)
                .count();
            eprintln!(
                "boon_compiler document artifacts expressions={} constants={} names={} functions={} templates={} materializations={} expression_cache={} projected_cache={} ordinary_variant_demands={} shared_ordinary_functions={} inlined_one_offs={} cache_scopes={}",
                self.expressions.len(),
                self.constants.len(),
                self.names.len(),
                self.functions.len(),
                self.templates.len(),
                self.materializations.len(),
                self.expression_cache.len(),
                self.projected_expression_cache.len(),
                self.ordinary_functions.len(),
                shared_ordinary_functions,
                inlined_one_offs,
                self.next_cache_scope,
            );
            eprintln!(
                "boon_compiler document ordinary function variants sample={:?}",
                self.ordinary_functions
                    .iter()
                    .filter(|variant| variant.call_sites.len() > 1)
                    .take(16)
                    .map(|variant| (
                        variant.id,
                        variant.key.function,
                        variant.key.call_instance,
                        &variant.key.parameter_types,
                        &variant.key.type_substitutions,
                    ))
                    .collect::<Vec<_>>(),
            );
            let mut call_histogram = BTreeMap::new();
            for variant in &self.ordinary_functions {
                *call_histogram
                    .entry(variant.call_sites.len())
                    .or_insert(0usize) += 1;
            }
            eprintln!("boon_compiler document ordinary function call histogram={call_histogram:?}");
        }
        Ok(DocumentPlan {
            root,
            initial_patch_batch,
            names: self.names,
            constants: self.constants,
            expressions: self.expressions,
            functions: self.functions,
            templates: self.templates,
            materializations: self.materializations,
            view_bindings,
            unresolved_op_count: 0,
        })
    }

    fn inline_single_use_ordinary_functions(
        &mut self,
        root: &mut DocumentRoot,
        initial_patch_batch: &mut DocumentInitialPatchBatch,
        view_bindings: &mut [DocumentViewBinding],
    ) -> Result<usize, PlanError> {
        let single_use = self
            .ordinary_functions
            .iter()
            .filter_map(|variant| {
                let [call_site] = variant.call_sites.as_slice() else {
                    return None;
                };
                Some((
                    variant.id,
                    variant.parameters.clone(),
                    variant.parameter_values.clone(),
                    *call_site,
                ))
            })
            .collect::<Vec<_>>();
        if single_use.is_empty() {
            return Ok(0);
        }

        let mut redirects = vec![None; self.expressions.len()];
        let mut removed_parameters = BTreeMap::new();
        let mut owner_redirects = BTreeMap::new();
        let mut removed_functions = BTreeSet::new();
        for (function_id, parameters, parameter_values, (call_id, caller_function)) in &single_use {
            let definition = self
                .functions
                .iter()
                .find(|candidate| candidate.id == *function_id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "single-use document function {} has no published definition",
                        function_id.0
                    ))
                })?;
            if definition.parameters != *parameters || parameters.len() != parameter_values.len() {
                return Err(PlanError::new(format!(
                    "single-use document function {} has an inconsistent parameter layout",
                    function_id.0
                )));
            }
            let call = self.expressions.get(call_id.0).ok_or_else(|| {
                PlanError::new(format!(
                    "single-use document function {} references missing call expression {}",
                    function_id.0, call_id.0
                ))
            })?;
            let DocumentExprOp::Call {
                function,
                arguments,
            } = &call.op
            else {
                return Err(PlanError::new(format!(
                    "single-use document function {} call expression {} is not a call frame",
                    function_id.0, call_id.0
                )));
            };
            if function != function_id {
                return Err(PlanError::new(format!(
                    "single-use document call expression {} targets function {}, expected {}",
                    call_id.0, function.0, function_id.0
                )));
            }
            let arguments = arguments
                .iter()
                .map(|argument| (argument.parameter, argument.value))
                .collect::<BTreeMap<_, _>>();
            if arguments.len() != parameters.len() {
                return Err(PlanError::new(format!(
                    "single-use document function {} has an incomplete call frame",
                    function_id.0
                )));
            }
            for (parameter, parameter_value) in parameters.iter().zip(parameter_values) {
                let argument = arguments.get(parameter).copied().ok_or_else(|| {
                    PlanError::new(format!(
                        "single-use document function {} call omits parameter {}",
                        function_id.0, parameter.0
                    ))
                })?;
                set_document_expression_redirect(
                    &mut redirects,
                    *parameter_value,
                    argument,
                    "single-use parameter",
                )?;
                if removed_parameters.insert(*parameter, argument).is_some() {
                    return Err(PlanError::new(format!(
                        "single-use document parameter {} belongs to multiple functions",
                        parameter.0
                    )));
                }
            }
            set_document_expression_redirect(
                &mut redirects,
                *call_id,
                definition.body,
                "single-use call",
            )?;
            removed_functions.insert(*function_id);
            owner_redirects.insert(*function_id, *caller_function);
        }

        for materialization in &mut self.materializations {
            if let DocumentMaterializationSource::Parameter {
                parameter,
                projection,
            } = &materialization.source
                && let Some(argument) = removed_parameters.get(parameter).copied()
            {
                if !projection.is_empty() {
                    return Err(PlanError::new(format!(
                        "single-use document parameter {} retains a projected materialization source",
                        parameter.0
                    )));
                }
                materialization.source = DocumentMaterializationSource::Expression {
                    expression: argument,
                };
            }
        }

        self.functions
            .retain(|function| !removed_functions.contains(&function.id));
        for template in &mut self.templates {
            if let Some(owner) = template.owner_function {
                template.owner_function = resolve_document_function_owner(
                    owner,
                    &owner_redirects,
                    self.functions.len() + owner_redirects.len(),
                )?;
            }
        }

        let mut dense_ids = vec![None; self.expressions.len()];
        let mut next = 0usize;
        for (ordinal, redirect) in redirects.iter().enumerate() {
            if redirect.is_none() {
                dense_ids[ordinal] = Some(DocumentExprId(next));
                next += 1;
            }
        }
        let final_ids = (0..self.expressions.len())
            .map(|ordinal| {
                let resolved =
                    resolve_document_expression_redirect(DocumentExprId(ordinal), &redirects)?;
                dense_document_expression_id(resolved, &dense_ids)
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        remap_document_external_expression_refs(
            root,
            initial_patch_batch,
            &mut self.functions,
            &mut self.templates,
            &mut self.materializations,
            view_bindings,
            |id| final_document_expression_id(id, &final_ids),
        )?;
        for expression in &mut self.expressions {
            remap_document_expression_op(&mut expression.op, &mut |id| {
                final_document_expression_id(id, &final_ids)
            })?;
        }

        let mut compact = Vec::with_capacity(next);
        for mut expression in std::mem::take(&mut self.expressions) {
            let Some(id) = dense_ids[expression.id.0] else {
                continue;
            };
            expression.id = id;
            compact.push(expression);
        }
        self.expressions = compact;

        for expression in &self.expressions {
            match &expression.op {
                DocumentExprOp::Call { function, .. } if removed_functions.contains(function) => {
                    return Err(PlanError::new(format!(
                        "single-use document function {} remains reachable after compaction",
                        function.0
                    )));
                }
                DocumentExprOp::Read {
                    read: DocumentRead::Parameter { parameter, .. },
                } if removed_parameters.contains_key(parameter) => {
                    return Err(PlanError::new(format!(
                        "single-use document parameter {} remains reachable after compaction",
                        parameter.0
                    )));
                }
                _ => {}
            }
        }
        Ok(single_use.len())
    }

    fn compile_expression(
        &mut self,
        expression_id: ir::ExecutableExprId,
        context: &CompileContext,
        input_override: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        let cache_key = (context.cache_scope, expression_id.0);
        if input_override.is_none()
            && let Some(expression) = self.expression_cache.get(&cache_key).copied()
        {
            return Ok(expression);
        }
        let expression = self.expression(expression_id)?.clone();
        self.compile_stack.push(expression_id);
        let result = if input_override.is_none() {
            if let Some(value) = self.distributed_by_expression.get(&expression_id).cloned() {
                self.compile_distributed_value(
                    expression.id.0,
                    value,
                    self.invocation_value_class(&expression, context)?,
                )
            } else {
                self.compile_expression_kind(&expression, context, input_override)
            }
        } else {
            self.compile_expression_kind(&expression, context, input_override)
        };
        self.compile_stack.pop();
        let result = result?;
        if input_override.is_none() {
            self.expression_cache.insert(cache_key, result);
        }
        Ok(result)
    }

    fn compile_expression_projection(
        &mut self,
        expression_id: ir::ExecutableExprId,
        projection: &[String],
        context: &CompileContext,
        input_override: Option<DocumentExprId>,
        final_class: DocumentValueClass,
    ) -> Result<DocumentExprId, PlanError> {
        if projection.is_empty() {
            return self.compile_expression(expression_id, context, input_override);
        }
        let cache_key = (context.cache_scope, expression_id.0, projection.to_vec());
        if input_override.is_none()
            && let Some(expression) = self.projected_expression_cache.get(&cache_key).copied()
        {
            return Ok(expression);
        }

        let expression = self.expression(expression_id)?.clone();
        self.compile_stack.push(expression_id);
        let result = (|| -> Result<DocumentExprId, PlanError> {
            if input_override.is_some()
                || self.distributed_by_expression.contains_key(&expression_id)
            {
                let input = self.compile_expression(expression_id, context, input_override)?;
                Ok(self.project_fields(expression_id.0, input, projection, final_class))
            } else {
                match &expression.kind {
                    ir::ExecutableExpressionKind::Object { fields }
                    | ir::ExecutableExpressionKind::TaggedObject { fields, .. }
                        if fields.iter().all(|field| !field.spread) =>
                    {
                        let matches = fields
                            .iter()
                            .filter(|field| field.name == projection[0])
                            .map(|field| field.value)
                            .collect::<Vec<_>>();
                        match matches.as_slice() {
                            [field] => self.compile_expression_projection(
                                *field,
                                &projection[1..],
                                context,
                                None,
                                final_class,
                            ),
                            _ => {
                                let input =
                                    self.compile_expression(expression_id, context, None)?;
                                Ok(self.project_fields(
                                    expression_id.0,
                                    input,
                                    projection,
                                    final_class,
                                ))
                            }
                        }
                    }
                    ir::ExecutableExpressionKind::TaggedObject { fields, .. }
                        if fields.iter().all(|field| !field.spread) =>
                    {
                        let matches = fields
                            .iter()
                            .filter(|field| field.name == projection[0])
                            .map(|field| field.value)
                            .collect::<Vec<_>>();
                        match matches.as_slice() {
                            [field] => self.compile_expression_projection(
                                *field,
                                &projection[1..],
                                context,
                                None,
                                final_class,
                            ),
                            _ => {
                                let input =
                                    self.compile_expression(expression_id, context, None)?;
                                Ok(self.project_fields(
                                    expression_id.0,
                                    input,
                                    projection,
                                    final_class,
                                ))
                            }
                        }
                    }
                    ir::ExecutableExpressionKind::Project { input, fields } => {
                        let mut combined = fields.clone();
                        combined.extend_from_slice(projection);
                        self.compile_expression_projection(
                            *input,
                            &combined,
                            context,
                            None,
                            final_class,
                        )
                    }
                    ir::ExecutableExpressionKind::CanonicalRead { .. } => self
                        .compile_erased_read_projection(
                            expression_id,
                            context,
                            projection,
                            final_class,
                        ),
                    ir::ExecutableExpressionKind::Draining { input } => self
                        .compile_expression_projection(
                            *input,
                            projection,
                            context,
                            None,
                            final_class,
                        ),
                    ir::ExecutableExpressionKind::LocalRead {
                        binding,
                        declaration,
                        projection: existing,
                    } => {
                        let local = context.locals.get(binding).copied().ok_or_else(|| {
                            PlanError::new(format!(
                                "executable expression {} reads inactive lexical binding {} for declaration {}",
                                expression_id.0, binding.0, declaration.0
                            ))
                        })?;
                        let projection = existing
                            .iter()
                            .chain(projection)
                            .map(|field| self.intern_name(field))
                            .collect();
                        Ok(self.push_expr(
                            expression_id.0,
                            final_class,
                            DocumentExprOp::Read {
                                read: DocumentRead::Local { local, projection },
                            },
                        ))
                    }
                    ir::ExecutableExpressionKind::ElementState {
                        context: element_context,
                        projection: existing,
                    } => {
                        let projection = existing
                            .iter()
                            .chain(projection)
                            .map(|field| self.intern_name(field))
                            .collect();
                        Ok(self.push_expr(
                            expression_id.0,
                            final_class,
                            DocumentExprOp::Read {
                                read: DocumentRead::ElementState {
                                    context: document_element_context(*element_context),
                                    projection,
                                },
                            },
                        ))
                    }
                    ir::ExecutableExpressionKind::MaterializationLocal {
                        owner,
                        local,
                        projection: existing,
                        ..
                    } => {
                        let projection = existing
                            .iter()
                            .chain(projection)
                            .cloned()
                            .collect::<Vec<_>>();
                        if let Some(read) =
                            self.materialization_resource_read(*owner, *local, &projection)?
                        {
                            return Ok(self.push_expr(
                                expression_id.0,
                                final_class,
                                DocumentExprOp::Read { read },
                            ));
                        }
                        let parameter = context
                            .materialization_locals
                            .get(&(*owner, *local))
                        .copied()
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "executable expression {} reads unbound materialization owner {} local {}; retained path: {}",
                                expression_id.0,
                                owner.0,
                                local.0,
                                self.compile_stack
                                    .iter()
                                    .copied()
                                    .map(|expression| executable_debug_label(
                                        self.program,
                                        expression
                                    ))
                                    .collect::<Vec<_>>()
                                    .join(" -> ")
                            ))
                        })?;
                        let projection = projection
                            .iter()
                            .map(|field| self.intern_name(field))
                            .collect();
                        Ok(self.push_expr(
                            expression_id.0,
                            final_class,
                            DocumentExprOp::Read {
                                read: DocumentRead::Parameter {
                                    parameter,
                                    projection,
                                },
                            },
                        ))
                    }
                    ir::ExecutableExpressionKind::Source { .. } => Err(PlanError::new(format!(
                        "document executable expression {} projects transient SOURCE payload `{}`; retain the event value in HOLD before rendering it",
                        expression_id.0,
                        projection.join(".")
                    ))),
                    _ => {
                        let input = self.compile_expression(expression_id, context, None)?;
                        Ok(self.project_fields(expression_id.0, input, projection, final_class))
                    }
                }
            }
        })();
        self.compile_stack.pop();
        let result = result?;
        if input_override.is_none() {
            self.projected_expression_cache.insert(cache_key, result);
        }
        Ok(result)
    }

    fn compile_distributed_value(
        &mut self,
        compiler_id: usize,
        value: ValueRef,
        class: DocumentValueClass,
    ) -> Result<DocumentExprId, PlanError> {
        match value {
            ValueRef::DistributedImport(import) => Ok(self.push_expr(
                compiler_id,
                class,
                DocumentExprOp::Read {
                    read: DocumentRead::DistributedImport { import },
                },
            )),
            ValueRef::Source(source) => Ok(self.push_expr(
                compiler_id,
                class,
                DocumentExprOp::Read {
                    read: DocumentRead::Source { source },
                },
            )),
            ValueRef::SourcePayload { source_id, field } => Err(PlanError::new(format!(
                "document expression {compiler_id} reads transient payload {field:?} from source {source_id:?}; retain the event value in HOLD before rendering it"
            ))),
            value => Err(PlanError::new(format!(
                "distributed executable expression {compiler_id} has unsupported document value {value:?}"
            ))),
        }
    }

    fn compile_expression_kind(
        &mut self,
        expression: &ir::ExecutableExpression,
        context: &CompileContext,
        input_override: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        let compiler_id = expression.id.0;
        let expression_value_class = self.invocation_value_class(expression, context)?;
        match &expression.kind {
            ir::ExecutableExpressionKind::CanonicalRead { .. } => {
                self.compile_erased_read(expression.id, context, expression_value_class)
            }
            ir::ExecutableExpressionKind::LocalRead {
                binding,
                declaration,
                projection,
            } => {
                let local = context.locals.get(binding).copied().ok_or_else(|| {
                    PlanError::new(format!(
                        "executable expression {compiler_id} reads inactive lexical binding {} for declaration {}",
                        binding.0, declaration.0
                    ))
                })?;
                let projection = projection
                    .iter()
                    .map(|field| self.intern_name(field))
                    .collect();
                Ok(self.push_expr(
                    compiler_id,
                    expression_value_class,
                    DocumentExprOp::Read {
                        read: DocumentRead::Local { local, projection },
                    },
                ))
            }
            ir::ExecutableExpressionKind::ExternalRead { canonical_path } => self
                .compile_external_read(
                    compiler_id,
                    canonical_path,
                    context,
                    expression_value_class,
                ),
            ir::ExecutableExpressionKind::ElementState {
                context,
                projection,
            } => {
                let projection = projection
                    .iter()
                    .map(|field| self.intern_name(field))
                    .collect();
                Ok(self.push_expr(
                    compiler_id,
                    expression_value_class,
                    DocumentExprOp::Read {
                        read: DocumentRead::ElementState {
                            context: document_element_context(*context),
                            projection,
                        },
                    },
                ))
            }
            ir::ExecutableExpressionKind::Drain { path, .. } => Err(PlanError::new(format!(
                "migration drain `{path}` at executable expression {compiler_id} cannot be lowered as a document value"
            ))),
            ir::ExecutableExpressionKind::Text { value } => Ok(self.constant_expr(
                compiler_id,
                DocumentConstantValue::Text {
                    value: value.clone(),
                },
            )),
            ir::ExecutableExpressionKind::TextTemplate { segments } => {
                self.compile_text_template(compiler_id, segments, context)
            }
            ir::ExecutableExpressionKind::Number { value } => Ok(self.constant_expr(
                compiler_id,
                DocumentConstantValue::Number {
                    value: value.clone(),
                },
            )),
            ir::ExecutableExpressionKind::Bits { value } => Ok(self.constant_expr(
                compiler_id,
                DocumentConstantValue::Bits {
                    value: value.clone(),
                },
            )),
            ir::ExecutableExpressionKind::BytesByte { value } => Ok(self.constant_expr(
                compiler_id,
                DocumentConstantValue::Bytes {
                    value: vec![*value],
                },
            )),
            ir::ExecutableExpressionKind::Absent => {
                Ok(self.push_expr(compiler_id, expression_value_class, DocumentExprOp::Absent))
            }
            ir::ExecutableExpressionKind::Flush { .. } => Err(PlanError::new(format!(
                "live FLUSH control at executable expression {compiler_id} cannot be materialized as retained document data"
            ))),
            ir::ExecutableExpressionKind::FlushBoundary { .. } => {
                let runtime_expression = lower_document_runtime_expression(
                    self.program,
                    self.value_index,
                    self.row_expressions,
                    self.machine_constants,
                    expression.id,
                )
                .map_err(|error| {
                    PlanError::new(format!(
                        "FLUSH boundary at executable expression {compiler_id} cannot be lowered as retained document data: {error}"
                    ))
                })?;
                let bindings = context
                    .materialization_locals
                    .iter()
                    .map(|((owner, local), parameter)| DocumentRuntimeLocalBinding {
                        owner: PlanStaticOwnerId(owner.0),
                        local: PlanLocalId(local.0 as usize),
                        parameter: *parameter,
                    })
                    .collect();
                Ok(self.push_expr(
                    compiler_id,
                    expression_value_class,
                    DocumentExprOp::RuntimeExpression {
                        expression: runtime_expression,
                        bindings,
                    },
                ))
            }
            ir::ExecutableExpressionKind::Tag { value } => self.compile_tag(compiler_id, value),
            ir::ExecutableExpressionKind::TaggedObject { tag, fields } => {
                self.compile_record_fields(compiler_id, Some(tag), fields, context)
            }
            ir::ExecutableExpressionKind::Source { .. } => {
                let mut definitions = self
                    .program
                    .executable
                    .sources
                    .iter()
                    .filter(|source| source.expression == expression.id);
                let definition = definitions.next().ok_or_else(|| {
                    PlanError::new(format!(
                        "SOURCE expression {} has no executable source definition",
                        expression.id
                    ))
                })?;
                if definitions.next().is_some() {
                    return Err(PlanError::new(format!(
                        "SOURCE expression {} owns multiple executable source definitions",
                        expression.id
                    )));
                }
                let mut runtime_sources = self.program.sources.iter().filter(|source| {
                    source.executable_source_id == Some(definition.id)
                        && source.static_owner == definition.owner
                });
                let runtime = runtime_sources.next().ok_or_else(|| {
                    PlanError::new(format!(
                        "executable source {} has no exact runtime SourceId",
                        definition.id
                    ))
                })?;
                if runtime_sources.next().is_some() {
                    return Err(PlanError::new(format!(
                        "executable source {} owns multiple runtime SourceIds",
                        definition.id
                    )));
                }
                Ok(self.push_expr(
                    compiler_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::Source {
                            source: SourceId(runtime.id.0),
                        },
                    },
                ))
            }
            ir::ExecutableExpressionKind::Call {
                checked_call,
                callable_kind,
                name,
                instance,
                arguments,
                contexts,
                context_ordinals,
                ..
            } => self.compile_call(
                ExecutableCall {
                    expression,
                    checked_call: *checked_call,
                    callable_kind: *callable_kind,
                    function: name,
                    instance: *instance,
                    arguments,
                    contexts,
                    context_ordinals,
                },
                context,
                input_override,
            ),
            ir::ExecutableExpressionKind::UserCall {
                checked_call,
                function,
                name,
                instance,
                arguments,
                type_substitutions,
                ..
            } => self.compile_user_call(
                expression,
                *checked_call,
                *function,
                name,
                *instance,
                arguments,
                type_substitutions,
                context,
                input_override,
            ),
            ir::ExecutableExpressionKind::Materialize { materialization } => {
                let info = self
                    .materializations_by_id
                    .get(materialization)
                    .copied()
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "executable expression {compiler_id} references missing contextual materialization {materialization}"
                        ))
                    })?;
                let body_is_render = is_renderable_type(&self.expression(info.body)?.flow_type.ty);
                let render_map = info.operation == ir::ContextualOperationKind::Map
                    && info.result_kind == ir::MaterializationResultKind::RenderSlot
                    && body_is_render;
                if render_map {
                    let materialization =
                        self.ensure_materialization(*materialization, compiler_id, context)?;
                    Ok(self.push_expr(
                        compiler_id,
                        DocumentValueClass::ChildList,
                        DocumentExprOp::Materialize { materialization },
                    ))
                } else {
                    if info.result_kind != ir::MaterializationResultKind::RuntimeValue
                        || body_is_render
                    {
                        return Err(PlanError::new(format!(
                            "contextual {:?} materialization {} has inconsistent {:?} / body type {:?}",
                            info.operation,
                            info.id,
                            info.result_kind,
                            self.expression(info.body)?.flow_type.ty
                        )));
                    }
                    let runtime_expression = lower_document_runtime_expression(
                        self.program,
                        self.value_index,
                        self.row_expressions,
                        self.machine_constants,
                        expression.id,
                    )
                    .map_err(|error| {
                        PlanError::new(format!(
                            "contextual {:?} materialization {} ({:?}, source {:?}, body {:?}, body type {:?}, result type {:?}) cannot be lowered as document runtime data: {error}",
                            info.operation,
                            info.id,
                            info.result_kind,
                            self.expression(info.source).map(|value| &value.kind),
                            self.expression(info.body).map(|value| &value.kind),
                            self.expression(info.body)
                                .map(|body| body.flow_type.ty.clone())
                                .unwrap_or(Type::Unknown),
                            expression.flow_type.ty
                        ))
                    })?;
                    let bindings = context
                        .materialization_locals
                        .iter()
                        .map(|((owner, local), parameter)| DocumentRuntimeLocalBinding {
                            owner: PlanStaticOwnerId(owner.0),
                            local: PlanLocalId(local.0 as usize),
                            parameter: *parameter,
                        })
                        .collect();
                    Ok(self.push_expr(
                        compiler_id,
                        expression_value_class,
                        DocumentExprOp::RuntimeExpression {
                            expression: runtime_expression,
                            bindings,
                        },
                    ))
                }
            }
            ir::ExecutableExpressionKind::Draining { input } => {
                self.compile_expression(*input, context, input_override)
            }
            ir::ExecutableExpressionKind::Hold { name, .. } => {
                let storage_binding = self.storage_binding_for_state_expression(expression.id)?;
                let global = self
                    .globals_by_storage
                    .get(&storage_binding)
                    .copied()
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "state storage binding {storage_binding} (`{name}`) has no document value"
                        ))
                    })?;
                self.compile_global_projection(
                    compiler_id,
                    global,
                    name,
                    &[],
                    context,
                    expression_value_class,
                )
            }
            ir::ExecutableExpressionKind::Latest { branches } => {
                let branches = branches
                    .iter()
                    .map(|branch| self.compile_expression(*branch, context, None))
                    .collect::<Result<Vec<_>, _>>()?;
                if branches.is_empty() {
                    return Err(PlanError::new(format!(
                        "LATEST executable expression {compiler_id} has no branch"
                    )));
                }
                Ok(self.push_expr(
                    compiler_id,
                    expression_value_class,
                    DocumentExprOp::Latest { branches },
                ))
            }
            ir::ExecutableExpressionKind::When { input, arms } => {
                let input = self.compile_expression(*input, context, input_override)?;
                self.compile_select(expression, input, arms, context)
            }
            ir::ExecutableExpressionKind::Then { input, output } => {
                let input = self.compile_expression(*input, context, input_override)?;
                let output = output
                    .map(|output| self.compile_expression(output, context, None))
                    .transpose()?;
                Ok(self.push_expr(
                    compiler_id,
                    expression_value_class,
                    DocumentExprOp::Then { input, output },
                ))
            }
            ir::ExecutableExpressionKind::Infix { left, op, right } => {
                let left = self.compile_expression(*left, context, None)?;
                let right = self.compile_expression(*right, context, None)?;
                Ok(self.push_expr(
                    compiler_id,
                    expression_value_class,
                    DocumentExprOp::Scalar {
                        operation: scalar_operation(op)?,
                        left,
                        right: Some(right),
                    },
                ))
            }
            ir::ExecutableExpressionKind::MatchArm { output, .. } => output
                .map(|output| self.compile_expression(output, context, None))
                .transpose()?
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "match arm executable expression {compiler_id} has no output"
                    ))
                }),
            ir::ExecutableExpressionKind::Object { fields } => {
                self.compile_record_fields(compiler_id, None, fields, context)
            }
            ir::ExecutableExpressionKind::Block { bindings, result } => {
                self.compile_local_block(expression, bindings, *result, context)
            }
            ir::ExecutableExpressionKind::List { items, .. } => {
                let items = items
                    .iter()
                    .map(|item| {
                        self.compile_expression(*item, context, None).map(|value| {
                            DocumentListItem {
                                value,
                                spread: false,
                            }
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let class = list_value_class(&items, &self.expressions);
                Ok(self.push_expr(compiler_id, class, DocumentExprOp::List { items }))
            }
            ir::ExecutableExpressionKind::MapEntry { .. }
            | ir::ExecutableExpressionKind::Map { .. }
            | ir::ExecutableExpressionKind::Set { .. } => Err(PlanError::new(format!(
                "MAP/SET expression {compiler_id} reached retained-document lowering before collection authority lowering"
            ))),
            ir::ExecutableExpressionKind::Bytes { items, .. } => {
                let bytes = items
                    .iter()
                    .map(|item| match self.expression_kind(*item)? {
                        ir::ExecutableExpressionKind::BytesByte { value } => Ok(*value),
                        other => Err(PlanError::new(format!(
                            "dynamic byte executable expression {} ({other:?}) is not a document constant",
                            item.0
                        ))),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.constant_expr(compiler_id, DocumentConstantValue::Bytes { value: bytes }))
            }
            ir::ExecutableExpressionKind::Delimiter => Ok(input_override.unwrap_or_else(|| {
                self.push_expr(
                    compiler_id,
                    DocumentValueClass::Static,
                    DocumentExprOp::Record { fields: Vec::new() },
                )
            })),
            ir::ExecutableExpressionKind::Project { input, fields } => self
                .compile_expression_projection(
                    *input,
                    fields,
                    context,
                    input_override,
                    expression_value_class,
                ),
            ir::ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
                ..
            } => {
                if let Some(read) =
                    self.materialization_resource_read(*owner, *local, projection)?
                {
                    return Ok(self.push_expr(
                        compiler_id,
                        expression_value_class,
                        DocumentExprOp::Read { read },
                    ));
                }
                let parameter = context
                    .materialization_locals
                    .get(&(*owner, *local))
                    .copied()
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "executable expression {compiler_id} reads unbound materialization owner {} local {}; retained path: {}",
                            owner.0,
                            local.0,
                            self.compile_stack
                                .iter()
                                .copied()
                                .map(|expression| executable_debug_label(self.program, expression))
                                .collect::<Vec<_>>()
                                .join(" -> ")
                        ))
                    })?;
                let projection = projection
                    .iter()
                    .map(|field| self.intern_name(field))
                    .collect();
                Ok(self.push_expr(
                    compiler_id,
                    expression_value_class,
                    DocumentExprOp::Read {
                        read: DocumentRead::Parameter {
                            parameter,
                            projection,
                        },
                    },
                ))
            }
            ir::ExecutableExpressionKind::FunctionParameter {
                parameter,
                projection,
            } => {
                let Some(binding) = context.function_parameters.get(parameter) else {
                    return Err(PlanError::new(format!(
                        "standalone executable function parameter {}:{} reached retained document lowering",
                        parameter.function.0, parameter.ordinal
                    )));
                };
                let projected_type = project_invocation_type(binding.ty.clone(), projection)
                    .unwrap_or_else(|| {
                        boon_checked::apply_checked_type_environment(
                            &expression.flow_type.ty,
                            &context.type_substitutions,
                        )
                    });
                Ok(self.project_fields(
                    compiler_id,
                    binding.value,
                    projection,
                    value_class_for_type(&projected_type),
                ))
            }
        }
    }

    fn compile_local_block(
        &mut self,
        expression: &ir::ExecutableExpression,
        bindings: &[ir::ExecutableBlockBinding],
        result: ir::ExecutableExprId,
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let mut context = context.clone();
        for binding in bindings {
            if context
                .locals
                .insert(binding.id, DocumentLocalId(self.next_local))
                .is_some()
            {
                return Err(PlanError::new(format!(
                    "erased BLOCK expression {} repeats lexical binding {} for declaration {}",
                    expression.id, binding.id.0, binding.declaration.0
                )));
            }
            self.next_local += 1;
        }

        let mut lowered = Vec::with_capacity(bindings.len());
        for binding in exact_block_binding_order(self.program, bindings)? {
            let local = context.locals[&binding.id];
            let value = self.compile_expression(binding.value, &context, None)?;
            lowered.push(DocumentLocalBinding { local, value });
        }
        let result = self.compile_expression(result, &context, None)?;
        let value_class = self.expressions[result.as_usize()].value_class;
        Ok(self.push_expr(
            expression.id.0,
            value_class,
            DocumentExprOp::LocalBlock {
                bindings: lowered,
                result,
            },
        ))
    }

    fn resolve_call_instance(
        &self,
        checked_call: boon_checked::CheckedCallId,
        explicit: Option<usize>,
        parent: Option<usize>,
    ) -> Result<Option<usize>, PlanError> {
        if let Some(instance) = explicit {
            let occurrence = self
                .program
                .executable
                .call_occurrences
                .get(instance)
                .filter(|occurrence| occurrence.id == instance)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "document call {} references missing invocation {instance}",
                        checked_call.0
                    ))
                })?;
            if occurrence.checked_call != Some(checked_call) {
                return Err(PlanError::new(format!(
                    "document invocation {instance} maps to checked call {:?}, expected {}",
                    occurrence.checked_call.map(|call| call.0),
                    checked_call.0
                )));
            }
            return Ok(Some(instance));
        }

        let Some(parent) = parent else {
            return Ok(None);
        };
        let mut matches = self
            .program
            .executable
            .call_occurrences
            .iter()
            .filter(|occurrence| {
                occurrence.parent == Some(parent) && occurrence.checked_call == Some(checked_call)
            })
            .map(|occurrence| occurrence.id);
        let Some(instance) = matches.next() else {
            // Context-free pure calls are deliberately absent from the OUT
            // occurrence tree. Keep the caller's nearest concrete anchor so
            // a contextual descendant can still resolve its own overlay.
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(PlanError::new(format!(
                "document invocation {parent} has multiple children for checked call {}",
                checked_call.0
            )));
        }
        Ok(Some(instance))
    }

    fn compile_user_call(
        &mut self,
        expression: &ir::ExecutableExpression,
        checked_call: boon_checked::CheckedCallId,
        function_id: ir::FunctionId,
        name: &str,
        instance: Option<usize>,
        arguments: &[ir::ExecutableCallArgument],
        type_substitutions: &[CheckedTypeSubstitution],
        caller_context: &CompileContext,
        input_override: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        let call_instance =
            self.resolve_call_instance(checked_call, instance, caller_context.call_instance)?;
        let overlay_anchor = call_instance.or(caller_context.call_instance);
        let function = self
            .program
            .executable
            .ordinary_functions
            .iter()
            .find(|function| function.id == function_id)
            .cloned()
            .ok_or_else(|| {
                PlanError::new(format!(
                    "ordinary document call `{name}` at expression {} references missing function {}",
                    expression.id.0, function_id.0
                ))
            })?;
        let mut arguments = arguments.iter().collect::<Vec<_>>();
        arguments.sort_by_key(|argument| argument.ordinal);
        if function.name != name || arguments.len() != function.parameters.len() {
            return Err(PlanError::new(format!(
                "ordinary document call `{name}` at expression {} differs from its retained function contract",
                expression.id.0
            )));
        }
        let mut bindings = BTreeMap::new();
        let mut parameter_types = Vec::with_capacity(arguments.len());
        let mut static_parameter_selectors = Vec::with_capacity(arguments.len());
        let mut call_arguments = Vec::with_capacity(arguments.len());
        for (argument, parameter) in arguments.into_iter().zip(&function.parameters) {
            if argument.ordinal != parameter.id.ordinal || argument.name != parameter.name {
                return Err(PlanError::new(format!(
                    "ordinary document call `{name}` at expression {} has a stale parameter binding",
                    expression.id.0
                )));
            }
            let argument_type = self.invocation_argument_type(argument.value, caller_context)?;
            let value = self.compile_call_argument(argument, caller_context, input_override)?;
            parameter_types.push(argument_type.clone());
            static_parameter_selectors.push(self.static_selector(value));
            if bindings
                .insert(
                    parameter.id,
                    DocumentFunctionParameterBinding {
                        value,
                        ty: argument_type,
                    },
                )
                .is_some()
            {
                return Err(PlanError::new(format!(
                    "ordinary document call `{name}` binds parameter {} twice",
                    parameter.id.ordinal
                )));
            }
        }
        let instantiated_type_substitutions = type_substitutions
            .iter()
            .map(|substitution| CheckedTypeSubstitution {
                variable: substitution.variable,
                value: boon_checked::apply_checked_type_environment(
                    &substitution.value,
                    &caller_context.type_substitutions,
                ),
            })
            .collect::<Vec<_>>();
        let mut type_environment = caller_context.type_substitutions.clone();
        for substitution in instantiated_type_substitutions {
            type_environment.insert(substitution.variable, substitution.value);
        }
        // A result type is not a capability contract: UI libraries may use
        // arbitrary tags (including `NoElement`) for helpers that still build
        // retained nodes. Retain an exact invocation overlay whenever the
        // body transitively consumes occurrence-owned document capabilities;
        // only genuinely context-free value/style helpers share one body.
        let requires_overlay = ordinary_function_requires_overlay(
            self.program,
            function_id,
            &mut self.ordinary_function_overlay_requirements,
        )?;
        let key = OrdinaryDocumentFunctionKey {
            function: function_id,
            call_instance: requires_overlay.then_some(overlay_anchor).flatten(),
            stable_owner: requires_overlay
                .then_some(caller_context.stable_owner)
                .flatten(),
            materialization_locals: requires_overlay
                .then(|| caller_context.materialization_locals.clone())
                .unwrap_or_default(),
            locals: requires_overlay
                .then(|| caller_context.locals.clone())
                .unwrap_or_default(),
            type_substitutions: type_environment,
            pattern_bindings: requires_overlay
                .then(|| caller_context.pattern_bindings.clone())
                .unwrap_or_default(),
            parameter_types,
            static_parameter_selectors,
        };
        let (document_function, parameters) =
            self.compile_ordinary_function(&function, name, key)?;
        for (parameter, executable) in parameters.into_iter().zip(&function.parameters) {
            let binding = bindings.get(&executable.id).ok_or_else(|| {
                PlanError::new(format!(
                    "ordinary document call `{name}` lost parameter {}",
                    executable.id.ordinal
                ))
            })?;
            call_arguments.push(DocumentCallArgument {
                parameter,
                value: binding.value,
            });
        }
        let body = self
            .functions
            .iter()
            .find(|candidate| candidate.id == document_function)
            .map(|function| function.body)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "ordinary document function {} was not published",
                    document_function.0
                ))
            })?;
        let value_class = self.expressions[body.0].value_class;
        let call_expression = self.push_expr(
            expression.id.0,
            value_class,
            DocumentExprOp::Call {
                function: document_function,
                arguments: call_arguments,
            },
        );
        let variant = self
            .ordinary_functions
            .iter_mut()
            .find(|candidate| candidate.id == document_function)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "ordinary document function {} lost its construction record",
                    document_function.0
                ))
            })?;
        variant
            .call_sites
            .push((call_expression, caller_context.owner_function));
        Ok(call_expression)
    }

    fn compile_ordinary_function(
        &mut self,
        function: &ir::ExecutableOrdinaryFunction,
        name: &str,
        key: OrdinaryDocumentFunctionKey,
    ) -> Result<(DocumentFunctionId, Vec<DocumentParameterId>), PlanError> {
        if let Some(existing) = self
            .ordinary_functions
            .iter_mut()
            .find(|candidate| candidate.key == key)
        {
            return Ok((existing.id, existing.parameters.clone()));
        }
        if !self.active_ordinary_functions.insert(function.id) {
            return Err(PlanError::new(format!(
                "ordinary document callable `{name}` is recursive"
            )));
        }
        let id = DocumentFunctionId(self.next_function_id);
        self.next_function_id = self
            .next_function_id
            .checked_add(1)
            .ok_or_else(|| PlanError::new("document function id overflow"))?;
        if !self.function_ids.insert(id) {
            return Err(PlanError::new(format!(
                "ordinary document callable `{name}` reuses function {}",
                id.0
            )));
        }
        let parameters = (0..function.parameters.len())
            .map(|ordinal| parameter_id(id, ordinal))
            .collect::<Result<Vec<_>, _>>()?;
        self.ordinary_functions.push(OrdinaryDocumentFunction {
            key: key.clone(),
            id,
            parameters: parameters.clone(),
            parameter_values: Vec::new(),
            call_sites: Vec::new(),
        });

        let mut context = CompileContext {
            cache_scope: self.allocate_cache_scope(),
            call_instance: key.call_instance,
            stable_owner: key.stable_owner,
            owner_function: Some(id),
            materialization_locals: key.materialization_locals,
            locals: key.locals,
            function_parameters: BTreeMap::new(),
            type_substitutions: key.type_substitutions,
            pattern_bindings: key.pattern_bindings,
        };
        let mut parameter_values = Vec::with_capacity(parameters.len());
        for (((parameter, executable), ty), static_selector) in parameters
            .iter()
            .copied()
            .zip(&function.parameters)
            .zip(key.parameter_types)
            .zip(key.static_parameter_selectors)
        {
            let value = self.push_expr(
                function.root.0,
                value_class_for_type(&ty),
                DocumentExprOp::Read {
                    read: DocumentRead::Parameter {
                        parameter,
                        projection: Vec::new(),
                    },
                },
            );
            if let Some(selector) = static_selector {
                self.static_expression_selectors.insert(value, selector);
            }
            parameter_values.push(value);
            context.function_parameters.insert(
                executable.id,
                DocumentFunctionParameterBinding { value, ty },
            );
        }
        self.ordinary_functions
            .iter_mut()
            .find(|candidate| candidate.id == id)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "ordinary document function {} lost its parameter construction record",
                    id.0
                ))
            })?
            .parameter_values = parameter_values;
        let body = self.compile_expression(function.root, &context, None).map_err(|error| {
            PlanError::new(format!(
                "ordinary document callable `{name}` body failed during shared function lowering: {error}"
            ))
        });
        self.active_ordinary_functions.remove(&function.id);
        let body = body?;
        self.functions.push(DocumentFunction {
            id,
            parameters: parameters.clone(),
            body,
        });
        Ok((id, parameters))
    }

    fn invocation_argument_type(
        &self,
        expression: ir::ExecutableExprId,
        context: &CompileContext,
    ) -> Result<Type, PlanError> {
        let definition = self.expression(expression)?;
        match &definition.kind {
            ir::ExecutableExpressionKind::FunctionParameter {
                parameter,
                projection,
            } => {
                let binding = context.function_parameters.get(parameter).ok_or_else(|| {
                    PlanError::new(format!(
                        "ordinary call argument reads unbound function parameter {}:{}",
                        parameter.function.0, parameter.ordinal
                    ))
                })?;
                Ok(
                    project_invocation_type(binding.ty.clone(), projection).unwrap_or_else(|| {
                        boon_checked::apply_checked_type_environment(
                            &definition.flow_type.ty,
                            &context.type_substitutions,
                        )
                    }),
                )
            }
            ir::ExecutableExpressionKind::Project { input, fields } => {
                let input = self.invocation_argument_type(*input, context)?;
                Ok(project_invocation_type(input, fields).unwrap_or_else(|| {
                    boon_checked::apply_checked_type_environment(
                        &definition.flow_type.ty,
                        &context.type_substitutions,
                    )
                }))
            }
            ir::ExecutableExpressionKind::Flush { payload }
            | ir::ExecutableExpressionKind::FlushBoundary { input: payload }
            | ir::ExecutableExpressionKind::Draining { input: payload } => {
                self.invocation_argument_type(*payload, context)
            }
            _ => Ok(boon_checked::apply_checked_type_environment(
                &definition.flow_type.ty,
                &context.type_substitutions,
            )),
        }
    }

    fn invocation_value_class(
        &self,
        expression: &ir::ExecutableExpression,
        context: &CompileContext,
    ) -> Result<DocumentValueClass, PlanError> {
        self.invocation_argument_type(expression.id, context)
            .map(|ty| value_class_for_type(&ty))
    }

    fn compile_call(
        &mut self,
        call: ExecutableCall<'_>,
        context: &CompileContext,
        input_override: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        let ExecutableCall {
            expression,
            checked_call,
            callable_kind,
            function,
            instance,
            arguments,
            contexts,
            context_ordinals,
        } = call;
        let compiler_id = expression.id.0;
        let expression_value_class = self.invocation_value_class(expression, context)?;
        let call_instance =
            self.resolve_call_instance(checked_call, instance, context.call_instance)?;
        let effective_contexts = match call_instance {
            Some(call_instance) => {
                let occurrence = self
                    .program
                    .executable
                    .call_occurrences
                    .get(call_instance)
                    .filter(|occurrence| occurrence.id == call_instance)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "document call {} references missing invocation {call_instance}",
                            checked_call.0
                        ))
                    })?;
                if occurrence.context_ordinals != context_ordinals {
                    return Err(PlanError::new(format!(
                        "document invocation {call_instance} context ordinals {:?} differ from checked call {} ordinals {:?}",
                        occurrence.context_ordinals, checked_call.0, context_ordinals
                    )));
                }
                let expected = context_ordinals
                    .iter()
                    .copied()
                    .map(|ordinal| ir::ExecutableCallContextId {
                        call_instance,
                        ordinal,
                    })
                    .collect::<Vec<_>>();
                if !contexts.is_empty() && contexts != expected {
                    return Err(PlanError::new(format!(
                        "document call {} concrete contexts differ from invocation {call_instance}",
                        checked_call.0
                    )));
                }
                expected
            }
            None => {
                if !context_ordinals.is_empty() || !contexts.is_empty() {
                    let mut anchor_lineage = Vec::new();
                    let mut anchor = context.call_instance;
                    while let Some(id) = anchor {
                        anchor_lineage.push(id);
                        anchor = self
                            .program
                            .executable
                            .call_occurrences
                            .get(id)
                            .filter(|candidate| candidate.id == id)
                            .and_then(|candidate| candidate.parent);
                    }
                    let candidates = self
                        .program
                        .executable
                        .call_occurrences
                        .iter()
                        .filter(|occurrence| occurrence.checked_call == Some(checked_call))
                        .take(16)
                        .map(|occurrence| {
                            let mut lineage = vec![occurrence.id];
                            let mut parent = occurrence.parent;
                            while let Some(id) = parent {
                                lineage.push(id);
                                parent = self
                                    .program
                                    .executable
                                    .call_occurrences
                                    .get(id)
                                    .filter(|candidate| candidate.id == id)
                                    .and_then(|candidate| candidate.parent);
                            }
                            (occurrence.id, lineage)
                        })
                        .collect::<Vec<_>>();
                    return Err(PlanError::new(format!(
                        "document call {} `{function}` at executable expression {compiler_id} owns contexts but has no concrete invocation below {:?}; anchor_lineage={anchor_lineage:?}; candidates={candidates:?}",
                        checked_call.0, context.call_instance,
                    )));
                }
                Vec::new()
            }
        };
        if callable_kind == ir::ExecutableCallableKind::External {
            return Err(PlanError::new(format!(
                "external executable call `{function}` at expression {compiler_id} has no directly encoded document import"
            )));
        }

        if let Some(field) = function.strip_prefix("Field/") {
            let mut inputs = arguments
                .iter()
                .filter(|argument| argument.from_pipe || argument.name == "input");
            let argument = inputs.next().ok_or_else(|| {
                PlanError::new(format!("field projection `{function}` has no typed input"))
            })?;
            if inputs.next().is_some() || arguments.len() != 1 {
                return Err(PlanError::new(format!(
                    "field projection `{function}` requires exactly one typed input"
                )));
            }
            let input = self.compile_call_argument(argument, context, input_override)?;
            let field = self.intern_name(field);
            return Ok(self.push_expr(
                compiler_id,
                expression_value_class,
                DocumentExprOp::Project { input, field },
            ));
        }
        if let Some(constructor) = document_constructor(function) {
            if arguments.iter().any(|argument| argument.from_pipe) {
                return Err(PlanError::new(format!(
                    "render constructor `{function}` cannot be used as a pipeline operator"
                )));
            }
            return self.compile_constructor(
                expression,
                constructor,
                arguments,
                &effective_contexts,
                call_instance,
                context,
            );
        }
        if !effective_contexts.is_empty() {
            return Err(PlanError::new(format!(
                "non-render executable call `{function}` at expression {compiler_id} owns a call-local host context"
            )));
        }
        if builtin_effect_contract(function)?
            .is_some_and(|contract| !matches!(contract.replay, EffectReplay::ReadOnly))
        {
            return Err(PlanError::new(format!(
                "consequential host operation `{function}` cannot run during retained document evaluation; publish a pure output descriptor or use a transactional effect branch"
            )));
        }
        let builtin = document_builtin(function).ok_or_else(|| {
            PlanError::new(format!(
                "unknown executable document function `{function}` at expression {compiler_id}"
            ))
        })?;
        let mut input = None;
        let mut compiled_arguments = Vec::new();
        for argument in arguments {
            let value = self.compile_call_argument(argument, context, input_override)?;
            if argument.from_pipe {
                if input.replace(value).is_some() {
                    return Err(PlanError::new(format!(
                        "document builtin `{function}` has more than one pipeline input"
                    )));
                }
            } else {
                compiled_arguments.push(DocumentBuiltinArgument {
                    name: self.intern_name(&argument.name),
                    value,
                });
            }
        }
        Ok(self.push_expr(
            compiler_id,
            expression_value_class,
            DocumentExprOp::Builtin {
                builtin,
                input,
                arguments: compiled_arguments,
            },
        ))
    }

    fn compile_call_argument(
        &mut self,
        argument: &ir::ExecutableCallArgument,
        context: &CompileContext,
        input_override: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        if argument.from_pipe
            && input_override.is_none()
            && matches!(
                self.expression_kind(argument.value)?,
                ir::ExecutableExpressionKind::Delimiter
            )
        {
            return Err(PlanError::new(format!(
                "pipeline argument executable expression {} has no input",
                argument.value.0
            )));
        }
        let input_override = argument.from_pipe.then_some(input_override).flatten();
        self.compile_expression(argument.value, context, input_override)
    }

    fn compile_constructor(
        &mut self,
        expression: &ir::ExecutableExpression,
        constructor: DocumentConstructor,
        arguments: &[ir::ExecutableCallArgument],
        contexts: &[ir::ExecutableCallContextId],
        invocation: Option<usize>,
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let compiler_id = expression.id.0;
        let mut compiled_arguments = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let value = self.compile_expression(argument.value, context, None)?;
            compiled_arguments.push(self.constructor_argument(
                constructor,
                &argument.name,
                value,
            )?);
        }
        verify_map_viewport_constructor_contract(constructor, &compiled_arguments, &self.names)
            .map_err(PlanError::new)?;
        let stable_owner = expression.owner.or(context.stable_owner);
        let template = DocumentTemplateId(stable_invocation_identity(
            3,
            stable_owner,
            invocation,
            compiler_id,
        )?);
        let node = DocumentNodeId(stable_invocation_identity(
            4,
            stable_owner,
            invocation,
            compiler_id,
        )?);
        if let Some(previous) = self
            .templates_by_node_expression
            .insert((invocation, expression.id), template)
            && previous != template
        {
            return Err(PlanError::new(format!(
                "retained constructor expression {compiler_id} maps to both template {} and {}",
                previous.0, template.0
            )));
        }
        if let Some(previous) = self.template_contexts.insert(template, context.clone())
            && previous != *context
        {
            return Err(PlanError::new(format!(
                "retained constructor expression {compiler_id} reuses template {} across different exact compile contexts",
                template.0
            )));
        }
        let result = self.push_expr(
            compiler_id,
            DocumentValueClass::Render,
            DocumentExprOp::Constructor {
                template,
                constructor,
                element_context: match (constructor.owns_element_context(), contexts) {
                    (false, []) => None,
                    (true, [context]) => Some(document_element_context(*context)),
                    (false, _) => {
                        return Err(PlanError::new(format!(
                            "root constructor at expression {compiler_id} cannot own an element context"
                        )));
                    }
                    (true, _) => {
                        return Err(PlanError::new(format!(
                            "element constructor at expression {compiler_id} must own exactly one element context"
                        )));
                    }
                },
                arguments: compiled_arguments,
            },
        );
        if self.template_ids.insert(template) {
            self.templates.push(DocumentTemplate {
                id: template,
                node,
                compiler_expr_id: compiler_id,
                owner_function: context.owner_function,
                constructor,
                expression: result,
            });
        }
        Ok(result)
    }

    fn constructor_argument(
        &mut self,
        constructor: DocumentConstructor,
        name: &str,
        value: DocumentExprId,
    ) -> Result<DocumentConstructorArgument, PlanError> {
        let class = self.expressions[value.0].value_class;
        let role = constructor_argument_role(constructor, name, class)?;
        Ok(DocumentConstructorArgument {
            name: self.intern_name(name),
            role,
            value,
        })
    }

    fn ensure_materialization(
        &mut self,
        materialization_id: usize,
        compiler_expr_id: usize,
        caller_context: &CompileContext,
    ) -> Result<DocumentMaterializationId, PlanError> {
        let info = self
            .materializations_by_id
            .get(&materialization_id)
            .copied()
            .ok_or_else(|| {
                PlanError::new(format!(
                    "executable expression {compiler_expr_id} references missing contextual materialization {materialization_id}"
                ))
            })?;
        let function = DocumentFunctionId(info.owner.0);
        let parameter = parameter_id(function, info.row_local.0 as usize)?;
        let scope = synthetic_scope_id(info.owner)?;
        let plan_id =
            DocumentMaterializationId(stable_compiler_identity(5, Some(info.owner), info.id)?);
        if self.compiled_materializations.contains(&materialization_id) {
            return Ok(plan_id);
        }
        if !self.materializations_in_progress.insert(materialization_id) {
            return Err(PlanError::new(format!(
                "render materialization {materialization_id} is recursively defined"
            )));
        }
        if info.operation != ir::ContextualOperationKind::Map {
            return Err(PlanError::new(format!(
                "render materialization {} uses unsupported contextual operation {:?}",
                info.id, info.operation
            )));
        }
        let body_owner = self.expression(info.body)?.owner;
        if !body_owner
            .map(|body_owner| {
                self.program
                    .scope_index
                    .owner_descends_from(body_owner, info.owner)
                    .map_err(PlanError::new)
            })
            .transpose()?
            .unwrap_or(false)
        {
            return Err(PlanError::new(format!(
                "render materialization {} body root {} has owner {:?}, expected owner subtree {}",
                info.id, info.body.0, body_owner, info.owner
            )));
        }

        let source_expression = self.compile_expression(info.source, caller_context, None)?;
        let mut function_parameters = vec![parameter];
        let mut template_arguments = Vec::new();
        let mut body_context = CompileContext {
            cache_scope: info.owner.0.saturating_add(1),
            stable_owner: Some(info.owner),
            owner_function: Some(function),
            ..CompileContext::default()
        };
        body_context
            .materialization_locals
            .insert((info.owner, info.row_local), parameter);
        for (capture_ordinal, (local, caller_parameter)) in
            caller_context.materialization_locals.iter().enumerate()
        {
            let capture_parameter = parameter_id(function, capture_ordinal + 1)?;
            let capture_value = self.push_expr(
                compiler_expr_id,
                DocumentValueClass::DynamicStructure,
                DocumentExprOp::Read {
                    read: DocumentRead::Parameter {
                        parameter: *caller_parameter,
                        projection: Vec::new(),
                    },
                },
            );
            function_parameters.push(capture_parameter);
            template_arguments.push(DocumentCallArgument {
                parameter: capture_parameter,
                value: capture_value,
            });
            body_context
                .materialization_locals
                .insert(*local, capture_parameter);
        }
        let body = self.compile_expression(info.body, &body_context, None)?;
        if self.expressions[body.0].value_class != DocumentValueClass::Render {
            return Err(PlanError::new(format!(
                "contextual materialization {} reached from document expression {compiler_expr_id} does not produce one render value",
                info.id
            )));
        }

        if !self.function_ids.insert(function) {
            return Err(PlanError::new(format!(
                "render materialization {} reuses synthetic function {}",
                info.id, function.0
            )));
        }
        self.functions.push(DocumentFunction {
            id: function,
            parameters: function_parameters,
            body,
        });

        let source = self.materialization_source(source_expression);
        let source_list = self.source_list_id(source_expression);
        let row_identity = source_list
            .map(|list| DocumentRowIdentity::ListHiddenKeyAndGeneration { list })
            .unwrap_or(DocumentRowIdentity::ScopedHiddenKeyAndGeneration { scope });
        if !self.materialization_ids.insert(plan_id) {
            return Err(PlanError::new(format!(
                "render materialization {} reuses document identity {}",
                info.id, plan_id.0
            )));
        }
        self.materializations.push(DocumentMaterialization {
            id: plan_id,
            compiler_expr_id,
            source,
            item_scope: scope,
            item_parameter: parameter,
            template_function: function,
            template_arguments,
            row_identity,
            policy: DocumentMaterializationPolicy::VisibleRange,
        });
        self.materializations_in_progress
            .remove(&materialization_id);
        self.compiled_materializations.insert(materialization_id);
        Ok(plan_id)
    }

    fn materialization_source(&self, expression: DocumentExprId) -> DocumentMaterializationSource {
        match &self.expressions[expression.0].op {
            DocumentExprOp::Read {
                read: DocumentRead::List { list },
            } => DocumentMaterializationSource::List { list: *list },
            DocumentExprOp::Read {
                read: DocumentRead::Field { field },
            } => DocumentMaterializationSource::Field { field: *field },
            DocumentExprOp::Read {
                read:
                    DocumentRead::Row {
                        scope,
                        field: Some(field),
                        ..
                    },
            } => DocumentMaterializationSource::ScopedField {
                scope: *scope,
                field: *field,
            },
            DocumentExprOp::Read {
                read:
                    DocumentRead::Parameter {
                        parameter,
                        projection,
                    },
            } => DocumentMaterializationSource::Parameter {
                parameter: *parameter,
                projection: projection.clone(),
            },
            _ => DocumentMaterializationSource::Expression { expression },
        }
    }

    fn compile_select(
        &mut self,
        expression: &ir::ExecutableExpression,
        input: DocumentExprId,
        executable_arms: &[ir::ExecutableSelectArm],
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        if let Some(selected) = self.static_select_arm(input, executable_arms) {
            let mut arm_context = context.clone();
            arm_context.cache_scope = self.allocate_cache_scope();
            arm_context
                .pattern_bindings
                .extend(selected.bindings.iter().map(|binding| {
                    (
                        binding.name.clone(),
                        PatternBindingContext {
                            selector: expression.id.0,
                            projection: binding.projection.clone(),
                        },
                    )
                }));
            return self.compile_expression(selected.output, &arm_context, None);
        }
        let mut arms = Vec::with_capacity(executable_arms.len());
        for arm in executable_arms {
            let mut arm_context = context.clone();
            arm_context.cache_scope = self.allocate_cache_scope();
            arm_context
                .pattern_bindings
                .extend(arm.bindings.iter().map(|binding| {
                    (
                        binding.name.clone(),
                        PatternBindingContext {
                            selector: expression.id.0,
                            projection: binding.projection.clone(),
                        },
                    )
                }));
            let output = self.compile_expression(arm.output, &arm_context, None)?;
            arms.push(DocumentSelectArm {
                pattern: self.compile_pattern(&arm.pattern)?,
                bindings: arm
                    .bindings
                    .iter()
                    .map(|binding| DocumentSelectBinding {
                        projection: binding
                            .projection
                            .iter()
                            .map(|field| self.intern_name(field))
                            .collect(),
                    })
                    .collect(),
                output,
            });
        }
        if arms.is_empty() {
            return Err(PlanError::new(format!(
                "conditional executable expression {} has no typed arms",
                expression.id.0
            )));
        }
        let class = std::iter::once(self.expressions[input.0].value_class)
            .chain(
                arms.iter()
                    .map(|arm| self.expressions[arm.output.0].value_class),
            )
            .max_by_key(|class| value_class_rank(*class))
            .unwrap_or(self.invocation_value_class(expression, context)?);
        Ok(self.push_expr(
            expression.id.0,
            class,
            DocumentExprOp::Select { input, arms },
        ))
    }

    fn static_select_arm<'b>(
        &self,
        input: DocumentExprId,
        arms: &'b [ir::ExecutableSelectArm],
    ) -> Option<&'b ir::ExecutableSelectArm> {
        let selector = self.static_selector(input)?;
        arms.iter().find(|arm| selector.matches(&arm.pattern))
    }

    fn static_selector(&self, input: DocumentExprId) -> Option<StaticDocumentSelector> {
        if let Some(selector) = self.static_expression_selectors.get(&input) {
            return Some(selector.clone());
        }
        let input = self.expressions.get(input.0)?;
        Some(match &input.op {
            DocumentExprOp::Constant { constant } => {
                let constant = self.constants.get(constant.0)?;
                match &constant.value {
                    DocumentConstantValue::Text { value } => {
                        StaticDocumentSelector::Text(value.clone())
                    }
                    DocumentConstantValue::Number { value } => {
                        StaticDocumentSelector::Number(value.clone())
                    }
                    DocumentConstantValue::Bits { value } => {
                        StaticDocumentSelector::Bits(value.clone())
                    }
                    DocumentConstantValue::Tag { name } => {
                        StaticDocumentSelector::Tag(self.names.get(name.0)?.clone())
                    }
                    DocumentConstantValue::Bytes { .. } => return None,
                }
            }
            DocumentExprOp::TaggedRecord { tag, .. } => {
                StaticDocumentSelector::Tag(self.names.get(tag.0)?.clone())
            }
            _ => return None,
        })
    }

    fn compile_pattern(
        &mut self,
        pattern: &boon_checked::CheckedMatchPattern,
    ) -> Result<DocumentPattern, PlanError> {
        use boon_checked::CheckedMatchPattern;

        if matches!(
            pattern,
            CheckedMatchPattern::Wildcard | CheckedMatchPattern::Binding { .. }
        ) {
            return Ok(DocumentPattern::Wildcard);
        }
        if let CheckedMatchPattern::Text { value } = pattern {
            let constant = self.push_constant(DocumentConstantValue::Text {
                value: value.clone(),
            });
            return Ok(DocumentPattern::Constant { constant });
        }
        if let CheckedMatchPattern::Number { value } = pattern {
            let constant = self.push_constant(DocumentConstantValue::Number {
                value: value.clone(),
            });
            return Ok(DocumentPattern::Constant { constant });
        }
        if let CheckedMatchPattern::Bits { value } = pattern {
            let constant = self.push_constant(DocumentConstantValue::Bits {
                value: value.clone(),
            });
            return Ok(DocumentPattern::Constant { constant });
        }
        match pattern {
            CheckedMatchPattern::Tag { name, .. } => Ok(DocumentPattern::Tag {
                tag: self.intern_name(name),
            }),
            CheckedMatchPattern::Wildcard
            | CheckedMatchPattern::Binding { .. }
            | CheckedMatchPattern::Number { .. }
            | CheckedMatchPattern::Text { .. }
            | CheckedMatchPattern::Bits { .. } => unreachable!(),
        }
    }

    fn compile_record_fields(
        &mut self,
        compiler_id: usize,
        tag: Option<&str>,
        executable_fields: &[ir::ExecutableRecordField],
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let mut fields = Vec::with_capacity(executable_fields.len());
        for field in executable_fields {
            let value = self.compile_expression(field.value, context, None)?;
            fields.push(DocumentRecordField {
                name: (!field.spread).then(|| self.intern_name(&field.name)),
                value,
                spread: field.spread,
            });
        }
        let class = record_value_class(&fields, &self.expressions);
        let op = tag.map_or_else(
            || DocumentExprOp::Record {
                fields: fields.clone(),
            },
            |tag| DocumentExprOp::TaggedRecord {
                tag: self.intern_name(tag),
                fields: fields.clone(),
            },
        );
        Ok(self.push_expr(compiler_id, class, op))
    }

    fn compile_text_template(
        &mut self,
        compiler_id: usize,
        executable_segments: &[ir::ExecutableTextSegment],
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let mut segments = Vec::with_capacity(executable_segments.len());
        for segment in executable_segments {
            match segment {
                ir::ExecutableTextSegment::Static { value } => {
                    let constant = self.push_constant(DocumentConstantValue::Text {
                        value: value.clone(),
                    });
                    segments.push(DocumentTextSegment::Static { constant });
                }
                ir::ExecutableTextSegment::Dynamic { value } => {
                    let value = self.compile_expression(*value, context, None)?;
                    segments.push(DocumentTextSegment::Dynamic { value });
                }
            }
        }
        Ok(self.push_expr(
            compiler_id,
            DocumentValueClass::DynamicScalar,
            DocumentExprOp::TextTemplate { segments },
        ))
    }

    fn compile_tag(
        &mut self,
        compiler_id: usize,
        value: &str,
    ) -> Result<DocumentExprId, PlanError> {
        if value == "NoElement" {
            return Ok(self.push_expr(
                compiler_id,
                DocumentValueClass::Render,
                DocumentExprOp::NoElement,
            ));
        }
        let name = self.intern_name(value);
        Ok(self.constant_expr(compiler_id, DocumentConstantValue::Tag { name }))
    }

    fn compile_erased_read(
        &mut self,
        expression: ir::ExecutableExprId,
        context: &CompileContext,
        final_class: DocumentValueClass,
    ) -> Result<DocumentExprId, PlanError> {
        self.compile_erased_read_projection(expression, context, &[], final_class)
    }

    fn compile_erased_read_projection(
        &mut self,
        expression: ir::ExecutableExprId,
        context: &CompileContext,
        additional_projection: &[String],
        final_class: DocumentValueClass,
    ) -> Result<DocumentExprId, PlanError> {
        let compiler_id = expression.0;
        let target = self
            .program
            .scope_index
            .reads
            .iter()
            .find(|read| read.expression == expression)
            .map(|read| read.target.clone())
            .ok_or_else(|| {
                PlanError::new(format!(
                    "executable read {expression} has no exact erased read target"
                ))
            })?;
        match target {
            ir::ErasedReadTarget::Binding {
                binding,
                mut projection,
            } => {
                projection.extend_from_slice(additional_projection);
                let storage = self
                    .program
                    .scope_index
                    .bindings
                    .get(binding.as_usize())
                    .filter(|candidate| candidate.id == binding)
                    .ok_or_else(|| {
                        PlanError::new(format!("erased read references missing {binding}"))
                    })?;
                let global = self
                    .globals_by_storage
                    .get(&binding)
                    .copied()
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "storage binding {binding} (`{}`) has no document value",
                            storage.diagnostic_path
                        ))
                    })?;
                self.compile_global_projection(
                    compiler_id,
                    global,
                    &storage.diagnostic_path,
                    &projection,
                    context,
                    final_class,
                )
            }
            ir::ErasedReadTarget::SourcePayload {
                source,
                field,
                mut projection,
                ..
            } => {
                projection.extend_from_slice(additional_projection);
                let source_path = self
                    .program
                    .sources
                    .iter()
                    .find(|candidate| candidate.id == source)
                    .map(|source| source.path.as_str())
                    .unwrap_or("<unknown>");
                Err(PlanError::new(format!(
                    "document executable read {expression} reads transient payload {field:?}{} from source {source} (`{source_path}`); retained path: {}; retain the event value in HOLD before rendering it",
                    if projection.is_empty() {
                        String::new()
                    } else {
                        format!(".{}", projection.join("."))
                    },
                    self.compile_stack
                        .iter()
                        .copied()
                        .map(|expression| executable_debug_label(self.program, expression))
                        .collect::<Vec<_>>()
                        .join(" -> ")
                )))
            }
            ir::ErasedReadTarget::StateProjection {
                state, mut fields, ..
            } => {
                fields.extend_from_slice(additional_projection);
                let base = self.push_expr(
                    compiler_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::State {
                            state: StateId(state.0),
                        },
                    },
                );
                Ok(self.project_fields(compiler_id, base, &fields, final_class))
            }
            ir::ErasedReadTarget::Expression {
                expression,
                mut projection,
            } => {
                projection.extend_from_slice(additional_projection);
                self.compile_expression_projection(
                    expression,
                    &projection,
                    context,
                    None,
                    final_class,
                )
            }
            target => Err(PlanError::new(format!(
                "executable read {expression} has non-document target {target:?}"
            ))),
        }
    }

    fn compile_global_projection(
        &mut self,
        compiler_id: usize,
        global: GlobalValue,
        path: &str,
        projection: &[String],
        context: &CompileContext,
        final_class: DocumentValueClass,
    ) -> Result<DocumentExprId, PlanError> {
        let exact_path = joined_path(path, projection);
        if !projection.is_empty()
            && let Some(exact) = self.value_index.resolve(&exact_path)
        {
            let exact = match exact {
                ValueRef::State(state) => Some(self.push_expr(
                    compiler_id,
                    final_class,
                    DocumentExprOp::Read {
                        read: DocumentRead::State { state },
                    },
                )),
                ValueRef::StateProjection {
                    state_id,
                    field_path,
                } => {
                    let base = self.push_expr(
                        compiler_id,
                        DocumentValueClass::DynamicScalar,
                        DocumentExprOp::Read {
                            read: DocumentRead::State { state: state_id },
                        },
                    );
                    Some(self.project_fields(compiler_id, base, &field_path, final_class))
                }
                ValueRef::Field(field) if self.root_computation_fields.contains(&field) => {
                    Some(self.push_expr(
                        compiler_id,
                        final_class,
                        DocumentExprOp::Read {
                            read: DocumentRead::Field { field },
                        },
                    ))
                }
                ValueRef::Field(_) => None,
                ValueRef::List(list) => Some(self.push_expr(
                    compiler_id,
                    final_class,
                    DocumentExprOp::Read {
                        read: DocumentRead::List { list },
                    },
                )),
                ValueRef::Source(source) => Some(self.push_expr(
                    compiler_id,
                    final_class,
                    DocumentExprOp::Read {
                        read: DocumentRead::Source { source },
                    },
                )),
                ValueRef::SourcePayload {
                    source_id: source, ..
                } => {
                    return Err(PlanError::new(format!(
                        "document path `{exact_path}` reads transient payload from source {source:?}; retain the event value in HOLD before rendering it"
                    )));
                }
                ValueRef::Pulse(pulse) => {
                    return Err(PlanError::new(format!(
                        "document path `{exact_path}` reads transient pulse batch {}; retain its value in HOLD before rendering it",
                        pulse.0
                    )));
                }
                ValueRef::Constant(_) | ValueRef::DistributedImport(_) => None,
            };
            if let Some(expression) = exact {
                self.record_compiled_path(&exact_path, expression);
                return Ok(expression);
            }
        }
        if let GlobalValue::Inline(producer) = global {
            let expression = self.compile_expression_projection(
                producer,
                projection,
                context,
                None,
                final_class,
            )?;
            self.record_compiled_path(&exact_path, expression);
            return Ok(expression);
        }
        if let GlobalValue::Source(source) = global
            && !projection.is_empty()
        {
            return Err(PlanError::new(format!(
                "document path `{}` projects transient payload from source {source:?}; retain the event value in HOLD before rendering it",
                joined_path(path, projection)
            )));
        }
        let base = match global {
            GlobalValue::Inline(_) => unreachable!("inline globals return above"),
            GlobalValue::State(state) => self.push_expr(
                compiler_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::State { state },
                },
            ),
            GlobalValue::Field(field) => self.push_expr(
                compiler_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::Field { field },
                },
            ),
            GlobalValue::List(list) => self.push_expr(
                compiler_id,
                DocumentValueClass::DynamicStructure,
                DocumentExprOp::Read {
                    read: DocumentRead::List { list },
                },
            ),
            GlobalValue::Source(source) => self.push_expr(
                compiler_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::Source { source },
                },
            ),
        };
        let expression = self.project_fields(compiler_id, base, projection, final_class);
        self.record_compiled_path(&exact_path, expression);
        Ok(expression)
    }

    fn compile_external_read(
        &mut self,
        compiler_id: usize,
        canonical_path: &str,
        context: &CompileContext,
        class: DocumentValueClass,
    ) -> Result<DocumentExprId, PlanError> {
        if let Some(ValueRef::DistributedImport(import)) =
            self.distributed_by_path.get(canonical_path)
        {
            return Ok(self.push_expr(
                compiler_id,
                class,
                DocumentExprOp::Read {
                    read: DocumentRead::DistributedImport { import: *import },
                },
            ));
        }
        let parts = canonical_path
            .split('.')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if let Some(binding) = parts
            .first()
            .and_then(|name| context.pattern_bindings.get(*name))
            .cloned()
        {
            let projection = binding
                .projection
                .iter()
                .map(String::as_str)
                .chain(parts.iter().skip(1).copied())
                .map(|part| self.intern_name(part))
                .collect();
            return Ok(self.push_expr(
                compiler_id,
                class,
                DocumentExprOp::Read {
                    read: DocumentRead::Matched {
                        selector: binding.selector,
                        projection,
                    },
                },
            ));
        }
        Err(PlanError::new(format!(
            "unresolved external executable document path `{canonical_path}` at expression {compiler_id}"
        )))
    }

    fn project_fields(
        &mut self,
        compiler_id: usize,
        mut input: DocumentExprId,
        fields: &[String],
        final_class: DocumentValueClass,
    ) -> DocumentExprId {
        for (index, field) in fields.iter().enumerate() {
            if let Some(value) = self.direct_record_field(input, field) {
                input = value;
                continue;
            }
            let field = self.intern_name(field);
            let class = if index + 1 == fields.len() {
                final_class
            } else {
                DocumentValueClass::DynamicStructure
            };
            input = self.push_expr(compiler_id, class, DocumentExprOp::Project { input, field });
        }
        input
    }

    fn direct_record_field(&self, input: DocumentExprId, name: &str) -> Option<DocumentExprId> {
        let fields = match &self.expressions.get(input.0)?.op {
            DocumentExprOp::Record { fields } | DocumentExprOp::TaggedRecord { fields, .. }
                if fields.iter().all(|field| !field.spread) =>
            {
                fields
            }
            _ => return None,
        };
        let mut matches = fields.iter().filter(|field| {
            field
                .name
                .and_then(|name_id| self.names.get(name_id.0))
                .is_some_and(|field_name| field_name == name)
        });
        let value = matches.next()?.value;
        matches.next().is_none().then_some(value)
    }

    fn compile_view_bindings(&mut self) -> Result<Vec<DocumentViewBinding>, PlanError> {
        let mut result = Vec::new();
        for binding in self.program.view_bindings.clone() {
            let mut templates = self
                .templates_by_node_expression
                .iter()
                .filter(|((_, expression), _)| *expression == binding.node_expression)
                .map(|(_, template)| *template);
            let template = templates.next().ok_or_else(|| {
                    PlanError::new(format!(
                        "view binding {} `{}`.{} references retained node expression {} with no exact document template",
                        binding.id.0,
                        binding.node_kind,
                        binding.attr,
                        binding.node_expression.0
                    ))
                })?;
            if templates.any(|candidate| candidate != template) {
                return Err(PlanError::new(format!(
                    "view binding {} `{}`.{} references shared node expression {} without an invocation overlay",
                    binding.id.0, binding.node_kind, binding.attr, binding.node_expression.0
                )));
            }
            let context = self
                .template_contexts
                .get(&template)
                .cloned()
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "view binding {} template {} has no exact compile context",
                        binding.id.0, template.0
                    ))
                })?;
            let scope = binding.scope_id.map(|scope| ScopeId(scope.0));
            let target = match &binding.target {
                ir::ViewBindingTarget::Read {
                    read,
                    additional_projection,
                } => {
                    let read = self
                        .program
                        .scope_index
                        .reads
                        .get(read.as_usize())
                        .filter(|candidate| candidate.id == *read)
                        .cloned()
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "view binding {} references missing erased read {read}",
                                binding.id.0
                            ))
                        })?;
                    let direct = match &read.target {
                        ir::ErasedReadTarget::Binding {
                            binding: storage_binding,
                            projection: read_projection,
                        } if read_projection.is_empty() && additional_projection.is_empty() => {
                            let binding = self
                                .program
                                .scope_index
                                .bindings
                                .get(storage_binding.as_usize())
                                .filter(|candidate| candidate.id == *storage_binding);
                            match binding.map(|binding| &binding.target) {
                                Some(ir::ErasedBindingTarget::Value {
                                    field: Some(field),
                                    row: Some(row),
                                }) => Some(DocumentBindingTarget::ScopedField {
                                    scope: ScopeId(row.scope.0),
                                    field: FieldId(field.0),
                                }),
                                _ => self
                                    .globals_by_storage
                                    .get(storage_binding)
                                    .copied()
                                    .and_then(|global| match global {
                                        GlobalValue::State(state) => {
                                            Some(DocumentBindingTarget::State { state })
                                        }
                                        GlobalValue::Field(field) => {
                                            Some(DocumentBindingTarget::Field { field })
                                        }
                                        GlobalValue::List(list) => {
                                            Some(DocumentBindingTarget::List { list })
                                        }
                                        GlobalValue::Source(source) => {
                                            Some(DocumentBindingTarget::Source { source })
                                        }
                                        GlobalValue::Inline(_) => None,
                                    }),
                            }
                        }
                        ir::ErasedReadTarget::MaterializationLocal {
                            owner,
                            local,
                            projection,
                        } if additional_projection.is_empty() && !projection.is_empty() => {
                            let definition = self
                                .program
                                .scope_index
                                .locals
                                .iter()
                                .find(|definition| {
                                    definition.owner == *owner
                                        && definition.local == *local
                                })
                                .ok_or_else(|| {
                                    PlanError::new(format!(
                                        "view binding {} references missing materialization local {}:{}",
                                        binding.id.0, owner.0, local.0
                                    ))
                                })?;
                            match (scope, definition.row) {
                                (Some(scope), _) => {
                                    let target = self.resolve_view_materialization_target(
                                        binding.id.0,
                                        *owner,
                                        *local,
                                        scope,
                                        projection,
                                    )?;
                                    Some(target)
                                }
                                (None, None) => None,
                                (None, Some(row)) => {
                                    return Err(PlanError::new(format!(
                                        "view binding {} `{}`.{} materialization local {}:{} owns stored row {}/{} but has no exact row scope",
                                        binding.id.0,
                                        binding.node_kind,
                                        binding.attr,
                                        owner.0,
                                        local.0,
                                        row.list.0,
                                        row.scope.0,
                                    )));
                                }
                            }
                        }
                        _ => None,
                    };
                    if let Some(direct) = direct {
                        direct
                    } else {
                        let expression =
                            self.compile_expression(binding.argument_expression, &context, None)?;
                        DocumentBindingTarget::Expression { expression }
                    }
                }
                ir::ViewBindingTarget::Source { source } => DocumentBindingTarget::Source {
                    source: SourceId(source.0),
                },
            };
            result.push(DocumentViewBinding {
                id: DocumentBindingId(binding.id.0),
                template,
                attribute: self.intern_name(&binding.attr),
                kind: match binding.kind {
                    ir::ViewBindingKind::Data => DocumentBindingKind::Data,
                    ir::ViewBindingKind::Source => DocumentBindingKind::Source,
                    ir::ViewBindingKind::Target => DocumentBindingKind::Target,
                },
                target,
            });
        }
        result.sort_by_key(|binding| binding.id);
        Ok(result)
    }

    fn resolve_view_materialization_target(
        &self,
        binding_id: usize,
        owner: ir::StaticOwnerId,
        local: ir::MaterializationLocalId,
        scope: ScopeId,
        projection: &[String],
    ) -> Result<DocumentBindingTarget, PlanError> {
        let definition = self
            .program
            .scope_index
            .locals
            .iter()
            .find(|definition| definition.owner == owner && definition.local == local)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "view binding {binding_id} references missing materialization local {owner}:{local:?}"
                ))
            })?;
        if definition.row.map(|row| row.scope.0) != Some(scope.0) {
            let owner = self
                .program
                .scope_index
                .owners
                .get(owner.as_usize())
                .filter(|definition| definition.id == owner);
            return Err(PlanError::new(format!(
                "view binding {binding_id} materialization local {}:{} source row {:?} does not directly own scope {}; owner source row {:?}, target row {:?}, projection `{}`, members {:?}",
                definition.owner,
                local.0,
                definition.row,
                scope.0,
                owner.and_then(|owner| owner.source_row),
                owner.and_then(|owner| owner.target_row),
                projection.join("."),
                definition.members,
            )));
        }
        if projection.is_empty() {
            return Err(PlanError::new(format!(
                "view binding {binding_id} has an empty materialization-local projection"
            )));
        }
        let matches = definition
            .members
            .iter()
            .filter(|member| projection.starts_with(&member.path))
            .collect::<Vec<_>>();
        let consumed = matches
            .iter()
            .map(|member| member.path.len())
            .max()
            .unwrap_or(0);
        let candidates = matches
            .into_iter()
            .filter(|member| member.path.len() == consumed)
            .collect::<Vec<_>>();
        let [member] = candidates.as_slice() else {
            let available = definition
                .members
                .iter()
                .map(|member| member.path.join("."))
                .collect::<Vec<_>>();
            return Err(PlanError::new(format!(
                "view binding {binding_id} materialization local {owner}:{} projection `{}` resolves to {} longest exact targets for type {:?}; available {available:?}",
                local.0,
                projection.join("."),
                candidates.len(),
                definition.item_type
            )));
        };
        let target = member.target.clone();
        let rest = &projection[consumed..];
        let ir::ErasedLocalMemberTarget::Field(mut field) = target else {
            if !rest.is_empty() {
                return Err(PlanError::new(format!(
                    "view binding {binding_id} materialization resource member `{}` cannot project `{}` directly",
                    member.path.join("."),
                    rest.join(".")
                )));
            }
            return Ok(match &target {
                ir::ErasedLocalMemberTarget::Sources(sources) => DocumentBindingTarget::Sources {
                    sources: sources.iter().map(|source| SourceId(source.0)).collect(),
                },
                ir::ErasedLocalMemberTarget::State(state) => DocumentBindingTarget::State {
                    state: StateId(state.0),
                },
                ir::ErasedLocalMemberTarget::Field(_) => unreachable!(),
            });
        };
        for name in rest {
            let nested = self
                .program
                .scope_index
                .fields
                .iter()
                .filter(|candidate| candidate.parent == Some(field) && candidate.name == *name)
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>();
            let [next] = nested.as_slice() else {
                let field_definition = self
                    .program
                    .scope_index
                    .fields
                    .iter()
                    .find(|candidate| candidate.id == field);
                let child_fields = self
                    .program
                    .scope_index
                    .fields
                    .iter()
                    .filter(|candidate| candidate.parent == Some(field))
                    .map(|candidate| {
                        (
                            candidate.id,
                            candidate.name.as_str(),
                            candidate.resource_only,
                            candidate.producer,
                            candidate.static_owner,
                            candidate.row,
                        )
                    })
                    .collect::<Vec<_>>();
                let authority_siblings = field_definition
                    .map(|field_definition| {
                        self.program
                            .scope_index
                            .fields
                            .iter()
                            .filter(|candidate| {
                                candidate.row == field_definition.row
                                    && candidate.name == field_definition.name
                            })
                            .map(|candidate| {
                                (
                                    candidate.id,
                                    candidate.role,
                                    candidate.resource_only,
                                    candidate.producer,
                                    candidate.static_owner,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let relevant_members = definition
                    .members
                    .iter()
                    .filter(|member| {
                        member.path.starts_with(&projection[..1])
                            || projection.starts_with(member.path.as_slice())
                    })
                    .collect::<Vec<_>>();
                return Err(PlanError::new(format!(
                    "view binding {binding_id} materialization field {} projection `{}` resolves to {} exact child fields; field={field_definition:?}; children={child_fields:?}; siblings={authority_siblings:?}; local_members={relevant_members:?}",
                    field.0,
                    projection.join("."),
                    nested.len()
                )));
            };
            field = *next;
        }
        Ok(DocumentBindingTarget::ScopedField {
            scope,
            field: FieldId(field.0),
        })
    }

    fn expression(&self, id: ir::ExecutableExprId) -> Result<&ir::ExecutableExpression, PlanError> {
        self.program
            .executable
            .expressions
            .get(id.0)
            .filter(|expression| expression.id == id)
            .ok_or_else(|| {
                PlanError::new(format!("missing executable document expression {}", id.0))
            })
    }

    fn storage_binding_for_state_expression(
        &self,
        expression: ir::ExecutableExprId,
    ) -> Result<ir::ErasedBindingId, PlanError> {
        let state = self
            .program
            .executable
            .states
            .iter()
            .find(|state| state.expression == expression)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "state expression {expression} has no executable state definition"
                ))
            })?;
        let matches = self
            .program
            .scope_index
            .bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.target,
                    ir::ErasedBindingTarget::State { executable, .. } if executable == state.id
                )
            })
            .collect::<Vec<_>>();
        let [binding] = matches.as_slice() else {
            return Err(PlanError::new(format!(
                "executable state {} has {} exact storage bindings",
                state.id,
                matches.len()
            )));
        };
        Ok(binding.id)
    }

    fn expression_kind(
        &self,
        id: ir::ExecutableExprId,
    ) -> Result<&ir::ExecutableExpressionKind, PlanError> {
        self.expression(id).map(|expression| &expression.kind)
    }

    fn source_list_id(&self, expression: DocumentExprId) -> Option<ListId> {
        match &self.expressions.get(expression.0)?.op {
            DocumentExprOp::Read {
                read: DocumentRead::List { list },
            } => Some(*list),
            DocumentExprOp::Builtin {
                input: Some(input), ..
            }
            | DocumentExprOp::Project { input, .. } => self.source_list_id(*input),
            _ => None,
        }
    }

    fn allocate_cache_scope(&mut self) -> usize {
        let scope = self.next_cache_scope;
        self.next_cache_scope = self.next_cache_scope.saturating_add(1);
        scope
    }

    fn push_expr(
        &mut self,
        compiler_id: usize,
        value_class: DocumentValueClass,
        op: DocumentExprOp,
    ) -> DocumentExprId {
        let id = DocumentExprId(self.expressions.len());
        self.expressions.push(DocumentExpr {
            id,
            compiler_id,
            value_class,
            op,
        });
        id
    }

    fn constant_expr(
        &mut self,
        compiler_id: usize,
        value: DocumentConstantValue,
    ) -> DocumentExprId {
        let constant = self.push_constant(value);
        self.push_expr(
            compiler_id,
            DocumentValueClass::Static,
            DocumentExprOp::Constant { constant },
        )
    }

    fn push_constant(&mut self, value: DocumentConstantValue) -> DocumentConstantId {
        if let Some(constant) = self
            .constants
            .iter()
            .find(|constant| constant.value == value)
        {
            return constant.id;
        }
        let id = DocumentConstantId(self.constants.len());
        self.constants.push(DocumentConstant { id, value });
        id
    }

    fn intern_name(&mut self, value: &str) -> DocumentNameId {
        if let Some(id) = self.name_ids.get(value) {
            return *id;
        }
        let id = DocumentNameId(self.names.len());
        self.names.push(value.to_owned());
        self.name_ids.insert(value.to_owned(), id);
        id
    }

    fn record_compiled_path(&mut self, path: &str, expression: DocumentExprId) {
        self.compiled_paths
            .entry((None, path.to_owned()))
            .or_insert(expression);
    }
}

fn set_document_expression_redirect(
    redirects: &mut [Option<DocumentExprId>],
    source: DocumentExprId,
    target: DocumentExprId,
    kind: &str,
) -> Result<(), PlanError> {
    let slot = redirects.get_mut(source.0).ok_or_else(|| {
        PlanError::new(format!(
            "{kind} expression {} is outside the document arena",
            source.0
        ))
    })?;
    if let Some(previous) = slot.replace(target)
        && previous != target
    {
        return Err(PlanError::new(format!(
            "{kind} expression {} redirects to both {} and {}",
            source.0, previous.0, target.0
        )));
    }
    Ok(())
}

fn resolve_document_expression_redirect(
    mut id: DocumentExprId,
    redirects: &[Option<DocumentExprId>],
) -> Result<DocumentExprId, PlanError> {
    for _ in 0..=redirects.len() {
        let Some(next) = redirects.get(id.0).ok_or_else(|| {
            PlanError::new(format!(
                "document expression redirect references missing expression {}",
                id.0
            ))
        })?
        else {
            return Ok(id);
        };
        id = *next;
    }
    Err(PlanError::new(
        "single-use document expression redirects contain a cycle",
    ))
}

fn dense_document_expression_id(
    id: DocumentExprId,
    dense_ids: &[Option<DocumentExprId>],
) -> Result<DocumentExprId, PlanError> {
    dense_ids.get(id.0).copied().flatten().ok_or_else(|| {
        PlanError::new(format!(
            "document expression {} has no dense post-compaction id",
            id.0
        ))
    })
}

fn final_document_expression_id(
    id: DocumentExprId,
    final_ids: &[DocumentExprId],
) -> Result<DocumentExprId, PlanError> {
    final_ids.get(id.0).copied().ok_or_else(|| {
        PlanError::new(format!(
            "document expression {} has no final post-compaction id",
            id.0
        ))
    })
}

fn resolve_document_function_owner(
    mut owner: DocumentFunctionId,
    redirects: &BTreeMap<DocumentFunctionId, Option<DocumentFunctionId>>,
    limit: usize,
) -> Result<Option<DocumentFunctionId>, PlanError> {
    for _ in 0..=limit {
        match redirects.get(&owner) {
            None => return Ok(Some(owner)),
            Some(None) => return Ok(None),
            Some(Some(parent)) => owner = *parent,
        }
    }
    Err(PlanError::new(
        "single-use document function owner redirects contain a cycle",
    ))
}

fn remap_document_external_expression_refs(
    root: &mut DocumentRoot,
    initial_patch_batch: &mut DocumentInitialPatchBatch,
    functions: &mut [DocumentFunction],
    templates: &mut [DocumentTemplate],
    materializations: &mut [DocumentMaterialization],
    view_bindings: &mut [DocumentViewBinding],
    mut map: impl FnMut(DocumentExprId) -> Result<DocumentExprId, PlanError>,
) -> Result<(), PlanError> {
    root.expression = map(root.expression)?;
    for patch in &mut initial_patch_batch.patches {
        if let DocumentInitialPatch::MountRoot { expression, .. } = patch {
            *expression = map(*expression)?;
        }
    }
    for function in functions {
        function.body = map(function.body)?;
    }
    for template in templates {
        template.expression = map(template.expression)?;
    }
    for materialization in materializations {
        for argument in &mut materialization.template_arguments {
            argument.value = map(argument.value)?;
        }
        if let DocumentMaterializationSource::Expression { expression } =
            &mut materialization.source
        {
            *expression = map(*expression)?;
        }
    }
    for binding in view_bindings {
        if let DocumentBindingTarget::Expression { expression } = &mut binding.target {
            *expression = map(*expression)?;
        }
    }
    Ok(())
}

fn remap_document_expression_op(
    op: &mut DocumentExprOp,
    map: &mut impl FnMut(DocumentExprId) -> Result<DocumentExprId, PlanError>,
) -> Result<(), PlanError> {
    match op {
        DocumentExprOp::Absent
        | DocumentExprOp::Constant { .. }
        | DocumentExprOp::Read { .. }
        | DocumentExprOp::Materialize { .. }
        | DocumentExprOp::RuntimeExpression { .. }
        | DocumentExprOp::NoElement => {}
        DocumentExprOp::Project { input, .. } => *input = map(*input)?,
        DocumentExprOp::Record { fields } | DocumentExprOp::TaggedRecord { fields, .. } => {
            for field in fields {
                field.value = map(field.value)?;
            }
        }
        DocumentExprOp::List { items } => {
            for item in items {
                item.value = map(item.value)?;
            }
        }
        DocumentExprOp::TextTemplate { segments } => {
            for segment in segments {
                if let DocumentTextSegment::Dynamic { value } = segment {
                    *value = map(*value)?;
                }
            }
        }
        DocumentExprOp::LocalBlock { bindings, result } => {
            for binding in bindings {
                binding.value = map(binding.value)?;
            }
            *result = map(*result)?;
        }
        DocumentExprOp::Call { arguments, .. } => {
            for argument in arguments {
                argument.value = map(argument.value)?;
            }
        }
        DocumentExprOp::Builtin {
            input, arguments, ..
        } => {
            if let Some(value) = input {
                *value = map(*value)?;
            }
            for argument in arguments {
                argument.value = map(argument.value)?;
            }
        }
        DocumentExprOp::Scalar { left, right, .. } => {
            *left = map(*left)?;
            if let Some(value) = right {
                *value = map(*value)?;
            }
        }
        DocumentExprOp::Select { input, arms } => {
            *input = map(*input)?;
            for arm in arms {
                arm.output = map(arm.output)?;
            }
        }
        DocumentExprOp::Latest { branches } => {
            for branch in branches {
                *branch = map(*branch)?;
            }
        }
        DocumentExprOp::Then { input, output } => {
            *input = map(*input)?;
            if let Some(value) = output {
                *value = map(*value)?;
            }
        }
        DocumentExprOp::Constructor { arguments, .. } => {
            for argument in arguments {
                argument.value = map(argument.value)?;
            }
        }
    }
    Ok(())
}

fn synthetic_scope_id(owner: ir::StaticOwnerId) -> Result<ScopeId, PlanError> {
    let namespace = 1usize << (usize::BITS - 1);
    if owner.0 >= namespace {
        return Err(PlanError::new(
            "static owner exceeds the synthetic document scope namespace",
        ));
    }
    Ok(ScopeId(namespace | owner.0))
}

fn ordinary_function_requires_overlay(
    program: &ErasedProgram,
    function: ir::FunctionId,
    memo: &mut BTreeMap<ir::FunctionId, bool>,
) -> Result<bool, PlanError> {
    fn visit_function(
        program: &ErasedProgram,
        function: ir::FunctionId,
        memo: &mut BTreeMap<ir::FunctionId, bool>,
        active: &mut BTreeSet<ir::FunctionId>,
    ) -> Result<bool, PlanError> {
        if let Some(requires_overlay) = memo.get(&function) {
            return Ok(*requires_overlay);
        }
        if !active.insert(function) {
            // Recursive ordinary document call graphs fail during lowering;
            // keep this preliminary capability walk finite and conservative.
            return Ok(true);
        }
        let definition = program
            .executable
            .ordinary_functions
            .iter()
            .find(|candidate| candidate.id == function)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "document overlay analysis references missing function {}",
                    function.0
                ))
            })?;
        let mut pending = vec![definition.root];
        let mut visited = BTreeSet::new();
        let mut requires_overlay = false;
        while let Some(expression_id) = pending.pop() {
            if !visited.insert(expression_id) {
                continue;
            }
            let expression = program
                .executable
                .expressions
                .get(expression_id.as_usize())
                .filter(|candidate| candidate.id == expression_id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "document overlay analysis for function {} reaches missing expression {}",
                        function.0, expression_id.0
                    ))
                })?;
            match &expression.kind {
                ir::ExecutableExpressionKind::ElementState { .. }
                | ir::ExecutableExpressionKind::LocalRead { .. }
                | ir::ExecutableExpressionKind::MaterializationLocal { .. }
                | ir::ExecutableExpressionKind::Materialize { .. } => {
                    requires_overlay = true;
                    break;
                }
                ir::ExecutableExpressionKind::Call {
                    name,
                    contexts,
                    context_ordinals,
                    ..
                } if document_constructor(name).is_some()
                    || !contexts.is_empty()
                    || !context_ordinals.is_empty() =>
                {
                    requires_overlay = true;
                    break;
                }
                ir::ExecutableExpressionKind::UserCall {
                    function: dependency,
                    ..
                } if visit_function(program, *dependency, memo, active)? => {
                    requires_overlay = true;
                    break;
                }
                _ => {}
            }
            pending.extend(ir::executable_expression_children(&expression.kind));
        }
        active.remove(&function);
        memo.insert(function, requires_overlay);
        Ok(requires_overlay)
    }

    visit_function(program, function, memo, &mut BTreeSet::new())
}

fn parameter_id(
    function: DocumentFunctionId,
    local_index: usize,
) -> Result<DocumentParameterId, PlanError> {
    const PARAMETER_STRIDE: usize = 4096;
    if local_index >= PARAMETER_STRIDE {
        return Err(PlanError::new(format!(
            "synthetic document function {} exceeds the typed local limit",
            function.0
        )));
    }
    function
        .0
        .checked_mul(PARAMETER_STRIDE)
        .and_then(|base| base.checked_add(local_index))
        .map(DocumentParameterId)
        .ok_or_else(|| PlanError::new("synthetic document parameter id overflow"))
}

fn stable_compiler_identity(
    kind: u8,
    owner: Option<ir::StaticOwnerId>,
    compiler_id: usize,
) -> Result<u64, PlanError> {
    let owner = owner.map(|owner| owner.0 + 1).unwrap_or(0);
    if owner > 0x00ff_ffff || compiler_id > u32::MAX as usize {
        return Err(PlanError::new(
            "executable document identity exceeds its stable encoding",
        ));
    }
    Ok((u64::from(kind) << 56) | ((owner as u64) << 32) | compiler_id as u64)
}

fn stable_invocation_identity(
    kind: u8,
    owner: Option<ir::StaticOwnerId>,
    invocation: Option<usize>,
    compiler_id: usize,
) -> Result<u64, PlanError> {
    let Some(invocation) = invocation else {
        return stable_compiler_identity(kind, owner, compiler_id);
    };
    let invocation_kind: u8 = match (kind, owner.is_some()) {
        (3, false) => 6,
        (4, false) => 7,
        (3, true) => 8,
        (4, true) => 9,
        _ => {
            return Err(PlanError::new(
                "unsupported invocation-scoped document identity kind",
            ));
        }
    };
    if let Some(owner) = owner {
        let owner_invocation = cantor_pair(owner.0 as u128, invocation as u128)?;
        let payload = cantor_pair(owner_invocation, compiler_id as u128)?;
        if payload > 0x00ff_ffff_ffff_ffff {
            return Err(PlanError::new(
                "owned document invocation identity exceeds its stable encoding",
            ));
        }
        return Ok((u64::from(invocation_kind) << 56) | payload as u64);
    }
    if invocation >= 0x00ff_ffff || compiler_id > u32::MAX as usize {
        return Err(PlanError::new(
            "document invocation identity exceeds its stable encoding",
        ));
    }
    Ok((u64::from(invocation_kind) << 56) | (((invocation + 1) as u64) << 32) | compiler_id as u64)
}

fn cantor_pair(left: u128, right: u128) -> Result<u128, PlanError> {
    let sum = left
        .checked_add(right)
        .ok_or_else(|| PlanError::new("document identity pairing overflow"))?;
    sum.checked_mul(
        sum.checked_add(1)
            .ok_or_else(|| PlanError::new("document identity pairing overflow"))?,
    )
    .and_then(|product| product.checked_div(2))
    .and_then(|triangle| triangle.checked_add(right))
    .ok_or_else(|| PlanError::new("document identity pairing overflow"))
}

fn joined_path(path: &str, projection: &[String]) -> String {
    if projection.is_empty() {
        path.to_owned()
    } else {
        format!("{path}.{}", projection.join("."))
    }
}

fn document_constructor(function: &str) -> Option<DocumentConstructor> {
    Some(match function {
        "Document/new" => DocumentConstructor::DocumentNew,
        "Element/container" => DocumentConstructor::ElementContainer,
        "Element/stripe" => DocumentConstructor::ElementStripe,
        "Element/text" => DocumentConstructor::ElementText,
        "Element/label" => DocumentConstructor::ElementLabel,
        "Element/paragraph" => DocumentConstructor::ElementParagraph,
        "Element/link" => DocumentConstructor::ElementLink,
        "Element/button" => DocumentConstructor::ElementButton,
        "Element/checkbox" => DocumentConstructor::ElementCheckbox,
        "Element/text_input" => DocumentConstructor::ElementTextInput,
        "Element/program" => DocumentConstructor::ElementProgram,
        "Element/embedded_media" => DocumentConstructor::ElementEmbeddedMedia,
        "Element/map" => DocumentConstructor::ElementMap,
        "Scene/new" => DocumentConstructor::SceneNew,
        "Scene/Element/stripe" => DocumentConstructor::SceneElementStripe,
        "Scene/Element/block" => DocumentConstructor::SceneElementBlock,
        "Scene/Element/text" => DocumentConstructor::SceneElementText,
        "Scene/Element/text_input" => DocumentConstructor::SceneElementTextInput,
        "Scene/Element/program" => DocumentConstructor::SceneElementProgram,
        "Scene/Element/checkbox" => DocumentConstructor::SceneElementCheckbox,
        "Scene/Element/label" => DocumentConstructor::SceneElementLabel,
        "Scene/Element/button" => DocumentConstructor::SceneElementButton,
        "Scene/Element/paragraph" => DocumentConstructor::SceneElementParagraph,
        "Scene/Element/link" => DocumentConstructor::SceneElementLink,
        "Scene/Element/embedded_media" => DocumentConstructor::SceneElementEmbeddedMedia,
        "Scene/Element/map" => DocumentConstructor::SceneElementMap,
        _ => return None,
    })
}

fn document_element_context(context: ir::ExecutableCallContextId) -> DocumentElementContextId {
    DocumentElementContextId {
        call_instance: context.call_instance,
        ordinal: context.ordinal,
    }
}

fn constructor_argument_role(
    constructor: DocumentConstructor,
    name: &str,
    class: DocumentValueClass,
) -> Result<DocumentArgumentRole, PlanError> {
    if let Some(role) = constructor.map_viewport_argument_role(name) {
        return Ok(role);
    }
    if constructor.is_map_viewport()
        && !matches!(
            name,
            "style" | "element" | "events" | "child" | "root" | "items" | "children" | "contents"
        )
    {
        return Err(PlanError::new(format!(
            "MapViewport constructor has unknown argument `{name}`"
        )));
    }
    Ok(match name {
        "style" => {
            if class == DocumentValueClass::Static {
                DocumentArgumentRole::StaticStyle
            } else {
                DocumentArgumentRole::DynamicStyle
            }
        }
        "text" | "label" | "placeholder" => match class {
            DocumentValueClass::Render => DocumentArgumentRole::Child,
            DocumentValueClass::ChildList => DocumentArgumentRole::Children,
            DocumentValueClass::Static => DocumentArgumentRole::StaticText,
            DocumentValueClass::DynamicScalar | DocumentValueClass::DynamicStructure => {
                DocumentArgumentRole::DynamicText
            }
        },
        "child" | "root" => DocumentArgumentRole::Child,
        "items" | "children" | "contents" => DocumentArgumentRole::Children,
        "element" | "events" => DocumentArgumentRole::EventBindings,
        _ => DocumentArgumentRole::Value,
    })
}

fn document_builtin(function: &str) -> Option<DocumentBuiltin> {
    Some(match function {
        "Bool/and" => DocumentBuiltin::BoolAnd,
        "Bool/not" => DocumentBuiltin::BoolNot,
        "Bool/toggle" => DocumentBuiltin::BoolToggle,
        "Bytes/find" => DocumentBuiltin::BytesFind,
        "Bytes/slice" => DocumentBuiltin::BytesSlice,
        "Bytes/starts_with" => DocumentBuiltin::BytesStartsWith,
        "Bytes/to_text" => DocumentBuiltin::BytesToText,
        "Directory/entries" => DocumentBuiltin::DirectoryEntries,
        "Light/ambient" => DocumentBuiltin::LightAmbient,
        "Light/directional" => DocumentBuiltin::LightDirectional,
        "Light/spot" => DocumentBuiltin::LightSpot,
        "List/append" => DocumentBuiltin::ListAppend,
        "List/chunk" => DocumentBuiltin::ListChunk,
        "List/count" => DocumentBuiltin::ListCount,
        "List/get" => DocumentBuiltin::ListGet,
        "List/is_not_empty" => DocumentBuiltin::ListIsNotEmpty,
        "List/latest" => DocumentBuiltin::ListLatest,
        "List/length" => DocumentBuiltin::ListLength,
        "List/range" => DocumentBuiltin::ListRange,
        "List/sort_by" => DocumentBuiltin::ListSortBy,
        "List/sum" => DocumentBuiltin::ListSum,
        "Number/bit_width" => DocumentBuiltin::NumberBitWidth,
        "Number/ceil" => DocumentBuiltin::NumberCeil,
        "Number/floor" => DocumentBuiltin::NumberFloor,
        "Number/interpolate" => DocumentBuiltin::NumberInterpolate,
        "Number/max" => DocumentBuiltin::NumberMax,
        "Number/min" => DocumentBuiltin::NumberMin,
        "Number/project_offset" => DocumentBuiltin::NumberProjectOffset,
        "Number/project_time" => DocumentBuiltin::NumberProjectTime,
        "Number/project_width" => DocumentBuiltin::NumberProjectWidth,
        "Number/round" => DocumentBuiltin::NumberRound,
        "Number/to_ascii_text" => DocumentBuiltin::NumberToAsciiText,
        "Number/to_text" => DocumentBuiltin::NumberToText,
        "Number/truncate" => DocumentBuiltin::NumberTruncate,
        "Router/go_to" => DocumentBuiltin::RouterGoTo,
        "Router/route" => DocumentBuiltin::RouterRoute,
        "C/svg" => DocumentBuiltin::Svg,
        "Text/all_chars_in" => DocumentBuiltin::TextAllCharsIn,
        "Text/concat" => DocumentBuiltin::TextConcat,
        "Text/contains" => DocumentBuiltin::TextContains,
        "Text/empty" => DocumentBuiltin::TextEmpty,
        "Text/find" => DocumentBuiltin::TextFind,
        "Text/is_empty" => DocumentBuiltin::TextIsEmpty,
        "Text/join" => DocumentBuiltin::TextJoin,
        "Text/join_lines" => DocumentBuiltin::TextJoinLines,
        "Text/length" => DocumentBuiltin::TextLength,
        "Text/space" => DocumentBuiltin::TextSpace,
        "Text/starts_with" => DocumentBuiltin::TextStartsWith,
        "Text/slice" => DocumentBuiltin::TextSlice,
        "Text/time_range_label" => DocumentBuiltin::TextTimeRangeLabel,
        "Text/to_bytes" => DocumentBuiltin::TextToBytes,
        "Text/to_lowercase" => DocumentBuiltin::TextToLowercase,
        "Text/to_number" => DocumentBuiltin::TextToNumber,
        "Text/to_uppercase" => DocumentBuiltin::TextToUppercase,
        "Text/trim" => DocumentBuiltin::TextTrim,
        "Ulid/generate" => DocumentBuiltin::UlidGenerate,
        "Url/encode" => DocumentBuiltin::UrlEncode,
        _ => return None,
    })
}

fn scalar_operation(operator: &str) -> Result<DocumentScalarOp, PlanError> {
    Ok(match operator {
        "+" => DocumentScalarOp::Add,
        "-" => DocumentScalarOp::Subtract,
        "*" => DocumentScalarOp::Multiply,
        "/" => DocumentScalarOp::Divide,
        "%" => DocumentScalarOp::Remainder,
        "==" => DocumentScalarOp::Equal,
        "!=" => DocumentScalarOp::NotEqual,
        "<" => DocumentScalarOp::Less,
        "<=" => DocumentScalarOp::LessOrEqual,
        ">" => DocumentScalarOp::Greater,
        ">=" => DocumentScalarOp::GreaterOrEqual,
        "&&" | "and" => DocumentScalarOp::And,
        "||" | "or" => DocumentScalarOp::Or,
        other => {
            return Err(PlanError::new(format!(
                "unsupported executable document scalar operator `{other}`"
            )));
        }
    })
}

fn exact_block_binding_order<'a>(
    program: &ErasedProgram,
    bindings: &'a [ir::ExecutableBlockBinding],
) -> Result<Vec<&'a ir::ExecutableBlockBinding>, PlanError> {
    let binding_ids = bindings
        .iter()
        .map(|binding| binding.id)
        .collect::<BTreeSet<_>>();
    let mut dependencies = BTreeMap::new();
    for binding in bindings {
        let mut pending = vec![binding.value];
        let mut visited = BTreeSet::new();
        let mut local_dependencies = BTreeSet::new();
        while let Some(expression_id) = pending.pop() {
            if !visited.insert(expression_id) {
                continue;
            }
            let expression = program
                .executable
                .expressions
                .get(expression_id.as_usize())
                .filter(|expression| expression.id == expression_id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "BLOCK declaration {} reaches missing executable expression {expression_id}",
                        binding.declaration.0
                    ))
                })?;
            if let ir::ExecutableExpressionKind::LocalRead { binding, .. } = expression.kind
                && binding_ids.contains(&binding)
            {
                local_dependencies.insert(binding);
            }
            pending.extend(ir::executable_expression_children(&expression.kind));
        }
        dependencies.insert(binding.id, local_dependencies);
    }

    let mut emitted = BTreeSet::new();
    let mut ordered = Vec::with_capacity(bindings.len());
    while ordered.len() < bindings.len() {
        let Some(binding) = bindings.iter().find(|binding| {
            !emitted.contains(&binding.id)
                && dependencies[&binding.id]
                    .iter()
                    .all(|dependency| emitted.contains(dependency))
        }) else {
            let remaining = bindings
                .iter()
                .filter(|binding| !emitted.contains(&binding.id))
                .map(|binding| (binding.id.0, binding.declaration.0))
                .collect::<Vec<_>>();
            return Err(PlanError::new(format!(
                "erased BLOCK contains a lexical value cycle across bindings and declarations {remaining:?}"
            )));
        };
        emitted.insert(binding.id);
        ordered.push(binding);
    }
    Ok(ordered)
}

fn value_class_for_type(ty: &Type) -> DocumentValueClass {
    match ty {
        Type::RenderContract => DocumentValueClass::Render,
        Type::List(item) if matches!(item.as_ref(), Type::RenderContract) => {
            DocumentValueClass::ChildList
        }
        Type::List(_) | Type::Map { .. } | Type::Set(_) | Type::Object(_) => {
            DocumentValueClass::DynamicStructure
        }
        Type::Union(members) => {
            if members.iter().all(|member| {
                matches!(
                    value_class_for_type(member),
                    DocumentValueClass::Static | DocumentValueClass::DynamicScalar
                )
            }) {
                DocumentValueClass::DynamicScalar
            } else {
                DocumentValueClass::DynamicStructure
            }
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::VariantSet(_)
        | Type::Function { .. }
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => DocumentValueClass::DynamicScalar,
    }
}

fn project_invocation_type(mut ty: Type, projection: &[String]) -> Option<Type> {
    for field in projection {
        ty = match ty {
            Type::Object(shape) => shape.fields.get(field)?.clone(),
            Type::Union(members) => {
                let projected = members
                    .iter()
                    .filter_map(|member| match member {
                        Type::Object(shape) => shape.fields.get(field).cloned(),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if projected.is_empty() {
                    return None;
                }
                boon_checked::canonical_union_type(projected)
            }
            _ => return None,
        };
    }
    Some(ty)
}

fn executable_debug_label(program: &ErasedProgram, id: ir::ExecutableExprId) -> String {
    let Some(expression) = program.executable.expressions.get(id.as_usize()) else {
        return format!("{id}:missing");
    };
    let kind = match &expression.kind {
        ir::ExecutableExpressionKind::Call { name, .. } => format!("call {name}"),
        ir::ExecutableExpressionKind::Materialize { materialization } => program
            .materializations
            .get(*materialization)
            .map(|value| format!("contextual {:?}", value.operation))
            .unwrap_or_else(|| format!("materialization {materialization}")),
        ir::ExecutableExpressionKind::CanonicalRead {
            path, projection, ..
        } => format!("read {path}.{}", projection.join(".")),
        ir::ExecutableExpressionKind::Hold { name, .. } => format!("HOLD {name}"),
        ir::ExecutableExpressionKind::When { .. } => "WHEN".to_owned(),
        ir::ExecutableExpressionKind::Latest { .. } => "LATEST".to_owned(),
        ir::ExecutableExpressionKind::Then { .. } => "THEN".to_owned(),
        ir::ExecutableExpressionKind::Project { fields, .. } => {
            format!("project {}", fields.join("."))
        }
        ir::ExecutableExpressionKind::Object { .. } => "object".to_owned(),
        ir::ExecutableExpressionKind::List { .. } => "list".to_owned(),
        _ => format!("{:?}", std::mem::discriminant(&expression.kind)),
    };
    format!("{id}:{kind}")
}

fn record_value_class(
    fields: &[DocumentRecordField],
    expressions: &[DocumentExpr],
) -> DocumentValueClass {
    let classes = fields
        .iter()
        .map(|field| expressions[field.value.0].value_class)
        .collect::<Vec<_>>();
    if classes
        .iter()
        .all(|class| *class == DocumentValueClass::Static)
    {
        DocumentValueClass::Static
    } else if classes.iter().any(|class| {
        matches!(
            class,
            DocumentValueClass::Render | DocumentValueClass::ChildList
        )
    }) {
        DocumentValueClass::DynamicStructure
    } else {
        DocumentValueClass::DynamicScalar
    }
}

fn list_value_class(
    items: &[DocumentListItem],
    expressions: &[DocumentExpr],
) -> DocumentValueClass {
    if items
        .iter()
        .all(|item| expressions[item.value.0].value_class == DocumentValueClass::Static)
    {
        DocumentValueClass::Static
    } else {
        DocumentValueClass::ChildList
    }
}

fn value_class_rank(class: DocumentValueClass) -> u8 {
    match class {
        DocumentValueClass::Static => 0,
        DocumentValueClass::DynamicScalar => 1,
        DocumentValueClass::DynamicStructure => 2,
        DocumentValueClass::Render => 3,
        DocumentValueClass::ChildList => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_fields_receive_dedicated_roles_and_unknown_fields_fail() {
        assert_eq!(
            constructor_argument_role(
                DocumentConstructor::ElementMap,
                "camera",
                DocumentValueClass::DynamicScalar,
            )
            .unwrap(),
            DocumentArgumentRole::MapCamera
        );
        assert_eq!(
            constructor_argument_role(
                DocumentConstructor::SceneElementMap,
                "overlays",
                DocumentValueClass::ChildList,
            )
            .unwrap(),
            DocumentArgumentRole::MapOverlays
        );
        assert!(
            constructor_argument_role(
                DocumentConstructor::ElementMap,
                "provider_secret",
                DocumentValueClass::Static,
            )
            .is_err()
        );
    }

    #[test]
    fn synthetic_scopes_are_disjoint_from_ir_scopes() {
        let scope = synthetic_scope_id(ir::StaticOwnerId(7)).unwrap();
        assert_ne!(scope.0 & (1usize << (usize::BITS - 1)), 0);
        assert_eq!(scope.0 & !(1usize << (usize::BITS - 1)), 7);
    }

    #[test]
    fn owned_invocation_identities_preserve_all_three_coordinates() {
        let identity = stable_invocation_identity(3, Some(ir::StaticOwnerId(7)), Some(11), 13)
            .expect("owned invocation identity");
        assert_eq!(
            identity,
            stable_invocation_identity(3, Some(ir::StaticOwnerId(7)), Some(11), 13).unwrap()
        );
        assert_ne!(
            identity,
            stable_invocation_identity(3, Some(ir::StaticOwnerId(8)), Some(11), 13).unwrap()
        );
        assert_ne!(
            identity,
            stable_invocation_identity(3, Some(ir::StaticOwnerId(7)), Some(12), 13).unwrap()
        );
        assert_ne!(
            identity,
            stable_invocation_identity(3, Some(ir::StaticOwnerId(7)), Some(11), 14).unwrap()
        );
    }
}
