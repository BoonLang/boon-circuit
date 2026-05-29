use boon_parser::{
    AstCallArg, AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind, ParsedProgram,
};
use ena::unify::{InPlaceUnificationTable, NoError, UnifyKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Type {
    Text,
    Number,
    VariantSet(Vec<Variant>),
    Object(ObjectShape),
    List(Box<Type>),
    Function {
        args: Vec<Type>,
        result: Box<FlowType>,
    },
    Var(TypeVar),
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Variant {
    Tag(String),
    Tagged { tag: String, fields: ObjectShape },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObjectShape {
    pub fields: BTreeMap<String, Type>,
    pub open: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct TypeVar(pub u32);

impl UnifyKey for TypeVar {
    type Value = ();

    fn index(&self) -> u32 {
        self.0
    }

    fn from_index(index: u32) -> Self {
        Self(index)
    }

    fn tag() -> &'static str {
        "BoonTypeVar"
    }
}

#[derive(Default)]
pub struct TypeVarStore {
    table: InPlaceUnificationTable<TypeVar>,
}

impl TypeVarStore {
    pub fn new_var(&mut self) -> TypeVar {
        self.table.new_key(())
    }

    pub fn unify(&mut self, left: TypeVar, right: TypeVar) -> Result<(), NoError> {
        self.table.unify_var_var(left, right)
    }

    pub fn root(&mut self, var: TypeVar) -> TypeVar {
        self.table.find(var)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeScheme {
    pub vars: Vec<TypeVar>,
    pub ty: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FlowType {
    pub mode: FlowMode,
    pub ty: Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FlowMode {
    Continuous,
    TickPresent,
    PresentOrAbsent,
    Absent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    Equal {
        left: Type,
        right: Type,
    },
    Assignable {
        actual: Type,
        expected: Type,
    },
    HasField {
        value: Type,
        field: String,
        field_type: Type,
    },
    HasVariant {
        value: Type,
        variant: Variant,
    },
    SatisfiesRenderSlot {
        slot_statement_id: usize,
        slot_name: String,
        actual: Type,
    },
    FlowCompatible {
        actual: FlowType,
        expected: FlowType,
    },
    PatternCovers {
        expr_id: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeDiagnostic {
    pub severity: DiagnosticSeverity,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExprTypeEntry {
    pub expr_id: usize,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct ExprTypeTable {
    pub entries: Vec<ExprTypeEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FunctionTypeEntry {
    pub name: String,
    pub args: Vec<String>,
    pub result: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct FunctionTypeTable {
    pub entries: Vec<FunctionTypeEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RenderSlot {
    pub slot_statement_id: usize,
    pub slot_name: String,
    pub expected_contract: String,
    pub value_expr_id: Option<usize>,
    pub actual_type: Type,
    pub diagnostics: Vec<TypeDiagnostic>,
    pub optional_list_map_binding_id: Option<usize>,
    pub item_scope_id: Option<usize>,
    pub template_function: Option<String>,
    pub template_args: Vec<AstCallArg>,
    pub materialization_policy: MaterializationPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct RenderSlotTable {
    pub slots: Vec<RenderSlot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListMapBinding {
    pub map_expr_id: usize,
    pub list_expr_id: usize,
    pub input_list_type: Type,
    pub item_expr_id: usize,
    pub item_binding_name: String,
    pub item_type: Type,
    pub result_type: Type,
    pub item_scope_id: Option<usize>,
    pub template_function: Option<String>,
    pub template_args: Vec<AstCallArg>,
    pub result_kind: ListMapResultKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListMapResultKind {
    RuntimeValue,
    RenderSlotMaterialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MaterializationPolicy {
    RuntimeValue,
    RenderSlotMaterialization,
    StaticChildren,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeCheckReport {
    pub expression_count: usize,
    pub checked_expression_count: usize,
    pub unresolved_type_variable_count: usize,
    pub dynamic_fallback_count: usize,
    pub render_slot_count: usize,
    pub render_slot_failure_count: usize,
    pub builtin_signature_coverage: Vec<String>,
    pub source_payload_shape_coverage: Vec<String>,
    pub full_document_typecheck_coverage: bool,
    pub list_map_binding_count_runtime_value: usize,
    pub list_map_binding_count_render_slot_materialization: usize,
    pub expr_type_table: ExprTypeTable,
    pub function_type_table: FunctionTypeTable,
    pub render_slot_table: RenderSlotTable,
    pub list_map_bindings: Vec<ListMapBinding>,
    pub constraints: Vec<Constraint>,
    pub diagnostics: Vec<TypeDiagnostic>,
}

impl TypeCheckReport {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            || self.render_slot_failure_count > 0
    }
}

pub fn check(program: &ParsedProgram) -> TypeCheckReport {
    let mut checker = Checker::new(program);
    checker.check_program()
}

struct Checker<'a> {
    program: &'a ParsedProgram,
    vars: TypeVarStore,
    builtins: BuiltinSignatureRegistry,
    render_contracts: RenderContractRegistry,
    source_paths: BTreeSet<String>,
    object_bindings: BTreeMap<String, ObjectShape>,
    name_bindings: BTreeMap<String, Type>,
    flow_bindings: BTreeMap<String, FlowMode>,
    expr_type_vars: BTreeMap<usize, TypeVar>,
    runtime_list_map_exprs: BTreeSet<usize>,
    visited: BTreeSet<usize>,
    expr_type_table: ExprTypeTable,
    function_type_table: FunctionTypeTable,
    render_slot_table: RenderSlotTable,
    list_map_bindings: Vec<ListMapBinding>,
    constraints: Vec<Constraint>,
    diagnostics: Vec<TypeDiagnostic>,
}

impl<'a> Checker<'a> {
    fn new(program: &'a ParsedProgram) -> Self {
        let source_paths = program
            .source_ports
            .iter()
            .map(|source| source.path.clone())
            .collect();
        let object_bindings = object_bindings(program);
        let name_bindings = name_bindings(program);
        let flow_bindings = flow_bindings(program);
        Self {
            program,
            vars: TypeVarStore::default(),
            builtins: BuiltinSignatureRegistry::default(),
            render_contracts: RenderContractRegistry::default(),
            source_paths,
            object_bindings,
            name_bindings,
            flow_bindings,
            expr_type_vars: BTreeMap::new(),
            runtime_list_map_exprs: BTreeSet::new(),
            visited: BTreeSet::new(),
            expr_type_table: ExprTypeTable::default(),
            function_type_table: FunctionTypeTable::default(),
            render_slot_table: RenderSlotTable::default(),
            list_map_bindings: Vec::new(),
            constraints: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn check_program(&mut self) -> TypeCheckReport {
        for statement in &self.program.ast.statements {
            self.check_statement(statement, false);
        }
        for expr in &self.program.expressions {
            self.ensure_expr(expr.id);
        }
        let render_slot_count = self.render_slot_table.slots.len();
        let render_slot_failure_count = self
            .render_slot_table
            .slots
            .iter()
            .map(|slot| slot.diagnostics.len())
            .sum();
        let list_map_binding_count_render_slot_materialization = self
            .list_map_bindings
            .iter()
            .filter(|binding| binding.result_kind == ListMapResultKind::RenderSlotMaterialization)
            .count();
        let list_map_binding_count_runtime_value = self
            .list_map_bindings
            .iter()
            .filter(|binding| binding.result_kind == ListMapResultKind::RuntimeValue)
            .count();
        let unresolved_type_variable_count = self.unresolved_type_variable_count();
        let unknown_type_count = self
            .expr_type_table
            .entries
            .iter()
            .filter(|entry| matches!(entry.flow_type.ty, Type::Unknown))
            .count();
        TypeCheckReport {
            expression_count: self.program.expressions.len(),
            checked_expression_count: self.visited.len(),
            unresolved_type_variable_count,
            dynamic_fallback_count: unknown_type_count + unresolved_type_variable_count,
            render_slot_count,
            render_slot_failure_count,
            builtin_signature_coverage: builtin_signature_coverage(self.program),
            source_payload_shape_coverage: self
                .program
                .source_ports
                .iter()
                .map(|source| source.path.clone())
                .collect(),
            full_document_typecheck_coverage: document_root(self.program).is_none_or(|root| {
                statement_expr_ids(root)
                    .into_iter()
                    .all(|expr_id| self.visited.contains(&expr_id))
            }),
            list_map_binding_count_runtime_value,
            list_map_binding_count_render_slot_materialization,
            expr_type_table: std::mem::take(&mut self.expr_type_table),
            function_type_table: std::mem::take(&mut self.function_type_table),
            render_slot_table: std::mem::take(&mut self.render_slot_table),
            list_map_bindings: std::mem::take(&mut self.list_map_bindings),
            constraints: std::mem::take(&mut self.constraints),
            diagnostics: std::mem::take(&mut self.diagnostics),
        }
    }

    fn check_statement(&mut self, statement: &AstStatement, in_document: bool) {
        let next_in_document =
            in_document || statement_field(statement).as_deref() == Some("document");
        if let Some(expr_id) = statement.expr {
            self.ensure_expr(expr_id);
        }
        self.check_hold_update_compatibility(statement);
        if let AstStatementKind::Function { name, args } = &statement.kind {
            self.function_type_table.entries.push(FunctionTypeEntry {
                name: name.clone(),
                args: args.clone(),
                result: FlowType {
                    mode: FlowMode::Continuous,
                    ty: self.type_for_call(name),
                },
            });
        }
        if matches!(
            statement_field(statement).as_deref(),
            Some("items" | "children")
        ) {
            self.check_render_slot(statement);
        }
        for child in &statement.children {
            self.check_statement(child, next_in_document);
        }
    }

    fn check_render_slot(&mut self, statement: &AstStatement) {
        let slot_name = statement_field(statement).unwrap_or_else(|| "items".to_owned());
        let expected_contract = self.render_contracts.slot_contract(&slot_name).to_owned();
        let mut value_expr_id = statement.expr;
        let mut optional_list_map_binding_id = None;
        let mut item_scope_id = None;
        let mut template_function = None;
        let mut template_args = Vec::new();
        let mut actual_type = value_expr_id
            .map(|expr_id| self.ensure_expr(expr_id).ty)
            .unwrap_or_else(|| Type::List(Box::new(open_object_type())));
        let mut materialization_policy = MaterializationPolicy::StaticChildren;

        let mut diagnostics = Vec::new();

        if let Some(mapped) = mapped_children_for_statement(statement, &self.program.expressions) {
            self.runtime_list_map_exprs.remove(&mapped.map_expr_id);
            self.list_map_bindings
                .retain(|binding| binding.map_expr_id != mapped.map_expr_id);
            value_expr_id = Some(mapped.map_expr_id);
            actual_type = Type::List(Box::new(renderable_contract_type()));
            item_scope_id = Some(mapped.item_scope_id);
            template_function = Some(mapped.template_function.clone());
            template_args = mapped.template_args.clone();
            materialization_policy = MaterializationPolicy::RenderSlotMaterialization;
            let binding_id = self.list_map_bindings.len();
            optional_list_map_binding_id = Some(binding_id);
            self.list_map_bindings.push(ListMapBinding {
                map_expr_id: mapped.map_expr_id,
                list_expr_id: mapped.list_expr_id,
                input_list_type: Type::List(Box::new(open_object_type())),
                item_expr_id: mapped.item_expr_id,
                item_binding_name: mapped.item_binding_name,
                item_type: open_object_type(),
                result_type: Type::List(Box::new(renderable_contract_type())),
                item_scope_id,
                template_function: template_function.clone(),
                template_args: template_args.clone(),
                result_kind: ListMapResultKind::RenderSlotMaterialization,
            });
        } else if let Some(expr_id) = statement.expr
            && self.expr_is_direct_data_list(expr_id)
        {
            diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!("expected a list of renderable values for `{slot_name}:`"),
            ));
        } else if render_slot_contains_malformed_list_map(statement, &self.program.expressions)
            && let Some(expr_id) = statement.expr.or_else(|| first_child_expr_id(statement))
        {
            diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!("expected `List/map(item, new: row(item: item))` to produce renderable values for `{slot_name}:`"),
            ));
        }

        self.constraints.push(Constraint::SatisfiesRenderSlot {
            slot_statement_id: statement.id,
            slot_name: slot_name.clone(),
            actual: actual_type.clone(),
        });
        self.render_slot_table.slots.push(RenderSlot {
            slot_statement_id: statement.id,
            slot_name,
            expected_contract,
            value_expr_id,
            actual_type,
            diagnostics: diagnostics.clone(),
            optional_list_map_binding_id,
            item_scope_id,
            template_function,
            template_args,
            materialization_policy,
        });
        self.diagnostics.extend(diagnostics);
    }

    fn ensure_expr(&mut self, expr_id: usize) -> FlowType {
        if let Some(existing) = self
            .expr_type_table
            .entries
            .iter()
            .find(|entry| entry.expr_id == expr_id)
            .map(|entry| entry.flow_type.clone())
        {
            return existing;
        }
        self.visited.insert(expr_id);
        let flow_type = self
            .program
            .expressions
            .get(expr_id)
            .map(|expr| self.infer_expr(expr))
            .unwrap_or(FlowType {
                mode: FlowMode::Continuous,
                ty: self.expr_type_var(expr_id),
            });
        self.expr_type_table.entries.push(ExprTypeEntry {
            expr_id,
            flow_type: flow_type.clone(),
        });
        flow_type
    }

    fn infer_expr(&mut self, expr: &AstExpr) -> FlowType {
        let ty = match &expr.kind {
            AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Type::Text,
            AstExprKind::Number(_) => Type::Number,
            AstExprKind::Bool(value) => Type::VariantSet(vec![Variant::Tag(if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            })]),
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) => {
                Type::VariantSet(vec![Variant::Tag(tag.clone())])
            }
            AstExprKind::TaggedObject { tag, fields } => {
                let shape = ObjectShape {
                    fields: fields
                        .iter()
                        .map(|field| (field.name.clone(), self.ensure_expr(field.value).ty))
                        .collect(),
                    open: false,
                };
                self.check_tagged_object_contract(expr, tag, fields, &shape);
                Type::VariantSet(vec![Variant::Tagged {
                    tag: tag.clone(),
                    fields: shape,
                }])
            }
            AstExprKind::Record(fields) | AstExprKind::Object(fields) => {
                Type::Object(ObjectShape {
                    fields: fields
                        .iter()
                        .map(|field| (field.name.clone(), self.ensure_expr(field.value).ty))
                        .collect(),
                    open: false,
                })
            }
            AstExprKind::ListLiteral { .. } => Type::List(Box::new(open_object_type())),
            AstExprKind::Call { function, args } => {
                for arg in args {
                    self.ensure_expr(arg.value);
                }
                if function == "Bool/not" {
                    let input_flow = args
                        .first()
                        .map(|arg| self.ensure_expr(arg.value))
                        .unwrap_or(FlowType {
                            mode: FlowMode::Continuous,
                            ty: Type::Unknown,
                        });
                    self.check_true_false_input(expr, &input_flow);
                    true_false_type()
                } else {
                    self.type_for_call_expr(expr.id, function)
                }
            }
            AstExprKind::Pipe { input, op, args } => {
                let input_flow = self.ensure_expr(*input);
                for arg in args {
                    self.ensure_expr(arg.value);
                }
                if op == "List/map" {
                    self.record_runtime_list_map(expr.id, *input, args);
                    Type::List(Box::new(open_object_type()))
                } else if op == "Bool/not" {
                    self.check_true_false_input(expr, &input_flow);
                    true_false_type()
                } else {
                    self.type_for_call_expr(expr.id, op)
                }
            }
            AstExprKind::Hold { initial, .. } => self.ensure_expr(*initial).ty,
            AstExprKind::Latest => open_object_type(),
            AstExprKind::When { input } => self.ensure_expr(*input).ty,
            AstExprKind::Then { input, output } => {
                let input_flow = self.ensure_expr(*input);
                if !matches!(
                    input_flow.mode,
                    FlowMode::TickPresent | FlowMode::PresentOrAbsent
                ) && !matches!(input_flow.ty, Type::Unknown)
                    && !is_open_object_type(&input_flow.ty)
                {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        *input,
                        "`THEN` requires a tick-present-or-absent value".to_owned(),
                    ));
                }
                output
                    .map(|output| self.ensure_expr(output).ty)
                    .unwrap_or(input_flow.ty)
            }
            AstExprKind::Infix { left, right, op } => {
                self.ensure_expr(*left);
                self.ensure_expr(*right);
                if matches!(op.as_str(), "==" | ">" | "<" | ">=" | "<=") {
                    true_false_type()
                } else {
                    Type::Number
                }
            }
            AstExprKind::MatchArm { output, .. } => output
                .map(|output| self.ensure_expr(output).ty)
                .unwrap_or_else(open_object_type),
            AstExprKind::Source => open_object_type(),
            AstExprKind::Identifier(value) => {
                if value == "BLOCK" {
                    open_object_type()
                } else if let Some(ty) = self.name_bindings.get(value) {
                    ty.clone()
                } else {
                    self.diagnostics.push(
                        self.diagnostic_for_expr(expr.id, format!("unknown identifier `{value}`")),
                    );
                    self.expr_type_var(expr.id)
                }
            }
            AstExprKind::Delimiter => Type::List(Box::new(open_object_type())),
            AstExprKind::Unknown(tokens) if tokens.iter().any(|token| token.starts_with('"')) => {
                Type::Text
            }
            AstExprKind::Unknown(tokens) => {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    format!("could not infer expression `{}`", tokens.join(" ")),
                ));
                self.expr_type_var(expr.id)
            }
            AstExprKind::Path(parts) => self.type_for_path(expr.id, parts),
        };
        FlowType {
            mode: self.flow_mode_for_expr(expr),
            ty,
        }
    }

    fn type_for_path(&mut self, expr_id: usize, parts: &[String]) -> Type {
        let path = parts.join(".");
        if path_is_source_path(&self.source_paths, &path) {
            return source_payload_type(parts);
        }
        if let Some(ty) = self.name_bindings.get(&path) {
            return ty.clone();
        }
        if parts.len() >= 2
            && let Some(shape) = self.object_bindings.get(&parts[0])
        {
            let field = &parts[1];
            self.constraints.push(Constraint::HasField {
                value: Type::Object(shape.clone()),
                field: field.clone(),
                field_type: shape.fields.get(field).cloned().unwrap_or(Type::Unknown),
            });
            if let Some(ty) = shape.fields.get(field) {
                return ty.clone();
            }
            self.diagnostics.push(
                self.diagnostic_for_expr(expr_id, format!("object is missing field `{field}`")),
            );
        }
        if parts.len() >= 2
            && self
                .name_bindings
                .get(&parts[0])
                .is_some_and(is_open_object_type)
        {
            return open_object_type();
        }
        self.diagnostics
            .push(self.diagnostic_for_expr(expr_id, format!("unknown path `{}`", parts.join("."))));
        self.expr_type_var(expr_id)
    }

    fn type_for_call(&self, function: &str) -> Type {
        let ty = self
            .builtins
            .type_for_call(function, &self.render_contracts);
        if !matches!(ty, Type::Unknown) {
            return ty;
        }
        if self.program.functions.iter().any(|name| name == function) {
            if function_returns_render(function, &self.program.ast.statements, self.program) {
                renderable_contract_type()
            } else {
                open_object_type()
            }
        } else {
            Type::Unknown
        }
    }

    fn type_for_call_expr(&mut self, expr_id: usize, function: &str) -> Type {
        let ty = self.type_for_call(function);
        if !matches!(ty, Type::Unknown) {
            return ty;
        }
        self.diagnostics.push(self.diagnostic_for_expr(
            expr_id,
            format!("unknown function or operator `{function}`"),
        ));
        self.expr_type_var(expr_id)
    }

    fn expr_type_var(&mut self, expr_id: usize) -> Type {
        let var = *self
            .expr_type_vars
            .entry(expr_id)
            .or_insert_with(|| self.vars.new_var());
        Type::Var(var)
    }

    fn unresolved_type_variable_count(&mut self) -> usize {
        let mut vars = BTreeSet::new();
        for entry in &self.expr_type_table.entries {
            collect_type_vars(&entry.flow_type.ty, &mut vars);
        }
        vars.into_iter()
            .map(|var| self.vars.root(var))
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn flow_mode_for_expr(&self, expr: &AstExpr) -> FlowMode {
        match &expr.kind {
            AstExprKind::Source => FlowMode::PresentOrAbsent,
            AstExprKind::Then { .. } => FlowMode::PresentOrAbsent,
            AstExprKind::Identifier(value) => {
                if let Some(mode) = self.flow_bindings.get(value) {
                    *mode
                } else if path_is_source_path(&self.source_paths, value) {
                    FlowMode::PresentOrAbsent
                } else {
                    FlowMode::Continuous
                }
            }
            AstExprKind::Path(parts) => {
                let path = parts.join(".");
                if let Some(mode) = self.flow_bindings.get(&path) {
                    *mode
                } else if path_is_source_path(&self.source_paths, &path) {
                    FlowMode::PresentOrAbsent
                } else {
                    FlowMode::Continuous
                }
            }
            _ => FlowMode::Continuous,
        }
    }

    fn expr_is_direct_data_list(&self, expr_id: usize) -> bool {
        expr_path(
            self.program.expressions.get(expr_id),
            &self.program.expressions,
        )
        .is_some_and(|path| {
            self.program
                .list_memories
                .iter()
                .any(|list| list.name == path || path.ends_with(&format!(".{}", list.name)))
        })
    }

    fn record_runtime_list_map(
        &mut self,
        map_expr_id: usize,
        list_expr_id: usize,
        args: &[AstCallArg],
    ) {
        if !self.runtime_list_map_exprs.insert(map_expr_id) {
            return;
        }
        let item_arg = args.iter().find(|arg| arg.name.is_none());
        let item_expr_id = item_arg.map(|arg| arg.value).unwrap_or(map_expr_id);
        let item_binding_name = item_arg
            .and_then(|arg| self.program.expressions.get(arg.value))
            .and_then(expr_single_name)
            .unwrap_or("item")
            .to_owned();
        let (template_function, template_args) = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("new"))
            .and_then(|arg| self.program.expressions.get(arg.value))
            .and_then(child_template)
            .map(|(function, args)| (Some(function), args))
            .unwrap_or((None, Vec::new()));
        self.list_map_bindings.push(ListMapBinding {
            map_expr_id,
            list_expr_id,
            input_list_type: Type::List(Box::new(open_object_type())),
            item_expr_id,
            item_binding_name,
            item_type: open_object_type(),
            result_type: Type::List(Box::new(open_object_type())),
            item_scope_id: Some(stable_scope_id_for_map(map_expr_id)),
            template_function,
            template_args,
            result_kind: ListMapResultKind::RuntimeValue,
        });
    }

    fn check_true_false_input(&mut self, expr: &AstExpr, input_flow: &FlowType) {
        if matches!(input_flow.ty, Type::Unknown)
            || is_open_object_type(&input_flow.ty)
            || type_accepts_true_false(&input_flow.ty)
        {
            return;
        }
        self.diagnostics.push(self.diagnostic_for_expr(
            expr.id,
            "`Bool/not` expects `True` or `False` tag".to_owned(),
        ));
    }

    fn check_hold_update_compatibility(&mut self, statement: &AstStatement) {
        let Some(expr_id) = statement.expr else {
            return;
        };
        let Some(AstExpr {
            kind: AstExprKind::Hold { initial, .. },
            ..
        }) = self.program.expressions.get(expr_id)
        else {
            return;
        };
        let initial_type = self.ensure_expr(*initial).ty;
        for update in hold_update_exprs(statement, &self.program.expressions) {
            let update_type = self.ensure_expr(update).ty;
            if concrete_type_conflict(&initial_type, &update_type) {
                self.constraints.push(Constraint::FlowCompatible {
                    actual: FlowType {
                        mode: FlowMode::TickPresent,
                        ty: update_type.clone(),
                    },
                    expected: FlowType {
                        mode: FlowMode::Continuous,
                        ty: initial_type.clone(),
                    },
                });
                self.diagnostics.push(self.diagnostic_for_expr(
                    update,
                    "`HOLD` update must match the held value type".to_owned(),
                ));
            }
        }
    }

    fn check_tagged_object_contract(
        &mut self,
        expr: &AstExpr,
        tag: &str,
        fields: &[AstRecordField],
        shape: &ObjectShape,
    ) {
        if tag != "Oklch" {
            return;
        }
        if !shape.fields.contains_key("lightness") {
            self.diagnostics.push(self.diagnostic_for_expr(
                expr.id,
                "tagged object `Oklch[...]` is missing field `lightness`".to_owned(),
            ));
        }
        for field in fields {
            if matches!(field.name.as_str(), "lightness" | "chroma" | "hue")
                && !matches!(
                    shape.fields.get(&field.name),
                    Some(Type::Number | Type::Unknown)
                )
            {
                self.diagnostics.push(self.diagnostic_for_expr(
                    field.value,
                    format!(
                        "tagged object `Oklch[...]` field `{}` must be a number",
                        field.name
                    ),
                ));
            }
        }
    }

    fn diagnostic_for_expr(&self, expr_id: usize, message: String) -> TypeDiagnostic {
        let expr = self.program.expressions.get(expr_id);
        TypeDiagnostic {
            severity: DiagnosticSeverity::Error,
            line: expr.map(|expr| expr.line).unwrap_or_default(),
            start: expr.map(|expr| expr.start).unwrap_or_default(),
            end: expr.map(|expr| expr.end).unwrap_or_default(),
            message,
        }
    }
}

#[derive(Clone, Debug)]
struct MappedChildren {
    map_expr_id: usize,
    list_expr_id: usize,
    item_expr_id: usize,
    item_binding_name: String,
    item_scope_id: usize,
    template_function: String,
    template_args: Vec<AstCallArg>,
}

fn mapped_children_for_statement(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<MappedChildren> {
    if let Some(expr_id) = statement.expr
        && let Some(mapped) = mapped_children_expr(expr_id, expressions, None)
    {
        return Some(mapped);
    }
    let mut previous_expr_id = statement.expr;
    for child in &statement.children {
        let Some(expr_id) = child.expr else {
            continue;
        };
        if let Some(mapped) = mapped_children_expr(expr_id, expressions, previous_expr_id) {
            return Some(mapped);
        }
        previous_expr_id = Some(expr_id);
    }
    None
}

fn render_slot_contains_malformed_list_map(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> bool {
    statement
        .expr
        .is_some_and(|expr_id| expr_contains_list_map(expr_id, expressions))
        || statement.children.iter().any(|child| {
            child
                .expr
                .is_some_and(|expr_id| expr_contains_list_map(expr_id, expressions))
        })
}

fn expr_contains_list_map(expr_id: usize, expressions: &[AstExpr]) -> bool {
    let Some(expr) = expressions.get(expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Pipe { input, op, args } => {
            op == "List/map"
                || expr_contains_list_map(*input, expressions)
                || args
                    .iter()
                    .any(|arg| expr_contains_list_map(arg.value, expressions))
        }
        AstExprKind::Call { args, .. } => args
            .iter()
            .any(|arg| expr_contains_list_map(arg.value, expressions)),
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            expr_contains_list_map(*initial, expressions)
        }
        AstExprKind::Then { input, output } => {
            expr_contains_list_map(*input, expressions)
                || output.is_some_and(|output| expr_contains_list_map(output, expressions))
        }
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_contains_list_map(*output, expressions),
        AstExprKind::Infix { left, right, .. } => {
            expr_contains_list_map(*left, expressions)
                || expr_contains_list_map(*right, expressions)
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields
            .iter()
            .any(|field| expr_contains_list_map(field.value, expressions)),
        _ => false,
    }
}

fn first_child_expr_id(statement: &AstStatement) -> Option<usize> {
    statement
        .children
        .iter()
        .find_map(|child| child.expr.or_else(|| first_child_expr_id(child)))
}

fn mapped_children_expr(
    expr_id: usize,
    expressions: &[AstExpr],
    fallback_input: Option<usize>,
) -> Option<MappedChildren> {
    let expr = expressions.get(expr_id)?;
    let AstExprKind::Pipe { input, op, args } = &expr.kind else {
        return None;
    };
    if op != "List/map" {
        return None;
    }
    let list_expr_id = pipe_input_expr(*input, expressions, fallback_input)?;
    let item_arg = args.iter().find(|arg| arg.name.is_none())?;
    let item_binding_name = expr_single_name(expressions.get(item_arg.value)?)?.to_owned();
    let new_expr = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("new"))
        .and_then(|arg| expressions.get(arg.value))?;
    let (template_function, template_args) = child_template(new_expr)?;
    Some(MappedChildren {
        map_expr_id: expr_id,
        list_expr_id,
        item_expr_id: item_arg.value,
        item_scope_id: stable_scope_id_for_map(expr_id),
        item_binding_name,
        template_function,
        template_args,
    })
}

fn pipe_input_expr(
    input: usize,
    expressions: &[AstExpr],
    fallback_input: Option<usize>,
) -> Option<usize> {
    match &expressions.get(input)?.kind {
        AstExprKind::Delimiter | AstExprKind::Unknown(_) => fallback_input,
        _ => Some(input),
    }
}

fn expr_path(expr: Option<&AstExpr>, expressions: &[AstExpr]) -> Option<String> {
    match &expr?.kind {
        AstExprKind::Identifier(value) => Some(value.clone()),
        AstExprKind::Path(parts) => Some(parts.join(".")),
        AstExprKind::Pipe { input, .. } => expr_path(expressions.get(*input), expressions),
        _ => None,
    }
}

fn expr_single_name(expr: &AstExpr) -> Option<&str> {
    match &expr.kind {
        AstExprKind::Identifier(value) => Some(value.as_str()),
        AstExprKind::Path(parts) if parts.len() == 1 => Some(parts[0].as_str()),
        _ => None,
    }
}

fn child_template(expr: &AstExpr) -> Option<(String, Vec<AstCallArg>)> {
    match &expr.kind {
        AstExprKind::Call { function, args } => Some((function.clone(), args.clone())),
        AstExprKind::Identifier(function) => Some((function.clone(), Vec::new())),
        _ => None,
    }
}

fn statement_field(statement: &AstStatement) -> Option<String> {
    match &statement.kind {
        AstStatementKind::Field { name } => Some(name.clone()),
        AstStatementKind::List {
            field: Some(name), ..
        } => Some(name.clone()),
        _ => None,
    }
}

fn statement_expr_ids(statement: &AstStatement) -> Vec<usize> {
    let mut expr_ids = Vec::new();
    collect_statement_expr_ids(statement, &mut expr_ids);
    expr_ids
}

fn collect_statement_expr_ids(statement: &AstStatement, expr_ids: &mut Vec<usize>) {
    if let Some(expr_id) = statement.expr {
        expr_ids.push(expr_id);
    }
    for child in &statement.children {
        collect_statement_expr_ids(child, expr_ids);
    }
}

fn document_root(program: &ParsedProgram) -> Option<&AstStatement> {
    program.ast.statements.iter().find(|statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "document"
        )
    })
}

#[derive(Clone, Debug)]
pub struct BuiltinSignatureRegistry {
    text_functions: BTreeSet<&'static str>,
    number_functions: BTreeSet<&'static str>,
    true_false_functions: BTreeSet<&'static str>,
}

impl Default for BuiltinSignatureRegistry {
    fn default() -> Self {
        Self {
            text_functions: ["Text/empty", "Text/trim", "Text/concat"]
                .into_iter()
                .collect(),
            number_functions: ["Number/add", "Number/subtract", "List/count"]
                .into_iter()
                .collect(),
            true_false_functions: ["Bool/not", "Bool/and"].into_iter().collect(),
        }
    }
}

impl BuiltinSignatureRegistry {
    fn type_for_call(&self, function: &str, render_contracts: &RenderContractRegistry) -> Type {
        if self.text_functions.contains(function) || function.starts_with("Text/") {
            Type::Text
        } else if self.number_functions.contains(function)
            || function.starts_with("Number/")
            || matches!(function, "List/sum")
        {
            Type::Number
        } else if self.true_false_functions.contains(function) || function.starts_with("Bool/") {
            true_false_type()
        } else if function.starts_with("List/") {
            Type::List(Box::new(open_object_type()))
        } else if function == "WHILE" {
            open_object_type()
        } else if function.starts_with("Error/") {
            Type::VariantSet(vec![Variant::Tagged {
                tag: "Error".to_owned(),
                fields: ObjectShape {
                    fields: BTreeMap::new(),
                    open: true,
                },
            }])
        } else if function.starts_with("Widget/") {
            open_object_type()
        } else if render_contracts.is_render_constructor(function) {
            renderable_contract_type()
        } else {
            Type::Unknown
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderContractRegistry {
    constructors: BTreeSet<&'static str>,
}

impl Default for RenderContractRegistry {
    fn default() -> Self {
        Self {
            constructors: [
                "Document/new",
                "Element/stripe",
                "Element/text",
                "Element/label",
                "Element/paragraph",
                "Element/link",
                "Element/button",
                "Element/checkbox",
                "Element/text_input",
            ]
            .into_iter()
            .collect(),
        }
    }
}

impl RenderContractRegistry {
    fn is_render_constructor(&self, function: &str) -> bool {
        self.constructors.contains(function) || function.starts_with("Element/")
    }

    fn slot_contract(&self, slot_name: &str) -> &'static str {
        match slot_name {
            "items" | "children" => "LIST<Element>",
            _ => "Element",
        }
    }
}

fn type_accepts_true_false(ty: &Type) -> bool {
    let Type::VariantSet(variants) = ty else {
        return false;
    };
    variants
        .iter()
        .all(|variant| matches!(variant, Variant::Tag(tag) if tag == "True" || tag == "False"))
}

fn concrete_type_conflict(left: &Type, right: &Type) -> bool {
    match (left, right) {
        (Type::Unknown, _) | (_, Type::Unknown) => false,
        (left, _) if is_open_object_type(left) => false,
        (_, right) if is_open_object_type(right) => false,
        (Type::Text, Type::Text) | (Type::Number, Type::Number) => false,
        (Type::VariantSet(_), Type::VariantSet(_)) => false,
        (Type::Object(left), Type::Object(right)) => {
            left.fields.iter().any(|(field, left_type)| {
                right
                    .fields
                    .get(field)
                    .is_some_and(|right_type| concrete_type_conflict(left_type, right_type))
            })
        }
        (Type::List(left), Type::List(right)) => concrete_type_conflict(left, right),
        (Type::Var(_), _) | (_, Type::Var(_)) => false,
        _ => true,
    }
}

fn hold_update_exprs(statement: &AstStatement, expressions: &[AstExpr]) -> Vec<usize> {
    let mut updates = Vec::new();
    collect_hold_update_exprs(statement, expressions, &mut updates);
    updates
}

fn collect_hold_update_exprs(
    statement: &AstStatement,
    expressions: &[AstExpr],
    updates: &mut Vec<usize>,
) {
    for child in &statement.children {
        if child.expr.is_some_and(|expr_id| {
            matches!(
                expressions.get(expr_id).map(|expr| &expr.kind),
                Some(AstExprKind::Latest)
            )
        }) {
            for update in &child.children {
                if let Some(expr_id) = update.expr {
                    updates.push(expr_id);
                }
            }
        }
    }
}

fn object_bindings(program: &ParsedProgram) -> BTreeMap<String, ObjectShape> {
    let mut bindings = BTreeMap::new();
    collect_object_bindings(
        &program.ast.statements,
        &program.expressions,
        &mut Vec::new(),
        &mut bindings,
    );
    bindings
}

fn collect_object_bindings(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    scope: &mut Vec<String>,
    bindings: &mut BTreeMap<String, ObjectShape>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Field { name } if name == "document" => continue,
            AstStatementKind::Field { name } => {
                let path = scoped_path(scope, name);
                if let Some(expr_id) = statement.expr
                    && let Some(shape) = object_shape_for_expr(expr_id, expressions)
                {
                    bindings.insert(path.clone(), shape);
                } else if !statement.children.is_empty() {
                    let shape = object_shape_for_statement(statement, expressions);
                    bindings.insert(name.clone(), shape.clone());
                    bindings.insert(path.clone(), shape);
                }
                scope.push(name.clone());
                collect_object_bindings(&statement.children, expressions, scope, bindings);
                scope.pop();
            }
            AstStatementKind::Function { .. } => {
                collect_object_bindings(&statement.children, expressions, scope, bindings);
            }
            _ => collect_object_bindings(&statement.children, expressions, scope, bindings),
        }
    }
}

fn object_shape_for_statement(statement: &AstStatement, expressions: &[AstExpr]) -> ObjectShape {
    ObjectShape {
        fields: statement
            .children
            .iter()
            .filter_map(|child| {
                let field = statement_field(child)?;
                let ty = child
                    .expr
                    .and_then(|expr_id| expressions.get(expr_id))
                    .map(|expr| simple_expr_type(expr, expressions))
                    .unwrap_or_else(open_object_type);
                Some((field, ty))
            })
            .collect(),
        open: true,
    }
}

fn object_shape_for_expr(expr_id: usize, expressions: &[AstExpr]) -> Option<ObjectShape> {
    let fields = match &expressions.get(expr_id)?.kind {
        AstExprKind::Object(fields) | AstExprKind::Record(fields) => fields,
        _ => return None,
    };
    Some(ObjectShape {
        fields: fields
            .iter()
            .map(|field| {
                (
                    field.name.clone(),
                    expressions
                        .get(field.value)
                        .map(|expr| simple_expr_type(expr, expressions))
                        .unwrap_or_else(open_object_type),
                )
            })
            .collect(),
        open: false,
    })
}

fn simple_expr_type(expr: &AstExpr, expressions: &[AstExpr]) -> Type {
    match &expr.kind {
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Type::Text,
        AstExprKind::Number(_) => Type::Number,
        AstExprKind::Bool(value) => Type::VariantSet(vec![Variant::Tag(if *value {
            "True".to_owned()
        } else {
            "False".to_owned()
        })]),
        AstExprKind::Tag(value) | AstExprKind::Enum(value) => {
            Type::VariantSet(vec![Variant::Tag(value.clone())])
        }
        AstExprKind::Object(fields) | AstExprKind::Record(fields) => Type::Object(ObjectShape {
            fields: fields
                .iter()
                .map(|field| {
                    (
                        field.name.clone(),
                        expressions
                            .get(field.value)
                            .map(|expr| simple_expr_type(expr, expressions))
                            .unwrap_or_else(open_object_type),
                    )
                })
                .collect(),
            open: false,
        }),
        AstExprKind::ListLiteral { .. } => Type::List(Box::new(open_object_type())),
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if function == "List/chunk" =>
        {
            Type::List(Box::new(Type::Object(ObjectShape {
                fields: [
                    ("row_number".to_owned(), Type::Text),
                    ("cells".to_owned(), Type::List(Box::new(open_object_type()))),
                ]
                .into_iter()
                .collect(),
                open: true,
            })))
        }
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if function == "Bool/not" || function == "Bool/and" =>
        {
            true_false_type()
        }
        AstExprKind::Infix { op, .. } if matches!(op.as_str(), "==" | ">" | "<" | ">=" | "<=") => {
            true_false_type()
        }
        AstExprKind::Infix { .. } => Type::Number,
        AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => open_object_type(),
        AstExprKind::Call { function, .. }
            if function.starts_with("Element/") || function == "Document/new" =>
        {
            renderable_contract_type()
        }
        _ => open_object_type(),
    }
}

fn name_bindings(program: &ParsedProgram) -> BTreeMap<String, Type> {
    let mut bindings = BTreeMap::new();
    collect_name_bindings(
        &program.ast.statements,
        &program.expressions,
        &mut Vec::new(),
        &mut bindings,
    );
    collect_row_scope_bindings(program, &mut bindings);
    bindings
}

fn flow_bindings(program: &ParsedProgram) -> BTreeMap<String, FlowMode> {
    let mut bindings = BTreeMap::new();
    collect_flow_bindings(
        &program.ast.statements,
        &program.expressions,
        &mut Vec::new(),
        &mut bindings,
    );
    bindings
}

fn collect_flow_bindings(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    scope: &mut Vec<String>,
    bindings: &mut BTreeMap<String, FlowMode>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Field { name } if name == "document" => continue,
            AstStatementKind::Field { name } => {
                if let Some(expr_id) = statement.expr
                    && let Some(expr) = expressions.get(expr_id)
                {
                    let mode = simple_flow_mode(expr, expressions);
                    bindings.insert(name.clone(), mode);
                    bindings.insert(scoped_path(scope, name), mode);
                }
                scope.push(name.clone());
                collect_flow_bindings(&statement.children, expressions, scope, bindings);
                scope.pop();
            }
            AstStatementKind::Hold { name: Some(name) } => {
                bindings.insert(name.clone(), FlowMode::Continuous);
                collect_flow_bindings(&statement.children, expressions, scope, bindings);
            }
            _ => collect_flow_bindings(&statement.children, expressions, scope, bindings),
        }
    }
}

fn simple_flow_mode(expr: &AstExpr, expressions: &[AstExpr]) -> FlowMode {
    match &expr.kind {
        AstExprKind::Source | AstExprKind::Then { .. } => FlowMode::PresentOrAbsent,
        AstExprKind::When { .. } => FlowMode::PresentOrAbsent,
        AstExprKind::Pipe { input, .. } => expressions
            .get(*input)
            .map(|expr| simple_flow_mode(expr, expressions))
            .unwrap_or(FlowMode::Continuous),
        _ => FlowMode::Continuous,
    }
}

fn collect_name_bindings(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    scope: &mut Vec<String>,
    bindings: &mut BTreeMap<String, Type>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Field { name } if name == "document" => continue,
            AstStatementKind::Field { name } => {
                let path = scoped_path(scope, name);
                let ty = statement
                    .expr
                    .and_then(|expr_id| expressions.get(expr_id))
                    .map(|expr| simple_expr_type(expr, expressions))
                    .unwrap_or_else(|| {
                        Type::Object(object_shape_for_statement(statement, expressions))
                    });
                bindings.insert(name.clone(), ty.clone());
                bindings.insert(path, ty);
                scope.push(name.clone());
                collect_name_bindings(&statement.children, expressions, scope, bindings);
                scope.pop();
            }
            AstStatementKind::Hold { name: Some(name) } => {
                if let Some(expr_id) = statement.expr {
                    let ty = expressions
                        .get(expr_id)
                        .map(|expr| simple_expr_type(expr, expressions))
                        .unwrap_or_else(open_object_type);
                    bindings.insert(name.clone(), ty);
                }
                collect_name_bindings(&statement.children, expressions, scope, bindings);
            }
            AstStatementKind::List {
                field: Some(name), ..
            } => {
                let ty = Type::List(Box::new(open_object_type()));
                bindings.insert(name.clone(), ty.clone());
                bindings.insert(scoped_path(scope, name), ty);
                collect_name_bindings(&statement.children, expressions, scope, bindings);
            }
            AstStatementKind::Source {
                field: Some(name), ..
            } => {
                let ty = open_object_type();
                bindings.insert(name.clone(), ty.clone());
                bindings.insert(scoped_path(scope, name), ty);
                collect_name_bindings(&statement.children, expressions, scope, bindings);
            }
            AstStatementKind::Function { args, .. } => {
                for arg in args {
                    bindings.insert(arg.clone(), open_object_type());
                }
                collect_name_bindings(&statement.children, expressions, scope, bindings);
            }
            _ => collect_name_bindings(&statement.children, expressions, scope, bindings),
        }
    }
}

fn collect_row_scope_bindings(program: &ParsedProgram, bindings: &mut BTreeMap<String, Type>) {
    bindings.insert("if".to_owned(), open_object_type());
    bindings.insert("when".to_owned(), open_object_type());
    for row_scope in &program.row_scope_functions {
        bindings
            .entry(row_scope.row_scope.clone())
            .or_insert_with(open_object_type);
        if let Some(shape) = list_item_shape(program, &row_scope.list)
            .or_else(|| function_result_shape(program, &row_scope.function))
        {
            for (field, ty) in &shape.fields {
                bindings.insert(field.clone(), ty.clone());
                bindings.insert(format!("{}.{}", row_scope.row_scope, field), ty.clone());
            }
            bindings.insert(row_scope.row_scope.clone(), Type::Object(shape));
        }
    }
    for expr in &program.expressions {
        if let AstExprKind::Pipe { op, args, .. } = &expr.kind
            && op == "List/map"
        {
            for arg in args.iter().filter(|arg| arg.name.is_none()) {
                if let Some(name) = program
                    .expressions
                    .get(arg.value)
                    .and_then(expr_single_name)
                {
                    bindings.insert(name.to_owned(), open_object_type());
                }
            }
        }
        if let AstExprKind::MatchArm { pattern, .. } = &expr.kind {
            for name in pattern_variable_names(pattern) {
                bindings.insert(name, Type::Text);
            }
        }
        if let AstExprKind::Call { function, args }
        | AstExprKind::Pipe {
            op: function, args, ..
        } = &expr.kind
            && function == "List/chunk"
        {
            for arg in args
                .iter()
                .filter(|arg| matches!(arg.name.as_deref(), Some("label" | "items")))
            {
                if let Some(name) = program
                    .expressions
                    .get(arg.value)
                    .and_then(expr_single_name)
                {
                    let ty = if arg.name.as_deref() == Some("items") {
                        Type::List(Box::new(open_object_type()))
                    } else {
                        Type::Text
                    };
                    bindings.insert(name.to_owned(), ty);
                }
            }
        }
        if let AstExprKind::Then {
            output: Some(output),
            ..
        } = &expr.kind
            && let Some(name) = program.expressions.get(*output).and_then(expr_single_name)
        {
            bindings.insert(name.to_owned(), open_object_type());
        }
    }
}

fn pattern_variable_names(pattern: &[String]) -> Vec<String> {
    pattern
        .iter()
        .filter(|part| {
            is_binding_name(part)
                && !matches!(part.as_str(), "__" | "TEXT" | "True" | "False")
                && part
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_lowercase())
        })
        .cloned()
        .collect()
}

fn is_binding_name(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn function_result_shape(program: &ParsedProgram, function: &str) -> Option<ObjectShape> {
    let function = find_function_statement(&program.ast.statements, function)?;
    let mut fields = BTreeMap::new();
    collect_statement_shape_fields(&function.children, &program.expressions, &mut fields);
    (!fields.is_empty()).then_some(ObjectShape { fields, open: true })
}

fn find_function_statement<'a>(
    statements: &'a [AstStatement],
    function: &str,
) -> Option<&'a AstStatement> {
    for statement in statements {
        if matches!(
            &statement.kind,
            AstStatementKind::Function { name, .. } if name == function
        ) {
            return Some(statement);
        }
        if let Some(found) = find_function_statement(&statement.children, function) {
            return Some(found);
        }
    }
    None
}

fn collect_statement_shape_fields(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    fields: &mut BTreeMap<String, Type>,
) {
    for statement in statements {
        if let Some(field) = statement_field(statement)
            && field != "sources"
        {
            let ty = statement
                .expr
                .and_then(|expr_id| expressions.get(expr_id))
                .map(|expr| simple_expr_type(expr, expressions))
                .unwrap_or_else(open_object_type);
            fields
                .entry(field)
                .and_modify(|existing| *existing = widen_structural_type(existing, &ty))
                .or_insert(ty);
        }
        collect_statement_shape_fields(&statement.children, expressions, fields);
    }
}

fn list_item_shape(program: &ParsedProgram, list_name: &str) -> Option<ObjectShape> {
    if let Some(shape) = list_item_shape_from_field(program, list_name) {
        return Some(shape);
    }
    let list = find_list_statement(&program.ast.statements, list_name)?;
    let mut fields = BTreeMap::new();
    for child in &list.children {
        let Some(expr_id) = child.expr else {
            continue;
        };
        let Some(AstExpr {
            kind: AstExprKind::Object(object_fields) | AstExprKind::Record(object_fields),
            ..
        }) = program.expressions.get(expr_id)
        else {
            continue;
        };
        for field in object_fields {
            let ty = program
                .expressions
                .get(field.value)
                .map(|expr| simple_expr_type(expr, &program.expressions))
                .unwrap_or_else(open_object_type);
            fields
                .entry(field.name.clone())
                .and_modify(|existing| *existing = widen_structural_type(existing, &ty))
                .or_insert(ty);
        }
    }
    (!fields.is_empty()).then_some(ObjectShape { fields, open: true })
}

fn list_item_shape_from_field(program: &ParsedProgram, list_name: &str) -> Option<ObjectShape> {
    let field_name = list_name.rsplit('.').next().unwrap_or(list_name);
    let statement = find_field_statement(&program.ast.statements, field_name)?;
    let expr_id = statement.expr?;
    match simple_expr_type(program.expressions.get(expr_id)?, &program.expressions) {
        Type::List(item) => match *item {
            Type::Object(shape) => Some(shape),
            _ => None,
        },
        _ => None,
    }
}

fn find_field_statement<'a>(
    statements: &'a [AstStatement],
    field_name: &str,
) -> Option<&'a AstStatement> {
    for statement in statements {
        if matches!(&statement.kind, AstStatementKind::Field { name } if name == field_name) {
            return Some(statement);
        }
        if let Some(found) = find_field_statement(&statement.children, field_name) {
            return Some(found);
        }
    }
    None
}

fn find_list_statement<'a>(
    statements: &'a [AstStatement],
    list_name: &str,
) -> Option<&'a AstStatement> {
    for statement in statements {
        if matches!(
            &statement.kind,
            AstStatementKind::List {
                field: Some(field),
                ..
            } if field == list_name
        ) {
            return Some(statement);
        }
        if let Some(found) = find_list_statement(&statement.children, list_name) {
            return Some(found);
        }
    }
    None
}

fn widen_structural_type(left: &Type, right: &Type) -> Type {
    match (left, right) {
        (Type::VariantSet(left), Type::VariantSet(right)) => {
            let mut variants = left.clone();
            for variant in right {
                if !variants.contains(variant) {
                    variants.push(variant.clone());
                }
            }
            Type::VariantSet(variants)
        }
        (Type::Text, Type::Text) => Type::Text,
        (Type::Number, Type::Number) => Type::Number,
        (Type::List(left), Type::List(right)) => {
            Type::List(Box::new(widen_structural_type(left, right)))
        }
        (Type::Object(left), Type::Object(right)) => {
            let mut fields = left.fields.clone();
            for (field, ty) in &right.fields {
                fields
                    .entry(field.clone())
                    .and_modify(|existing| *existing = widen_structural_type(existing, ty))
                    .or_insert_with(|| ty.clone());
            }
            Type::Object(ObjectShape {
                fields,
                open: left.open || right.open,
            })
        }
        _ => open_object_type(),
    }
}

fn source_payload_type(parts: &[String]) -> Type {
    match parts.last().map(String::as_str) {
        Some("text" | "key") => Type::Text,
        _ => open_object_type(),
    }
}

fn path_is_source_path(source_paths: &BTreeSet<String>, path: &str) -> bool {
    source_paths.iter().any(|source_path| {
        let relative = source_path
            .strip_prefix("store.")
            .unwrap_or(source_path.as_str());
        source_path == path
            || source_path.ends_with(&format!(".{path}"))
            || path.starts_with(&format!("{source_path}."))
            || relative == path
            || relative.ends_with(&format!(".{path}"))
            || path.starts_with(&format!("{relative}."))
    })
}

fn function_returns_render(
    function: &str,
    statements: &[AstStatement],
    program: &ParsedProgram,
) -> bool {
    statements.iter().any(|statement| match &statement.kind {
        AstStatementKind::Function { name, .. } if name == function => statement
            .children
            .iter()
            .any(|child| statement_returns_render(child, program)),
        _ => function_returns_render(function, &statement.children, program),
    })
}

fn statement_returns_render(statement: &AstStatement, program: &ParsedProgram) -> bool {
    statement
        .expr
        .and_then(|expr_id| program.expressions.get(expr_id))
        .is_some_and(|expr| expr_returns_render(expr, program))
        || statement
            .children
            .iter()
            .any(|child| statement_returns_render(child, program))
}

fn expr_returns_render(expr: &AstExpr, program: &ParsedProgram) -> bool {
    match &expr.kind {
        AstExprKind::Call { function, .. } => {
            function == "Document/new"
                || function.starts_with("Element/")
                || function_returns_render(function, &program.ast.statements, program)
        }
        AstExprKind::Pipe { input, op, args } => {
            op.starts_with("Element/")
                || expr_returns_render(&program.expressions[*input], program)
                || args
                    .iter()
                    .any(|arg| expr_returns_render(&program.expressions[arg.value], program))
        }
        _ => false,
    }
}

fn scoped_path(scope: &[String], name: &str) -> String {
    if scope.is_empty() {
        name.to_owned()
    } else {
        format!("{}.{name}", scope.join("."))
    }
}

fn true_false_type() -> Type {
    Type::VariantSet(vec![
        Variant::Tag("True".to_owned()),
        Variant::Tag("False".to_owned()),
    ])
}

fn renderable_contract_type() -> Type {
    open_object_type()
}

fn open_object_type() -> Type {
    Type::Object(ObjectShape {
        fields: BTreeMap::new(),
        open: true,
    })
}

fn is_open_object_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Object(ObjectShape {
            fields,
            open: true
        }) if fields.is_empty()
    )
}

fn collect_type_vars(ty: &Type, vars: &mut BTreeSet<TypeVar>) {
    match ty {
        Type::Var(var) => {
            vars.insert(*var);
        }
        Type::List(item) => collect_type_vars(item, vars),
        Type::Function { args, result } => {
            for arg in args {
                collect_type_vars(arg, vars);
            }
            collect_type_vars(&result.ty, vars);
        }
        Type::Object(shape) => {
            for field in shape.fields.values() {
                collect_type_vars(field, vars);
            }
        }
        Type::VariantSet(variants) => {
            for variant in variants {
                if let Variant::Tagged { fields, .. } = variant {
                    for field in fields.fields.values() {
                        collect_type_vars(field, vars);
                    }
                }
            }
        }
        Type::Text | Type::Number | Type::Unknown => {}
    }
}

fn builtin_signature_coverage(program: &ParsedProgram) -> Vec<String> {
    let mut names = program.operators.clone();
    names.extend(program.functions.iter().cloned());
    names.sort();
    names.dedup();
    names
}

fn stable_scope_id_for_map(expr_id: usize) -> usize {
    expr_id
}

#[allow(dead_code)]
fn object_shape(fields: &[AstRecordField]) -> ObjectShape {
    ObjectShape {
        fields: fields
            .iter()
            .map(|field| (field.name.clone(), open_object_type()))
            .collect(),
        open: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_document_typecheck_covers_document_expressions() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
items: LIST[1] {}
items |> List/map(item, new: item)
document:
    root: Element/label(label: TEXT { Hello })
"#;
        let parsed = boon_parser::parse_source("document-coverage.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.full_document_typecheck_coverage);
        assert_eq!(report.checked_expression_count, report.expression_count);
    }

    #[test]
    fn true_and_false_are_variant_tags_not_user_bool() {
        let parsed = boon_parser::parse_source(
            "truefalse.bn",
            "source: SOURCE\nflag: True |> HOLD flag { LATEST {} }\ndocument: []\n",
        )
        .unwrap();
        let report = check(&parsed);
        assert!(report.expr_type_table.entries.iter().any(|entry| {
            matches!(
                &entry.flow_type.ty,
                Type::VariantSet(variants)
                    if variants == &vec![Variant::Tag("True".to_owned())]
            )
        }));
    }

    #[test]
    fn document_items_list_map_becomes_render_slot_binding() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
visible_todos: LIST[4] {}
FUNCTION todo_row(todo) {
    Element/label(label: todo.title)
}
document:
    root:
        Element/stripe(
            direction: Column
            items:
                visible_todos
                |> List/map(todo, new: todo_row(todo: todo))
        )
"#;
        let parsed = boon_parser::parse_source("render-slot-list-map.bn", source).unwrap();
        let report = check(&parsed);
        assert_eq!(report.render_slot_count, 1);
        assert_eq!(report.list_map_binding_count_render_slot_materialization, 1);
        let slot = &report.render_slot_table.slots[0];
        assert_eq!(slot.slot_name, "items");
        assert_eq!(slot.expected_contract, "LIST<Element>");
        assert_eq!(slot.template_function.as_deref(), Some("todo_row"));
        let binding = &report.list_map_bindings[slot.optional_list_map_binding_id.unwrap()];
        assert_eq!(binding.item_binding_name, "todo");
        assert_eq!(binding.template_function.as_deref(), Some("todo_row"));
    }

    #[test]
    fn list_map_outside_render_slots_stays_ordinary_expression() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
todos: LIST[4] {}
rows:
    todos
    |> List/map(todo, new: todo)
document: []
"#;
        let parsed = boon_parser::parse_source("ordinary-list-map.bn", source).unwrap();
        let report = check(&parsed);
        assert_eq!(report.render_slot_count, 0);
        assert_eq!(report.list_map_binding_count_runtime_value, 1);
        assert_eq!(report.list_map_binding_count_render_slot_materialization, 0);
        assert!(
            report
                .expr_type_table
                .entries
                .iter()
                .any(|entry| { matches!(entry.flow_type.ty, Type::List(_)) })
        );
    }

    #[test]
    fn rejects_direct_data_list_passed_to_items() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
todos: LIST[4] {}
rows:
    todos
    |> List/map(todo, new: todo)
document:
    root:
        Element/stripe(
            items: todos
        )
"#;
        let parsed = boon_parser::parse_source("bad-items-data-list.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("expected a list of renderable values for `items:`")
        }));
    }

    #[test]
    fn rejects_malformed_render_slot_list_map() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
todos: LIST[4] {}
FUNCTION todo_row(todo) {
    Element/label(label: todo.title)
}
document:
    root:
        Element/stripe(
            items:
                todos
                |> List/map(todo)
        )
"#;
        let parsed = boon_parser::parse_source("bad-items-list-map.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("expected `List/map(item, new: row(item: item))`")
        }));
    }

    #[test]
    fn rejects_bool_not_on_non_true_false_value() {
        let source = r#"
source: SOURCE
value: 0 |> HOLD value { LATEST {} }
bad: 1 |> Bool/not()
items: LIST[1] {}
items |> List/map(item, new: item)
document: []
"#;
        let parsed = boon_parser::parse_source("bad-bool-not.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("`Bool/not` expects `True` or `False` tag")
        }));
    }

    #[test]
    fn rejects_then_on_continuous_value() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
bad: True |> THEN { False }
items: LIST[1] {}
items |> List/map(item, new: item)
document: []
"#;
        let parsed = boon_parser::parse_source("bad-then-continuous.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("`THEN` requires a tick-present-or-absent value")
        }));
    }

    #[test]
    fn rejects_wrong_oklch_field_type() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
items: LIST[1] {}
items |> List/map(item, new: item)
style: [color: Oklch[lightness:Bright]]
document: []
"#;
        let parsed = boon_parser::parse_source("bad-oklch.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("tagged object `Oklch[...]` field `lightness` must be a number")
        }));
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic
                    .message
                    .contains("tagged object `Oklch[...]` field `lightness` must be a number")
            })
            .expect("expected Oklch diagnostic");
        assert_eq!(
            &parsed.source[diagnostic.start..diagnostic.end],
            "Bright",
            "diagnostic should point at the bad field value, not the whole line"
        );
    }

    #[test]
    fn rejects_missing_field_on_known_structural_object() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
todo: [title: TEXT { Read }]
bad: todo.completed
items: LIST[1] {}
items |> List/map(item, new: item)
document: []
"#;
        let parsed = boon_parser::parse_source("missing-field.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("object is missing field `completed`")
        }));
        assert!(report.constraints.iter().any(|constraint| {
            matches!(constraint, Constraint::HasField { field, .. } if field == "completed")
        }));
    }

    #[test]
    fn rejects_hold_update_with_incompatible_concrete_type() {
        let source = r#"
source: SOURCE
value:
    0 |> HOLD value {
        LATEST {
            source |> THEN { TEXT { bad } }
        }
    }
items: LIST[1] {}
items |> List/map(item, new: item)
document: []
"#;
        let parsed = boon_parser::parse_source("bad-hold-update.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("`HOLD` update must match the held value type")
        }));
    }

    #[test]
    fn accepts_then_on_source_flow() {
        let source = r#"
source: SOURCE
value:
    0 |> HOLD value {
        LATEST {
            source |> THEN { 1 }
        }
    }
items: LIST[1] {}
items |> List/map(item, new: item)
document: []
"#;
        let parsed = boon_parser::parse_source("source-then-flow.bn", source).unwrap();
        let report = check(&parsed);
        assert!(
            !report.has_errors(),
            "unexpected diagnostics: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn rejects_unknown_identifier_path_and_function_with_type_vars() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
bad_identifier: missing
bad_path: missing.value
bad_function: Mystery/do()
items: LIST[1] {}
items |> List/map(item, new: item)
document: []
"#;
        let parsed = boon_parser::parse_source("unknown-symbols.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.unresolved_type_variable_count >= 3);
        assert!(report.dynamic_fallback_count >= report.unresolved_type_variable_count);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("unknown identifier `missing`"))
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("unknown path `missing.value`"))
        );
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("unknown function or operator `Mystery/do`")
        }));
        assert!(
            report
                .expr_type_table
                .entries
                .iter()
                .any(|entry| { matches!(entry.flow_type.ty, Type::Var(_)) })
        );
    }

    #[test]
    fn bundled_examples_have_complete_typecheck_reports() {
        let counter = boon_parser::parse_source(
            "examples/counter.bn",
            include_str!("../../../examples/counter.bn"),
        )
        .unwrap();
        let todomvc = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let cells = boon_parser::parse_project(
            "examples/cells.bn",
            [
                (
                    "examples/cells/defaults.bn".to_owned(),
                    include_str!("../../../examples/cells/defaults.bn").to_owned(),
                ),
                (
                    "examples/cells/formula.bn".to_owned(),
                    include_str!("../../../examples/cells/formula.bn").to_owned(),
                ),
                (
                    "examples/cells/cell.bn".to_owned(),
                    include_str!("../../../examples/cells/cell.bn").to_owned(),
                ),
                (
                    "examples/cells/model.bn".to_owned(),
                    include_str!("../../../examples/cells/model.bn").to_owned(),
                ),
                (
                    "examples/cells/columns.bn".to_owned(),
                    include_str!("../../../examples/cells/columns.bn").to_owned(),
                ),
                (
                    "examples/cells/store.bn".to_owned(),
                    include_str!("../../../examples/cells/store.bn").to_owned(),
                ),
                (
                    "examples/cells/view.bn".to_owned(),
                    include_str!("../../../examples/cells/view.bn").to_owned(),
                ),
                (
                    "examples/cells.bn".to_owned(),
                    include_str!("../../../examples/cells.bn").to_owned(),
                ),
            ],
        )
        .unwrap();

        for parsed in [&counter, &todomvc, &cells] {
            let report = check(parsed);
            assert!(
                !report.has_errors(),
                "{} diagnostics: {:?}",
                parsed.path,
                report.diagnostics
            );
            assert_eq!(report.dynamic_fallback_count, 0, "{}", parsed.path);
            assert_eq!(report.unresolved_type_variable_count, 0, "{}", parsed.path);
            assert_eq!(report.render_slot_failure_count, 0, "{}", parsed.path);
            assert!(report.full_document_typecheck_coverage, "{}", parsed.path);
        }
    }
}
