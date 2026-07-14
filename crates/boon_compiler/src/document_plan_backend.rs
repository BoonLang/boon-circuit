use boon_ir::{self as ir, TypedProgram};
use boon_parser::{AstCallArg, AstExprKind, AstRecordField, AstStatement, AstStatementKind};
use boon_plan::*;
use boon_typecheck::{ListMapBinding, ListMapResultKind, Type};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn compile_document_plan(
    program: &TypedProgram,
    executable_fields: &BTreeSet<FieldId>,
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
    DocumentCompiler::new(program, executable_fields)?
        .compile(output)
        .map(Some)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GlobalValue {
    State(StateId),
    Field(FieldId),
    List(ListId),
    Source(SourceId),
}

#[derive(Clone)]
struct FunctionInfo {
    id: DocumentFunctionId,
    name: String,
    args: Vec<String>,
    parameters: Vec<DocumentParameterId>,
    statement: AstStatement,
    row_scope: Option<ScopeId>,
    row_alias: Option<String>,
}

#[derive(Clone, Default)]
struct CompileContext {
    owner_function: Option<DocumentFunctionId>,
    owner_name: Option<String>,
    parameters: BTreeMap<String, DocumentParameterId>,
    locals: BTreeMap<String, DocumentLocalId>,
    pattern_bindings: BTreeMap<String, usize>,
    row_aliases: BTreeMap<String, ScopeId>,
}

#[derive(Default)]
struct SourceGroupNode {
    source: Option<SourceId>,
    children: BTreeMap<String, SourceGroupNode>,
}

struct DocumentCompiler<'a> {
    program: &'a TypedProgram,
    globals: BTreeMap<String, GlobalValue>,
    global_aliases: BTreeMap<String, Option<GlobalValue>>,
    scoped_fields: BTreeMap<(ScopeId, String), FieldId>,
    functions: BTreeMap<DocumentFunctionId, FunctionInfo>,
    functions_by_name: BTreeMap<String, DocumentFunctionId>,
    function_aliases: BTreeMap<String, Option<DocumentFunctionId>>,
    list_maps: BTreeMap<usize, ListMapBinding>,
    list_by_scope: BTreeMap<ScopeId, ListId>,
    names: Vec<String>,
    name_ids: BTreeMap<String, DocumentNameId>,
    constants: Vec<DocumentConstant>,
    expressions: Vec<DocumentExpr>,
    compiled_functions: Vec<DocumentFunction>,
    referenced_functions: BTreeSet<DocumentFunctionId>,
    finished_functions: BTreeSet<DocumentFunctionId>,
    templates: Vec<DocumentTemplate>,
    template_ids: BTreeSet<DocumentTemplateId>,
    materializations: Vec<DocumentMaterialization>,
    materialization_ids: BTreeSet<DocumentMaterializationId>,
    compiled_paths: BTreeMap<(Option<ScopeId>, String), DocumentExprId>,
}

impl<'a> DocumentCompiler<'a> {
    fn new(
        program: &'a TypedProgram,
        executable_fields: &BTreeSet<FieldId>,
    ) -> Result<Self, PlanError> {
        let mut globals = BTreeMap::new();
        for source in &program.sources {
            globals.insert(
                source.path.clone(),
                GlobalValue::Source(SourceId(source.id.0)),
            );
        }
        for state in &program.state_cells {
            if state.scope_id.is_some() {
                continue;
            }
            globals.insert(state.path.clone(), GlobalValue::State(StateId(state.id.0)));
        }
        for list in &program.lists {
            globals.insert(list.name.clone(), GlobalValue::List(ListId(list.id.0)));
        }
        for field in &program.derived_values {
            if field.scope_id.is_some() || statement_is_source_group(program, &field.statement) {
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
            let source_group = program.derived_values.iter().any(|derived| {
                derived.path == field.path && statement_is_source_group(program, &derived.statement)
            });
            if field.scope_id.is_none() && !source_group {
                let list = list_for_semantic_path(program, &field.path);
                globals.entry(field.path.clone()).or_insert_with(|| {
                    list.map(|list| GlobalValue::List(ListId(list.id.0)))
                        .unwrap_or(GlobalValue::Field(FieldId(field.id.0)))
                });
            }
        }

        let mut global_aliases = BTreeMap::new();
        for (path, value) in &globals {
            for alias in path_aliases(path) {
                insert_unique_alias(&mut global_aliases, alias, *value);
            }
        }

        let mut scoped_fields = BTreeMap::new();
        for field in &program.semantic_index.fields {
            let Some(scope) = field.scope_id else {
                continue;
            };
            let scope = ScopeId(scope.0);
            let id = FieldId(field.id.0);
            for name in [field.local_name.as_str(), field.path.as_str()] {
                scoped_fields.entry((scope, name.to_owned())).or_insert(id);
            }
            if let Some(local) = field.path.rsplit('.').next() {
                scoped_fields.entry((scope, local.to_owned())).or_insert(id);
            }
        }

        let mut functions = BTreeMap::new();
        let mut functions_by_name = BTreeMap::new();
        let mut function_aliases = BTreeMap::new();
        for function in &program.functions {
            let semantic = program
                .semantic_index
                .functions
                .iter()
                .find(|candidate| candidate.statement_id == function.statement.id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "function statement {} has no typed FunctionId",
                        function.statement.id
                    ))
                })?;
            let id = DocumentFunctionId(semantic.id.0);
            let parameters = function
                .args
                .iter()
                .enumerate()
                .map(|(index, _)| parameter_id(id, index))
                .collect::<Result<Vec<_>, _>>()?;
            let row_scope = program
                .row_scopes
                .iter()
                .find(|scope| {
                    scope.function == function.name
                        || function.name.ends_with(&format!("/{}", scope.function))
                })
                .map(|scope| ScopeId(scope.id.0));
            let row_alias = program
                .row_scopes
                .iter()
                .find(|scope| Some(ScopeId(scope.id.0)) == row_scope)
                .map(|scope| scope.row_scope.clone());
            let info = FunctionInfo {
                id,
                name: function.name.clone(),
                args: function.args.clone(),
                parameters,
                statement: function.statement.clone(),
                row_scope,
                row_alias,
            };
            functions_by_name.insert(function.name.clone(), id);
            for alias in function_aliases_for_name(&function.name) {
                insert_unique_alias(&mut function_aliases, alias, id);
            }
            functions.insert(id, info);
        }

        let list_by_scope = program
            .lists
            .iter()
            .filter_map(|list| {
                list.row_scope_id
                    .map(|scope| (ScopeId(scope.0), ListId(list.id.0)))
            })
            .collect();
        let list_maps = program
            .typecheck_report
            .list_map_bindings
            .iter()
            .cloned()
            .map(|binding| (binding.map_expr_id, binding))
            .collect();

        Ok(Self {
            program,
            globals,
            global_aliases,
            scoped_fields,
            functions,
            functions_by_name,
            function_aliases,
            list_maps,
            list_by_scope,
            names: Vec::new(),
            name_ids: BTreeMap::new(),
            constants: Vec::new(),
            expressions: Vec::new(),
            compiled_functions: Vec::new(),
            referenced_functions: BTreeSet::new(),
            finished_functions: BTreeSet::new(),
            templates: Vec::new(),
            template_ids: BTreeSet::new(),
            materializations: Vec::new(),
            materialization_ids: BTreeSet::new(),
            compiled_paths: BTreeMap::new(),
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
            self.compile_statement_value(&output.statement, &CompileContext::default(), None)?;

        while let Some(function_id) = self
            .referenced_functions
            .iter()
            .copied()
            .find(|id| !self.finished_functions.contains(id))
        {
            self.compile_function(function_id)?;
        }
        self.compiled_functions.sort_by_key(|function| function.id);
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
            functions: self.compiled_functions,
            templates: self.templates,
            materializations: self.materializations,
            view_bindings,
            unresolved_op_count: 0,
        })
    }

    fn compile_function(&mut self, id: DocumentFunctionId) -> Result<(), PlanError> {
        if !self.finished_functions.insert(id) {
            return Ok(());
        }
        let info =
            self.functions.get(&id).cloned().ok_or_else(|| {
                PlanError::new(format!("missing typed document function {}", id.0))
            })?;
        let mut context = CompileContext {
            owner_function: Some(info.id),
            owner_name: Some(info.name.clone()),
            ..CompileContext::default()
        };
        for (name, parameter) in info.args.iter().zip(&info.parameters) {
            context.parameters.insert(name.clone(), *parameter);
        }
        if let (Some(alias), Some(scope)) = (info.row_alias.clone(), info.row_scope) {
            context.row_aliases.insert(alias, scope);
        }
        let body = self.compile_block(&info.statement.children, &context, info.statement.id)?;
        self.compiled_functions.push(DocumentFunction {
            id,
            parameters: info.parameters,
            body,
        });
        Ok(())
    }

    fn compile_block(
        &mut self,
        statements: &[AstStatement],
        context: &CompileContext,
        compiler_id: usize,
    ) -> Result<DocumentExprId, PlanError> {
        let statements = statements
            .iter()
            .filter(|statement| !statement_is_empty_delimiter(statement, self.program))
            .collect::<Vec<_>>();
        if statements.is_empty() {
            return Err(PlanError::new(format!(
                "document block {compiler_id} has no result"
            )));
        }
        let has_expression_result = statements.iter().any(|statement| {
            matches!(
                statement.kind,
                AstStatementKind::Expression | AstStatementKind::Source { .. }
            )
        });
        if statements.len() > 1
            && statements
                .iter()
                .all(|statement| statement_is_record_entry(statement))
            && statements
                .iter()
                .any(|statement| matches!(statement.kind, AstStatementKind::Spread))
        {
            return self.compile_record_children_refs(&statements, context, compiler_id);
        }
        if !has_expression_result
            && statements
                .iter()
                .all(|statement| statement_has_named_field(statement))
            && statements.len() > 1
        {
            return self.compile_record_children_refs(&statements, context, compiler_id);
        }

        let mut scoped = context.clone();
        let mut bindings = Vec::new();
        let mut result = None;
        for (index, statement) in statements.iter().enumerate() {
            let is_last = index + 1 == statements.len();
            if !is_last && statement_has_named_field(statement) {
                let name = statement_field_name(statement).expect("checked named field");
                let value = self.compile_statement_value(statement, &scoped, None)?;
                let local = DocumentLocalId(statement.id);
                scoped.locals.insert(name, local);
                bindings.push(DocumentLocalBinding { local, value });
            } else {
                result = Some(self.compile_statement_value(statement, &scoped, None)?);
            }
        }
        let result = result.ok_or_else(|| {
            PlanError::new(format!(
                "document block {compiler_id} has no result expression"
            ))
        })?;
        if bindings.is_empty() {
            return Ok(result);
        }
        Ok(self.push_expr(
            compiler_id,
            DocumentValueClass::DynamicStructure,
            DocumentExprOp::LocalBlock { bindings, result },
        ))
    }

    fn compile_statement_value(
        &mut self,
        statement: &AstStatement,
        context: &CompileContext,
        input_override: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        match &statement.kind {
            AstStatementKind::Block => {
                return self.compile_block(&statement.children, context, statement.id);
            }
            AstStatementKind::List { .. } => {
                if let Some(expr_id) = statement.expr {
                    let compile_from_expression = match self.expr_kind(expr_id)? {
                        AstExprKind::MatchArm { .. } => true,
                        AstExprKind::ListLiteral { items, .. } => !items.is_empty(),
                        _ => false,
                    };
                    if compile_from_expression {
                        return self.compile_expr_with_children(
                            expr_id,
                            &statement.children,
                            context,
                            input_override,
                        );
                    }
                }
                return self.compile_list_children(&statement.children, context, statement.id);
            }
            AstStatementKind::Field { name } if statement.expr.is_none() => {
                if statement.children.is_empty() {
                    return Err(PlanError::new(format!(
                        "document field `{name}` at statement {} has no value",
                        statement.id
                    )));
                }
                if is_child_list_field(name) {
                    return self.compile_child_sequence(&statement.children, context, statement.id);
                }
                return self.compile_record_children(&statement.children, context, statement.id);
            }
            AstStatementKind::Source { field: Some(_), .. }
                if matches!(
                    statement.expr.and_then(|id| self.program.expressions.get(id)),
                    Some(expr) if matches!(expr.kind, AstExprKind::Source)
                ) =>
            {
                return Ok(self.push_expr(
                    statement.expr.unwrap_or(statement.id),
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::SourceContext,
                ));
            }
            AstStatementKind::Function { .. } => {
                return self.compile_block(&statement.children, context, statement.id);
            }
            _ => {}
        }

        let Some(expr_id) = statement.expr else {
            if statement.children.len() == 1 {
                return self.compile_statement_value(
                    &statement.children[0],
                    context,
                    input_override,
                );
            }
            return self.compile_block(&statement.children, context, statement.id);
        };
        let mut value =
            self.compile_expr_with_children(expr_id, &statement.children, context, input_override)?;
        if !statement.children.is_empty()
            && !self.expr_consumes_children(expr_id)?
            && statement
                .children
                .iter()
                .all(|child| child_is_pipeline_continuation(child, self.program))
        {
            for child in &statement.children {
                value = self.compile_statement_value(child, context, Some(value))?;
            }
        }
        Ok(value)
    }

    fn compile_expr_with_children(
        &mut self,
        expr_id: usize,
        children: &[AstStatement],
        context: &CompileContext,
        input_override: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        let kind = self.expr_kind(expr_id)?.clone();
        match kind {
            AstExprKind::Identifier(value) => self.compile_identifier(expr_id, &value, context),
            AstExprKind::Path(parts) => self.compile_path(expr_id, &parts.join("."), context),
            AstExprKind::StringLiteral(value) => {
                if let Some(path) = value.strip_prefix('$') {
                    self.compile_path(expr_id, path, context)
                } else {
                    Ok(self.constant_expr(expr_id, DocumentConstantValue::Text { value }))
                }
            }
            AstExprKind::TextLiteral(value) => self.compile_text_literal(expr_id, &value, context),
            AstExprKind::Number(value) => {
                let (coefficient, scale) = parse_decimal(&value)?;
                Ok(self.constant_expr(
                    expr_id,
                    DocumentConstantValue::Number { coefficient, scale },
                ))
            }
            AstExprKind::ByteLiteral { value, .. } => {
                Ok(self.constant_expr(expr_id, DocumentConstantValue::Byte { value }))
            }
            AstExprKind::Bool(value) => {
                Ok(self.constant_expr(expr_id, DocumentConstantValue::Bool { value }))
            }
            AstExprKind::Enum(value) | AstExprKind::Tag(value) => self.compile_tag(expr_id, &value),
            AstExprKind::TaggedObject { tag, fields } => {
                self.compile_record_expr(expr_id, Some(tag), &fields, children, context)
            }
            AstExprKind::Source => Ok(self.push_expr(
                expr_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::SourceContext,
            )),
            AstExprKind::Call { function, args } => {
                self.compile_call(expr_id, &function, &args, children, context, None)
            }
            AstExprKind::Pipe { input, op, args } => {
                if matches!(op.as_str(), "WHEN" | "WHILE") {
                    let input = if self.expr_is_delimiter(input) {
                        input_override.ok_or_else(|| {
                            PlanError::new(format!(
                                "conditional expression {expr_id} has no pipeline input"
                            ))
                        })?
                    } else {
                        self.compile_expr_with_children(input, &[], context, input_override)?
                    };
                    return self.compile_select(expr_id, input, children, context);
                }
                let input = if self.expr_is_delimiter(input) {
                    input_override.ok_or_else(|| {
                        PlanError::new(format!("pipeline expression {expr_id} has no typed input"))
                    })?
                } else {
                    self.compile_expr_with_children(input, &[], context, input_override)?
                };
                self.compile_call(expr_id, &op, &args, children, context, Some(input))
            }
            AstExprKind::Draining { input } => {
                self.compile_expr_with_children(input, children, context, input_override)
            }
            AstExprKind::Drain { .. } => Err(PlanError::new(format!(
                "migration drain expression {expr_id} cannot be lowered as a document value"
            ))),
            AstExprKind::Hold { name, .. } => self.compile_path(expr_id, &name, context),
            AstExprKind::Latest => {
                let branches = children
                    .iter()
                    .map(|child| self.compile_statement_value(child, context, None))
                    .collect::<Result<Vec<_>, _>>()?;
                if branches.is_empty() {
                    return Err(PlanError::new(format!(
                        "LATEST expression {expr_id} has no branch"
                    )));
                }
                Ok(self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Latest { branches },
                ))
            }
            AstExprKind::When { input } => {
                let input = if self.expr_is_delimiter(input) {
                    input_override.ok_or_else(|| {
                        PlanError::new(format!("WHEN expression {expr_id} has no typed input"))
                    })?
                } else {
                    self.compile_expr_with_children(input, &[], context, input_override)?
                };
                self.compile_select(expr_id, input, children, context)
            }
            AstExprKind::Then { input, output } => {
                let input = self.compile_expr_with_children(input, &[], context, input_override)?;
                let output = output
                    .map(|output| self.compile_expr_with_children(output, children, context, None))
                    .transpose()?;
                Ok(self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Then { input, output },
                ))
            }
            AstExprKind::Infix { left, op, right } => {
                let left = self.compile_expr_with_children(left, &[], context, None)?;
                let right = self.compile_expr_with_children(right, &[], context, None)?;
                let operation = scalar_operation(&op)?;
                Ok(self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Scalar {
                        operation,
                        left,
                        right: Some(right),
                    },
                ))
            }
            AstExprKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    if self.expr_is_block_keyword(output) && !children.is_empty() {
                        self.compile_block(children, context, expr_id)
                    } else if self.expr_is_empty_list(output) && !children.is_empty() {
                        self.compile_list_children(children, context, expr_id)
                    } else {
                        self.compile_expr_with_children(output, children, context, None)
                    }
                } else if children.is_empty() {
                    Err(PlanError::new(format!(
                        "match arm expression {expr_id} has no output"
                    )))
                } else if children.len() == 1 {
                    self.compile_statement_value(&children[0], context, None)
                } else {
                    self.compile_list_children(children, context, expr_id)
                }
            }
            AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
                if fields.is_empty() && !children.is_empty() {
                    self.compile_record_children(children, context, expr_id)
                } else {
                    self.compile_record_expr(expr_id, None, &fields, children, context)
                }
            }
            AstExprKind::ListLiteral { items, .. } => {
                if items.is_empty() && !children.is_empty() {
                    self.compile_list_children(children, context, expr_id)
                } else {
                    let items = items
                        .iter()
                        .map(|item| {
                            self.compile_expr_with_children(*item, &[], context, None)
                                .map(|value| DocumentListItem {
                                    value,
                                    spread: false,
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let class = list_value_class(&items, &self.expressions);
                    Ok(self.push_expr(expr_id, class, DocumentExprOp::List { items }))
                }
            }
            AstExprKind::BytesLiteral { items, .. } => {
                let bytes = items
                    .iter()
                    .map(|item| match self.expr_kind(*item)? {
                        AstExprKind::ByteLiteral { value, .. } => Ok(*value),
                        other => Err(PlanError::new(format!(
                            "dynamic byte expression {item} ({other:?}) is not a document constant"
                        ))),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.constant_expr(expr_id, DocumentConstantValue::Bytes { value: bytes }))
            }
            AstExprKind::Delimiter => {
                let class = if children.is_empty() {
                    DocumentValueClass::Static
                } else {
                    DocumentValueClass::DynamicStructure
                };
                if children.is_empty() {
                    Ok(self.push_expr(
                        expr_id,
                        class,
                        DocumentExprOp::Record { fields: Vec::new() },
                    ))
                } else {
                    self.compile_record_children(children, context, expr_id)
                }
            }
            AstExprKind::Unknown(tokens) => Err(PlanError::new(format!(
                "unknown executable document expression {expr_id}: {}",
                tokens.join(" ")
            ))),
        }
    }

    fn compile_call(
        &mut self,
        expr_id: usize,
        function: &str,
        args: &[AstCallArg],
        children: &[AstStatement],
        context: &CompileContext,
        input: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        if function == "SOURCE" {
            let input = input.ok_or_else(|| {
                PlanError::new(format!(
                    "SOURCE binding expression {expr_id} has no element input"
                ))
            })?;
            let source_arg = args
                .first()
                .map(|arg| arg.value)
                .or_else(|| children.iter().find_map(|child| child.expr))
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "SOURCE binding expression {expr_id} has no typed source"
                    ))
                })?;
            let source = self.compile_expr_with_children(source_arg, &[], context, None)?;
            return Ok(self.push_expr(
                expr_id,
                DocumentValueClass::Render,
                DocumentExprOp::BindSource { input, source },
            ));
        }
        if let Some(field) = function.strip_prefix("Field/") {
            let input = input
                .or_else(|| {
                    args.first()
                        .map(|arg| arg.value)
                        .and_then(|id| self.compile_expr_with_children(id, &[], context, None).ok())
                })
                .ok_or_else(|| {
                    PlanError::new(format!("field projection `{function}` has no input"))
                })?;
            let field = self.intern_name(field);
            return Ok(self.push_expr(
                expr_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Project { input, field },
            ));
        }
        if let Some(constructor) = document_constructor(function) {
            if input.is_some() {
                return Err(PlanError::new(format!(
                    "render constructor `{function}` cannot be used as a pipeline operator"
                )));
            }
            return self.compile_constructor(expr_id, constructor, args, children, context);
        }
        if function == "List/map"
            && self.list_maps.get(&expr_id).is_some_and(|binding| {
                binding.result_kind == ListMapResultKind::RenderSlotMaterialization
            })
        {
            let input = input.ok_or_else(|| {
                PlanError::new(format!(
                    "render List/map expression {expr_id} has no list input"
                ))
            })?;
            return self.compile_materialization(expr_id, input, context);
        }
        if let Some(function_id) = self.resolve_function(function, context) {
            return self.compile_function_call(
                expr_id,
                function_id,
                args,
                children,
                context,
                input,
            );
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
                "unknown executable document function `{function}` at expression {expr_id}"
            ))
        })?;
        let conditional_children = self.children_are_match_arms(children);
        let mut consumed_children = false;
        let mut arguments = Vec::new();
        for arg in args {
            let arg_children = if conditional_children && self.expr_is_conditional(arg.value) {
                consumed_children = true;
                children
            } else {
                &[]
            };
            let value = self.compile_expr_with_children(arg.value, arg_children, context, None)?;
            arguments.push(DocumentBuiltinArgument {
                name: arg.name.as_deref().map(|name| self.intern_name(name)),
                value,
            });
        }
        if !children.is_empty() && !consumed_children {
            let mut child_context = context.clone();
            if let Some(scope) = self.scope_for_expression(input) {
                child_context.row_aliases.insert("item".to_owned(), scope);
            }
            for child in children {
                let value = self.compile_statement_value(child, &child_context, None)?;
                arguments.push(DocumentBuiltinArgument {
                    name: statement_field_name(child).map(|name| self.intern_name(&name)),
                    value,
                });
            }
        }
        Ok(self.push_expr(
            expr_id,
            DocumentValueClass::DynamicScalar,
            DocumentExprOp::Builtin {
                builtin,
                input,
                arguments,
            },
        ))
    }

    fn compile_function_call(
        &mut self,
        expr_id: usize,
        function_id: DocumentFunctionId,
        args: &[AstCallArg],
        children: &[AstStatement],
        context: &CompileContext,
        input: Option<DocumentExprId>,
    ) -> Result<DocumentExprId, PlanError> {
        let info = self.functions.get(&function_id).cloned().ok_or_else(|| {
            PlanError::new(format!("missing resolved function {}", function_id.0))
        })?;
        self.referenced_functions.insert(function_id);
        self.compile_function(function_id)?;
        let value_class = self
            .compiled_functions
            .iter()
            .find(|function| function.id == function_id)
            .map(|function| self.expressions[function.body.0].value_class)
            .unwrap_or(DocumentValueClass::DynamicStructure);
        let mut values = BTreeMap::<DocumentParameterId, DocumentExprId>::new();
        let mut passed = None;
        let mut positional = 0usize;
        let conditional_children = self.children_are_match_arms(children);
        let mut consumed_children = false;
        if let Some(input) = input {
            let parameter = info.parameters.first().copied().ok_or_else(|| {
                PlanError::new(format!(
                    "pipeline function `{}` has no input parameter",
                    info.name
                ))
            })?;
            values.insert(parameter, input);
            positional = 1;
        }
        for arg in args {
            if arg.name.as_deref() == Some("PASS") {
                if passed.is_some() {
                    return Err(PlanError::new(format!(
                        "function call {expr_id} supplies PASS more than once"
                    )));
                }
                let arg_children = if conditional_children && self.expr_is_conditional(arg.value) {
                    consumed_children = true;
                    children
                } else {
                    &[]
                };
                passed = Some(self.compile_expr_with_children(
                    arg.value,
                    arg_children,
                    context,
                    None,
                )?);
                continue;
            }
            let index = if let Some(name) = &arg.name {
                info.args
                    .iter()
                    .position(|candidate| candidate == name)
                    .or_else(|| {
                        info.parameters
                            .iter()
                            .position(|parameter| !values.contains_key(parameter))
                    })
            } else {
                let index = positional;
                positional += 1;
                Some(index)
            }
            .ok_or_else(|| {
                PlanError::new(format!(
                    "function `{}` has no argument `{}`",
                    info.name,
                    arg.name.as_deref().unwrap_or("<positional>")
                ))
            })?;
            let parameter = *info.parameters.get(index).ok_or_else(|| {
                PlanError::new(format!(
                    "function `{}` received too many arguments",
                    info.name
                ))
            })?;
            let arg_children = if conditional_children && self.expr_is_conditional(arg.value) {
                consumed_children = true;
                children
            } else {
                &[]
            };
            let value = self.compile_expr_with_children(arg.value, arg_children, context, None)?;
            if values.insert(parameter, value).is_some() {
                return Err(PlanError::new(format!(
                    "function `{}` parameter {} is supplied more than once",
                    info.name, parameter.0
                )));
            }
        }
        for child in children {
            let Some(name) = statement_field_name(child) else {
                continue;
            };
            if name == "PASS" {
                passed = Some(self.compile_statement_value(child, context, None)?);
                continue;
            }
            let index = info
                .args
                .iter()
                .position(|candidate| candidate == &name)
                .or_else(|| {
                    info.parameters
                        .iter()
                        .position(|parameter| !values.contains_key(parameter))
                })
                .ok_or_else(|| {
                    PlanError::new(format!("function `{}` has no argument `{name}`", info.name))
                })?;
            let parameter = info.parameters[index];
            let value = self.compile_statement_value(child, context, None)?;
            values.insert(parameter, value);
        }
        let arguments = values
            .into_iter()
            .map(|(parameter, value)| DocumentCallArgument { parameter, value })
            .collect();
        let mut call = self.push_expr(
            expr_id,
            value_class,
            DocumentExprOp::FunctionCall {
                function: function_id,
                arguments,
                passed,
            },
        );
        if !consumed_children {
            for child in children
                .iter()
                .filter(|child| !statement_has_named_field(child))
            {
                call = self.compile_statement_value(child, context, Some(call))?;
            }
        }
        Ok(call)
    }

    fn compile_constructor(
        &mut self,
        expr_id: usize,
        constructor: DocumentConstructor,
        args: &[AstCallArg],
        children: &[AstStatement],
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let mut arguments = Vec::new();
        let mut continuations = Vec::new();
        for arg in args {
            let name = arg.name.as_deref().ok_or_else(|| {
                PlanError::new(format!(
                    "render constructor expression {expr_id} has a positional argument"
                ))
            })?;
            let value = self.compile_expr_with_children(arg.value, &[], context, None)?;
            arguments.push(self.constructor_argument(name, value));
        }
        for child in children {
            let Some(name) = statement_field_name(child) else {
                if statement_is_empty_delimiter(child, self.program) {
                    continue;
                }
                if child_is_pipeline_continuation(child, self.program) {
                    continuations.push(child);
                    continue;
                }
                return Err(PlanError::new(format!(
                    "render constructor expression {expr_id} contains an unnamed executable slot"
                )));
            };
            let value = self.compile_statement_value(child, context, None)?;
            arguments.push(self.constructor_argument(&name, value));
        }
        let template = DocumentTemplateId(stable_compiler_identity(
            3,
            context.owner_function,
            expr_id,
        )?);
        let node = DocumentNodeId(stable_compiler_identity(
            4,
            context.owner_function,
            expr_id,
        )?);
        let mut expression = self.push_expr(
            expr_id,
            DocumentValueClass::Render,
            DocumentExprOp::Constructor {
                template,
                constructor,
                arguments,
            },
        );
        if self.template_ids.insert(template) {
            self.templates.push(DocumentTemplate {
                id: template,
                node,
                compiler_expr_id: expr_id,
                owner_function: context.owner_function,
                constructor,
                expression,
            });
        }
        for continuation in continuations {
            expression = self.compile_statement_value(continuation, context, Some(expression))?;
        }
        Ok(expression)
    }

    fn constructor_argument(
        &mut self,
        name: &str,
        value: DocumentExprId,
    ) -> DocumentConstructorArgument {
        let class = self.expressions[value.0].value_class;
        let role = match name {
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
        };
        DocumentConstructorArgument {
            name: self.intern_name(name),
            role,
            value,
        }
    }

    fn compile_materialization(
        &mut self,
        expr_id: usize,
        input: DocumentExprId,
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let binding = self.list_maps.get(&expr_id).cloned().ok_or_else(|| {
            PlanError::new(format!(
                "render List/map expression {expr_id} has no typed row binding"
            ))
        })?;
        let function_name = binding.template_function.as_deref().ok_or_else(|| {
            PlanError::new(format!(
                "render List/map expression {expr_id} has no typed template function"
            ))
        })?;
        let function_id = self.resolve_function(function_name, context).ok_or_else(|| {
            PlanError::new(format!(
                "render List/map expression {expr_id} references unknown template `{function_name}`"
            ))
        })?;
        let info = self.functions.get(&function_id).cloned().ok_or_else(|| {
            PlanError::new(format!("missing template function {}", function_id.0))
        })?;
        self.referenced_functions.insert(function_id);
        let item_scope = binding
            .item_scope_id
            .map(ScopeId)
            .or(info.row_scope)
            .or_else(|| self.scope_for_expression(Some(input)))
            .ok_or_else(|| {
                PlanError::new(format!(
                    "render List/map expression {expr_id} has no typed row scope"
                ))
            })?;
        let item_index = info
            .args
            .iter()
            .position(|name| name == &binding.item_binding_name)
            .or_else(|| {
                binding.template_args.iter().position(|arg| {
                    arg.name
                        .as_ref()
                        .is_some_and(|name| name == &binding.item_binding_name)
                })
            })
            .unwrap_or(0);
        let item_parameter = *info.parameters.get(item_index).ok_or_else(|| {
            PlanError::new(format!(
                "render List/map expression {expr_id} cannot resolve its row parameter"
            ))
        })?;
        let mut row_context = context.clone();
        row_context
            .row_aliases
            .insert(binding.item_binding_name.clone(), item_scope);
        let mut template_arguments = Vec::new();
        for arg in &binding.template_args {
            let Some(name) = arg.name.as_deref() else {
                continue;
            };
            let Some(index) = info.args.iter().position(|candidate| candidate == name) else {
                continue;
            };
            let parameter = info.parameters[index];
            if parameter == item_parameter {
                continue;
            }
            let value = self.compile_expr_with_children(arg.value, &[], &row_context, None)?;
            template_arguments.push(DocumentCallArgument { parameter, value });
        }
        let source = match self.expressions.get(input.0).map(|expr| &expr.op) {
            Some(DocumentExprOp::Read {
                read: DocumentRead::List { list },
            }) => DocumentMaterializationSource::List { list: *list },
            Some(DocumentExprOp::Read {
                read: DocumentRead::Field { field },
            }) => DocumentMaterializationSource::Field { field: *field },
            Some(DocumentExprOp::Read {
                read:
                    DocumentRead::Row {
                        scope,
                        field: Some(field),
                        ..
                    },
            }) => DocumentMaterializationSource::ScopedField {
                scope: *scope,
                field: *field,
            },
            Some(DocumentExprOp::Read {
                read:
                    DocumentRead::Parameter {
                        parameter,
                        projection,
                    },
            }) => {
                let field = projection
                    .first()
                    .and_then(|name| self.names.get(name.0))
                    .and_then(|name| self.resolve_unique_scoped_field(name));
                if let Some(field) = field {
                    DocumentMaterializationSource::ParameterField {
                        parameter: *parameter,
                        field,
                    }
                } else {
                    DocumentMaterializationSource::Parameter {
                        parameter: *parameter,
                        projection: projection.clone(),
                    }
                }
            }
            _ => DocumentMaterializationSource::Expression { expression: input },
        };
        let source_list = self.source_list_id(input);
        let row_identity = source_list
            .map(|list| DocumentRowIdentity::ListHiddenKeyAndGeneration { list })
            .unwrap_or(DocumentRowIdentity::ScopedHiddenKeyAndGeneration { scope: item_scope });
        let id = DocumentMaterializationId(stable_compiler_identity(
            5,
            context.owner_function,
            expr_id,
        )?);
        if self.materialization_ids.insert(id) {
            self.materializations.push(DocumentMaterialization {
                id,
                compiler_expr_id: expr_id,
                source,
                item_scope,
                item_parameter,
                template_function: function_id,
                template_arguments,
                row_identity,
                policy: DocumentMaterializationPolicy::VisibleRange,
            });
        }
        Ok(self.push_expr(
            expr_id,
            DocumentValueClass::ChildList,
            DocumentExprOp::Materialize {
                materialization: id,
            },
        ))
    }

    fn compile_select(
        &mut self,
        expr_id: usize,
        input: DocumentExprId,
        children: &[AstStatement],
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let mut arms = Vec::new();
        for child in children {
            let arm_expr = child.expr.ok_or_else(|| {
                PlanError::new(format!(
                    "conditional expression {expr_id} contains an arm without an expression"
                ))
            })?;
            let AstExprKind::MatchArm { pattern, .. } = self.expr_kind(arm_expr)? else {
                return Err(PlanError::new(format!(
                    "conditional expression {expr_id} contains a non-arm statement {}",
                    child.id
                )));
            };
            let pattern = pattern.clone();
            let mut arm_context = context.clone();
            arm_context.pattern_bindings.extend(
                pattern_binding_names(&pattern)
                    .into_iter()
                    .map(|name| (name, expr_id)),
            );
            let output =
                self.compile_expr_with_children(arm_expr, &child.children, &arm_context, None)?;
            arms.push(DocumentSelectArm {
                pattern: self.compile_pattern(&pattern)?,
                output,
            });
        }
        if arms.is_empty() {
            return Err(PlanError::new(format!(
                "conditional expression {expr_id} has no typed arms"
            )));
        }
        let class = std::iter::once(self.expressions[input.0].value_class)
            .chain(
                arms.iter()
                    .map(|arm| self.expressions[arm.output.0].value_class),
            )
            .max_by_key(|class| value_class_rank(*class))
            .unwrap_or(DocumentValueClass::DynamicScalar);
        Ok(self.push_expr(expr_id, class, DocumentExprOp::Select { input, arms }))
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

    fn compile_record_expr(
        &mut self,
        expr_id: usize,
        tag: Option<String>,
        fields: &[AstRecordField],
        children: &[AstStatement],
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let conditional_field = fields
            .iter()
            .position(|field| match self.expr_kind(field.value) {
                Ok(AstExprKind::When { .. }) => true,
                Ok(AstExprKind::Pipe { op, .. }) => matches!(op.as_str(), "WHEN" | "WHILE"),
                _ => false,
            });
        let mut compiled_fields = Vec::new();
        for (index, field) in fields.iter().enumerate() {
            let field_children = if Some(index) == conditional_field {
                children
            } else {
                &[]
            };
            let value =
                self.compile_expr_with_children(field.value, field_children, context, None)?;
            compiled_fields.push(DocumentRecordField {
                name: (!field.spread).then(|| self.intern_name(&field.name)),
                value,
                spread: field.spread,
            });
        }
        if !children.is_empty() && conditional_field.is_none() {
            for child in children {
                let value = self.compile_statement_value(child, context, None)?;
                compiled_fields.push(DocumentRecordField {
                    name: statement_field_name(child).map(|name| self.intern_name(&name)),
                    value,
                    spread: !statement_has_named_field(child),
                });
            }
        }
        let fields = compiled_fields;
        let class = record_value_class(&fields, &self.expressions);
        let op = if let Some(tag) = tag {
            DocumentExprOp::TaggedRecord {
                tag: self.intern_name(&tag),
                fields,
            }
        } else {
            DocumentExprOp::Record { fields }
        };
        Ok(self.push_expr(expr_id, class, op))
    }

    fn compile_record_children(
        &mut self,
        children: &[AstStatement],
        context: &CompileContext,
        compiler_id: usize,
    ) -> Result<DocumentExprId, PlanError> {
        self.compile_record_children_refs(
            &children.iter().collect::<Vec<_>>(),
            context,
            compiler_id,
        )
    }

    fn compile_record_children_refs(
        &mut self,
        children: &[&AstStatement],
        context: &CompileContext,
        compiler_id: usize,
    ) -> Result<DocumentExprId, PlanError> {
        let mut fields = Vec::new();
        for child in children {
            let value = self.compile_statement_value(child, context, None)?;
            fields.push(DocumentRecordField {
                name: statement_field_name(child).map(|name| self.intern_name(&name)),
                value,
                spread: !statement_has_named_field(child),
            });
        }
        let class = record_value_class(&fields, &self.expressions);
        Ok(self.push_expr(compiler_id, class, DocumentExprOp::Record { fields }))
    }

    fn compile_list_children(
        &mut self,
        children: &[AstStatement],
        context: &CompileContext,
        compiler_id: usize,
    ) -> Result<DocumentExprId, PlanError> {
        let items = children
            .iter()
            .map(|child| {
                self.compile_statement_value(child, context, None)
                    .map(|value| DocumentListItem {
                        value,
                        spread: false,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let class = list_value_class(&items, &self.expressions);
        Ok(self.push_expr(compiler_id, class, DocumentExprOp::List { items }))
    }

    fn compile_child_sequence(
        &mut self,
        children: &[AstStatement],
        context: &CompileContext,
        compiler_id: usize,
    ) -> Result<DocumentExprId, PlanError> {
        let is_pipeline = children.len() > 1
            && children
                .iter()
                .skip(1)
                .all(|child| child_is_pipeline_continuation(child, self.program));
        if !is_pipeline {
            return self.compile_list_children(children, context, compiler_id);
        }
        let mut value = self.compile_statement_value(&children[0], context, None)?;
        for child in &children[1..] {
            value = self.compile_statement_value(child, context, Some(value))?;
        }
        Ok(value)
    }

    fn compile_text_literal(
        &mut self,
        expr_id: usize,
        value: &str,
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let Some(open) = value.find('{') else {
            return Ok(self.constant_expr(
                expr_id,
                DocumentConstantValue::Text {
                    value: value.to_owned(),
                },
            ));
        };
        let mut cursor = 0usize;
        let mut segments = Vec::new();
        let mut next_open = Some(open);
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
                        "text expression {expr_id} has an unterminated interpolation"
                    ))
                })?;
            let path = value[open + 1..close].trim();
            if path.is_empty() {
                return Err(PlanError::new(format!(
                    "text expression {expr_id} has an empty interpolation"
                )));
            }
            let dynamic = self.compile_path(expr_id, path, context)?;
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
            expr_id,
            DocumentValueClass::DynamicScalar,
            DocumentExprOp::TextTemplate { segments },
        ))
    }

    fn compile_identifier(
        &mut self,
        expr_id: usize,
        value: &str,
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        if value == "NoElement" {
            return Ok(self.push_expr(
                expr_id,
                DocumentValueClass::Render,
                DocumentExprOp::NoElement,
            ));
        }
        if value == "SOURCE" {
            return Ok(self.push_expr(
                expr_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::SourceContext,
            ));
        }
        if value.chars().next().is_some_and(char::is_uppercase) {
            return self.compile_tag(expr_id, value);
        }
        self.compile_path(expr_id, value, context)
    }

    fn compile_tag(&mut self, expr_id: usize, value: &str) -> Result<DocumentExprId, PlanError> {
        if value == "NoElement" {
            return Ok(self.push_expr(
                expr_id,
                DocumentValueClass::Render,
                DocumentExprOp::NoElement,
            ));
        }
        let name = self.intern_name(value);
        Ok(self.constant_expr(expr_id, DocumentConstantValue::Enum { name }))
    }

    fn compile_path(
        &mut self,
        expr_id: usize,
        path: &str,
        context: &CompileContext,
    ) -> Result<DocumentExprId, PlanError> {
        let path = path.trim().trim_start_matches('$');
        let parts = path
            .split('.')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return Err(PlanError::new(format!(
                "document expression {expr_id} has an empty path"
            )));
        }
        let explicit_passed = parts.first() == Some(&"PASSED");
        let stripped = parts.strip_prefix(&["PASSED"]).unwrap_or(parts.as_slice());
        if !explicit_passed && stripped.len() == 1 {
            let name = stripped[0];
            if let Some(scope) = context.row_aliases.get(name).copied() {
                let expression = self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::Row {
                            scope,
                            field: None,
                            projection: Vec::new(),
                        },
                    },
                );
                self.record_compiled_path(context, path, expression);
                return Ok(expression);
            }
            if let Some(parameter) = context.parameters.get(name).copied() {
                let expression = self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::Parameter {
                            parameter,
                            projection: Vec::new(),
                        },
                    },
                );
                self.record_compiled_path(context, path, expression);
                return Ok(expression);
            }
            if let Some(local) = context.locals.get(name).copied() {
                let expression = self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::Local {
                            local,
                            projection: Vec::new(),
                        },
                    },
                );
                self.record_compiled_path(context, path, expression);
                return Ok(expression);
            }
        }
        if !explicit_passed
            && let Some(selector) = context
                .pattern_bindings
                .get(stripped.first().copied().unwrap_or_default())
                .copied()
        {
            let projection = stripped.iter().map(|part| self.intern_name(part)).collect();
            let expression = self.push_expr(
                expr_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::Matched {
                        selector,
                        projection,
                    },
                },
            );
            self.record_compiled_path(context, path, expression);
            return Ok(expression);
        }
        if !explicit_passed && stripped.len() > 1 {
            let first = stripped[0];
            let projection = stripped[1..]
                .iter()
                .map(|part| self.intern_name(part))
                .collect::<Vec<_>>();
            if let Some(scope) = context.row_aliases.get(first).copied() {
                let field = stripped
                    .get(1)
                    .and_then(|name| self.resolve_scoped_field(scope, name));
                let expression = self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::Row {
                            scope,
                            field,
                            projection,
                        },
                    },
                );
                self.record_compiled_path(context, path, expression);
                return Ok(expression);
            }
            if let Some(parameter) = context.parameters.get(first).copied() {
                let expression = self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::Parameter {
                            parameter,
                            projection,
                        },
                    },
                );
                self.record_compiled_path(context, path, expression);
                return Ok(expression);
            }
            if let Some(local) = context.locals.get(first).copied() {
                let expression = self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Read {
                        read: DocumentRead::Local { local, projection },
                    },
                );
                self.record_compiled_path(context, path, expression);
                return Ok(expression);
            }
        }
        if let Some(source) = self.resolve_source_alias(stripped) {
            let expression = self.push_expr(
                expr_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::Source { source },
                },
            );
            self.record_compiled_path(context, path, expression);
            return Ok(expression);
        }
        if let Some(expression) = self.compile_source_group(expr_id, stripped) {
            self.record_compiled_path(context, path, expression);
            return Ok(expression);
        }
        let typed_global = (1..=stripped.len()).rev().find_map(|prefix_len| {
            let prefix = stripped[..prefix_len].join(".");
            self.resolve_global(&prefix)
                .map(|global| (global, prefix_len))
        });
        if let Some((global, prefix_len)) = typed_global {
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
            };
            let mut expression = self.push_expr(expr_id, class, DocumentExprOp::Read { read });
            for field in &stripped[prefix_len..] {
                let field = self.intern_name(field);
                expression = self.push_expr(
                    expr_id,
                    DocumentValueClass::DynamicScalar,
                    DocumentExprOp::Project {
                        input: expression,
                        field,
                    },
                );
            }
            self.record_compiled_path(context, path, expression);
            return Ok(expression);
        }
        let first = stripped[0];
        let projection = stripped[1..]
            .iter()
            .map(|part| self.intern_name(part))
            .collect::<Vec<_>>();
        if first == "element" {
            let expression = self.push_expr(
                expr_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::ElementState { projection },
                },
            );
            self.record_compiled_path(context, path, expression);
            return Ok(expression);
        }
        if parts.first() == Some(&"PASSED") || self.expr_has_known_type(expr_id) {
            let projection = stripped.iter().map(|part| self.intern_name(part)).collect();
            let expression = self.push_expr(
                expr_id,
                DocumentValueClass::DynamicScalar,
                DocumentExprOp::Read {
                    read: DocumentRead::Passed { projection },
                },
            );
            self.record_compiled_path(context, path, expression);
            return Ok(expression);
        }
        let line = self
            .program
            .expressions
            .get(expr_id)
            .map(|expression| expression.line)
            .unwrap_or_default();
        Err(PlanError::new(format!(
            "unresolved executable document path `{path}` at expression {expr_id} (line {line})"
        )))
    }

    fn resolve_source_alias(&self, parts: &[&str]) -> Option<SourceId> {
        let normalized = parts
            .iter()
            .copied()
            .filter(|part| *part != "events")
            .collect::<Vec<_>>()
            .join(".");
        if let Some(source) = self
            .program
            .sources
            .iter()
            .find(|source| source.path == normalized)
        {
            return Some(SourceId(source.id.0));
        }
        let suffix = normalized
            .find(".sources.")
            .map(|offset| &normalized[offset..])?;
        let mut matches = self
            .program
            .sources
            .iter()
            .filter(|source| source.path.ends_with(suffix));
        let first = matches.next()?;
        matches.next().is_none().then_some(SourceId(first.id.0))
    }

    fn compile_source_group(
        &mut self,
        compiler_id: usize,
        parts: &[&str],
    ) -> Option<DocumentExprId> {
        let normalized = parts
            .iter()
            .copied()
            .filter(|part| *part != "events")
            .collect::<Vec<_>>()
            .join(".");
        let prefix = format!("{normalized}.");
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
            let target = if let Some(source) = binding.source_id {
                DocumentBindingTarget::Source {
                    source: SourceId(source.0),
                }
            } else if let Some(global) = self.resolve_global(&binding.path) {
                match global {
                    GlobalValue::State(state) => DocumentBindingTarget::State { state },
                    GlobalValue::Field(field) => DocumentBindingTarget::Field { field },
                    GlobalValue::List(list) => DocumentBindingTarget::List { list },
                    GlobalValue::Source(source) => DocumentBindingTarget::Source { source },
                }
            } else if let Some(scope) = scope
                && let Some(field) = binding
                    .path
                    .rsplit('.')
                    .next()
                    .and_then(|name| self.resolve_scoped_field(scope, name))
            {
                DocumentBindingTarget::ScopedField { scope, field }
            } else {
                let expression = path_lookup_variants(&binding.path)
                    .into_iter()
                    .find_map(|path| {
                        self.compiled_paths
                            .get(&(scope, path.clone()))
                            .or_else(|| self.compiled_paths.get(&(None, path)))
                            .copied()
                    })
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "view binding {} has no typed SourceId, FieldId, StateId, ListId, or expression",
                            binding.id.0
                        ))
                    })?;
                DocumentBindingTarget::Expression { expression }
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

    fn resolve_function(&self, name: &str, context: &CompileContext) -> Option<DocumentFunctionId> {
        if let Some(id) = self.functions_by_name.get(name) {
            return Some(*id);
        }
        if !name.contains('/')
            && let Some(owner) = context.owner_name.as_deref()
            && let Some((namespace, _)) = owner.rsplit_once('/')
            && let Some(id) = self.functions_by_name.get(&format!("{namespace}/{name}"))
        {
            return Some(*id);
        }
        self.function_aliases.get(name).copied().flatten()
    }

    fn resolve_global(&self, path: &str) -> Option<GlobalValue> {
        self.globals
            .get(path)
            .copied()
            .or_else(|| self.global_aliases.get(path).copied().flatten())
    }

    fn resolve_scoped_field(&self, scope: ScopeId, name: &str) -> Option<FieldId> {
        self.scoped_fields.get(&(scope, name.to_owned())).copied()
    }

    fn resolve_unique_scoped_field(&self, name: &str) -> Option<FieldId> {
        let fields = self
            .scoped_fields
            .iter()
            .filter_map(|((_, candidate), field)| (candidate == name).then_some(*field))
            .collect::<BTreeSet<_>>();
        (fields.len() == 1).then(|| *fields.first().expect("checked one field"))
    }

    fn scope_for_expression(&self, expression: Option<DocumentExprId>) -> Option<ScopeId> {
        let expression = expression?;
        match &self.expressions.get(expression.0)?.op {
            DocumentExprOp::Read {
                read: DocumentRead::List { list },
            } => self
                .list_by_scope
                .iter()
                .find_map(|(scope, candidate)| (candidate == list).then_some(*scope)),
            DocumentExprOp::Read {
                read: DocumentRead::Row { scope, .. },
            } => Some(*scope),
            DocumentExprOp::Builtin { input, .. } => self.scope_for_expression(*input),
            _ => None,
        }
    }

    fn source_list_id(&self, expression: DocumentExprId) -> Option<ListId> {
        match &self.expressions.get(expression.0)?.op {
            DocumentExprOp::Read {
                read: DocumentRead::List { list },
            } => Some(*list),
            DocumentExprOp::Builtin {
                input: Some(input), ..
            } => self.source_list_id(*input),
            _ => None,
        }
    }

    fn expr_kind(&self, id: usize) -> Result<&AstExprKind, PlanError> {
        self.program
            .expressions
            .get(id)
            .map(|expr| &expr.kind)
            .ok_or_else(|| PlanError::new(format!("missing typed expression {id}")))
    }

    fn expr_is_delimiter(&self, id: usize) -> bool {
        matches!(self.expr_kind(id), Ok(AstExprKind::Delimiter))
    }

    fn expr_is_empty_list(&self, id: usize) -> bool {
        matches!(
            self.expr_kind(id),
            Ok(AstExprKind::ListLiteral { items, .. }) if items.is_empty()
        )
    }

    fn expr_is_block_keyword(&self, id: usize) -> bool {
        matches!(
            self.expr_kind(id),
            Ok(AstExprKind::Identifier(value) | AstExprKind::Enum(value)) if value == "BLOCK"
        )
    }

    fn expr_is_conditional(&self, id: usize) -> bool {
        match self.expr_kind(id) {
            Ok(AstExprKind::When { .. }) => true,
            Ok(AstExprKind::Pipe { op, .. }) => matches!(op.as_str(), "WHEN" | "WHILE"),
            _ => false,
        }
    }

    fn children_are_match_arms(&self, children: &[AstStatement]) -> bool {
        !children.is_empty()
            && children.iter().all(|child| {
                child.expr.is_some_and(|expr| {
                    matches!(self.expr_kind(expr), Ok(AstExprKind::MatchArm { .. }))
                })
            })
    }

    fn expr_consumes_children(&self, id: usize) -> Result<bool, PlanError> {
        Ok(match self.expr_kind(id)? {
            AstExprKind::Call { .. }
            | AstExprKind::When { .. }
            | AstExprKind::MatchArm { .. }
            | AstExprKind::Object(_)
            | AstExprKind::Record(_)
            | AstExprKind::ListLiteral { .. }
            | AstExprKind::Delimiter
            | AstExprKind::Latest => true,
            AstExprKind::Pipe { op, .. } => matches!(op.as_str(), "WHEN" | "WHILE"),
            _ => false,
        })
    }

    fn expr_has_known_type(&self, expr_id: usize) -> bool {
        self.program
            .typecheck_report
            .expr_type_table
            .entries
            .iter()
            .find(|entry| entry.expr_id == expr_id)
            .is_some_and(|entry| {
                !matches!(
                    entry.flow_type.ty,
                    Type::Unknown | Type::UnresolvedShape { .. }
                )
            })
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

    fn record_compiled_path(
        &mut self,
        context: &CompileContext,
        path: &str,
        expression: DocumentExprId,
    ) {
        let scope = path
            .split('.')
            .next()
            .and_then(|first| context.row_aliases.get(first))
            .copied();
        for path in path_lookup_variants(path) {
            self.compiled_paths
                .entry((scope, path))
                .or_insert(expression);
        }
    }
}

fn parameter_id(
    function: DocumentFunctionId,
    parameter_index: usize,
) -> Result<DocumentParameterId, PlanError> {
    const PARAMETER_STRIDE: usize = 4096;
    if parameter_index >= PARAMETER_STRIDE {
        return Err(PlanError::new(format!(
            "function {} exceeds the typed document parameter limit",
            function.0
        )));
    }
    function
        .0
        .checked_mul(PARAMETER_STRIDE)
        .and_then(|base| base.checked_add(parameter_index))
        .map(DocumentParameterId)
        .ok_or_else(|| PlanError::new("typed document parameter id overflow"))
}

fn statement_is_source_group(program: &TypedProgram, statement: &AstStatement) -> bool {
    !statement.children.is_empty()
        && statement.children.iter().all(|child| match child.kind {
            AstStatementKind::Source { .. } => true,
            AstStatementKind::Field { .. } => statement_is_source_group(program, child),
            _ if statement_is_empty_delimiter(child, program) => true,
            _ => false,
        })
}

fn list_for_semantic_path<'a>(
    program: &'a TypedProgram,
    path: &str,
) -> Option<&'a boon_ir::ListMemory> {
    program
        .lists
        .iter()
        .find(|list| path == list.name || path.ends_with(&format!(".{}", list.name)))
}

fn stable_compiler_identity(
    kind: u8,
    owner: Option<DocumentFunctionId>,
    compiler_id: usize,
) -> Result<u64, PlanError> {
    let owner = owner.map(|owner| owner.0 + 1).unwrap_or(0);
    if owner > 0x00ff_ffff || compiler_id > u32::MAX as usize {
        return Err(PlanError::new(
            "typed document compiler identity exceeds its stable encoding",
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

fn insert_unique_alias<T: Copy + Eq>(
    aliases: &mut BTreeMap<String, Option<T>>,
    alias: String,
    value: T,
) {
    aliases
        .entry(alias)
        .and_modify(|existing| {
            if *existing != Some(value) {
                *existing = None;
            }
        })
        .or_insert(Some(value));
}

fn path_aliases(path: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    if let Some(stripped) = path.strip_prefix("store.") {
        aliases.push(stripped.to_owned());
    }
    if let Some(last) = path.rsplit('.').next() {
        aliases.push(last.to_owned());
    }
    aliases
}

fn path_lookup_variants(path: &str) -> Vec<String> {
    let mut variants = vec![path.to_owned()];
    if let Some(path) = path.strip_prefix("PASSED.") {
        variants.push(path.to_owned());
    }
    if let Some(path) = path.strip_prefix("PASSED.store.") {
        variants.push(path.to_owned());
        variants.push(format!("store.{path}"));
    } else if let Some(path) = path.strip_prefix("store.") {
        variants.push(path.to_owned());
    }
    variants.sort();
    variants.dedup();
    variants
}

fn function_aliases_for_name(name: &str) -> Vec<String> {
    name.rsplit('/')
        .next()
        .filter(|short| *short != name)
        .map(|short| vec![short.to_owned()])
        .unwrap_or_default()
}

fn statement_field_name(statement: &AstStatement) -> Option<String> {
    match &statement.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::List {
            field: Some(name), ..
        }
        | AstStatementKind::Source {
            field: Some(name), ..
        }
        | AstStatementKind::Hold {
            field: Some(name), ..
        } => Some(name.clone()),
        _ => None,
    }
}

fn statement_has_named_field(statement: &AstStatement) -> bool {
    statement_field_name(statement).is_some()
}

fn statement_is_record_entry(statement: &AstStatement) -> bool {
    statement_has_named_field(statement) || matches!(statement.kind, AstStatementKind::Spread)
}

fn is_child_list_field(name: &str) -> bool {
    matches!(name, "items" | "children" | "shadows" | "lights")
}

fn child_is_pipeline_continuation(statement: &AstStatement, program: &TypedProgram) -> bool {
    statement
        .expr
        .and_then(|id| program.expressions.get(id))
        .is_some_and(|expr| {
            matches!(
                &expr.kind,
                AstExprKind::Pipe { input, .. }
                    if program
                        .expressions
                        .get(*input)
                        .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
            )
        })
}

fn statement_is_empty_delimiter(statement: &AstStatement, program: &TypedProgram) -> bool {
    statement.children.is_empty()
        && statement
            .expr
            .and_then(|id| program.expressions.get(id))
            .is_some_and(|expr| matches!(expr.kind, AstExprKind::Delimiter))
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
        "Element/embedded_media" => DocumentConstructor::ElementEmbeddedMedia,
        "Scene/new" => DocumentConstructor::SceneNew,
        "Scene/Element/stripe" => DocumentConstructor::SceneElementStripe,
        "Scene/Element/block" => DocumentConstructor::SceneElementBlock,
        "Scene/Element/text" => DocumentConstructor::SceneElementText,
        "Scene/Element/text_input" => DocumentConstructor::SceneElementTextInput,
        "Scene/Element/checkbox" => DocumentConstructor::SceneElementCheckbox,
        "Scene/Element/label" => DocumentConstructor::SceneElementLabel,
        "Scene/Element/button" => DocumentConstructor::SceneElementButton,
        "Scene/Element/paragraph" => DocumentConstructor::SceneElementParagraph,
        "Scene/Element/link" => DocumentConstructor::SceneElementLink,
        "Scene/Element/embedded_media" => DocumentConstructor::SceneElementEmbeddedMedia,
        _ => return None,
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
        "File/read_bytes" => DocumentBuiltin::FileReadBytes,
        "Light/ambient" => DocumentBuiltin::LightAmbient,
        "Light/directional" => DocumentBuiltin::LightDirectional,
        "Light/spot" => DocumentBuiltin::LightSpot,
        "List/any" => DocumentBuiltin::ListAny,
        "List/append" => DocumentBuiltin::ListAppend,
        "List/chunk" => DocumentBuiltin::ListChunk,
        "List/count" => DocumentBuiltin::ListCount,
        "List/filter_field_equal" => DocumentBuiltin::ListFilterFieldEqual,
        "List/filter_field_not_equal" => DocumentBuiltin::ListFilterFieldNotEqual,
        "List/filter_text_contains" => DocumentBuiltin::ListFilterTextContains,
        "List/find" => DocumentBuiltin::ListFind,
        "List/find_value" => DocumentBuiltin::ListFindValue,
        "List/get" => DocumentBuiltin::ListGet,
        "List/is_not_empty" => DocumentBuiltin::ListIsNotEmpty,
        "List/join_field" => DocumentBuiltin::ListJoinField,
        "List/latest" => DocumentBuiltin::ListLatest,
        "List/length" => DocumentBuiltin::ListLength,
        "List/map" => DocumentBuiltin::ListMap,
        "List/range" => DocumentBuiltin::ListRange,
        "List/remove" => DocumentBuiltin::ListRemove,
        "List/retain" => DocumentBuiltin::ListRetain,
        "List/sort_by" => DocumentBuiltin::ListSortBy,
        "List/sum" => DocumentBuiltin::ListSum,
        "Number/bit_width" => DocumentBuiltin::NumberBitWidth,
        "Number/interpolate" => DocumentBuiltin::NumberInterpolate,
        "Number/max" => DocumentBuiltin::NumberMax,
        "Number/min" => DocumentBuiltin::NumberMin,
        "Number/project_offset" => DocumentBuiltin::NumberProjectOffset,
        "Number/project_time" => DocumentBuiltin::NumberProjectTime,
        "Number/project_width" => DocumentBuiltin::NumberProjectWidth,
        "Number/to_ascii_text" => DocumentBuiltin::NumberToAsciiText,
        "Number/to_text" => DocumentBuiltin::NumberToText,
        "Router/go_to" => DocumentBuiltin::RouterGoTo,
        "Router/route" => DocumentBuiltin::RouterRoute,
        "C/svg" => DocumentBuiltin::Svg,
        "Text/all_chars_in" => DocumentBuiltin::TextAllCharsIn,
        "Text/concat" => DocumentBuiltin::TextConcat,
        "Text/contains" => DocumentBuiltin::TextContains,
        "Text/empty" => DocumentBuiltin::TextEmpty,
        "Text/is_empty" => DocumentBuiltin::TextIsEmpty,
        "Text/join_lines" => DocumentBuiltin::TextJoinLines,
        "Text/length" => DocumentBuiltin::TextLength,
        "Text/space" => DocumentBuiltin::TextSpace,
        "Text/starts_with" => DocumentBuiltin::TextStartsWith,
        "Text/substring" => DocumentBuiltin::TextSubstring,
        "Text/time_range_label" => DocumentBuiltin::TextTimeRangeLabel,
        "Text/to_bytes" => DocumentBuiltin::TextToBytes,
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
