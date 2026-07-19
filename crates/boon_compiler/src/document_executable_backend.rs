use boon_ir::{self as ir, ErasedProgram};
use boon_plan::*;
use boon_typecheck::Type;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn compile_document_plan(
    program: &ErasedProgram,
    executable_fields: &BTreeSet<FieldId>,
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
        executable_fields,
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
    locals: BTreeMap<String, DocumentLocalId>,
    pattern_bindings: BTreeMap<String, usize>,
}

#[derive(Default)]
struct SourceGroupNode {
    source: Option<SourceId>,
    children: BTreeMap<String, SourceGroupNode>,
}

struct DocumentCompiler<'a> {
    program: &'a ErasedProgram,
    globals: BTreeMap<String, GlobalValue>,
    globals_by_declaration:
        BTreeMap<(boon_typecheck::DeclId, Option<ir::StaticOwnerId>), GlobalValue>,
    globals_by_storage: BTreeMap<ir::StorageBindingId, GlobalValue>,
    scoped_fields: BTreeMap<(ScopeId, String), Option<FieldId>>,
    distributed_by_expression: BTreeMap<ir::ExecutableExprId, ValueRef>,
    distributed_by_path: BTreeMap<String, ValueRef>,
    materializations_by_id: BTreeMap<usize, ContextualMaterializationInfo>,
    names: Vec<String>,
    name_ids: BTreeMap<String, DocumentNameId>,
    constants: Vec<DocumentConstant>,
    expressions: Vec<DocumentExpr>,
    expression_cache: BTreeMap<(usize, usize), DocumentExprId>,
    functions: Vec<DocumentFunction>,
    function_ids: BTreeSet<DocumentFunctionId>,
    templates: Vec<DocumentTemplate>,
    template_ids: BTreeSet<DocumentTemplateId>,
    materializations: Vec<DocumentMaterialization>,
    materialization_ids: BTreeSet<DocumentMaterializationId>,
    materializations_in_progress: BTreeSet<usize>,
    compiled_materializations: BTreeSet<usize>,
    compiled_paths: BTreeMap<(Option<ScopeId>, String), DocumentExprId>,
    next_cache_scope: usize,
}

impl<'a> DocumentCompiler<'a> {
    fn new(
        program: &'a ErasedProgram,
        executable_fields: &BTreeSet<FieldId>,
        distributed_expression_refs: &'a BTreeMap<ir::ExecutableExprId, ValueRef>,
        distributed_path_refs: &'a BTreeMap<String, ValueRef>,
    ) -> Result<Self, PlanError> {
        let mut globals = BTreeMap::new();
        for source in &program.sources {
            globals.insert(
                source.path.clone(),
                GlobalValue::Source(SourceId(source.id.0)),
            );
        }
        for state in &program.state_cells {
            if state.scope_id.is_none() {
                globals.insert(state.path.clone(), GlobalValue::State(StateId(state.id.0)));
            }
        }
        for list in &program.lists {
            globals.insert(list.name.clone(), GlobalValue::List(ListId(list.id.0)));
        }
        for field in &program.derived_values {
            if field.scope_id.is_some() {
                continue;
            }
            let field_id = program
                .semantic_index
                .fields
                .iter()
                .find(|semantic| semantic.path == field.path)
                .map(|semantic| semantic.id.0)
                .unwrap_or(field.id.0);
            let field_id = FieldId(field_id);
            let computed_list_view = field.kind == ir::DerivedValueKind::ListView
                && executable_fields.contains(&field_id);
            if list_for_semantic_path(program, &field.path).is_some() && !computed_list_view {
                continue;
            }
            globals.insert(field.path.clone(), GlobalValue::Field(field_id));
        }
        for field in &program.semantic_index.fields {
            if field.scope_id.is_some() {
                continue;
            }
            globals.entry(field.path.clone()).or_insert_with(|| {
                list_for_semantic_path(program, &field.path)
                    .map(|list| GlobalValue::List(ListId(list.id.0)))
                    .unwrap_or(GlobalValue::Field(FieldId(field.id.0)))
            });
        }
        let mut globals_by_declaration = BTreeMap::new();
        let mut globals_by_storage = BTreeMap::new();
        for binding in &program.storage.bindings {
            let value = match binding.kind {
                ir::StorageBindingKind::Value {
                    list: Some(list), ..
                } => Some(GlobalValue::List(ListId(list.0))),
                ir::StorageBindingKind::Value {
                    field: Some(field), ..
                } => Some(GlobalValue::Field(FieldId(field.0))),
                ir::StorageBindingKind::Value { .. } => Some(GlobalValue::Inline(binding.producer)),
                ir::StorageBindingKind::Source { runtime, .. } => {
                    Some(GlobalValue::Source(SourceId(runtime.0)))
                }
                ir::StorageBindingKind::State { runtime, .. } => {
                    Some(GlobalValue::State(StateId(runtime.0)))
                }
            };
            if let Some(value) = value {
                globals_by_declaration.insert((binding.declaration, binding.static_owner), value);
                globals_by_storage.insert(binding.id, value);
            }
        }

        let mut scoped_fields = BTreeMap::new();
        for field in &program.semantic_index.fields {
            let Some(scope) = field.scope_id else {
                continue;
            };
            let scope = ScopeId(scope.0);
            let field_id = FieldId(field.id.0);
            for name in [&field.local_name, &field.path] {
                insert_unique_scoped_field(&mut scoped_fields, scope, name, field_id);
            }
        }

        let mut materializations_by_id = BTreeMap::new();
        for materialization in &program.materializations {
            let info = ContextualMaterializationInfo {
                id: materialization.id,
                operation: materialization.operation,
                source: materialization.source,
                body: materialization.body,
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
            globals,
            globals_by_declaration,
            globals_by_storage,
            scoped_fields,
            distributed_by_expression: distributed_expression_refs.clone(),
            distributed_by_path: distributed_path_refs.clone(),
            materializations_by_id,
            names: Vec::new(),
            name_ids: BTreeMap::new(),
            constants: Vec::new(),
            expressions: Vec::new(),
            expression_cache: BTreeMap::new(),
            functions: Vec::new(),
            function_ids: BTreeSet::new(),
            templates: Vec::new(),
            template_ids: BTreeSet::new(),
            materializations: Vec::new(),
            materialization_ids: BTreeSet::new(),
            materializations_in_progress: BTreeSet::new(),
            compiled_materializations: BTreeSet::new(),
            compiled_paths: BTreeMap::new(),
            next_cache_scope: program.static_owners.len().saturating_add(1),
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
        let result = if input_override.is_none() {
            if let Some(value) = self.distributed_by_expression.get(&expression_id).cloned() {
                self.compile_distributed_value(
                    expression.id.0,
                    value,
                    value_class_for_type(&expression.flow_type.ty),
                )?
            } else {
                self.compile_expression_kind(&expression, context, input_override)?
            }
        } else {
            self.compile_expression_kind(&expression, context, input_override)?
        };
        if input_override.is_none() {
            self.expression_cache.insert(cache_key, result);
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
            ValueRef::SourcePayload { source_id, field } => {
                let field = match field {
                    SourcePayloadField::Address => "address".to_owned(),
                    SourcePayloadField::Bytes => "bytes".to_owned(),
                    SourcePayloadField::Key => "key".to_owned(),
                    SourcePayloadField::Named(field) => field,
                    SourcePayloadField::Text => "text".to_owned(),
                };
                let base = self.push_expr(
                    compiler_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::Source { source: source_id },
                    },
                );
                Ok(self.project_fields(compiler_id, base, &[field], class))
            }
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
            ir::ExecutableExpressionKind::CanonicalRead {
                target,
                storage_binding,
                path,
                projection,
            } => self.compile_canonical_read(
                compiler_id,
                Some(*target),
                *storage_binding,
                path,
                projection,
                context,
                value_class_for_type(&expression.flow_type.ty),
            ),
            ir::ExecutableExpressionKind::ExternalRead { canonical_path } => self
                .compile_external_read(
                    compiler_id,
                    canonical_path,
                    context,
                    value_class_for_type(&expression.flow_type.ty),
                ),
            ir::ExecutableExpressionKind::Drain { path, .. } => Err(PlanError::new(format!(
                "migration drain `{path}` at executable expression {compiler_id} cannot be lowered as a document value"
            ))),
            ir::ExecutableExpressionKind::Text(value) => {
                self.compile_text(compiler_id, value, context)
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
            } => self.compile_call(
                expression,
                *callable_kind,
                name,
                arguments,
                context,
                input_override,
            ),
            ir::ExecutableExpressionKind::Materialize { materialization } => {
                let materialization =
                    self.ensure_materialization(*materialization, compiler_id, context)?;
                Ok(self.push_expr(
                    compiler_id,
                    DocumentValueClass::ChildList,
                    DocumentExprOp::Materialize { materialization },
                ))
            }
            ir::ExecutableExpressionKind::Draining { input } => {
                self.compile_expression(*input, context, input_override)
            }
            ir::ExecutableExpressionKind::Hold { name, .. } => {
                let declaration = self
                    .program
                    .executable
                    .states
                    .iter()
                    .find(|state| state.expression == expression.id)
                    .map(|state| state.declaration)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "HOLD executable expression {compiler_id} has no storage declaration"
                        ))
                    })?;
                self.compile_canonical_read(
                    compiler_id,
                    Some(declaration),
                    self.storage_binding_for_state_expression(expression.id)?,
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
            ir::ExecutableExpressionKind::Project { input, fields } => {
                let input = self.compile_expression(*input, context, input_override)?;
                Ok(self.project_fields(
                    compiler_id,
                    input,
                    fields,
                    value_class_for_type(&expression.flow_type.ty),
                ))
            }
            ir::ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => {
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

    fn compile_call(
        &mut self,
        expression: &ir::ExecutableExpression,
        callable_kind: ir::ExecutableCallableKind,
        function: &str,
        arguments: &[ir::ExecutableCallArgument],
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
            return self.compile_constructor(expression, constructor, arguments, context);
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
                    name: Some(self.intern_name(&argument.name)),
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
        if body_owner != Some(info.owner) {
            return Err(PlanError::new(format!(
                "render materialization {} body root {} has owner {:?}, expected {}",
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
            arm_context.pattern_bindings.extend(
                pattern_binding_names(&arm.pattern)
                    .into_iter()
                    .map(|name| (name, expression.id.0)),
            );
            let output = self.compile_expression(arm.output, &arm_context, None)?;
            arms.push(DocumentSelectArm {
                pattern: self.compile_pattern(&arm.pattern)?,
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

    fn compile_pattern(&mut self, tokens: &[String]) -> Result<DocumentPattern, PlanError> {
        if tokens.iter().any(|token| token == "__") {
            return Ok(DocumentPattern::Wildcard);
        }
        if tokens.first().is_some_and(|token| token == "TEXT") {
            let value = tokens
                .iter()
                .skip_while(|token| token.as_str() != "{")
                .skip(1)
                .take_while(|token| token.as_str() != "}")
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            let constant = self.push_constant(DocumentConstantValue::Text { value });
            return Ok(DocumentPattern::Constant { constant });
        }
        let token = tokens
            .first()
            .ok_or_else(|| PlanError::new("conditional arm has an empty pattern"))?;
        if token == "True" || token == "False" {
            let constant = self.push_constant(DocumentConstantValue::Bool {
                value: token == "True",
            });
            return Ok(DocumentPattern::Constant { constant });
        }
        if token.parse::<i64>().is_ok() {
            let (coefficient, scale) = parse_decimal(token)?;
            let constant = self.push_constant(DocumentConstantValue::Number { coefficient, scale });
            return Ok(DocumentPattern::Constant { constant });
        }
        let tag = token.split('[').next().unwrap_or(token);
        Ok(DocumentPattern::Tag {
            tag: self.intern_name(tag),
        })
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

    fn compile_text(
        &mut self,
        compiler_id: usize,
        value: &str,
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let Some(first_open) = value.find('{') else {
            return Ok(self.constant_expr(
                compiler_id,
                DocumentConstantValue::Text {
                    value: value.to_owned(),
                },
            ));
        };
        let mut cursor = 0usize;
        let mut segments = Vec::new();
        let mut next_open = Some(first_open);
        while let Some(open) = next_open {
            if open > cursor {
                let constant = self.push_constant(DocumentConstantValue::Text {
                    value: value[cursor..open].to_owned(),
                });
                segments.push(DocumentTextSegment::Static { constant });
            }
            let close = value[open + 1..]
                .find('}')
                .map(|offset| open + 1 + offset)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "text executable expression {compiler_id} has an unterminated interpolation"
                    ))
                })?;
            let path = value[open + 1..close].trim();
            if path.is_empty() {
                return Err(PlanError::new(format!(
                    "text executable expression {compiler_id} has an empty interpolation"
                )));
            }
            let dynamic = self.compile_named_path(compiler_id, path, context)?;
            segments.push(DocumentTextSegment::Dynamic { value: dynamic });
            cursor = close + 1;
            next_open = value[cursor..].find('{').map(|offset| cursor + offset);
        }
        if cursor < value.len() {
            let constant = self.push_constant(DocumentConstantValue::Text {
                value: value[cursor..].to_owned(),
            });
            segments.push(DocumentTextSegment::Static { constant });
        }
        Ok(self.push_expr(
            compiler_id,
            DocumentValueClass::DynamicScalar,
            DocumentExprOp::TextTemplate { segments },
        ))
    }

    fn compile_named_path(
        &mut self,
        compiler_id: usize,
        path: &str,
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let parts = path
            .trim()
            .trim_start_matches('$')
            .split('.')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return Err(PlanError::new(format!(
                "text executable expression {compiler_id} has an empty path"
            )));
        }
        if let Some(selector) = context.pattern_bindings.get(parts[0]).copied() {
            let projection = parts.iter().map(|part| self.intern_name(part)).collect();
            return Ok(self.push_expr(
                compiler_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::Matched {
                        selector,
                        projection,
                    },
                },
            ));
        }
        if parts[0] == "element" {
            return self.compile_external_read(
                compiler_id,
                path,
                context,
                DocumentValueClass::DynamicScalar,
            );
        }
        let root = (1..=parts.len()).rev().find_map(|length| {
            let candidate = parts[..length].join(".");
            (context.locals.contains_key(&candidate) || self.canonical_root_exists(&candidate))
                .then_some((candidate, length))
        });
        if let Some((root, length)) = root {
            let projection = parts[length..]
                .iter()
                .map(|part| (*part).to_owned())
                .collect::<Vec<_>>();
            return self.compile_canonical_read(
                compiler_id,
                None,
                None,
                &root,
                &projection,
                context,
                DocumentValueClass::DynamicScalar,
            );
        }
        if self.distributed_by_path.contains_key(path) {
            return self.compile_external_read(
                compiler_id,
                path,
                context,
                DocumentValueClass::DynamicScalar,
            );
        }
        Err(PlanError::new(format!(
            "unresolved executable document interpolation `{path}` at expression {compiler_id}"
        )))
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

    fn compile_canonical_read(
        &mut self,
        compiler_id: usize,
        declaration: Option<boon_typecheck::DeclId>,
        storage_binding: Option<ir::StorageBindingId>,
        path: &str,
        projection: &[String],
        context: &CompileContext,
        final_class: DocumentValueClass,
    ) -> Result<DocumentExprId, PlanError> {
        if let Some(local) = context.locals.get(path).copied() {
            let projection = projection
                .iter()
                .map(|field| self.intern_name(field))
                .collect::<Vec<_>>();
            let expression = self.push_expr(
                compiler_id,
                final_class,
                DocumentExprOp::Read {
                    read: DocumentRead::Local { local, projection },
                },
            );
            self.record_compiled_path(path, expression);
            return Ok(expression);
        }
        if let Some(storage_binding) = storage_binding {
            let global = self
                .globals_by_storage
                .get(&storage_binding)
                .copied()
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "storage binding {storage_binding} (`{path}`) has no document value"
                    ))
                })?;
            return self.compile_global_projection(
                compiler_id,
                global,
                path,
                projection,
                context,
                final_class,
            );
        }
        if let Some(declaration) = declaration {
            let global = self
                .globals_by_declaration
                .get(&(declaration, context.stable_owner))
                .or_else(|| self.globals_by_declaration.get(&(declaration, None)))
                .copied()
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "checked declaration {} owner {:?} (`{path}`) has no document storage binding",
                        declaration.0, context.stable_owner
                    ))
                })?;
            return self.compile_global_projection(
                compiler_id,
                global,
                path,
                projection,
                context,
                final_class,
            );
        }
        if let Some(ValueRef::DistributedImport(import)) = self.distributed_by_path.get(path) {
            let import = *import;
            let base = self.push_expr(
                compiler_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::DistributedImport { import },
                },
            );
            let expression = self.project_fields(compiler_id, base, projection, final_class);
            self.record_compiled_path(&joined_path(path, projection), expression);
            return Ok(expression);
        }
        if self.source_group_exists(path)
            && !matches!(self.globals.get(path), Some(GlobalValue::Source(_)))
            && let Some(base) = self.compile_source_group(compiler_id, path)
        {
            let expression = self.project_fields(compiler_id, base, projection, final_class);
            self.record_compiled_path(&joined_path(path, projection), expression);
            return Ok(expression);
        }
        if let Some(global) = self.globals.get(path).copied() {
            let (read, class) = match global {
                GlobalValue::State(state) => (
                    DocumentRead::State { state },
                    DocumentValueClass::DynamicScalar,
                ),
                GlobalValue::Field(field) => (
                    DocumentRead::Field { field },
                    DocumentValueClass::DynamicScalar,
                ),
                GlobalValue::List(list) => (
                    DocumentRead::List { list },
                    DocumentValueClass::DynamicStructure,
                ),
                GlobalValue::Source(source) => (
                    DocumentRead::Source { source },
                    DocumentValueClass::DynamicScalar,
                ),
                GlobalValue::Inline(_) => {
                    unreachable!("path-indexed globals never contain inline declarations")
                }
            };
            let base = self.push_expr(compiler_id, class, DocumentExprOp::Read { read });
            let expression = self.project_fields(compiler_id, base, projection, final_class);
            self.record_compiled_path(&joined_path(path, projection), expression);
            return Ok(expression);
        }
        if let Some(base) = self.compile_global_record(compiler_id, path)? {
            let expression = self.project_fields(compiler_id, base, projection, final_class);
            self.record_compiled_path(&joined_path(path, projection), expression);
            return Ok(expression);
        }
        Err(PlanError::new(format!(
            "unresolved canonical executable document path `{path}` at expression {compiler_id}"
        )))
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
        let base = match global {
            GlobalValue::Inline(producer) => self.compile_expression(producer, context, None)?,
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
        if let Some(selector) = parts
            .first()
            .and_then(|name| context.pattern_bindings.get(*name))
            .copied()
        {
            let projection = parts.iter().map(|part| self.intern_name(part)).collect();
            return Ok(self.push_expr(
                compiler_id,
                class,
                DocumentExprOp::Read {
                    read: DocumentRead::Matched {
                        selector,
                        projection,
                    },
                },
            ));
        }
        if parts.first() == Some(&"element") {
            let projection = parts
                .iter()
                .skip(1)
                .map(|part| self.intern_name(part))
                .collect();
            return Ok(self.push_expr(
                compiler_id,
                class,
                DocumentExprOp::Read {
                    read: DocumentRead::ElementState { projection },
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

    fn compile_global_record(
        &mut self,
        compiler_id: usize,
        path: &str,
    ) -> Result<Option<DocumentExprId>, PlanError> {
        let prefix = format!("{path}.");
        let child_names = self
            .globals
            .keys()
            .filter_map(|candidate| {
                candidate
                    .strip_prefix(&prefix)
                    .and_then(|rest| rest.split('.').next())
                    .filter(|child| !child.is_empty())
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>();
        if child_names.is_empty() {
            return Ok(None);
        }
        let mut fields = Vec::with_capacity(child_names.len());
        for child in child_names {
            let child_path = format!("{path}.{child}");
            let value = if let Some(global) = self.globals.get(&child_path).copied() {
                let (read, class) = match global {
                    GlobalValue::State(state) => (
                        DocumentRead::State { state },
                        DocumentValueClass::DynamicScalar,
                    ),
                    GlobalValue::Field(field) => (
                        DocumentRead::Field { field },
                        DocumentValueClass::DynamicScalar,
                    ),
                    GlobalValue::List(list) => (
                        DocumentRead::List { list },
                        DocumentValueClass::DynamicStructure,
                    ),
                    GlobalValue::Source(source) => (
                        DocumentRead::Source { source },
                        DocumentValueClass::DynamicScalar,
                    ),
                    GlobalValue::Inline(_) => {
                        unreachable!("path-indexed globals never contain inline declarations")
                    }
                };
                self.push_expr(compiler_id, class, DocumentExprOp::Read { read })
            } else {
                self.compile_global_record(compiler_id, &child_path)?
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "canonical document record `{child_path}` has no typed children"
                        ))
                    })?
            };
            fields.push(DocumentRecordField {
                name: Some(self.intern_name(&child)),
                value,
                spread: false,
            });
        }
        Ok(Some(self.push_expr(
            compiler_id,
            DocumentValueClass::DynamicStructure,
            DocumentExprOp::Record { fields },
        )))
    }

    fn compile_source_group(&mut self, compiler_id: usize, path: &str) -> Option<DocumentExprId> {
        let prefix = format!("{path}.");
        let routes = self
            .program
            .sources
            .iter()
            .filter_map(|source| {
                source
                    .path
                    .strip_prefix(&prefix)
                    .map(|rest| (rest.to_owned(), SourceId(source.id.0)))
            })
            .collect::<Vec<_>>();
        if routes.is_empty() {
            return None;
        }
        let mut root = SourceGroupNode::default();
        for (route, source) in routes {
            let mut node = &mut root;
            for part in route.split('.').filter(|part| !part.is_empty()) {
                node = node.children.entry(part.to_owned()).or_default();
            }
            node.source = Some(source);
        }
        Some(self.push_source_group_node(compiler_id, root))
    }

    fn push_source_group_node(
        &mut self,
        compiler_id: usize,
        node: SourceGroupNode,
    ) -> DocumentExprId {
        if node.children.is_empty()
            && let Some(source) = node.source
        {
            return self.push_expr(
                compiler_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::Source { source },
                },
            );
        }
        let mut fields =
            Vec::with_capacity(node.children.len() + usize::from(node.source.is_some()));
        if let Some(source) = node.source {
            let value = self.push_expr(
                compiler_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::Source { source },
                },
            );
            fields.push(DocumentRecordField {
                name: Some(self.intern_name("__source")),
                value,
                spread: false,
            });
        }
        for (name, child) in node.children {
            let value = self.push_source_group_node(compiler_id, child);
            fields.push(DocumentRecordField {
                name: Some(self.intern_name(&name)),
                value,
                spread: false,
            });
        }
        let class = record_value_class(&fields, &self.expressions);
        self.push_expr(compiler_id, class, DocumentExprOp::Record { fields })
    }

    fn compile_view_bindings(&mut self) -> Result<Vec<DocumentViewBinding>, PlanError> {
        let mut result = Vec::new();
        for binding in self.program.view_bindings.clone() {
            let scope = binding.scope_id.map(|scope| ScopeId(scope.0));
            let target = match &binding.target {
                ir::ViewBindingTarget::Storage {
                    binding: storage_binding,
                    projection,
                } => {
                    let global = self
                        .globals_by_storage
                        .get(storage_binding)
                        .copied()
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "view binding {} references storage binding {} without a document value",
                                binding.id.0, storage_binding
                            ))
                        })?;
                    if projection.is_empty() {
                        match global {
                            GlobalValue::State(state) => DocumentBindingTarget::State { state },
                            GlobalValue::Field(field) => DocumentBindingTarget::Field { field },
                            GlobalValue::List(list) => DocumentBindingTarget::List { list },
                            GlobalValue::Source(source) => DocumentBindingTarget::Source { source },
                            GlobalValue::Inline(_) => {
                                let expression = self.compile_global_projection(
                                    binding.id.0,
                                    global,
                                    &binding.path,
                                    projection,
                                    &CompileContext::default(),
                                    DocumentValueClass::DynamicScalar,
                                )?;
                                DocumentBindingTarget::Expression { expression }
                            }
                        }
                    } else {
                        let expression = self.compile_global_projection(
                            binding.id.0,
                            global,
                            &binding.path,
                            projection,
                            &CompileContext::default(),
                            DocumentValueClass::DynamicScalar,
                        )?;
                        DocumentBindingTarget::Expression { expression }
                    }
                }
                ir::ViewBindingTarget::MaterializationLocal { projection, .. } => {
                    let Some(scope) = scope else {
                        return Err(PlanError::new(format!(
                            "view binding {} materialization local has no exact row scope",
                            binding.id.0
                        )));
                    };
                    let field_path = projection.join(".");
                    let field = self.resolve_scoped_field(scope, &field_path).ok_or_else(|| {
                        let row_scope = self
                            .program
                            .row_scopes
                            .iter()
                            .find(|candidate| candidate.id.0 == scope.0)
                            .map(|candidate| {
                                format!("{} for list `{}`", candidate.row_scope, candidate.list)
                            })
                            .unwrap_or_else(|| "missing row scope".to_owned());
                        let available = self
                            .program
                            .semantic_index
                            .fields
                            .iter()
                            .filter(|field| field.scope_id.is_some_and(|id| id.0 == scope.0))
                            .map(|field| field.local_name.as_str())
                            .collect::<Vec<_>>();
                        PlanError::new(format!(
                            "view binding {} materialization local `{field_path}` has no typed field in scope {} ({row_scope}); available fields {available:?}",
                            binding.id.0, scope.0,
                        ))
                    })?;
                    DocumentBindingTarget::ScopedField { scope, field }
                }
                ir::ViewBindingTarget::ExternalExpression { expression } => {
                    let expression =
                        self.compile_expression(*expression, &CompileContext::default(), None)?;
                    DocumentBindingTarget::Expression { expression }
                }
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
    ) -> Result<Option<ir::StorageBindingId>, PlanError> {
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
            .storage
            .bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.kind,
                    ir::StorageBindingKind::State { executable, .. } if executable == state.id
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
        Ok(Some(binding.id))
    }

    fn expression_kind(
        &self,
        id: ir::ExecutableExprId,
    ) -> Result<&ir::ExecutableExpressionKind, PlanError> {
        self.expression(id).map(|expression| &expression.kind)
    }

    fn source_group_exists(&self, path: &str) -> bool {
        let prefix = format!("{path}.");
        self.program
            .sources
            .iter()
            .any(|source| source.path.starts_with(&prefix))
    }

    fn canonical_root_exists(&self, path: &str) -> bool {
        if self.globals.contains_key(path)
            || self.distributed_by_path.contains_key(path)
            || self.source_group_exists(path)
        {
            return true;
        }
        let prefix = format!("{path}.");
        self.globals.keys().any(|value| value.starts_with(&prefix))
    }

    fn resolve_scoped_field(&self, scope: ScopeId, path: &str) -> Option<FieldId> {
        self.scoped_fields
            .get(&(scope, path.to_owned()))
            .copied()
            .flatten()
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

fn insert_unique_scoped_field(
    fields: &mut BTreeMap<(ScopeId, String), Option<FieldId>>,
    scope: ScopeId,
    name: &str,
    field: FieldId,
) {
    fields
        .entry((scope, name.to_owned()))
        .and_modify(|existing| {
            if *existing != Some(field) {
                *existing = None;
            }
        })
        .or_insert(Some(field));
}

fn list_for_semantic_path<'a>(
    program: &'a ErasedProgram,
    path: &str,
) -> Option<&'a ir::ListMemory> {
    program.lists.iter().find(|list| list.name == path)
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

fn pattern_binding_names(tokens: &[String]) -> Vec<String> {
    let Some(open) = tokens.iter().position(|token| token == "[") else {
        return Vec::new();
    };
    tokens
        .iter()
        .skip(open + 1)
        .take_while(|token| token.as_str() != "]")
        .filter(|token| {
            token
                .chars()
                .next()
                .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
                && token
                    .chars()
                    .all(|character| character == '_' || character.is_ascii_alphanumeric())
        })
        .cloned()
        .collect()
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
        "List/any" => DocumentBuiltin::ListAny,
        "List/append" => DocumentBuiltin::ListAppend,
        "List/chunk" => DocumentBuiltin::ListChunk,
        "List/count" => DocumentBuiltin::ListCount,
        "List/find" => DocumentBuiltin::ListFind,
        "List/get" => DocumentBuiltin::ListGet,
        "List/is_not_empty" => DocumentBuiltin::ListIsNotEmpty,
        "List/latest" => DocumentBuiltin::ListLatest,
        "List/length" => DocumentBuiltin::ListLength,
        "List/map" => DocumentBuiltin::ListMap,
        "List/range" => DocumentBuiltin::ListRange,
        "List/remove" => DocumentBuiltin::ListRemove,
        "List/retain" => DocumentBuiltin::ListRetain,
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
