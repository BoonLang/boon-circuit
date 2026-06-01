use boon_parser::{
    AstCallArg, AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind, ParsedProgram,
};
use ena::unify::{EqUnifyValue, InPlaceUnificationTable, UnifyKey};
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
    UnresolvedShape {
        reason: String,
    },
    Var(TypeVar),
    Unknown,
}

impl EqUnifyValue for Type {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Variant {
    Tag(String),
    Tagged { tag: String, fields: ObjectShape },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObjectShape {
    pub fields: BTreeMap<String, Type>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_order: Vec<String>,
    pub open: bool,
}

impl ObjectShape {
    fn new(fields: BTreeMap<String, Type>, open: bool) -> Self {
        let field_order = fields.keys().cloned().collect();
        Self {
            fields,
            field_order,
            open,
        }
    }

    fn from_ordered_fields(fields: impl IntoIterator<Item = (String, Type)>, open: bool) -> Self {
        let mut shape_fields = BTreeMap::new();
        let mut field_order = Vec::new();
        for (field, ty) in fields {
            if !shape_fields.contains_key(&field) {
                field_order.push(field.clone());
            }
            shape_fields.insert(field, ty);
        }
        Self {
            fields: shape_fields,
            field_order,
            open,
        }
    }

    fn ordered_fields(&self) -> Vec<(&String, &Type)> {
        let mut seen = BTreeSet::new();
        let mut fields = Vec::new();
        for field in &self.field_order {
            if let Some(ty) = self.fields.get(field) {
                seen.insert(field.as_str());
                fields.push((field, ty));
            }
        }
        for (field, ty) in &self.fields {
            if seen.insert(field.as_str()) {
                fields.push((field, ty));
            }
        }
        fields
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct TypeVar(pub u32);

impl UnifyKey for TypeVar {
    type Value = Option<Type>;

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
        self.table.new_key(None)
    }

    pub fn unify(&mut self, left: TypeVar, right: TypeVar) -> Result<(), (Type, Type)> {
        self.table.unify_var_var(left, right)
    }

    pub fn bind(&mut self, var: TypeVar, ty: Type) -> Result<(), (Type, Type)> {
        self.table.unify_var_value(var, Some(ty))
    }

    pub fn root(&mut self, var: TypeVar) -> TypeVar {
        self.table.find(var)
    }

    pub fn resolved(&mut self, var: TypeVar) -> Option<Type> {
        self.table.probe_value(var)
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
    pub arg_types: Vec<Type>,
    pub result: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct FunctionTypeTable {
    pub entries: Vec<FunctionTypeEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeHintEntry {
    pub expr_id: Option<usize>,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub anchor_column: usize,
    pub category: String,
    pub compact_label: String,
    pub detail_label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct TypeHintTable {
    pub entries: Vec<TypeHintEntry>,
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
    pub type_hint_table: TypeHintTable,
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
    source_payload_shape_table: Vec<SourcePayloadShapeEntry>,
    source_payload_types: BTreeMap<String, Type>,
    object_bindings: BTreeMap<String, ObjectShape>,
    name_bindings: BTreeMap<String, Type>,
    flow_bindings: BTreeMap<String, FlowMode>,
    function_param_requirements: BTreeMap<String, BTreeMap<String, Type>>,
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
        let source_payload_shape_table = source_payload_shape_table(program);
        let source_payload_types = source_payload_shape_table
            .iter()
            .map(|entry| (entry.source_path.clone(), entry.payload_type.clone()))
            .collect();
        let object_bindings = object_bindings(program);
        let function_param_requirements = function_param_requirements(program);
        let name_bindings =
            name_bindings(program, &source_payload_types, &function_param_requirements);
        let flow_bindings = flow_bindings(program);
        let mut checker = Self {
            program,
            vars: TypeVarStore::default(),
            builtins: BuiltinSignatureRegistry::default(),
            render_contracts: RenderContractRegistry::default(),
            source_paths,
            source_payload_shape_table,
            source_payload_types,
            object_bindings,
            name_bindings,
            flow_bindings,
            function_param_requirements,
            expr_type_vars: BTreeMap::new(),
            runtime_list_map_exprs: BTreeSet::new(),
            visited: BTreeSet::new(),
            expr_type_table: ExprTypeTable::default(),
            function_type_table: FunctionTypeTable::default(),
            render_slot_table: RenderSlotTable::default(),
            list_map_bindings: Vec::new(),
            constraints: Vec::new(),
            diagnostics: Vec::new(),
        };
        checker.refresh_static_row_scope_bindings();
        checker
    }

    fn refresh_static_row_scope_bindings(&mut self) {
        for row_scope in &self.program.row_scope_functions {
            let Some(row_type) = canonical_row_scope_type(
                self.program,
                &self.name_bindings,
                &self.function_param_requirements,
                &row_scope.function,
                &row_scope.list,
                &row_scope.row_scope,
                self.user_function_return_type(&row_scope.function, &mut BTreeSet::new()),
            ) else {
                continue;
            };
            self.name_bindings
                .insert(row_scope.row_scope.clone(), row_type.clone());
            self.name_bindings.insert(
                row_scope.list.clone(),
                Type::List(Box::new(row_type.clone())),
            );
            if let Type::Object(shape) = &row_type {
                for (field, ty) in shape.ordered_fields() {
                    self.name_bindings.insert(field.clone(), ty.clone());
                    self.name_bindings
                        .insert(format!("{}.{}", row_scope.row_scope, field), ty.clone());
                }
            }
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
        let source_payload_shape_table = self.source_payload_shape_table.clone();
        let type_hint_table = type_hint_table(
            self.program,
            &self.expr_type_table,
            &self.function_type_table,
            &self.render_slot_table,
            &source_payload_shape_table,
            &self.name_bindings,
        );
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
            type_hint_table,
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
            if let Some(function) =
                render_constructor_for_expr(expr_id, &self.program.expressions).map(str::to_owned)
            {
                self.check_render_constructor_fields(statement, &function);
            }
        }
        if statement_field(statement).as_deref() == Some("style") {
            self.check_style_statement(statement);
        }
        self.check_pattern_constraints(statement);
        self.check_hold_update_compatibility(statement);
        self.check_latest_branch_compatibility(statement);
        if let AstStatementKind::Function { name, args } = &statement.kind {
            let arg_types = args
                .iter()
                .map(|arg| self.function_arg_display_type(name, arg))
                .collect();
            self.function_type_table.entries.push(FunctionTypeEntry {
                name: name.clone(),
                args: args.clone(),
                arg_types,
                result: FlowType {
                    mode: FlowMode::Continuous,
                    ty: self.type_for_call(name),
                },
            });
        }
        if next_in_document
            && matches!(
                statement_field(statement).as_deref(),
                Some("root" | "child" | "items" | "children")
            )
        {
            self.check_render_slot(statement);
        }
        for child in &statement.children {
            self.check_statement(child, next_in_document);
        }
    }

    fn function_arg_display_type(&self, function: &str, arg: &str) -> Type {
        if self
            .program
            .row_scope_functions
            .iter()
            .any(|row_scope| row_scope.function == function && row_scope.row_scope == arg)
            && let Some(ty) = self.name_bindings.get(arg)
        {
            return ty.clone();
        }
        let requirement = self
            .function_param_requirements
            .get(function)
            .and_then(|requirements| requirements.get(arg))
            .cloned();
        if let Some(ty) = self.function_arg_call_site_type(function, arg) {
            return requirement
                .as_ref()
                .map(|requirement| merge_canonical_row_type(&ty, requirement))
                .unwrap_or(ty);
        }
        if let Some(ty) = self.name_bindings.get(arg) {
            return requirement
                .as_ref()
                .map(|requirement| merge_canonical_row_type(ty, requirement))
                .unwrap_or_else(|| ty.clone());
        }
        requirement.unwrap_or_else(open_object_type)
    }

    fn function_arg_call_site_type(&self, function: &str, arg: &str) -> Option<Type> {
        let function_args = function_statement_args(self.program, function)?;
        let mut ty = None;
        for expr in &self.program.expressions {
            let arg_expr_id = match &expr.kind {
                AstExprKind::Call {
                    function: call,
                    args,
                } if call == function => {
                    function_call_argument_expr(function_args, arg, None, args)
                }
                AstExprKind::Pipe { input, op, args } if op == function => {
                    function_call_argument_expr(function_args, arg, Some(*input), args)
                }
                _ => None,
            };
            let Some(arg_expr_id) = arg_expr_id else {
                continue;
            };
            let Some(arg_expr) = self.program.expressions.get(arg_expr_id) else {
                continue;
            };
            let Some(arg_ty) = self.static_expr_type(arg_expr, &mut BTreeSet::new()) else {
                continue;
            };
            ty = Some(match ty {
                Some(existing) => merge_canonical_row_type(&existing, &arg_ty),
                None => arg_ty,
            });
        }
        ty
    }

    fn check_render_slot(&mut self, statement: &AstStatement) {
        let slot_name = statement_field(statement).unwrap_or_else(|| "items".to_owned());
        let expected_contract = self.render_contracts.slot_contract(&slot_name).to_owned();
        let mut value_expr_id =
            direct_statement_value_expr_id(statement, &self.program.expressions);
        let mut optional_list_map_binding_id = None;
        let mut item_scope_id = None;
        let mut template_function = None;
        let mut template_args = Vec::new();
        let mut actual_type = value_expr_id
            .map(|expr_id| self.ensure_expr(expr_id).ty)
            .unwrap_or_else(|| {
                if matches!(slot_name.as_str(), "items" | "children") {
                    Type::List(Box::new(open_object_type()))
                } else {
                    open_object_type()
                }
            });
        if let Some(static_list_type) = self.render_slot_static_list_type(statement) {
            actual_type = static_list_type;
        }
        let mut materialization_policy = MaterializationPolicy::StaticChildren;

        let mut diagnostics = Vec::new();

        if let Some(mapped) = mapped_children_for_statement(statement, self.program) {
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
                let input_list_type = self.ensure_expr(mapped.list_expr_id).ty;
                let item_type = list_item_type_from_list_type(&input_list_type)
                    .unwrap_or_else(open_object_type);
                self.list_map_bindings.push(ListMapBinding {
                    map_expr_id: mapped.map_expr_id,
                    list_expr_id: mapped.list_expr_id,
                    input_list_type,
                    item_expr_id: mapped.item_expr_id,
                    item_binding_name: mapped.item_binding_name,
                    item_type,
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
                    render_slot_type_error(&slot_name, &actual_type)
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
                render_slot_type_error(&slot_name, &actual_type)
            } else {
                format!(
                    "`{slot_name}` expects an object accepted by `document:`\nexpected: [...]\nfound: {}",
                    boon_facing_type_label(&actual_type)
                )
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

    fn check_render_constructor_fields(&mut self, statement: &AstStatement, function: &str) {
        for child in &statement.children {
            let Some(field) = statement_field(child) else {
                continue;
            };
            let Some(expected) = render_arg_expected_type(function, Some(&field)) else {
                continue;
            };
            if !render_arg_should_validate_directly(function, &field) {
                continue;
            }
            let Some(value_expr_id) =
                direct_statement_value_expr_id(child, &self.program.expressions)
            else {
                continue;
            };
            let actual = self.ensure_expr(value_expr_id).ty;
            if !render_field_type_accepts(&actual, &expected) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    value_expr_id,
                    format!(
                        "`{function}` field `{field}` has incompatible type\nexpected: {}\nfound: {}",
                        boon_facing_type_label(&expected),
                        boon_facing_type_label(&actual)
                    ),
                ));
            }
        }
    }

    fn check_render_constructor_call_args(
        &mut self,
        call_expr_id: usize,
        function: &str,
        input_flow: Option<&FlowType>,
        args: &[AstCallArg],
    ) {
        if let Some(input_flow) = input_flow
            && let Some(expected) = render_arg_expected_type(function, Some("input"))
            && !render_field_type_accepts(&input_flow.ty, &expected)
        {
            self.diagnostics.push(self.diagnostic_for_expr(
                call_expr_id,
                format!(
                    "`{function}` input has incompatible type\nexpected: {}\nfound: {}",
                    boon_facing_type_label(&expected),
                    boon_facing_type_label(&input_flow.ty)
                ),
            ));
        }
        for arg in args {
            let Some(name) = arg.name.as_deref() else {
                continue;
            };
            let Some(expected) = render_arg_expected_type(function, Some(name)) else {
                continue;
            };
            if !render_arg_should_validate_directly(function, name) {
                continue;
            }
            let actual = self.ensure_expr(arg.value).ty;
            if !render_field_type_accepts(&actual, &expected) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    arg.value,
                    format!(
                        "`{function}` argument `{name}` has incompatible type\nexpected: {}\nfound: {}",
                        boon_facing_type_label(&expected),
                        boon_facing_type_label(&actual)
                    ),
                ));
            }
        }
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
        let expr_var = self.expr_type_var_key(expr_id);
        self.visited.insert(expr_id);
        let flow_type = self
            .program
            .expressions
            .get(expr_id)
            .map(|expr| self.infer_expr(expr))
            .unwrap_or(FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(expr_var),
            });
        self.constraints.push(Constraint::Equal {
            left: Type::Var(expr_var),
            right: flow_type.ty.clone(),
        });
        if !matches!(flow_type.ty, Type::Var(var) if var == expr_var)
            && self.vars.bind(expr_var, flow_type.ty.clone()).is_err()
        {
            self.diagnostics.push(
                self.diagnostic_for_expr(
                    expr_id,
                    "incompatible inferred expression types".to_owned(),
                ),
            );
        }
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
                let shape = ObjectShape::from_ordered_fields(
                    fields
                        .iter()
                        .map(|field| (field.name.clone(), self.ensure_expr(field.value).ty)),
                    false,
                );
                self.check_tagged_object_contract(expr, tag, fields, &shape);
                Type::VariantSet(vec![Variant::Tagged {
                    tag: tag.clone(),
                    fields: shape,
                }])
            }
            AstExprKind::Record(fields) | AstExprKind::Object(fields) => {
                Type::Object(ObjectShape::from_ordered_fields(
                    fields
                        .iter()
                        .map(|field| (field.name.clone(), self.ensure_expr(field.value).ty)),
                    false,
                ))
            }
            AstExprKind::ListLiteral { .. } => Type::List(Box::new(open_object_type())),
            AstExprKind::Call { function, args } => {
                for arg in args {
                    self.ensure_expr(arg.value);
                }
                self.check_user_function_arguments(expr.id, function, None, args);
                if self.render_contracts.is_render_constructor(function) {
                    self.check_style_args(args);
                    self.check_render_constructor_call_args(expr.id, function, None, args);
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
                } else if self.render_contracts.is_render_constructor(function) {
                    self.render_constructor_type_for_args(function, None, args)
                } else {
                    self.type_for_call_expr(expr.id, function)
                }
            }
            AstExprKind::Pipe { input, op, args } => {
                let input_flow = self.ensure_expr(*input);
                for arg in args {
                    self.ensure_expr(arg.value);
                }
                self.check_user_function_arguments(expr.id, op, Some(*input), args);
                if self.render_contracts.is_render_constructor(op) {
                    self.check_style_args(args);
                    self.check_render_constructor_call_args(expr.id, op, Some(&input_flow), args);
                }
                if op == "List/map" {
                    if let Some(new_expr_id) = list_map_new_expr_id(args) {
                        let item_type = self.ensure_expr(new_expr_id).ty;
                        if type_contains_skip(&item_type) {
                            self.diagnostics.push(self.diagnostic_for_expr(
                                new_expr_id,
                                "`SKIP` cannot be used as a `List/map` item".to_owned(),
                            ));
                        }
                    }
                    self.record_runtime_list_map(expr.id, *input, args);
                    Type::List(Box::new(self.list_map_result_item_type(args)))
                } else if matches!(op.as_str(), "List/retain" | "List/remove") {
                    input_flow.ty
                } else if op == "List/append" {
                    let append_item = args
                        .iter()
                        .find(|arg| arg.name.as_deref() == Some("item"))
                        .map(|arg| self.ensure_expr(arg.value).ty);
                    match (input_flow.ty, append_item) {
                        (Type::List(input_item), Some(item_ty)) => {
                            Type::List(Box::new(widen_structural_type(&input_item, &item_ty)))
                        }
                        (input_ty, _) => input_ty,
                    }
                } else if op == "WHILE" {
                    if !matches!(input_flow.mode, FlowMode::Continuous) {
                        self.constraints.push(Constraint::FlowCompatible {
                            actual: input_flow.clone(),
                            expected: FlowType {
                                mode: FlowMode::Continuous,
                                ty: input_flow.ty.clone(),
                            },
                        });
                        self.diagnostics.push(self.diagnostic_for_expr(
                            *input,
                            "`WHILE` requires a continuous selector".to_owned(),
                        ));
                    }
                    self.type_for_call_expr(expr.id, op)
                } else if self.render_contracts.is_render_constructor(op) {
                    self.render_constructor_type_for_args(op, Some(&input_flow), args)
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
            AstExprKind::Hold { initial, .. } => self.hold_result_type(expr.id, *initial),
            AstExprKind::Latest => exact_empty_object_type(),
            AstExprKind::When { input } => self
                .when_result_type(expr.id)
                .unwrap_or_else(|| self.ensure_expr(*input).ty),
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
            AstExprKind::Source => exact_empty_object_type(),
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
        if let Some(access) = source_payload_access(&self.source_paths, parts) {
            match access {
                SourcePayloadAccess::Direct(source_path) => {
                    return source_payload_type_for_path(&self.source_payload_types, &source_path)
                        .unwrap_or_else(exact_empty_object_type);
                }
                SourcePayloadAccess::Field(field) => return source_payload_field_type(&field),
                SourcePayloadAccess::UnknownField(field) => {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        expr_id,
                        format!(
                            "unknown source payload field `{field}`\nknown fields here: text: TEXT, key: TEXT, address: TEXT"
                        ),
                    ));
                    return self.expr_type_var(expr_id);
                }
            }
        }
        if let Some(ty) = self.name_bindings.get(&path) {
            return ty.clone();
        }
        if let Some(base) = parts.first().and_then(|part| self.name_bindings.get(part))
            && parts.len() > 1
        {
            if let Some(ty) = type_for_nested_path(base, &parts[1..]) {
                return ty;
            }
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

    fn render_constructor_type_for_args(
        &mut self,
        function: &str,
        input_flow: Option<&FlowType>,
        args: &[AstCallArg],
    ) -> Type {
        let mut fields = Vec::new();
        if let Some(input_flow) = input_flow
            && !matches!(input_flow.ty, Type::Unknown)
        {
            fields.push(("input".to_owned(), input_flow.ty.clone()));
        }
        for arg in args {
            let Some(name) = &arg.name else {
                continue;
            };
            let ty = self.ensure_expr(arg.value).ty;
            fields.push((name.clone(), ty));
        }
        self.render_contracts.constructor_shape(function, fields)
    }

    fn expr_type_var(&mut self, expr_id: usize) -> Type {
        Type::Var(self.expr_type_var_key(expr_id))
    }

    fn expr_type_var_key(&mut self, expr_id: usize) -> TypeVar {
        let var = *self
            .expr_type_vars
            .entry(expr_id)
            .or_insert_with(|| self.vars.new_var());
        var
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

    fn when_result_type(&mut self, expr_id: usize) -> Option<Type> {
        let arm_expr_ids = when_arm_expr_ids(&self.program.ast.statements, expr_id);
        let mut result: Option<Type> = None;
        for arm_expr_id in arm_expr_ids {
            let arm_type = self.ensure_expr(arm_expr_id).ty;
            result = Some(match result {
                Some(existing) => widen_structural_type(&existing, &arm_type),
                None => arm_type,
            });
        }
        result
    }

    fn hold_result_type(&mut self, expr_id: usize, initial: usize) -> Type {
        let mut ty = self.ensure_expr(initial).ty;
        let updates = hold_update_exprs_for_expr(
            &self.program.ast.statements,
            expr_id,
            &self.program.expressions,
        );
        for update_expr_id in updates {
            let update_type = self.ensure_expr(update_expr_id).ty;
            if !matches!(update_type, Type::Skip) {
                ty = widen_structural_type(&ty, &update_type);
            }
        }
        ty
    }

    fn static_expr_type(
        &self,
        expr: &AstExpr,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        match &expr.kind {
            AstExprKind::Call { function, args } => {
                if self.render_contracts.is_render_constructor(function) {
                    return Some(self.static_render_constructor_type(
                        function,
                        None,
                        args,
                        active_functions,
                    ));
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
                } else if matches!(op.as_str(), "List/retain" | "List/remove") {
                    self.program
                        .expressions
                        .get(*input)
                        .and_then(|input_expr| self.static_expr_type(input_expr, active_functions))
                } else if op == "List/append" {
                    let input_ty =
                        self.program.expressions.get(*input).and_then(|input_expr| {
                            self.static_expr_type(input_expr, active_functions)
                        });
                    let append_ty = args
                        .iter()
                        .find(|arg| arg.name.as_deref() == Some("item"))
                        .and_then(|arg| self.program.expressions.get(arg.value))
                        .and_then(|expr| self.static_expr_type(expr, active_functions));
                    match (input_ty, append_ty) {
                        (Some(Type::List(input_item)), Some(item_ty)) => Some(Type::List(
                            Box::new(widen_structural_type(&input_item, &item_ty)),
                        )),
                        (Some(input_ty), _) => Some(input_ty),
                        _ => None,
                    }
                } else if self.render_contracts.is_render_constructor(op) {
                    let input_ty =
                        self.program.expressions.get(*input).and_then(|input_expr| {
                            self.static_expr_type(input_expr, active_functions)
                        });
                    Some(self.static_render_constructor_type(op, input_ty, args, active_functions))
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
                Some(Type::Object(ObjectShape::from_ordered_fields(
                    fields.iter().map(|field| {
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
                    }),
                    false,
                )))
            }
            AstExprKind::TaggedObject { tag, fields } => {
                Some(Type::VariantSet(vec![Variant::Tagged {
                    tag: tag.clone(),
                    fields: ObjectShape::from_ordered_fields(
                        fields.iter().map(|field| {
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
                        }),
                        false,
                    ),
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
            AstExprKind::Identifier(value) => self.name_bindings.get(value).cloned(),
            AstExprKind::Path(parts) => {
                let path = parts.join(".");
                if let Some(access) = source_payload_access(&self.source_paths, parts) {
                    match access {
                        SourcePayloadAccess::Direct(source_path) => {
                            return source_payload_type_for_path(
                                &self.source_payload_types,
                                &source_path,
                            );
                        }
                        SourcePayloadAccess::Field(field) => {
                            return Some(source_payload_field_type(&field));
                        }
                        SourcePayloadAccess::UnknownField(_) => return None,
                    }
                }
                self.name_bindings.get(&path).cloned().or_else(|| {
                    parts
                        .first()
                        .and_then(|base| self.name_bindings.get(base))
                        .and_then(|base| type_for_nested_path(base, &parts[1..]))
                })
            }
            AstExprKind::Infix { op, .. }
                if matches!(op.as_str(), "==" | ">" | "<" | ">=" | "<=") =>
            {
                Some(true_false_type())
            }
            AstExprKind::Infix { .. } => Some(Type::Number),
            AstExprKind::Hold { initial, .. } => {
                let mut ty = self
                    .program
                    .expressions
                    .get(*initial)
                    .and_then(|expr| self.static_expr_type(expr, active_functions))?;
                for update_expr_id in hold_update_exprs_for_expr(
                    &self.program.ast.statements,
                    expr.id,
                    &self.program.expressions,
                ) {
                    if let Some(update_type) = self
                        .program
                        .expressions
                        .get(update_expr_id)
                        .and_then(|expr| self.static_expr_type(expr, active_functions))
                        && !matches!(update_type, Type::Skip)
                    {
                        ty = widen_structural_type(&ty, &update_type);
                    }
                }
                Some(ty)
            }
            AstExprKind::When { input } => self
                .static_when_result_type(expr.id, active_functions)
                .or_else(|| {
                    self.program
                        .expressions
                        .get(*input)
                        .and_then(|expr| self.static_expr_type(expr, active_functions))
                }),
            AstExprKind::Then { input, output } => output
                .or(Some(*input))
                .and_then(|expr_id| self.program.expressions.get(expr_id))
                .and_then(|expr| self.static_expr_type(expr, active_functions)),
            AstExprKind::MatchArm {
                output: Some(output),
                ..
            } => self
                .program
                .expressions
                .get(*output)
                .and_then(|expr| self.static_expr_type(expr, active_functions)),
            AstExprKind::MatchArm { output: None, .. } => Some(Type::Skip),
            AstExprKind::Source | AstExprKind::Latest => Some(exact_empty_object_type()),
            _ => None,
        }
    }

    fn static_when_result_type(
        &self,
        expr_id: usize,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let mut result = None;
        for arm_expr_id in when_arm_expr_ids(&self.program.ast.statements, expr_id) {
            let Some(arm_type) = self
                .program
                .expressions
                .get(arm_expr_id)
                .and_then(|expr| self.static_expr_type(expr, active_functions))
            else {
                continue;
            };
            result = Some(match result {
                Some(existing) => widen_structural_type(&existing, &arm_type),
                None => arm_type,
            });
        }
        result
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

    fn static_render_constructor_type(
        &self,
        function: &str,
        input_ty: Option<Type>,
        args: &[AstCallArg],
        active_functions: &mut BTreeSet<String>,
    ) -> Type {
        let mut fields = Vec::new();
        if let Some(input_ty) = input_ty
            && !matches!(input_ty, Type::Unknown)
        {
            fields.push(("input".to_owned(), input_ty));
        }
        for arg in args {
            let Some(name) = &arg.name else {
                continue;
            };
            let ty = self
                .program
                .expressions
                .get(arg.value)
                .and_then(|expr| self.static_expr_type(expr, active_functions))
                .unwrap_or_else(open_object_type);
            fields.push((name.clone(), ty));
        }
        self.render_contracts.constructor_shape(function, fields)
    }

    fn user_function_return_type(
        &self,
        function: &str,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        if !active_functions.insert(function.to_owned()) {
            return None;
        }
        let result = find_function_statement(&self.program.ast.statements, function)
            .and_then(|statement| self.function_body_return_type(statement, active_functions));
        active_functions.remove(function);
        result
    }

    fn function_body_return_type(
        &self,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        if let Some(renderable) = statement.children.iter().find_map(|child| {
            self.static_statement_type(child, active_functions)
                .filter(type_contains_renderable)
        }) {
            return Some(renderable);
        }
        let mut fields = BTreeMap::new();
        let mut field_order = Vec::new();
        self.collect_static_statement_fields(
            &statement.children,
            active_functions,
            &mut fields,
            &mut field_order,
        );
        (!fields.is_empty()).then_some(Type::Object(ObjectShape {
            fields,
            field_order,
            open: false,
        }))
    }

    fn collect_static_statement_fields(
        &self,
        statements: &[AstStatement],
        active_functions: &mut BTreeSet<String>,
        fields: &mut BTreeMap<String, Type>,
        field_order: &mut Vec<String>,
    ) {
        for statement in statements {
            if let Some(field) = statement_output_name(statement)
                && field != "document"
                && let Some(ty) = self.static_statement_type(statement, active_functions)
            {
                insert_ordered_shape_field(fields, field_order, field, ty);
            } else {
                self.collect_static_statement_fields(
                    &statement.children,
                    active_functions,
                    fields,
                    field_order,
                );
            }
        }
    }

    fn static_statement_type(
        &self,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        match &statement.kind {
            AstStatementKind::Source { .. } => Some(source_statement_value_type(
                statement,
                &self.source_payload_shape_table,
            )),
            AstStatementKind::List { .. } => {
                self.static_list_statement_type(statement, active_functions)
            }
            _ => direct_statement_value_expr_id(statement, &self.program.expressions)
                .and_then(|expr_id| self.program.expressions.get(expr_id))
                .and_then(|expr| self.static_expr_type(expr, active_functions))
                .or_else(|| {
                    let mut fields = BTreeMap::new();
                    let mut field_order = Vec::new();
                    self.collect_static_statement_fields(
                        &statement.children,
                        active_functions,
                        &mut fields,
                        &mut field_order,
                    );
                    (!fields.is_empty()).then_some(Type::Object(ObjectShape {
                        fields,
                        field_order,
                        open: false,
                    }))
                }),
        }
    }

    fn static_list_statement_type(
        &self,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let mut item_type = None;
        for child in &statement.children {
            let Some(expr_id) = child.expr else {
                continue;
            };
            let Some(expr) = self.program.expressions.get(expr_id) else {
                continue;
            };
            let ty = self.static_expr_type(expr, active_functions)?;
            item_type = Some(match item_type {
                Some(existing) => widen_structural_type(&existing, &ty),
                None => ty,
            });
        }
        Some(Type::List(Box::new(
            item_type.unwrap_or_else(|| unresolved_shape("empty list item")),
        )))
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
            AstExprKind::Call { args, .. } => args
                .iter()
                .map(|arg| self.flow_mode_for_expr_id(arg.value))
                .fold(FlowMode::Continuous, merge_flow_modes),
            AstExprKind::Pipe { input, op, args } => {
                if op == "WHILE" {
                    FlowMode::Continuous
                } else if op == "List/map" || op == "WHEN" {
                    self.flow_mode_for_expr_id(*input)
                } else {
                    args.iter()
                        .map(|arg| self.flow_mode_for_expr_id(arg.value))
                        .chain(std::iter::once(self.flow_mode_for_expr_id(*input)))
                        .fold(FlowMode::Continuous, merge_flow_modes)
                }
            }
            AstExprKind::When { input } => self.flow_mode_for_expr_id(*input),
            AstExprKind::Hold { .. } => FlowMode::Continuous,
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

    fn flow_mode_for_expr_id(&self, expr_id: usize) -> FlowMode {
        self.program
            .expressions
            .get(expr_id)
            .map(|expr| self.flow_mode_for_expr(expr))
            .unwrap_or(FlowMode::Continuous)
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
        } else if !child_types.is_empty() && child_types.iter().all(is_renderable_type) {
            renderable_contract_type()
        } else if let Some(first) = child_types.first().cloned() {
            child_types
                .iter()
                .skip(1)
                .fold(first, |existing, ty| widen_structural_type(&existing, ty))
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
        let item_type = self.list_map_result_item_type(args);
        let input_list_type = self.ensure_expr(list_expr_id).ty;
        let input_item_type =
            list_item_type_from_list_type(&input_list_type).unwrap_or_else(open_object_type);
        self.list_map_bindings.push(ListMapBinding {
            map_expr_id,
            list_expr_id,
            input_list_type,
            item_expr_id,
            item_binding_name,
            item_type: input_item_type,
            result_type: Type::List(Box::new(item_type)),
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
            format!(
                "`{operator}` expects `True` or `False`\nexpected: BOOL\nfound: {}",
                boon_facing_type_label(&input_flow.ty)
            ),
        ));
    }

    fn check_pattern_constraints(&mut self, statement: &AstStatement) {
        let Some(expr_id) = statement.expr else {
            return;
        };
        let Some(selector_expr_id) = pattern_selector_expr_id(expr_id, &self.program.expressions)
        else {
            return;
        };
        let selector_type = self.ensure_expr(selector_expr_id).ty;
        self.constraints.push(Constraint::PatternCovers { expr_id });
        for arm_expr_id in statement.children.iter().filter_map(|child| child.expr) {
            let Some(AstExpr {
                kind: AstExprKind::MatchArm { pattern, .. },
                ..
            }) = self.program.expressions.get(arm_expr_id)
            else {
                continue;
            };
            if let Some(variant) = pattern_variant(pattern) {
                self.constraints.push(Constraint::HasVariant {
                    value: selector_type.clone(),
                    variant,
                });
            }
        }
    }

    fn check_user_function_arguments(
        &mut self,
        expr_id: usize,
        function: &str,
        pipe_input: Option<usize>,
        call_args: &[AstCallArg],
    ) {
        let Some(requirements) = self.function_param_requirements.get(function).cloned() else {
            return;
        };
        let Some(function_statement) =
            find_function_statement(&self.program.ast.statements, function)
        else {
            return;
        };
        let AstStatementKind::Function {
            args: function_args,
            ..
        } = &function_statement.kind
        else {
            return;
        };
        for (param, expected) in requirements {
            let Some(actual_expr_id) =
                function_call_argument_expr(function_args, &param, pipe_input, call_args)
            else {
                continue;
            };
            let actual = self.ensure_expr(actual_expr_id).ty;
            self.constraints.push(Constraint::Assignable {
                actual: actual.clone(),
                expected: expected.clone(),
            });
            if type_is_assignable_to(&actual, &expected) {
                continue;
            }
            let message = if let Some(field) = missing_field_name(&actual, &expected) {
                format!(
                    "object is missing field `{field}`\nexpected: {}\nfound: {}",
                    boon_facing_type_label(&expected),
                    boon_facing_type_label(&actual)
                )
            } else if let Some(field) = incompatible_field_name(&actual, &expected) {
                format!(
                    "object field `{field}` has incompatible type\nexpected: {}\nfound: {}",
                    boon_facing_type_label(&expected),
                    boon_facing_type_label(&actual)
                )
            } else {
                format!(
                    "`FUNCTION {function}` argument `{param}` does not satisfy the required structural shape\nexpected: {}\nfound: {}",
                    boon_facing_type_label(&expected),
                    boon_facing_type_label(&actual)
                )
            };
            let diagnostic_expr_id = if self.program.expressions.get(actual_expr_id).is_some() {
                actual_expr_id
            } else {
                expr_id
            };
            self.diagnostics
                .push(self.diagnostic_for_expr(diagnostic_expr_id, message));
        }
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
                    format!(
                        "`HOLD` update must match the held value type\nexpected: {}\nfound: {}",
                        boon_facing_type_label(&initial_type),
                        boon_facing_type_label(&update_type)
                    ),
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
        for branch_expr_id in statement
            .children
            .iter()
            .flat_map(|child| statement_update_value_exprs(child, &self.program.expressions))
        {
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
                    format!(
                        "`LATEST` branches must produce compatible data types\nexpected: {}\nfound: {}",
                        boon_facing_type_label(expected),
                        boon_facing_type_label(&branch_type)
                    ),
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
    program: &ParsedProgram,
) -> Option<MappedChildren> {
    let expressions = &program.expressions;
    if let Some(expr_id) = statement.expr
        && let Some(mapped) = mapped_children_expr(expr_id, expressions, None)
            .or_else(|| mapped_children_function_call(expr_id, program))
    {
        return Some(mapped);
    }
    let mut previous_expr_id = statement.expr;
    for child in &statement.children {
        let Some(expr_id) = child.expr else {
            continue;
        };
        if let Some(mapped) = mapped_children_expr(expr_id, expressions, previous_expr_id)
            .or_else(|| mapped_children_function_call(expr_id, program))
        {
            return Some(mapped);
        }
        previous_expr_id = Some(expr_id);
    }
    None
}

fn mapped_children_function_call(
    expr_id: usize,
    program: &ParsedProgram,
) -> Option<MappedChildren> {
    let expr = program.expressions.get(expr_id)?;
    let (function, input, args) = match &expr.kind {
        AstExprKind::Call { function, args } => (function.as_str(), None, args.as_slice()),
        AstExprKind::Pipe { input, op, args } if op != "List/map" => {
            (op.as_str(), Some(*input), args.as_slice())
        }
        _ => return None,
    };
    let function_statement = find_function_statement(&program.ast.statements, function)?;
    let AstStatementKind::Function {
        args: function_args,
        ..
    } = &function_statement.kind
    else {
        return None;
    };
    let mut mapped = mapped_children_for_function_body(function_statement, &program.expressions)?;
    let list_parameter = expr_single_name(program.expressions.get(mapped.list_expr_id)?)?;
    mapped.list_expr_id = function_call_argument_expr(function_args, list_parameter, input, args)?;
    mapped.map_expr_id = expr_id;
    Some(mapped)
}

fn mapped_children_for_function_body(
    function_statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<MappedChildren> {
    let mut previous_expr_id = None;
    for child in &function_statement.children {
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

fn function_call_argument_expr(
    function_args: &[String],
    parameter: &str,
    pipe_input: Option<usize>,
    call_args: &[AstCallArg],
) -> Option<usize> {
    let position = function_args.iter().position(|arg| arg == parameter)?;
    if position == 0
        && let Some(input) = pipe_input
    {
        return Some(input);
    }
    call_args
        .iter()
        .find(|arg| arg.name.as_deref() == Some(parameter))
        .map(|arg| arg.value)
        .or_else(|| {
            let positional_index = if pipe_input.is_some() {
                position.checked_sub(1)?
            } else {
                position
            };
            call_args
                .iter()
                .filter(|arg| arg.name.is_none())
                .nth(positional_index)
                .map(|arg| arg.value)
        })
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

fn function_statement_args<'a>(program: &'a ParsedProgram, function: &str) -> Option<&'a [String]> {
    let statement = find_function_statement(&program.ast.statements, function)?;
    let AstStatementKind::Function { args, .. } = &statement.kind else {
        return None;
    };
    Some(args)
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

fn direct_statement_value_expr_id(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    statement.expr.or_else(|| {
        let expression_children = statement
            .children
            .iter()
            .filter_map(|child| {
                matches!(
                    child.kind,
                    AstStatementKind::Expression
                        | AstStatementKind::Hold { .. }
                        | AstStatementKind::List { field: None, .. }
                )
                .then(|| child.expr.or_else(|| first_child_expr_id(child)))
                .flatten()
            })
            .collect::<Vec<_>>();
        match expression_children.as_slice() {
            [] => None,
            [single] => Some(*single),
            many if expression_sequence_is_pipeline(many, expressions) => many.last().copied(),
            _ => None,
        }
    })
}

fn expression_sequence_is_pipeline(expr_ids: &[usize], expressions: &[AstExpr]) -> bool {
    expr_ids.len() > 1
        && expr_ids.iter().skip(1).all(|expr_id| {
            matches!(
                expressions.get(*expr_id).map(|expr| &expr.kind),
                Some(AstExprKind::Pipe { input, .. })
                    if expressions.get(*input).is_some_and(|expr| matches!(
                        expr.kind,
                        AstExprKind::Delimiter | AstExprKind::Unknown(_)
                    ))
            )
        })
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

fn list_map_new_expr_id(args: &[AstCallArg]) -> Option<usize> {
    args.iter()
        .find(|arg| arg.name.as_deref() == Some("new"))
        .map(|arg| arg.value)
}

fn pattern_selector_expr_id(expr_id: usize, expressions: &[AstExpr]) -> Option<usize> {
    match &expressions.get(expr_id)?.kind {
        AstExprKind::When { input } => Some(*input),
        AstExprKind::Pipe { input, op, .. } if op == "WHILE" => Some(*input),
        _ => None,
    }
}

fn pattern_variant(pattern: &[String]) -> Option<Variant> {
    let first = pattern
        .iter()
        .find(|part| !matches!(part.as_str(), "__" | "=>" | "{" | "}"))?;
    if !starts_uppercase_identifier(first) {
        return None;
    }
    Some(Variant::Tag(first.clone()))
}

fn starts_uppercase_identifier(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
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

fn statement_output_name(statement: &AstStatement) -> Option<String> {
    match &statement.kind {
        AstStatementKind::Field { name } => Some(name.clone()),
        AstStatementKind::List {
            field: Some(name), ..
        } => Some(name.clone()),
        AstStatementKind::Source {
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
                fields: ObjectShape::new(BTreeMap::new(), true),
            }])
        } else if render_contracts.is_render_constructor(function) {
            render_contracts.constructor_shape(function, BTreeMap::new())
        } else {
            Type::Unknown
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderContractRegistry {
    active_root: &'static str,
    roots: BTreeMap<&'static str, RuntimeRootContract>,
}

#[derive(Clone, Debug)]
pub struct RuntimeRootContract {
    renderable_kinds: BTreeSet<&'static str>,
    constructors: BTreeMap<&'static str, RenderConstructorContract>,
}

#[derive(Clone, Debug)]
struct RenderConstructorContract {
    kind: RenderConstructorKind,
}

#[derive(Clone, Debug)]
enum RenderConstructorKind {
    Fixed(&'static str),
    StripeDirection,
}

impl Default for RenderContractRegistry {
    fn default() -> Self {
        Self {
            active_root: "document",
            roots: [("document", RuntimeRootContract::document())]
                .into_iter()
                .collect(),
        }
    }
}

impl RuntimeRootContract {
    pub fn new(renderable_kinds: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            renderable_kinds: renderable_kinds.into_iter().collect(),
            constructors: BTreeMap::new(),
        }
    }

    pub fn with_fixed_constructor(mut self, function: &'static str, kind: &'static str) -> Self {
        self.constructors.insert(
            function,
            RenderConstructorContract {
                kind: RenderConstructorKind::Fixed(kind),
            },
        );
        self
    }

    pub fn with_stripe_direction_constructor(mut self, function: &'static str) -> Self {
        self.constructors.insert(
            function,
            RenderConstructorContract {
                kind: RenderConstructorKind::StripeDirection,
            },
        );
        self
    }

    fn document() -> Self {
        Self::new([
            "Button",
            "Checkbox",
            "Document",
            "Row",
            "Stack",
            "Text",
            "TextInput",
        ])
        .with_fixed_constructor("Document/new", "Document")
        .with_fixed_constructor("Element/container", "Stack")
        .with_stripe_direction_constructor("Element/stripe")
        .with_fixed_constructor("Element/text", "Text")
        .with_fixed_constructor("Element/label", "Text")
        .with_fixed_constructor("Element/paragraph", "Text")
        .with_fixed_constructor("Element/link", "Text")
        .with_fixed_constructor("Element/button", "Button")
        .with_fixed_constructor("Element/checkbox", "Checkbox")
        .with_fixed_constructor("Element/text_input", "TextInput")
    }
}

impl RenderContractRegistry {
    pub fn register_root(mut self, root: &'static str, contract: RuntimeRootContract) -> Self {
        self.roots.insert(root, contract);
        self
    }

    pub fn with_active_root(mut self, root: &'static str) -> Self {
        self.active_root = root;
        self
    }

    pub fn active_root(&self) -> &'static str {
        self.active_root
    }

    fn is_render_constructor(&self, function: &str) -> bool {
        self.roots
            .values()
            .any(|root| root.constructors.contains_key(function))
    }

    fn slot_contract(&self, slot_name: &str) -> &'static str {
        match slot_name {
            "items" | "children" => "LIST<[...]>",
            _ => "[...]",
        }
    }

    fn constructor_shape(
        &self,
        function: &str,
        fields: impl IntoIterator<Item = (String, Type)>,
    ) -> Type {
        let mut ordered_fields = fields.into_iter().collect::<Vec<_>>();
        let lookup_fields = ordered_fields.iter().cloned().collect::<BTreeMap<_, _>>();
        let kind = self
            .roots
            .get(self.active_root)
            .and_then(|root| root.constructors.get(function))
            .map(|contract| contract.kind_type(&lookup_fields))
            .unwrap_or_else(|| Type::VariantSet(vec![Variant::Tag("Renderable".to_owned())]));
        ordered_fields.push(("kind".to_owned(), kind));
        Type::Object(ObjectShape::from_ordered_fields(ordered_fields, false))
    }

    fn is_renderable_object_type(&self, ty: &Type) -> bool {
        let Type::Object(shape) = ty else {
            return false;
        };
        let Some(root) = self.roots.get(self.active_root) else {
            return false;
        };
        matches!(
            shape.fields.get("kind"),
            Some(Type::VariantSet(variants))
                if variants.iter().all(|variant| {
                    matches!(
                        variant,
                        Variant::Tag(tag) if root.renderable_kinds.contains(tag.as_str())
                    )
                })
        )
    }
}

impl RenderConstructorContract {
    fn kind_type(&self, fields: &BTreeMap<String, Type>) -> Type {
        match self.kind {
            RenderConstructorKind::Fixed(kind) => tag_type(kind),
            RenderConstructorKind::StripeDirection => stripe_kind_type(fields.get("direction")),
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

pub fn is_registered_render_constructor(function: &str) -> bool {
    RENDER_CONSTRUCTORS.contains(&function)
}

pub fn is_registered_element_constructor(function: &str) -> bool {
    is_registered_render_constructor(function) && function != "Document/new"
}

fn type_accepts_true_false(ty: &Type) -> bool {
    let Type::VariantSet(variants) = ty else {
        return false;
    };
    variants
        .iter()
        .all(|variant| matches!(variant, Variant::Tag(tag) if tag == "True" || tag == "False"))
}

fn variants_are_bool_alias(variants: &[Variant]) -> bool {
    let mut tags = Vec::new();
    for variant in variants {
        let Variant::Tag(tag) = variant else {
            return false;
        };
        tags.push(tag.as_str());
    }
    tags.sort_unstable();
    tags.dedup();
    tags == ["False", "True"]
}

pub fn boon_facing_type_label(ty: &Type) -> String {
    boon_facing_type_label_with_depth(ty, 0, false, 12)
}

pub fn boon_facing_type_detail_label(ty: &Type) -> String {
    boon_facing_type_label_with_depth(ty, 0, false, 12)
}

pub fn boon_facing_type_compact_label(ty: &Type) -> String {
    boon_facing_type_label_with_depth(ty, 0, true, 4)
}

fn sorted_variants(variants: &[Variant]) -> Vec<Variant> {
    let mut sorted = variants.to_vec();
    sorted.sort_by_key(variant_sort_key);
    sorted.dedup();
    sorted
}

fn variant_sort_key(variant: &Variant) -> String {
    match variant {
        Variant::Tag(tag) => format!("0:{tag}"),
        Variant::Tagged { tag, fields } => format!("1:{tag}:{}", fields.fields.len()),
    }
}

fn boon_facing_type_label_with_depth(
    ty: &Type,
    depth: usize,
    compact: bool,
    max_depth: usize,
) -> String {
    if depth >= max_depth {
        return "VALUE".to_owned();
    }
    match ty {
        Type::Text => "TEXT".to_owned(),
        Type::Number => "NUMBER".to_owned(),
        Type::Skip => "ABSENT".to_owned(),
        Type::RenderContract => document_render_contract_label(compact),
        Type::Unknown | Type::Var(_) => "VALUE".to_owned(),
        Type::UnresolvedShape { reason } => {
            if reason.is_empty() {
                "VALUE".to_owned()
            } else {
                format!("VALUE ({reason})")
            }
        }
        Type::List(item) => {
            format!(
                "LIST<{}>",
                boon_facing_type_label_with_depth(item, depth + 1, compact, max_depth)
            )
        }
        Type::Function { args, result } => {
            let args = args
                .iter()
                .map(|arg| boon_facing_type_label_with_depth(arg, depth + 1, compact, max_depth))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "FUNCTION({args}) -> {}",
                boon_facing_type_label_with_depth(&result.ty, depth + 1, compact, max_depth)
            )
        }
        Type::Object(shape) => {
            if shape.fields.is_empty() {
                return if shape.open {
                    if compact {
                        "[...]".to_owned()
                    } else {
                        "VALUE".to_owned()
                    }
                } else {
                    "[]".to_owned()
                };
            }
            if compact && shape.fields.len() > 2 {
                return "[...]".to_owned();
            }
            object_shape_label(shape, depth, compact, max_depth)
        }
        Type::VariantSet(variants) => {
            let variants = sorted_variants(variants);
            if variants.is_empty() {
                return "VALUE".to_owned();
            }
            if variants_are_bool_alias(&variants) {
                return "BOOL".to_owned();
            }
            if variants
                .iter()
                .all(|variant| matches!(variant, Variant::Tag(_)))
            {
                let tags = variants
                    .iter()
                    .filter_map(|variant| match variant {
                        Variant::Tag(tag) => Some(tag.clone()),
                        Variant::Tagged { .. } => None,
                    })
                    .collect::<Vec<_>>();
                return tags.join(" | ");
            }
            let labels = variants
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) => tag.clone(),
                    Variant::Tagged { tag, fields } => {
                        tagged_object_shape_label(tag, fields, depth, compact, max_depth)
                    }
                })
                .collect::<Vec<_>>();
            labels.join(" | ")
        }
    }
}

fn document_render_contract_label(compact: bool) -> String {
    if compact {
        "[...]".to_owned()
    } else {
        "[
    kind: Button | Checkbox | Document | Row | Stack | Text | TextInput
]"
        .to_owned()
    }
}

fn object_shape_label(
    shape: &ObjectShape,
    depth: usize,
    compact: bool,
    max_depth: usize,
) -> String {
    if compact {
        let fields = shape
            .ordered_fields()
            .into_iter()
            .map(|(field, ty)| {
                format!(
                    "{field}: {}",
                    boon_facing_type_label_with_depth(ty, depth + 1, true, max_depth)
                )
            })
            .collect::<Vec<_>>();
        return format!("[{}]", fields.join(", "));
    }
    let indent = " ".repeat((depth + 1) * 4);
    let closing_indent = " ".repeat(depth * 4);
    let fields = shape
        .ordered_fields()
        .into_iter()
        .map(|(field, ty)| {
            let value = boon_facing_type_label_with_depth(ty, depth + 1, false, max_depth);
            if value.contains('\n') {
                let mut lines = value.lines();
                let first = lines.next().unwrap_or_default();
                let rest = lines.map(str::to_owned).collect::<Vec<_>>().join("\n");
                if rest.is_empty() {
                    format!("{indent}{field}: {first}")
                } else {
                    format!("{indent}{field}: {first}\n{rest}")
                }
            } else {
                format!("{indent}{field}: {value}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("[\n{fields}\n{closing_indent}]")
}

fn tagged_object_shape_label(
    tag: &str,
    fields: &ObjectShape,
    depth: usize,
    compact: bool,
    max_depth: usize,
) -> String {
    if fields.fields.is_empty() && !fields.open {
        return format!("{tag}[]");
    }
    if compact && (fields.open || fields.fields.len() > 2) {
        return format!("{tag}[...]");
    }
    let object = object_shape_label(fields, depth, compact, max_depth);
    format!("{tag}{object}")
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
        (Type::UnresolvedShape { .. }, _) | (_, Type::UnresolvedShape { .. }) => false,
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

fn merge_flow_modes(left: FlowMode, right: FlowMode) -> FlowMode {
    match (left, right) {
        (FlowMode::Absent, _) | (_, FlowMode::Absent) => FlowMode::Absent,
        (FlowMode::PresentOrAbsent, _) | (_, FlowMode::PresentOrAbsent) => {
            FlowMode::PresentOrAbsent
        }
        (FlowMode::TickPresent, _) | (_, FlowMode::TickPresent) => FlowMode::TickPresent,
        (FlowMode::Continuous, FlowMode::Continuous) => FlowMode::Continuous,
    }
}

fn type_is_assignable_to(actual: &Type, expected: &Type) -> bool {
    match (actual, expected) {
        (_, Type::Unknown) | (Type::Unknown, _) | (Type::Var(_), _) | (_, Type::Var(_)) => true,
        (Type::UnresolvedShape { .. }, _) | (_, Type::UnresolvedShape { .. }) => true,
        (_, expected) if is_open_object_type(expected) => true,
        (actual, _) if is_open_object_type(actual) => true,
        (Type::Text, Type::Text) | (Type::Number, Type::Number) => true,
        (actual, expected) if type_accepts_true_false(expected) => type_accepts_true_false(actual),
        (Type::RenderContract, Type::RenderContract) => true,
        (actual, Type::RenderContract) => is_renderable_type(actual),
        (Type::List(actual), Type::List(expected)) => type_is_assignable_to(actual, expected),
        (Type::Object(actual), Type::Object(expected)) => {
            expected.fields.iter().all(|(field, expected_field)| {
                actual
                    .fields
                    .get(field)
                    .is_some_and(|actual_field| type_is_assignable_to(actual_field, expected_field))
                    || actual.open
            })
        }
        (Type::VariantSet(actual), Type::VariantSet(expected)) => expected.iter().all(|expected| {
            actual
                .iter()
                .any(|actual| variant_is_assignable_to(actual, expected))
        }),
        _ => false,
    }
}

fn render_field_type_accepts(actual: &Type, expected: &Type) -> bool {
    if is_open_object_type(expected) {
        return matches!(
            actual,
            Type::Object(_) | Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
        );
    }
    type_is_assignable_to(actual, expected)
}

fn variant_is_assignable_to(actual: &Variant, expected: &Variant) -> bool {
    match (actual, expected) {
        (Variant::Tag(actual), Variant::Tag(expected)) => actual == expected,
        (
            Variant::Tagged {
                tag: actual_tag,
                fields: actual_fields,
            },
            Variant::Tagged {
                tag: expected_tag,
                fields: expected_fields,
            },
        ) => {
            actual_tag == expected_tag
                && type_is_assignable_to(
                    &Type::Object(actual_fields.clone()),
                    &Type::Object(expected_fields.clone()),
                )
        }
        _ => false,
    }
}

fn missing_field_name(actual: &Type, expected: &Type) -> Option<String> {
    let (Type::Object(actual), Type::Object(expected)) = (actual, expected) else {
        return None;
    };
    expected.fields.iter().find_map(|(field, expected_field)| {
        let Some(actual_field) = actual.fields.get(field) else {
            return (!actual.open).then(|| field.clone());
        };
        missing_field_name(actual_field, expected_field).map(|nested| format!("{field}.{nested}"))
    })
}

fn incompatible_field_name(actual: &Type, expected: &Type) -> Option<String> {
    let (Type::Object(actual), Type::Object(expected)) = (actual, expected) else {
        return None;
    };
    expected.fields.iter().find_map(|(field, expected_field)| {
        let actual_field = actual.fields.get(field)?;
        if let Some(nested) = incompatible_field_name(actual_field, expected_field) {
            return Some(format!("{field}.{nested}"));
        }
        (!type_is_assignable_to(actual_field, expected_field)).then(|| field.clone())
    })
}

fn hold_update_exprs(statement: &AstStatement, expressions: &[AstExpr]) -> Vec<usize> {
    let mut updates = Vec::new();
    collect_hold_update_exprs(statement, expressions, &mut updates);
    updates
}

fn hold_update_exprs_for_expr(
    statements: &[AstStatement],
    expr_id: usize,
    expressions: &[AstExpr],
) -> Vec<usize> {
    for statement in statements {
        if statement.expr == Some(expr_id) {
            return hold_update_exprs(statement, expressions);
        }
        let nested = hold_update_exprs_for_expr(&statement.children, expr_id, expressions);
        if !nested.is_empty() {
            return nested;
        }
    }
    Vec::new()
}

fn when_arm_expr_ids(statements: &[AstStatement], expr_id: usize) -> Vec<usize> {
    for statement in statements {
        if statement.expr == Some(expr_id) {
            return statement
                .children
                .iter()
                .filter_map(|child| child.expr)
                .collect();
        }
        let nested = when_arm_expr_ids(&statement.children, expr_id);
        if !nested.is_empty() {
            return nested;
        }
    }
    Vec::new()
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
                updates.extend(statement_update_value_exprs(update, expressions));
            }
        }
    }
}

fn statement_update_value_exprs(statement: &AstStatement, expressions: &[AstExpr]) -> Vec<usize> {
    if let Some(expr_id) = statement.expr {
        if let Some(AstExprKind::Then {
            output: Some(output),
            ..
        }) = expressions.get(expr_id).map(|expr| &expr.kind)
        {
            return vec![*output];
        }
        if matches!(
            expressions.get(expr_id).map(|expr| &expr.kind),
            Some(AstExprKind::Then { output: None, .. })
        ) {
            let nested = statement
                .children
                .iter()
                .flat_map(|child| statement_update_value_exprs(child, expressions))
                .collect::<Vec<_>>();
            if !nested.is_empty() {
                return nested;
            }
        }
        return vec![expr_id];
    }
    statement
        .children
        .iter()
        .flat_map(|child| statement_update_value_exprs(child, expressions))
        .collect()
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
                } else if direct_statement_value_expr_id(statement, expressions).is_none()
                    && !statement.children.is_empty()
                {
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
    ObjectShape::from_ordered_fields(
        statement.children.iter().filter_map(|child| {
            let field = statement_field(child)?;
            let ty =
                simple_statement_value_type(child, expressions).unwrap_or_else(open_object_type);
            Some((field, ty))
        }),
        true,
    )
}

fn simple_list_statement_type(statement: &AstStatement, expressions: &[AstExpr]) -> Type {
    let mut item_type = None;
    for child in &statement.children {
        let Some(expr_id) = child.expr else {
            continue;
        };
        let Some(expr) = expressions.get(expr_id) else {
            continue;
        };
        let ty = simple_expr_type(expr, expressions);
        item_type = Some(match item_type {
            Some(existing) => widen_structural_type(&existing, &ty),
            None => ty,
        });
    }
    Type::List(Box::new(item_type.unwrap_or_else(open_object_type)))
}

fn simple_statement_value_type(statement: &AstStatement, expressions: &[AstExpr]) -> Option<Type> {
    let expr_id = direct_statement_value_expr_id(statement, expressions)?;
    let expr = expressions.get(expr_id)?;
    Some(match &expr.kind {
        AstExprKind::Hold { initial, .. } => {
            let hold_statement = statement_for_expr(statement, expr_id).unwrap_or(statement);
            simple_hold_result_type(hold_statement, *initial, expressions)
        }
        AstExprKind::Pipe { input, op, .. } if op == "HOLD" => {
            let hold_statement = statement_for_expr(statement, expr_id).unwrap_or(statement);
            simple_hold_result_type(hold_statement, *input, expressions)
        }
        _ => simple_expr_type(expr, expressions),
    })
}

fn statement_for_expr(statement: &AstStatement, expr_id: usize) -> Option<&AstStatement> {
    if statement.expr == Some(expr_id) {
        return Some(statement);
    }
    statement
        .children
        .iter()
        .find_map(|child| statement_for_expr(child, expr_id))
}

fn simple_hold_result_type(
    statement: &AstStatement,
    initial: usize,
    expressions: &[AstExpr],
) -> Type {
    let mut ty = expressions
        .get(initial)
        .map(|expr| simple_expr_type(expr, expressions))
        .unwrap_or_else(open_object_type);
    for update_expr_id in hold_update_exprs(statement, expressions) {
        let update_type = expressions
            .get(update_expr_id)
            .map(|expr| simple_expr_type(expr, expressions))
            .unwrap_or_else(open_object_type);
        if !matches!(update_type, Type::Skip) {
            ty = widen_structural_type(&ty, &update_type);
        }
    }
    ty
}

fn object_shape_for_expr(expr_id: usize, expressions: &[AstExpr]) -> Option<ObjectShape> {
    let fields = match &expressions.get(expr_id)?.kind {
        AstExprKind::Object(fields) | AstExprKind::Record(fields) => fields,
        _ => return None,
    };
    Some(ObjectShape::from_ordered_fields(
        fields.iter().map(|field| {
            (
                field.name.clone(),
                expressions
                    .get(field.value)
                    .map(|expr| simple_expr_type(expr, expressions))
                    .unwrap_or_else(open_object_type),
            )
        }),
        false,
    ))
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
        AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
            Type::Object(ObjectShape::from_ordered_fields(
                fields.iter().map(|field| {
                    (
                        field.name.clone(),
                        expressions
                            .get(field.value)
                            .map(|expr| simple_expr_type(expr, expressions))
                            .unwrap_or_else(open_object_type),
                    )
                }),
                false,
            ))
        }
        AstExprKind::ListLiteral { .. } => Type::List(Box::new(open_object_type())),
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if matches!(
                function.as_str(),
                "List/count" | "List/sum" | "Text/find" | "Text/length" | "Text/to_number"
            ) =>
        {
            Type::Number
        }
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if matches!(
                function.as_str(),
                "Text/empty" | "Text/trim" | "Text/concat" | "Text/substring" | "Error/text"
            ) =>
        {
            Type::Text
        }
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if function == "List/chunk" =>
        {
            Type::List(Box::new(Type::Object(ObjectShape::from_ordered_fields(
                [
                    ("row_number".to_owned(), Type::Text),
                    ("cells".to_owned(), Type::List(Box::new(open_object_type()))),
                ]
                .into_iter(),
                true,
            ))))
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
        AstExprKind::Hold { initial, .. } => expressions
            .get(*initial)
            .map(|expr| simple_expr_type(expr, expressions))
            .unwrap_or_else(open_object_type),
        AstExprKind::Then { input, output } => output
            .or(Some(*input))
            .and_then(|expr_id| expressions.get(expr_id))
            .map(|expr| simple_expr_type(expr, expressions))
            .unwrap_or_else(open_object_type),
        AstExprKind::Pipe { input, op, .. } if op == "HOLD" || op == "WHILE" => expressions
            .get(*input)
            .map(|expr| simple_expr_type(expr, expressions))
            .unwrap_or_else(open_object_type),
        AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => open_object_type(),
        AstExprKind::Call { function, .. } if is_registered_render_constructor(function) => {
            RenderContractRegistry::default().constructor_shape(function, BTreeMap::new())
        }
        _ => open_object_type(),
    }
}

fn function_param_requirements(
    program: &ParsedProgram,
) -> BTreeMap<String, BTreeMap<String, Type>> {
    let mut requirements = BTreeMap::new();
    collect_function_param_requirements(
        &program.ast.statements,
        &program.expressions,
        &mut requirements,
    );
    requirements
}

fn collect_function_param_requirements(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    requirements: &mut BTreeMap<String, BTreeMap<String, Type>>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, args } = &statement.kind {
            let params = args.iter().cloned().collect::<BTreeSet<_>>();
            let function_requirements = requirements.entry(name.clone()).or_default();
            for child in &statement.children {
                collect_param_requirements_statement(
                    child,
                    expressions,
                    &params,
                    function_requirements,
                );
            }
        }
        collect_function_param_requirements(&statement.children, expressions, requirements);
    }
}

fn collect_param_requirements_statement(
    statement: &AstStatement,
    expressions: &[AstExpr],
    params: &BTreeSet<String>,
    requirements: &mut BTreeMap<String, Type>,
) {
    if let Some(expr_id) = statement.expr {
        collect_param_requirements_expr(expr_id, expressions, params, requirements, None);
        if let Some(function) = render_constructor_for_expr(expr_id, expressions) {
            for child in &statement.children {
                let Some(field) = statement_field(child) else {
                    continue;
                };
                let Some(expected) = render_arg_expected_type(function, Some(&field)) else {
                    continue;
                };
                let Some(value_expr) = direct_statement_value_expr_id(child, expressions) else {
                    continue;
                };
                collect_param_requirements_expr(
                    value_expr,
                    expressions,
                    params,
                    requirements,
                    Some(expected),
                );
            }
        }
    }
    for child in &statement.children {
        collect_param_requirements_statement(child, expressions, params, requirements);
    }
}

fn render_constructor_for_expr(expr_id: usize, expressions: &[AstExpr]) -> Option<&str> {
    match &expressions.get(expr_id)?.kind {
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if is_registered_render_constructor(function) =>
        {
            Some(function.as_str())
        }
        _ => None,
    }
}

fn collect_param_requirements_expr(
    expr_id: usize,
    expressions: &[AstExpr],
    params: &BTreeSet<String>,
    requirements: &mut BTreeMap<String, Type>,
    expected: Option<Type>,
) {
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Identifier(name) if params.contains(name) => {
            if let Some(expected) = expected {
                add_param_requirement(requirements, name, expected);
            }
        }
        AstExprKind::Path(parts) if parts.len() >= 2 && params.contains(&parts[0]) => {
            add_param_requirement(
                requirements,
                &parts[0],
                object_type_for_path_requirement(&parts[1..], expected),
            );
        }
        AstExprKind::Call { function, args } => {
            let arg_expected = argument_expected_type(function);
            for arg in args {
                let expected = render_arg_expected_type(function, arg.name.as_deref())
                    .or_else(|| arg_expected.clone());
                collect_param_requirements_expr(
                    arg.value,
                    expressions,
                    params,
                    requirements,
                    expected,
                );
            }
        }
        AstExprKind::Pipe { input, op, args } => {
            let input_expected = pipe_input_expected_type(op);
            collect_param_requirements_expr(
                *input,
                expressions,
                params,
                requirements,
                input_expected,
            );
            let arg_expected = argument_expected_type(op);
            for arg in args {
                let expected = render_arg_expected_type(op, arg.name.as_deref())
                    .or_else(|| arg_expected.clone());
                collect_param_requirements_expr(
                    arg.value,
                    expressions,
                    params,
                    requirements,
                    expected,
                );
            }
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            collect_param_requirements_expr(*initial, expressions, params, requirements, expected);
        }
        AstExprKind::Then { input, output } => {
            collect_param_requirements_expr(*input, expressions, params, requirements, None);
            if let Some(output) = output {
                collect_param_requirements_expr(
                    *output,
                    expressions,
                    params,
                    requirements,
                    expected,
                );
            }
        }
        AstExprKind::Infix { left, right, op } => {
            let expected = if matches!(op.as_str(), "-" | "*" | "/" | ">" | "<" | ">=" | "<=") {
                Some(Type::Number)
            } else {
                None
            };
            collect_param_requirements_expr(
                *left,
                expressions,
                params,
                requirements,
                expected.clone(),
            );
            collect_param_requirements_expr(*right, expressions, params, requirements, expected);
        }
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => collect_param_requirements_expr(*output, expressions, params, requirements, expected),
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                collect_param_requirements_expr(
                    field.value,
                    expressions,
                    params,
                    requirements,
                    None,
                );
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

fn add_param_requirement(requirements: &mut BTreeMap<String, Type>, param: &str, expected: Type) {
    requirements
        .entry(param.to_owned())
        .and_modify(|existing| *existing = widen_structural_type(existing, &expected))
        .or_insert(expected);
}

fn object_type_for_path_requirement(parts: &[String], leaf_type: Option<Type>) -> Type {
    let Some((field, rest)) = parts.split_first() else {
        return leaf_type.unwrap_or_else(open_object_type);
    };
    let field_type = if rest.is_empty() {
        leaf_type.unwrap_or_else(open_object_type)
    } else {
        object_type_for_path_requirement(rest, leaf_type)
    };
    Type::Object(ObjectShape::from_ordered_fields(
        [(field.clone(), field_type)].into_iter(),
        true,
    ))
}

fn pipe_input_expected_type(function: &str) -> Option<Type> {
    if function == "List/map" || matches!(function, "List/retain" | "List/count") {
        Some(Type::List(Box::new(open_object_type())))
    } else if function.starts_with("Text/") {
        Some(Type::Text)
    } else if function.starts_with("Number/") {
        Some(Type::Number)
    } else if function == "Bool/not" || function == "Bool/and" {
        Some(true_false_type())
    } else {
        None
    }
}

fn argument_expected_type(function: &str) -> Option<Type> {
    if function == "Bool/not" || function == "Bool/and" {
        Some(true_false_type())
    } else if function.starts_with("Text/") {
        Some(Type::Text)
    } else if function.starts_with("Number/") {
        Some(Type::Number)
    } else {
        None
    }
}

fn render_arg_expected_type(function: &str, arg_name: Option<&str>) -> Option<Type> {
    if !is_registered_render_constructor(function) {
        return None;
    }
    match arg_name {
        Some("input" | "root") => Some(Type::RenderContract),
        Some("items" | "children") => Some(Type::List(Box::new(Type::RenderContract))),
        Some(
            "label" | "text" | "value" | "display_value" | "edit_value" | "placeholder" | "target",
        ) => Some(Type::Text),
        Some("checked" | "visible" | "selected" | "focus") => Some(true_false_type()),
        _ => None,
    }
}

fn render_arg_should_validate_directly(_function: &str, arg_name: &str) -> bool {
    matches!(
        arg_name,
        "input" | "root" | "items" | "children" | "checked" | "visible" | "selected" | "focus"
    )
}

fn name_bindings(
    program: &ParsedProgram,
    source_payload_types: &BTreeMap<String, Type>,
    function_param_requirements: &BTreeMap<String, BTreeMap<String, Type>>,
) -> BTreeMap<String, Type> {
    let mut bindings = BTreeMap::new();
    collect_name_bindings(
        &program.ast.statements,
        &program.expressions,
        &mut Vec::new(),
        source_payload_types,
        function_param_requirements,
        &mut bindings,
    );
    collect_row_scope_bindings(program, function_param_requirements, &mut bindings);
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
                if let Some(expr_id) = direct_statement_value_expr_id(statement, expressions)
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
        AstExprKind::When { input } => expressions
            .get(*input)
            .map(|expr| simple_flow_mode(expr, expressions))
            .unwrap_or(FlowMode::Continuous),
        AstExprKind::Pipe { input, op, .. } if op == "WHILE" => {
            let _ = input;
            FlowMode::Continuous
        }
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
    source_payload_types: &BTreeMap<String, Type>,
    function_param_requirements: &BTreeMap<String, BTreeMap<String, Type>>,
    bindings: &mut BTreeMap<String, Type>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Field { name } if name == "document" => continue,
            AstStatementKind::Field { name } => {
                let path = scoped_path(scope, name);
                let ty = simple_statement_value_type(statement, expressions).unwrap_or_else(|| {
                    Type::Object(object_shape_for_statement(statement, expressions))
                });
                bindings.insert(name.clone(), ty.clone());
                bindings.insert(path, ty);
                scope.push(name.clone());
                collect_name_bindings(
                    &statement.children,
                    expressions,
                    scope,
                    source_payload_types,
                    function_param_requirements,
                    bindings,
                );
                scope.pop();
            }
            AstStatementKind::Hold { name: Some(name) } => {
                if let Some(ty) = simple_statement_value_type(statement, expressions) {
                    bindings.insert(name.clone(), ty);
                }
                collect_name_bindings(
                    &statement.children,
                    expressions,
                    scope,
                    source_payload_types,
                    function_param_requirements,
                    bindings,
                );
            }
            AstStatementKind::List {
                field: Some(name), ..
            } => {
                let ty = simple_list_statement_type(statement, expressions);
                bindings.insert(name.clone(), ty.clone());
                bindings.insert(scoped_path(scope, name), ty);
                collect_name_bindings(
                    &statement.children,
                    expressions,
                    scope,
                    source_payload_types,
                    function_param_requirements,
                    bindings,
                );
            }
            AstStatementKind::Source {
                field: Some(name), ..
            } => {
                let source_path = scoped_path(scope, name);
                let ty = source_payload_type_for_path(source_payload_types, &source_path)
                    .unwrap_or_else(exact_empty_object_type);
                bindings.insert(name.clone(), ty.clone());
                bindings.insert(scoped_path(scope, name), ty);
                collect_name_bindings(
                    &statement.children,
                    expressions,
                    scope,
                    source_payload_types,
                    function_param_requirements,
                    bindings,
                );
            }
            AstStatementKind::Function { name, args } => {
                for arg in args {
                    let ty = function_param_requirements
                        .get(name)
                        .and_then(|requirements| requirements.get(arg))
                        .cloned()
                        .unwrap_or_else(|| unresolved_shape(format!("parameter `{arg}`")));
                    bindings
                        .entry(arg.clone())
                        .and_modify(|existing| *existing = widen_structural_type(existing, &ty))
                        .or_insert(ty);
                }
                collect_name_bindings(
                    &statement.children,
                    expressions,
                    scope,
                    source_payload_types,
                    function_param_requirements,
                    bindings,
                );
            }
            _ => collect_name_bindings(
                &statement.children,
                expressions,
                scope,
                source_payload_types,
                function_param_requirements,
                bindings,
            ),
        }
    }
}

fn collect_row_scope_bindings(
    program: &ParsedProgram,
    function_param_requirements: &BTreeMap<String, BTreeMap<String, Type>>,
    bindings: &mut BTreeMap<String, Type>,
) {
    bindings.insert("if".to_owned(), open_object_type());
    bindings.insert("when".to_owned(), open_object_type());
    for row_scope in &program.row_scope_functions {
        bindings
            .entry(row_scope.row_scope.clone())
            .or_insert_with(open_object_type);
        if let Some(item_ty) = canonical_row_scope_type(
            program,
            bindings,
            function_param_requirements,
            &row_scope.function,
            &row_scope.list,
            &row_scope.row_scope,
            function_result_shape(program, &row_scope.function).map(Type::Object),
        ) {
            if let Type::Object(shape) = &item_ty {
                for (field, ty) in shape.ordered_fields() {
                    bindings.insert(field.clone(), ty.clone());
                    bindings.insert(format!("{}.{}", row_scope.row_scope, field), ty.clone());
                }
            }
            bindings.insert(row_scope.row_scope.clone(), item_ty.clone());
            bindings
                .entry(row_scope.list.clone())
                .and_modify(|existing| {
                    *existing = Type::List(Box::new(item_ty.clone()));
                })
                .or_insert_with(|| Type::List(Box::new(item_ty)));
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
                    && !bindings.contains_key(name)
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

fn canonical_row_scope_type(
    program: &ParsedProgram,
    bindings: &BTreeMap<String, Type>,
    function_param_requirements: &BTreeMap<String, BTreeMap<String, Type>>,
    function: &str,
    list: &str,
    row_scope: &str,
    canonical_return: Option<Type>,
) -> Option<Type> {
    let mut row_type = canonical_return.filter(type_has_known_user_shape);
    let list_item_type = bindings
        .get(list)
        .and_then(|existing| match existing {
            Type::List(item) => Some((**item).clone()),
            _ => None,
        })
        .or_else(|| list_item_shape(program, list).map(Type::Object));

    if let Some(extra) = list_item_type.filter(type_has_known_user_shape) {
        row_type = Some(match row_type {
            Some(existing) => merge_canonical_row_type(&existing, &extra),
            None => extra,
        });
    }

    let requirement_type = function_param_requirements
        .get(function)
        .and_then(|requirements| requirements.get(row_scope))
        .cloned();
    if let Some(extra) = requirement_type.filter(type_has_known_user_shape) {
        row_type = Some(match row_type {
            Some(existing) => merge_canonical_row_type(&existing, &extra),
            None => extra,
        });
    }

    row_type
}

fn type_has_known_user_shape(ty: &Type) -> bool {
    match ty {
        Type::Unknown | Type::UnresolvedShape { .. } => false,
        Type::Object(shape) => !shape.fields.is_empty(),
        Type::List(item) => type_has_known_user_shape(item),
        _ => true,
    }
}

fn list_item_type_from_list_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::List(item) => Some((**item).clone()),
        _ => None,
    }
}

fn merge_canonical_row_type(canonical: &Type, extra: &Type) -> Type {
    if is_value_placeholder_type(canonical) {
        return extra.clone();
    }
    if is_value_placeholder_type(extra) {
        return canonical.clone();
    }
    match (canonical, extra) {
        (Type::Object(canonical_shape), Type::Object(extra_shape)) => {
            let mut fields = canonical_shape.fields.clone();
            for (field, extra_ty) in extra_shape.ordered_fields() {
                fields
                    .entry(field.clone())
                    .and_modify(|existing| {
                        *existing = merge_canonical_row_type(existing, extra_ty);
                    })
                    .or_insert_with(|| extra_ty.clone());
            }
            Type::Object(ObjectShape {
                fields,
                field_order: object_field_order_for_widened_shapes(canonical_shape, extra_shape),
                open: canonical_shape.open || extra_shape.open,
            })
        }
        (Type::List(canonical_item), Type::List(extra_item)) => Type::List(Box::new(
            merge_canonical_row_type(canonical_item, extra_item),
        )),
        _ => widen_structural_type(canonical, extra),
    }
}

fn is_value_placeholder_type(ty: &Type) -> bool {
    match ty {
        Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. } => true,
        Type::Object(shape) => shape.open && shape.fields.is_empty(),
        _ => false,
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
    let mut field_order = Vec::new();
    collect_statement_shape_fields(
        &function.children,
        &program.expressions,
        &mut fields,
        &mut field_order,
    );
    (!fields.is_empty()).then_some(ObjectShape {
        fields,
        field_order,
        open: true,
    })
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
    field_order: &mut Vec<String>,
) {
    for statement in statements {
        if let Some(field) = statement_field(statement) {
            let ty = simple_statement_value_type(statement, expressions).unwrap_or_else(|| {
                Type::Object(object_shape_for_statement(statement, expressions))
            });
            insert_ordered_shape_field(fields, field_order, field, ty);
        } else {
            collect_statement_shape_fields(&statement.children, expressions, fields, field_order);
        }
    }
}

fn list_item_shape(program: &ParsedProgram, list_name: &str) -> Option<ObjectShape> {
    if let Some(shape) = list_item_shape_from_field(program, list_name) {
        return Some(shape);
    }
    let list = find_list_statement(&program.ast.statements, list_name)?;
    let mut fields = BTreeMap::new();
    let mut field_order = Vec::new();
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
            insert_ordered_shape_field(&mut fields, &mut field_order, field.name.clone(), ty);
        }
    }
    (!fields.is_empty()).then_some(ObjectShape {
        fields,
        field_order,
        open: true,
    })
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
        if matches!(&statement.kind, AstStatementKind::Field { name } if name == list_name)
            && let Some(list) = statement
                .children
                .iter()
                .find(|child| matches!(child.kind, AstStatementKind::List { field: None, .. }))
        {
            return Some(list);
        }
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
            variants.sort_by_key(variant_sort_key);
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
                field_order: object_field_order_for_widened_shapes(left, right),
                open: left.open || right.open,
            })
        }
        _ => open_object_type(),
    }
}

fn object_field_order_for_widened_shapes(left: &ObjectShape, right: &ObjectShape) -> Vec<String> {
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for field in left.field_order.iter().chain(right.field_order.iter()) {
        if left.fields.contains_key(field) || right.fields.contains_key(field) {
            if seen.insert(field.as_str()) {
                order.push(field.clone());
            }
        }
    }
    for field in left.fields.keys().chain(right.fields.keys()) {
        if seen.insert(field.as_str()) {
            order.push(field.clone());
        }
    }
    order
}

fn insert_ordered_shape_field(
    fields: &mut BTreeMap<String, Type>,
    field_order: &mut Vec<String>,
    field: String,
    ty: Type,
) {
    if !fields.contains_key(&field) {
        field_order.push(field.clone());
    }
    fields
        .entry(field)
        .and_modify(|existing| *existing = widen_structural_type(existing, &ty))
        .or_insert(ty);
}

fn type_for_nested_path(base: &Type, parts: &[String]) -> Option<Type> {
    let Some((field, rest)) = parts.split_first() else {
        return Some(base.clone());
    };
    match base {
        Type::Object(shape) => {
            if let Some(field_ty) = shape.fields.get(field) {
                return type_for_nested_path(field_ty, rest);
            }
            if shape.open {
                return Some(open_object_type());
            }
            None
        }
        Type::UnresolvedShape { .. } | Type::Unknown | Type::Var(_) => Some(base.clone()),
        _ => None,
    }
}

enum SourcePayloadAccess {
    Direct(String),
    Field(String),
    UnknownField(String),
}

fn source_payload_access(
    source_paths: &BTreeSet<String>,
    parts: &[String],
) -> Option<SourcePayloadAccess> {
    let path = parts.join(".");
    for source_path in source_paths {
        let relative = source_path.strip_prefix("store.").unwrap_or(source_path);
        for base in [source_path.as_str(), relative] {
            if path == base || base.ends_with(&format!(".{path}")) {
                return Some(SourcePayloadAccess::Direct(source_path.clone()));
            }
            if let Some(suffix) = path.strip_prefix(&format!("{base}.")) {
                return Some(source_payload_access_for_suffix(suffix));
            }
            if let Some((field, base_without_field)) = parts.split_last() {
                let base_without_field = base_without_field.join(".");
                if !base_without_field.is_empty()
                    && base.ends_with(&format!(".{base_without_field}"))
                {
                    return Some(source_payload_access_for_suffix(field));
                }
            }
        }
    }
    None
}

fn source_payload_access_for_suffix(suffix: &str) -> SourcePayloadAccess {
    match suffix {
        "text" | "key" | "address" => SourcePayloadAccess::Field(suffix.to_owned()),
        _ => SourcePayloadAccess::UnknownField(suffix.to_owned()),
    }
}

fn source_payload_field_type(field: &str) -> Type {
    match field {
        "text" | "key" | "address" => Type::Text,
        _ => unresolved_shape(format!("unknown source payload field `{field}`")),
    }
}

fn source_payload_type_for_path(
    source_payload_types: &BTreeMap<String, Type>,
    path: &str,
) -> Option<Type> {
    source_payload_types.get(path).cloned().or_else(|| {
        source_payload_types
            .iter()
            .find(|(source_path, _)| {
                let relative = source_path.strip_prefix("store.").unwrap_or(source_path);
                *source_path == path
                    || source_path.ends_with(&format!(".{path}"))
                    || relative == path
                    || relative.ends_with(&format!(".{path}"))
            })
            .map(|(_, ty)| ty.clone())
    })
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
            let payload_type = Type::Object(ObjectShape::from_ordered_fields(
                fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.clone())),
                false,
            ));
            SourcePayloadShapeEntry {
                source_path: source.path.clone(),
                payload_type,
                fields,
            }
        })
        .collect()
}

fn type_hint_table(
    program: &ParsedProgram,
    expr_type_table: &ExprTypeTable,
    function_type_table: &FunctionTypeTable,
    render_slot_table: &RenderSlotTable,
    source_payload_shape_table: &[SourcePayloadShapeEntry],
    name_bindings: &BTreeMap<String, Type>,
) -> TypeHintTable {
    let expr_types = expr_type_table
        .entries
        .iter()
        .map(|entry| (entry.expr_id, entry.flow_type.ty.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut entries = Vec::new();
    for expr in &program.expressions {
        let Some(ty) = expr_types.get(&expr.id) else {
            continue;
        };
        if !expr_kind_gets_type_hint(&expr.kind) {
            continue;
        }
        let category = type_hint_category_for_expr(&expr.kind);
        entries.push(type_hint_entry_for_range(
            program,
            Some(expr.id),
            expr.line,
            expr.start,
            expr.end,
            category,
            ty,
        ));
        collect_call_argument_type_hints(program, expr, &expr_types, &mut entries);
    }
    collect_statement_type_hints(
        program,
        &program.ast.statements,
        &expr_types,
        function_type_table,
        source_payload_shape_table,
        name_bindings,
        &mut entries,
    );
    for slot in &render_slot_table.slots {
        let Some(statement) = statement_by_id(&program.ast.statements, slot.slot_statement_id)
        else {
            continue;
        };
        entries.push(type_hint_entry_for_range(
            program,
            slot.value_expr_id,
            statement.line,
            statement.start,
            statement.end,
            "render_slot",
            &slot.actual_type,
        ));
    }
    entries.sort_by_key(|entry| (entry.line, entry.anchor_column, entry.start, entry.end));
    entries.dedup_by(|left, right| {
        left.line == right.line
            && left.start == right.start
            && left.end == right.end
            && left.category == right.category
            && left.compact_label == right.compact_label
    });
    TypeHintTable { entries }
}

fn type_hint_entry_for_range(
    program: &ParsedProgram,
    expr_id: Option<usize>,
    line: usize,
    start: usize,
    end: usize,
    category: &str,
    ty: &Type,
) -> TypeHintEntry {
    type_hint_entry_for_labels(
        program,
        expr_id,
        line,
        start,
        end,
        category,
        boon_facing_type_compact_label(ty),
        boon_facing_type_detail_label(ty),
    )
}

fn type_hint_entry_for_labels(
    program: &ParsedProgram,
    expr_id: Option<usize>,
    line: usize,
    start: usize,
    end: usize,
    category: &str,
    compact_label: String,
    detail_label: String,
) -> TypeHintEntry {
    TypeHintEntry {
        expr_id,
        line,
        start,
        end,
        anchor_column: byte_column_for_line(&program.source, line, end),
        category: category.to_owned(),
        compact_label,
        detail_label,
    }
}

fn collect_call_argument_type_hints(
    program: &ParsedProgram,
    expr: &AstExpr,
    expr_types: &BTreeMap<usize, Type>,
    entries: &mut Vec<TypeHintEntry>,
) {
    let args = match &expr.kind {
        AstExprKind::Call { args, .. } | AstExprKind::Pipe { args, .. } => args,
        _ => return,
    };
    for arg in args {
        let Some((line, start, end)) = call_arg_name_range(program, arg) else {
            continue;
        };
        let Some(ty) = expr_types.get(&arg.value) else {
            continue;
        };
        entries.push(type_hint_entry_for_range(
            program,
            Some(arg.value),
            line,
            start,
            end,
            "call_arg",
            ty,
        ));
    }
}

fn expr_kind_gets_type_hint(kind: &AstExprKind) -> bool {
    !matches!(
        kind,
        AstExprKind::StringLiteral(_)
            | AstExprKind::TextLiteral(_)
            | AstExprKind::Number(_)
            | AstExprKind::Bool(_)
            | AstExprKind::Delimiter
            | AstExprKind::Unknown(_)
            | AstExprKind::Source
            | AstExprKind::Latest
            | AstExprKind::ListLiteral { .. }
    )
}

fn type_hint_category_for_expr(kind: &AstExprKind) -> &'static str {
    match kind {
        AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => "call",
        AstExprKind::Path(_) => "path",
        AstExprKind::MatchArm { .. } => "match_arm",
        AstExprKind::Identifier(_) => "expression",
        AstExprKind::Object(_) | AstExprKind::Record(_) | AstExprKind::TaggedObject { .. } => {
            "expression"
        }
        _ => "expression",
    }
}

fn collect_statement_type_hints(
    program: &ParsedProgram,
    statements: &[AstStatement],
    expr_types: &BTreeMap<usize, Type>,
    function_type_table: &FunctionTypeTable,
    source_payload_shape_table: &[SourcePayloadShapeEntry],
    name_bindings: &BTreeMap<String, Type>,
    entries: &mut Vec<TypeHintEntry>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Field { .. } | AstStatementKind::Hold { .. } => {
                let value_expr = direct_statement_value_expr_id(statement, &program.expressions);
                let ty = statement_hint_type(
                    program,
                    statement,
                    expr_types,
                    source_payload_shape_table,
                    name_bindings,
                );
                entries.push(type_hint_entry_for_range(
                    program,
                    value_expr,
                    statement.line,
                    statement.start,
                    statement.end,
                    "definition",
                    &ty,
                ));
            }
            AstStatementKind::List { .. } => {
                let ty = statement_field(statement)
                    .and_then(|field| name_bindings.get(&field).cloned())
                    .unwrap_or_else(|| simple_list_statement_type(statement, &program.expressions));
                entries.push(type_hint_entry_for_range(
                    program,
                    statement.expr,
                    statement.line,
                    statement.start,
                    statement.end,
                    "definition",
                    &ty,
                ));
            }
            AstStatementKind::Source { .. } => {
                let source_path = source_payload_shape_table
                    .iter()
                    .find(|entry| {
                        entry
                            .source_path
                            .ends_with(statement_source_suffix(statement).as_str())
                    })
                    .map(|entry| entry.payload_type.clone())
                    .unwrap_or_else(exact_empty_object_type);
                entries.push(type_hint_entry_for_range(
                    program,
                    statement.expr,
                    statement.line,
                    statement.start,
                    statement.end,
                    "source_payload",
                    &source_path,
                ));
            }
            AstStatementKind::Function { name, args } => {
                if let Some(function) = function_type_table
                    .entries
                    .iter()
                    .find(|entry| entry.name == *name)
                {
                    if let Some((start, end)) = function_name_range(program, statement, name) {
                        if let Some(compact_label) = function_signature_compact_label(function) {
                            entries.push(type_hint_entry_for_labels(
                                program,
                                statement.expr,
                                statement.line,
                                start,
                                end,
                                "function_signature",
                                compact_label,
                                function_signature_detail_label(function),
                            ));
                        }
                        entries.push(type_hint_entry_for_range(
                            program,
                            statement.expr,
                            statement.line,
                            start,
                            end,
                            "function_return",
                            &function.result.ty,
                        ));
                    }
                    let arg_ranges = function_arg_ranges(program, statement, args);
                    for (index, arg_ty) in function.arg_types.iter().enumerate() {
                        if let Some(Some((start, end))) = arg_ranges.get(index) {
                            entries.push(type_hint_entry_for_range(
                                program,
                                statement.expr,
                                statement.line,
                                *start,
                                *end,
                                "function_arg",
                                arg_ty,
                            ));
                        }
                    }
                }
            }
            AstStatementKind::Block | AstStatementKind::Expression => {}
        }
        collect_statement_type_hints(
            program,
            &statement.children,
            expr_types,
            function_type_table,
            source_payload_shape_table,
            name_bindings,
            entries,
        );
    }
}

fn statement_hint_type(
    program: &ParsedProgram,
    statement: &AstStatement,
    expr_types: &BTreeMap<usize, Type>,
    source_payload_shape_table: &[SourcePayloadShapeEntry],
    name_bindings: &BTreeMap<String, Type>,
) -> Type {
    let value_expr = direct_statement_value_expr_id(statement, &program.expressions);
    if !statement.children.is_empty() {
        let mut fields = BTreeMap::new();
        let mut field_order = Vec::new();
        for child in &statement.children {
            let Some(field) = statement_output_name(child) else {
                continue;
            };
            let ty = match &child.kind {
                AstStatementKind::Source { .. } => {
                    source_statement_value_type(child, source_payload_shape_table)
                }
                _ => statement_hint_type(
                    program,
                    child,
                    expr_types,
                    source_payload_shape_table,
                    name_bindings,
                ),
            };
            insert_ordered_shape_field(&mut fields, &mut field_order, field, ty);
        }
        if !fields.is_empty() {
            return Type::Object(ObjectShape {
                fields,
                field_order,
                open: false,
            });
        }
    }
    if let Some(ty) = value_expr
        .and_then(|expr_id| expr_types.get(&expr_id).cloned())
        .filter(is_specific_type)
        .or_else(|| statement_pipeline_hint_type(program, statement, expr_types, name_bindings))
        .or_else(|| best_statement_expr_type(statement, expr_types))
    {
        return ty;
    }
    statement_field(statement)
        .and_then(|field| name_bindings.get(&field).cloned())
        .or_else(|| value_expr.and_then(|expr_id| expr_types.get(&expr_id).cloned()))
        .unwrap_or_else(|| {
            Type::Object(object_shape_for_statement(statement, &program.expressions))
        })
}

fn source_payload_type_for_statement(
    statement: &AstStatement,
    source_payload_shape_table: &[SourcePayloadShapeEntry],
) -> Option<Type> {
    source_payload_shape_table
        .iter()
        .find(|entry| {
            entry
                .source_path
                .ends_with(statement_source_suffix(statement).as_str())
        })
        .map(|entry| entry.payload_type.clone())
}

fn source_statement_value_type(
    statement: &AstStatement,
    source_payload_shape_table: &[SourcePayloadShapeEntry],
) -> Type {
    let payload = source_payload_type_for_statement(statement, source_payload_shape_table)
        .unwrap_or_else(exact_empty_object_type);
    match &statement.kind {
        AstStatementKind::Source {
            event: Some(event), ..
        } => Type::Object(ObjectShape::from_ordered_fields(
            [(event.clone(), payload)].into_iter(),
            false,
        )),
        _ => payload,
    }
}

fn is_specific_type(ty: &Type) -> bool {
    match ty {
        Type::Skip | Type::UnresolvedShape { .. } | Type::Unknown | Type::Var(_) => false,
        ty if is_open_object_type(ty) => false,
        Type::List(item) if is_open_object_type(item) => false,
        _ => true,
    }
}

fn best_statement_expr_type(
    statement: &AstStatement,
    expr_types: &BTreeMap<usize, Type>,
) -> Option<Type> {
    statement_expr_ids(statement)
        .into_iter()
        .rev()
        .filter_map(|expr_id| expr_types.get(&expr_id).cloned())
        .find(is_specific_type)
}

fn statement_pipeline_hint_type(
    program: &ParsedProgram,
    statement: &AstStatement,
    expr_types: &BTreeMap<usize, Type>,
    name_bindings: &BTreeMap<String, Type>,
) -> Option<Type> {
    let expr_ids = statement_expression_child_expr_ids(statement);
    if !expression_sequence_is_pipeline(&expr_ids, &program.expressions) {
        return None;
    }
    let (first, rest) = expr_ids.split_first()?;
    let mut ty = hint_type_for_expr_id(program, *first, expr_types, name_bindings)?;
    for expr_id in rest {
        let Some(AstExpr {
            kind: AstExprKind::Pipe { op, args, .. },
            ..
        }) = program.expressions.get(*expr_id)
        else {
            ty = hint_type_for_expr_id(program, *expr_id, expr_types, name_bindings)?;
            continue;
        };
        ty = match op.as_str() {
            "List/retain" | "List/remove" => ty,
            "List/count" | "List/sum" => Type::Number,
            "List/append" => {
                let append_ty = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("item"))
                    .and_then(|arg| {
                        hint_type_for_expr_id(program, arg.value, expr_types, name_bindings)
                    });
                match (ty, append_ty) {
                    (Type::List(item), Some(append_ty)) => {
                        Type::List(Box::new(widen_structural_type(&item, &append_ty)))
                    }
                    (existing, _) => existing,
                }
            }
            "List/map" => {
                hint_type_for_expr_id(program, *expr_id, expr_types, name_bindings).unwrap_or(ty)
            }
            "Bool/not" | "Bool/and" => true_false_type(),
            _ => hint_type_for_expr_id(program, *expr_id, expr_types, name_bindings).unwrap_or(ty),
        };
    }
    Some(ty)
}

fn statement_expression_child_expr_ids(statement: &AstStatement) -> Vec<usize> {
    statement
        .children
        .iter()
        .filter_map(|child| {
            matches!(
                child.kind,
                AstStatementKind::Expression
                    | AstStatementKind::Hold { .. }
                    | AstStatementKind::List { field: None, .. }
            )
            .then(|| child.expr.or_else(|| first_child_expr_id(child)))
            .flatten()
        })
        .collect()
}

fn hint_type_for_expr_id(
    program: &ParsedProgram,
    expr_id: usize,
    expr_types: &BTreeMap<usize, Type>,
    name_bindings: &BTreeMap<String, Type>,
) -> Option<Type> {
    expr_types
        .get(&expr_id)
        .filter(|ty| is_specific_type(ty))
        .cloned()
        .or_else(|| match &program.expressions.get(expr_id)?.kind {
            AstExprKind::Identifier(name) => name_bindings.get(name).cloned(),
            AstExprKind::Path(parts) => {
                let path = parts.join(".");
                name_bindings.get(&path).cloned().or_else(|| {
                    parts
                        .first()
                        .and_then(|base| name_bindings.get(base))
                        .and_then(|base| type_for_nested_path(base, &parts[1..]))
                })
            }
            _ => None,
        })
}

fn function_signature_compact_label(function: &FunctionTypeEntry) -> Option<String> {
    let result_label = signature_type_compact_label(&function.result.ty);
    let arg_labels = function
        .args
        .iter()
        .zip(function.arg_types.iter())
        .map(|(arg, ty)| format!("{arg}: {}", signature_type_compact_label(ty)))
        .collect::<Vec<_>>();
    if arg_labels.is_empty() {
        return None;
    }
    let label = format!("({}) -> {result_label}", arg_labels.join(", "));
    Some(label)
}

fn function_signature_detail_label(function: &FunctionTypeEntry) -> String {
    let args = function
        .args
        .iter()
        .zip(function.arg_types.iter())
        .map(|(arg, ty)| format!("{arg}: {}", boon_facing_type_detail_label(ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "function {}({args}) -> {}",
        function.name,
        boon_facing_type_detail_label(&function.result.ty)
    )
}

fn signature_type_compact_label(ty: &Type) -> String {
    match ty {
        Type::Object(shape) if shape.fields.is_empty() && !shape.open => "[]".to_owned(),
        Type::Object(shape) if shape.fields.is_empty() => "[...]".to_owned(),
        Type::Object(_) => {
            let label = boon_facing_type_compact_label(ty);
            if label.chars().count() <= 28 && !label.contains("...") {
                label
            } else {
                "[...]".to_owned()
            }
        }
        Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. } => "VALUE".to_owned(),
        _ => {
            let label = boon_facing_type_compact_label(ty);
            if label == "VALUE" {
                "VALUE".to_owned()
            } else {
                label
            }
        }
    }
}

fn function_name_range(
    program: &ParsedProgram,
    statement: &AstStatement,
    name: &str,
) -> Option<(usize, usize)> {
    let (line_start, line_text) = source_line_with_start(&program.source, statement.line)?;
    let keyword = line_text.find("FUNCTION")?;
    let name_search_start = keyword + "FUNCTION".len();
    let name_offset = line_text.get(name_search_start..)?.find(name)?;
    let start = line_start + name_search_start + name_offset;
    Some((start, start + name.len()))
}

fn function_arg_ranges(
    program: &ParsedProgram,
    statement: &AstStatement,
    args: &[String],
) -> Vec<Option<(usize, usize)>> {
    let Some((line_start, line_text)) = source_line_with_start(&program.source, statement.line)
    else {
        return vec![None; args.len()];
    };
    let Some(open) = line_text.find('(') else {
        return vec![None; args.len()];
    };
    let close = line_text[open + 1..]
        .find(')')
        .map(|offset| open + 1 + offset)
        .unwrap_or(line_text.len());
    let arg_text = &line_text[open + 1..close];
    let mut search_offset = 0;
    args.iter()
        .map(|arg| {
            let relative = arg_text.get(search_offset..)?.find(arg)?;
            let start = open + 1 + search_offset + relative;
            search_offset += relative + arg.len();
            Some((line_start + start, line_start + start + arg.len()))
        })
        .collect()
}

fn statement_by_id(statements: &[AstStatement], id: usize) -> Option<&AstStatement> {
    for statement in statements {
        if statement.id == id {
            return Some(statement);
        }
        if let Some(found) = statement_by_id(&statement.children, id) {
            return Some(found);
        }
    }
    None
}

fn statement_source_suffix(statement: &AstStatement) -> String {
    match &statement.kind {
        AstStatementKind::Source {
            field: Some(field),
            event: Some(event),
        } => format!("{field}.{event}"),
        AstStatementKind::Source {
            field: Some(field),
            event: None,
        } => field.clone(),
        _ => statement_field(statement).unwrap_or_else(|| "source".to_owned()),
    }
}

fn call_arg_name_range(program: &ParsedProgram, arg: &AstCallArg) -> Option<(usize, usize, usize)> {
    let name = arg.name.as_ref()?;
    let line = line_for_byte(&program.source, arg.start);
    let (line_start, line_text) = source_line_with_start(&program.source, line)?;
    let search_start = arg.start.saturating_sub(line_start).min(line_text.len());
    let search_end = arg.end.saturating_sub(line_start).min(line_text.len());
    let range_text = line_text.get(search_start..search_end)?;
    let name_offset = range_text.find(name)?;
    let start = line_start + search_start + name_offset;
    Some((line, start, start + name.len()))
}

fn source_line_with_start(source: &str, line: usize) -> Option<(usize, &str)> {
    let start = source
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    if start > source.len() {
        return None;
    }
    let rest = source.get(start..)?;
    let len = rest.find('\n').map(|index| index + 1).unwrap_or(rest.len());
    Some((start, &rest[..len]))
}

fn line_for_byte(source: &str, byte: usize) -> usize {
    let mut line = 1;
    let mut offset = 0;
    for chunk in source.split_inclusive('\n') {
        offset += chunk.len();
        if byte < offset {
            return line;
        }
        line += 1;
    }
    line
}

fn byte_column_for_line(source: &str, line: usize, byte: usize) -> usize {
    let line_start = source
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    source
        .get(line_start..byte.min(source.len()))
        .unwrap_or_default()
        .chars()
        .count()
        .saturating_add(1)
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
        Variant::Tag("False".to_owned()),
        Variant::Tag("True".to_owned()),
    ])
}

fn tag_type(tag: &str) -> Type {
    Type::VariantSet(vec![Variant::Tag(tag.to_owned())])
}

fn tag_union_type(tags: &[&str]) -> Type {
    Type::VariantSet(
        tags.iter()
            .map(|tag| Variant::Tag((*tag).to_owned()))
            .collect(),
    )
}

fn renderable_contract_type() -> Type {
    Type::RenderContract
}

fn stripe_kind_type(direction: Option<&Type>) -> Type {
    let Some(Type::VariantSet(variants)) = direction else {
        return tag_union_type(&["Row", "Stack"]);
    };
    let mut tags = BTreeSet::new();
    for variant in variants {
        match variant {
            Variant::Tag(tag) if tag == "Row" => {
                tags.insert("Row");
            }
            Variant::Tag(tag) if tag == "Column" => {
                tags.insert("Stack");
            }
            _ => {
                tags.insert("Row");
                tags.insert("Stack");
            }
        }
    }
    if tags.is_empty() {
        tags.insert("Row");
        tags.insert("Stack");
    }
    tag_union_type(&tags.into_iter().collect::<Vec<_>>())
}

fn render_slot_accepts_type(slot_name: &str, ty: &Type) -> bool {
    match slot_name {
        "items" | "children" => match ty {
            Type::List(item) => is_renderable_type(item),
            _ => false,
        },
        "child" => is_renderable_type(ty) || matches!(ty, Type::Text | Type::Number),
        _ => is_renderable_type(ty),
    }
}

fn render_slot_type_error(slot_name: &str, actual_type: &Type) -> String {
    let expected = match slot_name {
        "items" | "children" => "LIST<[...]>",
        _ => "[...]",
    };
    format!(
        "`{slot_name}` expects objects accepted by `document:`\nexpected: {expected}\nfound: {}",
        boon_facing_type_label(actual_type)
    )
}

fn is_renderable_type(ty: &Type) -> bool {
    matches!(ty, Type::RenderContract)
        || RenderContractRegistry::default().is_renderable_object_type(ty)
        || is_no_element_type(ty)
}

fn is_document_render_object_type(ty: &Type) -> bool {
    RenderContractRegistry::default().is_renderable_object_type(ty)
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
        ty if is_document_render_object_type(ty) => true,
        ty if is_no_element_type(ty) => true,
        Type::List(item) => type_contains_renderable(item),
        Type::Object(shape) => shape.fields.values().any(type_contains_renderable),
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields.fields.values().any(type_contains_renderable),
        }),
        Type::Function { result, .. } => type_contains_renderable(&result.ty),
        Type::Text
        | Type::Number
        | Type::Skip
        | Type::Var(_)
        | Type::Unknown
        | Type::UnresolvedShape { .. } => false,
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
        | Type::Unknown
        | Type::UnresolvedShape { .. } => false,
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
        Type::Text
        | Type::Number
        | Type::RenderContract
        | Type::Var(_)
        | Type::Unknown
        | Type::UnresolvedShape { .. } => false,
    }
}

fn expr_is_skip(expr: &AstExpr) -> bool {
    matches!(&expr.kind, AstExprKind::Tag(tag) | AstExprKind::Enum(tag) if tag == "SKIP")
}

fn open_object_type() -> Type {
    Type::Object(ObjectShape::new(BTreeMap::new(), true))
}

fn exact_empty_object_type() -> Type {
    Type::Object(ObjectShape::new(BTreeMap::new(), false))
}

fn unresolved_shape(reason: impl Into<String>) -> Type {
    Type::UnresolvedShape {
        reason: reason.into(),
    }
}

fn is_open_object_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Object(ObjectShape {
            fields,
            open: true,
            ..
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
        Type::Text
        | Type::Number
        | Type::Skip
        | Type::RenderContract
        | Type::Unknown
        | Type::UnresolvedShape { .. } => {}
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
    ObjectShape::from_ordered_fields(
        fields
            .iter()
            .map(|field| (field.name.clone(), open_object_type())),
        false,
    )
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
        assert_eq!(
            report
                .constraints
                .iter()
                .filter(|constraint| matches!(constraint, Constraint::Equal { .. }))
                .count(),
            report.expression_count
        );
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
        assert_eq!(report.render_slot_count, 2);
        assert_eq!(report.list_map_binding_count_render_slot_materialization, 1);
        let slot = report
            .render_slot_table
            .slots
            .iter()
            .find(|slot| slot.slot_name == "items")
            .expect("items render slot");
        assert_eq!(slot.slot_name, "items");
        assert_eq!(slot.expected_contract, "LIST<[...]>");
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
        assert!(report.list_map_bindings.iter().any(|binding| matches!(
            &binding.result_type,
            Type::List(item) if is_open_object_type(item)
        )));
    }

    #[test]
    fn rejects_skip_as_list_map_item_outside_render_slots() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
todos: LIST[4] {}
rows:
    todos
    |> List/map(todo, new: SKIP)
document: []
"#;
        let parsed = boon_parser::parse_source("bad-list-map-skip-item.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("`SKIP` cannot be used as a `List/map` item")
        }));
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
                .contains("`items` expects objects accepted by `document:`")
        }));
    }

    #[test]
    fn function_returning_renderable_list_for_items_gets_render_metadata() {
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
            Type::List(item) if is_document_render_object_type(item)
        ));
        assert_eq!(report.list_map_binding_count_render_slot_materialization, 1);
        let binding = slot
            .optional_list_map_binding_id
            .and_then(|id| report.list_map_bindings.get(id))
            .expect("function-returned render list should keep materialization metadata");
        let list_expr = parsed
            .expressions
            .get(binding.list_expr_id)
            .expect("binding list expression should point to call-site source list");
        assert!(matches!(
            &list_expr.kind,
            AstExprKind::Identifier(name) if name == "todos"
        ));
        assert_eq!(binding.template_function.as_deref(), Some("todo_row"));
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
                .contains("`items` expects objects accepted by `document:`")
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
                .contains("`items` expects objects accepted by `document:`")
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
                .contains("`Bool/not` expects `True` or `False`")
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
                    .contains("`Bool/and` expects `True` or `False`"))
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
    fn rejects_while_on_present_or_absent_selector() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
bad:
    source |> WHILE {
        __ => 1
    }
document: []
"#;
        let parsed = boon_parser::parse_source("bad-while-source-flow.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("`WHILE` requires a continuous selector")
        }));
        assert!(report.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                Constraint::FlowCompatible {
                    actual: FlowType {
                        mode: FlowMode::PresentOrAbsent,
                        ..
                    },
                    expected: FlowType {
                        mode: FlowMode::Continuous,
                        ..
                    }
                }
            )
        }));
    }

    #[test]
    fn when_preserves_continuous_selector_flow() {
        let source = r#"
source: SOURCE
selected: True |> HOLD selected { LATEST {} }
matched:
    selected |> WHEN {
        True => 1
        False => 0
    }
document: []
"#;
        let parsed = boon_parser::parse_source("continuous-when-flow.bn", source).unwrap();
        let report = check(&parsed);
        assert!(
            !report.has_errors(),
            "unexpected diagnostics: {:?}",
            report.diagnostics
        );
        let when_expr_id = parsed
            .expressions
            .iter()
            .find_map(|expr| matches!(expr.kind, AstExprKind::When { .. }).then_some(expr.id))
            .expect("fixture should contain WHEN expression");
        let when_flow = report
            .expr_type_table
            .entries
            .iter()
            .find(|entry| entry.expr_id == when_expr_id)
            .expect("WHEN expression should be typed");
        assert_eq!(when_flow.flow_type.mode, FlowMode::Continuous);
        assert!(report.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                Constraint::PatternCovers { expr_id } if *expr_id == when_expr_id
            )
        }));
        assert!(report.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                Constraint::HasVariant {
                    variant: Variant::Tag(tag),
                    ..
                } if tag == "True"
            )
        }));
        assert!(report.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                Constraint::HasVariant {
                    variant: Variant::Tag(tag),
                    ..
                } if tag == "False"
            )
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
    fn rejects_function_argument_missing_required_structural_field() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
FUNCTION row(todo) {
    Element/label(label: todo.title)
}
document:
    root:
        Element/stripe(
            items: LIST {
                row(todo: [completed: True])
            }
        )
"#;
        let parsed = boon_parser::parse_source("bad-function-arg-shape.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("object is missing field `title`")
        }));
        assert!(report.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                Constraint::Assignable {
                    expected: Type::Object(shape),
                    ..
                } if shape.fields.contains_key("title")
            )
        }));
        let row_entry = report
            .function_type_table
            .entries
            .iter()
            .find(|entry| entry.name == "row")
            .expect("row function type should be reported");
        assert_eq!(row_entry.args, vec!["todo".to_owned()]);
        assert!(matches!(
            row_entry.arg_types.as_slice(),
            [Type::Object(shape)] if matches!(shape.fields.get("title"), Some(Type::Text))
        ));
    }

    #[test]
    fn rejects_function_argument_field_with_incompatible_required_type() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
FUNCTION row(todo) {
    Element/label(label: todo.title)
}
document:
    root:
        Element/stripe(
            items: LIST {
                row(todo: [title: 1])
            }
        )
"#;
        let parsed = boon_parser::parse_source("bad-function-arg-field-type.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("object field `title` has incompatible type")
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
    draft:
        Text/empty() |> HOLD value {
            LATEST {
                sources.input.change.text
            }
        }
    text:
        Text/empty() |> HOLD text {
            LATEST {
                sources.input.change.text
                sources.input.key_down.key |> WHEN {
                    Enter => draft
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
        assert!(matches!(
            &change.payload_type,
            Type::Object(ObjectShape { open: false, .. })
        ));
        let key_down = report
            .source_payload_shape_table
            .iter()
            .find(|entry| entry.source_path == "store.sources.input.key_down")
            .expect("key_down source should have a payload shape");
        assert!(key_down.fields.iter().any(|field| field.name == "key"));
        assert!(!key_down.fields.iter().any(|field| field.name == "text"));
        assert!(matches!(
            &key_down.payload_type,
            Type::Object(ObjectShape { open: false, .. })
        ));
    }

    #[test]
    fn type_hint_table_uses_boon_facing_labels() {
        let source = r#"
source: SOURCE
todos: LIST[4] {}
visible: True |> HOLD visible { LATEST {} }
negated: visible |> Bool/not()
document:
    root:
        Element/stripe(
            items:
                todos |> List/map(todo, new: row(todo: todo))
        )
FUNCTION row(todo) {
    Element/text(label: todo.title)
}
"#;
        let parsed = boon_parser::parse_source("type-hints.bn", source).unwrap();
        let report = check(&parsed);
        assert!(
            !report.has_errors(),
            "unexpected diagnostics: {:?}",
            report.diagnostics
        );
        let labels = report
            .type_hint_table
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "{} {}",
                    entry.compact_label.as_str(),
                    entry.detail_label.as_str()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(labels.contains("BOOL"));
        assert!(
            !labels.contains("True | False"),
            "tag unions should be sorted for scan-stable display: {labels}"
        );
        let detail_labels = report
            .type_hint_table
            .entries
            .iter()
            .map(|entry| entry.detail_label.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !detail_labels.contains("..."),
            "detail type labels must not hide fields with ellipses: {detail_labels}"
        );
        assert!(labels.contains("LIST<"));
        assert!(labels.contains("kind: Text"));
        for forbidden in ["object ", "tag ", "Element"] {
            assert!(
                !labels.contains(forbidden),
                "visible type hints must use Boon notation and avoid `{forbidden}`: {labels}"
            );
        }
        for internal in [
            "TypeVar",
            "Bool",
            "Event",
            "Record",
            "RenderContract",
            "Unknown",
        ] {
            assert!(
                !labels.contains(internal),
                "visible type hints must not expose `{internal}`: {labels}"
            );
        }
    }

    #[test]
    fn render_contract_registry_can_register_future_roots() {
        let registry = RenderContractRegistry::default()
            .register_root(
                "scene",
                RuntimeRootContract::new(["Mesh", "Group"])
                    .with_fixed_constructor("Scene/mesh", "Mesh")
                    .with_fixed_constructor("Scene/group", "Group"),
            )
            .with_active_root("scene");
        assert_eq!(registry.active_root(), "scene");
        assert!(registry.is_render_constructor("Scene/mesh"));

        let mesh =
            registry.constructor_shape("Scene/mesh", [("id".to_owned(), Type::Text)].into_iter());
        assert!(registry.is_renderable_object_type(&mesh));
        assert_eq!(
            boon_facing_type_detail_label(&mesh),
            "[
    id: TEXT
    kind: Mesh
]"
        );

        let document_shape =
            RenderContractRegistry::default().constructor_shape("Element/text", BTreeMap::new());
        assert!(!registry.is_renderable_object_type(&document_shape));
    }

    #[test]
    fn todomvc_type_hints_have_complete_store_and_source_shapes() {
        let source = include_str!("../../../examples/todomvc.bn");
        let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
        let report = check(&parsed);
        assert!(
            !report.has_errors(),
            "unexpected diagnostics: {:?}",
            report.diagnostics
        );

        let store_line = source
            .lines()
            .position(|line| line.trim() == "store: [")
            .map(|index| index + 1)
            .expect("TodoMVC should declare store");
        let store_hint = report
            .type_hint_table
            .entries
            .iter()
            .find(|entry| entry.line == store_line && entry.category == "definition")
            .expect("store should have a definition type hint");
        for expected in [
            "new_todo_text: TEXT",
            "title_to_add: TEXT",
            "selected_filter: Active | All | Completed",
            "active_count: NUMBER",
            "completed_count: NUMBER",
            "has_completed: BOOL",
            "all_completed: BOOL",
            "todos: LIST<[",
            "visible_todos: LIST<[",
        ] {
            assert!(
                store_hint.detail_label.contains(expected),
                "store detail should contain `{expected}`:\n{}",
                store_hint.detail_label
            );
        }
        for forbidden in [
            "new_todo_text: unresolved object shape",
            "title_to_add: unresolved object shape",
            "todos: unresolved object shape",
            "visible_todos: unresolved object shape",
            "object ",
            "tag ",
        ] {
            assert!(
                !store_hint.detail_label.contains(forbidden),
                "store detail should not contain `{forbidden}`:\n{}",
                store_hint.detail_label
            );
        }
        let source_index = store_hint.detail_label.find("sources: [").unwrap();
        let new_text_index = store_hint.detail_label.find("new_todo_text: TEXT").unwrap();
        let title_index = store_hint.detail_label.find("title_to_add: TEXT").unwrap();
        let filter_index = store_hint
            .detail_label
            .find("selected_filter: Active | All | Completed")
            .unwrap();
        let todos_index = store_hint.detail_label.find("todos: LIST<[").unwrap();
        assert!(
            source_index < new_text_index
                && new_text_index < title_index
                && title_index < filter_index
                && filter_index < todos_index,
            "store detail fields should follow the Boon source order:\n{}",
            store_hint.detail_label
        );
        let todos_detail = &store_hint.detail_label[todos_index..];
        let todo_sources_index = todos_detail.find("sources: [").unwrap();
        let todo_title_index = todos_detail.find("title: TEXT").unwrap();
        let todo_completed_index = todos_detail.find("completed: BOOL").unwrap();
        assert!(
            todo_sources_index < todo_title_index && todo_title_index < todo_completed_index,
            "mapped todo items should keep the function return field order and refine field types:\n{}",
            store_hint.detail_label
        );

        let new_todo_line = source
            .lines()
            .position(|line| line.trim() == "FUNCTION new_todo(todo, store) {")
            .map(|index| index + 1)
            .expect("TodoMVC should declare new_todo");
        let new_todo_arg = report
            .type_hint_table
            .entries
            .iter()
            .find(|entry| {
                entry.line == new_todo_line
                    && entry.category == "function_arg"
                    && entry.detail_label.contains("sources: [")
            })
            .expect("new_todo row argument should have a canonical row shape");
        assert!(
            new_todo_arg.detail_label.find("sources: [").unwrap()
                < new_todo_arg.detail_label.find("title: TEXT").unwrap()
                && new_todo_arg.detail_label.find("title: TEXT").unwrap()
                    < new_todo_arg.detail_label.find("completed: BOOL").unwrap(),
            "function row argument should display function-return order with refined field types:\n{}",
            new_todo_arg.detail_label
        );

        for (line_text, expected) in [
            ("change: SOURCE", "[\n    text: TEXT\n]"),
            ("key_down: SOURCE", "[\n    key: TEXT\n]"),
            ("toggle_all_checkbox: [click: SOURCE]", "[]"),
            ("clear_completed_button: [press: SOURCE]", "[]"),
        ] {
            let line = source
                .lines()
                .position(|line| line.trim() == line_text)
                .map(|index| index + 1)
                .unwrap_or_else(|| panic!("TodoMVC should contain `{line_text}`"));
            let hint = report
                .type_hint_table
                .entries
                .iter()
                .find(|entry| entry.line == line && entry.category == "source_payload")
                .unwrap_or_else(|| panic!("source `{line_text}` should have a payload hint"));
            assert_eq!(hint.detail_label, expected, "{line_text}");
        }
    }

    #[test]
    fn todomvc_completed_hints_use_widened_true_false_shape() {
        let source = include_str!("../../../examples/todomvc.bn");
        let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
        let report = check(&parsed);
        assert!(
            !report.has_errors(),
            "unexpected diagnostics: {:?}",
            report.diagnostics
        );

        let clear_completed_line = source
            .lines()
            .position(|line| line.contains("THEN { todo.completed }"))
            .map(|index| index + 1)
            .expect("TodoMVC should use todo.completed in clear-completed removal");
        let completed_path_hint = report
            .type_hint_table
            .entries
            .iter()
            .find(|entry| {
                entry.line == clear_completed_line
                    && entry.category == "path"
                    && entry.detail_label == "BOOL"
            })
            .expect("todo.completed should have a hover hint");
        assert!(
            completed_path_hint.detail_label == "BOOL",
            "todo.completed should be widened from list rows, got {}",
            completed_path_hint.detail_label
        );

        let mut in_new_todo = false;
        let completed_field_line = source
            .lines()
            .position(|line| {
                if line.trim_start().starts_with("FUNCTION new_todo") {
                    in_new_todo = true;
                }
                in_new_todo && line.trim() == "completed:"
            })
            .map(|index| index + 1)
            .expect("new_todo should define a completed field");
        let completed_field_hint = report
            .type_hint_table
            .entries
            .iter()
            .find(|entry| entry.line == completed_field_line && entry.category == "definition")
            .expect("completed field should have a definition hint");
        assert!(
            completed_field_hint.detail_label == "BOOL",
            "completed HOLD field should be widened from LATEST branches, got {}",
            completed_field_hint.detail_label
        );

        let all_completed_line = source
            .lines()
            .position(|line| line.contains("Bool/and(completed_count > 0)"))
            .map(|index| index + 1)
            .expect("TodoMVC should combine active and completed counts");
        for count_name in ["active_count", "completed_count"] {
            let count_hint = report
                .type_hint_table
                .entries
                .iter()
                .find(|entry| {
                    entry.line == all_completed_line
                        && source
                            .get(entry.start..entry.end)
                            .is_some_and(|text| text.trim() == count_name)
                })
                .expect("count path should have a hover hint");
            assert_eq!(
                count_hint.detail_label, "NUMBER",
                "{count_name} should keep its List/count result type on later references"
            );
        }
    }

    #[test]
    fn normal_type_errors_use_boon_facing_expected_and_found_types() {
        let source = r#"
source: SOURCE
bad: "yes" |> Bool/not()
value:
    0 |> HOLD value {
        LATEST {
            source |> THEN { TEXT { bad } }
        }
    }
document: []
"#;
        let parsed = boon_parser::parse_source("boon-facing-errors.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        let messages = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(messages.contains("expected: BOOL"));
        assert!(messages.contains("found: TEXT"));
        assert!(messages.contains("expected: NUMBER"));
        for internal in [
            "TypeVar",
            "Bool\n",
            "Event",
            "Record",
            "RenderContract",
            "Unknown",
        ] {
            assert!(
                !messages.contains(internal),
                "normal diagnostics must not expose `{internal}`: {messages}"
            );
        }
    }

    #[test]
    fn rejects_unknown_source_payload_field() {
        let source = r#"
source: SOURCE
value:
    Text/empty() |> HOLD value {
        LATEST {
            source.foo
        }
    }
document: []
"#;
        let parsed = boon_parser::parse_source("bad-source-payload-field.bn", source).unwrap();
        let report = check(&parsed);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("unknown source payload field `foo`")
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
