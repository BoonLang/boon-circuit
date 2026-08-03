//! Immutable checked-language model shared by compiler phases.
//!
//! This crate deliberately contains no parser or solver implementation. Safe
//! code can inspect and serialize [`CheckedProgramFields`], but only the
//! typechecker may cross the unsafe sealing boundary to create a
//! proof-bearing [`CheckedProgram`].

use boon_contract::SourceBundleDigestV1;
use boon_data::{Bits, ExactNumber};
pub use boon_document_model::ProgramRole;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum Type {
    Text,
    Number,
    Bytes(BytesType),
    Absent,
    VariantSet(SharedVariantSet),
    Object(SharedObjectShape),
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
    /// An ordinary public sum of structurally distinct value types. Closed
    /// Tag alternatives remain normalized as `VariantSet`; this form is for
    /// sums such as a normal scalar/record result plus an exposed FLUSH
    /// payload. Appended to preserve every existing serialized type
    /// discriminant.
    Union(Vec<Type>),
    /// Canonical MAP authority view: one key type and one committed value type.
    Map {
        key: Box<Type>,
        value: Box<Type>,
    },
    /// Canonical SET authority view.
    Set(Box<Type>),
    /// A fixed-width raw bit sequence. Width is part of the static type and is
    /// never inferred from a numeric context.
    Bits {
        width: u32,
    },
}

impl Type {
    /// Seal an owned object shape into a cheaply cloneable type node.
    pub fn object(shape: ObjectShape) -> Self {
        Self::Object(shape.into())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum BytesType {
    Dynamic,
    Fixed(usize),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum Variant {
    Tag(String),
    Tagged {
        tag: String,
        fields: SharedObjectShape,
    },
}

impl Variant {
    /// Seal an owned tagged-payload shape into a cheaply cloneable variant.
    pub fn tagged(tag: String, fields: ObjectShape) -> Self {
        Self::Tagged {
            tag,
            fields: fields.into(),
        }
    }
}

/// An immutable, reference-counted set of canonical Tag alternatives.
///
/// Variant domains are copied through the checked solver and its expression
/// cache far more often than they are rebuilt. Sealing the canonical vector
/// makes those copies constant-time while retaining the exact serialized
/// array and deterministic alternative order.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct SharedVariantSet(Arc<Vec<Variant>>);

impl SharedVariantSet {
    pub fn new(variants: Vec<Variant>) -> Self {
        Self(Arc::new(variants))
    }

    pub fn ptr_eq(left: &Self, right: &Self) -> bool {
        Arc::ptr_eq(&left.0, &right.0)
    }

    pub fn into_owned(self) -> Vec<Variant> {
        Arc::unwrap_or_clone(self.0)
    }
}

impl Deref for SharedVariantSet {
    type Target = Vec<Variant>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Vec<Variant>> for SharedVariantSet {
    fn from(variants: Vec<Variant>) -> Self {
        Self::new(variants)
    }
}

impl FromIterator<Variant> for SharedVariantSet {
    fn from_iter<T: IntoIterator<Item = Variant>>(iter: T) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

impl IntoIterator for SharedVariantSet {
    type Item = Variant;
    type IntoIter = std::vec::IntoIter<Variant>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_owned().into_iter()
    }
}

impl<'a> IntoIterator for &'a SharedVariantSet {
    type Item = &'a Variant;
    type IntoIter = std::slice::Iter<'a, Variant>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Serialize for SharedVariantSet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_ref().serialize(serializer)
    }
}

impl AsRef<Vec<Variant>> for SharedVariantSet {
    fn as_ref(&self) -> &Vec<Variant> {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SharedVariantSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<Variant>::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ObjectShape {
    pub fields: BTreeMap<String, Type>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_order: Vec<String>,
    pub open: bool,
}

/// An immutable, reference-counted object shape used by sealed [`Type`] values.
///
/// Object shapes remain ordinary owned values while they are being assembled.
/// Converting one into this wrapper seals it so cloning a `Type::Object` shares
/// the complete shape (including all recursively nested field types) instead of
/// cloning the field tree.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SharedObjectShape(Arc<ObjectShape>);

impl SharedObjectShape {
    pub fn new(shape: ObjectShape) -> Self {
        Self(Arc::new(shape))
    }

    pub fn ptr_eq(left: &Self, right: &Self) -> bool {
        Arc::ptr_eq(&left.0, &right.0)
    }

    pub fn into_owned(self) -> ObjectShape {
        Arc::unwrap_or_clone(self.0)
    }
}

impl AsRef<ObjectShape> for SharedObjectShape {
    fn as_ref(&self) -> &ObjectShape {
        &self.0
    }
}

impl Deref for SharedObjectShape {
    type Target = ObjectShape;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<ObjectShape> for SharedObjectShape {
    fn from(shape: ObjectShape) -> Self {
        Self::new(shape)
    }
}

impl Serialize for SharedObjectShape {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_ref().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SharedObjectShape {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ObjectShape::deserialize(deserializer).map(Self::new)
    }
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
    Map {
        key: Box<TypeDisplayNode>,
        value: Box<TypeDisplayNode>,
    },
    Set {
        item: Box<TypeDisplayNode>,
    },
    Bits {
        width: u32,
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
    pub fn new<T>(fields: BTreeMap<String, Type>, open: bool) -> T
    where
        T: From<Self>,
    {
        let field_order = fields.keys().cloned().collect();
        Self {
            fields,
            field_order,
            open,
        }
        .into()
    }

    pub fn from_ordered_fields<T>(fields: impl IntoIterator<Item = (String, Type)>, open: bool) -> T
    where
        T: From<Self>,
    {
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
        .into()
    }

    pub fn ordered_fields(&self) -> Vec<(&String, &Type)> {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeScheme {
    pub vars: Vec<TypeVar>,
    pub ty: FlowType,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct FlowType {
    pub mode: FlowMode,
    pub ty: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExternalFunctionArgument {
    pub name: String,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExternalFunctionType {
    pub args: Vec<ExternalFunctionArgument>,
    pub result: FlowType,
    pub effect: CheckedEffectSummary,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedExternalDeclarationKind {
    Value,
    Callable,
}

/// Source-bound identity of the declaration selected by a distributed
/// interface solution.
///
/// Canonical external names remain diagnostic metadata. Semantic linking uses
/// this identity and therefore cannot silently retarget when names collide or
/// source bundles change.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CheckedExternalDeclarationIdentityV1 {
    pub producer_role: ProgramRole,
    pub producer_source_bundle_digest_v1: SourceBundleDigestV1,
    pub producer_declaration: DeclId,
    pub kind: CheckedExternalDeclarationKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExternalTypeEnvironment {
    pub current_role: ProgramRole,
    pub values: BTreeMap<String, FlowType>,
    pub functions: BTreeMap<String, ExternalFunctionType>,
    #[serde(default)]
    pub external_identities: BTreeMap<String, CheckedExternalDeclarationIdentityV1>,
    #[serde(default)]
    pub allow_unresolved: bool,
    #[serde(default)]
    pub require_resolved_identities: bool,
    #[serde(default)]
    pub local_function_requirements: BTreeMap<String, BTreeMap<String, Type>>,
}

impl ExternalTypeEnvironment {
    pub fn empty(current_role: ProgramRole) -> Self {
        Self {
            current_role,
            values: BTreeMap::new(),
            functions: BTreeMap::new(),
            external_identities: BTreeMap::new(),
            allow_unresolved: false,
            require_resolved_identities: false,
            local_function_requirements: BTreeMap::new(),
        }
    }

    pub fn provisional(current_role: ProgramRole) -> Self {
        Self {
            allow_unresolved: true,
            ..Self::empty(current_role)
        }
    }

    pub fn sealed(current_role: ProgramRole) -> Self {
        Self {
            require_resolved_identities: true,
            ..Self::empty(current_role)
        }
    }
}

impl Default for ExternalTypeEnvironment {
    fn default() -> Self {
        Self::empty(ProgramRole::Client)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
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
pub struct FunctionTypeParameterEntry {
    pub formal: DeclId,
    pub ordinal: usize,
    pub name: String,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FunctionTypeEntry {
    pub callable: DeclId,
    pub name: String,
    pub parameters: Vec<FunctionTypeParameterEntry>,
    pub result: FlowType,
    pub effect: CheckedEffectSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct FunctionTypeTable {
    pub entries: Vec<FunctionTypeEntry>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct NamedValueTypeOrigin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<CheckedStatementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<CheckedExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<CheckedSourceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<CheckedStateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<CheckedListId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NamedValueTypeEntry {
    /// Canonical spelling retained for diagnostics/editor presentation.
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<NamedValueTypeOrigin>,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NamedValueTypeTable {
    /// Dense, sorted exact parser statement sites covered by `entries`.
    ///
    /// Paths are presentation-only and may repeat, so totality is stated in
    /// checked identities instead of being inferred from strings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checked_statement_sites: Vec<CheckedStatementId>,
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
    /// Exact checked source origins covered by this payload contract.
    pub checked_sources: Vec<CheckedSourceId>,
    /// Canonical spelling retained for diagnostics/editor presentation only.
    pub diagnostic_path: String,
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

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
/// Stable lexical identity of one user callable's contextual formal.
pub struct ContextFormalId(pub u32);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct CheckedSourceId(pub u32);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct CheckedStateId(pub u32);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct CheckedListId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedSpan {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedMatchPattern {
    Wildcard,
    Number {
        value: ExactNumber,
    },
    Text {
        value: String,
    },
    Tag {
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fields: Vec<String>,
    },
    Binding {
        name: String,
    },
    Bits {
        value: Bits,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedScopeKind {
    Root,
    Function,
    Block,
    Record,
    RepeatedOutput,
    CallContext,
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
    ElementState,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedEvaluationScope {
    #[default]
    Parent,
    Output {
        formal: DeclId,
    },
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclId>,
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
pub enum CheckedTextSegment {
    Static { value: String },
    Dynamic { value: CheckedExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedExpressionKind {
    Read {
        target: DeclId,
        projection: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<CheckedSourceRead>,
    },
    Passed {
        formal: ContextFormalId,
        projection: Vec<String>,
        access: CheckedPassedAccess,
    },
    ExternalRead {
        canonical_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    },
    Drain {
        target: DeclId,
        projection: Vec<String>,
    },
    Text {
        value: String,
    },
    TextTemplate {
        segments: Vec<CheckedTextSegment>,
    },
    Number {
        value: ExactNumber,
    },
    BytesByte {
        value: u8,
    },
    /// Private flow absence produced by the source control spelling `SKIP`.
    ///
    /// This is never an application Tag or serializable Boon value.
    Absent,
    /// Private fail-fast control. `payload` is ordinary closed tag data; the
    /// carrier is consumed only by compiler-inserted lexical boundaries.
    Flush {
        payload: CheckedExprId,
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
    Latest {
        branches: Vec<CheckedExprId>,
    },
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
        pattern: CheckedMatchPattern,
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
    MapEntry {
        key: CheckedExprId,
        value: CheckedExprId,
    },
    Map {
        entries: Vec<CheckedExprId>,
    },
    Set {
        items: Vec<CheckedExprId>,
    },
    Bits {
        value: Bits,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CheckedSourceRead {
    pub source: CheckedSourceId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload_projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedExpression {
    pub id: CheckedExprId,
    pub scope_id: LexicalScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclId>,
    pub flow_type: FlowType,
    /// Checker-private `Flush<E>` control carried by this expression before
    /// a lexical boundary exposes `E` as ordinary public data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flush_type: Option<Type>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<CheckedResourceBinding>,
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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedResourceBinding {
    Source { source: CheckedSourceId },
    State { state: CheckedStateId },
    ListAuthority { list: CheckedListId },
    ListAlias { target: DeclId },
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
pub enum CheckedCallContextKind {
    ElementState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedCallableContext {
    pub name: String,
    pub kind: CheckedCallContextKind,
    pub provider: DeclId,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedCallContext {
    pub declaration: DeclId,
    pub signature: usize,
    pub scope_id: LexicalScopeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedPassedAccess {
    Read,
    Drain,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedContextScheme {
    /// Principal lexical scheme inferred independently of call-site actuals.
    pub flow_type: FlowType,
    /// Exact required leaf projections; extra actual fields remain uncaptured.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projections: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedContextFormal {
    pub id: ContextFormalId,
    pub callable: DeclId,
    pub scheme: CheckedContextScheme,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedParameterKind {
    Value,
    Out,
}

/// Canonical semantics for an omitted optional parameter.
///
/// `CallableProfile` is used by built-ins whose default is part of a versioned
/// callable profile rather than a Boon literal. The profile identifier is
/// retained alongside the callable and parameter identities, so a semantic
/// profile revision changes the checked artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedParameterDefault {
    CallableProfile { profile: String },
    Tag { name: String },
    ExactInteger { value: i64 },
    Text { value: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedParameterRequirement {
    Required,
    Optional { default: CheckedParameterDefault },
}

impl CheckedParameterRequirement {
    pub const fn is_optional(&self) -> bool {
        matches!(self, Self::Optional { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedParameter {
    pub decl_id: DeclId,
    pub name: String,
    pub kind: CheckedParameterKind,
    pub ordinal: usize,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
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
    Remove {
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
    SortBy {
        list: DeclId,
        row: DeclId,
        key: DeclId,
        direction: DeclId,
    },
    ThenBy {
        list: DeclId,
        row: DeclId,
        key: DeclId,
        direction: DeclId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedOrderKey {
    pub call_path: Vec<CheckedCallId>,
    pub key: CheckedExprId,
    pub direction: CheckedOrderDirection,
    pub key_type: Type,
    pub pure: bool,
    pub total: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedOrderDirection {
    Ascending,
    Descending,
    Dynamic { expression: CheckedExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedOrderChain {
    pub keys: Vec<CheckedOrderKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedCallOrderChain {
    pub call: CheckedCallId,
    pub chain: CheckedOrderChain,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedCallableSignature {
    pub decl_id: DeclId,
    pub scope_id: LexicalScopeId,
    pub kind: CheckedCallableKind,
    pub name: String,
    pub intrinsic: Option<CheckedIntrinsicV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub parameters: Vec<CheckedParameter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<CheckedCallableContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_formal: Option<ContextFormalId>,
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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedIntrinsicV1 {
    StreamPulses,
    StreamSkip,
}

impl CheckedCallableSignature {
    pub fn requires_pass(&self) -> bool {
        self.context_formal.is_some()
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// Complete contextual binding state for one checked call.
pub enum CheckedContextBinding {
    Explicit {
        value: CheckedExprId,
        span: CheckedSpan,
    },
    Inherited {
        formal: ContextFormalId,
    },
    None,
}

impl CheckedContextBinding {
    pub const fn explicit(self) -> Option<(CheckedExprId, CheckedSpan)> {
        match self {
            Self::Explicit { value, span } => Some((value, span)),
            Self::Inherited { .. } | Self::None => None,
        }
    }

    pub const fn inherited(self) -> Option<ContextFormalId> {
        match self {
            Self::Inherited { formal } => Some(formal),
            Self::Explicit { .. } | Self::None => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedContextTypeSubstitution {
    pub formal: ContextFormalId,
    pub variable: TypeVar,
    pub value: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedCall {
    pub id: CheckedCallId,
    pub expression: CheckedExprId,
    pub callable: DeclId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_callable: Option<DeclId>,
    pub function: String,
    pub intrinsic: Option<CheckedIntrinsicV1>,
    pub entries: Vec<CheckedCallEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<CheckedCallContext>,
    pub context_binding: CheckedContextBinding,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contextual_substitutions: Vec<CheckedContextTypeSubstitution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_substitutions: Vec<CheckedTypeSubstitution>,
    /// The exact call result was selected from syntax-level discriminants at
    /// this occurrence and can be more precise than the callable's principal
    /// result scheme.
    #[serde(default, skip_serializing_if = "checked_bool_is_false")]
    pub syntax_discriminated_result: bool,
    pub result: FlowType,
    pub role: ProgramRole,
    pub span: CheckedSpan,
}

fn checked_bool_is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CheckedCallResultPath {
    pub call: CheckedCallId,
    pub path: CheckedSemanticPath,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedTypeSubstitution {
    pub variable: TypeVar,
    pub value: Type,
}

pub trait CheckedTypeSubstitutionLookup {
    fn replacement(&self, variable: TypeVar) -> Option<&Type>;
}

impl CheckedTypeSubstitutionLookup for BTreeMap<TypeVar, Type> {
    fn replacement(&self, variable: TypeVar) -> Option<&Type> {
        self.get(&variable)
    }
}

impl CheckedTypeSubstitutionLookup for [CheckedTypeSubstitution] {
    fn replacement(&self, variable: TypeVar) -> Option<&Type> {
        self.iter()
            .find(|substitution| substitution.variable == variable)
            .map(|substitution| &substitution.value)
    }
}

pub fn apply_checked_type_substitutions(
    ty: &Type,
    substitutions: &[CheckedTypeSubstitution],
) -> Type {
    substitute_checked_type_from_lookup(ty, substitutions)
}

/// Applies a checked type environment without copying it into the serialized
/// call-substitution representation.
pub fn apply_checked_type_environment(ty: &Type, substitutions: &BTreeMap<TypeVar, Type>) -> Type {
    substitute_checked_type_from_lookup(ty, substitutions)
}

/// Applies substitutions from any indexed or persistent checked environment.
pub fn apply_checked_type_substitution_lookup(
    ty: &Type,
    substitutions: &(impl CheckedTypeSubstitutionLookup + ?Sized),
) -> Type {
    substitute_checked_type_from_lookup(ty, substitutions)
}

fn checked_type_has_applicable_substitution(
    ty: &Type,
    substitutions: &(impl CheckedTypeSubstitutionLookup + ?Sized),
) -> bool {
    match ty {
        Type::Var(variable) => substitutions
            .replacement(*variable)
            .is_some_and(|replacement| replacement != ty),
        Type::List(item) | Type::Set(item) => {
            checked_type_has_applicable_substitution(item, substitutions)
        }
        Type::Map { key, value } => {
            checked_type_has_applicable_substitution(key, substitutions)
                || checked_type_has_applicable_substitution(value, substitutions)
        }
        Type::Function { args, result } => {
            args.iter()
                .any(|argument| checked_type_has_applicable_substitution(argument, substitutions))
                || checked_type_has_applicable_substitution(&result.ty, substitutions)
        }
        Type::Object(shape) => shape
            .fields
            .values()
            .any(|field| checked_type_has_applicable_substitution(field, substitutions)),
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields
                .fields
                .values()
                .any(|field| checked_type_has_applicable_substitution(field, substitutions)),
        }),
        Type::Union(members) => members
            .iter()
            .any(|member| checked_type_has_applicable_substitution(member, substitutions)),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => false,
    }
}

fn substitute_checked_type_from_lookup(
    ty: &Type,
    substitutions: &(impl CheckedTypeSubstitutionLookup + ?Sized),
) -> Type {
    substitute_checked_type_inner(ty, substitutions, &mut BTreeSet::new())
}

fn substitute_checked_type_inner(
    ty: &Type,
    substitutions: &(impl CheckedTypeSubstitutionLookup + ?Sized),
    active: &mut BTreeSet<TypeVar>,
) -> Type {
    if !checked_type_has_applicable_substitution(ty, substitutions) {
        return ty.clone();
    }
    match ty {
        Type::Var(variable) => {
            let Some(replacement) = substitutions
                .replacement(*variable)
                .filter(|replacement| *replacement != ty)
            else {
                return ty.clone();
            };
            if !active.insert(*variable) {
                return ty.clone();
            }
            let substituted = substitute_checked_type_inner(replacement, substitutions, active);
            active.remove(variable);
            substituted
        }
        Type::List(item) => Type::List(Box::new(substitute_checked_type_inner(
            item,
            substitutions,
            active,
        ))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(substitute_checked_type_inner(key, substitutions, active)),
            value: Box::new(substitute_checked_type_inner(value, substitutions, active)),
        },
        Type::Set(item) => Type::Set(Box::new(substitute_checked_type_inner(
            item,
            substitutions,
            active,
        ))),
        Type::Function { args, result } => Type::Function {
            args: args
                .iter()
                .map(|argument| substitute_checked_type_inner(argument, substitutions, active))
                .collect(),
            result: Box::new(FlowType {
                mode: result.mode,
                ty: substitute_checked_type_inner(&result.ty, substitutions, active),
            }),
        },
        Type::Object(shape) => Type::object(ObjectShape {
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
                        }
                        .into(),
                    },
                })
                .collect(),
        ),
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|member| substitute_checked_type_inner(member, substitutions, active))
                .collect(),
        ),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => ty.clone(),
    }
}

pub fn type_is_recursively_closed(ty: &Type) -> bool {
    match ty {
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract => true,
        Type::List(item) | Type::Set(item) => type_is_recursively_closed(item),
        Type::Map { key, value } => {
            type_is_recursively_closed(key) && type_is_recursively_closed(value)
        }
        Type::Function { args, result } => {
            args.iter().all(type_is_recursively_closed) && type_is_recursively_closed(&result.ty)
        }
        Type::Object(shape) => !shape.open && shape.fields.values().all(type_is_recursively_closed),
        Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            Variant::Tag(_) => true,
            Variant::Tagged { fields, .. } => {
                !fields.open && fields.fields.values().all(type_is_recursively_closed)
            }
        }),
        Type::Union(members) => {
            !members.is_empty() && members.iter().all(type_is_recursively_closed)
        }
        Type::Unknown | Type::UnresolvedShape { .. } | Type::Var(_) => false,
    }
}

/// An opaque, read-only product emitted only by the typechecker.
///
/// External crates may inspect the public DTO fields through [`std::ops::Deref`],
/// but cannot forge the wrapper:
///
/// ```compile_fail
/// use boon_checked::{CheckedProgram, CheckedProgramFields};
///
/// fn forge(fields: CheckedProgramFields) -> CheckedProgram {
///     CheckedProgram { fields }
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckedProgram {
    #[serde(flatten)]
    fields: CheckedProgramFields,
}

/// Public read-only schema projected by an opaque [`CheckedProgram`].
///
/// Deserializing or constructing this DTO never creates a typechecker product.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedProgramFields {
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub role: ProgramRole,
    #[serde(default)]
    pub external_types: ExternalTypeEnvironment,
    #[serde(default)]
    pub lowering_metadata: CheckedProgramLoweringMetadata,
    pub root_scope: LexicalScopeId,
    pub scopes: Vec<CheckedScope>,
    pub declarations: Vec<CheckedDeclaration>,
    pub statements: Vec<CheckedStatement>,
    pub expressions: Vec<CheckedExpression>,
    pub callables: Vec<CheckedCallableSignature>,
    pub context_formals: Vec<CheckedContextFormal>,
    pub calls: Vec<CheckedCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub call_result_paths: Vec<CheckedCallResultPath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_chains: Vec<CheckedCallOrderChain>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pattern_bindings: Vec<CheckedPatternBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_projection_requirements: Vec<CheckedResourceProjectionRequirement>,
    pub sources: Vec<CheckedSource>,
    pub states: Vec<CheckedState>,
    pub lists: Vec<CheckedList>,
    pub occurrences: Vec<SemanticOccurrence>,
}

impl CheckedProgramFields {
    #[doc(hidden)]
    pub fn source_expression(&self, read: &CheckedSourceRead) -> Option<CheckedExprId> {
        self.sources
            .get(read.source.0 as usize)
            .filter(|source| source.id == read.source)
            .map(|source| source.expression)
    }
}

impl std::ops::Deref for CheckedProgram {
    type Target = CheckedProgramFields;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

fn checked_declaration_canonical_path(
    program: &CheckedProgram,
    declaration: &CheckedDeclaration,
) -> Option<String> {
    if !matches!(
        declaration.kind,
        CheckedDeclarationKind::Field
            | CheckedDeclarationKind::Source
            | CheckedDeclarationKind::Hold
            | CheckedDeclarationKind::List
    ) {
        return None;
    }
    let mut segments = vec![declaration.name.clone()];
    let mut scope = declaration.scope_id;
    let mut visited = BTreeSet::new();
    while scope != program.root_scope && visited.insert(scope) {
        let current = program
            .scopes
            .iter()
            .find(|candidate| candidate.id == scope)?;
        if current.kind == CheckedScopeKind::Function {
            return None;
        }
        if let Some(owner) = current.owner
            && let Some(owner) = program
                .declarations
                .iter()
                .find(|candidate| candidate.id == owner)
            && matches!(
                owner.kind,
                CheckedDeclarationKind::Field
                    | CheckedDeclarationKind::Source
                    | CheckedDeclarationKind::Hold
                    | CheckedDeclarationKind::List
            )
        {
            segments.push(owner.name.clone());
        }
        scope = current.parent?;
    }
    segments.reverse();
    Some(segments.join("."))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedResourceProjectionRequirement {
    pub expression: CheckedExprId,
    pub target: DeclId,
    pub projection: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_origins: Vec<CheckedSourceRead>,
    pub required_type: Type,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedSourceUnitMetadata {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    pub start_line: usize,
    pub line_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedProgramLoweringMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_units: Vec<CheckedSourceUnitMetadata>,
    pub original_source_expression_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_payload_shape_table: Vec<SourcePayloadShapeEntry>,
    #[serde(default)]
    pub host_port_table: HostPortTable,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_root_types: Vec<OutputRootTypeEntry>,
    #[serde(default)]
    pub expr_type_table: ExprTypeTable,
    #[serde(default)]
    pub function_type_table: FunctionTypeTable,
    #[serde(default)]
    pub named_value_type_table: NamedValueTypeTable,
    #[serde(default)]
    pub render_slot_table: RenderSlotTable,
    pub checked_expression_count: usize,
    pub dynamic_fallback_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<TypeDiagnostic>,
}

impl CheckedProgram {
    /// Seal a product after the typechecker has proved every invariant.
    ///
    /// # Safety
    ///
    /// The caller must be the successful typechecker construction path and
    /// must prove that every dense identity, reference, type, effect, resource,
    /// and lowering table is complete and internally consistent. The fields
    /// must carry the exact parser-produced `SourceBundleDigestV1`; this seam
    /// must never synthesize an empty or sentinel digest.
    pub unsafe fn from_typechecker_fields_unchecked(fields: CheckedProgramFields) -> Self {
        Self { fields }
    }

    /// Consume the sealed wrapper back into its inspectable DTO.
    ///
    /// This does not create another proof-bearing product; resealing modified
    /// fields still requires the unsafe typechecker invariant boundary.
    pub fn into_fields(self) -> CheckedProgramFields {
        self.fields
    }

    pub fn context_formal(&self, id: ContextFormalId) -> Option<&CheckedContextFormal> {
        self.context_formals.iter().find(|formal| formal.id == id)
    }

    pub fn callable_context_formal(&self, callable: DeclId) -> Option<&CheckedContextFormal> {
        self.context_formals
            .iter()
            .find(|formal| formal.callable == callable)
    }

    pub fn order_chain_for_call(&self, call: CheckedCallId) -> Option<CheckedOrderChain> {
        self.order_chains
            .iter()
            .find(|candidate| candidate.call == call)
            .map(|candidate| candidate.chain.clone())
    }

    pub fn result_path_for_call(&self, call: CheckedCallId) -> Option<&CheckedSemanticPath> {
        self.call_result_paths
            .iter()
            .find(|candidate| candidate.call == call)
            .map(|candidate| &candidate.path)
    }

    #[doc(hidden)]
    pub fn source_expression(&self, read: &CheckedSourceRead) -> Option<CheckedExprId> {
        self.fields.source_expression(read)
    }

    pub fn declaration_path(&self, declaration: DeclId) -> Option<String> {
        self.declarations
            .iter()
            .find(|candidate| candidate.id == declaration)
            .and_then(|declaration| checked_declaration_canonical_path(self, declaration))
    }

    pub fn semantic_path(&self, path: &CheckedSemanticPath) -> Option<String> {
        let declaration = self
            .declarations
            .iter()
            .find(|candidate| candidate.id == path.anchor)?;
        if declaration.kind == CheckedDeclarationKind::Function {
            return (!path.projection.is_empty()).then(|| path.projection.join("."));
        }
        let mut segments = vec![declaration.name.clone()];
        let mut scope = declaration.scope_id;
        let mut visited = BTreeSet::new();
        while scope != self.root_scope && visited.insert(scope) {
            let current = self.scopes.iter().find(|candidate| candidate.id == scope)?;
            if current.kind == CheckedScopeKind::Function {
                break;
            }
            if let Some(owner) = current.owner
                && let Some(owner) = self
                    .declarations
                    .iter()
                    .find(|candidate| candidate.id == owner)
                && matches!(
                    owner.kind,
                    CheckedDeclarationKind::Field
                        | CheckedDeclarationKind::Source
                        | CheckedDeclarationKind::Hold
                        | CheckedDeclarationKind::List
                )
            {
                segments.push(owner.name.clone());
            }
            scope = current.parent?;
        }
        segments.reverse();
        let mut result = segments.join(".");
        if !path.projection.is_empty() {
            result.push('.');
            result.push_str(&path.projection.join("."));
        }
        Some(result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedPatternBinding {
    pub declaration: DeclId,
    pub selector: CheckedExprId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CheckedSemanticPath {
    pub anchor: DeclId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedSource {
    pub id: CheckedSourceId,
    pub declaration: DeclId,
    pub statement: CheckedStatementId,
    pub expression: CheckedExprId,
    pub owner_scope: LexicalScopeId,
    pub path: CheckedSemanticPath,
    pub interval_ms: Option<u64>,
    pub payload_type: Type,
    pub span: CheckedSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckedStateKind {
    Hold,
    InitialLatest,
    StatefulCall,
    StatementHold,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedState {
    pub id: CheckedStateId,
    pub declaration: DeclId,
    pub statement: CheckedStatementId,
    pub expression: CheckedExprId,
    pub initial: CheckedExprId,
    pub owner_scope: LexicalScopeId,
    pub path: CheckedSemanticPath,
    pub kind: CheckedStateKind,
    pub flow_type: FlowType,
    pub span: CheckedSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedListKeyPolicy {
    GeneratedOccurrenceU64 { has_generation: bool },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedList {
    pub id: CheckedListId,
    pub declaration: DeclId,
    pub statement: CheckedStatementId,
    pub producer: CheckedExprId,
    pub owner_scope: LexicalScopeId,
    pub path: CheckedSemanticPath,
    pub item_type: Type,
    pub capacity: Option<usize>,
    pub key_policy: CheckedListKeyPolicy,
    pub span: CheckedSpan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedHostSourcePortBinding {
    pub source: CheckedSourceId,
    /// Canonical spelling retained for diagnostics/editor presentation.
    pub diagnostic_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckedHostOutputPortBinding {
    pub declaration: DeclId,
    pub statement: CheckedStatementId,
    /// Canonical spelling retained for diagnostics/editor presentation.
    pub diagnostic_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HttpServerPortTypeEntry {
    pub line: usize,
    pub request: CheckedHostSourcePortBinding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disconnect: Option<CheckedHostSourcePortBinding>,
    pub response: CheckedHostOutputPortBinding,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebSocketServerPortTypeEntry {
    pub line: usize,
    pub open: CheckedHostSourcePortBinding,
    pub message: CheckedHostSourcePortBinding,
    pub close: CheckedHostSourcePortBinding,
    pub error: CheckedHostSourcePortBinding,
    pub actions: CheckedHostOutputPortBinding,
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
    /// Diagnostic spelling retained for host/editor presentation.
    pub name: String,
    pub declaration: DeclId,
    pub statement: CheckedStatementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<CheckedExprId>,
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

const RENDERABLE_KINDS: &[&str] = &[
    "Block",
    "Button",
    "Checkbox",
    "Document",
    "EmbeddedMedia",
    "EmbeddedProgram",
    "Label",
    "Link",
    "MapViewport",
    "Paragraph",
    "Row",
    "Scene",
    "Stack",
    "Text",
    "TextInput",
];

pub fn is_registered_render_constructor(function: &str) -> bool {
    RENDER_CONSTRUCTORS.contains(&function)
}

pub fn is_typed_host_effect(operation: &str) -> bool {
    let Some(spec) = boon_effect_schema::host_effect_spec(operation) else {
        return false;
    };
    let Some(schema) = spec.schema else {
        return false;
    };
    spec.result_policy == boon_effect_schema::ResultPolicySpec::ReturnValue
        && matches!(
            schema.intent,
            boon_effect_schema::ValueType::Record { open: false, .. }
        )
}

pub fn variants_use_boolean_runtime_representation(variants: &[Variant]) -> bool {
    !variants.is_empty()
        && variants
            .iter()
            .all(|variant| matches!(variant, Variant::Tag(tag) if tag == "True" || tag == "False"))
}

pub fn is_renderable_type(ty: &Type) -> bool {
    if matches!(ty, Type::RenderContract) || is_no_element_type(ty) {
        return true;
    }
    let Type::Object(shape) = ty else {
        return false;
    };
    matches!(
        shape.fields.get("kind"),
        Some(Type::VariantSet(variants))
            if variants.iter().all(|variant| {
                matches!(variant, Variant::Tag(tag) if RENDERABLE_KINDS.contains(&tag.as_str()))
            })
    )
}

pub fn resolved_type_is_assignable_to(actual: &Type, expected: &Type) -> bool {
    if actual == expected {
        return true;
    }
    match (actual, expected) {
        (Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }, _)
        | (_, Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }) => false,
        (Type::Union(actual), _) if actual.is_empty() => false,
        (_, Type::Union(expected)) if expected.is_empty() => false,
        (Type::Union(actual), Type::Union(expected)) => actual.iter().all(|actual| {
            expected
                .iter()
                .any(|expected| resolved_type_is_assignable_to(actual, expected))
        }),
        (Type::Union(actual), expected) => actual
            .iter()
            .all(|actual| resolved_type_is_assignable_to(actual, expected)),
        (actual, Type::Union(expected)) => expected
            .iter()
            .any(|expected| resolved_type_is_assignable_to(actual, expected)),
        (Type::Text, Type::Text)
        | (Type::Number, Type::Number)
        | (Type::Absent, Type::Absent)
        | (Type::RenderContract, Type::RenderContract) => true,
        (Type::Bytes(actual), Type::Bytes(expected)) => bytes_type_assignable(actual, expected),
        (Type::Bits { width: actual }, Type::Bits { width: expected }) => actual == expected,
        (actual, Type::RenderContract) => is_renderable_type(actual),
        (Type::List(actual), Type::List(expected)) => {
            resolved_type_is_assignable_to(actual, expected)
        }
        (Type::Object(actual), Type::Object(expected)) => {
            expected.fields.iter().all(|(field, expected_field)| {
                actual.fields.get(field).is_some_and(|actual_field| {
                    resolved_type_is_assignable_to(actual_field, expected_field)
                })
            })
        }
        (Type::VariantSet(actual), Type::VariantSet(expected)) => actual.iter().all(|actual| {
            expected
                .iter()
                .any(|expected| resolved_variant_is_assignable_to(actual, expected))
        }),
        (
            Type::Function {
                args: actual_args,
                result: actual_result,
            },
            Type::Function {
                args: expected_args,
                result: expected_result,
            },
        ) => {
            actual_args.len() == expected_args.len()
                && actual_result.mode == expected_result.mode
                && expected_args
                    .iter()
                    .zip(actual_args)
                    .all(|(expected, actual)| resolved_type_is_assignable_to(expected, actual))
                && resolved_type_is_assignable_to(&actual_result.ty, &expected_result.ty)
        }
        _ => false,
    }
}

fn resolved_variant_is_assignable_to(actual: &Variant, expected: &Variant) -> bool {
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
                && resolved_type_is_assignable_to(
                    &Type::Object(actual_fields.clone()),
                    &Type::Object(expected_fields.clone()),
                )
        }
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

pub fn specialize_checked_call_result(instantiated: &Type, occurrence: &Type) -> Type {
    if is_value_placeholder_type(occurrence) {
        return instantiated.clone();
    }
    if is_value_placeholder_type(instantiated) {
        return occurrence.clone();
    }
    if type_is_recursively_closed(instantiated) {
        return instantiated.clone();
    }
    match (instantiated, occurrence) {
        (Type::List(instantiated), Type::List(occurrence)) => Type::List(Box::new(
            specialize_checked_call_result(instantiated, occurrence),
        )),
        (
            Type::Map {
                key: instantiated_key,
                value: instantiated_value,
            },
            Type::Map {
                key: occurrence_key,
                value: occurrence_value,
            },
        ) => Type::Map {
            key: Box::new(specialize_checked_call_result(
                instantiated_key,
                occurrence_key,
            )),
            value: Box::new(specialize_checked_call_result(
                instantiated_value,
                occurrence_value,
            )),
        },
        (Type::Set(instantiated), Type::Set(occurrence)) => Type::Set(Box::new(
            specialize_checked_call_result(instantiated, occurrence),
        )),
        (Type::Object(instantiated), Type::Object(occurrence)) => {
            let mut fields = instantiated.fields.clone();
            for (name, occurrence_type) in occurrence.ordered_fields() {
                if let Some(instantiated_type) = fields.get_mut(name) {
                    *instantiated_type =
                        specialize_checked_call_result(instantiated_type, occurrence_type);
                } else if instantiated.open {
                    fields.insert(name.clone(), occurrence_type.clone());
                }
            }
            Type::object(ObjectShape {
                fields,
                field_order: object_field_order_for_widened_shapes(instantiated, occurrence),
                open: instantiated.open && occurrence.open,
            })
        }
        (Type::VariantSet(instantiated), Type::VariantSet(occurrence)) => {
            let mut variants = instantiated.clone().into_owned();
            for occurrence_variant in occurrence {
                let Variant::Tagged {
                    tag,
                    fields: occurrence_fields,
                } = occurrence_variant
                else {
                    if !variants.contains(occurrence_variant) {
                        variants.push(occurrence_variant.clone());
                    }
                    continue;
                };
                let Some(existing) = variants.iter_mut().find(
                    |variant| matches!(variant, Variant::Tagged { tag: candidate, .. } if candidate == tag),
                ) else {
                    variants.push(occurrence_variant.clone());
                    continue;
                };
                let Variant::Tagged {
                    fields: instantiated_fields,
                    ..
                } = existing
                else {
                    unreachable!("tagged occurrence matched a tagged variant")
                };
                let Type::Object(specialized) = specialize_checked_call_result(
                    &Type::Object(instantiated_fields.clone()),
                    &Type::Object(occurrence_fields.clone()),
                ) else {
                    unreachable!("tagged payload specialization is an object")
                };
                *instantiated_fields = specialized;
            }
            Type::VariantSet(variants.into())
        }
        (Type::Union(instantiated), Type::Union(occurrence))
            if instantiated.len() == occurrence.len() =>
        {
            Type::Union(
                instantiated
                    .iter()
                    .zip(occurrence)
                    .map(|(instantiated, occurrence)| {
                        specialize_checked_call_result(instantiated, occurrence)
                    })
                    .collect(),
            )
        }
        _ => instantiated.clone(),
    }
}

pub fn canonical_union_type(candidates: Vec<Type>) -> Type {
    let mut members = Vec::new();
    let mut variants = Vec::new();
    for candidate in candidates {
        let candidates = match candidate {
            Type::Union(candidates) => candidates,
            candidate => vec![candidate],
        };
        for candidate in candidates {
            match candidate {
                Type::Absent => {}
                Type::VariantSet(candidate_variants) => {
                    for variant in candidate_variants {
                        merge_structural_variant(&mut variants, variant);
                    }
                }
                candidate if !members.contains(&candidate) => members.push(candidate),
                _ => {}
            }
        }
    }
    if !variants.is_empty() {
        variants.sort_by_key(variant_sort_key);
        members.push(Type::VariantSet(variants.into()));
    }
    members.sort_by_key(|member| format!("{member:?}"));
    members.dedup();
    match members.as_slice() {
        [] => Type::Absent,
        [member] => member.clone(),
        _ => Type::Union(members),
    }
}

fn merge_structural_variant(variants: &mut Vec<Variant>, incoming: Variant) {
    match incoming {
        Variant::Tag(incoming_tag) => {
            if !variants.iter().any(|variant| match variant {
                Variant::Tag(tag) | Variant::Tagged { tag, .. } => tag == &incoming_tag,
            }) {
                variants.push(Variant::Tag(incoming_tag));
            }
        }
        Variant::Tagged {
            tag: incoming_tag,
            fields: incoming_fields,
        } => {
            let Some(index) = variants.iter().position(|variant| match variant {
                Variant::Tag(tag) | Variant::Tagged { tag, .. } => tag == &incoming_tag,
            }) else {
                variants.push(Variant::Tagged {
                    tag: incoming_tag,
                    fields: incoming_fields,
                });
                return;
            };
            let fields = match &variants[index] {
                Variant::Tag(_) => incoming_fields,
                Variant::Tagged {
                    fields: existing_fields,
                    ..
                } => {
                    let merged = widen_structural_type(
                        &Type::Object(existing_fields.clone()),
                        &Type::Object(incoming_fields),
                    );
                    let Type::Object(fields) = merged else {
                        unreachable!("widening tagged payloads must produce an object")
                    };
                    fields
                }
            };
            variants[index] = Variant::Tagged {
                tag: incoming_tag,
                fields,
            };
        }
    }
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
            let mut variants = left.clone().into_owned();
            for variant in right {
                merge_structural_variant(&mut variants, variant.clone());
            }
            variants.sort_by_key(variant_sort_key);
            Type::VariantSet(variants.into())
        }
        (Type::Absent, ty) | (ty, Type::Absent) => ty.clone(),
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
        (Type::Bits { width: left }, Type::Bits { width: right }) if left == right => {
            Type::Bits { width: *left }
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
            Type::object(ObjectShape {
                fields,
                field_order: object_field_order_for_widened_shapes(left, right),
                open: left.open || right.open,
            })
        }
        _ => open_object_type(),
    }
}

fn is_value_placeholder_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
    ) || matches!(ty, Type::Object(shape) if shape.open && shape.fields.is_empty())
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

fn open_object_type() -> Type {
    Type::object(ObjectShape::new(BTreeMap::new(), true))
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

fn variant_sort_key(variant: &Variant) -> String {
    match variant {
        Variant::Tag(tag) => format!("0:{tag}"),
        Variant::Tagged { tag, fields } => format!("1:{tag}:{}", fields.fields.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str) -> Variant {
        Variant::Tag(name.to_owned())
    }

    fn object(fields: impl IntoIterator<Item = (&'static str, Type)>, open: bool) -> Type {
        Type::object(ObjectShape::from_ordered_fields(
            fields.into_iter().map(|(name, ty)| (name.to_owned(), ty)),
            open,
        ))
    }

    #[test]
    fn shared_checked_nodes_clone_without_copying_and_preserve_wire_shape() {
        let variants = SharedVariantSet::new(vec![tag("False"), tag("True")]);
        let variants_clone = variants.clone();
        assert!(SharedVariantSet::ptr_eq(&variants, &variants_clone));

        let shape = SharedObjectShape::new(ObjectShape::from_ordered_fields(
            [("kind".to_owned(), Type::VariantSet(variants))],
            false,
        ));
        let shape_clone = shape.clone();
        assert!(SharedObjectShape::ptr_eq(&shape, &shape_clone));

        let ty = Type::Object(shape);
        let encoded = serde_json::to_string(&ty).expect("checked type serializes");
        let decoded: Type = serde_json::from_str(&encoded).expect("checked type deserializes");
        assert_eq!(decoded, ty);
    }

    #[test]
    fn checked_contract_classification_matches_language_contracts() {
        assert!(is_registered_render_constructor("Document/new"));
        assert!(is_registered_render_constructor("Scene/Element/text"));
        assert!(!is_registered_render_constructor("Element/not_registered"));

        assert!(is_typed_host_effect("DevelopmentPasskey/register"));
        assert!(!is_typed_host_effect("Host/not_registered"));

        assert!(variants_use_boolean_runtime_representation(&[
            tag("False"),
            tag("True"),
        ]));
        assert!(!variants_use_boolean_runtime_representation(&[]));
        assert!(!variants_use_boolean_runtime_representation(&[
            tag("False"),
            tag("Maybe"),
        ]));
    }

    #[test]
    fn renderability_uses_the_shared_checked_kind_contract() {
        assert!(is_renderable_type(&Type::RenderContract));
        assert!(is_renderable_type(&Type::VariantSet(
            vec![tag("NoElement")].into()
        )));
        assert!(is_renderable_type(&object(
            [("kind", Type::VariantSet(vec![tag("Row")].into()))],
            false,
        )));
        assert!(!is_renderable_type(&object(
            [("kind", Type::VariantSet(vec![tag("UnknownKind")].into()))],
            false,
        )));
        assert!(!is_renderable_type(&object([("text", Type::Text)], false)));
    }

    #[test]
    fn resolved_assignability_is_strict_and_structural() {
        let expected = object([("name", Type::Text)], false);
        let extra_field = object([("name", Type::Text), ("count", Type::Number)], false);
        let missing_field = object([("count", Type::Number)], false);

        assert!(resolved_type_is_assignable_to(&extra_field, &expected));
        assert!(!resolved_type_is_assignable_to(&missing_field, &expected));
        assert!(!resolved_type_is_assignable_to(&Type::Unknown, &expected));
        assert!(resolved_type_is_assignable_to(
            &Type::Bytes(BytesType::Fixed(8)),
            &Type::Bytes(BytesType::Dynamic),
        ));
        assert!(!resolved_type_is_assignable_to(
            &Type::Bytes(BytesType::Dynamic),
            &Type::Bytes(BytesType::Fixed(8)),
        ));
    }

    #[test]
    fn checked_call_specialization_keeps_the_principal_shape_and_fills_unknowns() {
        let principal = object([("value", Type::Unknown)], true);
        let occurrence = object([("value", Type::Text), ("count", Type::Number)], false);
        let specialized = specialize_checked_call_result(&principal, &occurrence);
        let Type::Object(shape) = specialized else {
            panic!("object specialization must remain an object");
        };
        assert_eq!(shape.fields.get("value"), Some(&Type::Text));
        assert_eq!(shape.fields.get("count"), Some(&Type::Number));
        assert!(!shape.open);
    }

    #[test]
    fn canonical_union_is_order_independent_and_merges_variant_domains() {
        let candidates = vec![
            Type::VariantSet(vec![tag("True")].into()),
            Type::Number,
            Type::Absent,
            Type::VariantSet(vec![tag("False")].into()),
            Type::Number,
        ];
        let forward = canonical_union_type(candidates.clone());
        let reverse = canonical_union_type(candidates.into_iter().rev().collect());
        assert_eq!(forward, reverse);

        let Type::Union(members) = forward else {
            panic!("number plus variants must remain a union");
        };
        assert!(members.contains(&Type::Number));
        assert!(members.contains(&Type::VariantSet(vec![tag("False"), tag("True")].into())));
    }
}
