use crate::machine_plan_backend::{ValueIndex, lower_document_runtime_expression};
use boon_ir::{self as ir, ErasedProgram};
use boon_plan::*;
use boon_typecheck::{Type, is_renderable_type};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn compile_document_plan(
    program: &ErasedProgram,
    value_index: &ValueIndex,
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

#[derive(Clone, Default)]
struct CompileContext {
    cache_scope: usize,
    stable_owner: Option<ir::StaticOwnerId>,
    owner_function: Option<DocumentFunctionId>,
    materialization_locals:
        BTreeMap<(ir::StaticOwnerId, ir::MaterializationLocalId), DocumentParameterId>,
    locals: BTreeMap<boon_typecheck::DeclId, DocumentLocalId>,
    pattern_bindings: BTreeMap<String, PatternBindingContext>,
}

#[derive(Clone)]
struct PatternBindingContext {
    selector: usize,
    projection: Vec<String>,
}

struct DocumentCompiler<'a> {
    program: &'a ErasedProgram,
    value_index: &'a ValueIndex,
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
    functions: Vec<DocumentFunction>,
    function_ids: BTreeSet<DocumentFunctionId>,
    templates: Vec<DocumentTemplate>,
    template_ids: BTreeSet<DocumentTemplateId>,
    materializations: Vec<DocumentMaterialization>,
    materialization_ids: BTreeSet<DocumentMaterializationId>,
    materializations_in_progress: BTreeSet<usize>,
    compiled_materializations: BTreeSet<usize>,
    compiled_paths: BTreeMap<(Option<ScopeId>, String), DocumentExprId>,
    compile_stack: Vec<ir::ExecutableExprId>,
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
        match member.target {
            ir::ErasedLocalMemberTarget::Source(source) if rest.is_empty() => {
                Ok(Some(DocumentRead::Source {
                    source: SourceId(source.0),
                }))
            }
            ir::ErasedLocalMemberTarget::State(state) if rest.is_empty() => {
                if self
                    .program
                    .state_cells
                    .get(state.as_usize())
                    .is_some_and(|candidate| candidate.id == state && candidate.scope_id.is_some())
                {
                    return Ok(None);
                }
                Ok(Some(DocumentRead::State {
                    state: StateId(state.0),
                }))
            }
            ir::ErasedLocalMemberTarget::Source(_) | ir::ErasedLocalMemberTarget::State(_) => {
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
                } => Some(GlobalValue::Field(FieldId(field.0))),
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
            functions: Vec::new(),
            function_ids: BTreeSet::new(),
            templates: Vec::new(),
            template_ids: BTreeSet::new(),
            materializations: Vec::new(),
            materialization_ids: BTreeSet::new(),
            materializations_in_progress: BTreeSet::new(),
            compiled_materializations: BTreeSet::new(),
            compiled_paths: BTreeMap::new(),
            compile_stack: Vec::new(),
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
        let root = DocumentRoot {
            kind: root_kind,
            node: root_node,
            template: root_template,
            expression: root_expression,
        };
        let view_bindings = self.compile_view_bindings()?;
        let initial_patch_batch = DocumentPlan::build_initial_patch_batch(
            root,
            &self.templates,
            &view_bindings,
            &self.materializations,
        );
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
                    value_class_for_type(&expression.flow_type.ty),
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
                    ir::ExecutableExpressionKind::Object(fields)
                    | ir::ExecutableExpressionKind::Record(fields)
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
                        declaration,
                        projection: existing,
                    } => {
                        let local = context.locals.get(declaration).copied().ok_or_else(|| {
                            PlanError::new(format!(
                                "executable expression {} reads inactive lexical declaration {}",
                                expression_id.0, declaration.0
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
                                    "executable expression {} reads unbound materialization owner {} local {}",
                                    expression_id.0, owner.0, local.0
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
        match &expression.kind {
            ir::ExecutableExpressionKind::CanonicalRead { .. } => self.compile_erased_read(
                expression.id,
                context,
                value_class_for_type(&expression.flow_type.ty),
            ),
            ir::ExecutableExpressionKind::LocalRead {
                declaration,
                projection,
            } => {
                let local = context.locals.get(declaration).copied().ok_or_else(|| {
                    PlanError::new(format!(
                        "executable expression {compiler_id} reads inactive lexical declaration {}",
                        declaration.0
                    ))
                })?;
                let projection = projection
                    .iter()
                    .map(|field| self.intern_name(field))
                    .collect();
                Ok(self.push_expr(
                    compiler_id,
                    value_class_for_type(&expression.flow_type.ty),
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
                    value_class_for_type(&expression.flow_type.ty),
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
                    value_class_for_type(&expression.flow_type.ty),
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
            ir::ExecutableExpressionKind::Text(value) => Ok(self.constant_expr(
                compiler_id,
                DocumentConstantValue::Text {
                    value: value.clone(),
                },
            )),
            ir::ExecutableExpressionKind::TextTemplate { segments } => {
                self.compile_text_template(compiler_id, segments, context)
            }
            ir::ExecutableExpressionKind::Number(value) => {
                let (coefficient, scale) = parse_decimal(value)?;
                Ok(self.constant_expr(
                    compiler_id,
                    DocumentConstantValue::Number { coefficient, scale },
                ))
            }
            ir::ExecutableExpressionKind::BytesByte(value) => Ok(self.constant_expr(
                compiler_id,
                DocumentConstantValue::Bytes {
                    value: vec![*value],
                },
            )),
            ir::ExecutableExpressionKind::Bool(value) => {
                Ok(self.constant_expr(compiler_id, DocumentConstantValue::Bool { value: *value }))
            }
            ir::ExecutableExpressionKind::Tag(value) => self.compile_tag(compiler_id, value),
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
                callable_kind,
                name,
                arguments,
                contexts,
            } => self.compile_call(
                expression,
                *callable_kind,
                name,
                arguments,
                contexts,
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
                        value_class_for_type(&expression.flow_type.ty),
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
                    value_class_for_type(&expression.flow_type.ty),
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
                    value_class_for_type(&expression.flow_type.ty),
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
                    value_class_for_type(&expression.flow_type.ty),
                    DocumentExprOp::Then { input, output },
                ))
            }
            ir::ExecutableExpressionKind::Infix { left, op, right } => {
                let left = self.compile_expression(*left, context, None)?;
                let right = self.compile_expression(*right, context, None)?;
                Ok(self.push_expr(
                    compiler_id,
                    value_class_for_type(&expression.flow_type.ty),
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
            ir::ExecutableExpressionKind::Object(fields)
            | ir::ExecutableExpressionKind::Record(fields) => {
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
            ir::ExecutableExpressionKind::Bytes { items, .. } => {
                let bytes = items
                    .iter()
                    .map(|item| match self.expression_kind(*item)? {
                        ir::ExecutableExpressionKind::BytesByte(value) => Ok(*value),
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
                    value_class_for_type(&expression.flow_type.ty),
                ),
            ir::ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => {
                if let Some(read) =
                    self.materialization_resource_read(*owner, *local, projection)?
                {
                    return Ok(self.push_expr(
                        compiler_id,
                        value_class_for_type(&expression.flow_type.ty),
                        DocumentExprOp::Read { read },
                    ));
                }
                let parameter = context
                    .materialization_locals
                    .get(&(*owner, *local))
                    .copied()
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "executable expression {compiler_id} reads unbound materialization owner {} local {}",
                            owner.0, local.0
                        ))
                    })?;
                let projection = projection
                    .iter()
                    .map(|field| self.intern_name(field))
                    .collect();
                Ok(self.push_expr(
                    compiler_id,
                    value_class_for_type(&expression.flow_type.ty),
                    DocumentExprOp::Read {
                        read: DocumentRead::Parameter {
                            parameter,
                            projection,
                        },
                    },
                ))
            }
            ir::ExecutableExpressionKind::FunctionParameter { parameter, .. } => {
                Err(PlanError::new(format!(
                    "standalone executable function parameter {}:{} reached retained document lowering",
                    parameter.function.0, parameter.ordinal
                )))
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
                .insert(binding.declaration, DocumentLocalId(self.next_local))
                .is_some()
            {
                return Err(PlanError::new(format!(
                    "erased BLOCK expression {} repeats lexical declaration {}",
                    expression.id, binding.declaration.0
                )));
            }
            self.next_local += 1;
        }

        let mut lowered = Vec::with_capacity(bindings.len());
        for binding in exact_block_binding_order(self.program, bindings)? {
            let local = context.locals[&binding.declaration];
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

    fn compile_call(
        &mut self,
        expression: &ir::ExecutableExpression,
        callable_kind: ir::ExecutableCallableKind,
        function: &str,
        arguments: &[ir::ExecutableCallArgument],
        contexts: &[ir::ExecutableCallContextId],
        context: &CompileContext,
        input_override: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        let compiler_id = expression.id.0;
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
                value_class_for_type(&expression.flow_type.ty),
                DocumentExprOp::Project { input, field },
            ));
        }
        if let Some(constructor) = document_constructor(function) {
            if arguments.iter().any(|argument| argument.from_pipe) {
                return Err(PlanError::new(format!(
                    "render constructor `{function}` cannot be used as a pipeline operator"
                )));
            }
            return self.compile_constructor(expression, constructor, arguments, contexts, context);
        }
        if !contexts.is_empty() {
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
            value_class_for_type(&expression.flow_type.ty),
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
        let template = DocumentTemplateId(stable_compiler_identity(3, stable_owner, compiler_id)?);
        let node = DocumentNodeId(stable_compiler_identity(4, stable_owner, compiler_id)?);
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
            .unwrap_or_else(|| value_class_for_type(&expression.flow_type.ty));
        Ok(self.push_expr(
            expression.id.0,
            class,
            DocumentExprOp::Select { input, arms },
        ))
    }

    fn compile_pattern(
        &mut self,
        pattern: &boon_typecheck::CheckedMatchPattern,
    ) -> Result<DocumentPattern, PlanError> {
        use boon_typecheck::CheckedMatchPattern;

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
        if let CheckedMatchPattern::Bool { value } = pattern {
            let constant = self.push_constant(DocumentConstantValue::Bool { value: *value });
            return Ok(DocumentPattern::Constant { constant });
        }
        if let CheckedMatchPattern::Number { value } = pattern {
            let (coefficient, scale) = parse_decimal(value)?;
            let constant = self.push_constant(DocumentConstantValue::Number { coefficient, scale });
            return Ok(DocumentPattern::Constant { constant });
        }
        match pattern {
            CheckedMatchPattern::NaN => Ok(DocumentPattern::Tag {
                tag: self.intern_name("NaN"),
            }),
            CheckedMatchPattern::Tag { name } => Ok(DocumentPattern::Tag {
                tag: self.intern_name(name),
            }),
            CheckedMatchPattern::Unknown { tokens } => Err(PlanError::new(format!(
                "conditional arm has an unknown checked pattern `{}`",
                tokens.join(" ")
            ))),
            CheckedMatchPattern::Wildcard
            | CheckedMatchPattern::Binding { .. }
            | CheckedMatchPattern::Bool { .. }
            | CheckedMatchPattern::Number { .. }
            | CheckedMatchPattern::Text { .. } => unreachable!(),
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
        Ok(self.constant_expr(compiler_id, DocumentConstantValue::Enum { name }))
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
        if let GlobalValue::Inline(producer) = global {
            let expression = self.compile_expression_projection(
                producer,
                projection,
                context,
                None,
                final_class,
            )?;
            self.record_compiled_path(&joined_path(path, projection), expression);
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
        self.record_compiled_path(&joined_path(path, projection), expression);
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
                            let scope = scope.ok_or_else(|| {
                                PlanError::new(format!(
                                    "view binding {} materialization local has no exact row scope",
                                    binding.id.0
                                ))
                            })?;
                            let target = self.resolve_view_materialization_target(
                                binding.id.0,
                                *owner,
                                *local,
                                scope,
                                projection,
                            )?;
                            Some(target)
                        }
                        _ => None,
                    };
                    if let Some(direct) = direct {
                        direct
                    } else {
                        let expression = self.compile_expression(
                            read.expression,
                            &CompileContext::default(),
                            None,
                        )?;
                        let expression = self.project_fields(
                            binding.id.0,
                            expression,
                            additional_projection,
                            DocumentValueClass::DynamicScalar,
                        );
                        DocumentBindingTarget::Expression { expression }
                    }
                }
                ir::ViewBindingTarget::Source { source } => DocumentBindingTarget::Source {
                    source: SourceId(source.0),
                },
            };
            result.push(DocumentViewBinding {
                id: DocumentBindingId(binding.id.0),
                template: None,
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
        let target = member.target;
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
                ir::ErasedLocalMemberTarget::Source(source) => DocumentBindingTarget::Source {
                    source: SourceId(source.0),
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
                return Err(PlanError::new(format!(
                    "view binding {binding_id} materialization field {} projection `{}` resolves to {} exact child fields",
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

fn synthetic_scope_id(owner: ir::StaticOwnerId) -> Result<ScopeId, PlanError> {
    let namespace = 1usize << (usize::BITS - 1);
    if owner.0 >= namespace {
        return Err(PlanError::new(
            "static owner exceeds the synthetic document scope namespace",
        ));
    }
    Ok(ScopeId(namespace | owner.0))
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
        "Error/new" => DocumentBuiltin::ErrorNew,
        "Error/text" => DocumentBuiltin::ErrorText,
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
        "Text/is_empty" => DocumentBuiltin::TextIsEmpty,
        "Text/join" => DocumentBuiltin::TextJoin,
        "Text/join_lines" => DocumentBuiltin::TextJoinLines,
        "Text/length" => DocumentBuiltin::TextLength,
        "Text/space" => DocumentBuiltin::TextSpace,
        "Text/starts_with" => DocumentBuiltin::TextStartsWith,
        "Text/substring" => DocumentBuiltin::TextSubstring,
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
    let declarations = bindings
        .iter()
        .map(|binding| binding.declaration)
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
            if let ir::ExecutableExpressionKind::LocalRead { declaration, .. } = expression.kind
                && declarations.contains(&declaration)
            {
                local_dependencies.insert(declaration);
            }
            pending.extend(ir::executable_expression_children(&expression.kind));
        }
        dependencies.insert(binding.declaration, local_dependencies);
    }

    let mut emitted = BTreeSet::new();
    let mut ordered = Vec::with_capacity(bindings.len());
    while ordered.len() < bindings.len() {
        let Some(binding) = bindings.iter().find(|binding| {
            !emitted.contains(&binding.declaration)
                && dependencies[&binding.declaration]
                    .iter()
                    .all(|dependency| emitted.contains(dependency))
        }) else {
            let remaining = bindings
                .iter()
                .filter(|binding| !emitted.contains(&binding.declaration))
                .map(|binding| binding.declaration.0)
                .collect::<Vec<_>>();
            return Err(PlanError::new(format!(
                "erased BLOCK contains a lexical value cycle across declarations {remaining:?}"
            )));
        };
        emitted.insert(binding.declaration);
        ordered.push(binding);
    }
    Ok(ordered)
}

fn parse_decimal(value: &str) -> Result<(i64, u32), PlanError> {
    let value = value.replace('_', "");
    let (base, exponent) = value
        .split_once(['e', 'E'])
        .map(|(base, exponent)| {
            exponent
                .parse::<i32>()
                .map(|exponent| (base, exponent))
                .map_err(|_| PlanError::new(format!("invalid document number `{value}`")))
        })
        .transpose()?
        .unwrap_or((value.as_str(), 0));
    let negative = base.starts_with('-');
    let unsigned = base.trim_start_matches(['-', '+']);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if whole.is_empty() && fraction.is_empty() {
        return Err(PlanError::new(format!("invalid document number `{value}`")));
    }
    let digits = format!("{whole}{fraction}");
    let mut coefficient = digits
        .parse::<i64>()
        .map_err(|_| PlanError::new(format!("document number `{value}` exceeds i64")))?;
    if negative {
        coefficient = coefficient
            .checked_neg()
            .ok_or_else(|| PlanError::new(format!("document number `{value}` exceeds i64")))?;
    }
    let mut scale = fraction.len() as i32 - exponent;
    if scale < 0 {
        coefficient = coefficient
            .checked_mul(10_i64.pow((-scale) as u32))
            .ok_or_else(|| PlanError::new(format!("document number `{value}` exceeds i64")))?;
        scale = 0;
    }
    while scale > 0 && coefficient % 10 == 0 {
        coefficient /= 10;
        scale -= 1;
    }
    Ok((coefficient, scale as u32))
}

fn value_class_for_type(ty: &Type) -> DocumentValueClass {
    match ty {
        Type::RenderContract => DocumentValueClass::Render,
        Type::List(item) if matches!(item.as_ref(), Type::RenderContract) => {
            DocumentValueClass::ChildList
        }
        Type::List(_) | Type::Object(_) => DocumentValueClass::DynamicStructure,
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Skip
        | Type::VariantSet(_)
        | Type::Function { .. }
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => DocumentValueClass::DynamicScalar,
    }
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
        ir::ExecutableExpressionKind::Record(_) => "record".to_owned(),
        ir::ExecutableExpressionKind::Object(_) => "object".to_owned(),
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
}
