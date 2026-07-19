use boon_data::MAX_NUMBER_TEXT_DIGITS;
pub use boon_document_model::ProgramRole;
use boon_parser::{
    AstCallArg, AstCallArgKind, AstDrainPath, AstExpr, AstExprKind, AstParameter, AstParameterKind,
    AstRecordField, AstStatement, AstStatementKind, BytesSizeSyntax, ParsedProgram,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExternalFunctionArgument {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExternalFunctionType {
    pub args: Vec<ExternalFunctionArgument>,
    pub result: FlowType,
    pub pure: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExternalTypeEnvironment {
    pub current_role: ProgramRole,
    pub values: BTreeMap<String, FlowType>,
    pub functions: BTreeMap<String, ExternalFunctionType>,
    #[serde(default)]
    pub allow_unresolved: bool,
    #[serde(default)]
    pub local_function_requirements: BTreeMap<String, BTreeMap<String, Type>>,
}

impl ExternalTypeEnvironment {
    pub fn empty(current_role: ProgramRole) -> Self {
        Self {
            current_role,
            values: BTreeMap::new(),
            functions: BTreeMap::new(),
            allow_unresolved: false,
            local_function_requirements: BTreeMap::new(),
        }
    }

    pub fn provisional(current_role: ProgramRole) -> Self {
        Self {
            allow_unresolved: true,
            ..Self::empty(current_role)
        }
    }
}

impl Default for ExternalTypeEnvironment {
    fn default() -> Self {
        Self::empty(ProgramRole::Client)
    }
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
pub struct NamedValueTypeEntry {
    pub path: String,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NamedValueTypeTable {
    pub entries: Vec<NamedValueTypeEntry>,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct RenderSlotTable {
    pub slots: Vec<RenderSlot>,
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
pub struct HostPortTable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpServerPortTypeEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub websocket: Option<WebSocketServerPortTypeEntry>,
}

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct DeclId(pub u32);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct LexicalScopeId(pub u32);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct CheckedExprId(pub u32);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct CheckedStatementId(pub u32);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct CheckedCallId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedSpan {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedScopeKind {
    Root,
    Function,
    Block,
    Record,
    RepeatedOutput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedScope {
    pub id: LexicalScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<LexicalScopeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<DeclId>,
    pub kind: CheckedScopeKind,
    pub span: CheckedSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedDeclarationKind {
    Function,
    ValueParameter,
    OutParameter,
    FreshOut,
    PatternBinding,
    Field,
    Source,
    Hold,
    List,
    Builtin,
    External,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedDeclaration {
    pub id: DeclId,
    pub scope_id: LexicalScopeId,
    pub name: String,
    pub kind: CheckedDeclarationKind,
    pub flow_type: FlowType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<CheckedExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_scope: Option<LexicalScopeId>,
    pub span: CheckedSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedEvaluationScope {
    Parent,
    Output { formal: DeclId },
}

impl Default for CheckedEvaluationScope {
    fn default() -> Self {
        Self::Parent
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedEffectSummary {
    pub reads_state: bool,
    pub writes_state: bool,
    pub emits_source: bool,
    pub invokes_host: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedRecordField {
    pub name: String,
    pub value: CheckedExprId,
    pub spread: bool,
    pub span: CheckedSpan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedBlockBinding {
    pub declaration: DeclId,
    pub value: CheckedExprId,
    pub span: CheckedSpan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedExpressionKind {
    Read {
        target: DeclId,
        projection: Vec<String>,
    },
    Passed {
        projection: Vec<String>,
    },
    ExternalRead {
        canonical_path: String,
    },
    Drain {
        target: DeclId,
        projection: Vec<String>,
    },
    Text {
        value: String,
    },
    Number {
        value: String,
    },
    BytesByte {
        value: u8,
    },
    Bool {
        value: bool,
    },
    Tag {
        name: String,
    },
    TaggedObject {
        tag: String,
        fields: Vec<CheckedRecordField>,
    },
    Source,
    Call {
        call: CheckedCallId,
    },
    Draining {
        input: CheckedExprId,
    },
    Hold {
        initial: CheckedExprId,
        name: String,
    },
    Latest,
    When {
        input: CheckedExprId,
        #[serde(default)]
        arms: Vec<CheckedExprId>,
    },
    While {
        input: CheckedExprId,
        #[serde(default)]
        arms: Vec<CheckedExprId>,
    },
    Then {
        input: CheckedExprId,
        output: Option<CheckedExprId>,
    },
    Infix {
        left: CheckedExprId,
        op: String,
        right: CheckedExprId,
    },
    MatchArm {
        pattern: Vec<String>,
        #[serde(default)]
        bindings: Vec<DeclId>,
        output: Option<CheckedExprId>,
    },
    Block {
        #[serde(default)]
        bindings: Vec<CheckedBlockBinding>,
        result: Option<CheckedExprId>,
    },
    Object {
        fields: Vec<CheckedRecordField>,
    },
    Record {
        fields: Vec<CheckedRecordField>,
    },
    List {
        capacity: Option<usize>,
        items: Vec<CheckedExprId>,
    },
    Bytes {
        fixed_size: Option<usize>,
        items: Vec<CheckedExprId>,
    },
    Delimiter,
    Invalid {
        tokens: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedExpression {
    pub id: CheckedExprId,
    pub scope_id: LexicalScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclId>,
    pub flow_type: FlowType,
    pub effect: CheckedEffectSummary,
    pub kind: CheckedExpressionKind,
    pub span: CheckedSpan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedStatementKind {
    Function {
        declaration: DeclId,
    },
    Field {
        declaration: DeclId,
    },
    Source {
        declaration: Option<DeclId>,
        event: Option<String>,
    },
    Hold {
        declaration: Option<DeclId>,
        name: Option<String>,
    },
    List {
        declaration: Option<DeclId>,
        capacity: Option<usize>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedStatement {
    pub id: CheckedStatementId,
    pub scope_id: LexicalScopeId,
    pub kind: CheckedStatementKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<CheckedExprId>,
    #[serde(default)]
    pub value_use: CheckedValueUse,
    pub children: Vec<CheckedStatementId>,
    pub span: CheckedSpan,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedValueUse {
    #[default]
    RuntimeValue,
    RenderSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOccurrenceKind {
    Declaration,
    Read,
    Call,
    FreshOut,
    ForwardOut,
    Pass,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticOccurrence {
    pub target: DeclId,
    pub kind: SemanticOccurrenceKind,
    pub span: CheckedSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedCallableKind {
    User,
    Builtin,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedParameterKind {
    Value,
    Out,
}

impl From<AstParameterKind> for CheckedParameterKind {
    fn from(kind: AstParameterKind) -> Self {
        match kind {
            AstParameterKind::Value => Self::Value,
            AstParameterKind::Out => Self::Out,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedParameter {
    pub decl_id: DeclId,
    pub name: String,
    pub kind: CheckedParameterKind,
    pub ordinal: usize,
    pub flow_type: FlowType,
    #[serde(default)]
    pub evaluation_scope: CheckedEvaluationScope,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedContextualOperation {
    Map {
        list: DeclId,
        row: DeclId,
        body: DeclId,
    },
    Filter {
        list: DeclId,
        row: DeclId,
        predicate: DeclId,
    },
    Retain {
        list: DeclId,
        row: DeclId,
        predicate: DeclId,
    },
    Every {
        list: DeclId,
        row: DeclId,
        predicate: DeclId,
    },
    Any {
        list: DeclId,
        row: DeclId,
        predicate: DeclId,
    },
    Find {
        list: DeclId,
        row: DeclId,
        predicate: DeclId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedCallableSignature {
    pub decl_id: DeclId,
    pub scope_id: LexicalScopeId,
    pub kind: CheckedCallableKind,
    pub name: String,
    pub parameters: Vec<CheckedParameter>,
    pub result: FlowType,
    pub role: ProgramRole,
    pub effect: CheckedEffectSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<CheckedStatementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_expression: Option<CheckedExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contextual_operation: Option<CheckedContextualOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedCallEntry {
    Input {
        formal: DeclId,
        name: String,
        value: CheckedExprId,
        from_pipe: bool,
        evaluation_scope: CheckedEvaluationScope,
    },
    FreshOut {
        formal: DeclId,
        name: String,
        output: DeclId,
        scope_id: LexicalScopeId,
    },
    ForwardOut {
        formal: DeclId,
        name: String,
        target: DeclId,
        target_name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedCall {
    pub id: CheckedCallId,
    pub expression: CheckedExprId,
    pub callable: DeclId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_callable: Option<DeclId>,
    pub function: String,
    pub entries: Vec<CheckedCallEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<CheckedExprId>,
    pub result: FlowType,
    pub role: ProgramRole,
    pub span: CheckedSpan,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedProgram {
    pub role: ProgramRole,
    pub root_scope: LexicalScopeId,
    pub scopes: Vec<CheckedScope>,
    pub declarations: Vec<CheckedDeclaration>,
    pub statements: Vec<CheckedStatement>,
    pub expressions: Vec<CheckedExpression>,
    pub callables: Vec<CheckedCallableSignature>,
    pub calls: Vec<CheckedCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pattern_bindings: Vec<CheckedPatternBinding>,
    pub occurrences: Vec<SemanticOccurrence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedPatternBinding {
    pub declaration: DeclId,
    pub selector: CheckedExprId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HttpServerPortTypeEntry {
    pub line: usize,
    pub request_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disconnect_source: Option<String>,
    pub response_output: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebSocketServerPortTypeEntry {
    pub line: usize,
    pub open_source: String,
    pub message_source: String,
    pub close_source: String,
    pub error_source: String,
    pub actions_output: String,
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
    pub host_port_table: HostPortTable,
    pub full_document_typecheck_coverage: bool,
    #[serde(default)]
    pub output_root_types: Vec<OutputRootTypeEntry>,
    pub expr_type_table: ExprTypeTable,
    pub function_type_table: FunctionTypeTable,
    #[serde(default)]
    pub named_value_type_table: NamedValueTypeTable,
    pub type_hint_table: TypeHintTable,
    #[serde(default)]
    pub resolved_constant_table: ResolvedConstantTable,
    pub render_slot_table: RenderSlotTable,
    pub constraints: Vec<Constraint>,
    pub diagnostics: Vec<TypeDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckOutput {
    pub program: Option<CheckedProgram>,
    pub report: TypeCheckReport,
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

struct CheckedProgramBuilder<'a> {
    program: &'a ParsedProgram,
    role: ProgramRole,
    signatures: Vec<CheckedCallableSignature>,
    signature_by_name: BTreeMap<String, usize>,
    expression_owner: BTreeMap<usize, String>,
    next_decl_id: u32,
    next_scope_id: u32,
    next_call_id: u32,
    visited_exprs: BTreeSet<usize>,
    calls: Vec<CheckedCall>,
    scopes: Vec<CheckedScope>,
    declarations: Vec<CheckedDeclaration>,
    occurrences: Vec<SemanticOccurrence>,
    statement_declarations: BTreeMap<usize, DeclId>,
    statement_scopes: BTreeMap<usize, LexicalScopeId>,
    statement_body_scopes: BTreeMap<usize, LexicalScopeId>,
    expression_scopes: BTreeMap<usize, LexicalScopeId>,
    expression_declarations: BTreeMap<usize, DeclId>,
    scope_declarations: BTreeMap<(LexicalScopeId, String), DeclId>,
    pattern_declarations: BTreeMap<(usize, String), DeclId>,
    pattern_selectors: BTreeMap<usize, usize>,
    optional_parameters: BTreeSet<DeclId>,
    inferred_expr_types: BTreeMap<usize, FlowType>,
    builtin_static_symbol_exprs: BTreeSet<usize>,
    diagnostics: Vec<TypeDiagnostic>,
}

#[derive(Clone, Copy, Debug)]
enum ContextualBuiltinKind {
    Map,
    Filter,
    Retain,
    Every,
    Any,
    Find,
}

#[derive(Clone, Debug)]
struct AuthoritativeParameter {
    name: String,
    kind: CheckedParameterKind,
    flow_type: FlowType,
    required: bool,
}

#[derive(Clone, Debug)]
struct AuthoritativeCallableSignature {
    parameters: Vec<AuthoritativeParameter>,
    result: FlowType,
    effect: CheckedEffectSummary,
    contextual_builtin: Option<ContextualBuiltinKind>,
}

#[derive(Clone, Debug)]
struct InferredStructuralValue {
    flow_type: FlowType,
    structural_record: bool,
}

fn checked_contextual_operation(
    kind: ContextualBuiltinKind,
    parameters: &[CheckedParameter],
) -> CheckedContextualOperation {
    let [list, row, body] = parameters else {
        unreachable!("contextual builtin signatures always have three formals");
    };
    match kind {
        ContextualBuiltinKind::Map => CheckedContextualOperation::Map {
            list: list.decl_id,
            row: row.decl_id,
            body: body.decl_id,
        },
        ContextualBuiltinKind::Filter => CheckedContextualOperation::Filter {
            list: list.decl_id,
            row: row.decl_id,
            predicate: body.decl_id,
        },
        ContextualBuiltinKind::Retain => CheckedContextualOperation::Retain {
            list: list.decl_id,
            row: row.decl_id,
            predicate: body.decl_id,
        },
        ContextualBuiltinKind::Every => CheckedContextualOperation::Every {
            list: list.decl_id,
            row: row.decl_id,
            predicate: body.decl_id,
        },
        ContextualBuiltinKind::Any => CheckedContextualOperation::Any {
            list: list.decl_id,
            row: row.decl_id,
            predicate: body.decl_id,
        },
        ContextualBuiltinKind::Find => CheckedContextualOperation::Find {
            list: list.decl_id,
            row: row.decl_id,
            predicate: body.decl_id,
        },
    }
}

impl<'a> CheckedProgramBuilder<'a> {
    fn build(
        program: &'a ParsedProgram,
        external_types: &ExternalTypeEnvironment,
        expr_type_table: &ExprTypeTable,
        function_type_table: &FunctionTypeTable,
        named_value_type_table: &NamedValueTypeTable,
        render_slot_table: &RenderSlotTable,
        builtins: &BuiltinSignatureRegistry,
        render_contracts: &RenderContractRegistry,
    ) -> (CheckedProgram, Vec<TypeDiagnostic>) {
        let mut builder = Self {
            program,
            role: external_types.current_role,
            signatures: Vec::new(),
            signature_by_name: BTreeMap::new(),
            expression_owner: expression_owner_functions(program),
            next_decl_id: 1,
            next_scope_id: 1,
            next_call_id: 0,
            visited_exprs: BTreeSet::new(),
            calls: Vec::new(),
            scopes: vec![CheckedScope {
                id: LexicalScopeId(0),
                parent: None,
                owner: None,
                kind: CheckedScopeKind::Root,
                span: CheckedSpan::default(),
            }],
            declarations: Vec::new(),
            occurrences: Vec::new(),
            statement_declarations: BTreeMap::new(),
            statement_scopes: BTreeMap::new(),
            statement_body_scopes: BTreeMap::new(),
            expression_scopes: BTreeMap::new(),
            expression_declarations: BTreeMap::new(),
            scope_declarations: BTreeMap::new(),
            pattern_declarations: BTreeMap::new(),
            pattern_selectors: BTreeMap::new(),
            optional_parameters: BTreeSet::new(),
            inferred_expr_types: BTreeMap::new(),
            builtin_static_symbol_exprs: builtin_static_symbol_expression_ids(program),
            diagnostics: Vec::new(),
        };
        builder.collect_user_signatures(&program.ast.statements);
        builder.register_builtin_signatures(builtins);
        builder.register_field_projection_signatures(expr_type_table);
        builder.register_session_info_intrinsics();
        builder.register_render_signatures(render_contracts);
        builder.register_host_effect_signatures();
        builder.register_external_signatures(external_types);
        builder.predeclare_statement_tree(&program.ast.statements, LexicalScopeId(0));
        builder.predeclare_pattern_bindings(&program.ast.statements);

        let child_exprs = expression_child_ids(&program.expressions);
        for expr in &program.expressions {
            if !child_exprs.contains(&expr.id) {
                builder.visit_expr(expr.id, BTreeMap::new());
            }
        }
        for expr in &program.expressions {
            builder.visit_expr(expr.id, BTreeMap::new());
        }
        builder.validate_callable_coverage();
        builder.infer_contextual_scope_effects();
        builder.validate_output_producers();
        builder.apply_inferred_types(
            expr_type_table,
            function_type_table,
            named_value_type_table,
            external_types,
        );
        builder.infer_contextual_callable_schemes();
        builder.infer_checked_types();
        builder.specialize_consistent_user_body_types();
        builder.calls.sort_by_key(|call| call.expression);
        let (statements, expressions) = builder.lower_checked_tree(render_slot_table);
        builder.validate_user_callable_results();
        builder.validate_checked_expression_roots(&statements, &expressions);
        let pattern_bindings = builder.checked_pattern_bindings();
        let checked = CheckedProgram {
            role: builder.role,
            root_scope: LexicalScopeId(0),
            scopes: builder.scopes,
            declarations: builder.declarations,
            statements,
            expressions,
            callables: builder.signatures,
            calls: builder.calls,
            pattern_bindings,
            occurrences: builder.occurrences,
        };
        (checked, builder.diagnostics)
    }

    fn lexical_owner(&self, scope_id: LexicalScopeId) -> Option<DeclId> {
        let mut current = Some(scope_id);
        let mut visited = BTreeSet::new();
        while let Some(scope_id) = current {
            if !visited.insert(scope_id) {
                return None;
            }
            let scope = self.scopes.iter().find(|scope| scope.id == scope_id)?;
            if scope.owner.is_some() {
                return scope.owner;
            }
            current = scope.parent;
        }
        None
    }

    fn checked_pattern_bindings(&self) -> Vec<CheckedPatternBinding> {
        let mut result = self
            .pattern_declarations
            .iter()
            .filter_map(|((arm_expr_id, name), declaration)| {
                let selector = self.pattern_selectors.get(arm_expr_id).copied()?;
                let pattern =
                    self.program
                        .expressions
                        .get(*arm_expr_id)
                        .and_then(|expression| match &expression.kind {
                            AstExprKind::MatchArm { pattern, .. } => Some(pattern),
                            _ => None,
                        })?;
                let projection = if pattern_variant(pattern)
                    .is_some_and(|variant| matches!(variant, Variant::Tag(_)))
                {
                    vec![name.clone()]
                } else {
                    Vec::new()
                };
                Some(CheckedPatternBinding {
                    declaration: *declaration,
                    selector: CheckedExprId(selector as u32),
                    projection,
                })
            })
            .collect::<Vec<_>>();
        result.sort_by_key(|binding| binding.declaration);
        result
    }

    fn allocate_decl(&mut self) -> DeclId {
        let id = DeclId(self.next_decl_id);
        self.next_decl_id += 1;
        id
    }

    fn allocate_scope(&mut self) -> LexicalScopeId {
        let id = LexicalScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        id
    }

    fn collect_user_signatures(&mut self, statements: &[AstStatement]) {
        for statement in statements {
            if let AstStatementKind::Function { name, parameters } = &statement.kind {
                let callable = self.allocate_decl();
                let scope = self.allocate_scope();
                self.scopes.push(CheckedScope {
                    id: scope,
                    parent: Some(LexicalScopeId(0)),
                    owner: Some(callable),
                    kind: CheckedScopeKind::Function,
                    span: checked_statement_span(statement),
                });
                let checked_parameters = parameters
                    .iter()
                    .map(|parameter| CheckedParameter {
                        decl_id: self.allocate_decl(),
                        name: parameter.name.clone(),
                        kind: parameter.kind.into(),
                        ordinal: parameter.ordinal,
                        flow_type: unknown_flow_type(),
                        evaluation_scope: CheckedEvaluationScope::Parent,
                        start: parameter.start,
                        end: parameter.end,
                    })
                    .collect::<Vec<_>>();
                let index = self.signatures.len();
                if self.signature_by_name.insert(name.clone(), index).is_some() {
                    self.diagnostics.push(TypeDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        line: statement.line,
                        start: statement.start,
                        end: statement.end,
                        message: format!("function `{name}` is declared more than once"),
                    });
                }
                self.signatures.push(CheckedCallableSignature {
                    decl_id: callable,
                    scope_id: scope,
                    kind: CheckedCallableKind::User,
                    name: name.clone(),
                    parameters: checked_parameters.clone(),
                    result: unknown_flow_type(),
                    role: self.role,
                    effect: CheckedEffectSummary::default(),
                    body: Some(CheckedStatementId(statement.id as u32)),
                    result_expression: canonical_block_value_expression(
                        &statement.children,
                        &self.program.expressions,
                    )
                    .map(|expression| CheckedExprId(expression as u32)),
                    contextual_operation: None,
                });
                self.declarations.push(CheckedDeclaration {
                    id: callable,
                    scope_id: LexicalScopeId(0),
                    name: name.clone(),
                    kind: CheckedDeclarationKind::Function,
                    flow_type: unknown_flow_type(),
                    value: None,
                    body_scope: Some(scope),
                    span: checked_statement_span(statement),
                });
                self.statement_declarations.insert(statement.id, callable);
                self.scope_declarations
                    .insert((LexicalScopeId(0), name.clone()), callable);
                self.occurrences.push(SemanticOccurrence {
                    target: callable,
                    kind: SemanticOccurrenceKind::Declaration,
                    span: checked_statement_span(statement),
                });
                for parameter in checked_parameters {
                    self.scope_declarations
                        .insert((scope, parameter.name.clone()), parameter.decl_id);
                    let output_scope = (parameter.kind == CheckedParameterKind::Out).then(|| {
                        let output_scope = self.allocate_scope();
                        self.scopes.push(CheckedScope {
                            id: output_scope,
                            parent: Some(scope),
                            owner: Some(parameter.decl_id),
                            kind: CheckedScopeKind::RepeatedOutput,
                            span: CheckedSpan {
                                line: statement.line,
                                start: parameter.start,
                                end: parameter.end,
                            },
                        });
                        self.scope_declarations
                            .insert((output_scope, parameter.name.clone()), parameter.decl_id);
                        output_scope
                    });
                    self.declarations.push(CheckedDeclaration {
                        id: parameter.decl_id,
                        scope_id: scope,
                        name: parameter.name.clone(),
                        kind: match parameter.kind {
                            CheckedParameterKind::Value => CheckedDeclarationKind::ValueParameter,
                            CheckedParameterKind::Out => CheckedDeclarationKind::OutParameter,
                        },
                        flow_type: parameter.flow_type.clone(),
                        value: None,
                        body_scope: output_scope,
                        span: CheckedSpan {
                            line: statement.line,
                            start: parameter.start,
                            end: parameter.end,
                        },
                    });
                    self.occurrences.push(SemanticOccurrence {
                        target: parameter.decl_id,
                        kind: SemanticOccurrenceKind::Declaration,
                        span: CheckedSpan {
                            line: statement.line,
                            start: parameter.start,
                            end: parameter.end,
                        },
                    });
                }
            }
            self.collect_user_signatures(&statement.children);
        }
    }

    fn register_builtin_signatures(&mut self, builtins: &BuiltinSignatureRegistry) {
        for (name, signature) in builtins.authoritative_signatures() {
            if host_effect_signature(name).is_some() {
                continue;
            }
            self.register_authoritative_signature(name, CheckedCallableKind::Builtin, signature);
        }
    }

    fn register_field_projection_signatures(&mut self, expr_type_table: &ExprTypeTable) {
        let flow_types = expr_type_table
            .entries
            .iter()
            .map(|entry| (entry.expr_id, entry.flow_type.clone()))
            .collect::<BTreeMap<_, _>>();
        let projections = self
            .program
            .expressions
            .iter()
            .filter_map(|expr| {
                let AstExprKind::Pipe {
                    input, op, args, ..
                } = &expr.kind
                else {
                    return None;
                };
                (op.starts_with("Field/") && args.is_empty()).then(|| {
                    (
                        op.clone(),
                        flow_types
                            .get(input)
                            .cloned()
                            .unwrap_or_else(unknown_flow_type),
                        flow_types
                            .get(&expr.id)
                            .cloned()
                            .unwrap_or_else(unknown_flow_type),
                    )
                })
            })
            .fold(
                BTreeMap::<String, (FlowType, FlowType)>::new(),
                |mut projections, (function, input, result)| {
                    projections
                        .entry(function)
                        .and_modify(|(existing_input, existing_result)| {
                            existing_input.mode = merge_flow_modes(existing_input.mode, input.mode);
                            existing_input.ty =
                                merge_canonical_row_type(&existing_input.ty, &input.ty);
                            existing_result.mode =
                                merge_flow_modes(existing_result.mode, result.mode);
                            existing_result.ty =
                                merge_canonical_row_type(&existing_result.ty, &result.ty);
                        })
                        .or_insert((input, result));
                    projections
                },
            );
        for (function, (input, result)) in projections {
            self.register_authoritative_signature(
                &function,
                CheckedCallableKind::Builtin,
                AuthoritativeCallableSignature {
                    parameters: vec![AuthoritativeParameter {
                        name: "input".to_owned(),
                        kind: CheckedParameterKind::Value,
                        flow_type: input,
                        required: true,
                    }],
                    result,
                    effect: CheckedEffectSummary::default(),
                    contextual_builtin: None,
                },
            );
        }
    }

    fn register_session_info_intrinsics(&mut self) {
        for function in ["SessionInfo/status", "SessionInfo/principal"] {
            let result = session_info_intrinsic_type(function)
                .expect("registered SessionInfo intrinsic has a result type");
            self.register_authoritative_signature(
                function,
                CheckedCallableKind::Builtin,
                AuthoritativeCallableSignature {
                    parameters: Vec::new(),
                    result: continuous_flow_type(result),
                    effect: CheckedEffectSummary::default(),
                    contextual_builtin: None,
                },
            );
        }
    }

    fn register_render_signatures(&mut self, render_contracts: &RenderContractRegistry) {
        for (name, signature) in render_contracts.authoritative_signatures() {
            self.register_authoritative_signature(name, CheckedCallableKind::Builtin, signature);
        }
    }

    fn register_host_effect_signatures(&mut self) {
        let operations = self
            .program
            .expressions
            .iter()
            .filter_map(ast_callable_name)
            .collect::<BTreeSet<_>>();
        for operation in operations {
            let Some(signature) = host_effect_signature(operation) else {
                continue;
            };
            self.register_authoritative_signature(
                operation,
                CheckedCallableKind::Builtin,
                AuthoritativeCallableSignature {
                    parameters: signature
                        .intent_fields
                        .into_iter()
                        .map(|field| AuthoritativeParameter {
                            name: field.name,
                            kind: CheckedParameterKind::Value,
                            flow_type: continuous_flow_type(field.ty),
                            required: !field.has_default,
                        })
                        .collect(),
                    result: continuous_flow_type(signature.result_type),
                    effect: CheckedEffectSummary {
                        invokes_host: true,
                        ..CheckedEffectSummary::default()
                    },
                    contextual_builtin: None,
                },
            );
        }
    }

    fn register_external_signatures(&mut self, external_types: &ExternalTypeEnvironment) {
        for (name, signature) in &external_types.functions {
            self.register_authoritative_signature(
                name,
                CheckedCallableKind::External,
                AuthoritativeCallableSignature {
                    parameters: signature
                        .args
                        .iter()
                        .map(|argument| AuthoritativeParameter {
                            name: argument.name.clone(),
                            kind: CheckedParameterKind::Value,
                            flow_type: continuous_flow_type(argument.ty.clone()),
                            required: true,
                        })
                        .collect(),
                    result: signature.result.clone(),
                    effect: CheckedEffectSummary::default(),
                    contextual_builtin: None,
                },
            );
        }
        for (name, flow_type) in &external_types.values {
            let declaration = self.allocate_decl();
            self.declarations.push(CheckedDeclaration {
                id: declaration,
                scope_id: LexicalScopeId(0),
                name: name.clone(),
                kind: CheckedDeclarationKind::External,
                flow_type: flow_type.clone(),
                value: None,
                body_scope: None,
                span: CheckedSpan::default(),
            });
            self.scope_declarations
                .insert((LexicalScopeId(0), name.clone()), declaration);
        }
    }

    fn register_authoritative_signature(
        &mut self,
        name: &str,
        kind: CheckedCallableKind,
        signature: AuthoritativeCallableSignature,
    ) {
        if self.signature_by_name.contains_key(name) {
            return;
        }
        let callable = self.allocate_decl();
        let mut checked_parameters = signature
            .parameters
            .into_iter()
            .enumerate()
            .map(|(ordinal, parameter)| {
                let decl_id = self.allocate_decl();
                if !parameter.required {
                    self.optional_parameters.insert(decl_id);
                }
                CheckedParameter {
                    decl_id,
                    name: parameter.name,
                    kind: parameter.kind,
                    ordinal,
                    flow_type: parameter.flow_type,
                    evaluation_scope: CheckedEvaluationScope::Parent,
                    start: 0,
                    end: 0,
                }
            })
            .collect::<Vec<_>>();
        let output = checked_parameters
            .iter()
            .find(|parameter| parameter.kind == CheckedParameterKind::Out)
            .map(|parameter| parameter.decl_id);
        if let Some(output) = output {
            for parameter in &mut checked_parameters {
                if parameter.kind == CheckedParameterKind::Value
                    && matches!(parameter.name.as_str(), "new" | "if" | "when")
                {
                    parameter.evaluation_scope = CheckedEvaluationScope::Output { formal: output };
                }
            }
        }
        let contextual_operation = signature
            .contextual_builtin
            .map(|operation| checked_contextual_operation(operation, &checked_parameters));
        let index = self.signatures.len();
        self.signature_by_name.insert(name.to_owned(), index);
        self.signatures.push(CheckedCallableSignature {
            decl_id: callable,
            scope_id: LexicalScopeId(0),
            kind,
            name: name.to_owned(),
            parameters: checked_parameters.clone(),
            result: signature.result.clone(),
            role: self.role,
            effect: signature.effect,
            body: None,
            result_expression: None,
            contextual_operation,
        });
        self.declarations.push(CheckedDeclaration {
            id: callable,
            scope_id: LexicalScopeId(0),
            name: name.to_owned(),
            kind: match kind {
                CheckedCallableKind::Builtin => CheckedDeclarationKind::Builtin,
                CheckedCallableKind::External => CheckedDeclarationKind::External,
                CheckedCallableKind::User => CheckedDeclarationKind::Function,
            },
            flow_type: FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Function {
                    args: checked_parameters
                        .iter()
                        .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                        .map(|parameter| parameter.flow_type.ty.clone())
                        .collect(),
                    result: Box::new(signature.result),
                },
            },
            value: None,
            body_scope: None,
            span: CheckedSpan::default(),
        });
        for parameter in checked_parameters {
            self.declarations.push(CheckedDeclaration {
                id: parameter.decl_id,
                scope_id: LexicalScopeId(0),
                name: parameter.name,
                kind: match parameter.kind {
                    CheckedParameterKind::Value => CheckedDeclarationKind::ValueParameter,
                    CheckedParameterKind::Out => CheckedDeclarationKind::OutParameter,
                },
                flow_type: parameter.flow_type,
                value: None,
                body_scope: None,
                span: CheckedSpan::default(),
            });
        }
    }

    fn visit_expr(&mut self, expr_id: usize, mut available_outputs: BTreeMap<String, DeclId>) {
        if !self.visited_exprs.insert(expr_id) {
            return;
        }
        if let Some(owner) = self.expression_owner.get(&expr_id)
            && let Some(signature) = self.signature(owner)
        {
            for parameter in signature
                .parameters
                .iter()
                .filter(|parameter| parameter.kind == CheckedParameterKind::Out)
            {
                available_outputs.insert(parameter.name.clone(), parameter.decl_id);
            }
        }
        let Some(expr) = self.program.expressions.get(expr_id) else {
            return;
        };
        let (function, raw_pipe_input, args, pass) = match &expr.kind {
            AstExprKind::Call {
                function,
                args,
                pass,
            } => (
                Some(function.as_str()),
                None,
                args.as_slice(),
                pass.as_ref(),
            ),
            AstExprKind::Pipe {
                input,
                op,
                args,
                pass,
                ..
            } if op != "WHILE" => (
                Some(op.as_str()),
                Some(*input),
                args.as_slice(),
                pass.as_ref(),
            ),
            AstExprKind::Pipe {
                input, args, pass, ..
            } => (None, Some(*input), args.as_slice(), pass.as_ref()),
            _ => (None, None, &[][..], None),
        };
        let pipe_input = raw_pipe_input.map(|input| {
            expr.linked_input.unwrap_or_else(|| {
                pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr.id,
                    input,
                    &self.program.expressions,
                )
            })
        });

        let mut nested_outputs = available_outputs.clone();
        if let Some(function) = function
            && let Some(signature) = self.signature(function).cloned()
        {
            let (call, fresh_outputs) = self.bind_call(
                expr,
                &signature,
                pipe_input,
                args,
                pass.map(|pass| pass.value),
                &available_outputs,
            );
            for (name, output) in fresh_outputs {
                nested_outputs.insert(name, output);
            }
            if let Some(call) = call {
                self.calls.push(call);
            }
        }

        if let Some(input) = pipe_input {
            self.visit_expr(input, available_outputs.clone());
        }
        for arg in args {
            if arg.kind == AstCallArgKind::Named {
                self.visit_expr(arg.value, nested_outputs.clone());
            }
        }
        if let Some(pass) = pass {
            self.visit_expr(pass.value, available_outputs.clone());
        }
        for child in direct_expression_children(expr) {
            if raw_pipe_input == Some(child)
                || pipe_input == Some(child)
                || args.iter().any(|arg| arg.value == child)
                || pass.is_some_and(|pass| pass.value == child)
            {
                continue;
            }
            self.visit_expr(child, available_outputs.clone());
        }
    }

    fn bind_call(
        &mut self,
        expr: &AstExpr,
        signature: &CheckedCallableSignature,
        pipe_input: Option<usize>,
        args: &[AstCallArg],
        pass_expr_id: Option<usize>,
        available_outputs: &BTreeMap<String, DeclId>,
    ) -> (Option<CheckedCall>, Vec<(String, DeclId)>) {
        let piped_parameter = pipe_input.and_then(|_| {
            signature
                .parameters
                .iter()
                .find(|parameter| parameter.kind == CheckedParameterKind::Value)
        });
        let expected = signature
            .parameters
            .iter()
            .filter(|parameter| Some(parameter.decl_id) != piped_parameter.map(|item| item.decl_id))
            .collect::<Vec<_>>();
        let mut entries = Vec::new();
        let mut fresh_outputs = Vec::new();
        let mut valid = true;
        if let (Some(input), Some(parameter)) = (pipe_input, piped_parameter) {
            entries.push(CheckedCallEntry::Input {
                formal: parameter.decl_id,
                name: parameter.name.clone(),
                value: CheckedExprId(input as u32),
                from_pipe: true,
                evaluation_scope: parameter.evaluation_scope,
            });
        }
        if pipe_input.is_some() && piped_parameter.is_none() {
            self.call_diagnostic(
                expr,
                format!("`{}` has no ordinary input for the pipe", signature.name),
            );
            valid = false;
        }

        let mut expected_index = 0usize;
        for (call_index, arg) in args.iter().enumerate() {
            while let Some(parameter) = expected.get(expected_index).copied()
                && parameter.name != arg.name
                && self.optional_parameters.contains(&parameter.decl_id)
            {
                expected_index += 1;
            }
            let Some(parameter) = expected.get(expected_index).copied() else {
                self.call_diagnostic(
                    expr,
                    format!(
                        "`{}` has an unexpected extra call entry `{}`",
                        signature.name, arg.name
                    ),
                );
                valid = false;
                continue;
            };
            if arg.name != parameter.name {
                self.call_diagnostic(
                    expr,
                    format!(
                        "`{}` call entry {} must be `{}`, found `{}`; arguments keep declaration names and order",
                        signature.name,
                        call_index + 1,
                        parameter.name,
                        arg.name
                    ),
                );
                valid = false;
                expected_index += 1;
                continue;
            }
            expected_index += 1;
            match (parameter.kind, arg.kind) {
                (CheckedParameterKind::Value, AstCallArgKind::Named) => {
                    entries.push(CheckedCallEntry::Input {
                        formal: parameter.decl_id,
                        name: parameter.name.clone(),
                        value: CheckedExprId(arg.value as u32),
                        from_pipe: false,
                        evaluation_scope: parameter.evaluation_scope,
                    });
                }
                (CheckedParameterKind::Value, AstCallArgKind::BareBinding) => {
                    self.call_diagnostic(
                        expr,
                        format!(
                            "bare `{}` cannot fill ordinary input `{}`; write `{}: expression`",
                            arg.name, parameter.name, parameter.name
                        ),
                    );
                    valid = false;
                }
                (CheckedParameterKind::Out, AstCallArgKind::BareBinding) => {
                    let output = self.allocate_decl();
                    let scope_id = self.allocate_scope();
                    let parent = self
                        .expression_scopes
                        .get(&expr.id)
                        .copied()
                        .unwrap_or(LexicalScopeId(0));
                    self.scopes.push(CheckedScope {
                        id: scope_id,
                        parent: Some(parent),
                        owner: Some(output),
                        kind: CheckedScopeKind::RepeatedOutput,
                        span: checked_expr_span(expr),
                    });
                    self.scope_declarations
                        .insert((scope_id, parameter.name.clone()), output);
                    self.assign_expression_scope_override(arg.value, scope_id);
                    self.declarations.push(CheckedDeclaration {
                        id: output,
                        scope_id: parent,
                        name: parameter.name.clone(),
                        kind: CheckedDeclarationKind::FreshOut,
                        flow_type: parameter.flow_type.clone(),
                        value: None,
                        body_scope: Some(scope_id),
                        span: checked_expr_span(expr),
                    });
                    self.occurrences.push(SemanticOccurrence {
                        target: output,
                        kind: SemanticOccurrenceKind::FreshOut,
                        span: checked_expr_span(expr),
                    });
                    fresh_outputs.push((parameter.name.clone(), output));
                    entries.push(CheckedCallEntry::FreshOut {
                        formal: parameter.decl_id,
                        name: parameter.name.clone(),
                        output,
                        scope_id,
                    });
                }
                (CheckedParameterKind::Out, AstCallArgKind::Named) => {
                    let target_name = expression_single_name(self.program, arg.value);
                    let target = target_name
                        .as_deref()
                        .and_then(|name| available_outputs.get(name).copied());
                    match (target_name, target) {
                        (Some(target_name), Some(target)) => {
                            self.occurrences.push(SemanticOccurrence {
                                target,
                                kind: SemanticOccurrenceKind::ForwardOut,
                                span: CheckedSpan {
                                    line: expr.line,
                                    start: arg.start,
                                    end: arg.end,
                                },
                            });
                            entries.push(CheckedCallEntry::ForwardOut {
                                formal: parameter.decl_id,
                                name: parameter.name.clone(),
                                target,
                                target_name,
                            });
                        }
                        (Some(target_name), None) => {
                            self.call_diagnostic(
                                expr,
                                format!(
                                    "no enclosing OUT named `{target_name}` exists for output parameter `{}`",
                                    parameter.name
                                ),
                            );
                            valid = false;
                        }
                        (None, _) => {
                            self.call_diagnostic(
                                expr,
                                format!(
                                    "output parameter `{}` must be bare for a fresh output or name one existing OUT",
                                    parameter.name
                                ),
                            );
                            valid = false;
                        }
                    }
                }
            }
        }
        for parameter in expected.iter().skip(expected_index) {
            if self.optional_parameters.contains(&parameter.decl_id) {
                continue;
            }
            self.call_diagnostic(
                expr,
                format!(
                    "`{}` is missing call entry `{}`",
                    signature.name, parameter.name
                ),
            );
            valid = false;
        }

        let call = valid.then(|| {
            let id = CheckedCallId(self.next_call_id);
            self.next_call_id += 1;
            self.occurrences.push(SemanticOccurrence {
                target: signature.decl_id,
                kind: SemanticOccurrenceKind::Call,
                span: checked_expr_span(expr),
            });
            if pass_expr_id.is_some() {
                self.occurrences.push(SemanticOccurrence {
                    target: signature.decl_id,
                    kind: SemanticOccurrenceKind::Pass,
                    span: checked_expr_span(expr),
                });
            }
            CheckedCall {
                id,
                expression: CheckedExprId(expr.id as u32),
                callable: signature.decl_id,
                owner_callable: self
                    .expression_owner
                    .get(&expr.id)
                    .and_then(|owner| self.signature(owner))
                    .map(|owner| owner.decl_id),
                function: signature.name.clone(),
                entries,
                pass: pass_expr_id.map(|expr_id| CheckedExprId(expr_id as u32)),
                result: signature.result.clone(),
                role: signature.role,
                span: checked_expr_span(expr),
            }
        });
        (call, fresh_outputs)
    }

    fn apply_inferred_types(
        &mut self,
        expr_type_table: &ExprTypeTable,
        function_type_table: &FunctionTypeTable,
        named_value_type_table: &NamedValueTypeTable,
        external_types: &ExternalTypeEnvironment,
    ) {
        self.inferred_expr_types = expr_type_table
            .entries
            .iter()
            .map(|entry| (entry.expr_id, entry.flow_type.clone()))
            .collect();

        for signature in &mut self.signatures {
            if let Some(external) = external_types.functions.get(&signature.name) {
                signature.result = external.result.clone();
                for (parameter, argument) in signature.parameters.iter_mut().zip(&external.args) {
                    parameter.flow_type = FlowType {
                        mode: FlowMode::Continuous,
                        ty: argument.ty.clone(),
                    };
                }
            } else if let Some(function) = function_type_table
                .entries
                .iter()
                .find(|function| function.name == signature.name)
            {
                signature.result = function.result.clone();
                for parameter in &mut signature.parameters {
                    if let Some(index) = function
                        .args
                        .iter()
                        .position(|name| name == &parameter.name)
                        && let Some(ty) = function.arg_types.get(index)
                    {
                        parameter.flow_type = FlowType {
                            mode: FlowMode::Continuous,
                            ty: ty.clone(),
                        };
                    }
                }
            }
        }

        let calls_by_expression = self
            .calls
            .iter()
            .map(|call| (call.expression.0 as usize, call.callable))
            .collect::<BTreeMap<_, _>>();
        let mut callable_effects = self
            .signatures
            .iter()
            .map(|signature| (signature.decl_id, signature.effect))
            .collect::<BTreeMap<_, _>>();
        loop {
            let mut next = callable_effects.clone();
            for signature in self
                .signatures
                .iter()
                .filter(|signature| signature.kind == CheckedCallableKind::User)
            {
                let effect = self
                    .program
                    .expressions
                    .iter()
                    .filter(|expr| self.expression_owner.get(&expr.id) == Some(&signature.name))
                    .fold(CheckedEffectSummary::default(), |summary, expr| {
                        let summary =
                            merge_checked_effects(summary, checked_expression_effect(expr));
                        calls_by_expression
                            .get(&expr.id)
                            .and_then(|callable| callable_effects.get(callable))
                            .copied()
                            .map_or(summary, |called| merge_checked_effects(summary, called))
                    });
                next.insert(signature.decl_id, effect);
            }
            if next == callable_effects {
                break;
            }
            callable_effects = next;
        }
        for signature in self
            .signatures
            .iter_mut()
            .filter(|signature| signature.kind == CheckedCallableKind::User)
        {
            signature.effect = callable_effects
                .get(&signature.decl_id)
                .copied()
                .unwrap_or_default();
        }

        let parameter_types = self
            .signatures
            .iter()
            .flat_map(|signature| {
                signature
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.decl_id, parameter.flow_type.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let callable_types = self
            .signatures
            .iter()
            .map(|signature| {
                (
                    signature.decl_id,
                    FlowType {
                        mode: FlowMode::Continuous,
                        ty: Type::Function {
                            args: signature
                                .parameters
                                .iter()
                                .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                                .map(|parameter| parameter.flow_type.ty.clone())
                                .collect(),
                            result: Box::new(signature.result.clone()),
                        },
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let named_types = named_value_type_table
            .entries
            .iter()
            .map(|entry| (entry.path.as_str(), &entry.flow_type))
            .collect::<BTreeMap<_, _>>();
        for declaration in &mut self.declarations {
            if let Some(flow_type) = parameter_types
                .get(&declaration.id)
                .or_else(|| callable_types.get(&declaration.id))
            {
                declaration.flow_type = flow_type.clone();
            } else if let Some(value) = declaration.value
                && let Some(flow_type) = self.inferred_expr_types.get(&(value.0 as usize))
            {
                declaration.flow_type = flow_type.clone();
            } else if let Some(flow_type) = named_types.get(declaration.name.as_str()) {
                declaration.flow_type = (*flow_type).clone();
            }
        }
        let call_results = self
            .calls
            .iter()
            .map(|call| {
                (
                    call.expression,
                    self.inferred_expr_types
                        .get(&(call.expression.0 as usize))
                        .cloned()
                        .unwrap_or_else(unknown_flow_type),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for call in &mut self.calls {
            if let Some(result) = call_results.get(&call.expression) {
                call.result = result.clone();
            }
            call.role = self.role;
        }
    }

    fn infer_contextual_callable_schemes(&mut self) {
        self.infer_projected_user_parameter_schemes();
        self.infer_structural_user_result_schemes();

        for _ in 0..self.signatures.len().saturating_add(1) {
            let signatures = self.signatures.clone();
            let calls = self.calls.clone();
            let mut updates = Vec::new();

            for (owner_index, owner) in signatures.iter().enumerate() {
                if owner.kind != CheckedCallableKind::User {
                    continue;
                }
                let Some(result_expression) = owner.result_expression else {
                    continue;
                };
                let Some(call) = calls.iter().find(|call| {
                    call.owner_callable == Some(owner.decl_id)
                        && call.expression == result_expression
                }) else {
                    continue;
                };
                let Some(callee) = signatures
                    .iter()
                    .find(|signature| signature.decl_id == call.callable)
                else {
                    continue;
                };
                if !checked_signature_is_generic(callee) {
                    continue;
                }

                let owner_parameters = owner
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.decl_id, parameter))
                    .collect::<BTreeMap<_, _>>();
                let mut parameter_updates = BTreeMap::<DeclId, FlowType>::new();
                for entry in &call.entries {
                    let formal = match entry {
                        CheckedCallEntry::Input { formal, .. }
                        | CheckedCallEntry::FreshOut { formal, .. }
                        | CheckedCallEntry::ForwardOut { formal, .. } => *formal,
                    };
                    let Some(callee_parameter) = callee
                        .parameters
                        .iter()
                        .find(|parameter| parameter.decl_id == formal)
                    else {
                        continue;
                    };
                    let owner_parameter = match entry {
                        CheckedCallEntry::Input { value, .. } => self
                            .direct_read_declaration(*value)
                            .filter(|declaration| owner_parameters.contains_key(declaration)),
                        CheckedCallEntry::ForwardOut { target, .. }
                            if owner_parameters.contains_key(target) =>
                        {
                            Some(*target)
                        }
                        CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => {
                            None
                        }
                    };
                    if let Some(owner_parameter) = owner_parameter {
                        parameter_updates
                            .insert(owner_parameter, callee_parameter.flow_type.clone());
                    }
                }
                if parameter_updates.is_empty() {
                    continue;
                }
                updates.push((owner_index, parameter_updates, callee.result.clone()));
            }

            let mut changed = false;
            for (owner_index, parameter_updates, result) in updates {
                for parameter in &mut self.signatures[owner_index].parameters {
                    if let Some(flow_type) = parameter_updates.get(&parameter.decl_id)
                        && parameter.flow_type != *flow_type
                    {
                        parameter.flow_type = flow_type.clone();
                        changed = true;
                    }
                }
                if self.signatures[owner_index].result != result {
                    self.signatures[owner_index].result = result;
                    changed = true;
                }
            }
            self.sync_signature_declaration_types();
            if !changed {
                break;
            }
        }
    }

    fn infer_projected_user_parameter_schemes(&mut self) {
        let user_signatures = self
            .signatures
            .iter()
            .enumerate()
            .filter(|(_, signature)| signature.kind == CheckedCallableKind::User)
            .map(|(index, signature)| {
                (
                    index,
                    signature.name.clone(),
                    signature
                        .parameters
                        .iter()
                        .map(|parameter| parameter.decl_id)
                        .collect::<BTreeSet<_>>(),
                )
            })
            .collect::<Vec<_>>();

        let mut next_var = CONTEXTUAL_RESULT_VAR.0 + 1;
        for (signature_index, name, parameters) in user_signatures {
            let mut projections = BTreeMap::<DeclId, Vec<Vec<String>>>::new();
            for expression in self
                .program
                .expressions
                .iter()
                .filter(|expression| self.expression_owner.get(&expression.id) == Some(&name))
            {
                let Some(parts) = checked_read_path_parts(expression) else {
                    continue;
                };
                if parts.len() < 2 {
                    continue;
                }
                let scope_id = self
                    .expression_scopes
                    .get(&expression.id)
                    .copied()
                    .unwrap_or(LexicalScopeId(0));
                let Some(parameter) = self.resolve_name(scope_id, &parts[0]) else {
                    continue;
                };
                if parameters.contains(&parameter) {
                    let paths = projections.entry(parameter).or_default();
                    let projection = parts[1..].to_vec();
                    if !paths.contains(&projection) {
                        paths.push(projection);
                    }
                }
            }

            for parameter in &mut self.signatures[signature_index].parameters {
                let Some(paths) = projections.get(&parameter.decl_id) else {
                    continue;
                };
                parameter.flow_type.ty = projected_object_scheme(paths, &mut next_var);
            }
        }
        self.sync_signature_declaration_types();
    }

    fn infer_structural_user_result_schemes(&mut self) {
        let roots = self.program.ast.statements.clone();
        for statement in &roots {
            self.infer_structural_statement_value(statement);
        }

        let user_bodies = self
            .signatures
            .iter()
            .enumerate()
            .filter(|(_, signature)| signature.kind == CheckedCallableKind::User)
            .filter_map(|(index, signature)| {
                signature
                    .body
                    .map(|body| (index, body, signature.name.clone()))
            })
            .collect::<Vec<_>>();

        for (signature_index, body, _) in user_bodies {
            let Some(statement) =
                statement_by_id(&self.program.ast.statements, body.0 as usize).cloned()
            else {
                continue;
            };
            let Some(result) = self.infer_structural_statement_value(&statement) else {
                continue;
            };
            if !result.structural_record
                || matches!(
                    result.flow_type.ty,
                    Type::Unknown | Type::UnresolvedShape { .. }
                )
            {
                continue;
            }
            self.signatures[signature_index].result = result.flow_type;
        }
        self.sync_signature_declaration_types();
    }

    fn infer_structural_statement_value(
        &mut self,
        statement: &AstStatement,
    ) -> Option<InferredStructuralValue> {
        let child_values = statement
            .children
            .iter()
            .map(|child| self.infer_structural_statement_value(child))
            .collect::<Vec<_>>();
        let expression_value = self
            .statement_declarations
            .get(&statement.id)
            .and_then(|declaration| {
                self.declarations
                    .iter()
                    .find(|candidate| candidate.id == *declaration)
                    .and_then(|declaration| declaration.value)
            })
            .or_else(|| {
                canonical_statement_value_expression(
                    &statement.children,
                    statement,
                    &self.program.expressions,
                )
                .or(statement.expr)
                .map(|expression| CheckedExprId(expression as u32))
            })
            .map(|expression| {
                let flow_type =
                    self.infer_checked_expr_flow(expression.0 as usize, &mut BTreeSet::new());
                self.set_inferred_expr_flow(expression.0 as usize, flow_type.clone());
                let structural_record = self
                    .program
                    .expressions
                    .get(expression.0 as usize)
                    .is_some_and(|expression| {
                        matches!(
                            expression.kind,
                            AstExprKind::Record(_) | AstExprKind::Object(_)
                        )
                    });
                InferredStructuralValue {
                    flow_type,
                    structural_record,
                }
            });

        let value = if matches!(statement.kind, AstStatementKind::Function { .. }) {
            child_values.into_iter().rev().flatten().next()
        } else {
            expression_value.or_else(|| child_values.into_iter().rev().flatten().next())
        };

        if let Some(value) = &value
            && let Some(declaration) = self.statement_declarations.get(&statement.id).copied()
        {
            self.set_declaration_flow_type(declaration, value.flow_type.clone());
        }
        value
    }

    fn direct_read_declaration(&self, expression: CheckedExprId) -> Option<DeclId> {
        let expr_id = expression.0 as usize;
        let name = expression_single_name(self.program, expr_id)?;
        let scope_id = self
            .expression_scopes
            .get(&expr_id)
            .copied()
            .unwrap_or(LexicalScopeId(0));
        self.resolve_name(scope_id, &name)
    }

    fn sync_signature_declaration_types(&mut self) {
        let parameter_types = self
            .signatures
            .iter()
            .flat_map(|signature| {
                signature
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.decl_id, parameter.flow_type.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let callable_types = self
            .signatures
            .iter()
            .map(|signature| {
                (
                    signature.decl_id,
                    FlowType {
                        mode: FlowMode::Continuous,
                        ty: Type::Function {
                            args: signature
                                .parameters
                                .iter()
                                .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                                .map(|parameter| parameter.flow_type.ty.clone())
                                .collect(),
                            result: Box::new(signature.result.clone()),
                        },
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        for declaration in &mut self.declarations {
            if let Some(flow_type) = parameter_types
                .get(&declaration.id)
                .or_else(|| callable_types.get(&declaration.id))
            {
                declaration.flow_type = flow_type.clone();
            }
        }
    }

    fn infer_checked_types(&mut self) {
        let iteration_limit = self
            .calls
            .len()
            .saturating_add(self.signatures.len())
            .saturating_add(4);
        for _ in 0..iteration_limit {
            let mut changed = self.refresh_checked_callable_result_types();
            changed |= self.refresh_checked_declaration_types();
            let call_ids = self.calls.iter().map(|call| call.id).collect::<Vec<_>>();
            for call_id in call_ids {
                changed |= self.instantiate_checked_call(call_id);
            }
            changed |= self.refresh_pattern_binding_types();
            changed |= self.refresh_checked_expression_types();
            if !changed {
                return;
            }
        }
        self.diagnostics.push(TypeDiagnostic {
            severity: DiagnosticSeverity::Error,
            line: 0,
            start: 0,
            end: 0,
            message: "checked contextual type inference did not converge".to_owned(),
        });
    }

    fn refresh_checked_callable_result_types(&mut self) -> bool {
        let callables = self
            .signatures
            .iter()
            .enumerate()
            .filter(|(_, signature)| signature.kind == CheckedCallableKind::User)
            .filter_map(|(index, signature)| {
                signature
                    .result_expression
                    .map(|expression| (index, expression))
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for (index, expression) in callables {
            let result = self.infer_checked_expr_flow(expression.0 as usize, &mut BTreeSet::new());
            if matches!(result.ty, Type::Unknown | Type::UnresolvedShape { .. }) {
                continue;
            }
            if self.signatures[index].result != result {
                self.signatures[index].result = result;
                changed = true;
            }
        }
        if changed {
            self.sync_signature_declaration_types();
        }
        changed
    }

    fn refresh_checked_declaration_types(&mut self) -> bool {
        let values = self
            .declarations
            .iter()
            .filter_map(|declaration| declaration.value.map(|value| (declaration.id, value)))
            .collect::<Vec<_>>();
        let mut changed = false;
        for (declaration, value) in values {
            let flow_type = self.infer_checked_expr_flow(value.0 as usize, &mut BTreeSet::new());
            if matches!(flow_type.ty, Type::Unknown | Type::UnresolvedShape { .. }) {
                continue;
            }
            if let Some(target) = self
                .declarations
                .iter_mut()
                .find(|candidate| candidate.id == declaration)
                && target.flow_type != flow_type
            {
                target.flow_type = flow_type;
                changed = true;
            }
        }
        changed
    }

    fn specialize_consistent_user_body_types(&mut self) {
        let user_signatures = self
            .signatures
            .iter()
            .filter(|signature| {
                signature.kind == CheckedCallableKind::User
                    && checked_signature_is_generic(signature)
            })
            .cloned()
            .collect::<Vec<_>>();

        for signature in user_signatures {
            let calls = self
                .calls
                .iter()
                .filter(|call| call.callable == signature.decl_id)
                .cloned()
                .collect::<Vec<_>>();
            if calls.is_empty() {
                continue;
            }
            let mut call_substitutions = Vec::with_capacity(calls.len());
            for call in calls {
                let mut substitutions = BTreeMap::<TypeVar, Type>::new();
                for entry in &call.entries {
                    let (formal, actual) = match entry {
                        CheckedCallEntry::Input { formal, value, .. } => (
                            *formal,
                            self.infer_checked_expr_flow(value.0 as usize, &mut BTreeSet::new())
                                .ty,
                        ),
                        CheckedCallEntry::FreshOut { formal, output, .. } => (
                            *formal,
                            self.declarations
                                .iter()
                                .find(|declaration| declaration.id == *output)
                                .map(|declaration| declaration.flow_type.ty.clone())
                                .unwrap_or(Type::Unknown),
                        ),
                        CheckedCallEntry::ForwardOut { formal, target, .. } => (
                            *formal,
                            self.declarations
                                .iter()
                                .find(|declaration| declaration.id == *target)
                                .map(|declaration| declaration.flow_type.ty.clone())
                                .unwrap_or(Type::Unknown),
                        ),
                    };
                    if let Some(parameter) = signature
                        .parameters
                        .iter()
                        .find(|parameter| parameter.decl_id == formal)
                    {
                        unify_checked_type_pattern(
                            &parameter.flow_type.ty,
                            &actual,
                            &mut substitutions,
                        );
                    }
                }
                call_substitutions.push(substitutions);
            }

            let mut vars = BTreeSet::new();
            for parameter in &signature.parameters {
                collect_type_vars(&parameter.flow_type.ty, &mut vars);
            }
            collect_type_vars(&signature.result.ty, &mut vars);
            let substitutions = vars
                .into_iter()
                .filter_map(|var| {
                    let replacements = call_substitutions
                        .iter()
                        .map(|substitutions| substitutions.get(&var))
                        .collect::<Option<Vec<_>>>()?;
                    let first = replacements.first()?.to_owned();
                    (replacements.iter().all(|replacement| *replacement == first)
                        && !is_value_placeholder_type(first)
                        && !checked_type_contains_var(first))
                    .then(|| (var, first.clone()))
                })
                .collect::<BTreeMap<_, _>>();
            if substitutions.is_empty() {
                continue;
            }

            for (expr_id, flow_type) in &mut self.inferred_expr_types {
                if self.expression_owner.get(expr_id) == Some(&signature.name) {
                    flow_type.ty = substitute_checked_type(&flow_type.ty, &substitutions);
                }
            }
        }
    }

    fn instantiate_checked_call(&mut self, call_id: CheckedCallId) -> bool {
        let Some(call) = self.calls.iter().find(|call| call.id == call_id).cloned() else {
            return false;
        };
        let Some(signature) = self
            .signatures
            .iter()
            .find(|signature| signature.decl_id == call.callable)
            .cloned()
        else {
            return false;
        };
        let mut substitutions = BTreeMap::<TypeVar, Type>::new();

        for entry in &call.entries {
            let CheckedCallEntry::Input {
                formal,
                value,
                evaluation_scope: CheckedEvaluationScope::Parent,
                ..
            } = entry
            else {
                continue;
            };
            let Some(parameter) = signature
                .parameters
                .iter()
                .find(|parameter| parameter.decl_id == *formal)
            else {
                continue;
            };
            let actual = self
                .infer_checked_expr_flow(value.0 as usize, &mut BTreeSet::new())
                .ty;
            unify_checked_type_pattern(&parameter.flow_type.ty, &actual, &mut substitutions);
        }

        let mut changed = false;
        for entry in &call.entries {
            let (formal, output) = match entry {
                CheckedCallEntry::FreshOut { formal, output, .. } => (*formal, *output),
                CheckedCallEntry::ForwardOut { formal, target, .. } => (*formal, *target),
                CheckedCallEntry::Input { .. } => continue,
            };
            let Some(parameter) = signature
                .parameters
                .iter()
                .find(|parameter| parameter.decl_id == formal)
            else {
                continue;
            };
            if let Some(existing) = self
                .declarations
                .iter()
                .find(|declaration| declaration.id == output)
                .map(|declaration| declaration.flow_type.ty.clone())
            {
                unify_checked_type_pattern(&parameter.flow_type.ty, &existing, &mut substitutions);
            }
            let flow_type = FlowType {
                mode: parameter.flow_type.mode,
                ty: substitute_checked_type(&parameter.flow_type.ty, &substitutions),
            };
            changed |= self.set_declaration_flow_type(output, flow_type);
        }

        for entry in &call.entries {
            let CheckedCallEntry::Input {
                formal,
                value,
                evaluation_scope: CheckedEvaluationScope::Output { .. },
                name,
                ..
            } = entry
            else {
                continue;
            };
            let Some(parameter) = signature
                .parameters
                .iter()
                .find(|parameter| parameter.decl_id == *formal)
            else {
                continue;
            };
            let actual = self
                .infer_checked_expr_flow(value.0 as usize, &mut BTreeSet::new())
                .ty;
            unify_checked_type_pattern(&parameter.flow_type.ty, &actual, &mut substitutions);
            let expected = substitute_checked_type(&parameter.flow_type.ty, &substitutions);
            if !type_is_assignable_to(&actual, &expected) {
                self.contextual_type_diagnostic(
                    value.0 as usize,
                    format!(
                        "`{}` argument `{name}` has incompatible contextual type\nexpected: {}\nfound: {}",
                        signature.name,
                        boon_facing_type_label(&expected),
                        boon_facing_type_label(&actual)
                    ),
                );
            }
        }

        for entry in &call.entries {
            let (formal, output) = match entry {
                CheckedCallEntry::FreshOut { formal, output, .. } => (*formal, *output),
                CheckedCallEntry::ForwardOut { formal, target, .. } => (*formal, *target),
                CheckedCallEntry::Input { .. } => continue,
            };
            let Some(parameter) = signature
                .parameters
                .iter()
                .find(|parameter| parameter.decl_id == formal)
            else {
                continue;
            };
            changed |= self.set_declaration_flow_type(
                output,
                FlowType {
                    mode: parameter.flow_type.mode,
                    ty: substitute_checked_type(&parameter.flow_type.ty, &substitutions),
                },
            );
        }

        let result = FlowType {
            mode: signature.result.mode,
            ty: substitute_checked_type(&signature.result.ty, &substitutions),
        };
        if let Some(target) = self.calls.iter_mut().find(|call| call.id == call_id)
            && target.result != result
        {
            target.result = result.clone();
            changed = true;
        }
        changed |= self.set_inferred_expr_flow(call.expression.0 as usize, result);
        changed
    }

    fn refresh_pattern_binding_types(&mut self) -> bool {
        let bindings = self.pattern_declarations.clone();
        let selectors = self.pattern_selectors.clone();
        let mut changed = false;
        for ((arm_expr_id, name), declaration) in bindings {
            let Some(selector_expr_id) = selectors.get(&arm_expr_id).copied() else {
                continue;
            };
            let selector = self
                .infer_checked_expr_flow(selector_expr_id, &mut BTreeSet::new())
                .ty;
            let Some(AstExpr {
                kind: AstExprKind::MatchArm { pattern, .. },
                ..
            }) = self.program.expressions.get(arm_expr_id)
            else {
                continue;
            };
            let ty = if let Some(Variant::Tag(tag)) = pattern_variant(pattern) {
                let Some(ty) = tagged_variant_field_type(&selector, &tag, &name) else {
                    continue;
                };
                ty
            } else if pattern_variable_names(pattern).as_slice() == [name.as_str()] {
                selector
            } else {
                continue;
            };
            changed |= self.set_declaration_flow_type(declaration, continuous_flow_type(ty));
        }
        changed
    }

    fn refresh_checked_expression_types(&mut self) -> bool {
        let expression_ids = self
            .program
            .expressions
            .iter()
            .map(|expr| expr.id)
            .collect::<Vec<_>>();
        let mut changed = false;
        for expr_id in expression_ids {
            let flow_type = self.infer_checked_expr_flow(expr_id, &mut BTreeSet::new());
            if matches!(flow_type.ty, Type::Unknown | Type::UnresolvedShape { .. }) {
                continue;
            }
            changed |= self.set_inferred_expr_flow(expr_id, flow_type);
        }
        changed
    }

    fn infer_checked_expr_flow(
        &mut self,
        expr_id: usize,
        active: &mut BTreeSet<usize>,
    ) -> FlowType {
        let fallback = self
            .inferred_expr_types
            .get(&expr_id)
            .cloned()
            .unwrap_or_else(unknown_flow_type);
        if !active.insert(expr_id) {
            return fallback;
        }
        let Some(expr) = self.program.expressions.get(expr_id).cloned() else {
            active.remove(&expr_id);
            return fallback;
        };
        let ty = match expr.kind {
            AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Type::Text,
            AstExprKind::Number(_) => Type::Number,
            AstExprKind::ByteLiteral { .. } => Type::Bytes(BytesType::Fixed(1)),
            AstExprKind::BytesLiteral { size, .. } => Type::Bytes(match size {
                BytesSizeSyntax::Fixed(size) => BytesType::Fixed(size),
                BytesSizeSyntax::Dynamic | BytesSizeSyntax::Infer => BytesType::Dynamic,
            }),
            AstExprKind::Bool(value) => Type::VariantSet(vec![Variant::Tag(if value {
                "True".to_owned()
            } else {
                "False".to_owned()
            })]),
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) if tag == "SKIP" => Type::Skip,
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) => {
                Type::VariantSet(vec![Variant::Tag(tag)])
            }
            AstExprKind::TaggedObject { tag, fields } => Type::VariantSet(vec![Variant::Tagged {
                tag,
                fields: ObjectShape::from_ordered_fields(
                    fields
                        .into_iter()
                        .filter(|field| !field.spread)
                        .map(|field| {
                            (
                                field.name,
                                self.infer_checked_expr_flow(field.value, active).ty,
                            )
                        }),
                    false,
                ),
            }]),
            AstExprKind::Record(fields) | AstExprKind::Object(fields) => {
                Type::Object(ObjectShape::from_ordered_fields(
                    fields
                        .into_iter()
                        .filter(|field| !field.spread)
                        .map(|field| {
                            (
                                field.name,
                                self.infer_checked_expr_flow(field.value, active).ty,
                            )
                        }),
                    false,
                ))
            }
            AstExprKind::ListLiteral { items, .. } => {
                let fallback_item = match &fallback.ty {
                    Type::List(item) => Some((**item).clone()),
                    _ => None,
                };
                Type::List(Box::new(
                    items
                        .into_iter()
                        .map(|item| self.infer_checked_expr_flow(item, active).ty)
                        .reduce(|existing, extra| widen_structural_type(&existing, &extra))
                        .or(fallback_item)
                        .unwrap_or_else(open_object_type),
                ))
            }
            AstExprKind::Identifier(name) => {
                self.checked_read_type(expr_id, &[name], active, fallback.ty.clone())
            }
            AstExprKind::Path(parts) => {
                self.checked_read_type(expr_id, &parts, active, fallback.ty.clone())
            }
            AstExprKind::Drain { path } => self.checked_read_type(
                expr_id,
                &drain_path_parts(&path),
                active,
                fallback.ty.clone(),
            ),
            AstExprKind::Pipe {
                input, op, arms, ..
            } if op == "WHILE" => {
                let selector = expr.linked_input.unwrap_or_else(|| {
                    pipeline_source_expr_id(
                        &self.program.ast.statements,
                        expr_id,
                        input,
                        &self.program.expressions,
                    )
                });
                self.infer_checked_expr_flow(selector, active);
                arms.into_iter()
                    .map(|arm| self.infer_checked_expr_flow(arm, active).ty)
                    .reduce(|existing, extra| widen_structural_type(&existing, &extra))
                    .unwrap_or(fallback.ty.clone())
            }
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                let function = ast_callable_name(&expr).unwrap_or_default();
                if let AstExprKind::Pipe { input, op, .. } = &expr.kind
                    && let Some(field) = op.strip_prefix("Field/")
                {
                    let input = pipeline_source_expr_id(
                        &self.program.ast.statements,
                        expr_id,
                        *input,
                        &self.program.expressions,
                    );
                    let base = self.infer_checked_expr_flow(input, active).ty;
                    self.project_checked_type(expr_id, base, &[field.to_owned()])
                } else {
                    self.calls
                        .iter()
                        .find(|call| call.expression == CheckedExprId(expr_id as u32))
                        .map(|call| call.result.ty.clone())
                        .or_else(|| {
                            self.signature(function)
                                .map(|signature| signature.result.ty.clone())
                        })
                        .unwrap_or(fallback.ty.clone())
                }
            }
            AstExprKind::Infix { left, op, right } => {
                self.infer_checked_expr_flow(left, active);
                self.infer_checked_expr_flow(right, active);
                if matches!(op.as_str(), "==" | "!=" | ">" | "<" | ">=" | "<=") {
                    true_false_type()
                } else {
                    Type::Number
                }
            }
            AstExprKind::MatchArm { output, .. } => output
                .map(|output| self.infer_checked_expr_flow(output, active).ty)
                .unwrap_or(Type::Skip),
            AstExprKind::When { input, arms } => {
                let selector = expr.linked_input.unwrap_or_else(|| {
                    pipeline_source_expr_id(
                        &self.program.ast.statements,
                        expr_id,
                        input,
                        &self.program.expressions,
                    )
                });
                self.infer_checked_expr_flow(selector, active);
                arms.into_iter()
                    .map(|arm| self.infer_checked_expr_flow(arm, active).ty)
                    .reduce(|existing, extra| widen_structural_type(&existing, &extra))
                    .unwrap_or(fallback.ty.clone())
            }
            AstExprKind::Block { result, .. } => result
                .map(|result| self.infer_checked_expr_flow(result, active).ty)
                .unwrap_or(Type::Skip),
            AstExprKind::Then { input, output } => output
                .map(|output| self.infer_checked_expr_flow(output, active).ty)
                .unwrap_or_else(|| self.infer_checked_expr_flow(input, active).ty),
            AstExprKind::Hold { initial, .. } => {
                let initial = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr_id,
                    initial,
                    &self.program.expressions,
                );
                let mut ty = self.infer_checked_expr_flow(initial, active).ty;
                for update in hold_update_exprs_for_expr(
                    &self.program.ast.statements,
                    expr_id,
                    &self.program.expressions,
                ) {
                    let update = self.infer_checked_expr_flow(update, active).ty;
                    if !matches!(update, Type::Skip) {
                        ty = widen_checked_hold_type(&ty, &update);
                    }
                }
                ty
            }
            AstExprKind::Draining { input } => {
                let input = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr_id,
                    input,
                    &self.program.expressions,
                );
                self.infer_checked_expr_flow(input, active).ty
            }
            AstExprKind::Latest => fallback.ty.clone(),
            AstExprKind::Source => exact_empty_object_type(),
            AstExprKind::Delimiter => Type::List(Box::new(open_object_type())),
            AstExprKind::Unknown(_) => fallback.ty.clone(),
        };
        active.remove(&expr_id);
        FlowType {
            mode: fallback.mode,
            ty,
        }
    }

    fn checked_read_type(
        &mut self,
        expr_id: usize,
        parts: &[String],
        _active: &mut BTreeSet<usize>,
        fallback: Type,
    ) -> Type {
        let Some(name) = parts.first() else {
            return fallback;
        };
        let scope_id = self
            .expression_scopes
            .get(&expr_id)
            .copied()
            .unwrap_or(LexicalScopeId(0));
        let Some(declaration) = self.resolve_checked_read_name(expr_id, scope_id, name) else {
            return fallback;
        };
        let base = self
            .declarations
            .iter()
            .find(|candidate| candidate.id == declaration)
            .map(|declaration| declaration.flow_type.ty.clone())
            .unwrap_or(Type::Unknown);
        self.project_checked_type(expr_id, base, &parts[1..])
    }

    fn project_checked_type(&mut self, expr_id: usize, mut ty: Type, fields: &[String]) -> Type {
        for field in fields {
            ty = match ty {
                Type::Object(shape) => match shape.fields.get(field).cloned() {
                    Some(field_type) => field_type,
                    None => Type::Unknown,
                },
                Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. } => Type::Unknown,
                other @ (Type::Text
                | Type::Number
                | Type::Bytes(_)
                | Type::Skip
                | Type::List(_)) => {
                    self.contextual_type_diagnostic(
                        expr_id,
                        format!(
                            "cannot project field `{field}` from {}",
                            boon_facing_type_label(&other)
                        ),
                    );
                    Type::Unknown
                }
                Type::VariantSet(_) | Type::Function { .. } | Type::RenderContract => Type::Unknown,
            };
        }
        ty
    }

    fn set_declaration_flow_type(&mut self, declaration: DeclId, flow_type: FlowType) -> bool {
        let Some(target) = self
            .declarations
            .iter_mut()
            .find(|candidate| candidate.id == declaration)
        else {
            return false;
        };
        if target.flow_type == flow_type {
            return false;
        }
        target.flow_type = flow_type;
        true
    }

    fn set_inferred_expr_flow(&mut self, expr_id: usize, flow_type: FlowType) -> bool {
        match self.inferred_expr_types.get(&expr_id) {
            Some(existing) if existing == &flow_type => false,
            _ => {
                self.inferred_expr_types.insert(expr_id, flow_type);
                true
            }
        }
    }

    fn contextual_type_diagnostic(&mut self, expr_id: usize, message: String) {
        let Some(expr) = self.program.expressions.get(expr_id) else {
            return;
        };
        if self.diagnostics.iter().any(|diagnostic| {
            diagnostic.start == expr.start
                && diagnostic.end == expr.end
                && diagnostic.message == message
        }) {
            return;
        }
        self.diagnostics.push(TypeDiagnostic {
            severity: DiagnosticSeverity::Error,
            line: expr.line,
            start: expr.start,
            end: expr.end,
            message,
        });
    }

    fn infer_contextual_scope_effects(&mut self) {
        let signature_index = self
            .signatures
            .iter()
            .enumerate()
            .map(|(index, signature)| (signature.decl_id, index))
            .collect::<BTreeMap<_, _>>();

        for _ in 0..self.signatures.len().saturating_add(1) {
            let signatures = self.signatures.clone();
            let mut inferred = BTreeMap::<DeclId, CheckedEvaluationScope>::new();
            let mut conflicts = BTreeSet::<DeclId>::new();
            for call in &self.calls {
                let Some(owner_id) = call.owner_callable else {
                    continue;
                };
                let Some(owner) = signatures
                    .iter()
                    .find(|signature| signature.decl_id == owner_id)
                else {
                    continue;
                };
                let Some(callee) = signatures
                    .iter()
                    .find(|signature| signature.decl_id == call.callable)
                else {
                    continue;
                };
                let owner_values = owner
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                    .map(|parameter| (parameter.name.as_str(), parameter.decl_id))
                    .collect::<BTreeMap<_, _>>();
                let owner_outputs = owner
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.kind == CheckedParameterKind::Out)
                    .map(|parameter| parameter.decl_id)
                    .collect::<BTreeSet<_>>();
                for entry in &call.entries {
                    let CheckedCallEntry::Input { formal, value, .. } = entry else {
                        continue;
                    };
                    let Some(callee_parameter) = callee
                        .parameters
                        .iter()
                        .find(|parameter| parameter.decl_id == *formal)
                    else {
                        continue;
                    };
                    let CheckedEvaluationScope::Output {
                        formal: output_formal,
                    } = callee_parameter.evaluation_scope
                    else {
                        continue;
                    };
                    let target = call.entries.iter().find_map(|entry| match entry {
                        CheckedCallEntry::ForwardOut { formal, target, .. }
                            if *formal == output_formal && owner_outputs.contains(target) =>
                        {
                            Some(*target)
                        }
                        _ => None,
                    });
                    let Some(target) = target else {
                        continue;
                    };
                    for parameter in self.referenced_owner_parameters(*value, &owner_values) {
                        match inferred
                            .insert(parameter, CheckedEvaluationScope::Output { formal: target })
                        {
                            Some(CheckedEvaluationScope::Output { formal }) if formal != target => {
                                conflicts.insert(parameter);
                            }
                            _ => {}
                        }
                    }
                }
            }
            if !conflicts.is_empty() {
                for parameter in conflicts {
                    if let Some(declaration) = self
                        .declarations
                        .iter()
                        .find(|declaration| declaration.id == parameter)
                    {
                        self.diagnostics.push(TypeDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            line: declaration.span.line,
                            start: declaration.span.start,
                            end: declaration.span.end,
                            message: format!(
                                "parameter `{}` requires incompatible OUT evaluation scopes",
                                declaration.name
                            ),
                        });
                    }
                }
                return;
            }

            let mut changed = false;
            for (parameter, evaluation_scope) in inferred {
                let Some((signature_index_value, parameter_index)) = signatures
                    .iter()
                    .enumerate()
                    .find_map(|(signature_index_value, signature)| {
                        signature
                            .parameters
                            .iter()
                            .position(|candidate| candidate.decl_id == parameter)
                            .map(|parameter_index| (signature_index_value, parameter_index))
                    })
                else {
                    continue;
                };
                let current = self.signatures[signature_index_value].parameters[parameter_index]
                    .evaluation_scope;
                if current != evaluation_scope {
                    self.signatures[signature_index_value].parameters[parameter_index]
                        .evaluation_scope = evaluation_scope;
                    changed = true;
                }
            }
            self.refresh_call_evaluation_scopes(&signature_index);
            if !changed {
                self.apply_call_evaluation_scopes();
                return;
            }
        }
        self.diagnostics.push(TypeDiagnostic {
            severity: DiagnosticSeverity::Error,
            line: 0,
            start: 0,
            end: 0,
            message: "contextual OUT scope inference did not converge".to_owned(),
        });
    }

    fn apply_call_evaluation_scopes(&mut self) {
        let calls = self.calls.clone();
        for call in calls {
            for entry in &call.entries {
                let CheckedCallEntry::Input {
                    value,
                    evaluation_scope: CheckedEvaluationScope::Output { formal },
                    ..
                } = entry
                else {
                    continue;
                };
                let scope_id = call.entries.iter().find_map(|binding| match binding {
                    CheckedCallEntry::FreshOut {
                        formal: output_formal,
                        scope_id,
                        ..
                    } if output_formal == formal => Some(*scope_id),
                    CheckedCallEntry::ForwardOut {
                        formal: output_formal,
                        target,
                        ..
                    } if output_formal == formal => self
                        .declarations
                        .iter()
                        .find(|declaration| declaration.id == *target)
                        .and_then(|declaration| declaration.body_scope),
                    _ => None,
                });
                if let Some(scope_id) = scope_id {
                    self.assign_expression_scope_override(value.0 as usize, scope_id);
                }
            }
        }
    }

    fn assign_expression_scope_override(&mut self, expr_id: usize, scope_id: LexicalScopeId) {
        self.expression_scopes.insert(expr_id, scope_id);
        let Some(expr) = self.program.expressions.get(expr_id) else {
            return;
        };
        for child in direct_expression_children(expr) {
            self.assign_expression_scope_override(child, scope_id);
        }
    }

    fn refresh_call_evaluation_scopes(&mut self, signature_index: &BTreeMap<DeclId, usize>) {
        for call in &mut self.calls {
            let Some(signature) = signature_index
                .get(&call.callable)
                .and_then(|index| self.signatures.get(*index))
            else {
                continue;
            };
            for entry in &mut call.entries {
                let CheckedCallEntry::Input {
                    formal,
                    evaluation_scope,
                    ..
                } = entry
                else {
                    continue;
                };
                if let Some(parameter) = signature
                    .parameters
                    .iter()
                    .find(|parameter| parameter.decl_id == *formal)
                {
                    *evaluation_scope = parameter.evaluation_scope;
                }
            }
        }
    }

    fn referenced_owner_parameters(
        &self,
        expression: CheckedExprId,
        parameters: &BTreeMap<&str, DeclId>,
    ) -> BTreeSet<DeclId> {
        fn visit(
            program: &ParsedProgram,
            expr_id: usize,
            parameters: &BTreeMap<&str, DeclId>,
            visited: &mut BTreeSet<usize>,
            result: &mut BTreeSet<DeclId>,
        ) {
            if !visited.insert(expr_id) {
                return;
            }
            let Some(expr) = program.expressions.get(expr_id) else {
                return;
            };
            match &expr.kind {
                AstExprKind::Identifier(name) => {
                    if let Some(parameter) = parameters.get(name.as_str()) {
                        result.insert(*parameter);
                    }
                }
                AstExprKind::Path(parts) => {
                    if let Some(parameter) =
                        parts.first().and_then(|name| parameters.get(name.as_str()))
                    {
                        result.insert(*parameter);
                    }
                }
                _ => {}
            }
            for child in direct_expression_children(expr) {
                visit(program, child, parameters, visited, result);
            }
        }

        let mut result = BTreeSet::new();
        visit(
            self.program,
            expression.0 as usize,
            parameters,
            &mut BTreeSet::new(),
            &mut result,
        );
        result
    }

    fn predeclare_statement_tree(&mut self, statements: &[AstStatement], scope_id: LexicalScopeId) {
        for statement in statements {
            self.statement_scopes.insert(statement.id, scope_id);
            match &statement.kind {
                AstStatementKind::Function { name, .. } => {
                    if let Some(signature_index) = self.signature_by_name.get(name).copied() {
                        let declaration = self.signatures[signature_index].decl_id;
                        let body_scope = self.signatures[signature_index].scope_id;
                        self.statement_declarations
                            .insert(statement.id, declaration);
                        if let Some(scope) =
                            self.scopes.iter_mut().find(|scope| scope.id == body_scope)
                        {
                            scope.parent = Some(scope_id);
                        }
                        if let Some(declaration_entry) = self
                            .declarations
                            .iter_mut()
                            .find(|entry| entry.id == declaration)
                        {
                            declaration_entry.scope_id = scope_id;
                        }
                        self.scope_declarations
                            .insert((scope_id, name.clone()), declaration);
                    }
                }
                AstStatementKind::Field { name }
                | AstStatementKind::Source {
                    field: Some(name), ..
                }
                | AstStatementKind::Hold {
                    field: Some(name), ..
                }
                | AstStatementKind::List {
                    field: Some(name), ..
                } => {
                    let declaration = self.allocate_decl();
                    let kind = match statement.kind {
                        AstStatementKind::Field { .. } => CheckedDeclarationKind::Field,
                        AstStatementKind::Source { .. } => CheckedDeclarationKind::Source,
                        AstStatementKind::Hold { .. } => CheckedDeclarationKind::Hold,
                        AstStatementKind::List { .. } => CheckedDeclarationKind::List,
                        _ => unreachable!(),
                    };
                    if let Some(previous) = self
                        .scope_declarations
                        .insert((scope_id, name.clone()), declaration)
                    {
                        self.diagnostics.push(TypeDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            line: statement.line,
                            start: statement.start,
                            end: statement.end,
                            message: format!(
                                "declaration `{name}` conflicts with declaration {} in the same lexical scope",
                                previous.0
                            ),
                        });
                    }
                    self.statement_declarations
                        .insert(statement.id, declaration);
                    self.declarations.push(CheckedDeclaration {
                        id: declaration,
                        scope_id,
                        name: name.clone(),
                        kind,
                        flow_type: unknown_flow_type(),
                        value: canonical_statement_value_expression(
                            &statement.children,
                            statement,
                            &self.program.expressions,
                        )
                        .or_else(|| {
                            canonical_block_value_expression(
                                &statement.children,
                                &self.program.expressions,
                            )
                        })
                        .or(statement.expr)
                        .map(|expr| CheckedExprId(expr as u32)),
                        body_scope: None,
                        span: checked_statement_span(statement),
                    });
                    self.occurrences.push(SemanticOccurrence {
                        target: declaration,
                        kind: SemanticOccurrenceKind::Declaration,
                        span: checked_statement_span(statement),
                    });
                }
                AstStatementKind::Source { field: None, .. }
                | AstStatementKind::Hold { field: None, .. }
                | AstStatementKind::List { field: None, .. }
                | AstStatementKind::Block
                | AstStatementKind::Spread
                | AstStatementKind::Expression => {}
            }
        }

        for statement in statements {
            if let Some(expr_id) = statement.expr {
                self.assign_expression_scope(expr_id, scope_id);
            }
            if let Some(declaration) = self.statement_declarations.get(&statement.id).copied() {
                let mut expression_ids = BTreeSet::new();
                collect_statement_expression_tree_ids(
                    statement,
                    &self.program.expressions,
                    &mut expression_ids,
                );
                for expression_id in expression_ids {
                    self.expression_declarations
                        .insert(expression_id, declaration);
                }
            }
            if statement.children.is_empty() {
                continue;
            }
            let child_scope = if let AstStatementKind::Function { name, .. } = &statement.kind {
                self.signature(name)
                    .map_or(scope_id, |signature| signature.scope_id)
            } else {
                let child_scope = self.allocate_scope();
                let kind = statement
                    .expr
                    .and_then(|expr_id| self.program.expressions.get(expr_id))
                    .is_some_and(|expr| matches!(expr.kind, AstExprKind::Record(_)))
                    .then_some(CheckedScopeKind::Record)
                    .unwrap_or(CheckedScopeKind::Block);
                self.scopes.push(CheckedScope {
                    id: child_scope,
                    parent: Some(scope_id),
                    owner: self.statement_declarations.get(&statement.id).copied(),
                    kind,
                    span: checked_statement_span(statement),
                });
                self.statement_body_scopes.insert(statement.id, child_scope);
                if let Some(owner) = self.statement_declarations.get(&statement.id).copied()
                    && let Some(declaration) = self
                        .declarations
                        .iter_mut()
                        .find(|declaration| declaration.id == owner)
                {
                    declaration.body_scope = Some(child_scope);
                }
                if let Some(container) =
                    statement_body_container_expression(statement, &self.program.expressions)
                {
                    self.assign_expression_scope_override(container, child_scope);
                }
                child_scope
            };
            self.predeclare_statement_tree(&statement.children, child_scope);
        }
    }

    fn predeclare_pattern_bindings(&mut self, statements: &[AstStatement]) {
        fn roots(statements: &[AstStatement], output: &mut Vec<usize>) {
            for statement in statements {
                if let Some(expr_id) = statement.expr {
                    output.push(expr_id);
                }
                roots(&statement.children, output);
            }
        }

        let mut expression_roots = Vec::new();
        roots(statements, &mut expression_roots);
        let mut visited = BTreeSet::new();
        for expr_id in expression_roots {
            self.predeclare_pattern_expr(expr_id, &mut visited);
        }
        for expr_id in 0..self.program.expressions.len() {
            self.predeclare_pattern_expr(expr_id, &mut visited);
        }
    }

    fn predeclare_pattern_expr(&mut self, expr_id: usize, visited: &mut BTreeSet<usize>) {
        if !visited.insert(expr_id) {
            return;
        }
        let Some(expr) = self.program.expressions.get(expr_id).cloned() else {
            return;
        };
        let selector = match &expr.kind {
            AstExprKind::When { input, arms } => Some((*input, arms.clone())),
            AstExprKind::Pipe {
                input, op, arms, ..
            } if op == "WHILE" => Some((*input, arms.clone())),
            _ => None,
        };
        if let Some((raw_selector, arms)) = selector {
            let selector = expr.linked_input.unwrap_or_else(|| {
                pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr_id,
                    raw_selector,
                    &self.program.expressions,
                )
            });
            let parent = self
                .expression_scopes
                .get(&expr_id)
                .copied()
                .unwrap_or(LexicalScopeId(0));
            for arm in arms {
                self.predeclare_pattern_arm(arm, selector, parent);
            }
        }
        for child in direct_expression_children(&expr) {
            self.predeclare_pattern_expr(child, visited);
        }
    }

    fn predeclare_pattern_arm(&mut self, expr_id: usize, selector: usize, parent: LexicalScopeId) {
        let Some(AstExpr {
            kind: AstExprKind::MatchArm { pattern, output },
            ..
        }) = self.program.expressions.get(expr_id).cloned()
        else {
            return;
        };
        let statement = exact_expression_statement(&self.program.ast.statements, expr_id).cloned();
        let arm_scope = self.allocate_scope();
        self.scopes.push(CheckedScope {
            id: arm_scope,
            parent: Some(parent),
            owner: None,
            kind: CheckedScopeKind::Block,
            span: checked_expr_span(
                self.program
                    .expressions
                    .get(expr_id)
                    .expect("pattern expression exists"),
            ),
        });
        self.expression_scopes.insert(expr_id, arm_scope);
        if let Some(statement) = &statement {
            self.statement_scopes.insert(statement.id, arm_scope);
        }
        let body_scope = statement
            .as_ref()
            .and_then(|statement| self.statement_body_scopes.get(&statement.id).copied());
        if let Some(body_scope) = body_scope {
            if let Some(scope) = self.scopes.iter_mut().find(|scope| scope.id == body_scope) {
                scope.parent = Some(arm_scope);
            }
        } else if let Some(output) = output {
            self.assign_expression_scope_override(output, arm_scope);
        }
        self.pattern_selectors.insert(expr_id, selector);

        for name in pattern_variable_names(&pattern) {
            let declaration = self.allocate_decl();
            self.scope_declarations
                .insert((arm_scope, name.clone()), declaration);
            self.pattern_declarations
                .insert((expr_id, name.clone()), declaration);
            self.declarations.push(CheckedDeclaration {
                id: declaration,
                scope_id: arm_scope,
                name,
                kind: CheckedDeclarationKind::PatternBinding,
                flow_type: unknown_flow_type(),
                value: None,
                body_scope: None,
                span: checked_expr_span(
                    self.program
                        .expressions
                        .get(expr_id)
                        .expect("pattern expression exists"),
                ),
            });
            self.occurrences.push(SemanticOccurrence {
                target: declaration,
                kind: SemanticOccurrenceKind::Declaration,
                span: checked_expr_span(
                    self.program
                        .expressions
                        .get(expr_id)
                        .expect("pattern expression exists"),
                ),
            });
        }
    }

    fn assign_expression_scope(&mut self, expr_id: usize, scope_id: LexicalScopeId) {
        self.assign_expression_scope_seen(expr_id, scope_id, &mut BTreeSet::new());
    }

    fn assign_expression_scope_seen(
        &mut self,
        expr_id: usize,
        scope_id: LexicalScopeId,
        visited: &mut BTreeSet<usize>,
    ) {
        if !visited.insert(expr_id) {
            return;
        }
        self.expression_scopes.insert(expr_id, scope_id);
        let Some(expr) = self.program.expressions.get(expr_id) else {
            return;
        };
        for child in direct_expression_children(expr) {
            self.assign_expression_scope_seen(child, scope_id, visited);
        }
    }

    fn lower_checked_tree(
        &mut self,
        render_slots: &RenderSlotTable,
    ) -> (Vec<CheckedStatement>, Vec<CheckedExpression>) {
        let render_slot_statements = render_slots
            .slots
            .iter()
            .map(|slot| slot.slot_statement_id)
            .collect::<BTreeSet<_>>();
        let mut statements = Vec::new();
        self.lower_checked_statements(
            &self.program.ast.statements,
            &render_slot_statements,
            &mut statements,
        );
        statements.sort_by_key(|statement| statement.id);

        let call_ids = self
            .calls
            .iter()
            .map(|call| (call.expression.0 as usize, call.id))
            .collect::<BTreeMap<_, _>>();
        let signature_effects = self
            .signatures
            .iter()
            .map(|signature| (signature.decl_id, signature.effect))
            .collect::<BTreeMap<_, _>>();
        let call_effects = self
            .calls
            .iter()
            .filter_map(|call| {
                signature_effects
                    .get(&call.callable)
                    .copied()
                    .map(|effect| (call.expression.0 as usize, effect))
            })
            .collect::<BTreeMap<_, _>>();
        let mut expressions = self
            .program
            .expressions
            .iter()
            .map(|expr| {
                let scope_id = self
                    .expression_scopes
                    .get(&expr.id)
                    .copied()
                    .or_else(|| {
                        self.expression_owner
                            .get(&expr.id)
                            .and_then(|owner| self.signature(owner))
                            .map(|owner| owner.scope_id)
                    })
                    .unwrap_or(LexicalScopeId(0));
                CheckedExpression {
                    id: CheckedExprId(expr.id as u32),
                    scope_id,
                    declaration: self
                        .expression_declarations
                        .get(&expr.id)
                        .copied()
                        .or_else(|| self.lexical_owner(scope_id)),
                    flow_type: self
                        .inferred_expr_types
                        .get(&expr.id)
                        .cloned()
                        .unwrap_or_else(unknown_flow_type),
                    effect: call_effects.get(&expr.id).copied().map_or_else(
                        || checked_expression_effect(expr),
                        |call_effect| {
                            merge_checked_effects(checked_expression_effect(expr), call_effect)
                        },
                    ),
                    kind: self.lower_checked_expression_kind(expr, scope_id, &call_ids),
                    span: checked_expr_span(expr),
                }
            })
            .collect::<Vec<_>>();
        self.finalize_structured_values(&statements, &mut expressions);
        for expression in &expressions {
            let target = match &expression.kind {
                CheckedExpressionKind::Read { target, .. }
                | CheckedExpressionKind::Drain { target, .. } => Some(*target),
                _ => None,
            };
            if let Some(target) = target {
                self.occurrences.push(SemanticOccurrence {
                    target,
                    kind: SemanticOccurrenceKind::Read,
                    span: expression.span,
                });
            }
        }
        (statements, expressions)
    }

    fn finalize_structured_values(
        &mut self,
        statements: &[CheckedStatement],
        expressions: &mut [CheckedExpression],
    ) {
        let statement_values = statements
            .iter()
            .map(|statement| (statement.id, statement.value))
            .collect::<BTreeMap<_, _>>();
        let roots = self.program.ast.statements.clone();
        for statement in &roots {
            self.finalize_structured_statement_value(statement, &statement_values, expressions);
        }
    }

    fn finalize_structured_statement_value(
        &mut self,
        statement: &AstStatement,
        statement_values: &BTreeMap<CheckedStatementId, Option<CheckedExprId>>,
        expressions: &mut [CheckedExpression],
    ) {
        for child in &statement.children {
            self.finalize_structured_statement_value(child, statement_values, expressions);
        }
        let Some(value) = statement_values
            .get(&CheckedStatementId(statement.id as u32))
            .copied()
            .flatten()
        else {
            return;
        };
        if let AstStatementKind::Function { name, .. } = &statement.kind {
            self.set_function_result(name, value, expressions);
            return;
        }
        let Some(declaration) =
            self.statement_declarations
                .get(&statement.id)
                .and_then(|declaration| {
                    self.declarations
                        .iter_mut()
                        .find(|candidate| candidate.id == *declaration)
                })
        else {
            return;
        };
        declaration.value = Some(value);
        if let Some(expression) = expressions.iter().find(|expression| expression.id == value) {
            declaration.flow_type = expression.flow_type.clone();
        }
    }

    fn set_function_result(
        &mut self,
        name: &str,
        result: CheckedExprId,
        expressions: &mut [CheckedExpression],
    ) {
        let Some(signature_index) = self.signature_by_name.get(name).copied() else {
            return;
        };
        self.signatures[signature_index].result_expression = Some(result);
        let Some(expression_type) = expressions
            .iter()
            .find(|expression| expression.id == result)
            .map(|expression| expression.flow_type.clone())
        else {
            return;
        };
        if matches!(self.signatures[signature_index].result.ty, Type::Unknown) {
            self.signatures[signature_index].result = expression_type;
        }
        let callable = self.signatures[signature_index].decl_id;
        let result_type = self.signatures[signature_index].result.clone();
        if let Some(expression) = expressions
            .iter_mut()
            .find(|expression| expression.id == result)
        {
            expression.flow_type = result_type.clone();
        }
        if let Some(declaration) = self
            .declarations
            .iter_mut()
            .find(|declaration| declaration.id == callable)
            && let Type::Function { result, .. } = &mut declaration.flow_type.ty
        {
            **result = result_type.clone();
        }
        if !checked_signature_is_generic(&self.signatures[signature_index]) {
            let mut call_expressions = BTreeSet::new();
            for call in self
                .calls
                .iter_mut()
                .filter(|call| call.callable == callable)
            {
                call.result = result_type.clone();
                call_expressions.insert(call.expression);
            }
            for expression in expressions
                .iter_mut()
                .filter(|expression| call_expressions.contains(&expression.id))
            {
                expression.flow_type = result_type.clone();
            }
        }
    }

    fn validate_checked_expression_roots(
        &mut self,
        statements: &[CheckedStatement],
        expressions: &[CheckedExpression],
    ) {
        let expression_ids = expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<BTreeSet<_>>();
        let roots = self
            .declarations
            .iter()
            .filter_map(|declaration| declaration.value)
            .chain(statements.iter().filter_map(|statement| statement.value))
            .chain(
                self.signatures
                    .iter()
                    .filter(|signature| signature.kind == CheckedCallableKind::User)
                    .filter_map(|signature| signature.result_expression),
            )
            .collect::<BTreeSet<_>>();
        let mut reported_missing = BTreeSet::new();
        let mut reported_cycles = BTreeSet::new();
        for root in roots {
            self.validate_checked_expression_root(
                root,
                expressions,
                &expression_ids,
                &mut BTreeSet::new(),
                &mut Vec::new(),
                &mut BTreeSet::new(),
                &mut reported_missing,
                &mut reported_cycles,
            );
        }
    }

    fn validate_user_callable_results(&mut self) {
        let missing = self
            .signatures
            .iter()
            .filter(|signature| {
                signature.kind == CheckedCallableKind::User && signature.result_expression.is_none()
            })
            .map(|signature| (signature.name.clone(), signature.decl_id))
            .collect::<Vec<_>>();
        for (name, declaration) in missing {
            let span = self
                .declarations
                .iter()
                .find(|candidate| candidate.id == declaration)
                .map_or(CheckedSpan::default(), |candidate| candidate.span);
            self.diagnostics.push(TypeDiagnostic {
                severity: DiagnosticSeverity::Error,
                line: span.line,
                start: span.start,
                end: span.end,
                message: format!(
                    "FUNCTION `{name}` has no canonical checked result expression in its indented body"
                ),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_checked_expression_root(
        &mut self,
        expression: CheckedExprId,
        expressions: &[CheckedExpression],
        expression_ids: &BTreeSet<CheckedExprId>,
        visiting: &mut BTreeSet<CheckedExprId>,
        path: &mut Vec<CheckedExprId>,
        visited: &mut BTreeSet<CheckedExprId>,
        reported_missing: &mut BTreeSet<CheckedExprId>,
        reported_cycles: &mut BTreeSet<CheckedExprId>,
    ) {
        if !expression_ids.contains(&expression) {
            if reported_missing.insert(expression) {
                self.diagnostics.push(TypeDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    line: 1,
                    start: 0,
                    end: 0,
                    message: format!(
                        "CheckedProgram expression root {} references a missing expression",
                        expression.0
                    ),
                });
            }
            return;
        }
        if visited.contains(&expression) {
            return;
        }
        if !visiting.insert(expression) {
            if reported_cycles.insert(expression) {
                let cycle = path
                    .iter()
                    .position(|candidate| *candidate == expression)
                    .map_or_else(
                        || vec![expression],
                        |start| {
                            path[start..]
                                .iter()
                                .copied()
                                .chain(std::iter::once(expression))
                                .collect()
                        },
                    )
                    .into_iter()
                    .map(|expression_id| {
                        expressions
                            .iter()
                            .find(|expression| expression.id == expression_id)
                            .map_or_else(
                                || expression_id.0.to_string(),
                                |expression| {
                                    format!(
                                        "{}:{:?}@{}",
                                        expression_id.0, expression.kind, expression.span.line
                                    )
                                },
                            )
                    })
                    .collect::<Vec<_>>()
                    .join(" -> ");
                let span = expressions
                    .iter()
                    .find(|candidate| candidate.id == expression)
                    .map_or(CheckedSpan::default(), |candidate| candidate.span);
                self.diagnostics.push(TypeDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    line: span.line,
                    start: span.start,
                    end: span.end,
                    message: format!(
                        "canonical checked value contains an expansion cycle: {cycle}"
                    ),
                });
            }
            return;
        }
        path.push(expression);
        for dependency in self.checked_expression_dependencies(expression, expressions) {
            self.validate_checked_expression_root(
                dependency,
                expressions,
                expression_ids,
                visiting,
                path,
                visited,
                reported_missing,
                reported_cycles,
            );
        }
        path.pop();
        visiting.remove(&expression);
        visited.insert(expression);
    }

    fn checked_expression_dependencies(
        &self,
        expression: CheckedExprId,
        expressions: &[CheckedExpression],
    ) -> Vec<CheckedExprId> {
        let Some(expression) = expressions
            .iter()
            .find(|candidate| candidate.id == expression)
        else {
            return Vec::new();
        };
        match &expression.kind {
            CheckedExpressionKind::Read { target, .. } => self
                .declarations
                .iter()
                .find(|declaration| declaration.id == *target)
                .filter(|declaration| self.scope_is_function_local(declaration.scope_id))
                .and_then(|declaration| declaration.value)
                .into_iter()
                .collect(),
            CheckedExpressionKind::Call { call } => {
                let Some(call) = self.calls.iter().find(|candidate| candidate.id == *call) else {
                    return Vec::new();
                };
                call.entries
                    .iter()
                    .filter_map(|entry| match entry {
                        CheckedCallEntry::Input { value, .. } => Some(*value),
                        CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => {
                            None
                        }
                    })
                    .chain(call.pass)
                    .chain(
                        self.signatures
                            .iter()
                            .find(|signature| {
                                signature.decl_id == call.callable
                                    && signature.kind == CheckedCallableKind::User
                            })
                            .and_then(|signature| signature.result_expression),
                    )
                    .collect()
            }
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields }
            | CheckedExpressionKind::Record { fields } => {
                fields.iter().map(|field| field.value).collect()
            }
            CheckedExpressionKind::List { items, .. }
            | CheckedExpressionKind::Bytes { items, .. } => items.clone(),
            CheckedExpressionKind::When { input, arms }
            | CheckedExpressionKind::While { input, arms } => std::iter::once(*input)
                .chain(arms.iter().copied())
                .collect(),
            CheckedExpressionKind::Draining { input } => vec![*input],
            CheckedExpressionKind::Hold { initial, .. } => vec![*initial],
            CheckedExpressionKind::Then { input, output } => {
                std::iter::once(*input).chain(*output).collect()
            }
            CheckedExpressionKind::Infix { left, right, .. } => vec![*left, *right],
            CheckedExpressionKind::MatchArm { output, .. } => output.iter().copied().collect(),
            CheckedExpressionKind::Block { bindings, result } => bindings
                .iter()
                .map(|binding| binding.value)
                .chain(result.iter().copied())
                .collect(),
            CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Bool { .. }
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Latest
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. } => Vec::new(),
        }
    }

    fn scope_is_function_local(&self, mut scope_id: LexicalScopeId) -> bool {
        let mut visited = BTreeSet::new();
        while visited.insert(scope_id) {
            let Some(scope) = self.scopes.iter().find(|scope| scope.id == scope_id) else {
                return false;
            };
            if scope.kind == CheckedScopeKind::Function {
                return true;
            }
            let Some(parent) = scope.parent else {
                return false;
            };
            scope_id = parent;
        }
        false
    }

    fn lower_checked_statements(
        &self,
        source: &[AstStatement],
        render_slot_statements: &BTreeSet<usize>,
        target: &mut Vec<CheckedStatement>,
    ) {
        for statement in source {
            let declaration = self.statement_declarations.get(&statement.id).copied();
            let kind = match &statement.kind {
                AstStatementKind::Function { .. } => CheckedStatementKind::Function {
                    declaration: declaration.expect("function declaration was predeclared"),
                },
                AstStatementKind::Field { .. } => CheckedStatementKind::Field {
                    declaration: declaration.expect("field declaration was predeclared"),
                },
                AstStatementKind::Source { event, .. } => CheckedStatementKind::Source {
                    declaration,
                    event: event.clone(),
                },
                AstStatementKind::Hold { name, .. } => CheckedStatementKind::Hold {
                    declaration,
                    name: name.clone(),
                },
                AstStatementKind::List { capacity, .. } => CheckedStatementKind::List {
                    declaration,
                    capacity: *capacity,
                },
                AstStatementKind::Block => CheckedStatementKind::Block,
                AstStatementKind::Spread => CheckedStatementKind::Spread,
                AstStatementKind::Expression => CheckedStatementKind::Expression,
            };
            target.push(CheckedStatement {
                id: CheckedStatementId(statement.id as u32),
                scope_id: self
                    .statement_scopes
                    .get(&statement.id)
                    .copied()
                    .unwrap_or(LexicalScopeId(0)),
                kind,
                value: canonical_checked_statement_value_expression(
                    statement,
                    &self.program.expressions,
                )
                .map(|expr| CheckedExprId(expr as u32)),
                value_use: if render_slot_statements.contains(&statement.id) {
                    CheckedValueUse::RenderSlot
                } else {
                    CheckedValueUse::RuntimeValue
                },
                children: statement
                    .children
                    .iter()
                    .map(|child| CheckedStatementId(child.id as u32))
                    .collect(),
                span: checked_statement_span(statement),
            });
            self.lower_checked_statements(&statement.children, render_slot_statements, target);
        }
    }

    fn lower_checked_expression_kind(
        &self,
        expr: &AstExpr,
        scope_id: LexicalScopeId,
        call_ids: &BTreeMap<usize, CheckedCallId>,
    ) -> CheckedExpressionKind {
        let id = |value: usize| CheckedExprId(value as u32);
        let fields = |fields: &[AstRecordField]| {
            fields
                .iter()
                .map(|field| CheckedRecordField {
                    name: field.name.clone(),
                    value: id(field.value),
                    spread: field.spread,
                    span: CheckedSpan {
                        line: expr.line,
                        start: field.start,
                        end: field.end,
                    },
                })
                .collect::<Vec<_>>()
        };
        match &expr.kind {
            AstExprKind::Identifier(name)
                if self.builtin_static_symbol_exprs.contains(&expr.id) =>
            {
                CheckedExpressionKind::Text {
                    value: name.clone(),
                }
            }
            AstExprKind::Path(parts) if self.builtin_static_symbol_exprs.contains(&expr.id) => {
                CheckedExpressionKind::Text {
                    value: parts.join("."),
                }
            }
            AstExprKind::Identifier(name) => self.checked_read(expr.id, scope_id, &[name.clone()]),
            AstExprKind::Path(parts) => self.checked_read(expr.id, scope_id, parts),
            AstExprKind::Drain { path } => {
                let parts = match path {
                    AstDrainPath::Binding { name } => vec![name.clone()],
                    AstDrainPath::Field { binding, fields } => std::iter::once(binding.clone())
                        .chain(fields.iter().cloned())
                        .collect(),
                    AstDrainPath::Passed { fields } => {
                        return CheckedExpressionKind::Passed {
                            projection: fields.clone(),
                        };
                    }
                };
                if let Some((target, projection)) =
                    self.resolve_checked_read_path(expr.id, scope_id, &parts)
                {
                    CheckedExpressionKind::Drain { target, projection }
                } else {
                    CheckedExpressionKind::ExternalRead {
                        canonical_path: canonical_checked_path(&parts),
                    }
                }
            }
            AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
                CheckedExpressionKind::Text {
                    value: value.clone(),
                }
            }
            AstExprKind::Number(value) => CheckedExpressionKind::Number {
                value: value.clone(),
            },
            AstExprKind::ByteLiteral { value, .. } => {
                CheckedExpressionKind::BytesByte { value: *value }
            }
            AstExprKind::Bool(value) => CheckedExpressionKind::Bool { value: *value },
            AstExprKind::Enum(name) | AstExprKind::Tag(name) => {
                CheckedExpressionKind::Tag { name: name.clone() }
            }
            AstExprKind::TaggedObject {
                tag,
                fields: record,
            } => CheckedExpressionKind::TaggedObject {
                tag: tag.clone(),
                fields: fields(record),
            },
            AstExprKind::Source => CheckedExpressionKind::Source,
            AstExprKind::Pipe {
                input, op, arms, ..
            } if op == "WHILE" => CheckedExpressionKind::While {
                input: id(expr.linked_input.unwrap_or_else(|| {
                    pipeline_source_expr_id(
                        &self.program.ast.statements,
                        expr.id,
                        *input,
                        &self.program.expressions,
                    )
                })),
                arms: arms.iter().copied().map(id).collect(),
            },
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
                call_ids.get(&expr.id).copied().map_or_else(
                    || CheckedExpressionKind::Invalid {
                        tokens: vec!["unbound_call".to_owned()],
                    },
                    |call| CheckedExpressionKind::Call { call },
                )
            }
            AstExprKind::Draining { input } => CheckedExpressionKind::Draining {
                input: id(expr.linked_input.unwrap_or_else(|| {
                    pipeline_source_expr_id(
                        &self.program.ast.statements,
                        expr.id,
                        *input,
                        &self.program.expressions,
                    )
                })),
            },
            AstExprKind::Hold { initial, name } => CheckedExpressionKind::Hold {
                initial: id(expr.linked_input.unwrap_or_else(|| {
                    pipeline_source_expr_id(
                        &self.program.ast.statements,
                        expr.id,
                        *initial,
                        &self.program.expressions,
                    )
                })),
                name: name.clone(),
            },
            AstExprKind::Latest => CheckedExpressionKind::Latest,
            AstExprKind::When { input, arms } => CheckedExpressionKind::When {
                input: id(expr.linked_input.unwrap_or_else(|| {
                    pipeline_source_expr_id(
                        &self.program.ast.statements,
                        expr.id,
                        *input,
                        &self.program.expressions,
                    )
                })),
                arms: arms.iter().copied().map(id).collect(),
            },
            AstExprKind::Then { input, output } => CheckedExpressionKind::Then {
                input: id(expr.linked_input.unwrap_or_else(|| {
                    pipeline_source_expr_id(
                        &self.program.ast.statements,
                        expr.id,
                        *input,
                        &self.program.expressions,
                    )
                })),
                output: output.map(id),
            },
            AstExprKind::Infix { left, op, right } => CheckedExpressionKind::Infix {
                left: id(*left),
                op: op.clone(),
                right: id(*right),
            },
            AstExprKind::MatchArm { pattern, output } => CheckedExpressionKind::MatchArm {
                pattern: pattern.clone(),
                bindings: pattern_variable_names(pattern)
                    .into_iter()
                    .filter_map(|name| self.pattern_declarations.get(&(expr.id, name)).copied())
                    .collect(),
                output: output.map(id),
            },
            AstExprKind::Block { bindings, result } => CheckedExpressionKind::Block {
                bindings: bindings
                    .iter()
                    .filter_map(|binding| {
                        Some(CheckedBlockBinding {
                            declaration: *self.statement_declarations.get(&binding.statement)?,
                            value: id(binding.value),
                            span: CheckedSpan {
                                line: expr.line,
                                start: binding.start,
                                end: binding.end,
                            },
                        })
                    })
                    .collect(),
                result: result.map(id),
            },
            AstExprKind::Object(record) => CheckedExpressionKind::Object {
                fields: fields(record),
            },
            AstExprKind::Record(record) => CheckedExpressionKind::Record {
                fields: fields(record),
            },
            AstExprKind::ListLiteral { capacity, items } => CheckedExpressionKind::List {
                capacity: *capacity,
                items: items.iter().copied().map(id).collect(),
            },
            AstExprKind::BytesLiteral { size, items } => CheckedExpressionKind::Bytes {
                fixed_size: match size {
                    BytesSizeSyntax::Fixed(size) => Some(*size),
                    BytesSizeSyntax::Dynamic | BytesSizeSyntax::Infer => None,
                },
                items: items.iter().copied().map(id).collect(),
            },
            AstExprKind::Delimiter => CheckedExpressionKind::Delimiter,
            AstExprKind::Unknown(tokens) => CheckedExpressionKind::Invalid {
                tokens: tokens.clone(),
            },
        }
    }

    fn checked_read(
        &self,
        expr_id: usize,
        scope_id: LexicalScopeId,
        parts: &[String],
    ) -> CheckedExpressionKind {
        if parts.first().is_some_and(|part| part == "PASSED") {
            return CheckedExpressionKind::Passed {
                projection: parts.iter().skip(1).cloned().collect(),
            };
        }
        if parts.is_empty() {
            return CheckedExpressionKind::Invalid { tokens: Vec::new() };
        }
        self.resolve_checked_read_path(expr_id, scope_id, parts)
            .map_or_else(
                || CheckedExpressionKind::ExternalRead {
                    canonical_path: canonical_checked_path(parts),
                },
                |(target, projection)| CheckedExpressionKind::Read { target, projection },
            )
    }

    fn resolve_checked_read_path(
        &self,
        expr_id: usize,
        scope_id: LexicalScopeId,
        parts: &[String],
    ) -> Option<(DeclId, Vec<String>)> {
        let mut target = self.resolve_checked_read_name(expr_id, scope_id, parts.first()?)?;
        for (index, part) in parts.iter().enumerate().skip(1) {
            let Some(body_scope) = self
                .declarations
                .iter()
                .find(|declaration| declaration.id == target)
                .and_then(|declaration| declaration.body_scope)
            else {
                return Some((target, parts[index..].to_vec()));
            };
            let Some(child) = self
                .scope_declarations
                .get(&(body_scope, part.clone()))
                .copied()
            else {
                return Some((target, parts[index..].to_vec()));
            };
            target = child;
        }
        Some((target, Vec::new()))
    }

    fn resolve_checked_read_name(
        &self,
        expr_id: usize,
        scope_id: LexicalScopeId,
        name: &str,
    ) -> Option<DeclId> {
        let target = self.resolve_name(scope_id, name)?;
        let Some(initializing) = self.expression_declarations.get(&expr_id).copied() else {
            return Some(target);
        };
        if target != initializing {
            return Some(target);
        }
        let declaration = self
            .declarations
            .iter()
            .find(|declaration| declaration.id == initializing)?;
        if declaration.kind != CheckedDeclarationKind::Field {
            return Some(target);
        }
        let Some(parent_scope) = self
            .scopes
            .iter()
            .find(|scope| scope.id == scope_id)
            .and_then(|scope| scope.parent)
        else {
            return Some(target);
        };
        let Some(outer) = self.resolve_name(parent_scope, name) else {
            return Some(target);
        };
        self.declarations
            .iter()
            .find(|declaration| declaration.id == outer)
            .filter(|declaration| {
                matches!(
                    declaration.kind,
                    CheckedDeclarationKind::ValueParameter | CheckedDeclarationKind::OutParameter
                )
            })
            .map(|_| outer)
            .or(Some(target))
    }

    fn resolve_name(&self, mut scope_id: LexicalScopeId, name: &str) -> Option<DeclId> {
        loop {
            if let Some(declaration) = self
                .scope_declarations
                .get(&(scope_id, name.to_owned()))
                .copied()
            {
                return Some(declaration);
            }
            scope_id = self
                .scopes
                .iter()
                .find(|scope| scope.id == scope_id)
                .and_then(|scope| scope.parent)?;
        }
    }

    fn signature(&self, name: &str) -> Option<&CheckedCallableSignature> {
        self.signature_by_name
            .get(name)
            .and_then(|index| self.signatures.get(*index))
    }

    fn call_diagnostic(&mut self, expr: &AstExpr, message: String) {
        self.diagnostics.push(TypeDiagnostic {
            severity: DiagnosticSeverity::Error,
            line: expr.line,
            start: expr.start,
            end: expr.end,
            message,
        });
    }

    fn validate_callable_coverage(&mut self) {
        let bound = self
            .calls
            .iter()
            .map(|call| call.expression.0 as usize)
            .collect::<BTreeSet<_>>();
        let missing = self
            .program
            .expressions
            .iter()
            .filter_map(|expr| {
                if matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "WHILE") {
                    return None;
                }
                let function = ast_callable_name(expr)?;
                (!bound.contains(&expr.id) && self.signature(function).is_none())
                    .then(|| (expr.clone(), function.to_owned()))
            })
            .collect::<Vec<_>>();
        for (expr, function) in missing {
            self.call_diagnostic(
                &expr,
                format!(
                    "`{function}` has no authoritative canonical argument schema for CheckedProgram lowering"
                ),
            );
        }
    }

    fn validate_output_producers(&mut self) {
        let mut driven = self
            .signatures
            .iter()
            .filter(|signature| signature.kind == CheckedCallableKind::Builtin)
            .flat_map(|signature| {
                signature
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.kind == CheckedParameterKind::Out)
                    .map(|parameter| parameter.decl_id)
            })
            .collect::<BTreeSet<_>>();
        let forwarding = self
            .calls
            .iter()
            .flat_map(|call| {
                call.entries.iter().filter_map(move |entry| match entry {
                    CheckedCallEntry::ForwardOut { formal, target, .. } => {
                        Some((*formal, *target, call.expression.0 as usize))
                    }
                    _ => None,
                })
            })
            .collect::<Vec<_>>();
        let fresh = self
            .calls
            .iter()
            .flat_map(|call| {
                call.entries.iter().filter_map(move |entry| match entry {
                    CheckedCallEntry::FreshOut {
                        formal,
                        output,
                        name,
                        ..
                    } => Some((*formal, *output, name.clone(), call.expression.0 as usize)),
                    _ => None,
                })
            })
            .collect::<Vec<_>>();
        loop {
            let mut changed = false;
            for (formal, target, _) in &forwarding {
                if driven.contains(formal) {
                    changed |= driven.insert(*target);
                }
            }
            if !changed {
                break;
            }
        }

        let mut drivers = BTreeMap::<DeclId, Vec<usize>>::new();
        for (formal, target, expr_id) in &forwarding {
            if driven.contains(formal) {
                drivers.entry(*target).or_default().push(*expr_id);
            }
        }
        for (formal, output, _, expr_id) in &fresh {
            if driven.contains(formal) {
                drivers.entry(*output).or_default().push(*expr_id);
            }
        }

        let user_outputs = self
            .signatures
            .iter()
            .filter(|signature| signature.kind == CheckedCallableKind::User)
            .flat_map(|signature| {
                signature
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.kind == CheckedParameterKind::Out)
                    .map(|parameter| {
                        (
                            parameter.decl_id,
                            format!(
                                "output `{}` in `FUNCTION {}`",
                                parameter.name, signature.name
                            ),
                            parameter.start,
                            parameter.end,
                        )
                    })
            })
            .collect::<Vec<_>>();
        let fresh_outputs = fresh
            .iter()
            .map(|(_, output, name, expr_id)| {
                let expr = self.program.expressions.get(*expr_id);
                (
                    *output,
                    format!("fresh output `{name}`"),
                    expr.map_or(0, |expr| expr.start),
                    expr.map_or(0, |expr| expr.end),
                )
            })
            .collect::<Vec<_>>();
        for (output, label, start, end) in user_outputs.into_iter().chain(fresh_outputs) {
            let count = drivers.get(&output).map_or(0, Vec::len);
            if count == 1 {
                continue;
            }
            self.diagnostics.push(TypeDiagnostic {
                severity: DiagnosticSeverity::Error,
                line: line_for_byte(&self.program.source, start),
                start,
                end,
                message: if count == 0 {
                    format!("{label} has no structural producer")
                } else {
                    format!("{label} has {count} structural producers; exactly one is required")
                },
            });
        }

        let cycle_nodes = output_cycle_nodes(
            &forwarding
                .iter()
                .map(|(formal, target, _)| (*target, *formal))
                .collect::<Vec<_>>(),
        );
        for output in cycle_nodes {
            let label = self
                .signatures
                .iter()
                .flat_map(|signature| &signature.parameters)
                .find(|parameter| parameter.decl_id == output)
                .map_or_else(
                    || format!("output declaration {}", output.0),
                    |parameter| format!("output `{}`", parameter.name),
                );
            self.diagnostics.push(TypeDiagnostic {
                severity: DiagnosticSeverity::Error,
                line: 1,
                start: 0,
                end: 0,
                message: format!("{label} participates in an OUT forwarding cycle"),
            });
        }
    }
}

fn unknown_flow_type() -> FlowType {
    continuous_flow_type(Type::Unknown)
}

fn checked_read_path_parts(expression: &AstExpr) -> Option<Vec<String>> {
    match &expression.kind {
        AstExprKind::Identifier(name) => Some(vec![name.clone()]),
        AstExprKind::Path(parts) => Some(parts.clone()),
        AstExprKind::Drain { path } => Some(drain_path_parts(path)),
        _ => None,
    }
}

fn projected_object_scheme(paths: &[Vec<String>], next_var: &mut u32) -> Type {
    let mut shape = ObjectShape {
        fields: BTreeMap::new(),
        field_order: Vec::new(),
        open: true,
    };
    for path in paths {
        insert_projected_type_path(&mut shape, path, next_var);
    }
    Type::Object(shape)
}

fn insert_projected_type_path(shape: &mut ObjectShape, path: &[String], next_var: &mut u32) {
    let Some(field) = path.first() else {
        return;
    };
    if !shape.fields.contains_key(field) {
        shape.field_order.push(field.clone());
        let ty = if path.len() == 1 {
            let var = TypeVar(*next_var);
            *next_var += 1;
            Type::Var(var)
        } else {
            Type::Object(ObjectShape {
                fields: BTreeMap::new(),
                field_order: Vec::new(),
                open: true,
            })
        };
        shape.fields.insert(field.clone(), ty);
    }
    if path.len() == 1 {
        return;
    }
    let entry = shape.fields.get_mut(field).expect("projected field exists");
    if !matches!(entry, Type::Object(_)) {
        *entry = Type::Object(ObjectShape {
            fields: BTreeMap::new(),
            field_order: Vec::new(),
            open: true,
        });
    }
    let Type::Object(child) = entry else {
        unreachable!("projected nested field is an object")
    };
    insert_projected_type_path(child, &path[1..], next_var);
}

fn checked_signature_is_generic(signature: &CheckedCallableSignature) -> bool {
    signature
        .parameters
        .iter()
        .any(|parameter| checked_type_contains_var(&parameter.flow_type.ty))
        || checked_type_contains_var(&signature.result.ty)
}

fn checked_type_contains_var(ty: &Type) -> bool {
    match ty {
        Type::Var(_) => true,
        Type::List(item) => checked_type_contains_var(item),
        Type::Function { args, result } => {
            args.iter().any(checked_type_contains_var) || checked_type_contains_var(&result.ty)
        }
        Type::Object(shape) => shape.fields.values().any(checked_type_contains_var),
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields.fields.values().any(checked_type_contains_var),
        }),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Skip
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => false,
    }
}

fn substitute_checked_type(ty: &Type, substitutions: &BTreeMap<TypeVar, Type>) -> Type {
    substitute_checked_type_inner(ty, substitutions, &mut BTreeSet::new())
}

fn substitute_checked_type_inner(
    ty: &Type,
    substitutions: &BTreeMap<TypeVar, Type>,
    active: &mut BTreeSet<TypeVar>,
) -> Type {
    match ty {
        Type::Var(var) => {
            let Some(replacement) = substitutions
                .get(var)
                .filter(|replacement| *replacement != ty)
            else {
                return ty.clone();
            };
            if !active.insert(*var) {
                return ty.clone();
            }
            let substituted = substitute_checked_type_inner(replacement, substitutions, active);
            active.remove(var);
            substituted
        }
        Type::List(item) => Type::List(Box::new(substitute_checked_type_inner(
            item,
            substitutions,
            active,
        ))),
        Type::Function { args, result } => Type::Function {
            args: args
                .iter()
                .map(|arg| substitute_checked_type_inner(arg, substitutions, active))
                .collect(),
            result: Box::new(FlowType {
                mode: result.mode,
                ty: substitute_checked_type_inner(&result.ty, substitutions, active),
            }),
        },
        Type::Object(shape) => Type::Object(ObjectShape {
            fields: shape
                .fields
                .iter()
                .map(|(name, ty)| {
                    (
                        name.clone(),
                        substitute_checked_type_inner(ty, substitutions, active),
                    )
                })
                .collect(),
            field_order: shape.field_order.clone(),
            open: shape.open,
        }),
        Type::VariantSet(variants) => Type::VariantSet(
            variants
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) => Variant::Tag(tag.clone()),
                    Variant::Tagged { tag, fields } => Variant::Tagged {
                        tag: tag.clone(),
                        fields: ObjectShape {
                            fields: fields
                                .fields
                                .iter()
                                .map(|(name, ty)| {
                                    (
                                        name.clone(),
                                        substitute_checked_type_inner(ty, substitutions, active),
                                    )
                                })
                                .collect(),
                            field_order: fields.field_order.clone(),
                            open: fields.open,
                        },
                    },
                })
                .collect(),
        ),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Skip
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => ty.clone(),
    }
}

fn unify_checked_type_pattern(
    pattern: &Type,
    actual: &Type,
    substitutions: &mut BTreeMap<TypeVar, Type>,
) {
    match (pattern, actual) {
        (Type::Var(var), Type::Unknown | Type::UnresolvedShape { .. }) => {
            let _ = var;
        }
        (Type::Var(var), actual) => match substitutions.get(var) {
            Some(existing) if !is_value_placeholder_type(existing) => {}
            _ => {
                substitutions.insert(*var, actual.clone());
            }
        },
        (Type::List(pattern), Type::List(actual)) => {
            unify_checked_type_pattern(pattern, actual, substitutions);
        }
        (Type::Object(pattern), Type::Object(actual)) => {
            for (name, pattern) in &pattern.fields {
                if let Some(actual) = actual.fields.get(name) {
                    unify_checked_type_pattern(pattern, actual, substitutions);
                }
            }
        }
        (Type::VariantSet(pattern), Type::VariantSet(actual)) => {
            for pattern in pattern {
                let Variant::Tagged {
                    tag: pattern_tag,
                    fields: pattern_fields,
                } = pattern
                else {
                    continue;
                };
                let Some(Variant::Tagged {
                    fields: actual_fields,
                    ..
                }) = actual.iter().find(
                    |variant| matches!(variant, Variant::Tagged { tag, .. } if tag == pattern_tag),
                )
                else {
                    continue;
                };
                for (name, pattern) in &pattern_fields.fields {
                    if let Some(actual) = actual_fields.fields.get(name) {
                        unify_checked_type_pattern(pattern, actual, substitutions);
                    }
                }
            }
        }
        _ => {}
    }
}

fn tagged_variant_field_type(selector: &Type, tag: &str, field: &str) -> Option<Type> {
    let Type::VariantSet(variants) = selector else {
        return None;
    };
    variants.iter().find_map(|variant| match variant {
        Variant::Tagged {
            tag: variant_tag,
            fields,
        } if variant_tag == tag => fields.fields.get(field).cloned(),
        _ => None,
    })
}

fn continuous_flow_type(ty: Type) -> FlowType {
    FlowType {
        mode: FlowMode::Continuous,
        ty,
    }
}

fn ast_callable_name(expr: &AstExpr) -> Option<&str> {
    match &expr.kind {
        AstExprKind::Call { function, .. } => Some(function),
        AstExprKind::Pipe { op, .. } => Some(op),
        _ => None,
    }
}

fn checked_statement_span(statement: &AstStatement) -> CheckedSpan {
    CheckedSpan {
        line: statement.line,
        start: statement.start,
        end: statement.end,
    }
}

fn checked_expr_span(expr: &AstExpr) -> CheckedSpan {
    CheckedSpan {
        line: expr.line,
        start: expr.start,
        end: expr.end,
    }
}

fn checked_expression_effect(expr: &AstExpr) -> CheckedEffectSummary {
    match &expr.kind {
        AstExprKind::Source => CheckedEffectSummary {
            emits_source: true,
            ..CheckedEffectSummary::default()
        },
        AstExprKind::Hold { .. } | AstExprKind::Latest => CheckedEffectSummary {
            reads_state: true,
            writes_state: true,
            ..CheckedEffectSummary::default()
        },
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if is_typed_host_effect(function) =>
        {
            CheckedEffectSummary {
                invokes_host: true,
                ..CheckedEffectSummary::default()
            }
        }
        _ => CheckedEffectSummary::default(),
    }
}

fn merge_checked_effects(
    left: CheckedEffectSummary,
    right: CheckedEffectSummary,
) -> CheckedEffectSummary {
    CheckedEffectSummary {
        reads_state: left.reads_state || right.reads_state,
        writes_state: left.writes_state || right.writes_state,
        emits_source: left.emits_source || right.emits_source,
        invokes_host: left.invokes_host || right.invokes_host,
    }
}

fn canonical_checked_path(parts: &[String]) -> String {
    boon_parser::canonical_value_path(parts)
}

fn output_cycle_nodes(edges: &[(DeclId, DeclId)]) -> BTreeSet<DeclId> {
    fn visit(
        node: DeclId,
        graph: &BTreeMap<DeclId, Vec<DeclId>>,
        states: &mut BTreeMap<DeclId, u8>,
        stack: &mut Vec<DeclId>,
        cycles: &mut BTreeSet<DeclId>,
    ) {
        states.insert(node, 1);
        stack.push(node);
        for next in graph.get(&node).into_iter().flatten().copied() {
            match states.get(&next).copied().unwrap_or(0) {
                0 => visit(next, graph, states, stack, cycles),
                1 => {
                    if let Some(start) = stack.iter().position(|candidate| *candidate == next) {
                        cycles.extend(stack[start..].iter().copied());
                    }
                }
                _ => {}
            }
        }
        stack.pop();
        states.insert(node, 2);
    }

    let mut graph = BTreeMap::<DeclId, Vec<DeclId>>::new();
    for (from, to) in edges {
        graph.entry(*from).or_default().push(*to);
    }
    let mut states = BTreeMap::new();
    let mut cycles = BTreeSet::new();
    for node in graph.keys().copied().collect::<Vec<_>>() {
        if states.get(&node).copied().unwrap_or(0) == 0 {
            visit(node, &graph, &mut states, &mut Vec::new(), &mut cycles);
        }
    }
    cycles
}

fn expression_single_name(program: &ParsedProgram, expr_id: usize) -> Option<String> {
    match &program.expressions.get(expr_id)?.kind {
        AstExprKind::Identifier(name) => Some(name.clone()),
        AstExprKind::Path(parts) if parts.len() == 1 => Some(parts[0].clone()),
        _ => None,
    }
}

fn expression_owner_functions(program: &ParsedProgram) -> BTreeMap<usize, String> {
    fn collect(
        statements: &[AstStatement],
        expressions: &[AstExpr],
        owner: Option<&str>,
        output: &mut BTreeMap<usize, String>,
    ) {
        for statement in statements {
            let next_owner = match &statement.kind {
                AstStatementKind::Function { name, .. } => Some(name.as_str()),
                _ => owner,
            };
            if let (Some(expr_id), Some(owner)) = (statement.expr, next_owner) {
                let mut ids = BTreeSet::new();
                collect_expression_tree_ids(expr_id, expressions, &mut ids);
                output.extend(ids.into_iter().map(|expr_id| (expr_id, owner.to_owned())));
            }
            collect(&statement.children, expressions, next_owner, output);
        }
    }

    let mut output = BTreeMap::new();
    collect(
        &program.ast.statements,
        &program.expressions,
        None,
        &mut output,
    );
    output
}

fn collect_expression_tree_ids(
    expr_id: usize,
    expressions: &[AstExpr],
    output: &mut BTreeSet<usize>,
) {
    if !output.insert(expr_id) {
        return;
    }
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    for child in direct_expression_children(expr) {
        collect_expression_tree_ids(child, expressions, output);
    }
}

fn collect_statement_expression_tree_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
    result: &mut BTreeSet<usize>,
) {
    if let Some(expression) = statement.expr {
        collect_expression_tree_ids(expression, expressions, result);
    }
    for child in &statement.children {
        collect_statement_expression_tree_ids(child, expressions, result);
    }
}

fn expression_child_ids(expressions: &[AstExpr]) -> BTreeSet<usize> {
    expressions
        .iter()
        .flat_map(direct_expression_children)
        .collect()
}

fn statement_body_container_expression(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    fn is_container(expr_id: usize, expressions: &[AstExpr]) -> bool {
        expressions.get(expr_id).is_some_and(|expr| {
            matches!(
                expr.kind,
                AstExprKind::Block { .. }
                    | AstExprKind::Record(_)
                    | AstExprKind::ListLiteral { .. }
            )
        })
    }

    let expr_id = statement.expr?;
    if is_container(expr_id, expressions) {
        return Some(expr_id);
    }
    match &expressions.get(expr_id)?.kind {
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        }
        | AstExprKind::Then {
            output: Some(output),
            ..
        } if is_container(*output, expressions) => Some(*output),
        _ => None,
    }
}

fn direct_expression_children(expr: &AstExpr) -> Vec<usize> {
    match &expr.kind {
        AstExprKind::BytesLiteral { items, .. } | AstExprKind::ListLiteral { items, .. } => {
            items.clone()
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            fields.iter().map(|field| field.value).collect()
        }
        AstExprKind::Call { args, pass, .. } => args
            .iter()
            .map(|arg| arg.value)
            .chain(pass.iter().map(|pass| pass.value))
            .collect(),
        AstExprKind::Pipe {
            input,
            args,
            pass,
            arms,
            ..
        } => std::iter::once(expr.linked_input.unwrap_or(*input))
            .chain(args.iter().map(|arg| arg.value))
            .chain(pass.iter().map(|pass| pass.value))
            .chain(arms.iter().copied())
            .collect(),
        AstExprKind::When { input, arms } => std::iter::once(expr.linked_input.unwrap_or(*input))
            .chain(arms.iter().copied())
            .collect(),
        AstExprKind::Hold { initial, .. } | AstExprKind::Draining { input: initial } => {
            vec![expr.linked_input.unwrap_or(*initial)]
        }
        AstExprKind::Then { input, output } => std::iter::once(expr.linked_input.unwrap_or(*input))
            .chain(output.iter().copied())
            .collect(),
        AstExprKind::MatchArm { output, .. } => output.iter().copied().collect(),
        AstExprKind::Block { bindings, result } => bindings
            .iter()
            .map(|binding| binding.value)
            .chain(result.iter().copied())
            .collect(),
        AstExprKind::Infix { left, right, .. } => vec![*left, *right],
        AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => Vec::new(),
    }
}

#[derive(Clone)]
struct HostEffectSignature {
    intent_fields: Vec<HostEffectIntentFieldSignature>,
    result_type: Type,
}

#[derive(Clone)]
struct HostEffectIntentFieldSignature {
    name: String,
    ty: Type,
    has_default: bool,
}

fn host_effect_signature(operation: &str) -> Option<HostEffectSignature> {
    let spec = boon_effect_schema::host_effect_spec(operation)?;
    let schema = spec.schema?;
    if spec.result_policy != boon_effect_schema::ResultPolicySpec::ReturnValue {
        return None;
    }
    let boon_effect_schema::ValueType::Record {
        fields,
        open: false,
    } = &schema.intent
    else {
        return None;
    };
    let intent_fields = fields
        .iter()
        .map(|field| HostEffectIntentFieldSignature {
            name: field.name.to_owned(),
            ty: effect_schema_type_to_type(&field.value_type),
            has_default: schema
                .intent_defaults
                .iter()
                .any(|default| default.field_name == field.name),
        })
        .collect::<Vec<_>>();
    Some(HostEffectSignature {
        intent_fields,
        result_type: effect_schema_type_to_type(&schema.result),
    })
}

pub fn is_typed_host_effect(operation: &str) -> bool {
    host_effect_signature(operation).is_some()
}

fn effect_schema_type_to_type(value_type: &boon_effect_schema::ValueType) -> Type {
    match value_type {
        boon_effect_schema::ValueType::Bool => true_false_type(),
        boon_effect_schema::ValueType::Number => Type::Number,
        boon_effect_schema::ValueType::Text => Type::Text,
        boon_effect_schema::ValueType::Bytes { fixed_len } => {
            Type::Bytes(fixed_len.map_or(BytesType::Dynamic, |len| {
                BytesType::Fixed(
                    usize::try_from(len).expect("host effect fixed byte length fits usize"),
                )
            }))
        }
        boon_effect_schema::ValueType::List { item } => {
            Type::List(Box::new(effect_schema_type_to_type(item)))
        }
        boon_effect_schema::ValueType::Record { fields, open } => {
            Type::Object(ObjectShape::from_ordered_fields(
                fields.iter().map(|field| {
                    (
                        field.name.to_owned(),
                        effect_schema_type_to_type(&field.value_type),
                    )
                }),
                *open,
            ))
        }
        boon_effect_schema::ValueType::Variant { variants } => Type::VariantSet(
            variants
                .iter()
                .map(|variant| {
                    if variant.fields.is_empty() {
                        Variant::Tag(variant.tag.to_owned())
                    } else {
                        Variant::Tagged {
                            tag: variant.tag.to_owned(),
                            fields: ObjectShape::from_ordered_fields(
                                variant.fields.iter().map(|field| {
                                    (
                                        field.name.to_owned(),
                                        effect_schema_type_to_type(&field.value_type),
                                    )
                                }),
                                false,
                            ),
                        }
                    }
                })
                .collect(),
        ),
    }
}

fn host_port_table(
    program: &ParsedProgram,
    source_lookup: &SourcePayloadPathLookup,
) -> (HostPortTable, Vec<TypeDiagnostic>) {
    let mut table = HostPortTable::default();
    let mut diagnostics = Vec::new();
    let registries = program
        .ast
        .statements
        .iter()
        .filter(|statement| statement_field_name(statement) == Some("host_ports"))
        .collect::<Vec<_>>();
    if registries.len() > 1 {
        diagnostics.push(diagnostic_for_statement(
            registries.get(1).copied(),
            "top-level `host_ports` may be declared only once".to_owned(),
        ));
    }
    let Some(registry) = registries.first().copied() else {
        return (table, diagnostics);
    };

    let mut ports = BTreeMap::<&str, &AstStatement>::new();
    for port in &registry.children {
        let Some(name) = statement_field_name(port) else {
            diagnostics.push(diagnostic_for_statement(
                Some(port),
                "each `host_ports` entry must be a named record".to_owned(),
            ));
            continue;
        };
        if !matches!(name, "http" | "websocket") {
            diagnostics.push(diagnostic_for_statement(
                Some(port),
                format!("unsupported host port `{name}`; expected `http` or `websocket`"),
            ));
            continue;
        }
        if ports.insert(name, port).is_some() {
            diagnostics.push(diagnostic_for_statement(
                Some(port),
                format!("host port `{name}` is declared more than once"),
            ));
        }
    }
    if ports.is_empty() {
        diagnostics.push(diagnostic_for_statement(
            Some(registry),
            "`host_ports` must declare `http`, `websocket`, or both".to_owned(),
        ));
    }

    if let Some(port) = ports.get("http").copied() {
        let diagnostic_start = diagnostics.len();
        let members = host_port_members(
            port,
            "http",
            &["request", "disconnect", "response"],
            &["request", "response"],
            &mut diagnostics,
        );
        let request_source = members.get("request").and_then(|statement| {
            host_port_source_reference(
                program,
                source_lookup,
                statement,
                "http.request",
                &mut diagnostics,
            )
        });
        let disconnect_source = members.get("disconnect").and_then(|statement| {
            host_port_source_reference(
                program,
                source_lookup,
                statement,
                "http.disconnect",
                &mut diagnostics,
            )
        });
        let response_output = members.get("response").and_then(|statement| {
            host_port_output_reference(program, statement, "http.response", &mut diagnostics)
        });
        if diagnostics.len() == diagnostic_start
            && let (Some(request_source), Some(response_output)) = (request_source, response_output)
        {
            table.http = Some(HttpServerPortTypeEntry {
                line: port.line,
                request_source,
                disconnect_source,
                response_output,
            });
        }
    }

    if let Some(port) = ports.get("websocket").copied() {
        let diagnostic_start = diagnostics.len();
        let members = host_port_members(
            port,
            "websocket",
            &["open", "message", "close", "error", "actions"],
            &["open", "message", "close", "error", "actions"],
            &mut diagnostics,
        );
        let source = |member: &str, diagnostics: &mut Vec<TypeDiagnostic>| {
            members.get(member).and_then(|statement| {
                host_port_source_reference(
                    program,
                    source_lookup,
                    statement,
                    &format!("websocket.{member}"),
                    diagnostics,
                )
            })
        };
        let open_source = source("open", &mut diagnostics);
        let message_source = source("message", &mut diagnostics);
        let close_source = source("close", &mut diagnostics);
        let error_source = source("error", &mut diagnostics);
        let actions_output = members.get("actions").and_then(|statement| {
            host_port_output_reference(program, statement, "websocket.actions", &mut diagnostics)
        });
        if diagnostics.len() == diagnostic_start
            && let (
                Some(open_source),
                Some(message_source),
                Some(close_source),
                Some(error_source),
                Some(actions_output),
            ) = (
                open_source,
                message_source,
                close_source,
                error_source,
                actions_output,
            )
        {
            table.websocket = Some(WebSocketServerPortTypeEntry {
                line: port.line,
                open_source,
                message_source,
                close_source,
                error_source,
                actions_output,
            });
        }
    }

    (table, diagnostics)
}

fn host_port_members<'a>(
    port: &'a AstStatement,
    port_name: &str,
    allowed: &[&str],
    required: &[&str],
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> BTreeMap<&'a str, &'a AstStatement> {
    let mut members = BTreeMap::new();
    for member in &port.children {
        let Some(name) = statement_field_name(member) else {
            diagnostics.push(diagnostic_for_statement(
                Some(member),
                format!("host port `{port_name}` contains an unnamed member"),
            ));
            continue;
        };
        if !allowed.contains(&name) {
            diagnostics.push(diagnostic_for_statement(
                Some(member),
                format!("host port `{port_name}` has unsupported member `{name}`"),
            ));
            continue;
        }
        if members.insert(name, member).is_some() {
            diagnostics.push(diagnostic_for_statement(
                Some(member),
                format!("host port `{port_name}` repeats `{name}`"),
            ));
        }
    }
    for name in required {
        if !members.contains_key(name) {
            diagnostics.push(diagnostic_for_statement(
                Some(port),
                format!("host port `{port_name}` is missing `{name}`"),
            ));
        }
    }
    members
}

fn host_port_source_reference(
    program: &ParsedProgram,
    source_lookup: &SourcePayloadPathLookup,
    statement: &AstStatement,
    member: &str,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Option<String> {
    let Some(expr_id) = direct_statement_value_expr_id(statement, &program.expressions) else {
        diagnostics.push(diagnostic_for_statement(
            Some(statement),
            format!("host port `{member}` has no SOURCE reference"),
        ));
        return None;
    };
    let Some(source) = effect_source_path(program, expr_id, source_lookup, true) else {
        diagnostics.push(diagnostic_for_statement(
            Some(statement),
            format!("host port `{member}` must reference exactly one direct SOURCE"),
        ));
        return None;
    };
    Some(source)
}

fn host_port_output_reference(
    program: &ParsedProgram,
    statement: &AstStatement,
    member: &str,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Option<String> {
    let Some(expr_id) = direct_statement_value_expr_id(statement, &program.expressions) else {
        diagnostics.push(diagnostic_for_statement(
            Some(statement),
            format!("host port `{member}` has no output reference"),
        ));
        return None;
    };
    let output = match &program.expressions.get(expr_id)?.kind {
        AstExprKind::Identifier(name) => Some(name.clone()),
        AstExprKind::Path(parts) if parts.len() == 2 && parts[0] == "outputs" => {
            Some(parts[1].clone())
        }
        _ => None,
    };
    if output.is_none() {
        diagnostics.push(diagnostic_for_statement(
            Some(statement),
            format!("host port `{member}` must reference one named root from top-level `outputs`"),
        ));
    }
    output
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

pub fn check(program: &ParsedProgram) -> TypeCheckReport {
    check_profiled(program).0
}

pub fn check_program(program: &ParsedProgram) -> CheckOutput {
    check_program_profiled(program).0
}

pub fn check_with_external_types(
    program: &ParsedProgram,
    external_types: &ExternalTypeEnvironment,
) -> TypeCheckReport {
    check_profiled_with_external_types(program, external_types).0
}

pub fn check_program_with_external_types(
    program: &ParsedProgram,
    external_types: &ExternalTypeEnvironment,
) -> CheckOutput {
    check_program_profiled_with_external_types(program, external_types).0
}

pub fn check_profiled(program: &ParsedProgram) -> (TypeCheckReport, TypeCheckProfile) {
    let (output, profile) = check_program_profiled(program);
    (output.report, profile)
}

pub fn check_program_profiled(program: &ParsedProgram) -> (CheckOutput, TypeCheckProfile) {
    let (mut checker, init_profile) = Checker::new_profiled(program);
    checker.finish_program_profiled(true, init_profile)
}

pub fn check_profiled_with_external_types(
    program: &ParsedProgram,
    external_types: &ExternalTypeEnvironment,
) -> (TypeCheckReport, TypeCheckProfile) {
    let (output, profile) = check_program_profiled_with_external_types(program, external_types);
    (output.report, profile)
}

pub fn check_program_profiled_with_external_types(
    program: &ParsedProgram,
    external_types: &ExternalTypeEnvironment,
) -> (CheckOutput, TypeCheckProfile) {
    let (mut checker, init_profile) =
        Checker::new_profiled_with_external_types(program, external_types);
    checker.finish_program_profiled(true, init_profile)
}

pub fn check_runtime_profiled(program: &ParsedProgram) -> (TypeCheckReport, TypeCheckProfile) {
    let (output, profile) = check_runtime_program_profiled(program);
    (output.report, profile)
}

pub fn check_runtime_program_profiled(program: &ParsedProgram) -> (CheckOutput, TypeCheckProfile) {
    let (mut checker, init_profile) = Checker::new_profiled(program);
    checker.finish_program_profiled(false, init_profile)
}

pub fn check_runtime_profiled_with_external_types(
    program: &ParsedProgram,
    external_types: &ExternalTypeEnvironment,
) -> (TypeCheckReport, TypeCheckProfile) {
    let (output, profile) =
        check_runtime_program_profiled_with_external_types(program, external_types);
    (output.report, profile)
}

pub fn check_runtime_program_profiled_with_external_types(
    program: &ParsedProgram,
    external_types: &ExternalTypeEnvironment,
) -> (CheckOutput, TypeCheckProfile) {
    let (mut checker, init_profile) =
        Checker::new_profiled_with_external_types(program, external_types);
    checker.finish_program_profiled(false, init_profile)
}

fn role_for_namespace(namespace: &str) -> Option<ProgramRole> {
    match boon_parser::standard_root_kind(namespace) {
        Some(boon_parser::StandardRootKind::ProgramRole) => match namespace {
            "Client" => Some(ProgramRole::Client),
            "Session" => Some(ProgramRole::Session),
            "Server" => Some(ProgramRole::Server),
            _ => None,
        },
        _ => None,
    }
}

fn external_value_role(parts: &[String]) -> Option<ProgramRole> {
    parts.first().and_then(|part| role_for_namespace(part))
}

fn external_value_path(parts: &[String]) -> Option<String> {
    external_value_role(parts)?;
    (parts.len() > 1).then(|| boon_parser::canonical_value_path(parts))
}

fn external_value_uses_store_root(parts: &[String]) -> bool {
    parts.len() > 2 && parts.get(1).is_some_and(|part| part == "store")
}

fn external_function_role(function: &str) -> Option<ProgramRole> {
    function
        .split_once('/')
        .and_then(|(namespace, _)| role_for_namespace(namespace))
}

fn role_namespace(role: ProgramRole) -> &'static str {
    role.namespace()
}

fn external_data_type_is_closed(ty: &Type) -> bool {
    match ty {
        Type::Text | Type::Number | Type::Bytes(_) => true,
        Type::Object(shape) => {
            !shape.open && shape.fields.values().all(external_data_type_is_closed)
        }
        Type::List(item) => external_data_type_is_closed(item),
        Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            Variant::Tag(_) => true,
            Variant::Tagged { fields, .. } => {
                !fields.open && fields.fields.values().all(external_data_type_is_closed)
            }
        }),
        Type::Skip
        | Type::RenderContract
        | Type::Function { .. }
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

fn external_type_environment_diagnostics(
    environment: &ExternalTypeEnvironment,
) -> Vec<TypeDiagnostic> {
    let mut diagnostics = Vec::new();
    for (path, flow_type) in &environment.values {
        let valid_path = path.split_once('/').is_some_and(|(namespace, suffix)| {
            role_for_namespace(namespace).is_some()
                && !suffix.is_empty()
                && !suffix.split('.').any(str::is_empty)
        });
        if !valid_path {
            diagnostics.push(diagnostic_at_line(
                1,
                format!(
                    "external value `{path}` must use an exact qualified path such as `Server/store.count`"
                ),
            ));
        }
        if flow_type.mode == FlowMode::Absent {
            diagnostics.push(diagnostic_at_line(
                1,
                format!("external value `{path}` cannot be always absent"),
            ));
        }
        if !environment.allow_unresolved && !external_data_type_is_closed(&flow_type.ty) {
            diagnostics.push(diagnostic_at_line(
                1,
                format!(
                    "external value `{path}` must have a closed value type; found {}",
                    boon_facing_type_label(&flow_type.ty)
                ),
            ));
        }
    }
    for (function, signature) in &environment.functions {
        let valid_function = function.split_once('/').is_some_and(|(namespace, suffix)| {
            role_for_namespace(namespace).is_some()
                && !suffix.is_empty()
                && !suffix.split('/').any(str::is_empty)
        });
        if !valid_function {
            diagnostics.push(diagnostic_at_line(
                1,
                format!(
                    "external function `{function}` must use an exact qualified name such as `Server/Module/format`"
                ),
            ));
        }
        if !signature.pure {
            diagnostics.push(diagnostic_at_line(
                1,
                format!("external function `{function}` must be pure"),
            ));
        }
        if signature.result.mode != FlowMode::Continuous {
            diagnostics.push(diagnostic_at_line(
                1,
                format!("external function `{function}` must have a continuous result"),
            ));
        }
        if !environment.allow_unresolved && !external_data_type_is_closed(&signature.result.ty) {
            diagnostics.push(diagnostic_at_line(
                1,
                format!(
                    "external function `{function}` must have a closed result type; found {}",
                    boon_facing_type_label(&signature.result.ty)
                ),
            ));
        }
        let mut names = BTreeSet::new();
        for arg in &signature.args {
            if arg.name.is_empty() || !names.insert(arg.name.as_str()) {
                diagnostics.push(diagnostic_at_line(
                    1,
                    format!(
                        "external function `{function}` has an empty or duplicate argument name `{}`",
                        arg.name
                    ),
                ));
            }
            if !environment.allow_unresolved && !external_data_type_is_closed(&arg.ty) {
                diagnostics.push(diagnostic_at_line(
                    1,
                    format!(
                        "external function `{function}` argument `{}` must have a closed value type; found {}",
                        arg.name,
                        boon_facing_type_label(&arg.ty)
                    ),
                ));
            }
        }
    }
    diagnostics
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
    external_types: ExternalTypeEnvironment,
    vars: TypeVarStore,
    builtins: BuiltinSignatureRegistry,
    render_contracts: RenderContractRegistry,
    source_paths: BTreeSet<String>,
    source_payload_lookup: SourcePayloadPathLookup,
    source_payload_shape_table: Vec<SourcePayloadShapeEntry>,
    source_payload_types: BTreeMap<String, Type>,
    host_port_table: HostPortTable,
    function_statements: BTreeMap<String, &'a AstStatement>,
    function_call_graph: BTreeMap<String, BTreeSet<String>>,
    function_args_by_name: BTreeMap<String, Vec<AstParameter>>,
    function_arg_call_sites: BTreeMap<String, BTreeMap<String, Vec<usize>>>,
    function_arg_display_type_cache: RefCell<BTreeMap<(String, String), Type>>,
    function_return_type_cache: RefCell<BTreeMap<String, Option<Type>>>,
    object_bindings: BTreeMap<String, ObjectShape>,
    name_bindings: BTreeMap<String, Type>,
    declaration_exprs: BTreeMap<String, usize>,
    local_name_bindings: Vec<BTreeMap<String, Type>>,
    flow_bindings: BTreeMap<String, FlowMode>,
    function_param_requirements: BTreeMap<String, BTreeMap<String, Type>>,
    expr_type_vars: BTreeMap<usize, TypeVar>,
    builtin_symbol_exprs: BTreeSet<usize>,
    builtin_static_symbol_exprs: BTreeSet<usize>,
    visited: BTreeSet<usize>,
    expr_type_in_progress: BTreeSet<usize>,
    expr_type_cache: Vec<Option<FlowType>>,
    expr_type_table: ExprTypeTable,
    function_type_table: FunctionTypeTable,
    collect_type_hints: bool,
    render_slot_table: RenderSlotTable,
    constraints: Vec<Constraint>,
    diagnostics: Vec<TypeDiagnostic>,
}

impl<'a> Checker<'a> {
    fn new_profiled(program: &'a ParsedProgram) -> (Self, CheckerInitProfile) {
        Self::new_profiled_with_external_types(program, &ExternalTypeEnvironment::default())
    }

    fn new_profiled_with_external_types(
        program: &'a ParsedProgram,
        external_types: &ExternalTypeEnvironment,
    ) -> (Self, CheckerInitProfile) {
        let checker_init_started = Instant::now();
        let source_paths_started = Instant::now();
        let source_paths = program
            .source_ports
            .iter()
            .map(|source| source.path.clone())
            .collect();
        let source_paths_ms = typecheck_elapsed_ms(source_paths_started);
        let source_payload_lookup = SourcePayloadPathLookup::new(&source_paths);
        let (host_port_table, host_port_diagnostics) =
            host_port_table(program, &source_payload_lookup);
        let source_payload_shape_table_started = Instant::now();
        let source_payload_shape_table = source_payload_shape_table(
            program,
            &source_paths,
            &source_payload_lookup,
            &host_port_table,
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
        let mut function_param_requirements = function_param_requirements(program);
        for (function, requirements) in &external_types.local_function_requirements {
            let target = function_param_requirements
                .entry(function.clone())
                .or_default();
            for (argument, requirement) in requirements {
                match target.get(argument) {
                    Some(existing) if !is_value_placeholder_type(existing) => {}
                    _ => {
                        target.insert(argument.clone(), requirement.clone());
                    }
                }
            }
        }
        let function_param_requirements_ms =
            typecheck_elapsed_ms(function_param_requirements_started);
        let name_bindings_started = Instant::now();
        let mut name_bindings = name_bindings(program, &source_payload_types);
        for (path, flow_type) in &external_types.values {
            name_bindings.insert(path.clone(), flow_type.ty.clone());
        }
        refresh_external_declaration_bindings(
            &program.ast.statements,
            &program.expressions,
            external_types,
            &mut Vec::new(),
            &mut name_bindings,
        );
        let name_bindings_ms = typecheck_elapsed_ms(name_bindings_started);
        let passed_context_started = Instant::now();
        if let Some(passed_type) = passed_context_type(program, &name_bindings) {
            name_bindings.insert("PASSED".to_owned(), passed_type);
        }
        let passed_context_ms = typecheck_elapsed_ms(passed_context_started);
        let flow_bindings_started = Instant::now();
        let flow_bindings = flow_bindings(program, external_types);
        let flow_bindings_ms = typecheck_elapsed_ms(flow_bindings_started);
        let render_contracts_started = Instant::now();
        let render_contracts =
            RenderContractRegistry::default().with_active_root(if scene_root(program).is_some() {
                "scene"
            } else {
                "document"
            });
        let render_contracts_ms = typecheck_elapsed_ms(render_contracts_started);
        let builtin_static_symbol_exprs = builtin_static_symbol_expression_ids(program);
        let builtin_symbol_exprs = program
            .expressions
            .iter()
            .filter_map(|expr| match &expr.kind {
                AstExprKind::Call { args, .. } | AstExprKind::Pipe { args, .. } => Some(args),
                _ => None,
            })
            .flat_map(|args| {
                args.iter()
                    .filter(|arg| arg.is_bare_binding())
                    .map(|arg| arg.value)
            })
            .chain(builtin_static_symbol_exprs.iter().copied())
            .collect();
        let mut checker = Self {
            program,
            external_types: external_types.clone(),
            vars: TypeVarStore::default(),
            builtins: BuiltinSignatureRegistry::default(),
            render_contracts,
            source_paths,
            source_payload_lookup,
            source_payload_shape_table,
            source_payload_types,
            host_port_table,
            function_statements,
            function_call_graph,
            function_args_by_name,
            function_arg_call_sites,
            function_arg_display_type_cache: RefCell::new(BTreeMap::new()),
            function_return_type_cache: RefCell::new(BTreeMap::new()),
            object_bindings,
            name_bindings,
            declaration_exprs: declaration_expression_index(program),
            local_name_bindings: Vec::new(),
            flow_bindings,
            function_param_requirements,
            expr_type_vars: BTreeMap::new(),
            builtin_symbol_exprs,
            builtin_static_symbol_exprs,
            visited: BTreeSet::new(),
            expr_type_in_progress: BTreeSet::new(),
            expr_type_cache: vec![None; program.expressions.len()],
            expr_type_table: ExprTypeTable::default(),
            function_type_table: FunctionTypeTable::default(),
            collect_type_hints: true,
            render_slot_table: RenderSlotTable::default(),
            constraints: Vec::new(),
            diagnostics: external_type_environment_diagnostics(external_types)
                .into_iter()
                .chain(host_port_diagnostics)
                .collect(),
        };
        let refresh_started = Instant::now();
        checker.refresh_static_list_bindings();
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

    fn refresh_static_list_bindings(&mut self) {
        let mut updates = Vec::new();
        self.collect_static_list_binding_updates(
            &self.program.ast.statements,
            &mut Vec::new(),
            &mut updates,
        );
        for (name, path, value_type) in updates {
            self.name_bindings.insert(name, value_type.clone());
            self.name_bindings.insert(path, value_type);
        }
    }

    fn collect_static_list_binding_updates(
        &self,
        statements: &[AstStatement],
        scope: &mut Vec<String>,
        updates: &mut Vec<(String, String, Type)>,
    ) {
        for statement in statements {
            match &statement.kind {
                AstStatementKind::Function { .. } => continue,
                AstStatementKind::List {
                    field: Some(name), ..
                } => {
                    if let Some(value_type) =
                        self.static_list_statement_type(statement, &mut BTreeSet::new())
                        && type_has_known_user_shape(&value_type)
                    {
                        updates.push((name.clone(), scoped_path(scope, name), value_type));
                    }
                }
                AstStatementKind::Field { name } => {
                    scope.push(name.clone());
                    self.collect_static_list_binding_updates(&statement.children, scope, updates);
                    scope.pop();
                    continue;
                }
                _ => {}
            }
            self.collect_static_list_binding_updates(&statement.children, scope, updates);
        }
    }

    fn finish_program_profiled(
        &mut self,
        include_type_hints: bool,
        init_profile: CheckerInitProfile,
    ) -> (CheckOutput, TypeCheckProfile) {
        let trace_typecheck = std::env::var_os("BOON_TYPECHECK_TRACE").is_some();
        let trace_phase = |phase: &str, elapsed_ms: f64| {
            if trace_typecheck {
                eprintln!("boon_typecheck {phase}: {elapsed_ms:.3}ms");
            }
        };
        let total_started = Instant::now();
        self.collect_type_hints = include_type_hints;
        self.check_byte_literal_contexts();
        let recursive_functions_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck recursive_functions:start");
        }
        self.check_recursive_functions();
        let recursive_functions_ms = typecheck_elapsed_ms(recursive_functions_started);
        trace_phase("recursive_functions", recursive_functions_ms);
        let check_statements_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck check_statements:start");
        }
        for statement in &self.program.ast.statements {
            self.check_statement(statement, false);
        }
        self.check_host_effect_calls();
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
        let unresolved_type_variable_count = self.unresolved_type_variable_count();
        let source_payload_shape_table = self.source_payload_shape_table.clone();
        let report_counts_ms = typecheck_elapsed_ms(report_counts_started);
        trace_phase("report_counts", report_counts_ms);
        let type_hint_table_started = Instant::now();
        if trace_typecheck {
            eprintln!("boon_typecheck type_hint_table:start");
        }
        let mut type_hint_table = if include_type_hints {
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
        self.check_host_port_outputs(&output_root_types);
        let named_value_type_table = self.named_value_type_table();
        self.function_type_table
            .entries
            .sort_by(|left, right| left.name.cmp(&right.name));
        let (checked_program, call_diagnostics) = CheckedProgramBuilder::build(
            self.program,
            &self.external_types,
            &self.expr_type_table,
            &self.function_type_table,
            &named_value_type_table,
            &self.render_slot_table,
            &self.builtins,
            &self.render_contracts,
        );
        let resolved_checked_reads = checked_program
            .expressions
            .iter()
            .filter(|expression| {
                matches!(
                    &expression.kind,
                    CheckedExpressionKind::Read { .. } | CheckedExpressionKind::Drain { .. }
                ) && !matches!(
                    expression.flow_type.ty,
                    Type::Unknown | Type::UnresolvedShape { .. }
                )
            })
            .map(|expression| {
                (
                    expression.span.line,
                    expression.span.start,
                    expression.span.end,
                )
            })
            .collect::<BTreeSet<_>>();
        self.diagnostics.retain(|diagnostic| {
            (!diagnostic.message.starts_with("unknown path `")
                && !diagnostic.message.starts_with("unknown identifier `"))
                || !resolved_checked_reads.contains(&(
                    diagnostic.line,
                    diagnostic.start,
                    diagnostic.end,
                ))
        });
        self.diagnostics.extend(call_diagnostics);
        self.expr_type_table.entries = checked_program
            .expressions
            .iter()
            .filter(|expression| expression.id.0 < self.program.expressions.len() as u32)
            .map(|expression| ExprTypeEntry {
                expr_id: expression.id.0 as usize,
                flow_type: expression.flow_type.clone(),
            })
            .collect();
        self.expr_type_table
            .entries
            .sort_by_key(|entry| entry.expr_id);
        for signature in checked_program
            .callables
            .iter()
            .filter(|signature| signature.kind == CheckedCallableKind::User)
        {
            if let Some(entry) = self
                .function_type_table
                .entries
                .iter_mut()
                .find(|entry| entry.name == signature.name)
            {
                entry.arg_types = signature
                    .parameters
                    .iter()
                    .map(|parameter| parameter.flow_type.ty.clone())
                    .collect();
                entry.result = signature.result.clone();
            }
        }
        let unknown_type_count = self
            .expr_type_table
            .entries
            .iter()
            .filter(|entry| matches!(entry.flow_type.ty, Type::Unknown))
            .count();
        if include_type_hints {
            type_hint_table = crate::type_hint_table(
                self.program,
                &self.expr_type_table,
                &self.function_type_table,
                &self.render_slot_table,
                &source_payload_shape_table,
                &self.name_bindings,
            );
        }
        for slot in &mut self.render_slot_table.slots {
            if let Some(statement) = checked_program
                .statements
                .iter()
                .find(|statement| statement.id == CheckedStatementId(slot.slot_statement_id as u32))
            {
                slot.value_expr_id = statement.value.map(|expression| expression.0 as usize);
            }
        }
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
            host_port_table: self.host_port_table.clone(),
            full_document_typecheck_coverage: document_root(self.program).is_none_or(|root| {
                statement_expr_ids(root)
                    .into_iter()
                    .all(|expr_id| self.visited.contains(&expr_id))
            }),
            output_root_types,
            expr_type_table: std::mem::take(&mut self.expr_type_table),
            function_type_table: std::mem::take(&mut self.function_type_table),
            named_value_type_table,
            type_hint_table,
            resolved_constant_table,
            render_slot_table: std::mem::take(&mut self.render_slot_table),
            constraints: std::mem::take(&mut self.constraints),
            diagnostics: std::mem::take(&mut self.diagnostics),
        };
        let program = (!report.has_errors()).then_some(checked_program);
        let assemble_report_ms = typecheck_elapsed_ms(assemble_report_started);
        trace_phase("assemble_report", assemble_report_ms);
        (
            CheckOutput { program, report },
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
        if let AstStatementKind::Function { name, parameters } = &statement.kind {
            let args = parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect::<Vec<_>>();
            let arg_types = parameters
                .iter()
                .map(|parameter| self.function_arg_display_type(name, &parameter.name))
                .collect();
            self.function_type_table.entries.push(FunctionTypeEntry {
                name: name.clone(),
                args,
                arg_types,
                result: self.function_type_hint_result(name),
            });
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
        if next_in_document
            && matches!(
                statement_field(statement).as_deref(),
                Some("root" | "child" | "items" | "children")
            )
        {
            self.check_render_slot(statement);
        }
        let has_function_bindings =
            if let AstStatementKind::Function { name, parameters } = &statement.kind {
                self.local_name_bindings.push(
                    parameters
                        .iter()
                        .map(|parameter| {
                            (
                                parameter.name.clone(),
                                self.function_arg_display_type(name, &parameter.name),
                            )
                        })
                        .collect(),
                );
                true
            } else {
                false
            };
        let when_selector = statement
            .expr
            .and_then(|expr_id| {
                pattern_selector_expr_id(expr_id, &self.program.expressions).map(|input_expr_id| {
                    (
                        expr_id,
                        pipeline_source_expr_id(
                            &self.program.ast.statements,
                            expr_id,
                            input_expr_id,
                            &self.program.expressions,
                        ),
                    )
                })
            })
            .map(|(_, selector_expr_id)| {
                (
                    pattern_selector_path(self.program.expressions.get(selector_expr_id)),
                    self.ensure_expr(selector_expr_id).ty,
                )
            });
        for child in &statement.children {
            let narrowed_bindings =
                when_selector
                    .as_ref()
                    .and_then(|(selector_path, selector_ty)| {
                        let pattern = child
                            .expr
                            .and_then(|expr_id| self.program.expressions.get(expr_id))
                            .and_then(|expr| match &expr.kind {
                                AstExprKind::MatchArm { pattern, .. } => Some(pattern.as_slice()),
                                _ => None,
                            })?;
                        let narrowed = narrowed_pattern_binding(selector_ty, pattern)?;
                        let path = selector_path.as_ref()?;
                        let mut bindings = vec![(path.clone(), narrowed.clone())];
                        if let Some(name) = path.rsplit('.').next()
                            && name != path
                        {
                            bindings.push((name.to_owned(), narrowed));
                        }
                        Some(bindings)
                    });
            let payload_bindings = when_selector.as_ref().and_then(|(_, selector_ty)| {
                let pattern = child
                    .expr
                    .and_then(|expr_id| self.program.expressions.get(expr_id))
                    .and_then(|expr| match &expr.kind {
                        AstExprKind::MatchArm { pattern, .. } => Some(pattern.as_slice()),
                        _ => None,
                    })?;
                let bindings = pattern_payload_bindings(selector_ty, pattern);
                (!bindings.is_empty()).then_some(bindings)
            });
            let saved_arm_bindings = narrowed_bindings.as_ref().map(|bindings| {
                bindings
                    .iter()
                    .map(|(name, ty)| {
                        (
                            name.clone(),
                            self.name_bindings.insert(name.clone(), ty.clone()),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            if let Some(payload_bindings) = payload_bindings.as_ref() {
                self.local_name_bindings.push(payload_bindings.clone());
            }
            self.check_statement(child, next_in_document);
            if payload_bindings.is_some() {
                self.local_name_bindings.pop();
            }
            if let Some(saved_arm_bindings) = saved_arm_bindings {
                for (name, previous) in saved_arm_bindings {
                    if let Some(previous) = previous {
                        self.name_bindings.insert(name, previous);
                    } else {
                        self.name_bindings.remove(&name);
                    }
                }
            }
        }
        if has_function_bindings {
            self.local_name_bindings.pop();
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

    fn named_value_type_table(&self) -> NamedValueTypeTable {
        let mut paths = BTreeSet::new();
        collect_canonical_named_value_paths(
            &self.program.ast.statements,
            &mut Vec::new(),
            &mut paths,
        );
        let mut inferred_types = BTreeMap::new();
        collect_inferred_named_value_types(
            &self.program.ast.statements,
            &self.program.expressions,
            &self.expr_type_cache,
            &mut Vec::new(),
            &mut inferred_types,
        );
        let entries = paths
            .into_iter()
            .map(|path| {
                let descendant_prefix = format!("{path}.");
                let mode = self
                    .flow_bindings
                    .iter()
                    .filter(|(candidate, _)| {
                        candidate.as_str() == path || candidate.starts_with(&descendant_prefix)
                    })
                    .map(|(_, mode)| *mode)
                    .reduce(merge_flow_modes)
                    .unwrap_or(FlowMode::Continuous);
                NamedValueTypeEntry {
                    flow_type: FlowType {
                        mode,
                        ty: inferred_types
                            .get(&path)
                            .cloned()
                            .or_else(|| self.name_bindings.get(&path).cloned())
                            .unwrap_or(Type::Unknown),
                    },
                    path,
                }
            })
            .collect();
        NamedValueTypeTable { entries }
    }

    fn check_host_port_outputs(&mut self, outputs: &[OutputRootTypeEntry]) {
        if let Some(http) = &self.host_port_table.http {
            let Some(output) = outputs
                .iter()
                .find(|output| output.name == http.response_output)
            else {
                self.diagnostics.push(diagnostic_at_line(
                    http.line,
                    format!(
                        "host port `http.response` references missing output root `{}`",
                        http.response_output
                    ),
                ));
                return;
            };
            if !http_response_type_is_valid(&output.ty) {
                self.diagnostics.push(diagnostic_at_line(
                    http.line,
                    format!(
                        "host port `http.response` output `{}` must be exactly `{{ status: Number, body: Bytes }}` or `{{ status: Number, headers: List<{{ name: Text, value: Text|Bytes }}>, body: Bytes }}`; found {}",
                        output.name,
                        boon_facing_type_label(&output.ty)
                    ),
                ));
            }
        }
        if let Some(websocket) = &self.host_port_table.websocket {
            let Some(output) = outputs
                .iter()
                .find(|output| output.name == websocket.actions_output)
            else {
                self.diagnostics.push(diagnostic_at_line(
                    websocket.line,
                    format!(
                        "host port `websocket.actions` references missing output root `{}`",
                        websocket.actions_output
                    ),
                ));
                return;
            };
            if !websocket_actions_type_is_valid(&output.ty) {
                self.diagnostics.push(diagnostic_at_line(
                    websocket.line,
                    format!(
                        "host port `websocket.actions` output `{}` must be a list of closed generic WebSocket action envelopes; found {}",
                        output.name,
                        boon_facing_type_label(&output.ty)
                    ),
                ));
            }
        }
    }

    fn function_arg_display_type(&self, function: &str, arg: &str) -> Type {
        let cache_key = (function.to_owned(), arg.to_owned());
        let cached = self
            .function_arg_display_type_cache
            .borrow()
            .get(&cache_key)
            .cloned();
        let requirement = self
            .function_param_requirements
            .get(function)
            .and_then(|requirements| requirements.get(arg))
            .cloned();
        let checked_call_site = self.function_arg_checked_call_site_type(function, arg);
        let ty = if let Some(ty) = checked_call_site {
            requirement
                .as_ref()
                .map(|requirement| merge_canonical_row_type(&ty, requirement))
                .unwrap_or(ty)
        } else if let Some(cached) = cached {
            cached
        } else if let Some(ty) = self.function_arg_call_site_type(function, arg) {
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

    fn function_arg_checked_call_site_type(&self, function: &str, arg: &str) -> Option<Type> {
        let arg_expr_ids = self.function_arg_call_sites.get(function)?.get(arg)?;
        arg_expr_ids
            .iter()
            .filter_map(|expr_id| {
                self.expr_type_cache
                    .get(*expr_id)
                    .and_then(|entry| entry.as_ref())
                    .map(|flow| flow.ty.clone())
                    .filter(type_has_known_user_shape)
            })
            .reduce(|existing, extra| merge_canonical_row_type(&existing, &extra))
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
        let inferred =
            cached.or_else(|| self.user_function_return_type(function, &mut BTreeSet::new()));
        FlowType {
            mode: FlowMode::Continuous,
            ty: inferred.unwrap_or_else(|| {
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
        let mut ty = None;
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
        let value_expr_id = canonical_statement_value_expression(
            &self.program.ast.statements,
            statement,
            &self.program.expressions,
        );
        let actual_type = value_expr_id
            .map(|expr_id| self.ensure_expr(expr_id).ty)
            .unwrap_or_else(|| {
                if matches!(slot_name.as_str(), "items" | "children") {
                    Type::List(Box::new(open_object_type()))
                } else {
                    open_object_type()
                }
            });
        let mut diagnostics = Vec::new();
        if let Some(expr_id) = value_expr_id
            && !self
                .render_contracts
                .slot_accepts_type(&slot_name, &actual_type)
        {
            let message = if type_contains_skip(&actual_type) {
                "`SKIP` cannot be used as a render value".to_owned()
            } else {
                render_slot_type_error(&slot_name, &actual_type)
            };
            diagnostics.push(self.diagnostic_for_expr(expr_id, message));
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
            let Some(name) = arg.named_name() else {
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
            AstExprKind::ByteLiteral { .. } => Type::Bytes(BytesType::Fixed(1)),
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
            AstExprKind::ListLiteral { items, .. } => {
                let item_type = items
                    .iter()
                    .map(|item| self.ensure_expr(*item).ty)
                    .reduce(|existing, extra| widen_structural_type(&existing, &extra));
                item_type
                    .map(|item| Type::List(Box::new(item)))
                    .or_else(|| {
                        exact_expression_statement(&self.program.ast.statements, expr.id).and_then(
                            |statement| {
                                self.static_list_statement_type(statement, &mut BTreeSet::new())
                            },
                        )
                    })
                    .unwrap_or_else(|| Type::List(Box::new(open_object_type())))
            }
            AstExprKind::Call { function, args, .. }
                if external_function_role(function).is_some() =>
            {
                self.check_external_function_call(expr.id, function, None, args)
            }
            AstExprKind::Call { function, args, .. } => {
                for arg in args {
                    if !builtin_argument_is_symbol(function, arg.named_name()) {
                        self.ensure_expr(arg.value);
                    }
                }
                self.check_bytes_builtin_arguments(expr.id, function, args, None);
                self.check_number_to_text_arguments(expr.id, function, args, false);
                self.check_builtin_call_compatibility(expr.id, function, None, args);
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
                    self.type_for_call_expr(expr.id, function, None, args)
                }
            }
            AstExprKind::Pipe {
                input, op, args, ..
            } if external_function_role(op).is_some() => {
                let input = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr.id,
                    *input,
                    &self.program.expressions,
                );
                self.check_external_function_call(expr.id, op, Some(input), args)
            }
            AstExprKind::Pipe {
                input, op, args, ..
            } => {
                let input_expr_id = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr.id,
                    *input,
                    &self.program.expressions,
                );
                let input_flow = self.ensure_expr(input_expr_id);
                let contextual_body_type = contextual_body_parameter_name(op)
                    .and_then(|body| self.infer_contextual_body_type(&input_flow.ty, args, body));
                let output_bindings = self.legacy_call_output_bindings(op, args, &input_flow.ty);
                for arg in args {
                    if contextual_body_parameter_name(op).is_some()
                        && (arg.is_bare_binding()
                            || arg.named_name() == contextual_body_parameter_name(op))
                    {
                        continue;
                    }
                    if !builtin_argument_is_symbol(op, arg.named_name()) {
                        if !output_bindings.is_empty() {
                            self.local_name_bindings.push(output_bindings.clone());
                        }
                        self.ensure_expr(arg.value);
                        if !output_bindings.is_empty() {
                            self.local_name_bindings.pop();
                        }
                    }
                }
                self.check_bytes_builtin_arguments(expr.id, op, args, Some(input_expr_id));
                self.check_number_to_text_arguments(expr.id, op, args, true);
                self.check_builtin_call_compatibility(expr.id, op, Some(input_expr_id), args);
                if !op.starts_with("Field/") {
                    self.check_user_function_arguments(expr.id, op, Some(input_expr_id), args);
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
                    if let (Some(new_expr_id), Some(item_type)) = (
                        list_map_result_expr_id(
                            &self.program.ast.statements,
                            &self.program.expressions,
                            args,
                        ),
                        contextual_body_type.as_ref(),
                    ) {
                        if type_contains_skip(&item_type) {
                            self.diagnostics.push(self.diagnostic_for_expr(
                                new_expr_id,
                                "`SKIP` cannot be used as a `List/map` item".to_owned(),
                            ));
                        }
                    }
                    let item_type = contextual_body_type.unwrap_or_else(open_object_type);
                    Type::List(Box::new(item_type))
                } else if matches!(op.as_str(), "List/every" | "List/any" | "List/is_not_empty") {
                    true_false_type()
                } else if op == "List/latest" {
                    list_item_type_from_list_type(&input_flow.ty).unwrap_or_else(open_object_type)
                } else if op == "SOURCE" {
                    input_flow.ty
                } else if matches!(op.as_str(), "List/retain" | "List/remove") {
                    input_flow.ty
                } else if op == "List/append" {
                    let append_item = args
                        .iter()
                        .find(|arg| arg.named_name() == Some("item"))
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
                            input_expr_id,
                            "`WHILE` requires a continuous selector".to_owned(),
                        ));
                    }
                    self.when_result_type(expr.id).unwrap_or_else(|| {
                        self.type_for_call_expr(expr.id, op, Some(input_expr_id), args)
                    })
                } else if self.render_contracts.is_render_constructor(op) {
                    self.render_constructor_type_for_args(op, Some(&input_flow), args)
                } else if op == "Bool/not" || op == "Bool/toggle" {
                    self.check_true_false_input(expr, op, &input_flow);
                    true_false_type()
                } else if op == "Bool/and" {
                    self.check_true_false_input(expr, op, &input_flow);
                    for arg in args {
                        let arg_flow = self.ensure_expr(arg.value);
                        self.check_true_false_input(expr, op, &arg_flow);
                    }
                    true_false_type()
                } else if let Some(ty) =
                    self.contextual_bytes_result_type(op, Some(input_expr_id), args)
                {
                    ty
                } else {
                    self.type_for_call_expr(expr.id, op, Some(input_expr_id), args)
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
            AstExprKind::When { input, .. } => {
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
            AstExprKind::Block { bindings, result } => {
                for binding in bindings {
                    self.ensure_expr(binding.value);
                }
                result
                    .map(|result| self.ensure_expr(result).ty)
                    .unwrap_or(Type::Skip)
            }
            AstExprKind::Source => exact_empty_object_type(),
            AstExprKind::Identifier(value) => {
                if value == "BLOCK" {
                    open_object_type()
                } else if self.builtin_static_symbol_exprs.contains(&expr.id) {
                    Type::Text
                } else if self.builtin_symbol_exprs.contains(&expr.id) {
                    Type::Unknown
                } else if self.is_known_function(value) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        expr.id,
                        format!("function `{value}` must be called with parentheses: `{value}()`"),
                    ));
                    self.expr_type_var(expr.id)
                } else if let Some(ty) = self
                    .local_name_bindings
                    .iter()
                    .rev()
                    .find_map(|bindings| bindings.get(value))
                {
                    ty.clone()
                } else if let Some(declaration_expr_id) =
                    declaration_expr_for_path(&self.declaration_exprs, value)
                {
                    let declared = self.ensure_expr(declaration_expr_id).ty;
                    if is_specific_type(&declared) {
                        declared
                    } else {
                        self.name_bindings.get(value).cloned().unwrap_or(declared)
                    }
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
            AstExprKind::Unknown(tokens)
                if tokens.len() == 1 && self.is_known_function(&tokens[0]) =>
            {
                let function = &tokens[0];
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    format!(
                        "function `{function}` must be called with parentheses: `{function}()`"
                    ),
                ));
                self.expr_type_var(expr.id)
            }
            AstExprKind::Unknown(tokens) if unknown_tokens_are_quoted_text(tokens) => Type::Text,
            AstExprKind::Unknown(tokens) => {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    format!("could not infer expression `{}`", tokens.join(" ")),
                ));
                self.expr_type_var(expr.id)
            }
            AstExprKind::Path(parts) if self.builtin_static_symbol_exprs.contains(&expr.id) => {
                Type::Text
            }
            AstExprKind::Path(parts) => self.type_for_path(expr.id, parts),
        };
        FlowType {
            mode: self.flow_mode_for_expr(expr),
            ty,
        }
    }

    fn is_known_function(&self, name: &str) -> bool {
        self.program
            .functions
            .iter()
            .any(|function| function == name)
            || self.external_types.functions.contains_key(name)
            || !matches!(
                self.builtins.type_for_call(name, &self.render_contracts),
                Type::Unknown
            )
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

    fn check_byte_literal_contexts(&mut self) {
        let bytes_items = self
            .program
            .expressions
            .iter()
            .filter_map(|expr| match &expr.kind {
                AstExprKind::BytesLiteral { items, .. } => Some(items.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();
        for expr in &self.program.expressions {
            if matches!(expr.kind, AstExprKind::ByteLiteral { .. })
                && !bytes_items.contains(&expr.id)
            {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    "byte literals are only valid as direct BYTES constructor items".to_owned(),
                ));
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

    fn check_number_to_text_arguments(
        &mut self,
        expr_id: usize,
        function: &str,
        args: &[AstCallArg],
        piped: bool,
    ) {
        if function != "Number/to_text" {
            return;
        }
        let mut names = BTreeSet::new();
        let mut positional_count = 0usize;
        for arg in args {
            match arg.named_name() {
                Some(name)
                    if matches!(
                        name,
                        "value" | "radix" | "min_width" | "signed_width" | "group_size" | "prefix"
                    ) =>
                {
                    if !names.insert(name) {
                        self.diagnostics.push(self.diagnostic_for_expr(
                            arg.value,
                            format!("`Number/to_text` argument `{name}` is duplicated"),
                        ));
                    }
                    if piped && name == "value" {
                        self.diagnostics.push(self.diagnostic_for_expr(
                            arg.value,
                            "piped `Number/to_text` cannot also declare `value`".to_owned(),
                        ));
                    }
                }
                Some(name) => self.diagnostics.push(self.diagnostic_for_expr(
                    arg.value,
                    format!("`Number/to_text` does not accept argument `{name}`"),
                )),
                None => {
                    positional_count += 1;
                    if piped || positional_count > 1 {
                        self.diagnostics.push(self.diagnostic_for_expr(
                            arg.value,
                            "`Number/to_text` accepts one positional value only when it is not piped"
                                .to_owned(),
                        ));
                    }
                }
            }
        }
        if !piped && positional_count == 0 && !names.contains("value") {
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                "`Number/to_text` requires a Number value".to_owned(),
            ));
        }

        for (name, minimum, maximum) in [
            ("radix", 2_i128, 36_i128),
            ("min_width", 0, MAX_NUMBER_TEXT_DIGITS as i128),
            ("signed_width", 1, 63),
            ("group_size", 1, MAX_NUMBER_TEXT_DIGITS as i128),
        ] {
            let Some(value_expr) = named_arg_expr(args, name) else {
                continue;
            };
            if let Some(value) = self.static_integer_literal(value_expr) {
                if !(minimum..=maximum).contains(&value) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        value_expr,
                        format!(
                            "`Number/to_text` {name} must be a whole Number between {minimum} and {maximum}"
                        ),
                    ));
                }
            } else if self
                .program
                .expressions
                .get(value_expr)
                .is_some_and(|expr| matches!(expr.kind, AstExprKind::Number(_)))
            {
                self.diagnostics.push(self.diagnostic_for_expr(
                    value_expr,
                    format!("`Number/to_text` {name} must be a whole Number"),
                ));
            }
        }

        let prefix_enabled = named_arg_expr(args, "prefix")
            .and_then(|expr_id| self.program.expressions.get(expr_id))
            .is_some_and(|expr| matches!(expr.kind, AstExprKind::Bool(true)));
        if prefix_enabled {
            let radix = named_arg_expr(args, "radix")
                .and_then(|expr_id| self.static_integer_literal(expr_id))
                .unwrap_or(10);
            if !matches!(radix, 2 | 8 | 16) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    named_arg_expr(args, "prefix").unwrap_or(expr_id),
                    "`Number/to_text` prefix requires radix 2, 8, or 16".to_owned(),
                ));
            }
        }
    }

    fn check_bytes_builtin_allowed_args(
        &mut self,
        expr_id: usize,
        function: &str,
        args: &[AstCallArg],
        piped: bool,
    ) {
        for arg in args {
            let Some(name) = arg.named_name() else {
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
                    .any(|arg| arg.named_name().is_some_and(|name| name == "input")))
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
            let Some(name) = arg.named_name() else {
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
        if let Some(producer) = external_value_role(parts) {
            let path = external_value_path(parts).expect("external path has a role and suffix");
            if !external_value_uses_store_root(parts) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr_id,
                    format!(
                        "qualified external value `{path}` must use `{}/store.<value>`; role outputs are host boundaries, not distributed application state",
                        role_namespace(producer)
                    ),
                ));
                return self.expr_type_var(expr_id);
            }
            if !self.check_external_role_access(expr_id, producer, &path) {
                return self.expr_type_var(expr_id);
            }
            if let Some(flow_type) = self.external_types.values.get(&path) {
                return flow_type.ty.clone();
            }
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!("unknown qualified external value `{path}`"),
            ));
            return self.expr_type_var(expr_id);
        }
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
        if let Some(ty) = self
            .local_name_bindings
            .iter()
            .rev()
            .find_map(|bindings| type_from_longest_binding_prefix(bindings, parts))
        {
            return ty;
        }
        if let Some(declaration_expr_id) = declaration_expr_for_path(&self.declaration_exprs, &path)
        {
            return self.ensure_expr(declaration_expr_id).ty;
        }
        if let Some(ty) = type_from_longest_binding_prefix(&self.name_bindings, parts) {
            return ty;
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

    fn check_external_role_access(
        &mut self,
        expr_id: usize,
        producer: ProgramRole,
        reference: &str,
    ) -> bool {
        let current = self.external_types.current_role;
        if current == producer {
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!(
                    "same-role qualification `{reference}` is not allowed in {}; use an unqualified local name",
                    role_namespace(current)
                ),
            ));
            return false;
        }
        if !current.can_depend_on(producer) {
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!(
                    "{} cannot depend on {} through `{reference}`",
                    role_namespace(current),
                    role_namespace(producer)
                ),
            ));
            return false;
        }
        true
    }

    fn check_external_function_call(
        &mut self,
        expr_id: usize,
        function: &str,
        pipe_input: Option<usize>,
        call_args: &[AstCallArg],
    ) -> Type {
        let producer = external_function_role(function)
            .expect("external call checker requires a role-qualified function");
        if !self.check_external_role_access(expr_id, producer, function) {
            return self.expr_type_var(expr_id);
        }
        let Some(signature) = self.external_types.functions.get(function).cloned() else {
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!("unknown qualified external function `{function}`"),
            ));
            return self.expr_type_var(expr_id);
        };
        if let Some(input) = pipe_input {
            self.ensure_expr(input);
            self.diagnostics.push(self.diagnostic_for_expr(
                expr_id,
                format!(
                    "external function `{function}` must be called directly with named arguments"
                ),
            ));
        }

        let expected_names = signature
            .args
            .iter()
            .map(|arg| arg.name.as_str())
            .collect::<BTreeSet<_>>();
        let mut actual_by_name = BTreeMap::<String, usize>::new();
        for arg in call_args {
            let actual = self.ensure_expr(arg.value);
            let Some(name) = arg.named_name() else {
                self.diagnostics.push(self.diagnostic_for_expr(
                    arg.value,
                    format!("external function `{function}` requires named arguments"),
                ));
                let _ = actual;
                continue;
            };
            if !expected_names.contains(name) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    arg.value,
                    format!("external function `{function}` has no argument `{name}`"),
                ));
                continue;
            }
            if actual_by_name.insert(name.to_owned(), arg.value).is_some() {
                self.diagnostics.push(self.diagnostic_for_expr(
                    arg.value,
                    format!("external function `{function}` repeats argument `{name}`"),
                ));
            }
        }

        for expected in &signature.args {
            let Some(actual_expr_id) = actual_by_name.get(&expected.name).copied() else {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr_id,
                    format!(
                        "external function `{function}` is missing argument `{}`",
                        expected.name
                    ),
                ));
                continue;
            };
            let mut actual = self.ensure_expr(actual_expr_id);
            if !external_data_type_is_closed(&actual.ty)
                && let Some(static_actual) = self.static_expr_type_for_pipeline_expr(
                    actual_expr_id,
                    &mut BTreeSet::new(),
                    &self.name_bindings,
                )
            {
                actual.ty = static_actual;
            }
            self.constraints.push(Constraint::FlowCompatible {
                actual: actual.clone(),
                expected: FlowType {
                    mode: FlowMode::Continuous,
                    ty: expected.ty.clone(),
                },
            });
            if actual.mode != FlowMode::Continuous {
                self.diagnostics.push(self.diagnostic_for_expr(
                    actual_expr_id,
                    format!(
                        "external function `{function}` argument `{}` must be continuous",
                        expected.name
                    ),
                ));
            }
            self.constraints.push(Constraint::Assignable {
                actual: actual.ty.clone(),
                expected: expected.ty.clone(),
            });
            if !matches!(
                actual.ty,
                Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
            ) && !external_data_type_is_closed(&actual.ty)
            {
                self.diagnostics.push(self.diagnostic_for_expr(
                    actual_expr_id,
                    format!(
                        "external function `{function}` argument `{}` must have a closed value type; found {}",
                        expected.name,
                        boon_facing_type_label(&actual.ty)
                    ),
                ));
            } else if !type_is_assignable_to(&actual.ty, &expected.ty) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    actual_expr_id,
                    format!(
                        "external function `{function}` argument `{}` has incompatible type\nexpected: {}\nfound: {}",
                        expected.name,
                        boon_facing_type_label(&expected.ty),
                        boon_facing_type_label(&actual.ty)
                    ),
                ));
            }
        }
        signature.result.ty
    }

    fn static_type_for_path(&self, parts: &[String]) -> Option<Type> {
        if external_value_role(parts).is_some() {
            if !external_value_uses_store_root(parts) {
                return None;
            }
            let path = external_value_path(parts)?;
            return self
                .external_types
                .values
                .get(&path)
                .map(|flow_type| flow_type.ty.clone());
        }
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
        if let Some(ty) = self
            .local_name_bindings
            .iter()
            .rev()
            .find_map(|bindings| type_from_longest_binding_prefix(bindings, parts))
        {
            return Some(ty);
        }
        type_from_longest_binding_prefix(&self.name_bindings, parts).or_else(|| {
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
            None
        })
    }

    fn type_for_call(&self, function: &str) -> Type {
        if let Some(signature) = self.external_types.functions.get(function) {
            return signature.result.ty.clone();
        }
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

    fn type_for_call_expr(
        &mut self,
        expr_id: usize,
        function: &str,
        pipe_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Type {
        if let Some(ty) = self
            .user_function_return_type_for_call(function, pipe_input, args)
            .filter(is_specific_type)
        {
            return ty;
        }
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

    fn user_function_return_type_for_call(
        &mut self,
        function: &str,
        pipe_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Option<Type> {
        let parameters = self.function_args_by_name.get(function)?.clone();
        let statement = self.function_statements.get(function).copied()?;
        let mut bindings = self.user_function_static_bindings(function);
        for parameter in parameters
            .iter()
            .filter(|parameter| parameter.kind == AstParameterKind::Value)
        {
            let expr_id =
                function_call_argument_expr(&parameters, &parameter.name, pipe_input, args)?;
            bindings.insert(parameter.name.clone(), self.ensure_expr(expr_id).ty);
        }
        let mut active_functions = BTreeSet::from([function.to_owned()]);
        self.function_body_return_type_with_bindings(statement, &mut active_functions, &bindings)
    }

    fn contextual_bytes_result_type(
        &mut self,
        function: &str,
        piped_input: Option<usize>,
        args: &[AstCallArg],
    ) -> Option<Type> {
        match function {
            "Bytes/get" => Some(Type::Bytes(BytesType::Fixed(1))),
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
            .find(|arg| arg.named_name().is_some_and(|name| names.contains(&name)))
            .or_else(|| {
                args.iter()
                    .filter(|arg| arg.is_bare_binding())
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
            let Some(name) = arg.named_name() else {
                continue;
            };
            let ty = self.ensure_expr(arg.value).ty;
            fields.push((name.to_owned(), ty));
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

    fn infer_contextual_body_type(
        &mut self,
        input_type: &Type,
        args: &[AstCallArg],
        body_parameter: &str,
    ) -> Option<Type> {
        let body_expr_id = named_arg_expr(args, body_parameter)?;
        let Some(binding_name) = args
            .iter()
            .find(|arg| arg.is_bare_binding())
            .and_then(|arg| self.program.expressions.get(arg.value))
            .and_then(expr_single_name)
            .map(str::to_owned)
        else {
            return Some(self.ensure_expr(body_expr_id).ty);
        };
        let input_item_type =
            list_item_type_from_list_type(input_type).unwrap_or_else(open_object_type);
        self.local_name_bindings
            .push(BTreeMap::from([(binding_name, input_item_type)]));
        let body_type = self.ensure_expr(body_expr_id).ty;
        self.local_name_bindings.pop();
        Some(body_type)
    }

    fn legacy_call_output_bindings(
        &self,
        function: &str,
        args: &[AstCallArg],
        input_type: &Type,
    ) -> BTreeMap<String, Type> {
        let Some(parameters) = self.function_args_by_name.get(function) else {
            return BTreeMap::new();
        };
        let output_type = list_item_type_from_list_type(input_type).unwrap_or(Type::Unknown);
        parameters
            .iter()
            .filter(|parameter| parameter.kind == AstParameterKind::Out)
            .filter_map(|parameter| {
                let argument = args.iter().find(|argument| {
                    argument.name == parameter.name && argument.is_bare_binding()
                })?;
                let name = self
                    .program
                    .expressions
                    .get(argument.value)
                    .and_then(expr_single_name)?;
                Some((name.to_owned(), output_type.clone()))
            })
            .collect()
    }

    fn when_result_type(&mut self, expr_id: usize) -> Option<Type> {
        let selector_expr_id =
            pattern_selector_expr_id(expr_id, &self.program.expressions).map(|input_expr_id| {
                pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr_id,
                    input_expr_id,
                    &self.program.expressions,
                )
            });
        let selector_path = selector_expr_id.and_then(|selector_expr_id| {
            pattern_selector_path(self.program.expressions.get(selector_expr_id))
        });
        let selector_type =
            selector_expr_id.map(|selector_expr_id| self.ensure_expr(selector_expr_id).ty);
        let arms = when_arms(
            &self.program.ast.statements,
            expr_id,
            &self.program.expressions,
        );
        let mut result: Option<Type> = None;
        for (pattern, arm_expr_id) in arms {
            let narrowed = selector_type
                .as_ref()
                .and_then(|selector_type| narrowed_pattern_binding(selector_type, &pattern));
            let payload_bindings = selector_type
                .as_ref()
                .map(|selector_type| pattern_payload_bindings(selector_type, &pattern))
                .unwrap_or_default();
            let has_payload_bindings = !payload_bindings.is_empty();
            let binding_keys = selector_path
                .as_ref()
                .map(|path| {
                    let mut keys = vec![path.clone()];
                    if let Some(name) = path.rsplit('.').next()
                        && name != path
                    {
                        keys.push(name.to_owned());
                    }
                    keys
                })
                .unwrap_or_default();
            let saved_bindings = narrowed.as_ref().map(|narrowed| {
                binding_keys
                    .iter()
                    .map(|key| {
                        let previous = self.name_bindings.insert(key.clone(), narrowed.clone());
                        (key.clone(), previous)
                    })
                    .collect::<Vec<_>>()
            });
            if has_payload_bindings {
                self.local_name_bindings.push(payload_bindings);
            }
            let arm_type = self.ensure_expr(arm_expr_id).ty;
            if has_payload_bindings {
                self.local_name_bindings.pop();
            }
            if let Some(saved_bindings) = saved_bindings {
                for (key, previous) in saved_bindings {
                    if let Some(previous) = previous {
                        self.name_bindings.insert(key, previous);
                    } else {
                        self.name_bindings.remove(&key);
                    }
                }
            }
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
                ty = widen_hold_type(&ty, &update_type);
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
            AstExprKind::Call { function, args, .. } => {
                if let Some(signature) = self.external_types.functions.get(function) {
                    return Some(signature.result.ty.clone());
                }
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
            AstExprKind::Pipe {
                input, op, args, ..
            } => {
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
                        .find(|arg| arg.named_name() == Some("item"))
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
            AstExprKind::ByteLiteral { .. } => Some(Type::Bytes(BytesType::Fixed(1))),
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
            AstExprKind::ListLiteral { items, .. } => Some(static_list_literal_type(
                items,
                self.program.expressions.as_slice(),
                |item| self.static_expr_type(item, active_functions),
            )),
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
                        ty = widen_hold_type(&ty, &update_type);
                    }
                }
                Some(ty)
            }
            AstExprKind::When { input, .. } => self
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
            .find(|arg| arg.named_name() == Some("new"))
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
            let Some(name) = arg.named_name() else {
                continue;
            };
            let ty = self
                .program
                .expressions
                .get(arg.value)
                .and_then(|expr| self.static_expr_type(expr, active_functions))
                .unwrap_or_else(open_object_type);
            fields.push((name.to_owned(), ty));
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
        self.function_body_return_type_with_bindings(statement, active_functions, &local_bindings)
    }

    fn function_body_return_type_with_bindings(
        &self,
        statement: &AstStatement,
        active_functions: &mut BTreeSet<String>,
        local_bindings: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        if let Some(renderable) = statement.children.iter().find_map(|child| {
            self.static_statement_type_with_bindings(child, active_functions, local_bindings)
                .filter(type_contains_renderable)
        }) {
            return Some(renderable);
        }
        if statements_define_explicit_record(&statement.children, &self.program.expressions) {
            let mut fields = BTreeMap::new();
            let mut field_order = Vec::new();
            self.collect_static_statement_fields_with_bindings(
                &statement.children,
                active_functions,
                local_bindings,
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
        }
        self.static_block_return_type_with_bindings(
            &statement.children,
            active_functions,
            local_bindings,
        )
    }

    fn user_function_static_bindings(&self, function: &str) -> BTreeMap<String, Type> {
        let mut bindings = self.name_bindings.clone();
        if let Some(parameters) = self.function_args_by_name.get(function) {
            for parameter in parameters {
                bindings.insert(
                    parameter.name.clone(),
                    self.function_arg_display_type(function, &parameter.name),
                );
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
                && let Some(ty) = statement
                    .expr
                    .and_then(|expr_id| {
                        statement_pipeline_final_expr_id_containing_expr(
                            statements,
                            expr_id,
                            &self.program.expressions,
                        )
                    })
                    .and_then(|expr_id| {
                        self.static_expr_type_for_pipeline_expr(expr_id, active_functions, bindings)
                    })
                    .filter(is_specific_type)
                    .or_else(|| {
                        self.static_statement_type_with_bindings(
                            statement,
                            active_functions,
                            bindings,
                        )
                    })
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
                && let Some(ty) = statement
                    .expr
                    .and_then(|expr_id| {
                        statement_pipeline_final_expr_id_containing_expr(
                            statements,
                            expr_id,
                            &self.program.expressions,
                        )
                    })
                    .and_then(|expr_id| {
                        self.program
                            .expressions
                            .get(expr_id)
                            .and_then(|expr| self.static_expr_type(expr, active_functions))
                    })
                    .filter(is_specific_type)
                    .or_else(|| self.static_statement_type(statement, active_functions))
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
                | "List/filter"
                | "List/remove"
                | "List/query_prefix"
                | "List/move_field_first"
                | "List/move_field_last"
                | "SOURCE" => ty,
                "List/query" => indexed_query_page_type(),
                "List/count" | "List/sum" => Type::Number,
                "Text/join" => Type::Text,
                "List/append" => {
                    let append_ty = args
                        .iter()
                        .find(|arg| arg.named_name() == Some("item"))
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
        let mut item_type = statement
            .expr
            .and_then(|expr_id| self.program.expressions.get(expr_id))
            .and_then(|expr| match &expr.kind {
                AstExprKind::ListLiteral { items, .. } => items
                    .iter()
                    .filter_map(|item| {
                        self.program
                            .expressions
                            .get(*item)
                            .and_then(|item| self.static_expr_type(item, active_functions))
                    })
                    .reduce(|existing, extra| widen_structural_type(&existing, &extra)),
                _ => None,
            });
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
                if let Some(mode) = flow_binding_mode(&self.flow_bindings, value) {
                    mode
                } else if path_is_source_path(&self.source_paths, value) {
                    FlowMode::PresentOrAbsent
                } else {
                    FlowMode::Continuous
                }
            }
            AstExprKind::Path(parts) => {
                let path = external_value_path(parts).unwrap_or_else(|| parts.join("."));
                if let Some(flow_type) = self.external_types.values.get(&path) {
                    flow_type.mode
                } else if let Some(mode) = flow_binding_mode(&self.flow_bindings, &path) {
                    mode
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
                flow_binding_mode(&self.flow_bindings, &path).unwrap_or(FlowMode::Continuous)
            }
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) if tag == "SKIP" => FlowMode::Absent,
            AstExprKind::Call { function, .. }
                if self.external_types.functions.contains_key(function) =>
            {
                self.external_types.functions[function].result.mode
            }
            AstExprKind::Call { args, .. } => args
                .iter()
                .map(|arg| self.flow_mode_for_expr_id(arg.value))
                .fold(FlowMode::Continuous, merge_flow_modes),
            AstExprKind::Pipe {
                input, op, args, ..
            } => {
                let input = pipeline_source_expr_id(
                    &self.program.ast.statements,
                    expr.id,
                    *input,
                    &self.program.expressions,
                );
                if op == "WHILE" {
                    FlowMode::Continuous
                } else if op == "List/map" || op == "WHEN" {
                    self.flow_mode_for_expr_id(input)
                } else {
                    args.iter()
                        .map(|arg| self.flow_mode_for_expr_id(arg.value))
                        .chain(std::iter::once(self.flow_mode_for_expr_id(input)))
                        .fold(FlowMode::Continuous, merge_flow_modes)
                }
            }
            AstExprKind::When { input, .. } => self.flow_mode_for_expr_id(*input),
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
            let AstExprKind::Pipe {
                input, op, args, ..
            } = &expr.kind
            else {
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
        expr_id: usize,
        function: &str,
        pipe_input: Option<usize>,
        call_args: &[AstCallArg],
    ) {
        if session_info_intrinsic_type(function).is_some() {
            if !session_info_intrinsic_allowed(function, self.external_types.current_role) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr_id,
                    session_info_role_diagnostic(function, self.external_types.current_role),
                ));
            }
            if let Some(input_expr_id) = pipe_input {
                self.diagnostics.push(self.diagnostic_for_expr(
                    input_expr_id,
                    format!("`{function}` does not accept a pipe input"),
                ));
            }
            for arg in call_args {
                self.diagnostics.push(self.diagnostic_for_expr(
                    arg.value,
                    format!("`{function}` does not accept arguments"),
                ));
            }
            return;
        }
        if host_effect_signature(function).is_some() {
            return;
        }

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
            let arg_name = arg.named_name();
            if is_registered_render_constructor(function)
                && arg_name.is_some_and(|name| !render_arg_should_validate_directly(function, name))
            {
                continue;
            }
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
                    let arg_label = arg.named_name().unwrap_or("argument");
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

            let Some(expected) = builtin_argument_expected_type(function, arg.named_name(), piped)
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
            let arg_label = arg.named_name().unwrap_or("argument");
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
        let branch_expr_ids = statement
            .children
            .iter()
            .flat_map(|child| statement_update_value_exprs(child, &self.program.expressions))
            .collect::<Vec<_>>();
        if branch_expr_ids.len() == 1 {
            self.diagnostics.push(
                self.diagnostic_for_expr(
                    expr_id,
                    "`LATEST` merges two or more branches; use its single expression directly"
                        .to_owned(),
                ),
            );
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
        for branch_expr_id in branch_expr_ids {
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
        for arg in args.iter().filter(|arg| arg.named_name() == Some("style")) {
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

    fn check_host_effect_calls(&mut self) {
        for expr in &self.program.expressions {
            let (operation, inline_args, direct_call) = match &expr.kind {
                AstExprKind::Call { function, args, .. } => (function, args.as_slice(), true),
                AstExprKind::Pipe { op, args, .. } => (op, args.as_slice(), false),
                _ => continue,
            };
            let Some(signature) = host_effect_signature(operation) else {
                continue;
            };
            let event_gated = self.expression_is_in_triggered_hold_update(expr.id);
            if !event_gated {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    format!(
                        "typed host effect `{operation}` may only appear in a dependency-triggered `HOLD` update"
                    ),
                ));
            }
            if !direct_call {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    format!(
                        "typed host effect `{operation}` must use direct named-call syntax, not a pipeline"
                    ),
                ));
                continue;
            }

            let arguments = named_call_argument_exprs(self.program, expr.id, inline_args);
            let mut actual = BTreeMap::<&str, usize>::new();
            for (name, value_expr_id) in &arguments {
                if actual.insert(name.as_str(), *value_expr_id).is_some() {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        *value_expr_id,
                        format!("typed host effect `{operation}` repeats argument `{name}`"),
                    ));
                }
            }
            for argument in inline_args
                .iter()
                .filter(|argument| argument.is_bare_binding())
            {
                self.diagnostics.push(self.diagnostic_for_expr(
                    argument.value,
                    format!("typed host effect `{operation}` requires named arguments"),
                ));
            }

            let expected = signature
                .intent_fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<BTreeSet<_>>();
            for name in actual.keys().filter(|name| !expected.contains(**name)) {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    format!("typed host effect `{operation}` has no argument `{name}`"),
                ));
            }
            for (name, value_expr_id) in &arguments {
                let Some(expected_field) = signature
                    .intent_fields
                    .iter()
                    .find(|field| field.name == *name)
                else {
                    continue;
                };
                let actual = self.ensure_expr(*value_expr_id).ty;
                self.constraints.push(Constraint::Assignable {
                    actual: actual.clone(),
                    expected: expected_field.ty.clone(),
                });
                if !type_is_assignable_to(&actual, &expected_field.ty) {
                    self.diagnostics.push(self.diagnostic_for_expr(
                        *value_expr_id,
                        format!(
                            "`{operation}` argument `{name}` has incompatible type\nexpected: {}\nfound: {}",
                            boon_facing_type_label(&expected_field.ty),
                            boon_facing_type_label(&actual)
                        ),
                    ));
                }
            }
            for name in signature
                .intent_fields
                .iter()
                .filter(|field| !field.has_default && !actual.contains_key(field.name.as_str()))
                .map(|field| field.name.as_str())
            {
                self.diagnostics.push(self.diagnostic_for_expr(
                    expr.id,
                    format!(
                        "typed host effect `{operation}` is missing required argument `{name}`"
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

    fn expression_is_in_triggered_hold_update(&self, expr_id: usize) -> bool {
        fn contains(statement: &AstStatement, expr_id: usize, expressions: &[AstExpr]) -> bool {
            statement.expr.is_some_and(|root| {
                root == expr_id || expr_contains_expr_id(root, expr_id, expressions)
            }) || statement
                .children
                .iter()
                .any(|child| contains(child, expr_id, expressions))
        }

        fn visit_hold(statement: &AstStatement, expr_id: usize, expressions: &[AstExpr]) -> bool {
            let is_hold = statement
                .expr
                .and_then(|id| expressions.get(id))
                .is_some_and(|expr| {
                    matches!(expr.kind, AstExprKind::Hold { .. })
                        || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "HOLD")
                });
            if is_hold
                && statement
                    .children
                    .iter()
                    .any(|update| contains(update, expr_id, expressions))
            {
                return true;
            }
            statement
                .children
                .iter()
                .any(|child| visit_hold(child, expr_id, expressions))
        }

        fn branch_contains(
            statements: &[AstStatement],
            root: usize,
            expr_id: usize,
            expressions: &[AstExpr],
        ) -> bool {
            if root == expr_id || expr_contains_expr_id(root, expr_id, expressions) {
                return true;
            }
            statements.iter().any(|statement| {
                let owns_root = statement.expr.is_some_and(|statement_expr| {
                    statement_expr == root
                        || expr_contains_expr_id(statement_expr, root, expressions)
                });
                (owns_root && contains(statement, expr_id, expressions))
                    || branch_contains(&statement.children, root, expr_id, expressions)
            })
        }

        let expressions = &self.program.expressions;
        if !self
            .program
            .ast
            .statements
            .iter()
            .any(|statement| visit_hold(statement, expr_id, expressions))
        {
            return false;
        }

        expressions.iter().any(|trigger| match &trigger.kind {
            AstExprKind::Then {
                input,
                output: Some(output),
            } => {
                matches!(
                    self.flow_mode_for_expr_id(*input),
                    FlowMode::TickPresent | FlowMode::PresentOrAbsent
                ) && branch_contains(&self.program.ast.statements, *output, expr_id, expressions)
            }
            AstExprKind::When { input, .. }
                if matches!(
                    self.flow_mode_for_expr_id(*input),
                    FlowMode::TickPresent | FlowMode::PresentOrAbsent
                ) =>
            {
                when_arms(&self.program.ast.statements, trigger.id, expressions)
                    .into_iter()
                    .any(|(_, output)| {
                        branch_contains(&self.program.ast.statements, output, expr_id, expressions)
                    })
            }
            AstExprKind::Pipe { op, .. } if op == "WHILE" => {
                when_arms(&self.program.ast.statements, trigger.id, expressions)
                    .into_iter()
                    .any(|(_, output)| {
                        branch_contains(&self.program.ast.statements, output, expr_id, expressions)
                    })
            }
            _ => false,
        })
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

fn diagnostic_at_line(line: usize, message: String) -> TypeDiagnostic {
    TypeDiagnostic {
        severity: DiagnosticSeverity::Error,
        line,
        start: 0,
        end: 0,
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
        Type::Text | Type::Number | Type::Bytes(_) | Type::Skip => true,
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

fn http_response_type_is_valid(ty: &Type) -> bool {
    let Type::Object(shape) = ty else {
        return false;
    };
    if shape.open
        || shape.fields.get("status") != Some(&Type::Number)
        || !matches!(shape.fields.get("body"), Some(Type::Bytes(_)))
    {
        return false;
    }
    match shape.fields.len() {
        2 => shape
            .fields
            .keys()
            .all(|name| matches!(name.as_str(), "status" | "body")),
        3 => {
            shape
                .fields
                .keys()
                .all(|name| matches!(name.as_str(), "status" | "headers" | "body"))
                && shape
                    .fields
                    .get("headers")
                    .is_some_and(http_headers_type_is_valid)
        }
        _ => false,
    }
}

fn http_headers_type_is_valid(ty: &Type) -> bool {
    let Type::List(item) = ty else {
        return false;
    };
    let Type::Object(shape) = item.as_ref() else {
        return false;
    };
    !shape.open
        && shape.fields.len() == 2
        && shape.fields.get("name") == Some(&Type::Text)
        && matches!(shape.fields.get("value"), Some(Type::Text | Type::Bytes(_)))
}

fn websocket_actions_type_is_valid(ty: &Type) -> bool {
    let Type::List(item) = ty else {
        return false;
    };
    let Type::Object(shape) = item.as_ref() else {
        return false;
    };
    if shape.open || shape.fields.len() != 12 {
        return false;
    }
    let expected = BTreeMap::from([
        ("body_bytes".to_owned(), Type::Bytes(BytesType::Dynamic)),
        ("body_kind".to_owned(), Type::Text),
        ("body_text".to_owned(), Type::Text),
        ("bytes".to_owned(), Type::Bytes(BytesType::Dynamic)),
        ("code".to_owned(), Type::Number),
        ("frame_kind".to_owned(), Type::Text),
        ("include_current".to_owned(), true_false_type()),
        ("kind".to_owned(), Type::Text),
        ("reason".to_owned(), Type::Text),
        ("room".to_owned(), Type::Text),
        ("status".to_owned(), Type::Number),
        ("text".to_owned(), Type::Text),
    ]);
    shape.fields == expected
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

fn function_call_argument_expr(
    function_parameters: &[AstParameter],
    parameter: &str,
    pipe_input: Option<usize>,
    call_args: &[AstCallArg],
) -> Option<usize> {
    let position = function_parameters
        .iter()
        .position(|candidate| candidate.name == parameter)?;
    let first_value_position = function_parameters
        .iter()
        .position(|candidate| candidate.kind == AstParameterKind::Value)?;
    if position == first_value_position
        && let Some(input) = pipe_input
    {
        return Some(input);
    }
    call_args
        .iter()
        .find(|arg| arg.named_name() == Some(parameter))
        .map(|arg| arg.value)
}

fn function_statement_map(statements: &[AstStatement]) -> BTreeMap<String, &AstStatement> {
    let mut functions = BTreeMap::new();
    collect_function_statements(statements, &mut functions);
    functions
}

fn function_args_by_statement_map(
    function_statements: &BTreeMap<String, &AstStatement>,
) -> BTreeMap<String, Vec<AstParameter>> {
    function_statements
        .iter()
        .filter_map(|(name, statement)| {
            let AstStatementKind::Function { parameters, .. } = &statement.kind else {
                return None;
            };
            Some((name.clone(), parameters.clone()))
        })
        .collect()
}

fn function_arg_call_site_index(
    program: &ParsedProgram,
    function_args_by_name: &BTreeMap<String, Vec<AstParameter>>,
) -> BTreeMap<String, BTreeMap<String, Vec<usize>>> {
    let mut index: BTreeMap<String, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
    for expr in &program.expressions {
        let (function, pipe_input, call_args) = match &expr.kind {
            AstExprKind::Call { function, args, .. } => (function, None, args.as_slice()),
            AstExprKind::Pipe {
                input, op, args, ..
            } => (op, Some(*input), args.as_slice()),
            _ => continue,
        };
        let Some(function_args) = function_args_by_name.get(function) else {
            continue;
        };
        for parameter in function_args {
            let Some(arg_expr_id) =
                function_call_argument_expr(function_args, &parameter.name, pipe_input, call_args)
            else {
                continue;
            };
            index
                .entry(function.clone())
                .or_default()
                .entry(parameter.name.clone())
                .or_default()
                .push(arg_expr_id);
        }
    }
    collect_multiline_function_arg_call_sites(
        &program.ast.statements,
        &program.expressions,
        function_args_by_name,
        &mut index,
    );
    for parameters in index.values_mut() {
        for call_sites in parameters.values_mut() {
            call_sites.sort_unstable();
            call_sites.dedup();
        }
    }
    index
}

fn collect_multiline_function_arg_call_sites(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    function_args_by_name: &BTreeMap<String, Vec<AstParameter>>,
    index: &mut BTreeMap<String, BTreeMap<String, Vec<usize>>>,
) {
    for statement in statements {
        if let Some(expr) = statement.expr.and_then(|expr_id| expressions.get(expr_id)) {
            let function = match &expr.kind {
                AstExprKind::Call { function, .. } => Some(function),
                AstExprKind::Pipe { op, .. } => Some(op),
                _ => None,
            };
            if let Some(function) = function
                && let Some(parameters) = function_args_by_name.get(function)
            {
                for parameter in parameters {
                    let value = statement.children.iter().find_map(|child| {
                        (statement_field(child).as_deref() == Some(parameter.name.as_str()))
                            .then(|| direct_statement_value_expr_id(child, expressions))
                            .flatten()
                    });
                    if let Some(value) = value {
                        index
                            .entry(function.clone())
                            .or_default()
                            .entry(parameter.name.clone())
                            .or_default()
                            .push(value);
                    }
                }
            }
        }
        collect_multiline_function_arg_call_sites(
            &statement.children,
            expressions,
            function_args_by_name,
            index,
        );
    }
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
        AstExprKind::Call { function, args, .. } => {
            if user_functions.contains(function) {
                calls.insert(function.clone());
            }
            for arg in args {
                collect_expr_user_function_calls(arg.value, expressions, user_functions, calls);
            }
        }
        AstExprKind::Pipe {
            input, op, args, ..
        } => {
            collect_expr_user_function_calls(*input, expressions, user_functions, calls);
            if user_functions.contains(op) {
                calls.insert(op.clone());
            }
            for arg in args {
                collect_expr_user_function_calls(arg.value, expressions, user_functions, calls);
            }
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
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
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                collect_expr_user_function_calls(binding.value, expressions, user_functions, calls);
            }
            if let Some(result) = result {
                collect_expr_user_function_calls(*result, expressions, user_functions, calls);
            }
        }
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
        && !expr_is_pipeline_continuation(expr_ids[0], expressions)
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
        | Some(AstExprKind::When { input, .. })
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
        | AstExprKind::When { input, .. }
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
        let owns_structural_body = statement.expr.is_some_and(|expr_id| {
            expressions.get(expr_id).is_some_and(|expression| {
                matches!(
                    expression.kind,
                    AstExprKind::MatchArm { .. } | AstExprKind::Then { .. }
                )
            })
        });
        if !owns_structural_body
            && let Some(expr_ids) = statement_pipeline_expr_ids(statement, expressions)
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

fn list_map_new_expr_id(args: &[AstCallArg]) -> Option<usize> {
    args.iter()
        .find(|arg| arg.named_name() == Some("new"))
        .map(|arg| arg.value)
}

fn list_map_result_expr_id(
    _statements: &[AstStatement],
    _expressions: &[AstExpr],
    args: &[AstCallArg],
) -> Option<usize> {
    list_map_new_expr_id(args)
}

fn named_arg_expr(args: &[AstCallArg], name: &str) -> Option<usize> {
    args.iter()
        .find(|arg| arg.named_name() == Some(name))
        .map(|arg| arg.value)
}

fn contextual_body_parameter_name(function: &str) -> Option<&'static str> {
    match function {
        "List/map" => Some("new"),
        "List/filter" | "List/retain" | "List/every" | "List/any" | "List/find" => Some("if"),
        "List/remove" => Some("when"),
        _ => None,
    }
}

fn has_any_named_arg(args: &[AstCallArg], names: &[&str]) -> bool {
    args.iter()
        .any(|arg| arg.named_name().is_some_and(|name| names.contains(&name)))
}

fn has_unnamed_arg(args: &[AstCallArg]) -> bool {
    args.iter().any(|arg| arg.is_bare_binding())
}

fn pattern_selector_expr_id(expr_id: usize, expressions: &[AstExpr]) -> Option<usize> {
    match &expressions.get(expr_id)?.kind {
        AstExprKind::When { input, .. } => Some(*input),
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

fn pattern_selector_path(expr: Option<&AstExpr>) -> Option<String> {
    match &expr?.kind {
        AstExprKind::Identifier(value) => Some(value.clone()),
        AstExprKind::Path(parts) => Some(parts.join(".")),
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
        AstExprKind::ByteLiteral { .. } => None,
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
    entries: BTreeMap<&'static str, BuiltinSignatureEntry>,
}

#[derive(Clone, Debug)]
struct BuiltinSignatureEntry {
    result: Type,
    callable: Option<AuthoritativeCallableSignature>,
}

fn required_parameter(name: &str, ty: Type) -> AuthoritativeParameter {
    AuthoritativeParameter {
        name: name.to_owned(),
        kind: CheckedParameterKind::Value,
        flow_type: continuous_flow_type(ty),
        required: true,
    }
}

fn optional_parameter(name: &str, ty: Type) -> AuthoritativeParameter {
    AuthoritativeParameter {
        required: false,
        ..required_parameter(name, ty)
    }
}

fn output_parameter(name: &str, ty: Type) -> AuthoritativeParameter {
    AuthoritativeParameter {
        name: name.to_owned(),
        kind: CheckedParameterKind::Out,
        flow_type: continuous_flow_type(ty),
        required: true,
    }
}

const CONTEXTUAL_ITEM_VAR: TypeVar = TypeVar(0);
const CONTEXTUAL_RESULT_VAR: TypeVar = TypeVar(1);

fn contextual_item_type() -> Type {
    Type::Var(CONTEXTUAL_ITEM_VAR)
}

fn contextual_result_type() -> Type {
    Type::Var(CONTEXTUAL_RESULT_VAR)
}

fn found_or_not_found_type(item: Type) -> Type {
    Type::VariantSet(vec![
        Variant::Tagged {
            tag: "Found".to_owned(),
            fields: ObjectShape::from_ordered_fields([("value".to_owned(), item)], false),
        },
        Variant::Tag("NotFound".to_owned()),
    ])
}

impl Default for BuiltinSignatureRegistry {
    fn default() -> Self {
        let mut entries = BTreeMap::new();
        let mut register =
            |name: &'static str,
             result: Type,
             parameters: Vec<AuthoritativeParameter>,
             contextual_builtin: Option<ContextualBuiltinKind>| {
                entries.insert(
                    name,
                    BuiltinSignatureEntry {
                        result: result.clone(),
                        callable: Some(AuthoritativeCallableSignature {
                            parameters,
                            result: continuous_flow_type(result),
                            effect: CheckedEffectSummary::default(),
                            contextual_builtin,
                        }),
                    },
                );
            };

        register("Text/empty", Type::Text, Vec::new(), None);
        register("Text/space", Type::Text, Vec::new(), None);
        for name in ["Text/trim", "Text/to_lowercase", "Text/to_uppercase"] {
            register(
                name,
                Type::Text,
                vec![required_parameter("input", Type::Text)],
                None,
            );
        }
        register(
            "Text/concat",
            Type::Text,
            vec![
                required_parameter("input", Type::Unknown),
                required_parameter("with", Type::Unknown),
                optional_parameter("separator", Type::Unknown),
            ],
            None,
        );
        register(
            "Text/time_range_label",
            Type::Text,
            vec![
                required_parameter("input", Type::Unknown),
                required_parameter("end", Type::Unknown),
                required_parameter("unit", Type::Unknown),
            ],
            None,
        );
        register(
            "Text/substring",
            Type::Text,
            vec![
                required_parameter("input", Type::Text),
                required_parameter("start", Type::Number),
                required_parameter("length", Type::Number),
            ],
            None,
        );
        register(
            "Number/to_text",
            Type::Text,
            vec![
                required_parameter("value", Type::Number),
                optional_parameter("radix", Type::Number),
                optional_parameter("min_width", Type::Number),
                optional_parameter("signed_width", Type::Number),
                optional_parameter("group_size", Type::Number),
                optional_parameter("prefix", true_false_type()),
            ],
            None,
        );
        for name in ["Number/to_codepoint_text", "Number/to_ascii_text"] {
            register(
                name,
                Type::Text,
                vec![required_parameter("value", Type::Number)],
                None,
            );
        }
        register(
            "Text/join",
            Type::Text,
            vec![
                required_parameter("texts", Type::List(Box::new(Type::Text))),
                optional_parameter("separator", Type::Text),
                optional_parameter("empty", Type::Text),
            ],
            None,
        );
        register(
            "Error/text",
            Type::Text,
            vec![required_parameter("value", Type::Unknown)],
            None,
        );
        register("Ulid/generate", Type::Text, Vec::new(), None);
        register(
            "Bytes/to_text",
            Type::Text,
            vec![
                required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                required_parameter("encoding", Type::Unknown),
            ],
            None,
        );
        for name in ["Bytes/to_hex", "Bytes/to_base64"] {
            register(
                name,
                Type::Text,
                vec![required_parameter("input", Type::Bytes(BytesType::Dynamic))],
                None,
            );
        }
        register(
            "File/read_text",
            Type::Text,
            vec![required_parameter("path", Type::Text)],
            None,
        );
        for name in ["Log/error", "Log/info"] {
            register(
                name,
                Type::Text,
                vec![required_parameter("input", Type::Text)],
                None,
            );
        }

        for name in ["Number/add", "Number/subtract", "Number/min", "Number/max"] {
            register(
                name,
                Type::Number,
                vec![
                    required_parameter("left", Type::Number),
                    required_parameter("right", Type::Number),
                ],
                None,
            );
        }
        for name in [
            "Number/bit_width",
            "Number/ceil",
            "Number/floor",
            "Number/round",
            "Number/truncate",
        ] {
            register(
                name,
                Type::Number,
                vec![required_parameter("value", Type::Number)],
                None,
            );
        }
        register(
            "Number/interpolate",
            Type::Number,
            ["start", "end", "numerator", "denominator", "fallback"]
                .into_iter()
                .map(|name| required_parameter(name, Type::Number))
                .collect(),
            None,
        );
        register(
            "Number/project_offset",
            Type::Number,
            [
                "time",
                "viewport_start",
                "viewport_end",
                "canvas_width",
                "fallback",
            ]
            .into_iter()
            .map(|name| required_parameter(name, Type::Number))
            .chain([optional_parameter("zoom", Type::Unknown)])
            .collect(),
            None,
        );
        register(
            "Number/project_time",
            Type::Number,
            [
                "pointer_x",
                "pointer_width",
                "viewport_start",
                "viewport_end",
                "fallback",
            ]
            .into_iter()
            .map(|name| required_parameter(name, Type::Number))
            .collect(),
            None,
        );
        register(
            "Number/project_width",
            Type::Number,
            [
                "start_time",
                "end_time",
                "viewport_start",
                "viewport_end",
                "canvas_width",
                "fallback",
            ]
            .into_iter()
            .map(|name| required_parameter(name, Type::Number))
            .chain([optional_parameter("zoom", Type::Unknown)])
            .collect(),
            None,
        );
        for name in ["List/count", "List/length", "List/sum"] {
            register(
                name,
                Type::Number,
                vec![required_parameter(
                    "list",
                    Type::List(Box::new(open_object_type())),
                )],
                None,
            );
        }
        register(
            "Text/find",
            Type::Number,
            vec![
                required_parameter("input", Type::Text),
                required_parameter("needle", Type::Text),
            ],
            None,
        );
        register(
            "Text/length",
            Type::Number,
            vec![required_parameter("input", Type::Text)],
            None,
        );
        register(
            "Text/to_number",
            Type::Number,
            vec![
                required_parameter("input", Type::Text),
                optional_parameter("radix", Type::Number),
                optional_parameter("leading", true_false_type()),
                optional_parameter("fallback", Type::Number),
            ],
            None,
        );
        register(
            "Bytes/length",
            Type::Number,
            vec![required_parameter("input", Type::Bytes(BytesType::Dynamic))],
            None,
        );
        register(
            "Bytes/find",
            Type::Number,
            vec![
                required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                required_parameter("needle", Type::Bytes(BytesType::Dynamic)),
            ],
            None,
        );
        for name in ["Bytes/read_unsigned", "Bytes/read_signed"] {
            register(
                name,
                Type::Number,
                vec![
                    required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                    required_parameter("offset", Type::Number),
                    required_parameter("byte_count", Type::Number),
                    required_parameter("endian", Type::Unknown),
                ],
                None,
            );
        }

        register(
            "Bytes/get",
            Type::Bytes(BytesType::Dynamic),
            vec![
                required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                required_parameter("index", Type::Number),
            ],
            None,
        );
        register(
            "Bytes/set",
            Type::Bytes(BytesType::Dynamic),
            vec![
                required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                required_parameter("index", Type::Number),
                required_parameter("value", Type::Bytes(BytesType::Fixed(1))),
            ],
            None,
        );
        register(
            "Bytes/slice",
            Type::Bytes(BytesType::Dynamic),
            vec![
                required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                required_parameter("offset", Type::Number),
                required_parameter("byte_count", Type::Number),
            ],
            None,
        );
        for name in ["Bytes/take", "Bytes/drop"] {
            register(
                name,
                Type::Bytes(BytesType::Dynamic),
                vec![
                    required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                    required_parameter("byte_count", Type::Number),
                ],
                None,
            );
        }
        register(
            "Bytes/concat",
            Type::Bytes(BytesType::Dynamic),
            vec![
                required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                required_parameter("with", Type::Bytes(BytesType::Dynamic)),
            ],
            None,
        );
        register(
            "Bytes/zeros",
            Type::Bytes(BytesType::Dynamic),
            vec![required_parameter("byte_count", Type::Number)],
            None,
        );
        register(
            "Text/to_bytes",
            Type::Bytes(BytesType::Dynamic),
            vec![
                required_parameter("input", Type::Text),
                required_parameter("encoding", Type::Unknown),
            ],
            None,
        );
        for name in ["Bytes/from_hex", "Bytes/from_base64"] {
            register(
                name,
                Type::Bytes(BytesType::Dynamic),
                vec![required_parameter("input", Type::Text)],
                None,
            );
        }
        for name in ["Bytes/write_unsigned", "Bytes/write_signed"] {
            register(
                name,
                Type::Bytes(BytesType::Dynamic),
                vec![
                    required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                    required_parameter("offset", Type::Number),
                    required_parameter("byte_count", Type::Number),
                    required_parameter("endian", Type::Unknown),
                    required_parameter("value", Type::Number),
                ],
                None,
            );
        }

        register(
            "Bool/not",
            true_false_type(),
            vec![required_parameter("value", true_false_type())],
            None,
        );
        register(
            "Bool/and",
            true_false_type(),
            vec![
                required_parameter("left", true_false_type()),
                required_parameter("right", true_false_type()),
            ],
            None,
        );
        register(
            "Bool/toggle",
            true_false_type(),
            vec![
                required_parameter("value", true_false_type()),
                required_parameter("when", Type::Unknown),
            ],
            None,
        );
        for name in ["Text/is_empty", "Text/is_not_empty"] {
            register(
                name,
                true_false_type(),
                vec![required_parameter("input", Type::Text)],
                None,
            );
        }
        for (name, argument) in [
            ("Text/starts_with", "prefix"),
            ("Text/contains", "needle"),
            ("Text/all_chars_in", "chars"),
        ] {
            register(
                name,
                true_false_type(),
                vec![
                    required_parameter("input", Type::Text),
                    required_parameter(argument, Type::Text),
                ],
                None,
            );
        }
        register(
            "List/is_not_empty",
            true_false_type(),
            vec![required_parameter(
                "list",
                Type::List(Box::new(open_object_type())),
            )],
            None,
        );
        register(
            "Bytes/is_empty",
            true_false_type(),
            vec![required_parameter("input", Type::Bytes(BytesType::Dynamic))],
            None,
        );
        register(
            "Bytes/equal",
            true_false_type(),
            vec![
                required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                required_parameter("with", Type::Bytes(BytesType::Dynamic)),
            ],
            None,
        );
        for (name, argument) in [
            ("Bytes/starts_with", "prefix"),
            ("Bytes/ends_with", "suffix"),
        ] {
            register(
                name,
                true_false_type(),
                vec![
                    required_parameter("input", Type::Bytes(BytesType::Dynamic)),
                    required_parameter(argument, Type::Bytes(BytesType::Dynamic)),
                ],
                None,
            );
        }

        let list_type = || Type::List(Box::new(open_object_type()));
        for (name, body, operation, body_type, result) in [
            (
                "List/map",
                "new",
                ContextualBuiltinKind::Map,
                contextual_result_type(),
                Type::List(Box::new(contextual_result_type())),
            ),
            (
                "List/filter",
                "if",
                ContextualBuiltinKind::Filter,
                true_false_type(),
                Type::List(Box::new(contextual_item_type())),
            ),
            (
                "List/retain",
                "if",
                ContextualBuiltinKind::Retain,
                true_false_type(),
                Type::List(Box::new(contextual_item_type())),
            ),
            (
                "List/every",
                "if",
                ContextualBuiltinKind::Every,
                true_false_type(),
                true_false_type(),
            ),
            (
                "List/any",
                "if",
                ContextualBuiltinKind::Any,
                true_false_type(),
                true_false_type(),
            ),
            (
                "List/find",
                "if",
                ContextualBuiltinKind::Find,
                true_false_type(),
                found_or_not_found_type(contextual_item_type()),
            ),
        ] {
            register(
                name,
                result,
                vec![
                    required_parameter("list", Type::List(Box::new(contextual_item_type()))),
                    output_parameter("item", contextual_item_type()),
                    required_parameter(body, body_type),
                ],
                Some(operation),
            );
        }
        register(
            "List/append",
            list_type(),
            vec![
                required_parameter("list", list_type()),
                required_parameter("item", Type::Unknown),
            ],
            None,
        );
        register(
            "List/range",
            Type::List(Box::new(Type::Number)),
            vec![
                required_parameter("from", Type::Number),
                required_parameter("to", Type::Number),
            ],
            None,
        );
        register(
            "List/chunk",
            Type::List(Box::new(Type::Object(ObjectShape::from_ordered_fields(
                [
                    ("label".to_owned(), Type::Text),
                    (
                        "items".to_owned(),
                        Type::List(Box::new(contextual_item_type())),
                    ),
                ],
                false,
            )))),
            vec![
                required_parameter("list", Type::List(Box::new(contextual_item_type()))),
                required_parameter("size", Type::Number),
            ],
            None,
        );
        register(
            "List/query_prefix",
            list_type(),
            vec![
                required_parameter("list", list_type()),
                required_parameter("field", Type::Text),
                required_parameter("prefix", Type::Text),
                required_parameter("limit", Type::Number),
                required_parameter("normalization", Type::Text),
            ],
            None,
        );
        register(
            "List/get",
            open_object_type(),
            vec![
                required_parameter("list", list_type()),
                required_parameter("index", Type::Number),
            ],
            None,
        );
        register(
            "List/latest",
            open_object_type(),
            vec![required_parameter("list", list_type())],
            None,
        );
        register(
            "Timer/interval",
            open_object_type(),
            vec![required_parameter("duration", Type::Unknown)],
            None,
        );
        register(
            "Error/new",
            Type::VariantSet(vec![Variant::Tagged {
                tag: "Error".to_owned(),
                fields: ObjectShape::new(BTreeMap::new(), true),
            }]),
            vec![optional_parameter("code", Type::Text)],
            None,
        );

        register("Router/route", Type::Text, Vec::new(), None);
        register(
            "Router/go_to",
            Type::Text,
            vec![required_parameter("route", Type::Text)],
            None,
        );
        register(
            "List/remove",
            list_type(),
            vec![
                required_parameter("list", list_type()),
                output_parameter("item", open_object_type()),
                required_parameter("when", true_false_type()),
            ],
            None,
        );
        register(
            "List/query",
            indexed_query_page_type(),
            vec![
                required_parameter("list", list_type()),
                required_parameter("fields", Type::Text),
                required_parameter("normalization", Type::Text),
                optional_parameter("multi_value", Type::Text),
                required_parameter("select", Type::Unknown),
                optional_parameter("key", Type::Unknown),
                optional_parameter("leading", Type::Unknown),
                optional_parameter("prefix", Type::Text),
                optional_parameter("lower", Type::Unknown),
                optional_parameter("upper", Type::Unknown),
                optional_parameter("lower_inclusive", true_false_type()),
                optional_parameter("upper_inclusive", true_false_type()),
                optional_parameter("keys", Type::Unknown),
                required_parameter("limit", Type::Number),
                optional_parameter("unique", true_false_type()),
                required_parameter("order", Type::Unknown),
                required_parameter("residual", Type::Unknown),
                optional_parameter("residual_field", Type::Text),
                optional_parameter("residual_value", Type::Unknown),
                optional_parameter("needle", Type::Text),
                optional_parameter("minimum", Type::Number),
                optional_parameter("maximum", Type::Number),
                optional_parameter("latitude_field", Type::Text),
                optional_parameter("longitude_field", Type::Text),
                optional_parameter("center_latitude", Type::Number),
                optional_parameter("center_longitude", Type::Number),
                optional_parameter("radius_meters", Type::Number),
                optional_parameter("cursor", Type::Bytes(BytesType::Dynamic)),
            ],
            None,
        );

        let directional_light = Type::Object(ObjectShape::from_ordered_fields(
            [
                ("azimuth".to_owned(), Type::Number),
                ("altitude".to_owned(), Type::Number),
                ("spread".to_owned(), Type::Number),
                ("intensity".to_owned(), Type::Number),
                ("color".to_owned(), Type::Unknown),
            ],
            false,
        ));
        register(
            "Light/directional",
            directional_light,
            vec![
                required_parameter("azimuth", Type::Number),
                required_parameter("altitude", Type::Number),
                required_parameter("spread", Type::Number),
                required_parameter("intensity", Type::Number),
                required_parameter("color", Type::Unknown),
            ],
            None,
        );
        let ambient_light = Type::Object(ObjectShape::from_ordered_fields(
            [
                ("intensity".to_owned(), Type::Number),
                ("color".to_owned(), Type::Unknown),
            ],
            false,
        ));
        register(
            "Light/ambient",
            ambient_light,
            vec![
                required_parameter("intensity", Type::Number),
                required_parameter("color", Type::Unknown),
            ],
            None,
        );
        let spot_light = Type::Object(ObjectShape::from_ordered_fields(
            [
                ("target".to_owned(), Type::Unknown),
                ("color".to_owned(), Type::Unknown),
                ("intensity".to_owned(), Type::Number),
                ("radius".to_owned(), Type::Number),
                ("softness".to_owned(), Type::Number),
            ],
            false,
        ));
        register(
            "Light/spot",
            spot_light,
            vec![
                required_parameter("target", Type::Unknown),
                required_parameter("color", Type::Unknown),
                required_parameter("intensity", Type::Number),
                required_parameter("radius", Type::Number),
                required_parameter("softness", Type::Number),
            ],
            None,
        );

        drop(register);
        let stateful_effect = CheckedEffectSummary {
            reads_state: true,
            writes_state: true,
            ..CheckedEffectSummary::default()
        };
        for name in ["Bool/toggle"] {
            let callable = entries
                .get_mut(name)
                .and_then(|entry| entry.callable.as_mut())
                .expect("registered stateful builtin");
            callable.effect = stateful_effect;
        }
        let source_effect = CheckedEffectSummary {
            emits_source: true,
            ..CheckedEffectSummary::default()
        };
        for name in ["Timer/interval"] {
            let callable = entries
                .get_mut(name)
                .and_then(|entry| entry.callable.as_mut())
                .expect("registered source-emitting builtin");
            callable.effect = source_effect;
        }
        for (name, result) in [
            ("List/move_field_first", list_type()),
            ("List/move_field_last", list_type()),
            ("Widget/table", open_object_type()),
            ("Widget/selected", open_object_type()),
            ("Widget/rows", open_object_type()),
        ] {
            entries.insert(
                name,
                BuiltinSignatureEntry {
                    result,
                    callable: None,
                },
            );
        }

        Self { entries }
    }
}

impl BuiltinSignatureRegistry {
    fn authoritative_signatures(
        &self,
    ) -> impl Iterator<Item = (&str, AuthoritativeCallableSignature)> + '_ {
        self.entries
            .iter()
            .filter_map(|(name, entry)| entry.callable.clone().map(|signature| (*name, signature)))
    }

    fn type_for_call(&self, function: &str, render_contracts: &RenderContractRegistry) -> Type {
        if let Some(intrinsic_type) = session_info_intrinsic_type(function) {
            intrinsic_type
        } else if let Some(signature) = host_effect_signature(function) {
            signature.result_type
        } else if let Some(entry) = self.entries.get(function) {
            entry.result.clone()
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
            "MapViewport",
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
        .with_fixed_constructor("Element/map", "MapViewport")
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
            "MapViewport",
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
        .with_fixed_constructor("Scene/Element/map", "MapViewport")
    }
}

impl RenderContractRegistry {
    fn authoritative_signatures(
        &self,
    ) -> impl Iterator<Item = (&str, AuthoritativeCallableSignature)> + '_ {
        self.roots
            .values()
            .flat_map(|root| root.constructors.keys().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|function| {
                let parameters = render_constructor_parameters(function)?;
                Some((
                    function,
                    AuthoritativeCallableSignature {
                        parameters,
                        result: continuous_flow_type(
                            self.constructor_shape(function, BTreeMap::new()),
                        ),
                        effect: CheckedEffectSummary::default(),
                        contextual_builtin: None,
                    },
                ))
            })
    }

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

    fn slot_accepts_type(&self, slot_name: &str, ty: &Type) -> bool {
        match slot_name {
            "items" | "children" => match ty {
                Type::List(item) => self.accepts_renderable_type(item),
                _ => false,
            },
            "child" => self.accepts_renderable_type(ty) || matches!(ty, Type::Text | Type::Number),
            _ => self.accepts_renderable_type(ty),
        }
    }

    fn accepts_renderable_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::RenderContract)
            || self.is_renderable_object_type(ty)
            || is_no_element_type(ty)
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

fn render_parameter(name: &str, ty: Type, required: bool) -> AuthoritativeParameter {
    AuthoritativeParameter {
        name: name.to_owned(),
        kind: CheckedParameterKind::Value,
        flow_type: continuous_flow_type(ty),
        required,
    }
}

fn render_object_parameter(name: &str, required: bool) -> AuthoritativeParameter {
    render_parameter(name, open_object_type(), required)
}

fn render_constructor_parameters(function: &str) -> Option<Vec<AuthoritativeParameter>> {
    let renderable = |name, required| render_parameter(name, Type::RenderContract, required);
    let renderables = |name, required| {
        render_parameter(name, Type::List(Box::new(Type::RenderContract)), required)
    };
    let text = |name, required| render_parameter(name, Type::Text, required);
    let number = |name, required| render_parameter(name, Type::Number, required);
    let boolean = |name, required| render_parameter(name, true_false_type(), required);

    let parameters = match function {
        "Document/new" => vec![renderable("root", true)],
        "Scene/new" => vec![
            renderable("root", true),
            render_object_parameter("lights", false),
            render_object_parameter("geometry", false),
        ],
        "Element/container" | "Scene/Element/block" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            renderable("child", true),
        ],
        "Element/stripe" | "Scene/Element/stripe" => vec![
            render_object_parameter("element", true),
            render_object_parameter("direction", false),
            number("gap", false),
            render_object_parameter("style", false),
            boolean("visible", false),
            renderables("items", true),
        ],
        "Element/text" | "Scene/Element/text" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            text("text", true),
            boolean("visible", false),
            text("target", false),
        ],
        "Element/label" | "Scene/Element/label" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            text("label", true),
            boolean("visible", false),
            text("target", false),
        ],
        "Element/paragraph" | "Scene/Element/paragraph" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            renderables("contents", true),
        ],
        "Element/link" | "Scene/Element/link" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            text("label", true),
            text("to", true),
            boolean("visible", false),
            text("target", false),
        ],
        "Element/button" | "Scene/Element/button" => vec![
            render_object_parameter("element", true),
            render_object_parameter("activate_focus", false),
            render_object_parameter("style", false),
            text("label", true),
            boolean("visible", false),
            boolean("selected", false),
            text("target", false),
        ],
        "Element/checkbox" | "Scene/Element/checkbox" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            text("label", true),
            boolean("checked", true),
            boolean("visible", false),
            text("target", false),
        ],
        "Element/text_input" | "Scene/Element/text_input" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            text("label", true),
            text("text", true),
            render_object_parameter("placeholder", false),
            boolean("visible", false),
            text("target", false),
            boolean("focus", false),
        ],
        "Element/program" | "Scene/Element/program" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            text("source", false),
            render_parameter(
                "support_sources",
                Type::List(Box::new(open_object_type())),
                false,
            ),
            text("artifact_id", false),
            number("revision", true),
            render_parameter("artifact_retention", Type::Unknown, false),
            text("bootstrap_source", false),
            text("bootstrap_artifact_id", false),
            number("bootstrap_revision", false),
            render_parameter("capability_profile", Type::Unknown, true),
            text("session_key", false),
            boolean("mount", false),
        ],
        "Element/embedded_media" | "Scene/Element/embedded_media" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            text("title", true),
            text("to", true),
            renderable("child", true),
        ],
        "Element/map" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            number("generation", false),
            render_object_parameter("camera", true),
            render_object_parameter("bounds", true),
            render_object_parameter("tile_source", true),
            render_object_parameter("interaction", true),
            render_parameter("overlays", Type::List(Box::new(open_object_type())), true),
            renderables("children", false),
        ],
        "Scene/Element/map" => vec![
            render_object_parameter("element", true),
            render_object_parameter("style", false),
            number("generation", false),
            render_object_parameter("camera", true),
            render_object_parameter("bounds", true),
            render_object_parameter("tile_source", true),
            render_object_parameter("interaction", true),
            render_parameter("overlays", Type::List(Box::new(open_object_type())), true),
            renderables("items", false),
        ],
        _ => return None,
    };
    Some(parameters)
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
    "Element/map",
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
    "Scene/Element/map",
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
        (Type::Text, Type::Text) | (Type::Number, Type::Number) => true,
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
        (Type::VariantSet(actual), Type::VariantSet(expected)) => actual.iter().all(|actual| {
            expected
                .iter()
                .any(|expected| variant_is_assignable_to(actual, expected))
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

fn when_arms(
    statements: &[AstStatement],
    expr_id: usize,
    expressions: &[AstExpr],
) -> Vec<(Vec<String>, usize)> {
    fn from_statements(
        statements: &[AstStatement],
        expr_id: usize,
        expressions: &[AstExpr],
    ) -> Vec<(Vec<String>, usize)> {
        if let Some(statement) = exact_expression_statement(statements, expr_id) {
            return when_arms_from_statement(statement, expressions);
        }
        for statement in statements {
            let nested = from_statements(&statement.children, expr_id, expressions);
            if !nested.is_empty() {
                return nested;
            }
            if statement.expr.is_some_and(|statement_expr_id| {
                expr_contains_expr_id(statement_expr_id, expr_id, expressions)
            }) {
                return when_arms_from_statement(statement, expressions);
            }
        }
        Vec::new()
    }

    let arms = from_statements(statements, expr_id, expressions);
    if !arms.is_empty() {
        return arms;
    }
    inline_when_arms(expr_id, expressions)
}

fn inline_when_arms(expr_id: usize, expressions: &[AstExpr]) -> Vec<(Vec<String>, usize)> {
    let Some(select) = expressions.get(expr_id) else {
        return Vec::new();
    };
    let candidates = expressions
        .iter()
        .filter(|candidate| {
            candidate.start >= select.start
                && candidate.end <= select.end
                && matches!(candidate.kind, AstExprKind::MatchArm { .. })
        })
        .collect::<Vec<_>>();
    let mut direct = candidates
        .iter()
        .copied()
        .filter(|candidate| {
            !candidates.iter().any(|parent| {
                parent.id != candidate.id
                    && parent.start <= candidate.start
                    && candidate.end <= parent.end
            })
        })
        .collect::<Vec<_>>();
    direct.sort_by_key(|arm| arm.start);
    direct
        .into_iter()
        .filter_map(|arm| match &arm.kind {
            AstExprKind::MatchArm {
                pattern,
                output: Some(output),
            } => Some((pattern.clone(), *output)),
            _ => None,
        })
        .collect()
}

fn when_arms_from_statement(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Vec<(Vec<String>, usize)> {
    statement
        .children
        .iter()
        .flat_map(|child| {
            let pattern = child
                .expr
                .and_then(|arm_expr_id| expressions.get(arm_expr_id))
                .and_then(|arm_expr| match &arm_expr.kind {
                    AstExprKind::MatchArm { pattern, .. } => Some(pattern.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            statement_update_value_exprs(child, expressions)
                .into_iter()
                .map(move |arm_expr_id| (pattern.clone(), arm_expr_id))
        })
        .collect()
}

fn narrowed_pattern_binding(selector: &Type, pattern: &[String]) -> Option<Type> {
    let Variant::Tag(pattern_tag) = pattern_variant(pattern)? else {
        return None;
    };
    let Type::VariantSet(variants) = selector else {
        return None;
    };
    variants.iter().find_map(|variant| match variant {
        Variant::Tagged { tag, fields } if tag == &pattern_tag => {
            Some(Type::Object(fields.clone()))
        }
        Variant::Tag(tag) if tag == &pattern_tag => Some(Type::VariantSet(vec![variant.clone()])),
        _ => None,
    })
}

fn pattern_payload_bindings(selector: &Type, pattern: &[String]) -> BTreeMap<String, Type> {
    let variables = pattern_variable_names(pattern);
    let Some(Variant::Tag(pattern_tag)) = pattern_variant(pattern) else {
        return match variables.as_slice() {
            [name] => BTreeMap::from([(name.clone(), selector.clone())]),
            _ => BTreeMap::new(),
        };
    };
    let Type::VariantSet(variants) = selector else {
        return BTreeMap::new();
    };
    let Some(Variant::Tagged { fields, .. }) = variants
        .iter()
        .find(|variant| matches!(variant, Variant::Tagged { tag, .. } if tag == &pattern_tag))
    else {
        return BTreeMap::new();
    };
    variables
        .into_iter()
        .filter_map(|name| fields.fields.get(&name).cloned().map(|ty| (name, ty)))
        .collect()
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

fn named_call_argument_exprs(
    program: &ParsedProgram,
    call_expr_id: usize,
    inline_args: &[AstCallArg],
) -> Vec<(String, usize)> {
    if !inline_args.is_empty() {
        return inline_args
            .iter()
            .filter_map(|argument| Some((argument.named_name()?.to_owned(), argument.value)))
            .collect();
    }

    exact_expression_statement(&program.ast.statements, call_expr_id)
        .or_else(|| {
            containing_expression_statement(
                &program.ast.statements,
                call_expr_id,
                &program.expressions,
            )
        })
        .into_iter()
        .flat_map(|statement| &statement.children)
        .filter_map(|argument| {
            let name = match &argument.kind {
                AstStatementKind::Field { name }
                | AstStatementKind::List {
                    field: Some(name), ..
                } => name,
                _ => return None,
            };
            Some((name.clone(), argument.expr?))
        })
        .collect()
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
        | AstExprKind::When { input: initial, .. }
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
        } else {
            updates.extend(statement_update_value_exprs(child, expressions));
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
            return vec![
                statement_pipeline_final_expr_id_containing_expr(
                    &statement.children,
                    *output,
                    expressions,
                )
                .unwrap_or(*output),
            ];
        }
        return vec![expr_id];
    }
    if let Some(expr_id) = statement.expr {
        if let Some(AstExprKind::Then {
            output: Some(output),
            ..
        }) = expressions.get(expr_id).map(|expr| &expr.kind)
        {
            return vec![
                statement_pipeline_final_expr_id_containing_expr(
                    &statement.children,
                    *output,
                    expressions,
                )
                .unwrap_or(*output),
            ];
        }
        if matches!(
            expressions.get(expr_id).map(|expr| &expr.kind),
            Some(AstExprKind::Then { output: None, .. })
                | Some(AstExprKind::MatchArm { output: None, .. })
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

fn canonical_statement_value_expression(
    statements: &[AstStatement],
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    statement
        .expr
        .and_then(|expr_id| {
            statement_pipeline_final_expr_id_containing_expr(statements, expr_id, expressions)
        })
        .or_else(|| direct_statement_value_expr_id(statement, expressions))
}

fn canonical_checked_statement_value_expression(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    if matches!(statement.kind, AstStatementKind::Function { .. }) {
        canonical_block_value_expression(&statement.children, expressions).or_else(|| {
            canonical_statement_value_expression(&statement.children, statement, expressions)
        })
    } else {
        canonical_statement_value_expression(&statement.children, statement, expressions)
            .or_else(|| canonical_block_value_expression(&statement.children, expressions))
    }
}

fn canonical_block_value_expression(
    statements: &[AstStatement],
    expressions: &[AstExpr],
) -> Option<usize> {
    let mut result = None;
    for statement in statements {
        if statement_is_source_pipe_continuation(statement, expressions) && result.is_some() {
            continue;
        }
        if let Some(expression) =
            canonical_statement_value_expression(statements, statement, expressions)
        {
            result = Some(expression);
        }
    }
    result
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
    let mut item_type = statement
        .expr
        .and_then(|expr_id| expressions.get(expr_id))
        .and_then(|expr| match &expr.kind {
            AstExprKind::ListLiteral { items, .. } => items
                .iter()
                .filter_map(|item| expressions.get(*item))
                .map(|item| simple_expr_type(item, expressions))
                .reduce(|existing, extra| widen_structural_type(&existing, &extra)),
            _ => None,
        });
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
    if let Some(expr_id) = direct_statement_value_expr_id(statement, expressions)
        .map(|expr_id| expression_result_expr_id(expr_id, expressions))
        && let Some(expr) = expressions.get(expr_id)
    {
        match &expr.kind {
            AstExprKind::Hold { initial, .. } => {
                let hold_statement = statement_for_expr(statement, expr_id).unwrap_or(statement);
                return Some(simple_hold_result_type(
                    hold_statement,
                    *initial,
                    expressions,
                ));
            }
            AstExprKind::Pipe { input, op, .. } if op == "HOLD" => {
                let hold_statement = statement_for_expr(statement, expr_id).unwrap_or(statement);
                return Some(simple_hold_result_type(hold_statement, *input, expressions));
            }
            AstExprKind::When { .. } => {
                return static_when_type_from_bindings(
                    statement,
                    expr_id,
                    expressions,
                    &BTreeMap::new(),
                );
            }
            _ => {}
        }
    }
    if let Some(ty) = simple_statement_pipeline_type(statement, expressions) {
        return Some(ty);
    }
    let expr_id = expression_result_expr_id(
        direct_statement_value_expr_id(statement, expressions)?,
        expressions,
    );
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
        let next = if matches!(expr.kind, AstExprKind::When { .. }) {
            let when_statement = statement_for_expr(statement, *expr_id).unwrap_or(statement);
            static_when_type_from_bindings(when_statement, *expr_id, expressions, &BTreeMap::new())
                .unwrap_or_else(|| simple_expr_type(expr, expressions))
        } else {
            simple_expr_type(expr, expressions)
        };
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
    if let Some(expr_id) = direct_statement_value_expr_id(statement, expressions)
        .map(|expr_id| expression_result_expr_id(expr_id, expressions))
        && let Some(expr) = expressions.get(expr_id)
    {
        match &expr.kind {
            AstExprKind::Hold { initial, .. } => {
                let hold_statement = statement_for_expr(statement, expr_id).unwrap_or(statement);
                return Some(simple_hold_result_type(
                    hold_statement,
                    *initial,
                    expressions,
                ));
            }
            AstExprKind::Pipe { input, op, .. } if op == "HOLD" => {
                let hold_statement = statement_for_expr(statement, expr_id).unwrap_or(statement);
                return Some(simple_hold_result_type(hold_statement, *input, expressions));
            }
            AstExprKind::When { .. } => {
                return static_when_type_from_bindings(statement, expr_id, expressions, bindings);
            }
            _ => {}
        }
    }
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
            let next = match &expr.kind {
                AstExprKind::When { .. } => {
                    let when_statement =
                        statement_for_expr(statement, *expr_id).unwrap_or(statement);
                    static_when_type_from_bindings(when_statement, *expr_id, expressions, bindings)
                }
                AstExprKind::Pipe { op, args, .. } if op == "List/map" => {
                    let mut local_bindings = bindings.clone();
                    if let Some(binding_name) = args
                        .iter()
                        .find(|arg| arg.is_bare_binding())
                        .and_then(|arg| expressions.get(arg.value))
                        .and_then(expr_single_name)
                        && let Some(item_type) = list_item_type_from_list_type(&ty)
                    {
                        local_bindings.insert(binding_name.to_owned(), item_type);
                    }
                    let item_type =
                        list_map_result_expr_id(std::slice::from_ref(statement), expressions, args)
                            .and_then(|new_expr_id| expressions.get(new_expr_id))
                            .and_then(|new_expr| {
                                static_expr_type_from_bindings(
                                    new_expr,
                                    expressions,
                                    &local_bindings,
                                )
                            })
                            .unwrap_or_else(open_object_type);
                    Some(Type::List(Box::new(item_type)))
                }
                AstExprKind::Pipe { op, .. } if op == "List/latest" => {
                    list_item_type_from_list_type(&ty)
                }
                AstExprKind::Pipe { op, .. }
                    if matches!(
                        op.as_str(),
                        "List/retain"
                            | "List/filter"
                            | "List/remove"
                            | "List/move_field_first"
                            | "List/move_field_last"
                    ) =>
                {
                    Some(ty.clone())
                }
                _ => static_expr_type_from_bindings(expr, expressions, bindings),
            };
            if let Some(next) = next.or_else(|| {
                let ty = simple_expr_type(expr, expressions);
                is_specific_type(&ty).then_some(ty)
            }) {
                ty = next;
            }
        }
        return Some(ty);
    }
    let expr_id = expression_result_expr_id(
        direct_statement_value_expr_id(statement, expressions)?,
        expressions,
    );
    if matches!(expressions.get(expr_id)?.kind, AstExprKind::When { .. }) {
        return static_when_type_from_bindings(statement, expr_id, expressions, bindings);
    }
    static_expr_type_from_bindings(expressions.get(expr_id)?, expressions, bindings)
}

fn static_when_type_from_bindings(
    statement: &AstStatement,
    expr_id: usize,
    expressions: &[AstExpr],
    bindings: &BTreeMap<String, Type>,
) -> Option<Type> {
    let AstExprKind::When { input, .. } = &expressions.get(expr_id)?.kind else {
        return None;
    };
    let selector = expressions.get(*input)?;
    let selector_path = pattern_selector_path(Some(selector));
    let selector_type = static_expr_type_from_bindings(selector, expressions, bindings);
    let mut result = None;
    for arm in when_arm_statements(std::slice::from_ref(statement), expr_id, expressions) {
        let pattern = arm
            .expr
            .and_then(|arm_expr_id| expressions.get(arm_expr_id))
            .and_then(|arm_expr| match &arm_expr.kind {
                AstExprKind::MatchArm { pattern, .. } => Some(pattern.as_slice()),
                _ => None,
            })
            .unwrap_or_default();
        let narrowed = selector_type
            .as_ref()
            .and_then(|selector_type| narrowed_pattern_binding(selector_type, pattern));
        let mut arm_bindings = bindings.clone();
        if let (Some(path), Some(narrowed)) = (&selector_path, narrowed) {
            arm_bindings.insert(path.clone(), narrowed.clone());
            if let Some(name) = path.rsplit('.').next() {
                arm_bindings.insert(name.to_owned(), narrowed);
            }
        }
        let Some(arm_type) = statement_value_type_from_bindings(arm, expressions, &arm_bindings)
            .or_else(|| simple_statement_value_type(arm, expressions))
        else {
            continue;
        };
        if matches!(arm_type, Type::Skip) {
            continue;
        }
        result = Some(match result {
            Some(existing) => widen_structural_type(&existing, &arm_type),
            None => arm_type,
        });
    }
    result
}

fn expression_result_expr_id(mut expr_id: usize, expressions: &[AstExpr]) -> usize {
    loop {
        let Some(expr) = expressions.get(expr_id) else {
            return expr_id;
        };
        let next = match &expr.kind {
            AstExprKind::MatchArm {
                output: Some(output),
                ..
            }
            | AstExprKind::Then {
                output: Some(output),
                ..
            } => *output,
            _ => return expr_id,
        };
        if next == expr_id {
            return expr_id;
        }
        expr_id = next;
    }
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
            ty = widen_hold_type(&ty, &update_type);
        }
    }
    let mut statement_expr_ids = Vec::new();
    collect_statement_expr_ids(statement, &mut statement_expr_ids);
    for result_type in expressions.iter().filter_map(|expr| {
        if !statement_expr_ids
            .iter()
            .any(|root| *root == expr.id || expr_contains_expr_id(*root, expr.id, expressions))
        {
            return None;
        }
        let AstExprKind::Call { function, .. } = &expr.kind else {
            return None;
        };
        host_effect_signature(function).map(|signature| signature.result_type)
    }) {
        ty = widen_hold_type(&ty, &result_type);
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

fn static_list_literal_type<F>(
    items: &[usize],
    expressions: &[AstExpr],
    mut type_for_expr: F,
) -> Type
where
    F: FnMut(&AstExpr) -> Option<Type>,
{
    let item_type = items
        .iter()
        .filter_map(|item| expressions.get(*item).and_then(&mut type_for_expr))
        .reduce(|existing, extra| widen_structural_type(&existing, &extra));
    Type::List(Box::new(item_type.unwrap_or_else(open_object_type)))
}

fn simple_expr_type(expr: &AstExpr, expressions: &[AstExpr]) -> Type {
    if let AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. } = &expr.kind
        && let Some(intrinsic_type) = session_info_intrinsic_type(function)
    {
        return intrinsic_type;
    }
    if let AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. } = &expr.kind
        && let Some(signature) = host_effect_signature(function)
    {
        return signature.result_type;
    }
    match &expr.kind {
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Type::Text,
        AstExprKind::Number(_) => Type::Number,
        AstExprKind::ByteLiteral { .. } => Type::Bytes(BytesType::Fixed(1)),
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
        AstExprKind::ListLiteral { items, .. } => {
            static_list_literal_type(items, expressions, |item| {
                Some(simple_expr_type(item, expressions))
            })
        }
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
                    | "Number/ceil"
                    | "Number/floor"
                    | "Number/round"
                    | "Number/truncate"
                    | "List/count"
                    | "List/sum"
                    | "Text/find"
                    | "Text/length"
                    | "Text/to_number"
                    | "Bytes/length"
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
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if function == "List/chunk" =>
        {
            Type::List(Box::new(Type::Object(ObjectShape::from_ordered_fields(
                [
                    ("label".to_owned(), Type::Text),
                    ("items".to_owned(), Type::List(Box::new(open_object_type()))),
                ],
                false,
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
        if let AstStatementKind::Function { name, parameters } = &statement.kind {
            let params = parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect::<BTreeSet<_>>();
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
        AstExprKind::Call { function, args, .. } => {
            is_registered_render_constructor(function)
                || args
                    .iter()
                    .any(|arg| expr_contains_render_constructor_seen(arg.value, expressions, seen))
        }
        AstExprKind::Pipe {
            input, op, args, ..
        } => {
            is_registered_render_constructor(op)
                || expr_contains_render_constructor_seen(*input, expressions, seen)
                || args
                    .iter()
                    .any(|arg| expr_contains_render_constructor_seen(arg.value, expressions, seen))
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
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
        AstExprKind::Block { bindings, result } => {
            bindings.iter().any(|binding| {
                expr_contains_render_constructor_seen(binding.value, expressions, seen)
            }) || result.is_some_and(|result| {
                expr_contains_render_constructor_seen(result, expressions, seen)
            })
        }
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
        AstExprKind::Call { function, args, .. } => {
            for arg in args {
                let expected = builtin_argument_expected_type(function, arg.named_name(), false);
                collect_param_requirements_expr(
                    arg.value,
                    expressions,
                    params,
                    requirements,
                    expected,
                );
            }
        }
        AstExprKind::Pipe {
            input, op, args, ..
        } => {
            let input_expected = pipe_input_expected_type(op);
            collect_param_requirements_expr(
                *input,
                expressions,
                params,
                requirements,
                input_expected,
            );
            for arg in args {
                let expected = builtin_argument_expected_type(op, arg.named_name(), true);
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
        AstExprKind::When { input, .. } => {
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
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                collect_param_requirements_expr(
                    binding.value,
                    expressions,
                    params,
                    requirements,
                    None,
                );
            }
            if let Some(result) = result {
                collect_param_requirements_expr(
                    *result,
                    expressions,
                    params,
                    requirements,
                    expected,
                );
            }
        }
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
    if function == "Text/join" {
        Some(Type::List(Box::new(Type::Text)))
    } else if function == "List/map"
        || matches!(
            function,
            "List/retain"
                | "List/remove"
                | "List/query"
                | "List/query_prefix"
                | "List/count"
                | "List/every"
                | "List/any"
                | "List/is_not_empty"
                | "List/latest"
        )
    {
        Some(Type::List(Box::new(open_object_type())))
    } else if function == "Router/go_to" {
        Some(Type::Text)
    } else if matches!(
        function,
        "Text/to_bytes" | "File/read_text" | "Log/error" | "Log/info"
    ) {
        Some(Type::Text)
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
    } else if matches!(function, "File/read_text" | "Log/error" | "Log/info")
        || function.starts_with("Text/")
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
                .find_map(|field| (field.name == arg_name).then_some(field.ty))
        });
    }
    if function == "Bool/toggle" && arg_name == Some("when") {
        return Some(Type::Unknown);
    }
    if function == "File/read_text" {
        return match arg_name {
            Some("path") | Some("input") | None => Some(Type::Text),
            _ => None,
        };
    }
    render_arg_expected_type(function, arg_name)
        .or_else(|| list_argument_expected_type(function, arg_name))
        .or_else(|| light_argument_expected_type(function, arg_name))
        .or_else(|| router_argument_expected_type(function, arg_name))
        .or_else(|| bytes_argument_expected_type(function, arg_name))
        .or_else(|| text_argument_expected_type(function, arg_name, piped))
        .or_else(|| number_argument_expected_type(function, arg_name, piped))
        .or_else(|| argument_expected_type(function))
}

fn builtin_argument_is_symbol(function: &str, arg_name: Option<&str>) -> bool {
    matches!(
        (function, arg_name),
        ("List/query_prefix", Some("field" | "normalization"))
            | (
                "List/query",
                Some(
                    "select"
                        | "residual"
                        | "order"
                        | "residual_field"
                        | "latitude_field"
                        | "longitude_field"
                )
            )
    )
}

fn builtin_static_symbol_expression_ids(program: &ParsedProgram) -> BTreeSet<usize> {
    let mut symbols = program
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::Call { function, args, .. }
            | AstExprKind::Pipe {
                op: function, args, ..
            } => Some((function, args)),
            _ => None,
        })
        .flat_map(|(function, args)| {
            args.iter()
                .filter(move |arg| builtin_argument_is_symbol(function, arg.named_name()))
                .map(|arg| arg.value)
        })
        .collect();
    collect_builtin_symbol_statement_exprs(
        &program.ast.statements,
        &program.expressions,
        &mut symbols,
    );
    symbols
}

fn collect_builtin_symbol_statement_exprs(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    output: &mut BTreeSet<usize>,
) {
    for statement in statements {
        if let Some(expr_id) = statement.expr
            && let Some(AstExpr {
                kind: AstExprKind::Call { function, .. },
                ..
            }) = expressions.get(expr_id)
        {
            for child in &statement.children {
                let AstStatementKind::Field { name } = &child.kind else {
                    continue;
                };
                if builtin_argument_is_symbol(function, Some(name))
                    && let Some(value) = child.expr
                {
                    output.insert(value);
                }
            }
        }
        collect_builtin_symbol_statement_exprs(&statement.children, expressions, output);
    }
}

fn list_argument_expected_type(function: &str, arg_name: Option<&str>) -> Option<Type> {
    match (function, arg_name) {
        ("List/remove" | "List/query", Some("list")) => {
            Some(Type::List(Box::new(open_object_type())))
        }
        ("List/retain" | "List/every" | "List/any", Some("if")) => Some(true_false_type()),
        ("List/remove", Some("when")) => Some(true_false_type()),
        ("List/query", Some("fields" | "normalization" | "multi_value" | "prefix" | "needle")) => {
            Some(Type::Text)
        }
        (
            "List/query",
            Some(
                "limit" | "minimum" | "maximum" | "center_latitude" | "center_longitude"
                | "radius_meters",
            ),
        ) => Some(Type::Number),
        ("List/query", Some("unique" | "lower_inclusive" | "upper_inclusive")) => {
            Some(true_false_type())
        }
        ("List/query", Some("cursor")) => Some(Type::Bytes(BytesType::Dynamic)),
        _ => None,
    }
}

fn light_argument_expected_type(function: &str, arg_name: Option<&str>) -> Option<Type> {
    match (function, arg_name) {
        ("Light/directional", Some("azimuth" | "altitude" | "spread" | "intensity"))
        | ("Light/ambient", Some("intensity"))
        | ("Light/spot", Some("intensity" | "radius" | "softness")) => Some(Type::Number),
        ("Light/directional" | "Light/ambient" | "Light/spot", Some("color"))
        | ("Light/spot", Some("target")) => Some(Type::Unknown),
        _ => None,
    }
}

fn router_argument_expected_type(function: &str, arg_name: Option<&str>) -> Option<Type> {
    matches!((function, arg_name), ("Router/go_to", Some("route"))).then_some(Type::Text)
}

fn text_argument_expected_type(
    function: &str,
    arg_name: Option<&str>,
    piped: bool,
) -> Option<Type> {
    match (function, arg_name) {
        ("Text/join", Some("texts") | None) => Some(Type::List(Box::new(Type::Text))),
        ("Text/join", Some("separator" | "empty")) => Some(Type::Text),
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
    _piped: bool,
) -> Option<Type> {
    match (function, arg_name) {
        ("Number/to_text", Some("prefix")) => Some(true_false_type()),
        ("Number/to_text", Some("value")) => Some(Type::Number),
        ("Number/to_text", Some("radix" | "min_width" | "signed_width" | "group_size")) => {
            Some(Type::Number)
        }
        ("Number/to_text", None) => Some(Type::Number),
        ("Number/project_time", Some("pointer_x" | "pointer_width")) => Some(Type::Unknown),
        ("Number/project_time", Some("viewport_start" | "viewport_end" | "fallback")) => {
            Some(Type::Number)
        }
        ("Number/project_time", None) => Some(Type::Unknown),
        ("Number/project_offset" | "Number/project_width", Some("zoom")) => Some(Type::Unknown),
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
        Type::Text | Type::Number | Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
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
        Type::Number | Type::Text | Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
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
        ("Bytes/set", Some("value")) => Some(Type::Bytes(BytesType::Fixed(1))),
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
    if matches!(function, "Element/program" | "Scene/Element/program") {
        return match arg_name {
            Some("element" | "style") => Some(open_object_type()),
            Some(
                "source"
                | "artifact_id"
                | "bootstrap_source"
                | "bootstrap_artifact_id"
                | "session_key",
            ) => Some(Type::Text),
            Some("support_sources") => Some(Type::List(Box::new(open_object_type()))),
            Some("revision" | "bootstrap_revision") => Some(Type::Number),
            Some("mount") => Some(true_false_type()),
            Some("artifact_retention" | "capability_profile") => Some(Type::Unknown),
            _ => None,
        };
    }
    if matches!(
        function,
        "Element/embedded_media" | "Scene/Element/embedded_media"
    ) {
        return match arg_name {
            Some("element" | "style") => Some(open_object_type()),
            Some("title" | "to") => Some(Type::Text),
            Some("child") => Some(Type::RenderContract),
            _ => None,
        };
    }
    if matches!(function, "Element/map" | "Scene/Element/map") {
        return match arg_name {
            Some("element" | "style" | "camera" | "bounds" | "tile_source" | "interaction") => {
                Some(open_object_type())
            }
            Some("generation") => Some(Type::Number),
            Some("overlays") => Some(Type::List(Box::new(open_object_type()))),
            Some("items" | "children") => Some(Type::List(Box::new(Type::RenderContract))),
            _ => None,
        };
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

fn render_arg_should_validate_directly(function: &str, arg_name: &str) -> bool {
    if matches!(
        function,
        "Element/program"
            | "Scene/Element/program"
            | "Element/embedded_media"
            | "Scene/Element/embedded_media"
            | "Element/map"
            | "Scene/Element/map"
    ) {
        if matches!(arg_name, "element" | "style") {
            return false;
        }
        return render_arg_expected_type(function, Some(arg_name)).is_some();
    }
    matches!(
        arg_name,
        "input" | "root" | "items" | "children" | "checked" | "visible" | "selected" | "focus"
    )
}

fn name_bindings(
    program: &ParsedProgram,
    source_payload_types: &BTreeMap<String, Type>,
) -> BTreeMap<String, Type> {
    let mut bindings = BTreeMap::new();
    collect_name_bindings(
        &program.ast.statements,
        &program.expressions,
        &mut Vec::new(),
        source_payload_types,
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
        let pass = match &expr.kind {
            AstExprKind::Call { pass, .. } | AstExprKind::Pipe { pass, .. } => pass.as_ref(),
            _ => continue,
        };
        let Some(value_expr) = pass.and_then(|pass| program.expressions.get(pass.value)) else {
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
    if let AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. } = &expr.kind
        && let Some(signature) = host_effect_signature(function)
    {
        return Some(signature.result_type);
    }
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
        AstExprKind::Then { input, output } => output
            .or(Some(*input))
            .and_then(|expr_id| expressions.get(expr_id))
            .and_then(|expr| static_expr_type_from_bindings(expr, expressions, bindings)),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expressions
            .get(*output)
            .and_then(|expr| static_expr_type_from_bindings(expr, expressions, bindings)),
        AstExprKind::MatchArm { output: None, .. } => Some(Type::Skip),
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => Some(Type::Text),
        AstExprKind::Number(_) => Some(Type::Number),
        AstExprKind::ByteLiteral { .. } => Some(Type::Bytes(BytesType::Fixed(1))),
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
        AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => {
            let ty = simple_expr_type(expr, expressions);
            is_specific_type(&ty).then_some(ty)
        }
        _ => None,
    }
}

fn static_path_type_from_bindings(
    parts: &[String],
    bindings: &BTreeMap<String, Type>,
) -> Option<Type> {
    type_from_longest_binding_prefix(bindings, parts)
}

fn refresh_external_declaration_bindings(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    external_types: &ExternalTypeEnvironment,
    scope: &mut Vec<String>,
    bindings: &mut BTreeMap<String, Type>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Function { .. } => continue,
            AstStatementKind::Field { name } => {
                let path = scoped_path(scope, name);
                if let Some(expr_id) = direct_statement_value_expr_id(statement, expressions)
                    .map(|expr_id| expression_result_expr_id(expr_id, expressions))
                    && let Some(expr) = expressions.get(expr_id)
                    && let Some(ty) = static_expr_type_with_external_types(
                        expr,
                        expressions,
                        bindings,
                        external_types,
                    )
                {
                    insert_simple_binding_preserving_renderable(bindings, name, ty.clone());
                    bindings.insert(path.clone(), ty);
                }
                scope.push(name.clone());
                refresh_external_declaration_bindings(
                    &statement.children,
                    expressions,
                    external_types,
                    scope,
                    bindings,
                );
                scope.pop();
            }
            _ => refresh_external_declaration_bindings(
                &statement.children,
                expressions,
                external_types,
                scope,
                bindings,
            ),
        }
    }
}

fn static_expr_type_with_external_types(
    expr: &AstExpr,
    expressions: &[AstExpr],
    bindings: &BTreeMap<String, Type>,
    external_types: &ExternalTypeEnvironment,
) -> Option<Type> {
    match &expr.kind {
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
            if external_function_role(function).is_some() =>
        {
            external_types
                .functions
                .get(function)
                .map(|signature| signature.result.ty.clone())
        }
        AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
            Some(Type::Object(ObjectShape::from_ordered_fields(
                fields.iter().filter(|field| !field.spread).map(|field| {
                    (
                        field.name.clone(),
                        expressions
                            .get(field.value)
                            .and_then(|field_expr| {
                                static_expr_type_with_external_types(
                                    field_expr,
                                    expressions,
                                    bindings,
                                    external_types,
                                )
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
                    fields.iter().filter(|field| !field.spread).map(|field| {
                        (
                            field.name.clone(),
                            expressions
                                .get(field.value)
                                .and_then(|field_expr| {
                                    static_expr_type_with_external_types(
                                        field_expr,
                                        expressions,
                                        bindings,
                                        external_types,
                                    )
                                })
                                .unwrap_or_else(open_object_type),
                        )
                    }),
                    false,
                ),
            }]))
        }
        AstExprKind::Draining { input } => expressions.get(*input).and_then(|input| {
            static_expr_type_with_external_types(input, expressions, bindings, external_types)
        }),
        AstExprKind::Then { input, output } => output
            .or(Some(*input))
            .and_then(|expr_id| expressions.get(expr_id))
            .and_then(|output| {
                static_expr_type_with_external_types(output, expressions, bindings, external_types)
            }),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expressions.get(*output).and_then(|output| {
            static_expr_type_with_external_types(output, expressions, bindings, external_types)
        }),
        _ => static_expr_type_from_bindings(expr, expressions, bindings),
    }
}

fn flow_bindings(
    program: &ParsedProgram,
    external_types: &ExternalTypeEnvironment,
) -> BTreeMap<String, FlowMode> {
    let mut bindings = external_types
        .values
        .iter()
        .map(|(path, flow_type)| (path.clone(), flow_type.mode))
        .collect();
    collect_flow_bindings(
        &program.ast.statements,
        &program.expressions,
        &mut Vec::new(),
        &mut bindings,
    );
    bindings
}

fn collect_canonical_named_value_paths(
    statements: &[AstStatement],
    scope: &mut Vec<String>,
    paths: &mut BTreeSet<String>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Function { .. } => {}
            AstStatementKind::Field { name } if matches!(name.as_str(), "document" | "scene") => {}
            AstStatementKind::Field { name } => {
                let path = scoped_path(scope, name);
                paths.insert(path);
                scope.push(name.clone());
                collect_canonical_named_value_paths(&statement.children, scope, paths);
                scope.pop();
            }
            AstStatementKind::Hold { field, name, .. } => {
                if let Some(name) = field.as_ref().or(name.as_ref()) {
                    let path = if scope.last() == Some(name) {
                        scope.join(".")
                    } else {
                        scoped_path(scope, name)
                    };
                    paths.insert(path);
                }
            }
            AstStatementKind::List {
                field: Some(name), ..
            }
            | AstStatementKind::Source {
                field: Some(name), ..
            } => {
                paths.insert(scoped_path(scope, name));
            }
            AstStatementKind::Block => {
                collect_canonical_named_value_paths(&statement.children, scope, paths);
            }
            AstStatementKind::List { field: None, .. }
            | AstStatementKind::Source { field: None, .. }
            | AstStatementKind::Spread
            | AstStatementKind::Expression => {}
        }
    }
}

fn declaration_expression_index(program: &ParsedProgram) -> BTreeMap<String, usize> {
    let mut declarations = BTreeMap::new();
    collect_declaration_expressions(
        &program.ast.statements,
        &program.expressions,
        &mut Vec::new(),
        &mut declarations,
    );
    declarations
}

fn collect_declaration_expressions(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    scope: &mut Vec<String>,
    declarations: &mut BTreeMap<String, usize>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Function { .. } => continue,
            AstStatementKind::Field { name } if matches!(name.as_str(), "document" | "scene") => {
                continue;
            }
            AstStatementKind::Field { name } => {
                let path = scoped_path(scope, name);
                if let Some(expr_id) =
                    canonical_statement_value_expression(statements, statement, expressions)
                {
                    declarations.insert(path, expr_id);
                }
                scope.push(name.clone());
                collect_declaration_expressions(
                    &statement.children,
                    expressions,
                    scope,
                    declarations,
                );
                scope.pop();
                continue;
            }
            AstStatementKind::Hold { field, name, .. } => {
                if let Some(name) = field.as_ref().or(name.as_ref())
                    && let Some(expr_id) =
                        canonical_statement_value_expression(statements, statement, expressions)
                {
                    let path = if scope.last() == Some(name) {
                        scope.join(".")
                    } else {
                        scoped_path(scope, name)
                    };
                    declarations.insert(path, expr_id);
                }
            }
            AstStatementKind::List {
                field: Some(name), ..
            }
            | AstStatementKind::Source {
                field: Some(name), ..
            } => {
                if let Some(expr_id) =
                    canonical_statement_value_expression(statements, statement, expressions)
                {
                    declarations.insert(scoped_path(scope, name), expr_id);
                }
            }
            AstStatementKind::Block
            | AstStatementKind::List { field: None, .. }
            | AstStatementKind::Source { field: None, .. }
            | AstStatementKind::Spread
            | AstStatementKind::Expression => {}
        }
        collect_declaration_expressions(&statement.children, expressions, scope, declarations);
    }
}

fn declaration_expr_for_path(declarations: &BTreeMap<String, usize>, path: &str) -> Option<usize> {
    declarations.get(path).copied().or_else(|| {
        let suffix = format!(".{path}");
        let mut matches = declarations
            .iter()
            .filter(|(candidate, _)| candidate.ends_with(&suffix))
            .map(|(_, expr_id)| *expr_id);
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    })
}

fn collect_inferred_named_value_types(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    expr_type_cache: &[Option<FlowType>],
    scope: &mut Vec<String>,
    types: &mut BTreeMap<String, Type>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Function { .. } => continue,
            AstStatementKind::Field { name } if matches!(name.as_str(), "document" | "scene") => {
                continue;
            }
            AstStatementKind::Field { name } => {
                let path = scoped_path(scope, name);
                if let Some(ty) = inferred_statement_type(statement, expressions, expr_type_cache) {
                    types.insert(path, ty);
                }
                scope.push(name.clone());
                collect_inferred_named_value_types(
                    &statement.children,
                    expressions,
                    expr_type_cache,
                    scope,
                    types,
                );
                scope.pop();
                continue;
            }
            AstStatementKind::Hold { field, name, .. } => {
                if let Some(name) = field.as_ref().or(name.as_ref())
                    && let Some(ty) =
                        inferred_statement_type(statement, expressions, expr_type_cache)
                {
                    let path = if scope.last() == Some(name) {
                        scope.join(".")
                    } else {
                        scoped_path(scope, name)
                    };
                    types.insert(path, ty);
                }
            }
            AstStatementKind::List {
                field: Some(name), ..
            }
            | AstStatementKind::Source {
                field: Some(name), ..
            } => {
                if let Some(ty) = inferred_statement_type(statement, expressions, expr_type_cache) {
                    types.insert(scoped_path(scope, name), ty);
                }
            }
            AstStatementKind::Block
            | AstStatementKind::List { field: None, .. }
            | AstStatementKind::Source { field: None, .. }
            | AstStatementKind::Spread
            | AstStatementKind::Expression => {}
        }
        collect_inferred_named_value_types(
            &statement.children,
            expressions,
            expr_type_cache,
            scope,
            types,
        );
    }
}

fn inferred_statement_type(
    statement: &AstStatement,
    expressions: &[AstExpr],
    expr_type_cache: &[Option<FlowType>],
) -> Option<Type> {
    statement_pipeline_final_expr_id(statement, expressions)
        .or_else(|| direct_statement_value_expr_id(statement, expressions))
        .and_then(|expr_id| expr_type_cache.get(expr_id))
        .and_then(Option::as_ref)
        .map(|flow| flow.ty.clone())
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
                    let mode = simple_statement_flow_mode(statement, expr, expressions, bindings);
                    insert_flow_binding(bindings, name.clone(), mode);
                    insert_flow_binding(bindings, scoped_path(scope, name), mode);
                }
                scope.push(name.clone());
                collect_flow_bindings(&statement.children, expressions, scope, bindings);
                scope.pop();
            }
            AstStatementKind::Hold {
                name: Some(name), ..
            } => {
                let mode = if statement_contains_typed_host_effect(statement, expressions) {
                    FlowMode::PresentOrAbsent
                } else {
                    FlowMode::Continuous
                };
                let path = if scope.last() == Some(name) {
                    scope.join(".")
                } else {
                    scoped_path(scope, name)
                };
                insert_flow_binding(bindings, name.clone(), mode);
                insert_flow_binding(bindings, path, mode);
                collect_flow_bindings(&statement.children, expressions, scope, bindings);
            }
            AstStatementKind::Source {
                field: Some(name), ..
            } => {
                insert_flow_binding(bindings, name.clone(), FlowMode::PresentOrAbsent);
                insert_flow_binding(
                    bindings,
                    scoped_path(scope, name),
                    FlowMode::PresentOrAbsent,
                );
                collect_flow_bindings(&statement.children, expressions, scope, bindings);
            }
            _ => collect_flow_bindings(&statement.children, expressions, scope, bindings),
        }
    }
}

fn insert_flow_binding(bindings: &mut BTreeMap<String, FlowMode>, path: String, mode: FlowMode) {
    bindings
        .entry(path)
        .and_modify(|existing| *existing = merge_flow_modes(*existing, mode))
        .or_insert(mode);
}

fn flow_binding_mode(bindings: &BTreeMap<String, FlowMode>, path: &str) -> Option<FlowMode> {
    bindings.get(path).copied().or_else(|| {
        let suffix = format!(".{path}");
        bindings
            .iter()
            .filter(|(candidate, _)| candidate.ends_with(&suffix))
            .map(|(_, mode)| *mode)
            .reduce(merge_flow_modes)
    })
}

fn statement_contains_typed_host_effect(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    let host_calls = expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::Call { function, .. } if is_typed_host_effect(function) => Some(expr.id),
            _ => None,
        })
        .collect::<Vec<_>>();
    statement_expr_ids(statement).into_iter().any(|root| {
        host_calls
            .iter()
            .any(|host_call| expr_contains_expr_id(root, *host_call, expressions))
    })
}

fn simple_statement_flow_mode(
    statement: &AstStatement,
    expr: &AstExpr,
    expressions: &[AstExpr],
    bindings: &BTreeMap<String, FlowMode>,
) -> FlowMode {
    if !matches!(expr.kind, AstExprKind::Latest) {
        return simple_flow_mode_with_bindings(expr, expressions, bindings);
    }
    statement
        .children
        .iter()
        .filter_map(|child| {
            direct_statement_value_expr_id(child, expressions)
                .and_then(|expr_id| expressions.get(expr_id))
                .map(|expr| simple_statement_flow_mode(child, expr, expressions, bindings))
        })
        .reduce(merge_flow_modes)
        .unwrap_or(FlowMode::Continuous)
}

fn simple_flow_mode_with_bindings(
    expr: &AstExpr,
    expressions: &[AstExpr],
    bindings: &BTreeMap<String, FlowMode>,
) -> FlowMode {
    match &expr.kind {
        AstExprKind::Identifier(path) => {
            flow_binding_mode(bindings, path).unwrap_or(FlowMode::Continuous)
        }
        AstExprKind::Path(parts) => {
            let path = external_value_path(parts).unwrap_or_else(|| parts.join("."));
            flow_binding_mode(bindings, &path).unwrap_or(FlowMode::Continuous)
        }
        AstExprKind::Call { args, .. } => args
            .iter()
            .filter_map(|arg| expressions.get(arg.value))
            .map(|arg| simple_flow_mode_with_bindings(arg, expressions, bindings))
            .fold(FlowMode::Continuous, merge_flow_modes),
        AstExprKind::Pipe {
            input, op, args, ..
        } if op != "WHILE" => args
            .iter()
            .filter_map(|arg| expressions.get(arg.value))
            .chain(expressions.get(*input))
            .map(|input| simple_flow_mode_with_bindings(input, expressions, bindings))
            .fold(FlowMode::Continuous, merge_flow_modes),
        AstExprKind::When { input, .. } | AstExprKind::Draining { input } => expressions
            .get(*input)
            .map(|input| simple_flow_mode_with_bindings(input, expressions, bindings))
            .unwrap_or(FlowMode::Continuous),
        _ => simple_flow_mode(expr, expressions),
    }
}

fn simple_flow_mode(expr: &AstExpr, expressions: &[AstExpr]) -> FlowMode {
    match &expr.kind {
        AstExprKind::Source | AstExprKind::Then { .. } => FlowMode::PresentOrAbsent,
        AstExprKind::When { input, .. } => expressions
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
                    bindings,
                );
            }
            AstStatementKind::Function { .. } => {
                collect_name_bindings(
                    &statement.children,
                    expressions,
                    scope,
                    source_payload_types,
                    bindings,
                );
            }
            _ => collect_name_bindings(
                &statement.children,
                expressions,
                scope,
                source_payload_types,
                bindings,
            ),
        }
    }
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

fn statements_define_explicit_record(statements: &[AstStatement], expressions: &[AstExpr]) -> bool {
    statements.iter().any(|statement| {
        if semantic_block_statement(statement, expressions) {
            return semantic_block_return_statement(&statement.children).is_some_and(|returned| {
                statements_define_explicit_record(std::slice::from_ref(returned), expressions)
            });
        }
        matches!(statement.kind, AstStatementKind::Block)
            && statement
                .children
                .iter()
                .any(|child| statement_field(child).is_some())
    })
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

fn widen_structural_type(left: &Type, right: &Type) -> Type {
    if is_value_placeholder_type(left) {
        return right.clone();
    }
    if is_value_placeholder_type(right) {
        return left.clone();
    }
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
        (Type::Bytes(left), Type::Bytes(right)) => match (left, right) {
            (BytesType::Fixed(left), BytesType::Fixed(right)) if left == right => {
                Type::Bytes(BytesType::Fixed(*left))
            }
            _ => Type::Bytes(BytesType::Dynamic),
        },
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

fn widen_hold_type(current: &Type, update: &Type) -> Type {
    if is_specific_type(current) && !is_specific_type(update) {
        current.clone()
    } else {
        widen_structural_type(current, update)
    }
}

fn widen_checked_hold_type(current: &Type, update: &Type) -> Type {
    if is_value_placeholder_type(update) {
        current.clone()
    } else if is_value_placeholder_type(current) {
        update.clone()
    } else {
        widen_hold_type(current, update)
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

fn type_from_longest_binding_prefix(
    bindings: &BTreeMap<String, Type>,
    parts: &[String],
) -> Option<Type> {
    for prefix_len in (1..=parts.len()).rev() {
        let prefix = boon_parser::canonical_value_path(&parts[..prefix_len]);
        let Some(base) = bindings.get(&prefix) else {
            continue;
        };
        if let Some(ty) = type_for_nested_path(base, &parts[prefix_len..]) {
            return Some(ty);
        }
    }
    None
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

fn source_path_part_candidates(parts: &[String]) -> Vec<Vec<String>> {
    let mut candidates = Vec::new();
    for (strip_event, strip_events) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let candidate = parts
            .iter()
            .filter(|part| {
                part.as_str() != "PASSED"
                    && (!strip_event || part.as_str() != "event")
                    && (!strip_events || part.as_str() != "events")
            })
            .cloned()
            .collect::<Vec<_>>();
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn source_payload_access_for_suffix(suffix: &str) -> SourcePayloadAccess {
    let suffix = suffix
        .strip_prefix("event.")
        .or_else(|| suffix.strip_prefix("events."))
        .unwrap_or(suffix);
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
    host_ports: &HostPortTable,
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
    collect_payload_hold_update_types(
        &program.ast.statements,
        &program.ast.statements,
        &program.expressions,
        source_lookup,
        &mut fields_by_source,
    );
    for expr in &program.expressions {
        let AstExprKind::Call { function, args, .. } = &expr.kind else {
            continue;
        };
        let Some(signature) = host_effect_signature(function) else {
            continue;
        };
        for (argument_name, argument_value) in named_call_argument_exprs(program, expr.id, args) {
            let Some(expected_field) = signature
                .intent_fields
                .iter()
                .find(|field| field.name == argument_name)
            else {
                continue;
            };
            let expected_type = &expected_field.ty;
            let Some(AstExpr {
                kind: AstExprKind::Path(parts),
                ..
            }) = program.expressions.get(argument_value)
            else {
                continue;
            };
            let Some(SourcePayloadAccess::Field(payload_field)) =
                source_lookup.access_for_parts(parts)
            else {
                continue;
            };
            for source_path in source_lookup.source_paths_for_parts(parts) {
                if let Some(fields) = fields_by_source.get_mut(&source_path) {
                    fields.insert(payload_field.clone(), expected_type.clone());
                }
            }
        }
    }
    for (source_path, host_fields) in host_port_payload_types(host_ports) {
        let Some(fields) = fields_by_source.get_mut(&source_path) else {
            continue;
        };
        for (name, ty) in host_fields {
            fields.insert(name, ty);
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

fn host_port_payload_types(host_ports: &HostPortTable) -> BTreeMap<String, BTreeMap<String, Type>> {
    let mut payloads = BTreeMap::new();
    if let Some(http) = &host_ports.http {
        payloads.insert(http.request_source.clone(), http_request_payload_fields());
        if let Some(disconnect) = &http.disconnect_source {
            payloads.insert(
                disconnect.clone(),
                BTreeMap::from([
                    ("peer".to_owned(), Type::Text),
                    ("reason".to_owned(), Type::Text),
                ]),
            );
        }
    }
    if let Some(websocket) = &host_ports.websocket {
        payloads.insert(
            websocket.open_source.clone(),
            BTreeMap::from([
                ("path".to_owned(), Type::Text),
                ("path_segments".to_owned(), Type::List(Box::new(Type::Text))),
                ("query".to_owned(), named_text_pairs_type()),
                ("headers".to_owned(), named_text_pairs_type()),
                ("cookies".to_owned(), named_text_pairs_type()),
                ("peer".to_owned(), Type::Text),
                ("protocols".to_owned(), Type::List(Box::new(Type::Text))),
            ]),
        );
        payloads.insert(
            websocket.message_source.clone(),
            BTreeMap::from([
                (
                    "kind".to_owned(),
                    Type::VariantSet(vec![
                        Variant::Tag("TextMessage".to_owned()),
                        Variant::Tag("BinaryMessage".to_owned()),
                    ]),
                ),
                ("text".to_owned(), Type::Text),
                ("bytes".to_owned(), Type::Bytes(BytesType::Dynamic)),
            ]),
        );
        payloads.insert(
            websocket.close_source.clone(),
            BTreeMap::from([
                ("code".to_owned(), Type::Number),
                ("reason".to_owned(), Type::Text),
                ("clean".to_owned(), true_false_type()),
            ]),
        );
        payloads.insert(
            websocket.error_source.clone(),
            BTreeMap::from([
                ("code".to_owned(), Type::Text),
                ("message".to_owned(), Type::Text),
                ("retryable".to_owned(), true_false_type()),
            ]),
        );
    }
    payloads
}

fn http_request_payload_fields() -> BTreeMap<String, Type> {
    BTreeMap::from([
        ("method".to_owned(), Type::Text),
        ("scheme".to_owned(), Type::Text),
        ("path".to_owned(), Type::Text),
        ("path_segments".to_owned(), Type::List(Box::new(Type::Text))),
        ("query".to_owned(), named_text_pairs_type()),
        ("headers".to_owned(), named_text_pairs_type()),
        ("cookies".to_owned(), named_text_pairs_type()),
        ("body".to_owned(), Type::Bytes(BytesType::Dynamic)),
        ("peer".to_owned(), Type::Text),
        ("deadline_ms".to_owned(), Type::Number),
    ])
}

fn named_text_pairs_type() -> Type {
    Type::List(Box::new(Type::Object(ObjectShape::from_ordered_fields(
        [
            ("name".to_owned(), Type::Text),
            ("value".to_owned(), Type::Text),
        ],
        false,
    ))))
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
            AstStatementKind::Function { name, parameters } => {
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
                    let arg_ranges = function_arg_ranges(parameters);
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
            | "List/filter"
            | "List/remove"
            | "List/query_prefix"
            | "List/move_field_first"
            | "List/move_field_last"
            | "SOURCE" => ty,
            "List/query" => indexed_query_page_type(),
            "List/count" | "List/sum" => Type::Number,
            "Text/join" => Type::Text,
            "List/append" => {
                let append_ty = args
                    .iter()
                    .find(|arg| arg.named_name() == Some("item"))
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
            AstExprKind::Path(parts) => type_from_longest_binding_prefix(name_bindings, parts),
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

fn function_arg_ranges(parameters: &[AstParameter]) -> Vec<Option<(usize, usize)>> {
    parameters
        .iter()
        .map(|parameter| Some((parameter.start, parameter.end)))
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
    let name = arg.named_name()?;
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
                kind: AstExprKind::When { input, .. },
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

fn collect_payload_hold_update_types(
    roots: &[AstStatement],
    statements: &[AstStatement],
    expressions: &[AstExpr],
    source_lookup: &SourcePayloadPathLookup,
    fields_by_source: &mut BTreeMap<String, BTreeMap<String, Type>>,
) {
    for statement in statements {
        if let Some(expr_id) = statement.expr
            && let Some(AstExpr {
                kind: AstExprKind::Hold { initial, .. },
                ..
            }) = expressions.get(expr_id)
        {
            let initial = pipeline_source_expr_id(roots, expr_id, *initial, expressions);
            let expected = expressions
                .get(initial)
                .map(|expression| simple_expr_type(expression, expressions))
                .map(|ty| {
                    if type_accepts_true_false(&ty) {
                        true_false_type()
                    } else {
                        ty
                    }
                });
            if let Some(expected) = expected
                .filter(is_specific_type)
                .filter(|ty| !matches!(ty, Type::RenderContract))
            {
                for update in hold_update_exprs(statement, expressions) {
                    let Some(AstExpr {
                        kind: AstExprKind::Path(parts),
                        ..
                    }) = expressions.get(update)
                    else {
                        continue;
                    };
                    let Some(SourcePayloadAccess::Field(field)) =
                        source_lookup.access_for_parts(parts)
                    else {
                        continue;
                    };
                    for source_path in source_lookup.source_paths_for_parts(parts) {
                        if let Some(fields) = fields_by_source.get_mut(&source_path) {
                            fields.insert(field.clone(), expected.clone());
                        }
                    }
                }
            }
        }
        collect_payload_hold_update_types(
            roots,
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
        source_path_part_candidates(parts)
            .into_iter()
            .find_map(|parts| self.access_for_normalized_parts(&parts))
    }

    fn access_for_normalized_parts(
        &self,
        normalized_parts: &[String],
    ) -> Option<SourcePayloadAccess> {
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
        for normalized_parts in source_path_part_candidates(parts) {
            let matches = self.source_paths_for_normalized_parts(&normalized_parts);
            if !matches.is_empty() {
                return matches;
            }
        }
        Vec::new()
    }

    fn source_paths_for_normalized_parts(&self, normalized_parts: &[String]) -> Vec<String> {
        let path = normalized_parts.join(".");
        let path_without_payload = parts_without_payload(normalized_parts).join(".");
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
        Some(AstExprKind::Pipe { input, .. }) | Some(AstExprKind::When { input, .. }) => {
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
    let parts = path.split('.').map(str::to_owned).collect::<Vec<_>>();
    source_path_part_candidates(&parts)
        .into_iter()
        .map(|parts| parts.join("."))
        .any(|normalized_path| {
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

fn session_info_intrinsic_type(function: &str) -> Option<Type> {
    match function {
        "SessionInfo/status" => Some(Type::VariantSet(vec![
            Variant::Tag("Connecting".to_owned()),
            Variant::Tag("Current".to_owned()),
            Variant::Tag("Stale".to_owned()),
            Variant::Tagged {
                tag: "Failed".to_owned(),
                fields: ObjectShape::from_ordered_fields([("code".to_owned(), Type::Text)], false),
            },
        ])),
        "SessionInfo/principal" => Some(Type::VariantSet(vec![
            Variant::Tag("Anonymous".to_owned()),
            Variant::Tagged {
                tag: "Authenticated".to_owned(),
                fields: ObjectShape::from_ordered_fields(
                    [
                        ("subject".to_owned(), Type::Text),
                        ("roles".to_owned(), Type::List(Box::new(Type::Text))),
                    ],
                    false,
                ),
            },
        ])),
        _ => None,
    }
}

fn session_info_intrinsic_allowed(function: &str, role: ProgramRole) -> bool {
    match function {
        "SessionInfo/status" => matches!(
            role,
            ProgramRole::Client | ProgramRole::Session | ProgramRole::Server
        ),
        "SessionInfo/principal" => matches!(role, ProgramRole::Session | ProgramRole::Server),
        _ => false,
    }
}

fn session_info_role_diagnostic(function: &str, role: ProgramRole) -> String {
    match (function, role) {
        ("SessionInfo/principal", ProgramRole::Client) => {
            "`SessionInfo/principal()` is unavailable in Client; expose only explicitly selected account-facing data from Session".to_owned()
        }
        _ => format!("`{function}()` is unavailable in {}", role.namespace()),
    }
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

fn indexed_query_page_type() -> Type {
    Type::Object(ObjectShape::from_ordered_fields(
        [
            ("rows".to_owned(), Type::List(Box::new(open_object_type()))),
            ("cursor".to_owned(), Type::Bytes(BytesType::Dynamic)),
        ],
        false,
    ))
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
