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
    Skip,
    VariantSet(Vec<Variant>),
    Object(ObjectShape),
    RenderContract,
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
pub struct SourcePayloadShapeEntry {
    pub source_path: String,
    pub payload_type: Type,
    pub fields: Vec<SourcePayloadShapeField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePayloadShapeField {
    pub name: String,
    pub ty: Type,
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
    pub source_payload_shape_table: Vec<SourcePayloadShapeEntry>,
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
        self.check_recursive_functions();
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
        let source_payload_shape_table = source_payload_shape_table(self.program);
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
            source_payload_shape_table,
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
        let next_in_document = in_document
            || statement_field(statement).as_deref() == Some("document")
            || self.statement_enters_render_context(statement);
        if let Some(expr_id) = statement.expr {
            let flow = self.ensure_expr(expr_id);
            if !next_in_document && type_contains_no_element(&flow.ty) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr_id,
                    "`NoElement` can only be used as a render value".to_owned(),
                ));
            }
        }
        if statement_field(statement).as_deref() == Some("style") {
            self.check_style_statement(statement);
        }
        self.check_hold_update_compatibility(statement);
        self.check_latest_branch_compatibility(statement);
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
        if next_in_document
            && matches!(
                statement_field(statement).as_deref(),
                Some("items" | "children")
            )
        {
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
        if let Some(static_list_type) = self.render_slot_static_list_type(statement) {
            actual_type = static_list_type;
        }
        let mut materialization_policy = MaterializationPolicy::StaticChildren;

        let mut diagnostics = Vec::new();

        if let Some(mapped) = mapped_children_for_statement(statement, &self.program.expressions) {
            value_expr_id = Some(mapped.map_expr_id);
            actual_type = self.ensure_expr(mapped.map_expr_id).ty;
            if render_slot_accepts_type(&slot_name, &actual_type) {
                self.runtime_list_map_exprs.remove(&mapped.map_expr_id);
                self.list_map_bindings
                    .retain(|binding| binding.map_expr_id != mapped.map_expr_id);
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
            } else {
                let message = if type_contains_skip(&actual_type) {
                    "`SKIP` cannot be used as a render value".to_owned()
                } else {
                    format!("expected a list of renderable values for `{slot_name}:`")
                };
                diagnostics.push(self.diagnostic_for_expr(mapped.map_expr_id, message));
            }
        } else if let Some(expr_id) = statement.expr
            && !render_slot_accepts_type(&slot_name, &actual_type)
        {
            let message = if type_contains_skip(&actual_type) {
                "`SKIP` cannot be used as a render value".to_owned()
            } else if matches!(actual_type, Type::List(_)) || self.expr_is_direct_data_list(expr_id)
            {
                format!("expected a list of renderable values for `{slot_name}:`")
            } else {
                format!("expected renderable values for `{slot_name}:`")
            };
            diagnostics.push(self.diagnostic_for_expr(expr_id, message));
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
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) if tag == "SKIP" => Type::Skip,
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
                if self.render_contracts.is_render_constructor(function) {
                    self.check_style_args(args);
                }
                if function == "Bool/not" {
                    let input_flow = args
                        .first()
                        .map(|arg| self.ensure_expr(arg.value))
                        .unwrap_or(FlowType {
                            mode: FlowMode::Continuous,
                            ty: Type::Unknown,
                        });
                    self.check_true_false_input(expr, function, &input_flow);
                    true_false_type()
                } else if function == "Bool/and" {
                    for arg in args {
                        let arg_flow = self.ensure_expr(arg.value);
                        self.check_true_false_input(expr, function, &arg_flow);
                    }
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
                if self.render_contracts.is_render_constructor(op) {
                    self.check_style_args(args);
                }
                if op == "List/map" {
                    self.record_runtime_list_map(expr.id, *input, args);
                    Type::List(Box::new(self.list_map_result_item_type(args)))
                } else if op == "Bool/not" {
                    self.check_true_false_input(expr, op, &input_flow);
                    true_false_type()
                } else if op == "Bool/and" {
                    self.check_true_false_input(expr, op, &input_flow);
                    for arg in args {
                        let arg_flow = self.ensure_expr(arg.value);
                        self.check_true_false_input(expr, op, &arg_flow);
                    }
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
                .unwrap_or_else(|| Type::Skip),
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
        self.user_function_return_type(function, &mut BTreeSet::new())
            .unwrap_or_else(|| {
                if self.program.functions.iter().any(|name| name == function) {
                    open_object_type()
                } else {
                    Type::Unknown
                }
            })
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

    fn list_map_result_item_type(&self, args: &[AstCallArg]) -> Type {
        let Some(new_expr) = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("new"))
            .and_then(|arg| self.program.expressions.get(arg.value))
        else {
            return open_object_type();
        };
        self.static_expr_type(new_expr, &mut BTreeSet::new())
            .unwrap_or_else(open_object_type)
    }

    fn static_expr_type(
        &self,
        expr: &AstExpr,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        match &expr.kind {
            AstExprKind::Call { function, args } => {
                if self.render_contracts.is_render_constructor(function) {
                    return Some(renderable_contract_type());
                }
                self.user_function_return_type(function, active_functions)
                    .or_else(|| {
                        Some(
                            self.builtins
                                .type_for_call(function, &self.render_contracts),
                        )
                    })
                    .filter(|ty| !matches!(ty, Type::Unknown))
                    .or_else(|| {
                        args.iter().find_map(|arg| {
                            self.program
                                .expressions
                                .get(arg.value)
                                .and_then(|arg_expr| {
                                    self.static_expr_type(arg_expr, active_functions)
                                })
                        })
                    })
            }
            AstExprKind::Pipe { input, op, args } => {
                if op == "List/map" {
                    Some(Type::List(Box::new(
                        self.static_list_map_result_item_type(args, active_functions),
                    )))
                } else if self.render_contracts.is_render_constructor(op) {
                    Some(renderable_contract_type())
                } else {
                    self.user_function_return_type(op, active_functions)
                        .or_else(|| Some(self.builtins.type_for_call(op, &self.render_contracts)))
                        .filter(|ty| !matches!(ty, Type::Unknown))
                        .or_else(|| {
                            self.program.expressions.get(*input).and_then(|input_expr| {
                                self.static_expr_type(input_expr, active_functions)
                            })
                        })
                }
            }
            AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
                Some(Type::Object(ObjectShape {
                    fields: fields
                        .iter()
                        .map(|field| {
                            (
                                field.name.clone(),
                                self.program
                                    .expressions
                                    .get(field.value)
                                    .and_then(|field_expr| {
                                        self.static_expr_type(field_expr, active_functions)
                                    })
                                    .unwrap_or_else(open_object_type),
                            )
                        })
                        .collect(),
                    open: false,
                }))
            }
            AstExprKind::TaggedObject { tag, fields } => {
                Some(Type::VariantSet(vec![Variant::Tagged {
                    tag: tag.clone(),
                    fields: ObjectShape {
                        fields: fields
                            .iter()
                            .map(|field| {
                                (
                                    field.name.clone(),
                                    self.program
                                        .expressions
                                        .get(field.value)
                                        .and_then(|field_expr| {
                                            self.static_expr_type(field_expr, active_functions)
                                        })
                                        .unwrap_or_else(open_object_type),
                                )
                            })
                            .collect(),
                        open: false,
                    },
                }]))
            }
            AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Some(Type::Text),
            AstExprKind::Number(_) => Some(Type::Number),
            AstExprKind::Bool(_) => Some(true_false_type()),
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) if tag == "SKIP" => Some(Type::Skip),
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) => {
                Some(Type::VariantSet(vec![Variant::Tag(tag.clone())]))
            }
            AstExprKind::ListLiteral { .. } => Some(Type::List(Box::new(open_object_type()))),
            AstExprKind::Infix { op, .. }
                if matches!(op.as_str(), "==" | ">" | "<" | ">=" | "<=") =>
            {
                Some(true_false_type())
            }
            AstExprKind::Infix { .. } => Some(Type::Number),
            _ => None,
        }
    }

    fn static_list_map_result_item_type(
        &self,
        args: &[AstCallArg],
        active_functions: &mut BTreeSet<String>,
    ) -> Type {
        args.iter()
            .find(|arg| arg.name.as_deref() == Some("new"))
            .and_then(|arg| self.program.expressions.get(arg.value))
            .and_then(|expr| self.static_expr_type(expr, active_functions))
            .unwrap_or_else(open_object_type)
    }

    fn user_function_return_type(
        &self,
        function: &str,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        if !active_functions.insert(function.to_owned()) {
            return None;
        }
        let result =
            find_function_statement(&self.program.ast.statements, function).and_then(|statement| {
                statement.children.iter().find_map(|child| {
                    child.expr.and_then(|expr_id| {
                        self.program
                            .expressions
                            .get(expr_id)
                            .and_then(|expr| self.static_expr_type(expr, active_functions))
                    })
                })
            });
        active_functions.remove(function);
        result
    }

    fn statement_enters_render_context(&self, statement: &AstStatement) -> bool {
        let AstStatementKind::Function { name, .. } = &statement.kind else {
            return false;
        };
        self.user_function_return_type(name, &mut BTreeSet::new())
            .is_some_and(|ty| type_contains_renderable(&ty))
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
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) if tag == "SKIP" => FlowMode::Absent,
            AstExprKind::MatchArm { output, .. }
                if output.is_none_or(|output| {
                    self.program
                        .expressions
                        .get(output)
                        .is_some_and(expr_is_skip)
                }) =>
            {
                FlowMode::Absent
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

    fn render_slot_static_list_type(&self, statement: &AstStatement) -> Option<Type> {
        let expr = self.program.expressions.get(statement.expr?)?;
        if !matches!(expr.kind, AstExprKind::ListLiteral { .. }) {
            return None;
        }
        if statement.children.is_empty() {
            return Some(Type::List(Box::new(renderable_contract_type())));
        }
        let child_types = statement
            .children
            .iter()
            .filter_map(|child| child.expr)
            .filter_map(|expr_id| {
                self.program
                    .expressions
                    .get(expr_id)
                    .and_then(|expr| self.static_expr_type(expr, &mut BTreeSet::new()))
            })
            .collect::<Vec<_>>();
        let item_type = if child_types.iter().any(type_contains_skip) {
            Type::Skip
        } else if child_types.iter().all(is_renderable_type) {
            renderable_contract_type()
        } else {
            open_object_type()
        };
        Some(Type::List(Box::new(item_type)))
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

    fn check_true_false_input(&mut self, expr: &AstExpr, operator: &str, input_flow: &FlowType) {
        if matches!(input_flow.ty, Type::Unknown)
            || is_open_object_type(&input_flow.ty)
            || type_accepts_true_false(&input_flow.ty)
        {
            return;
        }
        self.diagnostics.push(self.diagnostic_for_expr(
            expr.id,
            format!("`{operator}` expects `True` or `False` tag"),
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
        if matches!(initial_type, Type::Skip) {
            self.diagnostics.push(
                self.diagnostic_for_expr(
                    *initial,
                    "`SKIP` cannot initialize a held value".to_owned(),
                ),
            );
            return;
        }
        for update in hold_update_exprs(statement, &self.program.expressions) {
            let update_type = self.ensure_expr(update).ty;
            if matches!(update_type, Type::Skip) {
                continue;
            }
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

    fn check_latest_branch_compatibility(&mut self, statement: &AstStatement) {
        let Some(expr_id) = statement.expr else {
            return;
        };
        if !matches!(
            self.program.expressions.get(expr_id).map(|expr| &expr.kind),
            Some(AstExprKind::Latest)
        ) {
            return;
        }
        let mut expected_type: Option<Type> = None;
        for branch_expr_id in statement.children.iter().filter_map(|child| child.expr) {
            let branch_type = self.ensure_expr(branch_expr_id).ty;
            if matches!(branch_type, Type::Skip) {
                continue;
            }
            let Some(expected) = expected_type.as_ref() else {
                expected_type = Some(branch_type);
                continue;
            };
            if concrete_type_conflict(expected, &branch_type) {
                self.constraints.push(Constraint::FlowCompatible {
                    actual: FlowType {
                        mode: FlowMode::PresentOrAbsent,
                        ty: branch_type.clone(),
                    },
                    expected: FlowType {
                        mode: FlowMode::PresentOrAbsent,
                        ty: expected.clone(),
                    },
                });
                self.diagnostics.push(self.diagnostic_for_expr(
                    branch_expr_id,
                    "`LATEST` branches must produce compatible data types".to_owned(),
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

    fn check_style_args(&mut self, args: &[AstCallArg]) {
        for arg in args
            .iter()
            .filter(|arg| arg.name.as_deref() == Some("style"))
        {
            self.check_style_expr(arg.value);
        }
    }

    fn check_style_expr(&mut self, expr_id: usize) {
        let Some(expr) = self.program.expressions.get(expr_id) else {
            return;
        };
        if matches!(
            expr.kind,
            AstExprKind::ListLiteral { .. } | AstExprKind::Delimiter
        ) {
            return;
        }
        let (AstExprKind::Object(fields) | AstExprKind::Record(fields)) = &expr.kind else {
            let ty = self.ensure_expr(expr_id).ty;
            if !is_open_object_type(&ty) {
                self.diagnostics
                    .push(self.diagnostic_for_expr(expr_id, "style must be an object".to_owned()));
            }
            return;
        };
        let fields = fields.clone();
        for field in &fields {
            self.check_style_field(field);
        }
    }

    fn check_style_statement(&mut self, statement: &AstStatement) {
        if let Some(expr_id) = statement.expr {
            self.check_style_expr(expr_id);
        }
        for child in &statement.children {
            let Some(name) = statement_field(child) else {
                continue;
            };
            if let Some(expr_id) = child.expr {
                self.check_style_field_value(&name, expr_id);
            }
        }
    }

    fn check_style_field(&mut self, field: &AstRecordField) {
        self.check_style_field_value(&field.name, field.value);
    }

    fn check_style_field_value(&mut self, field_name: &str, value_expr_id: usize) {
        match field_name {
            "width" | "height" | "padding" | "gap" => {
                let ty = self.ensure_expr(value_expr_id).ty;
                if !style_dimension_accepts_type(&ty) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        value_expr_id,
                        format!("style field `{field_name}` must be a number or `Fill` tag"),
                    ));
                }
            }
            "font" => self.check_style_nested_object(value_expr_id, |checker, nested| match nested
                .name
                .as_str()
            {
                "size" => {
                    let ty = checker.ensure_expr(nested.value).ty;
                    if !matches!(ty, Type::Number) {
                        checker.diagnostics.push(checker.diagnostic_for_expr(
                            nested.value,
                            "style field `font.size` must be a number".to_owned(),
                        ));
                    }
                }
                "color" => checker.check_style_color_field("font.color", nested.value),
                _ => {}
            }),
            "background" | "border" | "selected_border" => {
                let prefix = field_name.to_owned();
                self.check_style_nested_object(value_expr_id, |checker, nested| {
                    if nested.name == "color" {
                        checker.check_style_color_field(&format!("{prefix}.color"), nested.value);
                    }
                });
            }
            "color" => self.check_style_color_field("color", value_expr_id),
            _ => {}
        }
    }

    fn check_style_nested_object<F>(&mut self, expr_id: usize, mut check_field: F)
    where
        F: FnMut(&mut Self, &AstRecordField),
    {
        let Some(expr) = self.program.expressions.get(expr_id) else {
            return;
        };
        let (AstExprKind::Object(fields) | AstExprKind::Record(fields)) = &expr.kind else {
            self.diagnostics.push(
                self.diagnostic_for_expr(
                    expr_id,
                    "style nested field must be an object".to_owned(),
                ),
            );
            return;
        };
        let fields = fields.clone();
        for field in &fields {
            check_field(self, field);
        }
    }

    fn check_style_color_field(&mut self, field_name: &str, expr_id: usize) {
        let ty = self.ensure_expr(expr_id).ty;
        if !style_color_accepts_type(&ty) {
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!("style field `{field_name}` must be `Oklch[...]`"),
            ));
        }
    }

    fn check_recursive_functions(&mut self) {
        let graph = function_call_graph(self.program);
        let function_statements = function_statement_map(&self.program.ast.statements);
        let mut visited = BTreeSet::new();
        let mut active = Vec::new();
        let mut reported = BTreeSet::new();
        for function in graph.keys() {
            report_recursive_function_cycles(
                function,
                &graph,
                &function_statements,
                &mut visited,
                &mut active,
                &mut reported,
                &mut self.diagnostics,
            );
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

fn diagnostic_for_statement(statement: Option<&AstStatement>, message: String) -> TypeDiagnostic {
    TypeDiagnostic {
        severity: DiagnosticSeverity::Error,
        line: statement
            .map(|statement| statement.line)
            .unwrap_or_default(),
        start: statement
            .map(|statement| statement.start)
            .unwrap_or_default(),
        end: statement.map(|statement| statement.end).unwrap_or_default(),
        message,
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

fn function_statement_map(statements: &[AstStatement]) -> BTreeMap<String, &AstStatement> {
    let mut functions = BTreeMap::new();
    collect_function_statements(statements, &mut functions);
    functions
}

fn collect_function_statements<'a>(
    statements: &'a [AstStatement],
    functions: &mut BTreeMap<String, &'a AstStatement>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &statement.kind {
            functions.insert(name.clone(), statement);
        }
        collect_function_statements(&statement.children, functions);
    }
}

fn function_call_graph(program: &ParsedProgram) -> BTreeMap<String, BTreeSet<String>> {
    let user_functions = program.functions.iter().cloned().collect::<BTreeSet<_>>();
    let mut graph = BTreeMap::new();
    collect_function_call_graph(
        &program.ast.statements,
        &program.expressions,
        &user_functions,
        &mut graph,
    );
    graph
}

fn collect_function_call_graph(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    user_functions: &BTreeSet<String>,
    graph: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &statement.kind {
            let mut calls = BTreeSet::new();
            collect_statement_user_function_calls(
                statement,
                expressions,
                user_functions,
                &mut calls,
            );
            graph.insert(name.clone(), calls);
        }
        collect_function_call_graph(&statement.children, expressions, user_functions, graph);
    }
}

fn collect_statement_user_function_calls(
    statement: &AstStatement,
    expressions: &[AstExpr],
    user_functions: &BTreeSet<String>,
    calls: &mut BTreeSet<String>,
) {
    if let Some(expr_id) = statement.expr {
        collect_expr_user_function_calls(expr_id, expressions, user_functions, calls);
    }
    for child in &statement.children {
        collect_statement_user_function_calls(child, expressions, user_functions, calls);
    }
}

fn collect_expr_user_function_calls(
    expr_id: usize,
    expressions: &[AstExpr],
    user_functions: &BTreeSet<String>,
    calls: &mut BTreeSet<String>,
) {
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Call { function, args } => {
            if user_functions.contains(function) {
                calls.insert(function.clone());
            }
            for arg in args {
                collect_expr_user_function_calls(arg.value, expressions, user_functions, calls);
            }
        }
        AstExprKind::Pipe { input, op, args } => {
            collect_expr_user_function_calls(*input, expressions, user_functions, calls);
            if user_functions.contains(op) {
                calls.insert(op.clone());
            }
            for arg in args {
                collect_expr_user_function_calls(arg.value, expressions, user_functions, calls);
            }
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            collect_expr_user_function_calls(*initial, expressions, user_functions, calls);
        }
        AstExprKind::Then { input, output } => {
            collect_expr_user_function_calls(*input, expressions, user_functions, calls);
            if let Some(output) = output {
                collect_expr_user_function_calls(*output, expressions, user_functions, calls);
            }
        }
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => collect_expr_user_function_calls(*output, expressions, user_functions, calls),
        AstExprKind::Infix { left, right, .. } => {
            collect_expr_user_function_calls(*left, expressions, user_functions, calls);
            collect_expr_user_function_calls(*right, expressions, user_functions, calls);
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                collect_expr_user_function_calls(field.value, expressions, user_functions, calls);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::ListLiteral { .. }
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_)
        | AstExprKind::MatchArm { output: None, .. } => {}
    }
}

fn report_recursive_function_cycles(
    function: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    function_statements: &BTreeMap<String, &AstStatement>,
    visited: &mut BTreeSet<String>,
    active: &mut Vec<String>,
    reported: &mut BTreeSet<String>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) {
    if let Some(position) = active.iter().position(|candidate| candidate == function) {
        let cycle = active[position..]
            .iter()
            .cloned()
            .chain(std::iter::once(function.to_owned()))
            .collect::<Vec<_>>();
        for name in &cycle[..cycle.len().saturating_sub(1)] {
            if reported.insert(name.clone()) {
                diagnostics.push(diagnostic_for_statement(
                    function_statements.get(name).copied(),
                    format!(
                        "`FUNCTION {name}` is recursive; recursive functions are not supported by v1 type inference: {}",
                        cycle.join(" -> ")
                    ),
                ));
            }
        }
        return;
    }
    if !visited.insert(function.to_owned()) {
        return;
    }
    active.push(function.to_owned());
    if let Some(calls) = graph.get(function) {
        for call in calls {
            report_recursive_function_cycles(
                call,
                graph,
                function_statements,
                visited,
                active,
                reported,
                diagnostics,
            );
        }
    }
    active.pop();
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
    list_functions: BTreeSet<&'static str>,
    list_item_functions: BTreeSet<&'static str>,
    open_object_functions: BTreeSet<&'static str>,
}

impl Default for BuiltinSignatureRegistry {
    fn default() -> Self {
        Self {
            text_functions: [
                "Text/empty",
                "Text/trim",
                "Text/concat",
                "Text/substring",
                "Error/text",
            ]
            .into_iter()
            .collect(),
            number_functions: [
                "Number/add",
                "Number/subtract",
                "List/count",
                "List/sum",
                "Text/find",
                "Text/length",
                "Text/to_number",
            ]
            .into_iter()
            .collect(),
            true_false_functions: ["Bool/not", "Bool/and", "Text/is_empty", "Text/starts_with"]
                .into_iter()
                .collect(),
            list_functions: [
                "List/map",
                "List/retain",
                "List/append",
                "List/remove",
                "List/range",
                "List/chunk",
            ]
            .into_iter()
            .collect(),
            list_item_functions: ["List/find", "List/find_value", "List/get"]
                .into_iter()
                .collect(),
            open_object_functions: ["WHILE", "Widget/table", "Widget/selected", "Widget/rows"]
                .into_iter()
                .collect(),
        }
    }
}

impl BuiltinSignatureRegistry {
    fn type_for_call(&self, function: &str, render_contracts: &RenderContractRegistry) -> Type {
        if self.text_functions.contains(function) {
            Type::Text
        } else if self.number_functions.contains(function) {
            Type::Number
        } else if self.true_false_functions.contains(function) {
            true_false_type()
        } else if self.list_functions.contains(function) {
            Type::List(Box::new(open_object_type()))
        } else if self.list_item_functions.contains(function) {
            open_object_type()
        } else if self.open_object_functions.contains(function) {
            open_object_type()
        } else if function == "Error/new" {
            Type::VariantSet(vec![Variant::Tagged {
                tag: "Error".to_owned(),
                fields: ObjectShape {
                    fields: BTreeMap::new(),
                    open: true,
                },
            }])
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
            constructors: RENDER_CONSTRUCTORS.iter().copied().collect(),
        }
    }
}

impl RenderContractRegistry {
    fn is_render_constructor(&self, function: &str) -> bool {
        self.constructors.contains(function)
    }

    fn slot_contract(&self, slot_name: &str) -> &'static str {
        match slot_name {
            "items" | "children" => "LIST<Element>",
            _ => "Element",
        }
    }
}

const RENDER_CONSTRUCTORS: &[&str] = &[
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
];

fn is_registered_render_constructor(function: &str) -> bool {
    RENDER_CONSTRUCTORS.contains(&function)
}

fn type_accepts_true_false(ty: &Type) -> bool {
    let Type::VariantSet(variants) = ty else {
        return false;
    };
    variants
        .iter()
        .all(|variant| matches!(variant, Variant::Tag(tag) if tag == "True" || tag == "False"))
}

fn style_dimension_accepts_type(ty: &Type) -> bool {
    matches!(ty, Type::Number)
        || matches!(
            ty,
            Type::VariantSet(variants)
                if variants.iter().all(|variant| {
                    matches!(variant, Variant::Tag(tag) if tag == "Fill")
                })
        )
}

fn style_color_accepts_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::VariantSet(variants)
            if variants.iter().all(|variant| {
                matches!(variant, Variant::Tagged { tag, .. } if tag == "Oklch")
            })
    )
}

fn concrete_type_conflict(left: &Type, right: &Type) -> bool {
    match (left, right) {
        (Type::Unknown, _) | (_, Type::Unknown) => false,
        (Type::Skip, _) | (_, Type::Skip) => false,
        (left, _) if is_open_object_type(left) => false,
        (_, right) if is_open_object_type(right) => false,
        (Type::Text, Type::Text)
        | (Type::Number, Type::Number)
        | (Type::RenderContract, Type::RenderContract) => false,
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
        AstExprKind::Tag(value) | AstExprKind::Enum(value) if value == "SKIP" => Type::Skip,
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
        AstExprKind::Call { function, .. } if is_registered_render_constructor(function) => {
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
        (Type::Skip, ty) | (ty, Type::Skip) => ty.clone(),
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
        Some("address") => Type::Text,
        _ => open_object_type(),
    }
}

fn source_payload_shape_table(program: &ParsedProgram) -> Vec<SourcePayloadShapeEntry> {
    let source_paths = program
        .source_ports
        .iter()
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    program
        .source_ports
        .iter()
        .map(|source| {
            let fields = source_payload_fields_for_path(program, &source_paths, &source.path);
            let payload_type = Type::Object(ObjectShape {
                fields: fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.clone()))
                    .collect(),
                open: true,
            });
            SourcePayloadShapeEntry {
                source_path: source.path.clone(),
                payload_type,
                fields,
            }
        })
        .collect()
}

fn source_payload_fields_for_path(
    program: &ParsedProgram,
    source_paths: &BTreeSet<String>,
    source_path: &str,
) -> Vec<SourcePayloadShapeField> {
    let mut fields = BTreeMap::new();
    for expr in &program.expressions {
        match &expr.kind {
            AstExprKind::Path(parts) => {
                if source_path_matches_parts(source_paths, source_path, parts)
                    && let Some(field) = source_payload_field_name(parts)
                {
                    fields.insert(field.to_owned(), Type::Text);
                }
            }
            _ => {}
        }
    }
    collect_payload_pattern_fields_for_source(
        &program.ast.statements,
        &program.expressions,
        source_paths,
        source_path,
        &mut fields,
    );
    fields
        .into_iter()
        .map(|(name, ty)| SourcePayloadShapeField { name, ty })
        .collect()
}

fn collect_payload_pattern_fields_for_source(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    source_paths: &BTreeSet<String>,
    source_path: &str,
    fields: &mut BTreeMap<String, Type>,
) {
    for statement in statements {
        if let Some(expr_id) = statement.expr
            && let Some(AstExpr {
                kind: AstExprKind::When { input },
                ..
            }) = expressions.get(expr_id)
            && expr_is_source_path(*input, expressions, source_paths, source_path)
        {
            for child in &statement.children {
                if let Some(AstExpr {
                    kind: AstExprKind::MatchArm { pattern, .. },
                    ..
                }) = child.expr.and_then(|expr_id| expressions.get(expr_id))
                {
                    for field in source_payload_fields_from_pattern(pattern) {
                        fields.insert(field.to_owned(), Type::Text);
                    }
                }
            }
        }
        collect_payload_pattern_fields_for_source(
            &statement.children,
            expressions,
            source_paths,
            source_path,
            fields,
        );
    }
}

fn expr_is_source_path(
    expr_id: usize,
    expressions: &[AstExpr],
    source_paths: &BTreeSet<String>,
    source_path: &str,
) -> bool {
    match expressions.get(expr_id).map(|expr| &expr.kind) {
        Some(AstExprKind::Identifier(value)) => {
            source_path_matches_parts(source_paths, source_path, std::slice::from_ref(value))
        }
        Some(AstExprKind::Path(parts)) => {
            source_path_matches_parts(source_paths, source_path, parts)
        }
        Some(AstExprKind::Pipe { input, .. }) | Some(AstExprKind::When { input }) => {
            expr_is_source_path(*input, expressions, source_paths, source_path)
        }
        _ => false,
    }
}

fn source_path_matches_parts(
    source_paths: &BTreeSet<String>,
    source_path: &str,
    parts: &[String],
) -> bool {
    let path = parts.join(".");
    if !path_is_source_path(source_paths, &path) {
        return false;
    }
    let relative = source_path.strip_prefix("store.").unwrap_or(source_path);
    path.starts_with(&format!("{source_path}."))
        || path.starts_with(&format!("{relative}."))
        || source_path.ends_with(&format!(".{}", parts_without_payload(parts).join(".")))
        || relative.ends_with(&format!(".{}", parts_without_payload(parts).join(".")))
}

fn parts_without_payload(parts: &[String]) -> &[String] {
    match parts.last().map(String::as_str) {
        Some("text" | "key" | "address") => &parts[..parts.len().saturating_sub(1)],
        _ => parts,
    }
}

fn source_payload_field_name(parts: &[String]) -> Option<&str> {
    match parts.last().map(String::as_str) {
        Some(field @ ("text" | "key" | "address")) => Some(field),
        _ => None,
    }
}

fn source_payload_fields_from_pattern(pattern: &[String]) -> Vec<&'static str> {
    let mut fields = Vec::new();
    for window in pattern.windows(2) {
        if matches!(window[0].as_str(), "text" | "key" | "address") && window[1].as_str() == ":" {
            fields.push(match window[0].as_str() {
                "text" => "text",
                "key" => "key",
                "address" => "address",
                _ => unreachable!(),
            });
        }
    }
    fields
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
    Type::RenderContract
}

fn render_slot_accepts_type(slot_name: &str, ty: &Type) -> bool {
    match slot_name {
        "items" | "children" => match ty {
            Type::List(item) => is_renderable_type(item),
            _ => false,
        },
        _ => is_renderable_type(ty),
    }
}

fn is_renderable_type(ty: &Type) -> bool {
    matches!(ty, Type::RenderContract) || is_no_element_type(ty)
}

fn is_no_element_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::VariantSet(variants)
            if variants.iter().all(|variant| {
                matches!(variant, Variant::Tag(tag) if tag == "NoElement")
            })
    )
}

fn type_contains_renderable(ty: &Type) -> bool {
    match ty {
        Type::RenderContract => true,
        ty if is_no_element_type(ty) => true,
        Type::List(item) => type_contains_renderable(item),
        Type::Object(shape) => shape.fields.values().any(type_contains_renderable),
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields.fields.values().any(type_contains_renderable),
        }),
        Type::Function { result, .. } => type_contains_renderable(&result.ty),
        Type::Text | Type::Number | Type::Skip | Type::Var(_) | Type::Unknown => false,
    }
}

fn type_contains_no_element(ty: &Type) -> bool {
    match ty {
        ty if is_no_element_type(ty) => true,
        Type::List(item) => type_contains_no_element(item),
        Type::Object(shape) => shape.fields.values().any(type_contains_no_element),
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields.fields.values().any(type_contains_no_element),
        }),
        Type::Function { result, .. } => type_contains_no_element(&result.ty),
        Type::Text
        | Type::Number
        | Type::Skip
        | Type::RenderContract
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

fn type_contains_skip(ty: &Type) -> bool {
    match ty {
        Type::Skip => true,
        Type::List(item) => type_contains_skip(item),
        Type::Object(shape) => shape.fields.values().any(type_contains_skip),
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields.fields.values().any(type_contains_skip),
        }),
        Type::Function { result, .. } => type_contains_skip(&result.ty),
        Type::Text | Type::Number | Type::RenderContract | Type::Var(_) | Type::Unknown => false,
    }
}

fn expr_is_skip(expr: &AstExpr) -> bool {
    matches!(&expr.kind, AstExprKind::Tag(tag) | AstExprKind::Enum(tag) if tag == "SKIP")
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
        Type::Text | Type::Number | Type::Skip | Type::RenderContract | Type::Unknown => {}
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
    fn accepts_function_returning_renderable_list_for_items_without_slot_list_map() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
todos: LIST[4] {}
FUNCTION todo_row(todo) {
    Element/label(label: todo.title)
}
FUNCTION make_rows(todos) {
    todos
    |> List/map(todo, new: todo_row(todo: todo))
}
document:
    root:
        Element/stripe(
            items: make_rows(todos: todos)
        )
"#;
        let parsed = boon_parser::parse_source("items-function-list.bn", source).unwrap();
        let report = check(&parsed);
        assert!(
            !report.has_errors(),
            "unexpected diagnostics: {:?}",
            report.diagnostics
        );
        let slot = report
            .render_slot_table
            .slots
            .iter()
            .find(|slot| slot.slot_name == "items")
            .expect("items slot should be typed");
        assert!(matches!(
            &slot.actual_type,
            Type::List(item) if matches!(**item, Type::RenderContract)
        ));
        assert_eq!(report.list_map_binding_count_render_slot_materialization, 0);
    }

    #[test]
    fn rejects_list_map_to_data_in_render_slot() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
todos: LIST[4] {}
document:
    root:
        Element/stripe(
            items:
                todos
                |> List/map(todo, new: todo)
        )
"#;
        let parsed = boon_parser::parse_source("bad-items-data-map.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert_eq!(report.list_map_binding_count_render_slot_materialization, 0);
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("expected a list of renderable values for `items:`")
        }));
    }

    #[test]
    fn rejects_nested_renderable_list_passed_to_items_without_flattening() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
FUNCTION renderable_list() {
    LIST {
        Element/label(label: TEXT { nested })
    }
}
document:
    root:
        Element/stripe(
            items: LIST {
                renderable_list()
            }
        )
"#;
        let parsed = boon_parser::parse_source("bad-nested-render-list.bn", source).unwrap();
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
    fn rejects_bool_and_on_non_true_false_values() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
bad_input: 1 |> Bool/and(True)
bad_arg: True |> Bool/and(1)
document: []
"#;
        let parsed = boon_parser::parse_source("bad-bool-and.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic
                    .message
                    .contains("`Bool/and` expects `True` or `False` tag"))
                .count(),
            2
        );
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
    fn rejects_wrong_style_field_types() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
document:
    root:
        Element/stripe(
            style: [
                width: TEXT { wide }
                background: [color: Bright]
                font: [size: Big]
            ]
            items: LIST {}
        )
"#;
        let parsed = boon_parser::parse_source("bad-style.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("style field `width` must be a number or `Fill` tag")
        }));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("style field `background.color` must be `Oklch[...]`")
        }));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("style field `font.size` must be a number")
        }));
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
    fn rejects_latest_branches_with_incompatible_data_types() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
bad:
    LATEST {
        source |> THEN { 1 }
        source |> THEN { TEXT { bad } }
        source |> THEN { SKIP }
    }
document: []
"#;
        let parsed = boon_parser::parse_source("bad-latest-branches.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("`LATEST` branches must produce compatible data types")
        }));
    }

    #[test]
    fn rejects_skip_as_hold_initial_but_allows_absent_updates() {
        let source = r#"
source: SOURCE
bad:
    SKIP |> HOLD bad {
        LATEST {}
    }
good:
    TEXT { ok } |> HOLD good {
        LATEST {
            source |> THEN { SKIP }
        }
    }
document: []
"#;
        let parsed = boon_parser::parse_source("bad-skip-hold.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic
                    .message
                    .contains("`SKIP` cannot initialize a held value"))
                .count(),
            1
        );
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("`HOLD` update must match"))
        );
    }

    #[test]
    fn rejects_skip_as_render_value() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
document:
    root:
        Element/stripe(
            items: LIST {
                SKIP
            }
        )
"#;
        let parsed = boon_parser::parse_source("bad-skip-render.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("`SKIP` cannot be used as a render value")
        }));
    }

    #[test]
    fn rejects_no_element_as_normal_data_but_allows_it_in_render_slot() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
bad: NoElement
document:
    root:
        Element/stripe(
            items: LIST {
                NoElement
            }
        )
"#;
        let parsed = boon_parser::parse_source("bad-no-element-data.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic
                    .message
                    .contains("`NoElement` can only be used as a render value"))
                .count(),
            1
        );
        assert_eq!(report.render_slot_failure_count, 0);
    }

    #[test]
    fn rejects_unregistered_element_prefix_as_unknown_function() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
document:
    root: Element/not_registered(label: TEXT { bad })
"#;
        let parsed = boon_parser::parse_source("unknown-element-prefix.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("unknown function or operator `Element/not_registered`")
        }));
    }

    #[test]
    fn infers_tagged_objects_and_accepts_structural_extra_fields() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
todo: [title: TEXT { Read }, extra: 1, hidden: Hidden[id: TEXT { runtime-free }]]
FUNCTION row(todo) {
    Element/label(label: todo.title)
}
document:
    root:
        Element/stripe(
            items: LIST {
                row(todo: todo)
            }
        )
"#;
        let parsed = boon_parser::parse_source("structural-extra-fields.bn", source).unwrap();
        let report = check(&parsed);
        assert!(
            !report.has_errors(),
            "unexpected diagnostics: {:?}",
            report.diagnostics
        );
        assert!(report.expr_type_table.entries.iter().any(|entry| {
            matches!(
                &entry.flow_type.ty,
                Type::VariantSet(variants)
                    if variants.iter().any(|variant| {
                        matches!(variant, Variant::Tagged { tag, .. } if tag == "Hidden")
                    })
            )
        }));
    }

    #[test]
    fn source_payload_shape_table_reports_payload_fields() {
        let source = r#"
store: [
    sources: [
        input: [
            change: SOURCE
            key_down: SOURCE
        ]
    ]
    text:
        Text/empty() |> HOLD text {
            LATEST {
                sources.input.change.text
                sources.input.key_down |> WHEN {
                    [key: Enter, text: submitted] => submitted
                    __ => SKIP
                }
            }
        }
]
document: []
"#;
        let parsed = boon_parser::parse_source("source-payload-shapes.bn", source).unwrap();
        let report = check(&parsed);
        let change = report
            .source_payload_shape_table
            .iter()
            .find(|entry| entry.source_path == "store.sources.input.change")
            .expect("change source should have a payload shape");
        assert!(change.fields.iter().any(|field| field.name == "text"));
        let key_down = report
            .source_payload_shape_table
            .iter()
            .find(|entry| entry.source_path == "store.sources.input.key_down")
            .expect("key_down source should have a payload shape");
        assert!(key_down.fields.iter().any(|field| field.name == "key"));
        assert!(key_down.fields.iter().any(|field| field.name == "text"));
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
    fn rejects_unknown_same_prefix_builtin_names() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
bad_bool: True |> Bool/xor()
bad_text: value |> Text/frob()
items: LIST {}
bad_list: items |> List/shuffle()
document: []
"#;
        let parsed = boon_parser::parse_source("unknown-prefixed-builtins.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        for function in ["Bool/xor", "Text/frob", "List/shuffle"] {
            assert!(
                report.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .message
                        .contains(&format!("unknown function or operator `{function}`"))
                }),
                "missing diagnostic for {function}: {:?}",
                report.diagnostics
            );
        }
    }

    #[test]
    fn rejects_recursive_functions_in_v1() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
FUNCTION left(value) {
    right(value: value)
}
FUNCTION right(value) {
    left(value: value)
}
document: []
"#;
        let parsed = boon_parser::parse_source("bad-recursive-functions.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("recursive functions are not supported by v1 type inference")
                && diagnostic.message.contains("left -> right -> left")
        }));
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
