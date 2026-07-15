use boon_parser::{
    AstCallArg, AstDrainPath, AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind,
    BytesSizeSyntax, ParsedProgram,
};
use ena::unify::{EqUnifyValue, InPlaceUnificationTable, UnifyKey};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Type {
    Text,
    Number,
    Byte,
    Bytes(BytesType),
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
pub enum BytesType {
    Dynamic,
    Fixed(usize),
}

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeDisplayNode {
    Scalar {
        label: String,
    },
    Object {
        fields: Vec<TypeDisplayField>,
        open: bool,
    },
    TaggedObject {
        tag: String,
        fields: Vec<TypeDisplayField>,
        open: bool,
    },
    List {
        item: Box<TypeDisplayNode>,
    },
    Union {
        variants: Vec<TypeDisplayNode>,
    },
    Function {
        name: Option<String>,
        args: Vec<TypeDisplayFunctionArg>,
        result: Box<TypeDisplayNode>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeDisplayField {
    pub name: String,
    pub ty: TypeDisplayNode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeDisplayFunctionArg {
    pub name: Option<String>,
    pub ty: TypeDisplayNode,
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
pub struct ResolvedConstantEntry {
    pub expr_id: usize,
    pub value: ResolvedConstantValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolvedConstantValue {
    UnsignedInteger { value: u64 },
    SignedInteger { value: i64 },
    Byte { value: u8 },
    Symbol { value: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct ResolvedConstantTable {
    pub entries: Vec<ResolvedConstantEntry>,
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
    pub display_tree: TypeDisplayNode,
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostEffectTable {
    pub declarations: Vec<HostEffectDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostEffectDeclaration {
    pub name: String,
    pub line: usize,
    pub trigger_source: String,
    pub trigger_expr_id: usize,
    pub operation: String,
    pub perform_expr_id: usize,
    pub intent_type: Type,
    pub intent_fields: Vec<HostEffectIntentField>,
    pub result_type: Type,
    pub result_routes: Vec<HostEffectResultRoute>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostEffectIntentField {
    pub name: String,
    pub value_expr_id: usize,
    pub value_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostEffectResultRoute {
    pub variant: String,
    pub source_path: String,
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
    #[serde(default)]
    pub host_effect_table: HostEffectTable,
    pub full_document_typecheck_coverage: bool,
    pub list_map_binding_count_runtime_value: usize,
    pub list_map_binding_count_render_slot_materialization: usize,
    #[serde(default)]
    pub output_root_types: Vec<OutputRootTypeEntry>,
    pub expr_type_table: ExprTypeTable,
    pub function_type_table: FunctionTypeTable,
    pub type_hint_table: TypeHintTable,
    #[serde(default)]
    pub resolved_constant_table: ResolvedConstantTable,
    pub render_slot_table: RenderSlotTable,
    pub list_map_bindings: Vec<ListMapBinding>,
    pub constraints: Vec<Constraint>,
    pub diagnostics: Vec<TypeDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputRootTypeEntry {
    pub name: String,
    pub statement_id: usize,
    pub value_expr_id: Option<usize>,
    pub ty: Type,
}

impl TypeCheckReport {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            || self.render_slot_failure_count > 0
    }
}

#[derive(Clone)]
struct HostEffectSignature {
    intent_type: Type,
    intent_fields: Vec<(String, Type)>,
    result_type: Type,
}

fn host_effect_signature(operation: &str) -> Option<HostEffectSignature> {
    let text = || Type::Text;
    let number = || Type::Number;
    let boolean = || true_false_type();
    let tagged = |tag: &str, fields: Vec<(&str, Type)>| Variant::Tagged {
        tag: tag.to_owned(),
        fields: ObjectShape::from_ordered_fields(
            fields.into_iter().map(|(name, ty)| (name.to_owned(), ty)),
            false,
        ),
    };
    let (intent_fields, variants) = match operation {
        "Passkey/register" => (
            vec![
                ("workspace_id".to_owned(), text()),
                ("account_id".to_owned(), text()),
                ("credential_count".to_owned(), number()),
                ("simulation".to_owned(), passkey_simulation_type()),
            ],
            vec![
                tagged(
                    "RegistrationSucceeded",
                    vec![
                        ("account_id", text()),
                        ("credential_id", text()),
                        ("label", text()),
                    ],
                ),
                tagged("RegistrationCancelled", Vec::new()),
                tagged(
                    "RegistrationFailed",
                    vec![
                        ("code", text()),
                        ("message", text()),
                        ("retryable", boolean()),
                    ],
                ),
                tagged(
                    "DuplicateCredential",
                    vec![("account_id", text()), ("credential_id", text())],
                ),
            ],
        ),
        "Passkey/authenticate" => (
            vec![
                ("account_id".to_owned(), text()),
                ("credential_count".to_owned(), number()),
                ("simulation".to_owned(), passkey_simulation_type()),
            ],
            vec![
                tagged(
                    "AuthenticationSucceeded",
                    vec![("account_id", text()), ("credential_id", text())],
                ),
                tagged("AuthenticationCancelled", Vec::new()),
                tagged(
                    "AuthenticationFailed",
                    vec![
                        ("code", text()),
                        ("message", text()),
                        ("retryable", boolean()),
                    ],
                ),
            ],
        ),
        _ => return None,
    };
    Some(HostEffectSignature {
        intent_type: Type::Object(ObjectShape::from_ordered_fields(
            intent_fields.iter().cloned(),
            false,
        )),
        intent_fields,
        result_type: Type::VariantSet(variants),
    })
}

fn passkey_simulation_type() -> Type {
    Type::VariantSet(
        ["Success", "Cancel", "Failure", "Duplicate"]
            .into_iter()
            .map(|tag| Variant::Tag(tag.to_owned()))
            .collect(),
    )
}

fn host_effect_table(
    program: &ParsedProgram,
    source_lookup: &SourcePayloadPathLookup,
) -> (HostEffectTable, Vec<TypeDiagnostic>) {
    let mut table = HostEffectTable::default();
    let mut diagnostics = Vec::new();
    let effects = program
        .ast
        .statements
        .iter()
        .filter(|statement| statement_field_name(statement) == Some("effects"))
        .collect::<Vec<_>>();
    if effects.len() > 1 {
        diagnostics.push(diagnostic_for_statement(
            effects.get(1).copied(),
            "top-level `effects` may be declared only once".to_owned(),
        ));
    }
    let Some(effects) = effects.first().copied() else {
        return (table, diagnostics);
    };
    let mut declaration_names = BTreeSet::new();
    for declaration in &effects.children {
        let Some(name) = statement_field_name(declaration).map(str::to_owned) else {
            diagnostics.push(diagnostic_for_statement(
                Some(declaration),
                "each `effects` entry must be a named record".to_owned(),
            ));
            continue;
        };
        let diagnostic_start = diagnostics.len();
        if !declaration_names.insert(name.clone()) {
            diagnostics.push(diagnostic_for_statement(
                Some(declaration),
                format!("effect declaration `{name}` is duplicated"),
            ));
        }
        let mut members = BTreeMap::<&str, &AstStatement>::new();
        for member in &declaration.children {
            let Some(member_name) = statement_field_name(member) else {
                diagnostics.push(diagnostic_for_statement(
                    Some(member),
                    format!("effect declaration `{name}` contains an unnamed member"),
                ));
                continue;
            };
            if !matches!(member_name, "on" | "perform" | "results") {
                diagnostics.push(diagnostic_for_statement(
                    Some(member),
                    format!(
                        "effect declaration `{name}` has unsupported member `{member_name}`; expected `on`, `perform`, or `results`"
                    ),
                ));
                continue;
            }
            if members.insert(member_name, member).is_some() {
                diagnostics.push(diagnostic_for_statement(
                    Some(member),
                    format!("effect declaration `{name}` repeats `{member_name}`"),
                ));
            }
        }
        for required in ["on", "perform", "results"] {
            if !members.contains_key(required) {
                diagnostics.push(diagnostic_for_statement(
                    Some(declaration),
                    format!("effect declaration `{name}` is missing `{required}`"),
                ));
            }
        }
        let Some(on) = members.get("on").copied() else {
            continue;
        };
        let Some(trigger_expr_id) = on.expr else {
            diagnostics.push(diagnostic_for_statement(
                Some(on),
                format!("effect declaration `{name}` has no `on` source"),
            ));
            continue;
        };
        let Some(trigger_source) =
            effect_source_path(program, trigger_expr_id, source_lookup, false)
        else {
            diagnostics.push(diagnostic_for_statement(
                Some(on),
                format!("effect declaration `{name}` `on` must resolve to one SOURCE"),
            ));
            continue;
        };
        let Some(perform) = members.get("perform").copied() else {
            continue;
        };
        let Some(perform_expr_id) = perform.expr else {
            diagnostics.push(diagnostic_for_statement(
                Some(perform),
                format!("effect declaration `{name}` has no `perform` call"),
            ));
            continue;
        };
        let Some(AstExpr {
            kind: AstExprKind::Call { function, args },
            ..
        }) = program.expressions.get(perform_expr_id)
        else {
            diagnostics.push(diagnostic_for_statement(
                Some(perform),
                format!("effect declaration `{name}` `perform` must be a direct host call"),
            ));
            continue;
        };
        let Some(signature) = host_effect_signature(function) else {
            diagnostics.push(diagnostic_for_statement(
                Some(perform),
                format!("effect declaration `{name}` uses unknown typed host effect `{function}`"),
            ));
            continue;
        };
        let mut args_by_name = BTreeMap::new();
        for arg in args {
            let Some(arg_name) = arg.name.as_deref() else {
                diagnostics.push(diagnostic_for_statement(
                    Some(perform),
                    format!("effect declaration `{name}` requires named intent fields"),
                ));
                continue;
            };
            if args_by_name.insert(arg_name, arg.value).is_some() {
                diagnostics.push(diagnostic_for_statement(
                    Some(perform),
                    format!("effect declaration `{name}` repeats intent field `{arg_name}`"),
                ));
            }
        }
        for arg in &perform.children {
            let Some(arg_name) = statement_field_name(arg) else {
                diagnostics.push(diagnostic_for_statement(
                    Some(arg),
                    format!("effect declaration `{name}` requires named intent fields"),
                ));
                continue;
            };
            let Some(value_expr_id) = arg.expr else {
                diagnostics.push(diagnostic_for_statement(
                    Some(arg),
                    format!("intent field `{arg_name}` has no value"),
                ));
                continue;
            };
            if args_by_name.insert(arg_name, value_expr_id).is_some() {
                diagnostics.push(diagnostic_for_statement(
                    Some(arg),
                    format!("effect declaration `{name}` repeats intent field `{arg_name}`"),
                ));
            }
        }
        let expected_args = signature
            .intent_fields
            .iter()
            .map(|(field, _)| field.as_str())
            .collect::<BTreeSet<_>>();
        for actual in args_by_name.keys().copied() {
            if !expected_args.contains(actual) {
                diagnostics.push(diagnostic_for_statement(
                    Some(perform),
                    format!("`{function}` has no intent field `{actual}`"),
                ));
            }
        }
        let mut intent_fields = Vec::new();
        for (field, ty) in &signature.intent_fields {
            let Some(value_expr_id) = args_by_name.get(field.as_str()).copied() else {
                diagnostics.push(diagnostic_for_statement(
                    Some(perform),
                    format!("`{function}` is missing intent field `{field}`"),
                ));
                continue;
            };
            intent_fields.push(HostEffectIntentField {
                name: field.clone(),
                value_expr_id,
                value_type: ty.clone(),
            });
        }
        let Some(results) = members.get("results").copied() else {
            continue;
        };
        let expected_variants = host_effect_variants(&signature.result_type);
        let mut routes_by_variant = BTreeMap::new();
        for route in &results.children {
            let Some(variant) = statement_field_name(route) else {
                diagnostics.push(diagnostic_for_statement(
                    Some(route),
                    format!("effect declaration `{name}` has an unnamed result route"),
                ));
                continue;
            };
            if !expected_variants.contains_key(variant) {
                diagnostics.push(diagnostic_for_statement(
                    Some(route),
                    format!("`{function}` has no result variant `{variant}`"),
                ));
                continue;
            }
            let Some(expr_id) = route.expr else {
                diagnostics.push(diagnostic_for_statement(
                    Some(route),
                    format!("result variant `{variant}` has no SOURCE route"),
                ));
                continue;
            };
            let Some(source_path) = effect_source_path(program, expr_id, source_lookup, true)
            else {
                diagnostics.push(diagnostic_for_statement(
                    Some(route),
                    format!("result variant `{variant}` must route to one direct SOURCE"),
                ));
                continue;
            };
            if routes_by_variant.insert(variant, source_path).is_some() {
                diagnostics.push(diagnostic_for_statement(
                    Some(route),
                    format!("result variant `{variant}` is routed more than once"),
                ));
            }
        }
        let mut result_routes = Vec::new();
        let mut routed_sources = BTreeSet::new();
        for variant in expected_variants.keys() {
            let Some(source_path) = routes_by_variant.get(variant.as_str()).cloned() else {
                diagnostics.push(diagnostic_for_statement(
                    Some(results),
                    format!("`{function}` is missing result route `{variant}`"),
                ));
                continue;
            };
            if !routed_sources.insert(source_path.clone()) {
                diagnostics.push(diagnostic_for_statement(
                    Some(results),
                    format!(
                        "effect declaration `{name}` routes multiple variants to `{source_path}`"
                    ),
                ));
            }
            result_routes.push(HostEffectResultRoute {
                variant: variant.clone(),
                source_path,
            });
        }
        if diagnostics.len() == diagnostic_start {
            table.declarations.push(HostEffectDeclaration {
                name,
                line: declaration.line,
                trigger_source,
                trigger_expr_id,
                operation: function.clone(),
                perform_expr_id,
                intent_type: signature.intent_type,
                intent_fields,
                result_type: signature.result_type,
                result_routes,
            });
        }
    }
    table
        .declarations
        .sort_by(|left, right| left.name.cmp(&right.name));
    (table, diagnostics)
}

fn statement_field_name(statement: &AstStatement) -> Option<&str> {
    match &statement.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::Source {
            field: Some(name), ..
        }
        | AstStatementKind::List {
            field: Some(name), ..
        } => Some(name),
        _ => None,
    }
}

fn effect_source_path(
    program: &ParsedProgram,
    expr_id: usize,
    source_lookup: &SourcePayloadPathLookup,
    direct: bool,
) -> Option<String> {
    let parts = match &program.expressions.get(expr_id)?.kind {
        AstExprKind::Identifier(value) => vec![value.clone()],
        AstExprKind::Path(parts) => parts.clone(),
        _ => return None,
    };
    if direct
        && !matches!(
            source_lookup.access_for_parts(&parts),
            Some(SourcePayloadAccess::Direct(_))
        )
    {
        return None;
    }
    let mut matches = source_lookup.source_paths_for_parts(&parts);
    matches.sort();
    matches.dedup();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn host_effect_variants(result_type: &Type) -> BTreeMap<String, ObjectShape> {
    let Type::VariantSet(variants) = result_type else {
        return BTreeMap::new();
    };
    variants
        .iter()
        .map(|variant| match variant {
            Variant::Tag(tag) => (tag.clone(), ObjectShape::new(BTreeMap::new(), false)),
            Variant::Tagged { tag, fields } => (tag.clone(), fields.clone()),
        })
        .collect()
}

pub fn check(program: &ParsedProgram) -> TypeCheckReport {
    check_profiled(program).0
}

pub fn check_profiled(program: &ParsedProgram) -> (TypeCheckReport, TypeCheckProfile) {
    let (mut checker, init_profile) = Checker::new_profiled(program);
    checker.check_program_profiled(true, init_profile)
}

pub fn check_runtime_profiled(program: &ParsedProgram) -> (TypeCheckReport, TypeCheckProfile) {
    let (mut checker, init_profile) = Checker::new_profiled(program);
    checker.check_program_profiled(false, init_profile)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TypeCheckProfile {
    pub checker_init_ms: f64,
    pub source_paths_ms: f64,
    pub source_payload_shape_table_ms: f64,
    pub source_payload_types_ms: f64,
    #[serde(default)]
    pub function_index_ms: f64,
    pub object_bindings_ms: f64,
    pub function_param_requirements_ms: f64,
    pub name_bindings_ms: f64,
    pub passed_context_ms: f64,
    pub flow_bindings_ms: f64,
    pub render_contracts_ms: f64,
    pub refresh_static_row_scope_bindings_ms: f64,
    pub recursive_functions_ms: f64,
    pub check_statements_ms: f64,
    pub ensure_all_expressions_ms: f64,
    pub report_counts_ms: f64,
    pub type_hint_table_ms: f64,
    pub assemble_report_ms: f64,
    pub total_ms: f64,
}

#[derive(Clone, Copy, Debug)]
struct CheckerInitProfile {
    checker_init_ms: f64,
    source_paths_ms: f64,
    source_payload_shape_table_ms: f64,
    source_payload_types_ms: f64,
    function_index_ms: f64,
    object_bindings_ms: f64,
    function_param_requirements_ms: f64,
    name_bindings_ms: f64,
    passed_context_ms: f64,
    flow_bindings_ms: f64,
    render_contracts_ms: f64,
    refresh_static_row_scope_bindings_ms: f64,
}

struct Checker<'a> {
    program: &'a ParsedProgram,
    vars: TypeVarStore,
    builtins: BuiltinSignatureRegistry,
    render_contracts: RenderContractRegistry,
    source_paths: BTreeSet<String>,
    source_payload_lookup: SourcePayloadPathLookup,
    source_payload_shape_table: Vec<SourcePayloadShapeEntry>,
    source_payload_types: BTreeMap<String, Type>,
    host_effect_table: HostEffectTable,
    function_statements: BTreeMap<String, &'a AstStatement>,
    function_call_graph: BTreeMap<String, BTreeSet<String>>,
    function_args_by_name: BTreeMap<String, Vec<String>>,
    function_arg_call_sites: BTreeMap<String, BTreeMap<String, Vec<usize>>>,
    function_arg_display_type_cache: RefCell<BTreeMap<(String, String), Type>>,
    function_return_type_cache: RefCell<BTreeMap<String, Option<Type>>>,
    object_bindings: BTreeMap<String, ObjectShape>,
    name_bindings: BTreeMap<String, Type>,
    flow_bindings: BTreeMap<String, FlowMode>,
    function_param_requirements: BTreeMap<String, BTreeMap<String, Type>>,
    expr_type_vars: BTreeMap<usize, TypeVar>,
    runtime_list_map_exprs: BTreeSet<usize>,
    visited: BTreeSet<usize>,
    expr_type_in_progress: BTreeSet<usize>,
    expr_type_cache: Vec<Option<FlowType>>,
    expr_type_table: ExprTypeTable,
    function_type_table: FunctionTypeTable,
    collect_type_hints: bool,
    render_slot_table: RenderSlotTable,
    list_map_bindings: Vec<ListMapBinding>,
    constraints: Vec<Constraint>,
    diagnostics: Vec<TypeDiagnostic>,
}

impl<'a> Checker<'a> {
    fn new_profiled(program: &'a ParsedProgram) -> (Self, CheckerInitProfile) {
        let checker_init_started = Instant::now();
        let source_paths_started = Instant::now();
        let source_paths = program
            .source_ports
            .iter()
            .map(|source| source.path.clone())
            .collect();
        let source_paths_ms = typecheck_elapsed_ms(source_paths_started);
        let source_payload_lookup = SourcePayloadPathLookup::new(&source_paths);
        let (host_effect_table, host_effect_diagnostics) =
            host_effect_table(program, &source_payload_lookup);
        let source_payload_shape_table_started = Instant::now();
        let source_payload_shape_table = source_payload_shape_table(
            program,
            &source_paths,
            &source_payload_lookup,
            &host_effect_table,
        );
        let source_payload_shape_table_ms =
            typecheck_elapsed_ms(source_payload_shape_table_started);
        let source_payload_types_started = Instant::now();
        let source_payload_types = source_payload_shape_table
            .iter()
            .map(|entry| (entry.source_path.clone(), entry.payload_type.clone()))
            .collect();
        let source_payload_types_ms = typecheck_elapsed_ms(source_payload_types_started);
        let function_index_started = Instant::now();
        let function_statements = function_statement_map(&program.ast.statements);
        let function_call_graph = function_call_graph(program);
        let function_args_by_name = function_args_by_statement_map(&function_statements);
        let function_arg_call_sites = function_arg_call_site_index(program, &function_args_by_name);
        let function_index_ms = typecheck_elapsed_ms(function_index_started);
        let object_bindings_started = Instant::now();
        let object_bindings = object_bindings(program);
        let object_bindings_ms = typecheck_elapsed_ms(object_bindings_started);
        let function_param_requirements_started = Instant::now();
        let function_param_requirements = function_param_requirements(program);
        let function_param_requirements_ms =
            typecheck_elapsed_ms(function_param_requirements_started);
        let name_bindings_started = Instant::now();
        let mut name_bindings = name_bindings(
            program,
            &source_payload_types,
            &function_param_requirements,
            &function_args_by_name,
        );
        let name_bindings_ms = typecheck_elapsed_ms(name_bindings_started);
        let passed_context_started = Instant::now();
        if let Some(passed_type) = passed_context_type(program, &name_bindings) {
            name_bindings.insert("PASSED".to_owned(), passed_type);
        }
        let passed_context_ms = typecheck_elapsed_ms(passed_context_started);
        let flow_bindings_started = Instant::now();
        let flow_bindings = flow_bindings(program);
        let flow_bindings_ms = typecheck_elapsed_ms(flow_bindings_started);
        let render_contracts_started = Instant::now();
        let render_contracts =
            RenderContractRegistry::default().with_active_root(if scene_root(program).is_some() {
                "scene"
            } else {
                "document"
            });
        let render_contracts_ms = typecheck_elapsed_ms(render_contracts_started);
        let mut checker = Self {
            program,
            vars: TypeVarStore::default(),
            builtins: BuiltinSignatureRegistry::default(),
            render_contracts,
            source_paths,
            source_payload_lookup,
            source_payload_shape_table,
            source_payload_types,
            host_effect_table,
            function_statements,
            function_call_graph,
            function_args_by_name,
            function_arg_call_sites,
            function_arg_display_type_cache: RefCell::new(BTreeMap::new()),
            function_return_type_cache: RefCell::new(BTreeMap::new()),
            object_bindings,
            name_bindings,
            flow_bindings,
            function_param_requirements,
            expr_type_vars: BTreeMap::new(),
            runtime_list_map_exprs: BTreeSet::new(),
            visited: BTreeSet::new(),
            expr_type_in_progress: BTreeSet::new(),
            expr_type_cache: vec![None; program.expressions.len()],
            expr_type_table: ExprTypeTable::default(),
            function_type_table: FunctionTypeTable::default(),
            collect_type_hints: true,
            render_slot_table: RenderSlotTable::default(),
            list_map_bindings: Vec::new(),
            constraints: Vec::new(),
            diagnostics: host_effect_diagnostics,
        };
        let refresh_started = Instant::now();
        checker.refresh_static_row_scope_bindings();
        let refresh_static_row_scope_bindings_ms = typecheck_elapsed_ms(refresh_started);
        let init_profile = CheckerInitProfile {
            checker_init_ms: typecheck_elapsed_ms(checker_init_started),
            source_paths_ms,
            source_payload_shape_table_ms,
            source_payload_types_ms,
            function_index_ms,
            object_bindings_ms,
            function_param_requirements_ms,
            name_bindings_ms,
            passed_context_ms,
            flow_bindings_ms,
            render_contracts_ms,
            refresh_static_row_scope_bindings_ms,
        };
        (checker, init_profile)
    }

    fn refresh_static_row_scope_bindings(&mut self) {
        for row_scope in &self.program.row_scope_functions {
            let return_type =
                self.user_function_return_type(&row_scope.function, &mut BTreeSet::new());
            if return_type.as_ref().is_some_and(is_renderable_type) {
                continue;
            }
            let Some(row_type) = canonical_row_scope_type(
                self.program,
                &self.name_bindings,
                &self.function_param_requirements,
                &self.function_args_by_name,
                &row_scope.function,
                &row_scope.list,
                &row_scope.row_scope,
                return_type.clone(),
            ) else {
                continue;
            };
            self.name_bindings
                .insert(row_scope.row_scope.clone(), row_type.clone());
            let output_item_type = return_type
                .filter(type_has_known_user_shape)
                .unwrap_or_else(|| row_type.clone());
            if !matches!(self.name_bindings.get(&row_scope.list), Some(Type::List(_)))
                || list_map_input_item_type_for_function_param(
                    self.program,
                    &self.name_bindings,
                    &self.function_args_by_name,
                    &row_scope.function,
                    &row_scope.row_scope,
                )
                .is_none()
            {
                self.name_bindings.insert(
                    row_scope.list.clone(),
                    Type::List(Box::new(output_item_type)),
                );
            }
            if let Type::Object(shape) = &row_type {
                for (field, ty) in shape.ordered_fields() {
                    insert_simple_binding_preserving_renderable(
                        &mut self.name_bindings,
                        field,
                        ty.clone(),
                    );
                    self.name_bindings
                        .insert(format!("{}.{}", row_scope.row_scope, field), ty.clone());
                }
            }
        }
    }

    fn check_program_profiled(
        &mut self,
        include_type_hints: bool,
        init_profile: CheckerInitProfile,
    ) -> (TypeCheckReport, TypeCheckProfile) {
        let trace_typecheck = std::env::var_os("BOON_TYPECHECK_TRACE").is_some();
        let trace_phase = |phase: &str, elapsed_ms: f64| {
            if trace_typecheck {
                eprintln!("boon_typecheck {phase}: {elapsed_ms:.3}ms");
            }
        };
        let total_started = Instant::now();
        self.collect_type_hints = include_type_hints;
        let recursive_functions_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck recursive_functions:start");
        }
        self.check_recursive_functions();
        self.check_host_effect_declarations();
        let recursive_functions_ms = typecheck_elapsed_ms(recursive_functions_started);
        trace_phase("recursive_functions", recursive_functions_ms);
        let check_statements_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck check_statements:start");
        }
        for statement in &self.program.ast.statements {
            self.check_statement(statement, false);
        }
        let check_statements_ms = typecheck_elapsed_ms(check_statements_started);
        trace_phase("check_statements", check_statements_ms);
        let ensure_all_expressions_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck ensure_all_expressions:start");
        }
        if include_type_hints {
            for expr in &self.program.expressions {
                self.ensure_expr(expr.id);
            }
        }
        let ensure_all_expressions_ms = typecheck_elapsed_ms(ensure_all_expressions_started);
        trace_phase("ensure_all_expressions", ensure_all_expressions_ms);
        let report_counts_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck report_counts:start");
        }
        let render_slot_count = self.render_slot_table.slots.len();
        let render_slot_failure_count = self
            .render_slot_table
            .slots
            .iter()
            .flat_map(|slot| &slot.diagnostics)
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            .count();
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
        let report_counts_ms = typecheck_elapsed_ms(report_counts_started);
        trace_phase("report_counts", report_counts_ms);
        let type_hint_table_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck type_hint_table:start");
        }
        let type_hint_table = if include_type_hints {
            type_hint_table(
                self.program,
                &self.expr_type_table,
                &self.function_type_table,
                &self.render_slot_table,
                &source_payload_shape_table,
                &self.name_bindings,
            )
        } else {
            TypeHintTable::default()
        };
        let type_hint_table_ms = typecheck_elapsed_ms(type_hint_table_started);
        trace_phase("type_hint_table", type_hint_table_ms);
        if trace_typecheck {
            eprintln!("boon_typecheck resolved_constant_table:start");
        }
        let resolved_constant_table = resolved_constant_table(self.program);
        let output_root_types = self.check_output_roots();
        let assemble_report_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck assemble_report:start");
        }
        let report = TypeCheckReport {
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
            host_effect_table: self.host_effect_table.clone(),
            full_document_typecheck_coverage: document_root(self.program).is_none_or(|root| {
                statement_expr_ids(root)
                    .into_iter()
                    .all(|expr_id| self.visited.contains(&expr_id))
            }),
            list_map_binding_count_runtime_value,
            list_map_binding_count_render_slot_materialization,
            output_root_types,
            expr_type_table: std::mem::take(&mut self.expr_type_table),
            function_type_table: std::mem::take(&mut self.function_type_table),
            type_hint_table,
            resolved_constant_table,
            render_slot_table: std::mem::take(&mut self.render_slot_table),
            list_map_bindings: std::mem::take(&mut self.list_map_bindings),
            constraints: std::mem::take(&mut self.constraints),
            diagnostics: std::mem::take(&mut self.diagnostics),
        };
        let assemble_report_ms = typecheck_elapsed_ms(assemble_report_started);
        trace_phase("assemble_report", assemble_report_ms);
        (
            report,
            TypeCheckProfile {
                checker_init_ms: init_profile.checker_init_ms,
                source_paths_ms: init_profile.source_paths_ms,
                source_payload_shape_table_ms: init_profile.source_payload_shape_table_ms,
                source_payload_types_ms: init_profile.source_payload_types_ms,
                function_index_ms: init_profile.function_index_ms,
                object_bindings_ms: init_profile.object_bindings_ms,
                function_param_requirements_ms: init_profile.function_param_requirements_ms,
                name_bindings_ms: init_profile.name_bindings_ms,
                passed_context_ms: init_profile.passed_context_ms,
                flow_bindings_ms: init_profile.flow_bindings_ms,
                render_contracts_ms: init_profile.render_contracts_ms,
                refresh_static_row_scope_bindings_ms: init_profile
                    .refresh_static_row_scope_bindings_ms,
                recursive_functions_ms,
                check_statements_ms,
                ensure_all_expressions_ms,
                report_counts_ms,
                type_hint_table_ms,
                assemble_report_ms,
                total_ms: init_profile.checker_init_ms + typecheck_elapsed_ms(total_started),
            },
        )
    }

    fn check_statement(&mut self, statement: &AstStatement, in_document: bool) {
        if std::env::var_os("BOON_TYPECHECK_STATEMENT_TRACE").is_some() {
            eprintln!(
                "boon_typecheck statement kind={:?} expr={:?} line={} children={}",
                statement.kind,
                statement.expr,
                statement.line,
                statement.children.len()
            );
        }
        if !self.collect_type_hints && matches!(statement.kind, AstStatementKind::Function { .. }) {
            self.collect_runtime_document_contracts(statement, false);
            return;
        }
        let next_in_document = in_document
            || statement_field(statement).as_deref() == Some("document")
            || statement_field(statement).as_deref() == Some("scene")
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
        self.check_pipeline_continuation_compatibility(statement);
        self.check_pattern_constraints(statement);
        self.check_hold_update_compatibility(statement);
        self.check_latest_branch_compatibility(statement);
        if self.collect_type_hints
            && let AstStatementKind::Function { name, args } = &statement.kind
        {
            let arg_types = args
                .iter()
                .map(|arg| self.function_arg_display_type(name, arg))
                .collect();
            self.function_type_table.entries.push(FunctionTypeEntry {
                name: name.clone(),
                args: args.clone(),
                arg_types,
                result: self.function_type_hint_result(name),
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
        let saved_name_bindings = if let AstStatementKind::Function { name, args } = &statement.kind
        {
            let arg_bindings = args
                .iter()
                .map(|arg| (arg.clone(), self.function_arg_display_type(name, arg)))
                .collect::<Vec<_>>();
            let saved = self.name_bindings.clone();
            for (arg, ty) in arg_bindings {
                self.name_bindings.insert(arg, ty);
            }
            Some(saved)
        } else {
            None
        };
        for child in &statement.children {
            self.check_statement(child, next_in_document);
        }
        if let Some(saved) = saved_name_bindings {
            self.name_bindings = saved;
        }
    }

    fn check_output_roots(&mut self) -> Vec<OutputRootTypeEntry> {
        let containers = self
            .program
            .ast
            .statements
            .iter()
            .filter(|statement| {
                matches!(&statement.kind, AstStatementKind::Field { name } if name == "outputs")
            })
            .cloned()
            .collect::<Vec<_>>();
        if containers.len() > 1 {
            self.diagnostics.push(diagnostic_for_statement(
                containers.get(1),
                "Boon source may declare only one top-level `outputs` registry".to_owned(),
            ));
        }
        let Some(container) = containers.first() else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        let mut names = BTreeSet::new();
        for child in &container.children {
            if let AstStatementKind::Hold {
                field: Some(name), ..
            }
            | AstStatementKind::Source {
                field: Some(name), ..
            } = &child.kind
            {
                self.diagnostics.push(diagnostic_for_statement(
                    Some(child),
                    format!(
                        "output root `{name}` declares SOURCE or HOLD authority; outputs must be reconstructed from existing current values"
                    ),
                ));
                continue;
            }
            let name = match &child.kind {
                AstStatementKind::Field { name }
                | AstStatementKind::List {
                    field: Some(name), ..
                } => name,
                _ => {
                    if !statement_is_empty_delimiter(child, &self.program.expressions) {
                        self.diagnostics.push(diagnostic_for_statement(
                            Some(child),
                            "`outputs` accepts only named output fields".to_owned(),
                        ));
                    }
                    continue;
                }
            };
            if !names.insert(name.clone()) {
                self.diagnostics.push(diagnostic_for_statement(
                    Some(child),
                    format!("duplicate output root `{name}`"),
                ));
                continue;
            }
            if statement_contains_output_authority(child) {
                self.diagnostics.push(diagnostic_for_statement(
                    Some(child),
                    format!(
                        "output root `{name}` declares SOURCE or HOLD authority; outputs must be reconstructed from existing current values"
                    ),
                ));
            }
            let ty = direct_statement_value_expr_id(child, &self.program.expressions)
                .map(|expr_id| self.ensure_expr(expr_id).ty)
                .filter(is_specific_type)
                .or_else(|| self.static_statement_type(child, &mut BTreeSet::new()))
                .unwrap_or(Type::Unknown);
            if !host_output_type_is_closed(&ty) {
                self.diagnostics.push(diagnostic_for_statement(
                    Some(child),
                    format!(
                        "output root `{name}` must have a closed scalar, record, or list host-value type; found {}",
                        boon_facing_type_label(&ty)
                    ),
                ));
            }
            entries.push(OutputRootTypeEntry {
                name: name.clone(),
                statement_id: child.id,
                value_expr_id: direct_statement_value_expr_id(child, &self.program.expressions),
                ty,
            });
        }
        if entries.is_empty() {
            self.diagnostics.push(diagnostic_for_statement(
                Some(container),
                "`outputs` must declare at least one named output root".to_owned(),
            ));
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        entries
    }

    fn collect_runtime_document_contracts(&mut self, statement: &AstStatement, in_document: bool) {
        let next_in_document = in_document
            || statement_field(statement).as_deref() == Some("document")
            || statement_field(statement).as_deref() == Some("scene")
            || self.statement_enters_render_context(statement);
        if next_in_document
            && matches!(
                statement_field(statement).as_deref(),
                Some("root" | "child" | "items" | "children")
            )
        {
            let diagnostic_count = self.diagnostics.len();
            self.check_render_slot(statement);
            self.diagnostics.truncate(diagnostic_count);
        }
        for child in &statement.children {
            self.collect_runtime_document_contracts(child, next_in_document);
        }
    }

    fn function_arg_display_type(&self, function: &str, arg: &str) -> Type {
        if self
            .program
            .row_scope_functions
            .iter()
            .any(|row_scope| row_scope.function == function && row_scope.row_scope == arg)
        {
            if let Some(ty) = list_map_input_item_type_for_function_param(
                self.program,
                &self.name_bindings,
                &self.function_args_by_name,
                function,
                arg,
            ) {
                return ty;
            }
            if let Some(ty) = self.name_bindings.get(arg) {
                return ty.clone();
            }
        }
        let cache_key = (function.to_owned(), arg.to_owned());
        if let Some(cached) = self
            .function_arg_display_type_cache
            .borrow()
            .get(&cache_key)
            .cloned()
        {
            return cached;
        }
        let requirement = self
            .function_param_requirements
            .get(function)
            .and_then(|requirements| requirements.get(arg))
            .cloned();
        let ty = if let Some(ty) = self.function_arg_call_site_type(function, arg) {
            requirement
                .as_ref()
                .map(|requirement| merge_canonical_row_type(&ty, requirement))
                .unwrap_or(ty)
        } else {
            requirement.unwrap_or_else(open_object_type)
        };
        self.function_arg_display_type_cache
            .borrow_mut()
            .insert(cache_key, ty.clone());
        ty
    }

    fn function_type_hint_result(&self, function: &str) -> FlowType {
        let builtin = self
            .builtins
            .type_for_call(function, &self.render_contracts);
        if !matches!(builtin, Type::Unknown) {
            return FlowType {
                mode: FlowMode::Continuous,
                ty: builtin,
            };
        }
        let cached = self
            .function_return_type_cache
            .borrow()
            .get(function)
            .cloned()
            .flatten();
        FlowType {
            mode: FlowMode::Continuous,
            ty: cached.unwrap_or_else(|| {
                if self.program.functions.iter().any(|name| name == function) {
                    open_object_type()
                } else {
                    Type::Unknown
                }
            }),
        }
    }

    fn function_arg_call_site_type(&self, function: &str, arg: &str) -> Option<Type> {
        let arg_expr_ids = self.function_arg_call_sites.get(function)?.get(arg)?;
        let mut ty = list_map_input_item_type_for_function_param(
            self.program,
            &self.name_bindings,
            &self.function_args_by_name,
            function,
            arg,
        );
        for arg_expr_id in arg_expr_ids {
            let Some(arg_expr) = self.program.expressions.get(*arg_expr_id) else {
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
            statement_pipeline_final_expr_id(statement, &self.program.expressions)
                .or_else(|| direct_statement_value_expr_id(statement, &self.program.expressions));
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
        if let Some(static_type) = self
            .static_statement_type(statement, &mut BTreeSet::new())
            .filter(|ty| render_slot_accepts_type(&slot_name, ty))
        {
            actual_type = static_type;
        }
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
            if field == "style" {
                self.check_style_statement(child);
            }
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
            let mut actual = self.ensure_expr(arg.value).ty;
            if !render_field_type_accepts(&actual, &expected)
                && let Some(static_actual) = self.static_expr_type_for_pipeline_expr(
                    arg.value,
                    &mut BTreeSet::new(),
                    &self.name_bindings,
                )
            {
                actual = static_actual;
            }
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
            .expr_type_cache
            .get(expr_id)
            .and_then(|entry| entry.as_ref())
            .cloned()
        {
            return existing;
        }
        let expr_var = self.expr_type_var_key(expr_id);
        if self.expr_type_in_progress.contains(&expr_id) {
            return FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(expr_var),
            };
        }
        self.expr_type_in_progress.insert(expr_id);
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
        self.expr_type_in_progress.remove(&expr_id);
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
        if let Some(slot) = self.expr_type_cache.get_mut(expr_id) {
            *slot = Some(flow_type.clone());
        }
        flow_type
    }

    fn infer_expr(&mut self, expr: &AstExpr) -> FlowType {
        let ty = match &expr.kind {
            AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Type::Text,
            AstExprKind::Number(_) => Type::Number,
            AstExprKind::ByteLiteral { .. } => Type::Byte,
            AstExprKind::BytesLiteral { size, items } => {
                self.infer_bytes_literal(expr, size, items)
            }
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
                        .filter(|field| !field.spread)
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
                Type::Object(self.infer_record_shape(fields))
            }
            AstExprKind::Drain { path } => self.type_for_path(expr.id, &drain_path_parts(path)),
            AstExprKind::ListLiteral { .. } => Type::List(Box::new(open_object_type())),
            AstExprKind::Call { function, args } => {
                for arg in args {
                    self.ensure_expr(arg.value);
                }
                self.check_bytes_builtin_arguments(expr.id, function, args, None);
                self.check_builtin_call_compatibility(function, None, args);
                self.check_user_function_arguments(expr.id, function, None, args);
                if self.render_contracts.is_render_constructor(function) {
                    self.check_style_args(args);
                    self.check_render_constructor_call_args(expr.id, function, None, args);
                }
                if function == "Bool/not" || function == "Bool/toggle" {
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
                } else if let Some(ty) = self.contextual_bytes_result_type(function, None, args) {
                    ty
                } else {
                    self.type_for_call_expr(expr.id, function)
                }
            }
            AstExprKind::Pipe { input, op, args } => {
                let input_flow = self.ensure_expr(*input);
                let input_is_placeholder = self.expr_id_is_pipe_placeholder(*input);
                for arg in args {
                    self.ensure_expr(arg.value);
                }
                self.check_bytes_builtin_arguments(expr.id, op, args, Some(*input));
                self.check_builtin_call_compatibility(op, Some(*input), args);
                if !op.starts_with("Field/") {
                    self.check_user_function_arguments(expr.id, op, Some(*input), args);
                }
                if self.render_contracts.is_render_constructor(op) {
                    self.check_style_args(args);
                    self.check_render_constructor_call_args(expr.id, op, Some(&input_flow), args);
                }
                if let Some(field) = op.strip_prefix("Field/") {
                    match &input_flow.ty {
                        Type::Object(shape) => {
                            shape.fields.get(field).cloned().unwrap_or(Type::Unknown)
                        }
                        Type::Unknown => Type::Unknown,
                        _ => Type::Unknown,
                    }
                } else if op == "List/map" {
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
                } else if matches!(op.as_str(), "List/every" | "List/any" | "List/is_not_empty") {
                    true_false_type()
                } else if op == "List/latest" {
                    if input_is_placeholder {
                        open_object_type()
                    } else {
                        list_item_type_from_list_type(&input_flow.ty)
                            .unwrap_or_else(open_object_type)
                    }
                } else if op == "SOURCE" {
                    input_flow.ty
                } else if matches!(op.as_str(), "List/retain" | "List/remove") {
                    if input_is_placeholder {
                        Type::List(Box::new(open_object_type()))
                    } else {
                        input_flow.ty
                    }
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
                    self.when_result_type(expr.id)
                        .unwrap_or_else(|| self.type_for_call_expr(expr.id, op))
                } else if self.render_contracts.is_render_constructor(op) {
                    self.render_constructor_type_for_args(op, Some(&input_flow), args)
                } else if op == "Bool/not" || op == "Bool/toggle" {
                    if !input_is_placeholder {
                        self.check_true_false_input(expr, op, &input_flow);
                    }
                    true_false_type()
                } else if op == "Bool/and" {
                    if !input_is_placeholder {
                        self.check_true_false_input(expr, op, &input_flow);
                    }
                    for arg in args {
                        let arg_flow = self.ensure_expr(arg.value);
                        self.check_true_false_input(expr, op, &arg_flow);
                    }
                    true_false_type()
                } else if let Some(ty) = self.contextual_bytes_result_type(op, Some(*input), args) {
                    ty
                } else {
                    self.type_for_call_expr(expr.id, op)
                }
            }
            AstExprKind::Draining { input } => {
                let input = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr.id,
                    *input,
                    &self.program.expressions,
                );
                self.ensure_expr(input).ty
            }
            AstExprKind::Hold { initial, .. } => {
                let initial = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr.id,
                    *initial,
                    &self.program.expressions,
                );
                self.hold_result_type(expr.id, initial)
            }
            AstExprKind::Latest => self
                .latest_result_type(expr.id)
                .unwrap_or_else(exact_empty_object_type),
            AstExprKind::When { input } => {
                if self.expr_id_is_bytes_source_payload_path(*input) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        *input,
                        "BYTES source payload guards are not supported in v1; use `THEN` to route the BYTES payload or convert it explicitly before matching".to_owned(),
                    ));
                }
                self.when_result_type(expr.id)
                    .unwrap_or_else(|| self.ensure_expr(*input).ty)
            }
            AstExprKind::Then { input, output } => {
                let input_flow = self.ensure_expr(*input);
                if !matches!(
                    input_flow.mode,
                    FlowMode::TickPresent | FlowMode::PresentOrAbsent
                ) && !self.expr_id_is_event_payload_path(*input)
                    && !self.expr_id_is_pipe_placeholder(*input)
                    && !matches!(input_flow.ty, Type::Unknown)
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
            AstExprKind::Unknown(tokens) if unknown_tokens_are_quoted_text(tokens) => Type::Text,
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

    fn infer_bytes_literal(
        &mut self,
        expr: &AstExpr,
        size: &BytesSizeSyntax,
        items: &[usize],
    ) -> Type {
        let mut known_len = 0usize;
        let mut all_fixed = true;
        for item in items {
            let item_flow = self.ensure_expr(*item);
            match item_flow.ty {
                Type::Byte => known_len += 1,
                Type::Bytes(BytesType::Fixed(len)) => known_len += len,
                Type::Bytes(BytesType::Dynamic) => {
                    all_fixed = false;
                    if !matches!(size, BytesSizeSyntax::Dynamic) {
                        self.diagnostics.push(self.diagnostic_for_expr(
                            *item,
                            "fixed BYTES constructors cannot contain dynamic BYTES".to_owned(),
                        ));
                    }
                }
                Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. } => {
                    all_fixed = false;
                }
                other => {
                    all_fixed = false;
                    self.diagnostics.push(self.diagnostic_for_expr(
                        *item,
                        format!(
                            "BYTES constructor items must be byte literals or BYTES values, found {}; use Text/to_bytes for explicit TEXT/BYTES conversion",
                            boon_facing_type_label(&other)
                        ),
                    ));
                }
            }
        }
        match size {
            BytesSizeSyntax::Dynamic => Type::Bytes(BytesType::Dynamic),
            BytesSizeSyntax::Infer => {
                if all_fixed {
                    Type::Bytes(BytesType::Fixed(known_len))
                } else {
                    self.diagnostics.push(
                        self.diagnostic_for_expr(
                            expr.id,
                            "BYTES[__] length cannot be inferred from dynamic or unknown content"
                                .to_owned(),
                        ),
                    );
                    Type::Bytes(BytesType::Dynamic)
                }
            }
            BytesSizeSyntax::Fixed(expected) => {
                if !items.is_empty() && all_fixed && known_len != *expected {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        expr.id,
                        format!(
                            "BYTES[{expected}] contains {known_len} byte(s); fixed BYTES length must match exactly"
                        ),
                    ));
                }
                Type::Bytes(BytesType::Fixed(*expected))
            }
        }
    }

    fn check_bytes_builtin_arguments(
        &mut self,
        expr_id: usize,
        function: &str,
        args: &[AstCallArg],
        piped_input: Option<usize>,
    ) {
        if !is_bytes_boundary_builtin(function) {
            return;
        }
        let piped = piped_input.is_some();
        self.check_bytes_builtin_allowed_args(expr_id, function, args, piped);
        self.check_bytes_builtin_required_args(expr_id, function, args, piped);

        if matches!(function, "Text/to_bytes" | "Bytes/to_text") {
            self.check_bytes_encoding_argument(expr_id, function, args);
        }

        if matches!(
            function,
            "Bytes/read_unsigned"
                | "Bytes/read_signed"
                | "Bytes/write_unsigned"
                | "Bytes/write_signed"
        ) {
            self.check_bytes_numeric_arguments(expr_id, args);
        }
        self.check_bytes_static_integer_argument_overflow(function, args);
        self.check_bytes_static_bounds(expr_id, function, piped_input, args);
        self.check_bytes_static_text_conversion(expr_id, function, piped_input, args);
    }

    fn check_bytes_builtin_allowed_args(
        &mut self,
        expr_id: usize,
        function: &str,
        args: &[AstCallArg],
        piped: bool,
    ) {
        for arg in args {
            let Some(name) = arg.name.as_deref() else {
                self.diagnostics.push(self.diagnostic_for_expr(
                    arg.value,
                    format!(
                        "`{function}` requires named arguments; positional BYTES builtin arguments are ambiguous"
                    ),
                ));
                continue;
            };
            if bytes_builtin_arg_allowed(function, name, piped) {
                continue;
            }
            self.diagnostics.push(self.diagnostic_for_expr(
                arg.value,
                format!("`{function}` does not accept argument `{name}`"),
            ));
        }
        if function == "Bytes/zeros"
            && (piped
                || args
                    .iter()
                    .any(|arg| arg.name.as_deref().is_some_and(|name| name == "input")))
        {
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                "`Bytes/zeros` creates BYTES and does not accept an input BYTES value".to_owned(),
            ));
        }
    }

    fn check_bytes_builtin_required_args(
        &mut self,
        expr_id: usize,
        function: &str,
        args: &[AstCallArg],
        piped: bool,
    ) {
        let has_input =
            piped || has_any_named_arg(args, &["input", "text"]) || has_unnamed_arg(args);
        let missing_input = || {
            format!(
                "`{function}` requires an input {} value",
                if matches!(
                    function,
                    "Text/to_bytes" | "Bytes/from_hex" | "Bytes/from_base64"
                ) {
                    "TEXT"
                } else {
                    "BYTES"
                }
            )
        };
        match function {
            "Text/to_bytes" | "Bytes/from_hex" | "Bytes/from_base64" => {
                if !has_input {
                    self.diagnostics
                        .push(self.diagnostic_for_expr(expr_id, missing_input()));
                }
            }
            "Bytes/length"
            | "Bytes/is_empty"
            | "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/find"
            | "Bytes/starts_with"
            | "Bytes/ends_with"
            | "Bytes/to_text"
            | "Bytes/to_hex"
            | "Bytes/to_base64"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed" => {
                let pair_input = matches!(function, "Bytes/concat" | "Bytes/equal")
                    && has_any_named_arg(args, &["left", "right"]);
                if !has_input && !pair_input {
                    self.diagnostics
                        .push(self.diagnostic_for_expr(expr_id, missing_input()));
                }
            }
            "Bytes/concat" | "Bytes/equal" => {
                if has_any_named_arg(args, &["left", "right"]) {
                    self.require_one_of(expr_id, function, args, &["left"], "left BYTES input");
                    self.require_one_of(expr_id, function, args, &["right"], "right BYTES input");
                } else {
                    if !has_input {
                        self.diagnostics
                            .push(self.diagnostic_for_expr(expr_id, missing_input()));
                    }
                    self.require_one_of(expr_id, function, args, &["with"], "second BYTES input");
                }
            }
            "Bytes/zeros" => {}
            _ => {}
        }

        match function {
            "Bytes/get" => self.require_one_of(expr_id, function, args, &["index"], "`index`"),
            "Bytes/set" => {
                self.require_one_of(expr_id, function, args, &["index"], "`index`");
                self.require_one_of(expr_id, function, args, &["value"], "`value`");
            }
            "Bytes/slice" => {
                self.require_one_of(expr_id, function, args, &["offset", "start"], "`offset`");
                self.require_one_of(
                    expr_id,
                    function,
                    args,
                    &["byte_count", "length", "count"],
                    "`byte_count`",
                );
            }
            "Bytes/take" | "Bytes/drop" | "Bytes/zeros" => self.require_one_of(
                expr_id,
                function,
                args,
                &["byte_count", "length", "count"],
                "`byte_count`",
            ),
            "Bytes/find" => self.require_one_of(expr_id, function, args, &["needle"], "`needle`"),
            "Bytes/starts_with" => {
                self.require_one_of(expr_id, function, args, &["prefix"], "`prefix`");
            }
            "Bytes/ends_with" => {
                self.require_one_of(expr_id, function, args, &["suffix"], "`suffix`");
            }
            "Bytes/read_unsigned" | "Bytes/read_signed" => {
                self.require_one_of(expr_id, function, args, &["offset"], "`offset`");
                self.require_one_of(expr_id, function, args, &["byte_count"], "`byte_count`");
                self.require_one_of(expr_id, function, args, &["endian"], "`endian: Little|Big`");
            }
            "Bytes/write_unsigned" | "Bytes/write_signed" => {
                self.require_one_of(expr_id, function, args, &["offset"], "`offset`");
                self.require_one_of(expr_id, function, args, &["byte_count"], "`byte_count`");
                self.require_one_of(expr_id, function, args, &["endian"], "`endian: Little|Big`");
                self.require_one_of(expr_id, function, args, &["value"], "`value`");
            }
            _ => {}
        }
    }

    fn require_one_of(
        &mut self,
        expr_id: usize,
        function: &str,
        args: &[AstCallArg],
        names: &[&str],
        label: &str,
    ) {
        if has_any_named_arg(args, names) {
            return;
        }
        self.diagnostics
            .push(self.diagnostic_for_expr(expr_id, format!("`{function}` requires {label}")));
    }

    fn check_bytes_encoding_argument(
        &mut self,
        expr_id: usize,
        function: &str,
        args: &[AstCallArg],
    ) {
        match named_arg_expr(args, "encoding").and_then(|arg| self.program.expressions.get(arg)) {
            Some(AstExpr {
                kind:
                    AstExprKind::Tag(value) | AstExprKind::Enum(value) | AstExprKind::Identifier(value),
                ..
            }) if value == "Utf8" || value == "Ascii" => {}
            Some(expr) => self.diagnostics.push(
                self.diagnostic_for_expr(
                    expr.id,
                    "`encoding` must be `Utf8` or `Ascii` for explicit TEXT/BYTES conversion"
                        .to_owned(),
                ),
            ),
            None => self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!("`{function}` requires explicit `encoding: Utf8|Ascii`"),
            )),
        }
    }

    fn check_bytes_numeric_arguments(&mut self, expr_id: usize, args: &[AstCallArg]) {
        match named_arg_expr(args, "endian").and_then(|arg| self.program.expressions.get(arg)) {
            Some(AstExpr {
                kind:
                    AstExprKind::Tag(value) | AstExprKind::Enum(value) | AstExprKind::Identifier(value),
                ..
            }) if value == "Little" || value == "Big" => {}
            Some(expr) => self.diagnostics.push(
                self.diagnostic_for_expr(
                    expr.id,
                    "`endian` must be `Little` or `Big` for multi-byte BYTES numeric operations"
                        .to_owned(),
                ),
            ),
            None => self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                "BYTES numeric operations require explicit `endian: Little|Big`".to_owned(),
            )),
        }

        match named_arg_expr(args, "byte_count").and_then(|arg| self.program.expressions.get(arg)) {
            Some(expr) if matches!(self.static_integer_literal(expr.id), Some(1 | 2 | 4 | 8)) => {}
            Some(expr) => self.diagnostics.push(self.diagnostic_for_expr(
                expr.id,
                "`byte_count` for BYTES numeric operations must be 1, 2, 4, or 8 in v1".to_owned(),
            )),
            None => self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                "BYTES numeric operations require explicit `byte_count`".to_owned(),
            )),
        }
    }

    fn check_bytes_static_integer_argument_overflow(
        &mut self,
        function: &str,
        args: &[AstCallArg],
    ) {
        for arg in args {
            let Some(name) = arg.name.as_deref() else {
                continue;
            };
            if !matches!(
                (function, name),
                ("Bytes/get", "index")
                    | ("Bytes/set", "index")
                    | (
                        "Bytes/slice",
                        "offset" | "start" | "byte_count" | "length" | "count"
                    )
                    | (
                        "Bytes/take" | "Bytes/drop" | "Bytes/zeros",
                        "byte_count" | "length" | "count"
                    )
                    | (
                        "Bytes/read_unsigned"
                            | "Bytes/read_signed"
                            | "Bytes/write_unsigned"
                            | "Bytes/write_signed",
                        "offset" | "byte_count" | "value"
                    )
            ) {
                continue;
            }
            match static_integer_expr_checked(self.program, arg.value) {
                Err(StaticIntegerExprError::Overflow) => {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        arg.value,
                        format!(
                            "`{function}` argument `{name}` static integer expression overflows Boon's supported integer range"
                        ),
                    ));
                }
                Ok(None) if unsupported_literal_static_integer_expr(self.program, arg.value) => {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        arg.value,
                        format!(
                            "`{function}` argument `{name}` requires a static integer expression using integer literals and checked `+`, `-`, or `*`"
                        ),
                    ));
                }
                Ok(Some(value))
                    if bytes_static_integer_arg_is_out_of_plan_range(function, name, value) =>
                {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        arg.value,
                        format!(
                            "`{function}` argument `{name}` static integer value {value} is outside MachinePlan's supported integer range"
                        ),
                    ));
                }
                _ => {}
            }
        }
    }

    fn check_bytes_static_bounds(
        &mut self,
        _expr_id: usize,
        function: &str,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) {
        let Some(Type::Bytes(BytesType::Fixed(len))) =
            self.bytes_named_input_type(piped_input, args)
        else {
            return;
        };
        let len = len as i128;

        match function {
            "Bytes/get" | "Bytes/set" => {
                let Some((index_expr, index)) = self.static_integer_arg(args, &["index"]) else {
                    return;
                };
                if index < 0 || index >= len {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        index_expr,
                        format!(
                            "`{function}` index {index} is out of bounds for fixed BYTES[{len}]"
                        ),
                    ));
                }
            }
            "Bytes/slice" => {
                let Some((_offset_expr, offset)) =
                    self.static_integer_arg(args, &["offset", "start"])
                else {
                    return;
                };
                let Some((count_expr, count)) =
                    self.static_integer_arg(args, &["byte_count", "length", "count"])
                else {
                    return;
                };
                self.check_bytes_static_range(count_expr, function, len, offset, count);
            }
            "Bytes/take" | "Bytes/drop" => {
                let Some((count_expr, count)) =
                    self.static_integer_arg(args, &["byte_count", "length", "count"])
                else {
                    return;
                };
                self.check_bytes_static_range(count_expr, function, len, 0, count);
            }
            "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed" => {
                let Some((_offset_expr, offset)) = self.static_integer_arg(args, &["offset"])
                else {
                    return;
                };
                let Some((count_expr, count)) = self.static_integer_arg(args, &["byte_count"])
                else {
                    return;
                };
                self.check_bytes_static_range(count_expr, function, len, offset, count);
            }
            _ => {}
        }
    }

    fn check_bytes_static_range(
        &mut self,
        expr_id: usize,
        function: &str,
        len: i128,
        offset: i128,
        count: i128,
    ) {
        let end = offset.checked_add(count);
        if offset < 0 || count < 0 || end.is_none_or(|end| end > len) {
            let range_end = end
                .map(|value| value.to_string())
                .unwrap_or_else(|| "overflow".to_owned());
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!(
                    "`{function}` byte range {offset}..{range_end} is out of bounds for fixed BYTES[{len}]"
                ),
            ));
        }
    }

    fn check_bytes_static_text_conversion(
        &mut self,
        _expr_id: usize,
        function: &str,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) {
        match function {
            "Text/to_bytes" => {
                let Some((input_expr, text)) = self.static_text_input(piped_input, args) else {
                    return;
                };
                let Some(encoding) = self.static_encoding_arg(args) else {
                    return;
                };
                if encoding == "Ascii" && !text.is_ascii() {
                    self.diagnostics.push(
                        self.diagnostic_for_expr(
                            input_expr,
                            "`Text/to_bytes` with `encoding: Ascii` requires ASCII input text"
                                .to_owned(),
                        ),
                    );
                }
            }
            "Bytes/from_hex" => {
                let Some((input_expr, text)) = self.static_text_input(piped_input, args) else {
                    return;
                };
                if static_hex_decoded_len(&text).is_none() {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        input_expr,
                        "`Bytes/from_hex` requires static hex text with an even number of valid hex digits".to_owned(),
                    ));
                }
            }
            "Bytes/from_base64" => {
                let Some((input_expr, text)) = self.static_text_input(piped_input, args) else {
                    return;
                };
                if static_base64_decoded_len(&text).is_none() {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        input_expr,
                        "`Bytes/from_base64` requires valid static base64 text".to_owned(),
                    ));
                }
            }
            _ => {}
        }
    }

    fn infer_record_shape(&mut self, fields: &[AstRecordField]) -> ObjectShape {
        let mut shape_fields = BTreeMap::new();
        let mut field_order = Vec::new();
        let mut explicit_fields = BTreeSet::new();
        for field in fields {
            let ty = self.ensure_expr(field.value).ty;
            if field.spread {
                match ty {
                    Type::Object(shape) => {
                        merge_shape_override(&mut shape_fields, &mut field_order, &shape);
                    }
                    Type::VariantSet(ref variants)
                        if variants.iter().any(
                            |variant| matches!(variant, Variant::Tag(tag) if tag == "UNPLUGGED"),
                        ) => {}
                    Type::Unknown | Type::UnresolvedShape { .. } | Type::Var(_) => {}
                    _ => self.diagnostics.push(self.diagnostic_for_expr(
                        field.value,
                        "record spread expects a record value".to_owned(),
                    )),
                }
                continue;
            }
            if !explicit_fields.insert(field.name.clone()) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    field.value,
                    format!("duplicate explicit record field `{}`", field.name),
                ));
            }
            insert_shape_field_override(
                &mut shape_fields,
                &mut field_order,
                field.name.clone(),
                ty,
            );
        }
        ObjectShape {
            fields: shape_fields,
            field_order,
            open: false,
        }
    }

    fn type_for_path(&mut self, expr_id: usize, parts: &[String]) -> Type {
        let path = parts.join(".");
        if path == "element.hovered" {
            return true_false_type();
        }
        if let Some(access) = self.source_payload_lookup.access_for_parts(parts) {
            match access {
                SourcePayloadAccess::Direct(source_path) => {
                    return source_payload_type_for_path(&self.source_payload_types, &source_path)
                        .unwrap_or_else(exact_empty_object_type);
                }
                SourcePayloadAccess::Field(field) => {
                    return declared_source_payload_field_type(
                        &self.source_payload_lookup,
                        &self.source_payload_types,
                        parts,
                        &field,
                    )
                    .unwrap_or_else(|| source_payload_field_type(&field));
                }
                SourcePayloadAccess::UnknownField(field) => {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        expr_id,
                        format!(
                            "unsupported nested source payload path `{field}`\nsource payload fields are open text payloads; event.press, event.click, event.double_click, event.blur, event.change, and event.key_down are event objects"
                        ),
                    ));
                    return self.expr_type_var(expr_id);
                }
            }
        }
        if let Some(ty) = self.name_bindings.get(&path) {
            return ty.clone();
        }
        if parts.first().is_some_and(|part| part == "PASSED") && parts.len() > 1 {
            let passed_path = parts[1..].join(".");
            if let Some(ty) = self.name_bindings.get(&passed_path) {
                return ty.clone();
            }
            if let Some(base) = parts.get(1).and_then(|part| self.name_bindings.get(part))
                && parts.len() > 2
                && let Some(ty) = type_for_nested_path(base, &parts[2..])
            {
                return ty;
            }
            if let Some(ty) = parts.last().and_then(|field| self.name_bindings.get(field)) {
                return ty.clone();
            }
        }
        if let Some(base) = parts.first().and_then(|part| self.name_bindings.get(part))
            && parts.len() > 1
            && let Some(ty) = type_for_nested_path(base, &parts[1..])
        {
            return ty;
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

    fn static_type_for_path(&self, parts: &[String]) -> Option<Type> {
        let path = parts.join(".");
        if let Some(access) = self.source_payload_lookup.access_for_parts(parts) {
            return match access {
                SourcePayloadAccess::Direct(source_path) => {
                    source_payload_type_for_path(&self.source_payload_types, &source_path)
                }
                SourcePayloadAccess::Field(field) => declared_source_payload_field_type(
                    &self.source_payload_lookup,
                    &self.source_payload_types,
                    parts,
                    &field,
                )
                .or_else(|| Some(source_payload_field_type(&field))),
                SourcePayloadAccess::UnknownField(_) => None,
            };
        }
        self.name_bindings.get(&path).cloned().or_else(|| {
            if parts.first().is_some_and(|part| part == "PASSED") && parts.len() > 1 {
                let passed_path = parts[1..].join(".");
                if let Some(ty) = self.name_bindings.get(&passed_path) {
                    return Some(ty.clone());
                }
                if let Some(base) = parts.get(1).and_then(|part| self.name_bindings.get(part))
                    && parts.len() > 2
                {
                    return type_for_nested_path(base, &parts[2..]);
                }
                if let Some(ty) = parts.last().and_then(|field| self.name_bindings.get(field)) {
                    return Some(ty.clone());
                }
            }
            parts
                .first()
                .and_then(|base| self.name_bindings.get(base))
                .and_then(|base| type_for_nested_path(base, &parts[1..]))
        })
    }

    fn type_for_call(&self, function: &str) -> Type {
        let ty = self
            .builtins
            .type_for_call(function, &self.render_contracts);
        if !matches!(ty, Type::Unknown) {
            return ty;
        }
        if self.program.functions.iter().any(|name| name == function) {
            self.user_function_return_type(function, &mut BTreeSet::new())
                .filter(is_specific_type)
                .unwrap_or_else(open_object_type)
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

    fn contextual_bytes_result_type(
        &mut self,
        function: &str,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Option<Type> {
        match function {
            "Bytes/set" | "Bytes/write_unsigned" | "Bytes/write_signed" => Some(
                self.bytes_input_type(piped_input, args)
                    .unwrap_or(Type::Bytes(BytesType::Dynamic)),
            ),
            "Bytes/slice" | "Bytes/take" => Some(Type::Bytes(
                self.static_bytes_count(args)
                    .map(BytesType::Fixed)
                    .unwrap_or(BytesType::Dynamic),
            )),
            "Bytes/drop" => Some(Type::Bytes(
                match (
                    self.bytes_input_type(piped_input, args),
                    self.static_bytes_count(args),
                ) {
                    (Some(Type::Bytes(BytesType::Fixed(len))), Some(count)) if count <= len => {
                        BytesType::Fixed(len - count)
                    }
                    _ => BytesType::Dynamic,
                },
            )),
            "Bytes/concat" => Some(Type::Bytes(
                match (
                    self.bytes_pair_left_type(piped_input, args),
                    self.bytes_pair_right_type(args),
                ) {
                    (
                        Some(Type::Bytes(BytesType::Fixed(left))),
                        Some(Type::Bytes(BytesType::Fixed(right))),
                    ) => left
                        .checked_add(right)
                        .map(BytesType::Fixed)
                        .unwrap_or(BytesType::Dynamic),
                    _ => BytesType::Dynamic,
                },
            )),
            "Bytes/zeros" => Some(Type::Bytes(
                self.static_bytes_count(args)
                    .map(BytesType::Fixed)
                    .unwrap_or(BytesType::Dynamic),
            )),
            "Text/to_bytes" => Some(Type::Bytes(
                self.static_text_to_bytes_len(piped_input, args)
                    .map(BytesType::Fixed)
                    .unwrap_or(BytesType::Dynamic),
            )),
            "Bytes/from_hex" => Some(Type::Bytes(
                self.static_text_input(piped_input, args)
                    .and_then(|(_, text)| static_hex_decoded_len(&text))
                    .map(BytesType::Fixed)
                    .unwrap_or(BytesType::Dynamic),
            )),
            "Bytes/from_base64" => Some(Type::Bytes(
                self.static_text_input(piped_input, args)
                    .and_then(|(_, text)| static_base64_decoded_len(&text))
                    .map(BytesType::Fixed)
                    .unwrap_or(BytesType::Dynamic),
            )),
            _ => None,
        }
    }

    fn bytes_input_type(
        &mut self,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Option<Type> {
        if let Some(input) = piped_input {
            return Some(self.ensure_expr(input).ty);
        }
        self.arg_expr_for_names_or_unnamed(args, &["input", "left"])
            .map(|expr_id| self.ensure_expr(expr_id).ty)
    }

    fn bytes_named_input_type(
        &mut self,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Option<Type> {
        if let Some(input) = piped_input {
            return Some(self.ensure_expr(input).ty);
        }
        ["input", "left"]
            .iter()
            .find_map(|name| named_arg_expr(args, name))
            .map(|expr_id| self.ensure_expr(expr_id).ty)
    }

    fn bytes_pair_left_type(
        &mut self,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Option<Type> {
        if let Some(input) = piped_input {
            return Some(self.ensure_expr(input).ty);
        }
        self.arg_expr_for_names_or_unnamed_at(args, &["left", "input"], 0)
            .map(|expr_id| self.ensure_expr(expr_id).ty)
    }

    fn bytes_pair_right_type(&mut self, args: &[AstCallArg]) -> Option<Type> {
        self.arg_expr_for_names_or_unnamed_at(args, &["right", "with"], 1)
            .map(|expr_id| self.ensure_expr(expr_id).ty)
    }

    fn arg_expr_for_names_or_unnamed(&self, args: &[AstCallArg], names: &[&str]) -> Option<usize> {
        self.arg_expr_for_names_or_unnamed_at(args, names, 0)
    }

    fn arg_expr_for_names_or_unnamed_at(
        &self,
        args: &[AstCallArg],
        names: &[&str],
        unnamed_index: usize,
    ) -> Option<usize> {
        args.iter()
            .find(|arg| {
                arg.name
                    .as_deref()
                    .is_some_and(|name| names.contains(&name))
            })
            .or_else(|| {
                args.iter()
                    .filter(|arg| arg.name.is_none())
                    .nth(unnamed_index)
            })
            .map(|arg| arg.value)
    }

    fn static_bytes_count(&self, args: &[AstCallArg]) -> Option<usize> {
        ["byte_count", "length", "count"].iter().find_map(|name| {
            named_arg_expr(args, name).and_then(|expr_id| self.static_usize_literal(expr_id))
        })
    }

    fn static_text_to_bytes_len(
        &self,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Option<usize> {
        let (_, text) = self.static_text_input(piped_input, args)?;
        match self.static_encoding_arg(args)?.as_str() {
            "Utf8" => Some(text.len()),
            "Ascii" if text.is_ascii() => Some(text.len()),
            _ => None,
        }
    }

    fn static_text_input(
        &self,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Option<(usize, String)> {
        let expr_id = piped_input.or_else(|| {
            ["input", "text"]
                .iter()
                .find_map(|name| named_arg_expr(args, name))
        })?;
        self.static_text_literal(expr_id)
            .map(|text| (expr_id, text.to_owned()))
    }

    fn static_text_literal(&self, expr_id: usize) -> Option<&str> {
        match &self.program.expressions.get(expr_id)?.kind {
            AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => Some(value),
            _ => None,
        }
    }

    fn static_encoding_arg(&self, args: &[AstCallArg]) -> Option<String> {
        let expr = self
            .program
            .expressions
            .get(named_arg_expr(args, "encoding")?)?;
        match &expr.kind {
            AstExprKind::Tag(value) | AstExprKind::Enum(value) | AstExprKind::Identifier(value)
                if value == "Utf8" || value == "Ascii" =>
            {
                Some(value.clone())
            }
            _ => None,
        }
    }

    fn static_integer_arg(&self, args: &[AstCallArg], names: &[&str]) -> Option<(usize, i128)> {
        names.iter().find_map(|name| {
            named_arg_expr(args, name).and_then(|expr_id| {
                self.static_integer_literal(expr_id)
                    .map(|value| (expr_id, value))
            })
        })
    }

    fn static_usize_literal(&self, expr_id: usize) -> Option<usize> {
        let value = static_integer_expr(self.program, expr_id)?;
        usize::try_from(value).ok()
    }

    fn static_integer_literal(&self, expr_id: usize) -> Option<i128> {
        static_integer_expr(self.program, expr_id)
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
        *self
            .expr_type_vars
            .entry(expr_id)
            .or_insert_with(|| self.vars.new_var())
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
        let arm_expr_ids = when_arm_expr_ids(
            &self.program.ast.statements,
            expr_id,
            &self.program.expressions,
        );
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

    fn latest_result_type(&mut self, expr_id: usize) -> Option<Type> {
        let branch_expr_ids = latest_branch_expr_ids(
            &self.program.ast.statements,
            expr_id,
            &self.program.expressions,
        );
        let mut result: Option<Type> = None;
        for branch_expr_id in branch_expr_ids {
            let branch_type = self.ensure_expr(branch_expr_id).ty;
            if matches!(branch_type, Type::Skip) {
                continue;
            }
            result = Some(match result {
                Some(existing) => widen_structural_type(&existing, &branch_type),
                None => branch_type,
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
                if let Some(field) = op.strip_prefix("Field/") {
                    self.program
                        .expressions
                        .get(*input)
                        .and_then(|input_expr| self.static_expr_type(input_expr, active_functions))
                        .and_then(|ty| match ty {
                            Type::Object(shape) => shape.fields.get(field).cloned(),
                            _ => None,
                        })
                } else if op == "List/map" {
                    Some(Type::List(Box::new(
                        self.static_list_map_result_item_type(args, active_functions),
                    )))
                } else if matches!(op.as_str(), "List/any" | "List/every" | "List/is_not_empty") {
                    Some(true_false_type())
                } else if op == "List/latest" {
                    self.program
                        .expressions
                        .get(*input)
                        .and_then(|input_expr| self.static_expr_type(input_expr, active_functions))
                        .and_then(|ty| list_item_type_from_list_type(&ty))
                } else if op == "WHILE" {
                    self.static_when_result_type(expr.id, active_functions)
                } else if matches!(op.as_str(), "List/retain" | "List/remove") {
                    self.program
                        .expressions
                        .get(*input)
                        .and_then(|input_expr| self.static_expr_type(input_expr, active_functions))
                } else if op == "SOURCE" {
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
            AstExprKind::Object(fields) | AstExprKind::Record(fields) => Some(Type::Object(
                self.static_record_shape(fields, active_functions),
            )),
            AstExprKind::TaggedObject { tag, fields } => {
                Some(Type::VariantSet(vec![Variant::Tagged {
                    tag: tag.clone(),
                    fields: ObjectShape::from_ordered_fields(
                        fields.iter().filter(|field| !field.spread).map(|field| {
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
            AstExprKind::ByteLiteral { .. } => Some(Type::Byte),
            AstExprKind::BytesLiteral { size, items } => Some(static_bytes_literal_type(
                size,
                items,
                self.program.expressions.as_slice(),
                |expr| self.static_expr_type(expr, active_functions),
            )),
            AstExprKind::Bool(_) => Some(true_false_type()),
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) if tag == "SKIP" => Some(Type::Skip),
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) => {
                Some(Type::VariantSet(vec![Variant::Tag(tag.clone())]))
            }
            AstExprKind::ListLiteral { .. } => Some(Type::List(Box::new(open_object_type()))),
            AstExprKind::Identifier(value) => self.name_bindings.get(value).cloned(),
            AstExprKind::Path(parts) => self.static_type_for_path(parts),
            AstExprKind::Drain { path } => self.static_type_for_path(&drain_path_parts(path)),
            AstExprKind::Infix { op, .. }
                if matches!(op.as_str(), "==" | ">" | "<" | ">=" | "<=") =>
            {
                Some(true_false_type())
            }
            AstExprKind::Infix { .. } => Some(Type::Number),
            AstExprKind::Hold { initial, .. } => {
                let initial = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr.id,
                    *initial,
                    &self.program.expressions,
                );
                let mut ty = self
                    .program
                    .expressions
                    .get(initial)
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
            AstExprKind::Draining { input } => {
                let input = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr.id,
                    *input,
                    &self.program.expressions,
                );
                self.program
                    .expressions
                    .get(input)
                    .and_then(|expr| self.static_expr_type(expr, active_functions))
            }
            AstExprKind::MatchArm {
                output: Some(output),
                ..
            } => self
                .program
                .expressions
                .get(*output)
                .and_then(|expr| self.static_expr_type(expr, active_functions)),
            AstExprKind::MatchArm { output: None, .. } => Some(Type::Skip),
            AstExprKind::Source => Some(exact_empty_object_type()),
            AstExprKind::Latest => self
                .static_latest_result_type(expr.id, active_functions)
                .or_else(|| Some(exact_empty_object_type())),
            _ => None,
        }
    }

    fn static_when_result_type(
        &self,
        expr_id: usize,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let mut result = None;
        for arm in when_arm_statements(
            &self.program.ast.statements,
            expr_id,
            &self.program.expressions,
        ) {
            let Some(arm_type) = self.static_statement_type(arm, active_functions) else {
                continue;
            };
            result = Some(match result {
                Some(existing) => widen_structural_type(&existing, &arm_type),
                None => arm_type,
            });
        }
        result
    }

    fn static_latest_result_type(
        &self,
        expr_id: usize,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let mut result = None;
        for branch_expr_id in latest_branch_expr_ids(
            &self.program.ast.statements,
            expr_id,
            &self.program.expressions,
        ) {
            let Some(branch_type) = self
                .program
                .expressions
                .get(branch_expr_id)
                .and_then(|expr| self.static_expr_type(expr, active_functions))
            else {
                continue;
            };
            if matches!(branch_type, Type::Skip) {
                continue;
            }
            result = Some(match result {
                Some(existing) => widen_structural_type(&existing, &branch_type),
                None => branch_type,
            });
        }
        result
    }

    fn static_record_shape(
        &self,
        fields: &[AstRecordField],
        active_functions: &mut BTreeSet<String>,
    ) -> ObjectShape {
        let mut shape_fields = BTreeMap::new();
        let mut field_order = Vec::new();
        for field in fields {
            let ty = self
                .program
                .expressions
                .get(field.value)
                .and_then(|field_expr| self.static_expr_type(field_expr, active_functions))
                .unwrap_or_else(open_object_type);
            if field.spread {
                if let Type::Object(shape) = ty {
                    merge_shape_override(&mut shape_fields, &mut field_order, &shape);
                }
                continue;
            }
            insert_shape_field_override(
                &mut shape_fields,
                &mut field_order,
                field.name.clone(),
                ty,
            );
        }
        ObjectShape {
            fields: shape_fields,
            field_order,
            open: false,
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
        if let Some(cached) = self
            .function_return_type_cache
            .borrow()
            .get(function)
            .cloned()
        {
            return cached;
        }
        if !active_functions.insert(function.to_owned()) {
            return None;
        }
        let result = self
            .function_statements
            .get(function)
            .copied()
            .and_then(|statement| {
                self.function_body_return_type(function, statement, active_functions)
            });
        active_functions.remove(function);
        self.function_return_type_cache
            .borrow_mut()
            .insert(function.to_owned(), result.clone());
        result
    }

    fn function_body_return_type(
        &self,
        function: &str,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let local_bindings = self.user_function_static_bindings(function);
        if let Some(renderable) = statement.children.iter().find_map(|child| {
            self.static_statement_type_with_bindings(child, active_functions, &local_bindings)
                .filter(type_contains_renderable)
        }) {
            return Some(renderable);
        }
        let mut fields = BTreeMap::new();
        let mut field_order = Vec::new();
        self.collect_static_statement_fields_with_bindings(
            &statement.children,
            active_functions,
            &local_bindings,
            &mut fields,
            &mut field_order,
        );
        if !fields.is_empty() {
            return Some(Type::Object(ObjectShape {
                fields,
                field_order,
                open: false,
            }));
        }
        self.static_block_return_type_with_bindings(
            &statement.children,
            active_functions,
            &local_bindings,
        )
    }

    fn user_function_static_bindings(&self, function: &str) -> BTreeMap<String, Type> {
        let mut bindings = self.name_bindings.clone();
        if let Some(args) = self.function_args_by_name.get(function) {
            for arg in args {
                bindings.insert(arg.clone(), self.function_arg_display_type(function, arg));
            }
        }
        bindings
    }

    fn collect_static_statement_fields_with_bindings(
        &self,
        statements: &[AstStatement],
        active_functions: &mut BTreeSet<String>,
        bindings: &BTreeMap<String, Type>,
        fields: &mut BTreeMap<String, Type>,
        field_order: &mut Vec<String>,
    ) {
        for statement in statements {
            if semantic_block_statement(statement, &self.program.expressions) {
                if let Some(Type::Object(shape)) = self.static_block_return_type_with_bindings(
                    &statement.children,
                    active_functions,
                    bindings,
                ) {
                    merge_shape_override(fields, field_order, &shape);
                }
                continue;
            }
            if let Some(field) = statement_output_name(statement)
                && !matches!(field.as_str(), "document" | "scene")
                && let Some(ty) =
                    self.static_statement_type_with_bindings(statement, active_functions, bindings)
            {
                insert_ordered_shape_field(fields, field_order, field, ty);
            } else {
                self.collect_static_statement_fields_with_bindings(
                    &statement.children,
                    active_functions,
                    bindings,
                    fields,
                    field_order,
                );
            }
        }
    }

    fn static_statement_type_with_bindings(
        &self,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
        bindings: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        if semantic_block_statement(statement, &self.program.expressions) {
            return self.static_block_return_type_with_bindings(
                &statement.children,
                active_functions,
                bindings,
            );
        }
        if let Some(ty) =
            self.static_statement_pipeline_type_with_bindings(statement, active_functions, bindings)
        {
            return Some(ty);
        }
        if let Some(expr_id) =
            statement_pipeline_final_expr_id(statement, &self.program.expressions)
                .or_else(|| direct_statement_value_expr_id(statement, &self.program.expressions))
            && let Some(expr) = self.program.expressions.get(expr_id)
            && let Some(ty) =
                static_expr_type_from_bindings(expr, &self.program.expressions, bindings)
        {
            return Some(ty);
        }
        self.static_statement_type(statement, active_functions)
            .or_else(|| {
                let mut fields = BTreeMap::new();
                let mut field_order = Vec::new();
                self.collect_static_statement_fields_with_bindings(
                    &statement.children,
                    active_functions,
                    bindings,
                    &mut fields,
                    &mut field_order,
                );
                (!fields.is_empty()).then_some(Type::Object(ObjectShape {
                    fields,
                    field_order,
                    open: false,
                }))
            })
    }

    fn static_block_return_type_with_bindings(
        &self,
        statements: &[AstStatement],
        active_functions: &mut BTreeSet<String>,
        bindings: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let mut result = None;
        for statement in statements {
            if statement_is_source_pipe_continuation(statement, &self.program.expressions)
                && result.is_some()
            {
                continue;
            }
            if let Some(ty) =
                self.static_statement_type_with_bindings(statement, active_functions, bindings)
            {
                result = Some(ty);
            }
        }
        result
    }

    fn collect_static_statement_fields(
        &self,
        statements: &[AstStatement],
        active_functions: &mut BTreeSet<String>,
        fields: &mut BTreeMap<String, Type>,
        field_order: &mut Vec<String>,
    ) {
        for statement in statements {
            if semantic_block_statement(statement, &self.program.expressions) {
                if let Some(Type::Object(shape)) =
                    self.static_block_return_type(&statement.children, active_functions)
                {
                    merge_shape_override(fields, field_order, &shape);
                }
                continue;
            }
            if let Some(field) = statement_output_name(statement)
                && !matches!(field.as_str(), "document" | "scene")
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
        if semantic_block_statement(statement, &self.program.expressions) {
            return self.static_block_return_type(&statement.children, active_functions);
        }
        if let Some(arm_type) = self.static_match_arm_statement_type(statement, active_functions) {
            return Some(arm_type);
        }
        if let Some(ty) = self.static_statement_pipeline_type_with_bindings(
            statement,
            active_functions,
            &self.name_bindings,
        ) {
            return Some(ty);
        }
        match &statement.kind {
            AstStatementKind::Source { .. } => statement
                .expr
                .and_then(|expr_id| self.program.expressions.get(expr_id))
                .and_then(|expr| self.static_expr_type(expr, active_functions))
                .or_else(|| {
                    Some(source_statement_value_type(
                        statement,
                        &self.source_payload_shape_table,
                    ))
                }),
            AstStatementKind::List { .. } => {
                statement_pipeline_final_expr_id(statement, &self.program.expressions)
                    .or_else(|| {
                        direct_statement_value_expr_id(statement, &self.program.expressions)
                    })
                    .and_then(|expr_id| self.program.expressions.get(expr_id))
                    .and_then(|expr| self.static_expr_type(expr, active_functions))
                    .filter(is_specific_type)
                    .or_else(|| self.static_list_statement_type(statement, active_functions))
            }
            _ => statement_pipeline_final_expr_id(statement, &self.program.expressions)
                .or_else(|| direct_statement_value_expr_id(statement, &self.program.expressions))
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

    fn static_statement_pipeline_type_with_bindings(
        &self,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
        bindings: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let expr_ids = statement_expression_child_expr_ids(statement);
        if !expression_sequence_is_pipeline(&expr_ids, &self.program.expressions) {
            return None;
        }
        let (first, rest) = expr_ids.split_first()?;
        let mut ty = self.static_expr_type_for_pipeline_expr(*first, active_functions, bindings)?;
        for expr_id in rest {
            if matches!(
                self.program
                    .expressions
                    .get(*expr_id)
                    .map(|expr| &expr.kind),
                Some(AstExprKind::Draining { .. } | AstExprKind::Hold { .. })
            ) {
                continue;
            }
            let Some(AstExpr {
                kind: AstExprKind::Pipe { op, args, .. },
                ..
            }) = self.program.expressions.get(*expr_id)
            else {
                ty =
                    self.static_expr_type_for_pipeline_expr(*expr_id, active_functions, bindings)?;
                continue;
            };
            ty = match op.as_str() {
                "List/retain"
                | "List/remove"
                | "List/filter_field_equal"
                | "List/filter_field_not_equal"
                | "List/move_field_first"
                | "List/move_field_last"
                | "SOURCE" => ty,
                "List/count" | "List/sum" => Type::Number,
                "List/join_field" => Type::Text,
                "List/append" => {
                    let append_ty = args
                        .iter()
                        .find(|arg| arg.name.as_deref() == Some("item"))
                        .and_then(|arg| {
                            self.static_expr_type_for_pipeline_expr(
                                arg.value,
                                active_functions,
                                bindings,
                            )
                        });
                    match (ty, append_ty) {
                        (Type::List(item), Some(append_ty)) => {
                            Type::List(Box::new(widen_structural_type(&item, &append_ty)))
                        }
                        (existing, _) => existing,
                    }
                }
                "List/map" => self
                    .static_expr_type_for_pipeline_expr(*expr_id, active_functions, bindings)
                    .unwrap_or(ty),
                "Bool/not" | "Bool/and" | "Bool/toggle" | "Text/is_not_empty" | "List/every"
                | "List/any" | "List/is_not_empty" => true_false_type(),
                "List/latest" => {
                    list_item_type_from_list_type(&ty).unwrap_or_else(open_object_type)
                }
                _ if op.starts_with("Field/") => {
                    if let (Type::Object(shape), Some(field)) = (&ty, op.strip_prefix("Field/")) {
                        shape.fields.get(field).cloned().unwrap_or(Type::Unknown)
                    } else {
                        Type::Unknown
                    }
                }
                _ => self
                    .static_expr_type_for_pipeline_expr(*expr_id, active_functions, bindings)
                    .unwrap_or(ty),
            };
        }
        Some(ty)
    }

    fn static_expr_type_for_pipeline_expr(
        &self,
        expr_id: usize,
        active_functions: &mut BTreeSet<String>,
        bindings: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let expr = self.program.expressions.get(expr_id)?;
        static_expr_type_from_bindings(expr, &self.program.expressions, bindings)
            .or_else(|| self.static_expr_type(expr, active_functions))
    }

    fn static_block_return_type(
        &self,
        statements: &[AstStatement],
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let mut result = None;
        for statement in statements {
            if statement_is_source_pipe_continuation(statement, &self.program.expressions)
                && result.is_some()
            {
                continue;
            }
            if let Some(ty) = self.static_statement_type(statement, active_functions) {
                result = Some(ty);
            }
        }
        result
    }

    fn static_match_arm_statement_type(
        &self,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let expr = self.program.expressions.get(statement.expr?)?;
        let AstExprKind::MatchArm {
            output: Some(output),
            ..
        } = &expr.kind
        else {
            return None;
        };
        let output_expr = self.program.expressions.get(*output)?;
        if !matches!(output_expr.kind, AstExprKind::ListLiteral { .. }) {
            return None;
        }
        (!statement.children.is_empty())
            .then(|| self.static_list_statement_type(statement, active_functions))
            .flatten()
    }

    fn static_list_statement_type(
        &self,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let mut item_type = None;
        for child in &statement.children {
            let ty = self.static_statement_type(child, active_functions)?;
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
        if self.collect_type_hints
            && self
                .user_function_return_type(name, &mut BTreeSet::new())
                .as_ref()
                .is_some_and(type_contains_renderable)
        {
            return true;
        }
        statement_contains_render_context_syntax(statement, &self.program.expressions)
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
                } else if path == "element.hovered" || path.ends_with(".element.hovered") {
                    FlowMode::Continuous
                } else if path_is_source_path(&self.source_paths, &path)
                    || path_is_event_payload_parts(parts)
                {
                    FlowMode::PresentOrAbsent
                } else {
                    FlowMode::Continuous
                }
            }
            AstExprKind::Drain { path } => {
                let parts = drain_path_parts(path);
                let path = parts.join(".");
                self.flow_bindings
                    .get(&path)
                    .copied()
                    .unwrap_or(FlowMode::Continuous)
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
            AstExprKind::Draining { input } => self.flow_mode_for_expr_id(pipeline_source_expr_id(
                &self.program.ast.statements,
                expr.id,
                *input,
                &self.program.expressions,
            )),
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

    fn expr_id_is_event_payload_path(&self, expr_id: usize) -> bool {
        matches!(
            self.program.expressions.get(expr_id).map(|expr| &expr.kind),
            Some(AstExprKind::Path(parts)) if path_is_event_payload_parts(parts)
        )
    }

    fn expr_id_is_bytes_source_payload_path(&self, expr_id: usize) -> bool {
        matches!(
            self.program.expressions.get(expr_id).map(|expr| &expr.kind),
            Some(AstExprKind::Path(parts))
                if matches!(
                    self.source_payload_lookup.access_for_parts(parts),
                    Some(SourcePayloadAccess::Field(field)) if field == "bytes"
                )
        )
    }

    fn expr_id_is_pipe_placeholder(&self, expr_id: usize) -> bool {
        self.program
            .expressions
            .get(expr_id)
            .is_some_and(expr_is_pipe_placeholder)
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
            .filter_map(|child| self.static_statement_type(child, &mut BTreeSet::new()))
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

    fn check_pipeline_continuation_compatibility(&mut self, statement: &AstStatement) {
        let Some(expr_ids) = statement_pipeline_expr_ids(statement, &self.program.expressions)
        else {
            return;
        };
        for pair in expr_ids.windows(2) {
            let [previous_expr_id, expr_id] = pair else {
                continue;
            };
            let Some(expr) = self.program.expressions.get(*expr_id).cloned() else {
                continue;
            };
            let AstExprKind::Pipe { input, op, args } = &expr.kind else {
                continue;
            };
            if !self.expr_id_is_pipe_placeholder(*input) {
                continue;
            }
            let previous_flow = self.ensure_expr(*previous_expr_id);
            if op == "Bool/not" || op == "Bool/toggle" {
                self.check_true_false_input(&expr, op, &previous_flow);
            } else if op == "Bool/and" {
                self.check_true_false_input(&expr, op, &previous_flow);
                for arg in args {
                    let arg_flow = self.ensure_expr(arg.value);
                    self.check_true_false_input(&expr, op, &arg_flow);
                }
            } else if op == "WHILE" && !matches!(previous_flow.mode, FlowMode::Continuous) {
                self.constraints.push(Constraint::FlowCompatible {
                    actual: previous_flow.clone(),
                    expected: FlowType {
                        mode: FlowMode::Continuous,
                        ty: previous_flow.ty.clone(),
                    },
                });
                self.diagnostics.push(self.diagnostic_for_expr(
                    *previous_expr_id,
                    "`WHILE` requires a continuous selector".to_owned(),
                ));
            }
        }
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
        let Some(function_args) = self.function_args_by_name.get(function).cloned() else {
            return;
        };
        for (param, expected) in requirements {
            let Some(actual_expr_id) =
                function_call_argument_expr(&function_args, &param, pipe_input, call_args)
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
            if is_open_object_type(&actual)
                || matches!(
                    actual,
                    Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
                )
            {
                continue;
            }
            let continuation_final = statement_pipeline_final_expr_id_containing_expr(
                &self.program.ast.statements,
                actual_expr_id,
                &self.program.expressions,
            );
            if let Some(final_expr_id) = continuation_final
                && final_expr_id != actual_expr_id
            {
                let final_actual = self.ensure_expr(final_expr_id).ty;
                self.constraints.push(Constraint::Assignable {
                    actual: final_actual.clone(),
                    expected: expected.clone(),
                });
                if type_is_assignable_to(&final_actual, &expected) {
                    continue;
                }
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

    fn check_builtin_call_compatibility(
        &mut self,
        function: &str,
        pipe_input: Option<usize>,
        call_args: &[AstCallArg],
    ) {
        if let Some(input_expr_id) = pipe_input
            && !self.expr_id_is_pipe_placeholder(input_expr_id)
        {
            let actual = self.ensure_expr(input_expr_id).ty;
            if let Some(expected_label) = builtin_pipe_input_custom_expected_label(function) {
                if !builtin_pipe_input_custom_accepts(function, &actual) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        input_expr_id,
                        format!(
                            "`{function}` pipe input has incompatible type\nexpected: {expected_label}\nfound: {}",
                            boon_facing_type_label(&actual)
                        ),
                    ));
                }
            } else if let Some(expected) = pipe_input_expected_type(function) {
                self.constraints.push(Constraint::Assignable {
                    actual: actual.clone(),
                    expected: expected.clone(),
                });
                if !type_is_assignable_to(&actual, &expected) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        input_expr_id,
                        format!(
                            "`{function}` pipe input has incompatible type\nexpected: {}\nfound: {}",
                            boon_facing_type_label(&expected),
                            boon_facing_type_label(&actual)
                        ),
                    ));
                }
            }
        }

        let piped = pipe_input.is_some();
        for arg in call_args {
            let arg_name = arg.name.as_deref();
            if function == "Bool/toggle" && arg_name == Some("when") {
                let actual_flow = self.ensure_expr(arg.value);
                self.constraints.push(Constraint::FlowCompatible {
                    actual: actual_flow.clone(),
                    expected: FlowType {
                        mode: FlowMode::PresentOrAbsent,
                        ty: actual_flow.ty.clone(),
                    },
                });
                if !bool_toggle_when_accepts_flow(
                    &actual_flow,
                    self.expr_id_is_event_payload_path(arg.value)
                        || self.expr_id_is_pipe_placeholder(arg.value),
                ) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        arg.value,
                        format!(
                            "`{function}` argument `when` requires a tick-present-or-absent value\nexpected: SOURCE pulse\nfound: {}",
                            boon_facing_type_label(&actual_flow.ty)
                        ),
                    ));
                }
                continue;
            }

            if let Some(expected_label) =
                builtin_argument_custom_expected_label(function, arg_name, piped)
            {
                let actual = self.ensure_expr(arg.value).ty;
                if !builtin_argument_custom_accepts(function, arg_name, &actual, piped) {
                    let arg_label = arg.name.as_deref().unwrap_or("argument");
                    self.diagnostics.push(self.diagnostic_for_expr(
                        arg.value,
                        format!(
                            "`{function}` argument `{arg_label}` has incompatible type\nexpected: {expected_label}\nfound: {}",
                            boon_facing_type_label(&actual)
                        ),
                    ));
                }
                continue;
            }

            let Some(expected) =
                builtin_argument_expected_type(function, arg.name.as_deref(), piped)
            else {
                continue;
            };
            let actual = self.ensure_expr(arg.value).ty;
            self.constraints.push(Constraint::Assignable {
                actual: actual.clone(),
                expected: expected.clone(),
            });
            if type_is_assignable_to(&actual, &expected) {
                continue;
            }
            let arg_label = arg.name.as_deref().unwrap_or("argument");
            self.diagnostics.push(self.diagnostic_for_expr(
                arg.value,
                format!(
                    "`{function}` argument `{arg_label}` has incompatible type\nexpected: {}\nfound: {}",
                    boon_facing_type_label(&expected),
                    boon_facing_type_label(&actual)
                ),
            ));
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
        let initial = pipeline_source_expr_id(
            &self.program.ast.statements,
            expr_id,
            *initial,
            &self.program.expressions,
        );
        let initial_type = self.ensure_expr(initial).ty;
        if matches!(initial_type, Type::Skip) {
            self.diagnostics.push(
                self.diagnostic_for_expr(
                    initial,
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
        let mut direct_then_sources = BTreeMap::new();
        for child in &statement.children {
            let Some((trigger_expr_id, trigger)) =
                latest_direct_then_trigger_key(child, &self.program.expressions)
            else {
                continue;
            };
            if let Some(first_expr_id) =
                direct_then_sources.insert(trigger.clone(), trigger_expr_id)
            {
                let first_line = self
                    .program
                    .expressions
                    .get(first_expr_id)
                    .map(|expr| expr.line)
                    .unwrap_or_default();
                self.diagnostics.push(self.diagnostic_for_expr(
                    trigger_expr_id,
                    format!(
                        "duplicate direct `LATEST` branch for source `{trigger}`; first branch is on line {first_line}. Use one branch for a source trigger or make disjoint `WHEN` guards explicit."
                    ),
                ));
            }
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
            if !matches!(
                expr.kind,
                AstExprKind::StringLiteral(_)
                    | AstExprKind::TextLiteral(_)
                    | AstExprKind::Number(_)
                    | AstExprKind::Bool(_)
                    | AstExprKind::Enum(_)
                    | AstExprKind::Tag(_)
            ) {
                return;
            }
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
            let Some(field) = statement_field(child) else {
                continue;
            };
            if let Some(value_expr_id) =
                direct_statement_value_expr_id(child, &self.program.expressions)
            {
                self.check_style_field_value(&field, value_expr_id);
            } else {
                self.check_style_statement(child);
            }
        }
    }

    fn check_style_field(&mut self, field: &AstRecordField) {
        self.check_style_field_value(&field.name, field.value);
    }

    fn check_style_field_value(&mut self, field_name: &str, value_expr_id: usize) {
        if is_deleted_public_style_field(field_name) {
            self.diagnostics.push(self.diagnostic_for_expr(
                value_expr_id,
                format!("style field `{field_name}` is not public Boon API"),
            ));
            return;
        }
        match field_name {
            "width" | "height" | "padding" | "gap" => {
                let ty = self.ensure_expr(value_expr_id).ty;
                if !style_dimension_accepts_type(&ty) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        value_expr_id,
                        format!(
                            "style field `{field_name}` must be a number, `Fill` tag, or `Auto` tag"
                        ),
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
            "background" | "border" | "outline" | "borders" => {
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
            let ty = self.ensure_expr(expr_id).ty;
            if matches!(
                ty,
                Type::Object(_) | Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
            ) {
                return;
            }
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
                format!(
                    "style field `{field_name}` must be `Oklch[...]` or CSS hex text, found `{}`",
                    boon_facing_type_label(&ty)
                ),
            ));
        }
    }

    fn check_recursive_functions(&mut self) {
        let mut visited = BTreeSet::new();
        let mut active = Vec::new();
        let mut reported = BTreeSet::new();
        for function in self.function_call_graph.keys() {
            report_recursive_function_cycles(
                function,
                &self.function_call_graph,
                &self.function_statements,
                &mut visited,
                &mut active,
                &mut reported,
                &mut self.diagnostics,
            );
        }
    }

    fn check_host_effect_declarations(&mut self) {
        let declarations = self.host_effect_table.declarations.clone();
        let perform_expr_ids = declarations
            .iter()
            .map(|declaration| declaration.perform_expr_id)
            .collect::<BTreeSet<_>>();
        for declaration in declarations {
            for field in declaration.intent_fields {
                let actual = self.ensure_expr(field.value_expr_id).ty;
                self.constraints.push(Constraint::Assignable {
                    actual: actual.clone(),
                    expected: field.value_type.clone(),
                });
                if !type_is_assignable_to(&actual, &field.value_type) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        field.value_expr_id,
                        format!(
                            "`{}` intent field `{}` has incompatible type\nexpected: {}\nfound: {}",
                            declaration.operation,
                            field.name,
                            boon_facing_type_label(&field.value_type),
                            boon_facing_type_label(&actual)
                        ),
                    ));
                }
            }
        }
        for expr in &self.program.expressions {
            let AstExprKind::Call { function, .. } = &expr.kind else {
                continue;
            };
            if host_effect_signature(function).is_some() && !perform_expr_ids.contains(&expr.id) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    format!(
                        "typed host effect `{function}` may only appear as `effects.<name>.perform`"
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

fn typecheck_elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
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

fn statement_is_empty_delimiter(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    statement.children.is_empty()
        && statement
            .expr
            .and_then(|expr_id| expressions.get(expr_id))
            .is_some_and(|expr| matches!(expr.kind, AstExprKind::Delimiter))
}

fn statement_contains_output_authority(statement: &AstStatement) -> bool {
    matches!(
        statement.kind,
        AstStatementKind::Hold { .. } | AstStatementKind::Source { .. }
    ) || statement
        .children
        .iter()
        .any(statement_contains_output_authority)
}

fn host_output_type_is_closed(ty: &Type) -> bool {
    match ty {
        Type::Text | Type::Number | Type::Byte | Type::Bytes(_) | Type::Skip => true,
        Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            Variant::Tag(_) => true,
            Variant::Tagged { fields, .. } => {
                !fields.open && fields.fields.values().all(host_output_type_is_closed)
            }
        }),
        Type::Object(shape) => !shape.open && shape.fields.values().all(host_output_type_is_closed),
        Type::List(item) => host_output_type_is_closed(item),
        Type::RenderContract
        | Type::Function { .. }
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

fn is_deleted_public_style_field(field_name: &str) -> bool {
    field_name.starts_with("shadow1_")
        || field_name.starts_with("shadow2_")
        || field_name.starts_with("shadow3_")
        || field_name.starts_with("shadow4_")
        || field_name.starts_with("shadow5_")
        || matches!(
            field_name,
            "border_top"
                | "selected_border"
                | "strike_if"
                | "color_if"
                | "focus_border"
                | "focus_border_width"
                | "hover_visible"
                | "hover_color"
                | "hover_border"
                | "hover_underline_if"
                | "hover_scope"
        )
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
    expr_contains_list_map_seen(expr_id, expressions, &mut BTreeSet::new())
}

fn expr_contains_list_map_seen(
    expr_id: usize,
    expressions: &[AstExpr],
    seen: &mut BTreeSet<usize>,
) -> bool {
    if !seen.insert(expr_id) {
        return false;
    }
    let Some(expr) = expressions.get(expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Pipe { input, op, args } => {
            op == "List/map"
                || expr_contains_list_map_seen(*input, expressions, seen)
                || args
                    .iter()
                    .any(|arg| expr_contains_list_map_seen(arg.value, expressions, seen))
        }
        AstExprKind::Call { args, .. } => args
            .iter()
            .any(|arg| expr_contains_list_map_seen(arg.value, expressions, seen)),
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial }
        | AstExprKind::Draining { input: initial } => {
            expr_contains_list_map_seen(*initial, expressions, seen)
        }
        AstExprKind::Then { input, output } => {
            expr_contains_list_map_seen(*input, expressions, seen)
                || output
                    .is_some_and(|output| expr_contains_list_map_seen(output, expressions, seen))
        }
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_contains_list_map_seen(*output, expressions, seen),
        AstExprKind::Infix { left, right, .. } => {
            expr_contains_list_map_seen(*left, expressions, seen)
                || expr_contains_list_map_seen(*right, expressions, seen)
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields
            .iter()
            .any(|field| expr_contains_list_map_seen(field.value, expressions, seen)),
        _ => false,
    }
}

fn function_statement_map(statements: &[AstStatement]) -> BTreeMap<String, &AstStatement> {
    let mut functions = BTreeMap::new();
    collect_function_statements(statements, &mut functions);
    functions
}

fn function_args_by_statement_map(
    function_statements: &BTreeMap<String, &AstStatement>,
) -> BTreeMap<String, Vec<String>> {
    function_statements
        .iter()
        .filter_map(|(name, statement)| {
            let AstStatementKind::Function { args, .. } = &statement.kind else {
                return None;
            };
            Some((name.clone(), args.clone()))
        })
        .collect()
}

fn function_arg_call_site_index(
    program: &ParsedProgram,
    function_args_by_name: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, BTreeMap<String, Vec<usize>>> {
    let mut index: BTreeMap<String, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
    for expr in &program.expressions {
        let (function, pipe_input, call_args) = match &expr.kind {
            AstExprKind::Call { function, args } => (function, None, args.as_slice()),
            AstExprKind::Pipe { input, op, args } => (op, Some(*input), args.as_slice()),
            _ => continue,
        };
        let Some(function_args) = function_args_by_name.get(function) else {
            continue;
        };
        for parameter in function_args {
            let Some(arg_expr_id) =
                function_call_argument_expr(function_args, parameter, pipe_input, call_args)
            else {
                continue;
            };
            index
                .entry(function.clone())
                .or_default()
                .entry(parameter.clone())
                .or_default()
                .push(arg_expr_id);
        }
    }
    index
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
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial }
        | AstExprKind::Draining { input: initial } => {
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
        AstExprKind::BytesLiteral { items, .. } => {
            for item in items {
                collect_expr_user_function_calls(*item, expressions, user_functions, calls);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
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
    if let Some(expr_id) = statement_pipeline_final_expr_id(statement, expressions) {
        return Some(expr_id);
    }
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
        && expr_ids
            .iter()
            .skip(1)
            .all(|expr_id| expr_is_pipeline_continuation(*expr_id, expressions))
}

fn statement_is_source_pipe_continuation(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> bool {
    let Some(expr) = statement.expr.and_then(|expr_id| expressions.get(expr_id)) else {
        return false;
    };
    let AstExprKind::Pipe { input, op, .. } = &expr.kind else {
        return false;
    };
    op == "SOURCE"
        && expressions
            .get(*input)
            .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
}

fn expr_is_pipeline_continuation(expr_id: usize, expressions: &[AstExpr]) -> bool {
    let input = match expressions.get(expr_id).map(|expr| &expr.kind) {
        Some(AstExprKind::Pipe { input, .. })
        | Some(AstExprKind::Then { input, .. })
        | Some(AstExprKind::When { input })
        | Some(AstExprKind::Draining { input })
        | Some(AstExprKind::Hold { initial: input, .. }) => *input,
        _ => return false,
    };
    expr_chain_starts_with_pipe_placeholder(input, expressions)
}

fn expr_chain_starts_with_pipe_placeholder(expr_id: usize, expressions: &[AstExpr]) -> bool {
    let Some(expr) = expressions.get(expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Delimiter => true,
        AstExprKind::Unknown(tokens) => !unknown_tokens_are_quoted_text(tokens),
        AstExprKind::Pipe { input, .. }
        | AstExprKind::Then { input, .. }
        | AstExprKind::When { input }
        | AstExprKind::Draining { input }
        | AstExprKind::Hold { initial: input, .. } => {
            expr_chain_starts_with_pipe_placeholder(*input, expressions)
        }
        _ => false,
    }
}

fn pipeline_source_expr_id(
    statements: &[AstStatement],
    marker_expr_id: usize,
    input_expr_id: usize,
    expressions: &[AstExpr],
) -> usize {
    if !expressions
        .get(input_expr_id)
        .is_some_and(expr_is_pipe_placeholder)
    {
        return input_expr_id;
    }
    previous_pipeline_expr_id(statements, marker_expr_id, expressions).unwrap_or(input_expr_id)
}

fn previous_pipeline_expr_id(
    statements: &[AstStatement],
    marker_expr_id: usize,
    expressions: &[AstExpr],
) -> Option<usize> {
    let mut previous = None;
    for statement in statements {
        if let Some(expr_ids) = statement_pipeline_expr_ids(statement, expressions)
            && let Some(position) = expr_ids
                .iter()
                .position(|expr_id| *expr_id == marker_expr_id)
            && position > 0
        {
            return expr_ids.get(position - 1).copied();
        }
        if statement.expr == Some(marker_expr_id) {
            return previous;
        }
        if let Some(found) =
            previous_pipeline_expr_id(&statement.children, marker_expr_id, expressions)
        {
            return Some(found);
        }
        previous = statement_pipeline_final_expr_id(statement, expressions).or(statement.expr);
    }
    None
}

fn expr_is_pipe_placeholder(expr: &AstExpr) -> bool {
    match &expr.kind {
        AstExprKind::Delimiter => true,
        AstExprKind::Unknown(tokens) => !unknown_tokens_are_quoted_text(tokens),
        _ => false,
    }
}

fn unknown_tokens_are_quoted_text(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|token| token.trim_start().starts_with('"'))
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

fn named_arg_expr(args: &[AstCallArg], name: &str) -> Option<usize> {
    args.iter()
        .find(|arg| arg.name.as_deref() == Some(name))
        .map(|arg| arg.value)
}

fn has_any_named_arg(args: &[AstCallArg], names: &[&str]) -> bool {
    args.iter().any(|arg| {
        arg.name
            .as_deref()
            .is_some_and(|name| names.contains(&name))
    })
}

fn has_unnamed_arg(args: &[AstCallArg]) -> bool {
    args.iter().any(|arg| arg.name.is_none())
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
    let expr = expressions.get(input)?;
    if expr_is_pipe_placeholder(expr) {
        fallback_input
    } else {
        Some(input)
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
        AstExprKind::Pipe { input, op, args } => {
            let mut template_args = Vec::with_capacity(args.len() + 1);
            template_args.push(AstCallArg {
                name: None,
                value: *input,
                start: expr.start,
                end: expr.end,
            });
            template_args.extend(args.iter().cloned());
            Some((op.clone(), template_args))
        }
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

fn resolved_constant_table(program: &ParsedProgram) -> ResolvedConstantTable {
    let entries = program
        .expressions
        .iter()
        .filter_map(|expr| {
            let value = resolved_constant_value_for_expr(program, expr.id)?;
            Some(ResolvedConstantEntry {
                expr_id: expr.id,
                value,
            })
        })
        .collect();
    ResolvedConstantTable { entries }
}

fn resolved_constant_value_for_expr(
    program: &ParsedProgram,
    expr_id: usize,
) -> Option<ResolvedConstantValue> {
    let expr = program.expressions.get(expr_id)?;
    match &expr.kind {
        AstExprKind::Number(_) | AstExprKind::Infix { .. } => {
            let value = static_integer_expr(program, expr_id)?;
            if value >= 0 {
                Some(ResolvedConstantValue::UnsignedInteger {
                    value: u64::try_from(value).ok()?,
                })
            } else {
                Some(ResolvedConstantValue::SignedInteger {
                    value: i64::try_from(value).ok()?,
                })
            }
        }
        AstExprKind::ByteLiteral { value, .. } => {
            Some(ResolvedConstantValue::Byte { value: *value })
        }
        AstExprKind::Enum(value) | AstExprKind::Tag(value)
            if matches!(value.as_str(), "Little" | "Big" | "Utf8" | "Ascii") =>
        {
            Some(ResolvedConstantValue::Symbol {
                value: value.clone(),
            })
        }
        _ => None,
    }
}

fn static_integer_expr(program: &ParsedProgram, expr_id: usize) -> Option<i128> {
    static_integer_expr_checked(program, expr_id).ok().flatten()
}

fn bytes_static_integer_arg_is_out_of_plan_range(function: &str, name: &str, value: i128) -> bool {
    let allows_negative = function == "Bytes/write_signed" && name == "value";
    let min = if allows_negative { i64::MIN as i128 } else { 0 };
    value < min || value > i64::MAX as i128
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StaticIntegerExprError {
    Overflow,
}

fn static_integer_expr_checked(
    program: &ParsedProgram,
    expr_id: usize,
) -> Result<Option<i128>, StaticIntegerExprError> {
    let Some(expr) = program.expressions.get(expr_id) else {
        return Ok(None);
    };
    match &expr.kind {
        AstExprKind::Number(value) => value
            .parse::<i128>()
            .map(Some)
            .map_err(|_| StaticIntegerExprError::Overflow),
        AstExprKind::Infix { left, op, right } => {
            let Some(left) = static_integer_expr_checked(program, *left)? else {
                return Ok(None);
            };
            let Some(right) = static_integer_expr_checked(program, *right)? else {
                return Ok(None);
            };
            match op.as_str() {
                "+" => left.checked_add(right),
                "-" => left.checked_sub(right),
                "*" => left.checked_mul(right),
                _ => return Ok(None),
            }
            .map(Some)
            .ok_or(StaticIntegerExprError::Overflow)
        }
        _ => Ok(None),
    }
}

fn unsupported_literal_static_integer_expr(program: &ParsedProgram, expr_id: usize) -> bool {
    let Some(expr) = program.expressions.get(expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Infix { left, op, right } => {
            if !matches!(op.as_str(), "+" | "-" | "*") {
                return literal_integer_expr_tree(program, *left)
                    && literal_integer_expr_tree(program, *right);
            }
            unsupported_literal_static_integer_expr(program, *left)
                || unsupported_literal_static_integer_expr(program, *right)
        }
        _ => false,
    }
}

fn literal_integer_expr_tree(program: &ParsedProgram, expr_id: usize) -> bool {
    let Some(expr) = program.expressions.get(expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Number(_) => true,
        AstExprKind::Infix { left, right, .. } => {
            literal_integer_expr_tree(program, *left) && literal_integer_expr_tree(program, *right)
        }
        _ => false,
    }
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

fn scene_root(program: &ParsedProgram) -> Option<&AstStatement> {
    program.ast.statements.iter().find(|statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "scene"
        )
    })
}

#[derive(Clone, Debug)]
pub struct BuiltinSignatureRegistry {
    text_functions: BTreeSet<&'static str>,
    number_functions: BTreeSet<&'static str>,
    byte_functions: BTreeSet<&'static str>,
    bytes_functions: BTreeSet<&'static str>,
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
                "Text/space",
                "Text/trim",
                "Text/to_uppercase",
                "Text/concat",
                "Text/time_range_label",
                "Text/substring",
                "Number/to_text",
                "Number/to_codepoint_text",
                "Number/to_ascii_text",
                "List/join_field",
                "Error/text",
                "Router/route",
                "Router/go_to",
                "Ulid/generate",
                "Bytes/to_text",
                "Bytes/to_hex",
                "Bytes/to_base64",
                "File/read_text",
                "File/write_bytes",
                "Log/error",
                "Log/info",
            ]
            .into_iter()
            .collect(),
            number_functions: [
                "Number/add",
                "Number/subtract",
                "Number/min",
                "Number/max",
                "Number/bit_width",
                "Number/interpolate",
                "Number/project_width",
                "Number/project_offset",
                "Number/project_time",
                "List/count",
                "List/length",
                "List/sum",
                "Text/find",
                "Text/length",
                "Text/to_number",
                "Bytes/length",
                "Bytes/find",
                "Bytes/read_unsigned",
                "Bytes/read_signed",
            ]
            .into_iter()
            .collect(),
            byte_functions: ["Bytes/get"].into_iter().collect(),
            bytes_functions: [
                "Bytes/set",
                "Bytes/slice",
                "Bytes/take",
                "Bytes/drop",
                "Bytes/concat",
                "Bytes/zeros",
                "Text/to_bytes",
                "Bytes/from_hex",
                "Bytes/from_base64",
                "Bytes/write_unsigned",
                "Bytes/write_signed",
                "File/read_bytes",
            ]
            .into_iter()
            .collect(),
            true_false_functions: [
                "Bool/not",
                "Bool/and",
                "Bool/toggle",
                "Text/is_empty",
                "Text/is_not_empty",
                "Text/starts_with",
                "Text/contains",
                "Text/all_chars_in",
                "List/every",
                "List/any",
                "List/is_not_empty",
                "Bytes/is_empty",
                "Bytes/equal",
                "Bytes/starts_with",
                "Bytes/ends_with",
            ]
            .into_iter()
            .collect(),
            list_functions: [
                "List/map",
                "List/retain",
                "List/append",
                "List/remove",
                "List/range",
                "List/chunk",
                "List/filter_text_contains",
                "List/filter_field_equal",
                "List/filter_field_not_equal",
                "List/move_field_first",
                "List/move_field_last",
            ]
            .into_iter()
            .collect(),
            list_item_functions: ["List/find", "List/find_value", "List/get", "List/latest"]
                .into_iter()
                .collect(),
            open_object_functions: [
                "WHILE",
                "Timer/interval",
                "Widget/table",
                "Widget/selected",
                "Widget/rows",
                "Light/directional",
                "Light/ambient",
                "Light/spot",
            ]
            .into_iter()
            .collect(),
        }
    }
}

impl BuiltinSignatureRegistry {
    fn type_for_call(&self, function: &str, render_contracts: &RenderContractRegistry) -> Type {
        if let Some(signature) = host_effect_signature(function) {
            signature.result_type
        } else if self.text_functions.contains(function) {
            Type::Text
        } else if self.number_functions.contains(function) {
            Type::Number
        } else if self.byte_functions.contains(function) {
            Type::Byte
        } else if self.bytes_functions.contains(function) {
            Type::Bytes(BytesType::Dynamic)
        } else if self.true_false_functions.contains(function) {
            true_false_type()
        } else if function == "List/range" {
            Type::List(Box::new(Type::Number))
        } else if self.list_functions.contains(function) {
            Type::List(Box::new(open_object_type()))
        } else if self.list_item_functions.contains(function)
            || self.open_object_functions.contains(function)
        {
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
            roots: [
                ("document", RuntimeRootContract::document()),
                ("scene", RuntimeRootContract::scene()),
            ]
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
            "EmbeddedProgram",
            "EmbeddedMedia",
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
        .with_fixed_constructor("Element/program", "EmbeddedProgram")
        .with_fixed_constructor("Element/embedded_media", "EmbeddedMedia")
    }

    fn scene() -> Self {
        Self::new([
            "Block",
            "Button",
            "Checkbox",
            "Label",
            "Link",
            "Paragraph",
            "Row",
            "Scene",
            "Stack",
            "Text",
            "TextInput",
            "EmbeddedProgram",
            "EmbeddedMedia",
        ])
        .with_fixed_constructor("Scene/new", "Scene")
        .with_stripe_direction_constructor("Scene/Element/stripe")
        .with_fixed_constructor("Scene/Element/block", "Block")
        .with_fixed_constructor("Scene/Element/text", "Text")
        .with_fixed_constructor("Scene/Element/text_input", "TextInput")
        .with_fixed_constructor("Scene/Element/program", "EmbeddedProgram")
        .with_fixed_constructor("Scene/Element/checkbox", "Checkbox")
        .with_fixed_constructor("Scene/Element/label", "Label")
        .with_fixed_constructor("Scene/Element/button", "Button")
        .with_fixed_constructor("Scene/Element/paragraph", "Paragraph")
        .with_fixed_constructor("Scene/Element/link", "Link")
        .with_fixed_constructor("Scene/Element/embedded_media", "EmbeddedMedia")
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
            .or_else(|| {
                self.roots
                    .values()
                    .find_map(|root| root.constructors.get(function))
            })
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

    fn is_any_renderable_object_type(&self, ty: &Type) -> bool {
        if self.is_renderable_object_type(ty) {
            return true;
        }
        let Type::Object(shape) = ty else {
            return false;
        };
        self.roots.values().any(|root| {
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
        })
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
    "Element/program",
    "Element/embedded_media",
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

pub fn boon_facing_type_display_tree(ty: &Type) -> TypeDisplayNode {
    boon_facing_type_display_tree_with_depth(ty, 0, 12)
}

fn scalar_type_display_node(label: impl Into<String>) -> TypeDisplayNode {
    TypeDisplayNode::Scalar {
        label: label.into(),
    }
}

fn object_shape_display_fields(
    shape: &ObjectShape,
    depth: usize,
    max_depth: usize,
) -> Vec<TypeDisplayField> {
    shape
        .ordered_fields()
        .into_iter()
        .map(|(name, ty)| TypeDisplayField {
            name: name.clone(),
            ty: boon_facing_type_display_tree_with_depth(ty, depth + 1, max_depth),
        })
        .collect()
}

fn boon_facing_type_display_tree_with_depth(
    ty: &Type,
    depth: usize,
    max_depth: usize,
) -> TypeDisplayNode {
    if depth >= max_depth {
        return scalar_type_display_node("VALUE");
    }
    match ty {
        Type::Text => scalar_type_display_node("TEXT"),
        Type::Number => scalar_type_display_node("NUMBER"),
        Type::Byte => scalar_type_display_node("BYTE"),
        Type::Bytes(bytes) => scalar_type_display_node(bytes_type_label(bytes)),
        Type::Skip => scalar_type_display_node("ABSENT"),
        Type::RenderContract => TypeDisplayNode::Object {
            fields: vec![TypeDisplayField {
                name: "kind".to_owned(),
                ty: scalar_type_display_node(
                    "Button | Checkbox | Document | Row | Stack | Text | TextInput",
                ),
            }],
            open: false,
        },
        Type::Unknown | Type::Var(_) => scalar_type_display_node("VALUE"),
        Type::UnresolvedShape { reason } => {
            if reason.is_empty() {
                scalar_type_display_node("VALUE")
            } else {
                scalar_type_display_node(format!("VALUE ({reason})"))
            }
        }
        Type::List(item) => TypeDisplayNode::List {
            item: Box::new(boon_facing_type_display_tree_with_depth(
                item,
                depth + 1,
                max_depth,
            )),
        },
        Type::Function { args, result } => TypeDisplayNode::Function {
            name: None,
            args: args
                .iter()
                .map(|arg| TypeDisplayFunctionArg {
                    name: None,
                    ty: boon_facing_type_display_tree_with_depth(arg, depth + 1, max_depth),
                })
                .collect(),
            result: Box::new(boon_facing_type_display_tree_with_depth(
                &result.ty,
                depth + 1,
                max_depth,
            )),
        },
        Type::Object(shape) => {
            if shape.fields.is_empty() && shape.open {
                scalar_type_display_node("VALUE")
            } else {
                TypeDisplayNode::Object {
                    fields: object_shape_display_fields(shape, depth, max_depth),
                    open: shape.open,
                }
            }
        }
        Type::VariantSet(variants) => {
            let variants = sorted_variants(variants);
            if variants.is_empty() {
                return scalar_type_display_node("VALUE");
            }
            if variants_are_bool_alias(&variants) {
                return scalar_type_display_node("BOOL");
            }
            TypeDisplayNode::Union {
                variants: variants
                    .iter()
                    .map(|variant| match variant {
                        Variant::Tag(tag) => scalar_type_display_node(tag.clone()),
                        Variant::Tagged { tag, fields } => TypeDisplayNode::TaggedObject {
                            tag: tag.clone(),
                            fields: object_shape_display_fields(fields, depth, max_depth),
                            open: fields.open,
                        },
                    })
                    .collect(),
            }
        }
    }
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
        Type::Byte => "BYTE".to_owned(),
        Type::Bytes(bytes) => bytes_type_label(bytes),
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

fn bytes_type_label(bytes: &BytesType) -> String {
    match bytes {
        BytesType::Dynamic => "BYTES".to_owned(),
        BytesType::Fixed(len) => format!("BYTES[{len}]"),
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
    matches!(
        ty,
        Type::Number | Type::Object(_) | Type::Unknown | Type::UnresolvedShape { .. }
    ) || matches!(
        ty,
        Type::VariantSet(variants)
            if variants.iter().all(|variant| {
                matches!(variant, Variant::Tag(tag) if tag == "Fill" || tag == "Auto" || tag == "Screen")
            })
    )
}

fn style_color_accepts_type(ty: &Type) -> bool {
    if matches!(ty, Type::Text) {
        return true;
    }
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
        | (Type::Byte, Type::Byte)
        | (Type::RenderContract, Type::RenderContract) => false,
        (Type::Bytes(left), Type::Bytes(right)) => bytes_type_conflict(left, right),
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

fn bytes_type_conflict(left: &BytesType, right: &BytesType) -> bool {
    match (left, right) {
        (BytesType::Fixed(left), BytesType::Fixed(right)) => left != right,
        (BytesType::Dynamic, _) | (_, BytesType::Dynamic) => false,
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
        (Type::Text, Type::Text) | (Type::Number, Type::Number) | (Type::Byte, Type::Byte) => true,
        (Type::Bytes(actual), Type::Bytes(expected)) => bytes_type_assignable(actual, expected),
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

fn bytes_type_assignable(actual: &BytesType, expected: &BytesType) -> bool {
    match (actual, expected) {
        (_, BytesType::Dynamic) => true,
        (BytesType::Fixed(actual), BytesType::Fixed(expected)) => actual == expected,
        (BytesType::Dynamic, BytesType::Fixed(_)) => false,
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

fn when_arm_expr_ids(
    statements: &[AstStatement],
    expr_id: usize,
    expressions: &[AstExpr],
) -> Vec<usize> {
    for statement in statements {
        if statement.expr == Some(expr_id)
            || statement.expr.is_some_and(|statement_expr_id| {
                expr_contains_expr_id(statement_expr_id, expr_id, expressions)
            })
        {
            return statement
                .children
                .iter()
                .filter_map(|child| child.expr)
                .collect();
        }
        let nested = when_arm_expr_ids(&statement.children, expr_id, expressions);
        if !nested.is_empty() {
            return nested;
        }
    }
    Vec::new()
}

fn latest_branch_expr_ids(
    statements: &[AstStatement],
    expr_id: usize,
    expressions: &[AstExpr],
) -> Vec<usize> {
    for statement in statements {
        if statement.expr == Some(expr_id) {
            return statement
                .children
                .iter()
                .flat_map(|child| statement_update_value_exprs(child, expressions))
                .collect();
        }
        let nested = latest_branch_expr_ids(&statement.children, expr_id, expressions);
        if !nested.is_empty() {
            return nested;
        }
        if statement.expr.is_some_and(|statement_expr_id| {
            expr_contains_expr_id(statement_expr_id, expr_id, expressions)
        }) {
            return statement
                .children
                .iter()
                .flat_map(|child| statement_update_value_exprs(child, expressions))
                .collect();
        }
    }
    Vec::new()
}

fn when_arm_statements<'a>(
    statements: &'a [AstStatement],
    expr_id: usize,
    expressions: &[AstExpr],
) -> Vec<&'a AstStatement> {
    if let Some(statement) = exact_expression_statement(statements, expr_id) {
        return statement.children.iter().collect();
    }
    for statement in statements {
        let nested = containing_expression_statement(&statement.children, expr_id, expressions);
        if let Some(nested) = nested {
            return nested.children.iter().collect();
        }
        if statement.expr.is_some_and(|statement_expr_id| {
            expr_contains_expr_id(statement_expr_id, expr_id, expressions)
        }) {
            return statement.children.iter().collect();
        }
    }
    Vec::new()
}

fn exact_expression_statement(
    statements: &[AstStatement],
    expr_id: usize,
) -> Option<&AstStatement> {
    for statement in statements {
        if statement.expr == Some(expr_id) {
            return Some(statement);
        }
        if let Some(found) = exact_expression_statement(&statement.children, expr_id) {
            return Some(found);
        }
    }
    None
}

fn containing_expression_statement<'a>(
    statements: &'a [AstStatement],
    expr_id: usize,
    expressions: &[AstExpr],
) -> Option<&'a AstStatement> {
    for statement in statements {
        if let Some(found) =
            containing_expression_statement(&statement.children, expr_id, expressions)
        {
            return Some(found);
        }
        if statement.expr.is_some_and(|statement_expr_id| {
            expr_contains_expr_id(statement_expr_id, expr_id, expressions)
        }) {
            return Some(statement);
        }
    }
    None
}

fn statement_pipeline_final_expr_id_containing_expr(
    statements: &[AstStatement],
    expr_id: usize,
    expressions: &[AstExpr],
) -> Option<usize> {
    for (index, statement) in statements.iter().enumerate() {
        if statement.expr == Some(expr_id)
            || statement.expr.is_some_and(|statement_expr_id| {
                expr_contains_expr_id(statement_expr_id, expr_id, expressions)
            })
        {
            return statement_pipeline_final_expr_id(statement, expressions)
                .or_else(|| {
                    let mut expr_ids = Vec::new();
                    if let Some(statement_expr_id) = statement.expr {
                        expr_ids.push(statement_expr_id);
                    }
                    collect_pipe_continuation_expr_ids(statement, expressions, &mut expr_ids);
                    collect_following_sibling_pipe_continuation_expr_ids(
                        &statements[index + 1..],
                        expressions,
                        &mut expr_ids,
                    );
                    expression_sequence_is_pipeline(&expr_ids, expressions)
                        .then(|| *expr_ids.last().unwrap())
                })
                .or_else(|| {
                    let mut continuations = Vec::new();
                    collect_pipe_continuation_expr_ids(statement, expressions, &mut continuations);
                    collect_following_sibling_pipe_continuation_expr_ids(
                        &statements[index + 1..],
                        expressions,
                        &mut continuations,
                    );
                    continuations.last().copied()
                });
        }
        if let Some(found) = statement_pipeline_final_expr_id_containing_expr(
            &statement.children,
            expr_id,
            expressions,
        ) {
            return Some(found);
        }
    }
    None
}

fn collect_following_sibling_pipe_continuation_expr_ids(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    expr_ids: &mut Vec<usize>,
) {
    for statement in statements {
        if !matches!(statement.kind, AstStatementKind::Expression)
            || !statement
                .expr
                .is_some_and(|expr_id| expr_is_pipeline_continuation(expr_id, expressions))
        {
            break;
        }
        if let Some(expr_id) = statement.expr {
            expr_ids.push(expr_id);
        }
        collect_pipe_continuation_expr_ids(statement, expressions, expr_ids);
    }
}

fn expr_contains_expr_id(root: usize, needle: usize, expressions: &[AstExpr]) -> bool {
    expr_contains_expr_id_seen(root, needle, expressions, &mut BTreeSet::new())
}

fn expr_contains_expr_id_seen(
    root: usize,
    needle: usize,
    expressions: &[AstExpr],
    seen: &mut BTreeSet<usize>,
) -> bool {
    if root == needle {
        return true;
    }
    if !seen.insert(root) {
        return false;
    }
    let Some(expr) = expressions.get(root) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => args
            .iter()
            .any(|arg| expr_contains_expr_id_seen(arg.value, needle, expressions, seen)),
        AstExprKind::Pipe { input, args, .. } => {
            expr_contains_expr_id_seen(*input, needle, expressions, seen)
                || args
                    .iter()
                    .any(|arg| expr_contains_expr_id_seen(arg.value, needle, expressions, seen))
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial }
        | AstExprKind::Draining { input: initial } => {
            expr_contains_expr_id_seen(*initial, needle, expressions, seen)
        }
        AstExprKind::Then {
            input,
            output: Some(output),
            ..
        } => {
            expr_contains_expr_id_seen(*input, needle, expressions, seen)
                || expr_contains_expr_id_seen(*output, needle, expressions, seen)
        }
        AstExprKind::Then {
            input,
            output: None,
            ..
        } => expr_contains_expr_id_seen(*input, needle, expressions, seen),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_contains_expr_id_seen(*output, needle, expressions, seen),
        AstExprKind::Infix { left, right, .. } => {
            expr_contains_expr_id_seen(*left, needle, expressions, seen)
                || expr_contains_expr_id_seen(*right, needle, expressions, seen)
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields
            .iter()
            .any(|field| expr_contains_expr_id_seen(field.value, needle, expressions, seen)),
        _ => false,
    }
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
    if let Some(expr_id) = statement_pipeline_final_expr_id(statement, expressions) {
        if let Some(AstExprKind::Then {
            output: Some(output),
            ..
        }) = expressions.get(expr_id).map(|expr| &expr.kind)
        {
            return vec![*output];
        }
        return vec![expr_id];
    }
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

fn latest_direct_then_trigger_key(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<(usize, String)> {
    let expr_ids = statement_pipeline_expr_ids(statement, expressions)
        .or_else(|| statement.expr.map(|expr_id| vec![expr_id]))?;
    if expr_ids.iter().any(|expr_id| {
        matches!(
            expressions.get(*expr_id).map(|expr| &expr.kind),
            Some(AstExprKind::When { .. })
        )
    }) {
        return None;
    }
    let expr_id = *expr_ids.last()?;
    let AstExprKind::Then { input, .. } = expressions.get(expr_id).map(|expr| &expr.kind)? else {
        return None;
    };
    let key = latest_branch_trigger_expr_key(*input, expressions)?;
    Some((*input, key))
}

fn latest_branch_trigger_expr_key(expr_id: usize, expressions: &[AstExpr]) -> Option<String> {
    match expressions.get(expr_id).map(|expr| &expr.kind)? {
        AstExprKind::Identifier(value) => Some(value.clone()),
        AstExprKind::Path(parts) => Some(parts.join(".")),
        AstExprKind::Pipe { input, .. } => latest_branch_trigger_expr_key(*input, expressions),
        _ => None,
    }
}

fn statement_pipeline_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<Vec<usize>> {
    let mut expr_ids = Vec::new();
    if let Some(expr_id) = statement.expr {
        expr_ids.push(expr_id);
    }
    collect_pipe_continuation_expr_ids(statement, expressions, &mut expr_ids);
    expression_sequence_is_pipeline(&expr_ids, expressions).then_some(expr_ids)
}

fn statement_pipeline_final_expr_id(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    statement_pipeline_expr_ids(statement, expressions).map(|expr_ids| *expr_ids.last().unwrap())
}

fn collect_pipe_continuation_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
    expr_ids: &mut Vec<usize>,
) {
    for child in statement.children.iter().filter(|child| {
        matches!(child.kind, AstStatementKind::Expression)
            && child
                .expr
                .is_some_and(|expr_id| expr_is_pipeline_continuation(expr_id, expressions))
    }) {
        if let Some(expr_id) = child.expr {
            expr_ids.push(expr_id);
        }
        collect_pipe_continuation_expr_ids(child, expressions, expr_ids);
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
    if let Some(ty) = simple_statement_pipeline_type(statement, expressions) {
        return Some(ty);
    }
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

fn simple_statement_pipeline_type(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<Type> {
    let mut expr_ids = statement.expr.into_iter().collect::<Vec<_>>();
    expr_ids.extend(statement_expression_child_expr_ids(statement));
    if !expression_sequence_is_pipeline(&expr_ids, expressions) {
        return None;
    }
    let (first, rest) = expr_ids.split_first()?;
    let mut ty = simple_expr_type(expressions.get(*first)?, expressions);
    for expr_id in rest {
        let expr = expressions.get(*expr_id)?;
        if matches!(
            expr.kind,
            AstExprKind::Draining { .. } | AstExprKind::Hold { .. }
        ) {
            continue;
        }
        let next = simple_expr_type(expr, expressions);
        if is_specific_type(&next) {
            ty = next;
        }
    }
    Some(ty)
}

fn statement_value_type_from_bindings(
    statement: &AstStatement,
    expressions: &[AstExpr],
    bindings: &BTreeMap<String, Type>,
) -> Option<Type> {
    let mut expr_ids = statement.expr.into_iter().collect::<Vec<_>>();
    expr_ids.extend(statement_expression_child_expr_ids(statement));
    if expression_sequence_is_pipeline(&expr_ids, expressions) {
        let (first, rest) = expr_ids.split_first()?;
        let first = expressions.get(*first)?;
        let mut ty = static_expr_type_from_bindings(first, expressions, bindings)
            .unwrap_or_else(|| simple_expr_type(first, expressions));
        for expr_id in rest {
            let expr = expressions.get(*expr_id)?;
            if matches!(
                expr.kind,
                AstExprKind::Draining { .. } | AstExprKind::Hold { .. }
            ) {
                continue;
            }
            if let Some(next) =
                static_expr_type_from_bindings(expr, expressions, bindings).or_else(|| {
                    let ty = simple_expr_type(expr, expressions);
                    is_specific_type(&ty).then_some(ty)
                })
            {
                ty = next;
            }
        }
        return Some(ty);
    }
    let expr_id = direct_statement_value_expr_id(statement, expressions)?;
    static_expr_type_from_bindings(expressions.get(expr_id)?, expressions, bindings)
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
    Some(simple_record_shape(fields, expressions))
}

fn static_bytes_literal_type<F>(
    size: &BytesSizeSyntax,
    items: &[usize],
    expressions: &[AstExpr],
    mut type_for_expr: F,
) -> Type
where
    F: FnMut(&AstExpr) -> Option<Type>,
{
    let mut known_len = 0usize;
    let mut all_fixed = true;
    for item in items {
        match expressions.get(*item).and_then(&mut type_for_expr) {
            Some(Type::Byte) => known_len += 1,
            Some(Type::Bytes(BytesType::Fixed(len))) => known_len += len,
            Some(Type::Bytes(BytesType::Dynamic)) | None => all_fixed = false,
            Some(_) => all_fixed = false,
        }
    }
    match size {
        BytesSizeSyntax::Dynamic => Type::Bytes(BytesType::Dynamic),
        BytesSizeSyntax::Infer if all_fixed => Type::Bytes(BytesType::Fixed(known_len)),
        BytesSizeSyntax::Infer => Type::Bytes(BytesType::Dynamic),
        BytesSizeSyntax::Fixed(expected) => Type::Bytes(BytesType::Fixed(*expected)),
    }
}

fn simple_expr_type(expr: &AstExpr, expressions: &[AstExpr]) -> Type {
    match &expr.kind {
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Type::Text,
        AstExprKind::Number(_) => Type::Number,
        AstExprKind::ByteLiteral { .. } => Type::Byte,
        AstExprKind::BytesLiteral { size, items } => {
            static_bytes_literal_type(size, items, expressions, |expr| {
                Some(simple_expr_type(expr, expressions))
            })
        }
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
            Type::Object(simple_record_shape(fields, expressions))
        }
        AstExprKind::ListLiteral { .. } => Type::List(Box::new(open_object_type())),
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if matches!(
                function.as_str(),
                "Number/project_width"
                    | "Number/project_offset"
                    | "Number/project_time"
                    | "Number/interpolate"
                    | "Number/min"
                    | "Number/max"
                    | "Number/bit_width"
                    | "List/count"
                    | "List/sum"
                    | "Text/find"
                    | "Text/length"
                    | "Text/to_number"
            ) =>
        {
            Type::Number
        }
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if matches!(
                function.as_str(),
                "Text/empty"
                    | "Text/space"
                    | "Text/trim"
                    | "Text/to_uppercase"
                    | "Text/concat"
                    | "Text/time_range_label"
                    | "Text/substring"
                    | "Number/to_text"
                    | "Number/to_codepoint_text"
                    | "Number/to_ascii_text"
                    | "Error/text"
                    | "Router/route"
                    | "Router/go_to"
                    | "Ulid/generate"
            ) =>
        {
            Type::Text
        }
        AstExprKind::Call { function, args }
        | AstExprKind::Pipe {
            op: function, args, ..
        } if function == "List/chunk" => {
            let output_name = |role: &str, fallback: &str| {
                args.iter()
                    .find(|arg| arg.name.as_deref() == Some(role))
                    .and_then(|arg| expressions.get(arg.value))
                    .and_then(expr_single_name)
                    .unwrap_or(fallback)
                    .to_owned()
            };
            Type::List(Box::new(Type::Object(ObjectShape::from_ordered_fields(
                [
                    (output_name("label", "label"), Type::Text),
                    (
                        output_name("items", "items"),
                        Type::List(Box::new(open_object_type())),
                    ),
                ],
                true,
            ))))
        }
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if function == "Bool/not"
                || function == "Bool/and"
                || function == "Bool/toggle"
                || function == "Text/is_empty"
                || function == "Text/all_chars_in"
                || function == "Text/is_not_empty"
                || function == "Text/starts_with"
                || function == "Text/contains"
                || function == "List/every" =>
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
        AstExprKind::Draining { input } => expressions
            .get(*input)
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

fn statement_contains_render_context_syntax(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> bool {
    statement_field(statement).as_deref().is_some_and(|field| {
        matches!(
            field,
            "document" | "scene" | "root" | "child" | "items" | "children"
        )
    }) || statement
        .expr
        .is_some_and(|expr_id| expr_contains_render_constructor(expr_id, expressions))
        || statement
            .children
            .iter()
            .any(|child| statement_contains_render_context_syntax(child, expressions))
}

fn expr_contains_render_constructor(expr_id: usize, expressions: &[AstExpr]) -> bool {
    expr_contains_render_constructor_seen(expr_id, expressions, &mut BTreeSet::new())
}

fn expr_contains_render_constructor_seen(
    expr_id: usize,
    expressions: &[AstExpr],
    seen: &mut BTreeSet<usize>,
) -> bool {
    if !seen.insert(expr_id) {
        return false;
    }
    let Some(expr) = expressions.get(expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Call { function, args } => {
            is_registered_render_constructor(function)
                || args
                    .iter()
                    .any(|arg| expr_contains_render_constructor_seen(arg.value, expressions, seen))
        }
        AstExprKind::Pipe { input, op, args } => {
            is_registered_render_constructor(op)
                || expr_contains_render_constructor_seen(*input, expressions, seen)
                || args
                    .iter()
                    .any(|arg| expr_contains_render_constructor_seen(arg.value, expressions, seen))
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial }
        | AstExprKind::Draining { input: initial } => {
            expr_contains_render_constructor_seen(*initial, expressions, seen)
        }
        AstExprKind::Then { input, output } => {
            expr_contains_render_constructor_seen(*input, expressions, seen)
                || output.is_some_and(|output| {
                    expr_contains_render_constructor_seen(output, expressions, seen)
                })
        }
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_contains_render_constructor_seen(*output, expressions, seen),
        AstExprKind::Infix { left, right, .. } => {
            expr_contains_render_constructor_seen(*left, expressions, seen)
                || expr_contains_render_constructor_seen(*right, expressions, seen)
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields
            .iter()
            .any(|field| expr_contains_render_constructor_seen(field.value, expressions, seen)),
        AstExprKind::BytesLiteral { items, .. } => items
            .iter()
            .any(|item| expr_contains_render_constructor_seen(*item, expressions, seen)),
        AstExprKind::ListLiteral { .. }
        | AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::MatchArm { output: None, .. }
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => false,
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
        AstExprKind::Drain { path } => {
            let parts = drain_path_parts(path);
            if let Some(root) = parts.first().filter(|root| params.contains(*root)) {
                if parts.len() == 1 {
                    if let Some(expected) = expected {
                        add_param_requirement(requirements, root, expected);
                    }
                } else {
                    add_param_requirement(
                        requirements,
                        root,
                        object_type_for_path_requirement(&parts[1..], expected),
                    );
                }
            }
        }
        AstExprKind::Call { function, args } => {
            for arg in args {
                let expected = builtin_argument_expected_type(function, arg.name.as_deref(), false);
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
            for arg in args {
                let expected = builtin_argument_expected_type(op, arg.name.as_deref(), true);
                collect_param_requirements_expr(
                    arg.value,
                    expressions,
                    params,
                    requirements,
                    expected,
                );
            }
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::Draining { input: initial } => {
            collect_param_requirements_expr(*initial, expressions, params, requirements, expected);
        }
        AstExprKind::When { input } => {
            collect_param_requirements_expr(*input, expressions, params, requirements, None);
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
            let expected = if matches!(op.as_str(), "+" | "-" | "*" | "/" | ">" | "<" | ">=" | "<=")
            {
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
        AstExprKind::BytesLiteral { items, .. } => {
            for item in items {
                collect_param_requirements_expr(*item, expressions, params, requirements, None);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
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
        [(field.clone(), field_type)],
        true,
    ))
}

fn pipe_input_expected_type(function: &str) -> Option<Type> {
    if function == "List/map"
        || matches!(
            function,
            "List/retain"
                | "List/count"
                | "List/every"
                | "List/any"
                | "List/is_not_empty"
                | "List/latest"
        )
    {
        Some(Type::List(Box::new(open_object_type())))
    } else if matches!(
        function,
        "Text/to_bytes" | "File/read_bytes" | "File/read_text" | "Log/error" | "Log/info"
    ) {
        Some(Type::Text)
    } else if function == "File/write_bytes" {
        Some(Type::Bytes(BytesType::Dynamic))
    } else if function.starts_with("Text/") {
        Some(Type::Text)
    } else if matches!(
        function,
        "Bytes/length"
            | "Bytes/is_empty"
            | "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/concat"
            | "Bytes/equal"
            | "Bytes/find"
            | "Bytes/starts_with"
            | "Bytes/ends_with"
            | "Bytes/to_text"
            | "Bytes/to_hex"
            | "Bytes/to_base64"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
    ) {
        Some(Type::Bytes(BytesType::Dynamic))
    } else if matches!(function, "Bytes/from_hex" | "Bytes/from_base64") {
        Some(Type::Text)
    } else if function.starts_with("Number/") {
        Some(Type::Number)
    } else if function == "Bool/not" || function == "Bool/and" || function == "Bool/toggle" {
        Some(true_false_type())
    } else {
        None
    }
}

fn argument_expected_type(function: &str) -> Option<Type> {
    if function == "Bool/not" || function == "Bool/and" || function == "Bool/toggle" {
        Some(true_false_type())
    } else if function == "Text/to_bytes" {
        None
    } else if matches!(
        function,
        "File/read_bytes" | "File/read_text" | "File/write_bytes" | "Log/error" | "Log/info"
    ) || function.starts_with("Text/")
    {
        Some(Type::Text)
    } else if function.starts_with("Number/") {
        Some(Type::Number)
    } else {
        None
    }
}

fn builtin_argument_expected_type(
    function: &str,
    arg_name: Option<&str>,
    piped: bool,
) -> Option<Type> {
    if let Some(signature) = host_effect_signature(function) {
        return arg_name.and_then(|arg_name| {
            signature
                .intent_fields
                .into_iter()
                .find_map(|(name, ty)| (name == arg_name).then_some(ty))
        });
    }
    if function == "Bool/toggle" && arg_name == Some("when") {
        return Some(Type::Unknown);
    }
    if function == "File/write_bytes" {
        return match (piped, arg_name) {
            (true, Some("path") | None) => Some(Type::Text),
            (false, Some("input") | None) => Some(Type::Bytes(BytesType::Dynamic)),
            (false, Some("path")) => Some(Type::Text),
            _ => None,
        };
    }
    if matches!(function, "File/read_bytes" | "File/read_text") {
        return match arg_name {
            Some("path") | Some("input") | None => Some(Type::Text),
            _ => None,
        };
    }
    render_arg_expected_type(function, arg_name)
        .or_else(|| list_argument_expected_type(function, arg_name))
        .or_else(|| bytes_argument_expected_type(function, arg_name))
        .or_else(|| text_argument_expected_type(function, arg_name, piped))
        .or_else(|| number_argument_expected_type(function, arg_name, piped))
        .or_else(|| argument_expected_type(function))
}

fn list_argument_expected_type(function: &str, arg_name: Option<&str>) -> Option<Type> {
    match (function, arg_name) {
        ("List/retain" | "List/every" | "List/any", Some("if")) => Some(true_false_type()),
        _ => None,
    }
}

fn text_argument_expected_type(
    function: &str,
    arg_name: Option<&str>,
    piped: bool,
) -> Option<Type> {
    match (function, arg_name) {
        // Current function parameter inference can still classify generic
        // helper parameters as TEXT before their numeric use is observed.
        // Unknown blocks the old all-Text fallback without over-constraining
        // otherwise valid formula helpers.
        ("Text/substring", Some("start" | "length")) => Some(Type::Unknown),
        ("Text/substring", Some("input" | "text")) => Some(Type::Text),
        ("Text/find", Some("needle" | "input" | "text")) => Some(Type::Text),
        ("Text/starts_with", Some("prefix" | "input" | "text")) => Some(Type::Text),
        ("Text/ends_with", Some("suffix" | "input" | "text")) => Some(Type::Text),
        ("Text/concat", Some("with" | "separator" | "input" | "text") | None) => {
            Some(Type::Unknown)
        }
        ("Text/time_range_label", Some("end" | "unit" | "input" | "text") | None) => {
            Some(Type::Unknown)
        }
        ("Text/to_number", Some("radix" | "fallback")) => Some(Type::Number),
        ("Text/to_number", Some("leading")) => Some(true_false_type()),
        ("Text/to_number", Some("input" | "text")) => Some(Type::Text),
        ("Text/to_number", None) if piped => Some(Type::Number),
        ("Text/to_number", None) => Some(Type::Text),
        ("Text/to_bytes", Some("input" | "text")) => Some(Type::Text),
        ("Text/to_bytes", Some("encoding")) => Some(Type::Unknown),
        _ => None,
    }
}

fn number_argument_expected_type(
    function: &str,
    arg_name: Option<&str>,
    piped: bool,
) -> Option<Type> {
    match (function, arg_name) {
        ("Number/to_text", Some("prefix")) => Some(true_false_type()),
        ("Number/to_text", Some("radix" | "min_width" | "signed_width" | "group_size")) => {
            Some(Type::Number)
        }
        ("Number/to_text", None) if piped => Some(Type::Number),
        ("Number/project_time", Some("pointer_x" | "pointer_width")) => Some(Type::Unknown),
        ("Number/project_time", Some("viewport_start" | "viewport_end" | "fallback")) => {
            Some(Type::Number)
        }
        ("Number/project_time", None) => Some(Type::Unknown),
        _ => None,
    }
}

fn builtin_pipe_input_custom_expected_label(function: &str) -> Option<&'static str> {
    match function {
        "Text/concat" | "Text/time_range_label" => Some("TEXT, NUMBER, BOOL, or tag"),
        _ => None,
    }
}

fn builtin_pipe_input_custom_accepts(function: &str, actual: &Type) -> bool {
    match function {
        "Text/concat" | "Text/time_range_label" => type_is_text_formattable_scalar(actual),
        _ => false,
    }
}

fn builtin_argument_custom_expected_label(
    function: &str,
    arg_name: Option<&str>,
    _piped: bool,
) -> Option<&'static str> {
    match (function, arg_name) {
        ("Text/concat", Some("with" | "separator" | "input" | "text") | None) => {
            Some("TEXT, NUMBER, BOOL, or tag")
        }
        ("Text/time_range_label", Some("end" | "unit" | "input" | "text") | None) => {
            Some("TEXT, NUMBER, BOOL, or tag")
        }
        ("Number/project_time", Some("pointer_x" | "pointer_width") | None) => {
            Some("NUMBER or numeric TEXT")
        }
        _ => None,
    }
}

fn builtin_argument_custom_accepts(
    function: &str,
    arg_name: Option<&str>,
    actual: &Type,
    _piped: bool,
) -> bool {
    match (function, arg_name) {
        ("Text/concat", Some("with" | "separator" | "input" | "text") | None)
        | ("Text/time_range_label", Some("end" | "unit" | "input" | "text") | None) => {
            type_is_text_formattable_scalar(actual)
        }
        ("Number/project_time", Some("pointer_x" | "pointer_width") | None) => {
            type_is_number_or_numeric_text(actual)
        }
        _ => false,
    }
}

fn type_is_text_formattable_scalar(ty: &Type) -> bool {
    if matches!(
        ty,
        Type::Text
            | Type::Number
            | Type::Byte
            | Type::Unknown
            | Type::Var(_)
            | Type::UnresolvedShape { .. }
    ) || is_open_object_type(ty)
    {
        return true;
    }
    matches!(
        ty,
        Type::VariantSet(variants)
            if variants.iter().all(|variant| matches!(variant, Variant::Tag(_)))
    )
}

fn type_is_number_or_numeric_text(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Number
            | Type::Text
            | Type::Byte
            | Type::Unknown
            | Type::Var(_)
            | Type::UnresolvedShape { .. }
    ) || is_open_object_type(ty)
}

fn bool_toggle_when_accepts_flow(actual: &FlowType, is_event_payload_or_placeholder: bool) -> bool {
    matches!(
        actual.mode,
        FlowMode::TickPresent | FlowMode::PresentOrAbsent
    ) || is_event_payload_or_placeholder
        || matches!(
            actual.ty,
            Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
        )
        || is_open_object_type(&actual.ty)
}

fn bytes_argument_expected_type(function: &str, arg_name: Option<&str>) -> Option<Type> {
    match (function, arg_name) {
        (
            "Bytes/length"
            | "Bytes/is_empty"
            | "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/find"
            | "Bytes/starts_with"
            | "Bytes/ends_with"
            | "Bytes/to_text"
            | "Bytes/to_hex"
            | "Bytes/to_base64"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed",
            Some("input" | "left"),
        ) => Some(Type::Bytes(BytesType::Dynamic)),
        (
            "Bytes/concat" | "Bytes/equal" | "Bytes/find" | "Bytes/starts_with" | "Bytes/ends_with",
            Some("input" | "left" | "right" | "with" | "needle" | "prefix" | "suffix"),
        ) => Some(Type::Bytes(BytesType::Dynamic)),
        ("Bytes/from_hex" | "Bytes/from_base64", Some("input" | "text")) => Some(Type::Text),
        ("Bytes/to_text", Some("encoding")) => Some(Type::Unknown),
        ("Bytes/set", Some("value")) => Some(Type::Byte),
        (
            "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
            | "Bytes/zeros",
            Some("index" | "offset" | "start" | "length" | "count" | "byte_count" | "value"),
        ) => Some(Type::Number),
        (
            "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed",
            Some("endian"),
        ) => Some(Type::Unknown),
        _ => None,
    }
}

fn is_bytes_boundary_builtin(function: &str) -> bool {
    matches!(
        function,
        "Text/to_bytes"
            | "Bytes/length"
            | "Bytes/is_empty"
            | "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/concat"
            | "Bytes/equal"
            | "Bytes/find"
            | "Bytes/starts_with"
            | "Bytes/ends_with"
            | "Bytes/zeros"
            | "Bytes/to_text"
            | "Bytes/to_hex"
            | "Bytes/from_hex"
            | "Bytes/to_base64"
            | "Bytes/from_base64"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
    )
}

fn bytes_builtin_arg_allowed(function: &str, name: &str, piped: bool) -> bool {
    if piped && matches!(name, "input" | "text" | "left" | "right") {
        return false;
    }
    match function {
        "Text/to_bytes" => matches!(name, "input" | "text" | "encoding"),
        "Bytes/length" | "Bytes/is_empty" | "Bytes/to_hex" | "Bytes/to_base64" => name == "input",
        "Bytes/get" => matches!(name, "input" | "index"),
        "Bytes/set" => matches!(name, "input" | "index" | "value"),
        "Bytes/slice" => matches!(
            name,
            "input" | "offset" | "start" | "byte_count" | "length" | "count"
        ),
        "Bytes/take" | "Bytes/drop" => {
            matches!(name, "input" | "byte_count" | "length" | "count")
        }
        "Bytes/concat" | "Bytes/equal" => matches!(name, "input" | "with" | "left" | "right"),
        "Bytes/find" => matches!(name, "input" | "needle"),
        "Bytes/starts_with" => matches!(name, "input" | "prefix"),
        "Bytes/ends_with" => matches!(name, "input" | "suffix"),
        "Bytes/zeros" => matches!(name, "byte_count" | "length" | "count"),
        "Bytes/to_text" => matches!(name, "input" | "encoding"),
        "Bytes/from_hex" | "Bytes/from_base64" => matches!(name, "input" | "text"),
        "Bytes/read_unsigned" | "Bytes/read_signed" => {
            matches!(name, "input" | "offset" | "byte_count" | "endian")
        }
        "Bytes/write_unsigned" | "Bytes/write_signed" => {
            matches!(name, "input" | "offset" | "byte_count" | "endian" | "value")
        }
        _ => true,
    }
}

fn static_hex_decoded_len(text: &str) -> Option<usize> {
    let mut digits = 0usize;
    for byte in text.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if !byte.is_ascii_hexdigit() {
            return None;
        }
        digits = digits.checked_add(1)?;
    }
    digits.is_multiple_of(2).then_some(digits / 2)
}

fn static_base64_decoded_len(text: &str) -> Option<usize> {
    let input = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if input.is_empty() {
        return Some(0);
    }
    if input.len() % 4 != 0 {
        return None;
    }
    let chunk_count = input.len() / 4;
    let mut decoded_len = 0usize;
    for (chunk_index, chunk) in input.chunks_exact(4).enumerate() {
        let final_chunk = chunk_index == chunk_count - 1;
        if chunk[0] == b'=' || chunk[1] == b'=' {
            return None;
        }
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (!final_chunk && padding > 0) {
            return None;
        }
        if padding == 1 && chunk[2] == b'=' {
            return None;
        }
        for byte in &chunk[..4 - padding] {
            if !static_base64_digit(*byte) {
                return None;
            }
        }
        decoded_len = decoded_len.checked_add(3usize.checked_sub(padding)?)?;
    }
    Some(decoded_len)
}

fn static_base64_digit(byte: u8) -> bool {
    matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/')
}

fn render_arg_expected_type(function: &str, arg_name: Option<&str>) -> Option<Type> {
    if !is_registered_render_constructor(function) {
        return None;
    }
    match arg_name {
        Some("input" | "root" | "child") => Some(Type::RenderContract),
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
    function_args_by_name: &BTreeMap<String, Vec<String>>,
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
    collect_row_scope_bindings(
        program,
        function_param_requirements,
        function_args_by_name,
        &mut bindings,
    );
    collect_state_cell_path_bindings(program, &mut bindings);
    bindings
}

fn collect_state_cell_path_bindings(
    program: &ParsedProgram,
    bindings: &mut BTreeMap<String, Type>,
) {
    for cell in &program.state_cells {
        let ty = bindings
            .get(cell.hold_name.as_str())
            .cloned()
            .unwrap_or_else(open_object_type);
        bindings.insert(cell.path.clone(), ty.clone());
        if let Some(last) = cell.path.rsplit('.').next() {
            bindings.entry(last.to_owned()).or_insert(ty);
        }
    }
    for item in &program.ast.items {
        let (Some(field), Some(hold_name)) = (item.field.as_ref(), item.hold.as_ref()) else {
            continue;
        };
        let ty = bindings
            .get(hold_name.as_str())
            .cloned()
            .unwrap_or_else(open_object_type);
        bindings.entry(field.clone()).or_insert(ty);
    }
}

fn passed_context_type(program: &ParsedProgram, bindings: &BTreeMap<String, Type>) -> Option<Type> {
    let mut context: Option<Type> = None;
    for expr in &program.expressions {
        let args = match &expr.kind {
            AstExprKind::Call { args, .. } | AstExprKind::Pipe { args, .. } => args.as_slice(),
            _ => continue,
        };
        for arg in args
            .iter()
            .filter(|arg| arg.name.as_deref() == Some("PASS"))
        {
            let Some(value_expr) = program.expressions.get(arg.value) else {
                continue;
            };
            let Some(arg_type) =
                static_expr_type_from_bindings(value_expr, &program.expressions, bindings)
            else {
                continue;
            };
            for context_type in passed_context_candidates(arg_type) {
                context = Some(match context {
                    Some(existing) => widen_structural_type(&existing, &context_type),
                    None => context_type,
                });
            }
        }
    }
    context
}

fn passed_context_candidates(ty: Type) -> Vec<Type> {
    let mut candidates = vec![ty.clone()];
    if let Type::Object(shape) = ty {
        for field_ty in shape.fields.values() {
            if matches!(field_ty, Type::Object(_)) {
                candidates.push(field_ty.clone());
            }
        }
    }
    candidates
}

fn static_expr_type_from_bindings(
    expr: &AstExpr,
    expressions: &[AstExpr],
    bindings: &BTreeMap<String, Type>,
) -> Option<Type> {
    match &expr.kind {
        AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
            Some(Type::Object(ObjectShape::from_ordered_fields(
                fields.iter().filter(|field| !field.spread).map(|field| {
                    (
                        field.name.clone(),
                        expressions
                            .get(field.value)
                            .and_then(|field_expr| {
                                static_expr_type_from_bindings(field_expr, expressions, bindings)
                            })
                            .unwrap_or_else(open_object_type),
                    )
                }),
                false,
            )))
        }
        AstExprKind::Identifier(name) => bindings.get(name).cloned(),
        AstExprKind::Path(parts) => static_path_type_from_bindings(parts, bindings),
        AstExprKind::Drain { path } => {
            static_path_type_from_bindings(&drain_path_parts(path), bindings)
        }
        AstExprKind::Draining { input } => expressions
            .get(*input)
            .and_then(|expr| static_expr_type_from_bindings(expr, expressions, bindings)),
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Some(Type::Text),
        AstExprKind::Number(_) => Some(Type::Number),
        AstExprKind::ByteLiteral { .. } => Some(Type::Byte),
        AstExprKind::BytesLiteral { size, items } => Some(static_bytes_literal_type(
            size,
            items,
            expressions,
            |expr| static_expr_type_from_bindings(expr, expressions, bindings),
        )),
        AstExprKind::Bool(_) => Some(true_false_type()),
        AstExprKind::Enum(tag) | AstExprKind::Tag(tag) if tag == "SKIP" => Some(Type::Skip),
        AstExprKind::Enum(tag) | AstExprKind::Tag(tag) => {
            Some(Type::VariantSet(vec![Variant::Tag(tag.clone())]))
        }
        _ => None,
    }
}

fn static_path_type_from_bindings(
    parts: &[String],
    bindings: &BTreeMap<String, Type>,
) -> Option<Type> {
    let path = parts.join(".");
    bindings.get(&path).cloned().or_else(|| {
        parts
            .first()
            .and_then(|base| bindings.get(base))
            .and_then(|base| type_for_nested_path(base, &parts[1..]))
    })
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
            AstStatementKind::Hold {
                name: Some(name), ..
            } => {
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
        AstExprKind::Draining { input } => expressions
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
                let ty = statement_value_type_from_bindings(statement, expressions, bindings)
                    .or_else(|| simple_statement_value_type(statement, expressions))
                    .unwrap_or_else(|| {
                        Type::Object(object_shape_for_statement(statement, expressions))
                    });
                insert_simple_binding_preserving_renderable(bindings, name, ty.clone());
                if path != *name {
                    bindings.insert(path, ty);
                }
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
            AstStatementKind::Hold {
                name: Some(name), ..
            } => {
                if let Some(ty) =
                    statement_value_type_from_bindings(statement, expressions, bindings)
                        .or_else(|| simple_statement_value_type(statement, expressions))
                {
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
                insert_simple_binding_preserving_renderable(bindings, name, ty.clone());
                let path = scoped_path(scope, name);
                if path != *name {
                    bindings.insert(path, ty);
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
            AstStatementKind::Source {
                field: Some(name), ..
            } => {
                let source_path = scoped_path(scope, name);
                let ty = source_payload_type_for_path(source_payload_types, &source_path)
                    .unwrap_or_else(exact_empty_object_type);
                insert_simple_binding_preserving_renderable(bindings, name, ty.clone());
                if source_path != *name {
                    bindings.insert(source_path, ty);
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
            AstStatementKind::Function { name, args } => {
                for arg in args {
                    let ty = function_param_requirements
                        .get(name)
                        .and_then(|requirements| requirements.get(arg))
                        .cloned()
                        .unwrap_or_else(|| unresolved_shape(format!("parameter `{arg}`")));
                    let next = if type_contains_renderable(&ty) {
                        ty
                    } else if let Some(existing) = bindings.get(arg) {
                        widen_structural_type(existing, &ty)
                    } else {
                        ty
                    };
                    bindings.insert(arg.clone(), next);
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
    function_args_by_name: &BTreeMap<String, Vec<String>>,
    bindings: &mut BTreeMap<String, Type>,
) {
    bindings.insert("if".to_owned(), open_object_type());
    bindings.insert("when".to_owned(), open_object_type());
    for row_scope in &program.row_scope_functions {
        let return_type = function_result_shape(program, &row_scope.function)
            .map(Type::Object)
            .unwrap_or_else(open_object_type);
        if is_renderable_type(&return_type) {
            continue;
        }
        bindings
            .entry(row_scope.row_scope.clone())
            .or_insert_with(open_object_type);
        if let Some(item_ty) = canonical_row_scope_type(
            program,
            bindings,
            function_param_requirements,
            function_args_by_name,
            &row_scope.function,
            &row_scope.list,
            &row_scope.row_scope,
            Some(return_type),
        ) {
            if let Type::Object(shape) = &item_ty {
                for (field, ty) in shape.ordered_fields() {
                    insert_simple_binding_preserving_renderable(bindings, field, ty.clone());
                    bindings.insert(format!("{}.{}", row_scope.row_scope, field), ty.clone());
                }
            }
            bindings.insert(row_scope.row_scope.clone(), item_ty.clone());
            if list_map_input_item_type_for_function_param(
                program,
                bindings,
                function_args_by_name,
                &row_scope.function,
                &row_scope.row_scope,
            )
            .is_none()
            {
                bindings
                    .entry(row_scope.list.clone())
                    .and_modify(|existing| {
                        *existing = Type::List(Box::new(item_ty.clone()));
                    })
                    .or_insert_with(|| Type::List(Box::new(item_ty)));
            }
        }
    }
    for expr in &program.expressions {
        if let AstExprKind::Pipe { op, args, .. } = &expr.kind
            && list_row_binding_operator(op)
        {
            for arg in args.iter().filter(|arg| arg.name.is_none()) {
                if let Some(name) = program
                    .expressions
                    .get(arg.value)
                    .and_then(expr_single_name)
                {
                    let mut item_ty = bindings.get(name).cloned().unwrap_or_else(open_object_type);
                    if let Some(predicate_expr) = args
                        .iter()
                        .find(|arg| arg.name.as_deref() == Some("if"))
                        .map(|arg| arg.value)
                        && let Some(requirement) = row_binding_requirement_for_expr(
                            program,
                            predicate_expr,
                            name,
                            Some(true_false_type()),
                        )
                    {
                        item_ty = widen_structural_type(&item_ty, &requirement);
                    }
                    if let Type::Object(shape) = &item_ty {
                        for (field, ty) in shape.ordered_fields() {
                            insert_simple_binding_preserving_renderable(
                                bindings,
                                field,
                                ty.clone(),
                            );
                            bindings.insert(format!("{name}.{field}"), ty.clone());
                        }
                    }
                    bindings.insert(name.to_owned(), item_ty);
                }
            }
        }
        if let AstExprKind::MatchArm { pattern, .. } = &expr.kind {
            for name in pattern_variable_names(pattern) {
                bindings.entry(name).or_insert(Type::Text);
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
    for item in &program.ast.items {
        for name in parser_item_pattern_variable_names(&item.symbols) {
            bindings.entry(name).or_insert(Type::Text);
        }
    }
}

fn parser_item_pattern_variable_names(symbols: &[String]) -> Vec<String> {
    let Some(arrow) = symbols.iter().position(|symbol| symbol == "=>") else {
        return Vec::new();
    };
    pattern_variable_names(&symbols[..arrow])
}

fn insert_simple_binding_preserving_renderable(
    bindings: &mut BTreeMap<String, Type>,
    name: &str,
    ty: Type,
) {
    if bindings.get(name).is_some_and(type_contains_renderable) && !type_contains_renderable(&ty) {
        return;
    }
    bindings.insert(name.to_owned(), ty);
}

fn list_row_binding_operator(op: &str) -> bool {
    matches!(op, "List/map" | "List/retain" | "List/every" | "List/any")
}

fn row_binding_requirement_for_expr(
    program: &ParsedProgram,
    expr_id: usize,
    binding: &str,
    expected: Option<Type>,
) -> Option<Type> {
    let expr = program.expressions.get(expr_id)?;
    match &expr.kind {
        AstExprKind::Identifier(name) if name == binding => expected,
        AstExprKind::Path(parts) if parts.first().is_some_and(|part| part == binding) => {
            Some(object_type_for_path_requirement(&parts[1..], expected))
        }
        AstExprKind::Drain { path } => {
            let parts = drain_path_parts(path);
            parts
                .first()
                .filter(|part| part.as_str() == binding)
                .map(|_| object_type_for_path_requirement(&parts[1..], expected))
        }
        AstExprKind::Pipe { input, op, args } => {
            let input_expected = pipe_input_expected_type(op);
            let mut requirement =
                row_binding_requirement_for_expr(program, *input, binding, input_expected);
            for arg in args {
                let expected = builtin_argument_expected_type(op, arg.name.as_deref(), true);
                if let Some(arg_requirement) =
                    row_binding_requirement_for_expr(program, arg.value, binding, expected)
                {
                    requirement = Some(match requirement {
                        Some(existing) => widen_structural_type(&existing, &arg_requirement),
                        None => arg_requirement,
                    });
                }
            }
            requirement
        }
        AstExprKind::Call { function, args } => {
            let mut requirement = None;
            for arg in args {
                let expected = builtin_argument_expected_type(function, arg.name.as_deref(), false);
                if let Some(arg_requirement) =
                    row_binding_requirement_for_expr(program, arg.value, binding, expected)
                {
                    requirement = Some(match requirement {
                        Some(existing) => widen_structural_type(&existing, &arg_requirement),
                        None => arg_requirement,
                    });
                }
            }
            requirement
        }
        AstExprKind::Infix { left, right, op } => {
            let expected = if matches!(op.as_str(), "+" | "-" | "*" | "/" | ">" | "<" | ">=" | "<=")
            {
                Some(Type::Number)
            } else {
                None
            };
            let left_requirement =
                row_binding_requirement_for_expr(program, *left, binding, expected.clone());
            let right_requirement =
                row_binding_requirement_for_expr(program, *right, binding, expected);
            match (left_requirement, right_requirement) {
                (Some(left), Some(right)) => Some(widen_structural_type(&left, &right)),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            }
        }
        AstExprKind::When { input }
        | AstExprKind::Hold { initial: input, .. }
        | AstExprKind::Draining { input } => {
            row_binding_requirement_for_expr(program, *input, binding, expected)
        }
        AstExprKind::Then { input, output } => {
            let input_requirement =
                row_binding_requirement_for_expr(program, *input, binding, None);
            let output_requirement = output.and_then(|output| {
                row_binding_requirement_for_expr(program, output, binding, expected)
            });
            match (input_requirement, output_requirement) {
                (Some(left), Some(right)) => Some(widen_structural_type(&left, &right)),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            }
        }
        AstExprKind::Object(fields)
        | AstExprKind::Record(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields
            .iter()
            .filter_map(|field| {
                row_binding_requirement_for_expr(program, field.value, binding, None)
            })
            .reduce(|left, right| widen_structural_type(&left, &right)),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => row_binding_requirement_for_expr(program, *output, binding, expected),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn canonical_row_scope_type(
    program: &ParsedProgram,
    bindings: &BTreeMap<String, Type>,
    function_param_requirements: &BTreeMap<String, BTreeMap<String, Type>>,
    function_args_by_name: &BTreeMap<String, Vec<String>>,
    function: &str,
    list: &str,
    row_scope: &str,
    canonical_return: Option<Type>,
) -> Option<Type> {
    let list_map_input_item_type = list_map_input_item_type_for_function_param(
        program,
        bindings,
        function_args_by_name,
        function,
        row_scope,
    );
    let list_item_type = list_map_input_item_type.or_else(|| {
        bindings
            .get(list)
            .and_then(|existing| match existing {
                Type::List(item) => Some((**item).clone()),
                _ => None,
            })
            .filter(type_has_known_user_shape)
            .or_else(|| list_item_shape(program, list).map(Type::Object))
    });

    let mut row_type = list_item_type
        .filter(type_has_known_user_shape)
        .or_else(|| canonical_return.filter(type_has_known_user_shape));

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

fn list_map_input_item_type_for_function_param(
    program: &ParsedProgram,
    bindings: &BTreeMap<String, Type>,
    function_args_by_name: &BTreeMap<String, Vec<String>>,
    function: &str,
    param: &str,
) -> Option<Type> {
    let function_args = function_args_by_name.get(function)?;
    let mut item_type = None;
    for expr in &program.expressions {
        let AstExprKind::Pipe { input, op, args } = &expr.kind else {
            continue;
        };
        if op != "List/map" {
            continue;
        }
        let Some(map_item_expr_id) = args
            .iter()
            .find(|arg| arg.name.is_none())
            .map(|arg| arg.value)
        else {
            continue;
        };
        let Some((template_function, template_args)) = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("new"))
            .and_then(|arg| program.expressions.get(arg.value))
            .and_then(child_template)
        else {
            continue;
        };
        if template_function != function {
            continue;
        }
        let Some(param_expr_id) =
            function_call_argument_expr(function_args, param, None, &template_args)
        else {
            continue;
        };
        if param_expr_id != map_item_expr_id
            && !exprs_name_same_binding(
                program.expressions.get(param_expr_id),
                program.expressions.get(map_item_expr_id),
            )
        {
            continue;
        }
        let input = pipeline_source_expr_id(
            &program.ast.statements,
            expr.id,
            *input,
            &program.expressions,
        );
        let Some(found) = list_item_type_for_expr_id(program, input, bindings) else {
            continue;
        };
        item_type = Some(match item_type {
            Some(existing) => merge_canonical_row_type(&existing, &found),
            None => found,
        });
    }
    item_type
}

fn exprs_name_same_binding(left: Option<&AstExpr>, right: Option<&AstExpr>) -> bool {
    match (
        left.and_then(expr_single_name),
        right.and_then(expr_single_name),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn list_item_type_for_expr_id(
    program: &ParsedProgram,
    expr_id: usize,
    bindings: &BTreeMap<String, Type>,
) -> Option<Type> {
    let expr = program.expressions.get(expr_id)?;
    if let AstExprKind::ListLiteral { items, .. } = &expr.kind {
        let item_type = items
            .iter()
            .filter_map(|item| program.expressions.get(*item))
            .map(|item| simple_expr_type(item, &program.expressions))
            .reduce(|left, right| merge_canonical_row_type(&left, &right));
        if item_type.as_ref().is_some_and(type_has_known_user_shape) {
            return item_type;
        }
    }
    let static_item_type = static_expr_type_from_bindings(expr, &program.expressions, bindings)
        .and_then(|ty| list_item_type_from_list_type(&ty));
    if static_item_type
        .as_ref()
        .is_some_and(|ty| !is_open_object_type(ty))
    {
        return static_item_type;
    }
    expr_path(Some(expr), &program.expressions)
        .and_then(|path| {
            bindings
                .get(&path)
                .and_then(list_item_type_from_list_type)
                .filter(|ty| !is_open_object_type(ty))
                .or_else(|| list_item_shape(program, &path).map(Type::Object))
        })
        .or_else(|| static_item_type.filter(type_has_known_user_shape))
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
        if semantic_block_statement(statement, expressions) {
            if let Some(returned) = semantic_block_return_statement(&statement.children) {
                collect_statement_shape_fields(
                    std::slice::from_ref(returned),
                    expressions,
                    fields,
                    field_order,
                );
            }
            continue;
        }
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

fn semantic_block_statement(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    matches!(statement.kind, AstStatementKind::Block)
        && statement
            .expr
            .and_then(|expr_id| expressions.get(expr_id))
            .is_some_and(
                |expr| matches!(&expr.kind, AstExprKind::Identifier(value) if value == "BLOCK"),
            )
}

fn semantic_block_return_statement(statements: &[AstStatement]) -> Option<&AstStatement> {
    statements
        .iter()
        .rev()
        .find(|statement| statement_output_name(statement).is_none())
        .or_else(|| statements.last())
}

fn list_item_shape(program: &ParsedProgram, list_name: &str) -> Option<ObjectShape> {
    if let Some(shape) = list_item_shape_from_field(program, list_name)
        && !shape.fields.is_empty()
    {
        return Some(shape);
    }
    let field_name = list_name.rsplit('.').next().unwrap_or(list_name);
    let list = find_list_statement(&program.ast.statements, field_name)?;
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
            if field.spread {
                continue;
            }
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
        (ty, no_element) if is_no_element_type(no_element) => ty.clone(),
        (no_element, ty) if is_no_element_type(no_element) => ty.clone(),
        (Type::Text, Type::Text) => Type::Text,
        (Type::Number, Type::Number) => Type::Number,
        (Type::Byte, Type::Byte) => Type::Byte,
        (Type::Bytes(left), Type::Bytes(right)) => match (left, right) {
            (BytesType::Fixed(left), BytesType::Fixed(right)) if left == right => {
                Type::Bytes(BytesType::Fixed(*left))
            }
            _ => Type::Bytes(BytesType::Dynamic),
        },
        (Type::Byte, Type::Bytes(_)) | (Type::Bytes(_), Type::Byte) => {
            Type::Bytes(BytesType::Dynamic)
        }
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
        if (left.fields.contains_key(field) || right.fields.contains_key(field))
            && seen.insert(field.as_str())
        {
            order.push(field.clone());
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

fn merge_shape_override(
    fields: &mut BTreeMap<String, Type>,
    field_order: &mut Vec<String>,
    shape: &ObjectShape,
) {
    for (field, ty) in shape.ordered_fields() {
        insert_shape_field_override(fields, field_order, field.clone(), ty.clone());
    }
}

fn insert_shape_field_override(
    fields: &mut BTreeMap<String, Type>,
    field_order: &mut Vec<String>,
    field: String,
    ty: Type,
) {
    if !fields.contains_key(&field) {
        field_order.push(field.clone());
    }
    fields.insert(field, ty);
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

fn drain_path_parts(path: &AstDrainPath) -> Vec<String> {
    match path {
        AstDrainPath::Binding { name } => vec![name.clone()],
        AstDrainPath::Field { binding, fields } => std::iter::once(binding.clone())
            .chain(fields.iter().cloned())
            .collect(),
        AstDrainPath::Passed { fields } => std::iter::once("PASSED".to_owned())
            .chain(fields.iter().cloned())
            .collect(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourcePayloadAccess {
    Direct(String),
    Field(String),
    UnknownField(String),
}

fn normalized_source_path_parts(parts: &[String]) -> Vec<String> {
    parts
        .iter()
        .filter(|part| !matches!(part.as_str(), "PASSED" | "event" | "events"))
        .cloned()
        .collect()
}

fn source_payload_access_for_suffix(suffix: &str) -> SourcePayloadAccess {
    match suffix {
        "change.text" => SourcePayloadAccess::Field("text".to_owned()),
        "change.bytes" => SourcePayloadAccess::Field("bytes".to_owned()),
        "key_down.key" => SourcePayloadAccess::Field("key".to_owned()),
        "press" | "click" | "double_click" | "blur" | "change" | "key_down" => {
            SourcePayloadAccess::Field(suffix.to_owned())
        }
        field if !field.contains('.') => SourcePayloadAccess::Field(field.to_owned()),
        _ => SourcePayloadAccess::UnknownField(suffix.to_owned()),
    }
}

fn simple_record_shape(fields: &[AstRecordField], expressions: &[AstExpr]) -> ObjectShape {
    let mut shape_fields = BTreeMap::new();
    let mut field_order = Vec::new();
    for field in fields {
        let ty = expressions
            .get(field.value)
            .map(|expr| simple_expr_type(expr, expressions))
            .unwrap_or_else(open_object_type);
        if field.spread {
            if let Type::Object(shape) = ty {
                merge_shape_override(&mut shape_fields, &mut field_order, &shape);
            }
            continue;
        }
        insert_shape_field_override(&mut shape_fields, &mut field_order, field.name.clone(), ty);
    }
    ObjectShape {
        fields: shape_fields,
        field_order,
        open: false,
    }
}

fn source_payload_field_type(field: &str) -> Type {
    match field {
        "press" | "click" | "double_click" | "blur" | "change" | "key_down" => {
            exact_empty_object_type()
        }
        "bytes" => Type::Bytes(BytesType::Dynamic),
        _ => Type::Text,
    }
}

fn declared_source_payload_field_type(
    source_lookup: &SourcePayloadPathLookup,
    source_payload_types: &BTreeMap<String, Type>,
    parts: &[String],
    field: &str,
) -> Option<Type> {
    source_lookup
        .source_paths_for_parts(parts)
        .into_iter()
        .find_map(|source_path| {
            let Type::Object(shape) =
                source_payload_type_for_path(source_payload_types, &source_path)?
            else {
                return None;
            };
            shape.fields.get(field).cloned()
        })
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

fn source_payload_shape_table(
    program: &ParsedProgram,
    source_paths: &BTreeSet<String>,
    source_lookup: &SourcePayloadPathLookup,
    host_effects: &HostEffectTable,
) -> Vec<SourcePayloadShapeEntry> {
    let mut fields_by_source = source_paths
        .iter()
        .map(|source_path| (source_path.clone(), BTreeMap::new()))
        .collect::<BTreeMap<String, BTreeMap<String, Type>>>();
    for expr in &program.expressions {
        let AstExprKind::Path(parts) = &expr.kind else {
            continue;
        };
        let Some(SourcePayloadAccess::Field(field)) = source_lookup.access_for_parts(parts) else {
            continue;
        };
        for source_path in source_lookup.source_paths_for_parts(parts) {
            if let Some(fields) = fields_by_source.get_mut(&source_path) {
                fields.insert(field.clone(), source_payload_field_type(&field));
            }
        }
    }
    collect_payload_pattern_fields(
        &program.ast.statements,
        &program.expressions,
        source_lookup,
        &mut fields_by_source,
    );
    for declaration in &host_effects.declarations {
        let variants = host_effect_variants(&declaration.result_type);
        for route in &declaration.result_routes {
            let Some(fields) = fields_by_source.get_mut(&route.source_path) else {
                continue;
            };
            let Some(result_fields) = variants.get(&route.variant) else {
                continue;
            };
            for (name, ty) in result_fields.ordered_fields() {
                fields.insert(name.clone(), ty.clone());
            }
        }
    }
    program
        .source_ports
        .iter()
        .map(|source| {
            let fields = fields_by_source
                .get(&source.path)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|(name, ty)| SourcePayloadShapeField { name, ty })
                .collect::<Vec<_>>();
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
        boon_facing_type_display_tree(ty),
    )
}

#[allow(clippy::too_many_arguments)]
fn type_hint_entry_for_labels(
    program: &ParsedProgram,
    expr_id: Option<usize>,
    line: usize,
    start: usize,
    end: usize,
    category: &str,
    compact_label: String,
    detail_label: String,
    display_tree: TypeDisplayNode,
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
        display_tree,
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
                                function_signature_display_tree(function),
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
            AstStatementKind::Block | AstStatementKind::Spread | AstStatementKind::Expression => {}
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
            [(event.clone(), payload)],
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
        if matches!(
            program.expressions.get(*expr_id).map(|expr| &expr.kind),
            Some(AstExprKind::Draining { .. } | AstExprKind::Hold { .. })
        ) {
            continue;
        }
        let Some(AstExpr {
            kind: AstExprKind::Pipe { op, args, .. },
            ..
        }) = program.expressions.get(*expr_id)
        else {
            ty = hint_type_for_expr_id(program, *expr_id, expr_types, name_bindings)?;
            continue;
        };
        ty = match op.as_str() {
            "List/retain"
            | "List/remove"
            | "List/filter_field_equal"
            | "List/filter_field_not_equal"
            | "List/move_field_first"
            | "List/move_field_last"
            | "SOURCE" => ty,
            "List/count" | "List/sum" => Type::Number,
            "List/join_field" => Type::Text,
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
            "Bool/not" | "Bool/and" | "Bool/toggle" | "Text/is_not_empty" | "List/every"
            | "List/any" | "List/is_not_empty" => true_false_type(),
            "List/latest" => list_item_type_from_list_type(&ty).unwrap_or_else(open_object_type),
            _ if op.starts_with("Field/") => {
                if let (Type::Object(shape), Some(field)) = (&ty, op.strip_prefix("Field/")) {
                    shape.fields.get(field).cloned().unwrap_or(Type::Unknown)
                } else {
                    Type::Unknown
                }
            }
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
                    | AstStatementKind::Spread
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

fn function_signature_display_tree(function: &FunctionTypeEntry) -> TypeDisplayNode {
    TypeDisplayNode::Function {
        name: Some(function.name.clone()),
        args: function
            .args
            .iter()
            .zip(function.arg_types.iter())
            .map(|(name, ty)| TypeDisplayFunctionArg {
                name: Some(name.clone()),
                ty: boon_facing_type_display_tree(ty),
            })
            .collect(),
        result: Box::new(boon_facing_type_display_tree(&function.result.ty)),
    }
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

fn collect_payload_pattern_fields(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    source_lookup: &SourcePayloadPathLookup,
    fields_by_source: &mut BTreeMap<String, BTreeMap<String, Type>>,
) {
    for statement in statements {
        if let Some(expr_id) = statement.expr
            && let Some(AstExpr {
                kind: AstExprKind::When { input },
                ..
            }) = expressions.get(expr_id)
        {
            for source_path in expr_source_paths(*input, expressions, source_lookup) {
                if let Some(fields) = fields_by_source.get_mut(&source_path) {
                    for child in &statement.children {
                        if let Some(AstExpr {
                            kind: AstExprKind::MatchArm { pattern, .. },
                            ..
                        }) = child.expr.and_then(|expr_id| expressions.get(expr_id))
                        {
                            for field in source_payload_fields_from_pattern(pattern) {
                                fields.insert(field.to_owned(), source_payload_field_type(&field));
                            }
                        }
                    }
                }
            }
        }
        collect_payload_pattern_fields(
            &statement.children,
            expressions,
            source_lookup,
            fields_by_source,
        );
    }
}

struct SourcePayloadPathLookup {
    exact_prefix: BTreeMap<String, Vec<String>>,
    suffix: BTreeMap<String, Vec<String>>,
    source_order: BTreeMap<String, usize>,
}

impl SourcePayloadPathLookup {
    fn new(source_paths: &BTreeSet<String>) -> Self {
        let mut exact_prefix = BTreeMap::<String, Vec<String>>::new();
        let mut suffix = BTreeMap::<String, Vec<String>>::new();
        let mut source_order = BTreeMap::new();
        for (index, source_path) in source_paths.iter().enumerate() {
            source_order.insert(source_path.clone(), index);
            for alias in source_path_aliases(source_path) {
                push_unique(
                    exact_prefix.entry(alias.clone()).or_default(),
                    source_path.clone(),
                );
                let parts = alias.split('.').collect::<Vec<_>>();
                for index in 0..parts.len() {
                    push_unique(
                        suffix.entry(parts[index..].join(".")).or_default(),
                        source_path.clone(),
                    );
                }
            }
        }
        Self {
            exact_prefix,
            suffix,
            source_order,
        }
    }

    fn access_for_parts(&self, parts: &[String]) -> Option<SourcePayloadAccess> {
        let normalized_parts = normalized_source_path_parts(parts);
        let path = normalized_parts.join(".");
        if path.is_empty() {
            return None;
        }

        let mut best = SourcePayloadAccessMatch::default();
        if let Some(sources) = self.suffix.get(&path) {
            for source_path in sources {
                best.push(self.source_index(source_path), || {
                    SourcePayloadAccess::Direct(source_path.clone())
                });
            }
        }

        let path_parts = path.split('.').collect::<Vec<_>>();
        for end in 1..path_parts.len() {
            let prefix = path_parts[..end].join(".");
            let suffix = path_parts[end..].join(".");
            if let Some(sources) = self.exact_prefix.get(&prefix) {
                for source_path in sources {
                    best.push(self.source_index(source_path), || {
                        source_payload_access_for_suffix(&suffix)
                    });
                }
            }
        }

        if let Some((field, base_without_field)) = normalized_parts.split_last() {
            let base_without_field = base_without_field.join(".");
            if !base_without_field.is_empty()
                && let Some(sources) = self.suffix.get(&base_without_field)
            {
                for source_path in sources {
                    best.push(self.source_index(source_path), || {
                        source_payload_access_for_suffix(field)
                    });
                }
            }
        }

        best.access
    }

    fn source_paths_for_parts(&self, parts: &[String]) -> Vec<String> {
        let normalized_parts = normalized_source_path_parts(parts);
        let path = normalized_parts.join(".");
        let path_without_payload = parts_without_payload(&normalized_parts).join(".");
        let mut matches = Vec::new();
        let path_parts = path.split('.').collect::<Vec<_>>();
        for end in 1..=path_parts.len() {
            if let Some(sources) = self.exact_prefix.get(&path_parts[..end].join(".")) {
                for source in sources {
                    push_unique(&mut matches, source.clone());
                }
            }
        }
        if let Some(sources) = self.suffix.get(&path_without_payload) {
            for source in sources {
                push_unique(&mut matches, source.clone());
            }
        }
        matches
    }

    fn source_index(&self, source_path: &str) -> usize {
        self.source_order
            .get(source_path)
            .copied()
            .unwrap_or(usize::MAX)
    }
}

#[derive(Default)]
struct SourcePayloadAccessMatch {
    index: Option<usize>,
    access: Option<SourcePayloadAccess>,
}

impl SourcePayloadAccessMatch {
    fn push(&mut self, source_index: usize, access: impl FnOnce() -> SourcePayloadAccess) {
        if self.index.is_none_or(|index| source_index < index) {
            self.index = Some(source_index);
            self.access = Some(access());
        }
    }
}

fn source_path_aliases(source_path: &str) -> Vec<String> {
    let mut aliases = vec![source_path.to_owned()];
    aliases.push(
        source_path
            .strip_prefix("store.")
            .unwrap_or(source_path)
            .to_owned(),
    );
    if let Some((_, relative)) = source_path.split_once('.') {
        aliases.push(relative.to_owned());
    }
    let mut unique = Vec::new();
    for alias in aliases {
        push_unique(&mut unique, alias);
    }
    unique
}

fn push_unique<T: Eq>(items: &mut Vec<T>, item: T) {
    if !items.iter().any(|existing| existing == &item) {
        items.push(item);
    }
}

fn expr_source_paths(
    expr_id: usize,
    expressions: &[AstExpr],
    source_lookup: &SourcePayloadPathLookup,
) -> Vec<String> {
    match expressions.get(expr_id).map(|expr| &expr.kind) {
        Some(AstExprKind::Identifier(value)) => {
            source_lookup.source_paths_for_parts(std::slice::from_ref(value))
        }
        Some(AstExprKind::Path(parts)) => source_lookup.source_paths_for_parts(parts),
        Some(AstExprKind::Pipe { input, .. }) | Some(AstExprKind::When { input }) => {
            expr_source_paths(*input, expressions, source_lookup)
        }
        _ => Vec::new(),
    }
}

fn parts_without_payload(parts: &[String]) -> &[String] {
    match parts.last().map(String::as_str) {
        Some("press" | "click" | "double_click" | "blur" | "change" | "key_down") => {
            &parts[..parts.len().saturating_sub(1)]
        }
        Some(_) => &parts[..parts.len().saturating_sub(1)],
        _ => parts,
    }
}

fn source_payload_fields_from_pattern(pattern: &[String]) -> Vec<String> {
    let mut fields = Vec::new();
    for window in pattern.windows(2) {
        if window[1].as_str() == ":" && !matches!(window[0].as_str(), "__" | "SKIP") {
            fields.push(window[0].clone());
        }
    }
    fields
}

fn path_is_source_path(source_paths: &BTreeSet<String>, path: &str) -> bool {
    let normalized_path = path
        .split('.')
        .filter(|part| !matches!(*part, "PASSED" | "event" | "events"))
        .collect::<Vec<_>>()
        .join(".");
    source_paths.iter().any(|source_path| {
        let store_relative = source_path
            .strip_prefix("store.")
            .unwrap_or(source_path.as_str());
        let scoped_relative = source_path
            .split_once('.')
            .map(|(_, relative)| relative)
            .unwrap_or(source_path.as_str());
        [source_path.as_str(), store_relative, scoped_relative]
            .into_iter()
            .any(|base| {
                base == normalized_path
                    || base.ends_with(&format!(".{normalized_path}"))
                    || normalized_path.starts_with(&format!("{base}."))
            })
    })
}

fn path_is_event_payload_parts(parts: &[String]) -> bool {
    parts.windows(2).any(|window| {
        window[0] == "event"
            && matches!(
                window[1].as_str(),
                "press" | "click" | "double_click" | "blur" | "change" | "key_down"
            )
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
        || RenderContractRegistry::default().is_any_renderable_object_type(ty)
        || is_no_element_type(ty)
}

fn is_document_render_object_type(ty: &Type) -> bool {
    RenderContractRegistry::default().is_any_renderable_object_type(ty)
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
        | Type::Byte
        | Type::Bytes(_)
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
        | Type::Byte
        | Type::Bytes(_)
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
        | Type::Byte
        | Type::Bytes(_)
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
        | Type::Byte
        | Type::Bytes(_)
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
mod tests;
