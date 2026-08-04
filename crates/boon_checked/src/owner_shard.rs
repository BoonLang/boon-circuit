//! Stable, span-free checked rows for one independently checked syntax owner.
//!
//! These DTOs deliberately use owner-local identities and explicit stable
//! relocations. They are not a second `CheckedProgram`: compatibility dense
//! IDs are assigned only by a later non-checking assembly step.

use crate::{
    CheckedCallContextKind, CheckedCallableKind, CheckedEffectSummary,
    CheckedExternalDeclarationIdentityV1, CheckedIntrinsicV1, CheckedListKeyPolicy,
    CheckedMatchPattern, CheckedParameterKind, CheckedParameterRequirement, CheckedPassedAccess,
    CheckedScopeKind, CheckedStateKind, CheckedValueUse, FlowType, ProgramRole,
    SemanticOccurrenceKind, Type, TypeVar,
};
use boon_data::{Bits, ExactNumber};
use boon_syntax::{StableCheckOwnerKey, StableExpressionKey, StableStatementKey};
use serde::Serialize;

macro_rules! owner_local_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub u32);
    };
}

owner_local_id!(OwnerScopeId);
owner_local_id!(OwnerDeclarationId);
owner_local_id!(OwnerStatementId);
owner_local_id!(OwnerExpressionId);
owner_local_id!(OwnerCallId);
owner_local_id!(OwnerContextFormalId);
owner_local_id!(OwnerSourceId);
owner_local_id!(OwnerStateId);
owner_local_id!(OwnerListId);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerDeclarationStableKey {
    Public,
    Parameter {
        ordinal: u32,
    },
    Statement {
        statement: StableStatementKey,
    },
    PatternBinding {
        selector: StableExpressionKey,
        ordinal: u32,
        name: String,
    },
    FreshOut {
        call: StableExpressionKey,
        formal_ordinal: u32,
    },
    CallContext {
        call: StableExpressionKey,
        ordinal: u32,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerScopeStableKey {
    Root,
    Statement {
        statement: StableStatementKey,
        role: OwnerStatementScopeRole,
    },
    Expression {
        expression: StableExpressionKey,
        role: OwnerExpressionScopeRole,
    },
    GeneratedOut {
        call: StableExpressionKey,
        formal_ordinal: u32,
    },
    CallContext {
        call: StableExpressionKey,
        ordinal: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerStatementScopeRole {
    Body,
    RepeatedOutput { parameter_ordinal: u32 },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerExpressionScopeRole {
    Block,
    Record,
    MatchArm,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerInterfaceMemberRef {
    PublicDeclaration,
    Parameter { ordinal: u32 },
    ContextFormal,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerAbiDeclarationKind {
    BuiltinCallable,
    ExternalCallable,
    ExternalValue,
}

/// Stable identity of one declaration supplied by the frozen project ABI.
///
/// The contract fingerprint binds the active render root, role, parameter and
/// context contract, external identity, and effect without copying that rich
/// contract into every owner row.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerAbiDeclarationKey {
    pub role: ProgramRole,
    pub kind: OwnerAbiDeclarationKind,
    pub contract_fingerprint_v1: [u8; 32],
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerAbiMemberRef {
    Declaration,
    Parameter { ordinal: u32 },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerDeclarationRef {
    Local {
        declaration: OwnerDeclarationId,
    },
    Imported {
        owner: StableCheckOwnerKey,
        member: OwnerInterfaceMemberRef,
    },
    Abi {
        canonical_name: String,
        declaration: OwnerAbiDeclarationKey,
        member: OwnerAbiMemberRef,
    },
    ScopeOwner {
        scope: OwnerScopeRef,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerScopeRef {
    Local {
        scope: OwnerScopeId,
    },
    Imported {
        owner: StableCheckOwnerKey,
        scope: OwnerScopeStableKey,
    },
    ProjectRoot,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerContextFormalRef {
    Local { formal: OwnerContextFormalId },
    Imported { owner: StableCheckOwnerKey },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerExpressionRef {
    Local {
        expression: OwnerExpressionId,
    },
    Child {
        owner: StableCheckOwnerKey,
        expression: StableExpressionKey,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerSourceStableKey {
    pub owner: StableCheckOwnerKey,
    pub statement: StableStatementKey,
    pub expression: StableExpressionKey,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSourceRef {
    Local { source: OwnerSourceId },
    Imported { source: OwnerSourceStableKey },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSourceReadSeed {
    pub source: OwnerSourceRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload_projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerScopeRow {
    pub id: OwnerScopeId,
    pub stable_key: OwnerScopeStableKey,
    pub parent: Option<OwnerScopeRef>,
    pub owner: Option<OwnerDeclarationRef>,
    pub kind: CheckedScopeKind,
    pub source: Option<OwnerSourceSite>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerDeclarationRow {
    pub id: OwnerDeclarationId,
    pub stable_key: OwnerDeclarationStableKey,
    pub scope: OwnerScopeRef,
    pub name: String,
    pub kind: crate::CheckedDeclarationKind,
    pub flow_type: FlowType,
    pub value: Option<OwnerExpressionRef>,
    pub body_scope: Option<OwnerScopeId>,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSourceSite {
    Statement {
        statement: StableStatementKey,
    },
    Expression {
        expression: StableExpressionKey,
    },
    FunctionParameter {
        statement: StableStatementKey,
        ordinal: u32,
    },
    CallArgument {
        expression: StableExpressionKey,
        ordinal: u32,
    },
    CallPass {
        expression: StableExpressionKey,
    },
    PipeArgument {
        expression: StableExpressionKey,
        ordinal: u32,
    },
    PipePass {
        expression: StableExpressionKey,
    },
    RecordField {
        expression: StableExpressionKey,
        ordinal: u32,
    },
    BlockBinding {
        expression: StableExpressionKey,
        ordinal: u32,
    },
    PatternBinding {
        expression: StableExpressionKey,
        ordinal: u32,
    },
    Synthetic {
        label: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerRecordField {
    pub declaration: Option<OwnerDeclarationRef>,
    pub name: String,
    pub value: OwnerExpressionRef,
    pub spread: bool,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerBlockBinding {
    pub declaration: OwnerDeclarationId,
    pub value: OwnerExpressionRef,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerTextSegment {
    Static { value: String },
    Dynamic { value: OwnerExpressionRef },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerExpressionKind {
    Read {
        target: OwnerDeclarationRef,
        projection: Vec<String>,
        source_seed: Option<OwnerSourceReadSeed>,
    },
    Passed {
        formal: OwnerContextFormalRef,
        projection: Vec<String>,
        access: CheckedPassedAccess,
    },
    ExternalRead {
        canonical_path: String,
        declaration: Option<OwnerAbiDeclarationKey>,
    },
    Drain {
        target: OwnerDeclarationRef,
        projection: Vec<String>,
    },
    Text {
        value: String,
    },
    TextTemplate {
        segments: Vec<OwnerTextSegment>,
    },
    Number {
        value: ExactNumber,
    },
    BytesByte {
        value: u8,
    },
    Absent,
    Flush {
        payload: OwnerExpressionRef,
    },
    Tag {
        name: String,
    },
    TaggedObject {
        tag: String,
        fields: Vec<OwnerRecordField>,
    },
    Source,
    Call {
        call: OwnerCallId,
    },
    Draining {
        input: OwnerExpressionRef,
    },
    Hold {
        initial: OwnerExpressionRef,
        name: String,
    },
    Latest {
        branches: Vec<OwnerExpressionRef>,
    },
    When {
        input: OwnerExpressionRef,
        arms: Vec<OwnerExpressionRef>,
    },
    While {
        input: OwnerExpressionRef,
        arms: Vec<OwnerExpressionRef>,
    },
    Then {
        input: OwnerExpressionRef,
        output: Option<OwnerExpressionRef>,
    },
    Infix {
        left: OwnerExpressionRef,
        op: String,
        right: OwnerExpressionRef,
    },
    MatchArm {
        pattern: CheckedMatchPattern,
        bindings: Vec<OwnerDeclarationId>,
        output: Option<OwnerExpressionRef>,
    },
    Block {
        bindings: Vec<OwnerBlockBinding>,
        result: Option<OwnerExpressionRef>,
    },
    Object {
        fields: Vec<OwnerRecordField>,
    },
    List {
        capacity: Option<usize>,
        items: Vec<OwnerExpressionRef>,
    },
    Bytes {
        fixed_size: Option<usize>,
        items: Vec<OwnerExpressionRef>,
    },
    Delimiter,
    Invalid {
        tokens: Vec<String>,
    },
    MapEntry {
        key: OwnerExpressionRef,
        value: OwnerExpressionRef,
    },
    Map {
        entries: Vec<OwnerExpressionRef>,
    },
    Set {
        items: Vec<OwnerExpressionRef>,
    },
    Bits {
        value: Bits,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerExpressionRow {
    pub id: OwnerExpressionId,
    pub stable_key: StableExpressionKey,
    pub scope: OwnerScopeRef,
    pub declaration: Option<OwnerDeclarationRef>,
    pub flow_type: FlowType,
    pub flush_type: Option<Type>,
    pub effect: CheckedEffectSummary,
    pub kind: OwnerExpressionKind,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerStatementKind {
    Function {
        declaration: OwnerDeclarationId,
    },
    Field {
        declaration: OwnerDeclarationId,
    },
    Source {
        declaration: Option<OwnerDeclarationId>,
        event: Option<String>,
    },
    Hold {
        declaration: Option<OwnerDeclarationId>,
        name: Option<String>,
    },
    List {
        declaration: Option<OwnerDeclarationId>,
        capacity: Option<usize>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerStatementChild {
    Local { statement: OwnerStatementId },
    Owner { owner: StableCheckOwnerKey },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerResourceBinding {
    Source { source: OwnerSourceRef },
    State { state: OwnerStateId },
    ListAuthority { list: OwnerListId },
    ListAlias { target: OwnerDeclarationRef },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerStatementRow {
    pub id: OwnerStatementId,
    pub stable_key: StableStatementKey,
    pub scope: OwnerScopeRef,
    pub kind: OwnerStatementKind,
    pub resources: Vec<OwnerResourceBinding>,
    pub value: Option<OwnerExpressionRef>,
    pub value_use: CheckedValueUse,
    pub children: Vec<OwnerStatementChild>,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerEvaluationScope {
    Parent,
    Output { formal: OwnerDeclarationRef },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerParameterRow {
    pub declaration: OwnerDeclarationId,
    pub name: String,
    pub kind: CheckedParameterKind,
    pub ordinal: u32,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: OwnerEvaluationScope,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableContextRow {
    pub name: String,
    pub kind: CheckedCallContextKind,
    pub provider: OwnerDeclarationRef,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerContextFormalRow {
    pub id: OwnerContextFormalId,
    pub callable: OwnerDeclarationId,
    pub flow_type: FlowType,
    pub projections: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableRow {
    pub declaration: OwnerDeclarationId,
    pub scope: OwnerScopeRef,
    pub kind: CheckedCallableKind,
    pub name: String,
    pub intrinsic: Option<CheckedIntrinsicV1>,
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub parameters: Vec<OwnerParameterRow>,
    pub contexts: Vec<OwnerCallableContextRow>,
    pub context_formal: Option<OwnerContextFormalId>,
    pub result: FlowType,
    pub role: ProgramRole,
    pub effect: CheckedEffectSummary,
    pub body: Option<OwnerStatementId>,
    pub result_expression: Option<OwnerExpressionRef>,
    pub contextual_operation: Option<OwnerContextualOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerContextualOperation {
    Map {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        body: OwnerDeclarationRef,
    },
    Filter {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        predicate: OwnerDeclarationRef,
    },
    Retain {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        predicate: OwnerDeclarationRef,
    },
    Remove {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        predicate: OwnerDeclarationRef,
    },
    Every {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        predicate: OwnerDeclarationRef,
    },
    Any {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        predicate: OwnerDeclarationRef,
    },
    Find {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        predicate: OwnerDeclarationRef,
    },
    SortBy {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        key: OwnerDeclarationRef,
        direction: OwnerDeclarationRef,
    },
    ThenBy {
        list: OwnerDeclarationRef,
        row: OwnerDeclarationRef,
        key: OwnerDeclarationRef,
        direction: OwnerDeclarationRef,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerCallEntry {
    Input {
        formal: OwnerDeclarationRef,
        name: String,
        value: OwnerExpressionRef,
        from_pipe: bool,
        evaluation_scope: OwnerEvaluationScope,
    },
    FreshOut {
        formal: OwnerDeclarationRef,
        name: String,
        output: OwnerDeclarationId,
        scope_id: OwnerScopeId,
    },
    ForwardOut {
        formal: OwnerDeclarationRef,
        name: String,
        target: OwnerDeclarationRef,
        target_name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerContextBinding {
    Explicit {
        value: OwnerExpressionRef,
        source: OwnerSourceSite,
    },
    Inherited {
        formal: OwnerContextFormalRef,
    },
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerContextTypeSubstitution {
    pub formal: OwnerContextFormalRef,
    pub variable: TypeVar,
    pub value: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerTypeSubstitution {
    pub variable: TypeVar,
    pub value: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallRow {
    pub id: OwnerCallId,
    pub stable_key: StableExpressionKey,
    pub expression: OwnerExpressionId,
    pub callable: OwnerDeclarationRef,
    pub owner_callable: Option<OwnerDeclarationRef>,
    pub function: String,
    pub intrinsic: Option<CheckedIntrinsicV1>,
    pub entries: Vec<OwnerCallEntry>,
    pub contexts: Vec<OwnerCallContextRow>,
    pub context_binding: OwnerContextBinding,
    pub contextual_substitutions: Vec<OwnerContextTypeSubstitution>,
    pub type_substitutions: Vec<OwnerTypeSubstitution>,
    pub syntax_discriminated_result: bool,
    pub result: FlowType,
    pub role: ProgramRole,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallContextRow {
    pub declaration: OwnerDeclarationId,
    pub context_ordinal: u32,
    pub scope_id: OwnerScopeId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallResultPathRow {
    pub call: OwnerCallId,
    pub anchor: OwnerDeclarationRef,
    pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerPatternBindingRow {
    pub declaration: OwnerDeclarationId,
    pub selector: OwnerExpressionId,
    pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerResourceProjectionSeedRow {
    pub expression: OwnerExpressionId,
    pub target: OwnerDeclarationRef,
    pub projection: Vec<String>,
    pub required_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSemanticPath {
    pub anchor: OwnerDeclarationRef,
    pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSourceRow {
    pub id: OwnerSourceId,
    pub stable_key: OwnerSourceStableKey,
    pub declaration: OwnerDeclarationRef,
    pub statement: OwnerStatementId,
    pub expression: OwnerExpressionId,
    pub owner_scope: OwnerScopeRef,
    pub path: OwnerSemanticPath,
    pub interval_ms: Option<u64>,
    pub payload_type: Type,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerStateRow {
    pub id: OwnerStateId,
    pub declaration: OwnerDeclarationRef,
    pub statement: OwnerStatementId,
    pub expression: OwnerExpressionId,
    pub initial: OwnerExpressionRef,
    pub owner_scope: OwnerScopeRef,
    pub path: OwnerSemanticPath,
    pub kind: CheckedStateKind,
    pub flow_type: FlowType,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerListRow {
    pub id: OwnerListId,
    pub declaration: OwnerDeclarationRef,
    pub statement: OwnerStatementId,
    pub producer: OwnerExpressionId,
    pub owner_scope: OwnerScopeRef,
    pub path: OwnerSemanticPath,
    pub item_type: Type,
    pub capacity: Option<usize>,
    pub key_policy: CheckedListKeyPolicy,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerOccurrenceRow {
    pub target: OwnerDeclarationRef,
    pub kind: SemanticOccurrenceKind,
    pub source: OwnerSourceSite,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerCheckedRowDomain {
    Scope,
    Declaration,
    Statement,
    Expression,
    Callable,
    ContextFormal,
    Call,
    CallResultPath,
    PatternBinding,
    ResourceProjection,
    Source,
    State,
    List,
    Occurrence,
    Diagnostic,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerRelocationTarget {
    Declaration {
        owner: StableCheckOwnerKey,
        member: OwnerInterfaceMemberRef,
    },
    Scope {
        owner: StableCheckOwnerKey,
        scope: OwnerScopeStableKey,
    },
    ContextFormal {
        owner: StableCheckOwnerKey,
    },
    ChildExpression {
        owner: StableCheckOwnerKey,
        expression: StableExpressionKey,
    },
    Source {
        source: OwnerSourceStableKey,
    },
    AbiDeclaration {
        canonical_name: String,
        declaration: OwnerAbiDeclarationKey,
        member: OwnerAbiMemberRef,
    },
    ProjectRootScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCheckedRelocation {
    pub source_domain: OwnerCheckedRowDomain,
    pub source_row: u32,
    pub target: OwnerRelocationTarget,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OwnerRelocationSpan {
    pub start: u32,
    pub len: u32,
}

impl OwnerRelocationSpan {
    pub fn checked_range(self) -> Option<std::ops::Range<usize>> {
        let end = self.start.checked_add(self.len)?;
        Some(self.start as usize..end as usize)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCheckedRowReceipt {
    pub domain: OwnerCheckedRowDomain,
    pub row: u32,
    pub stable_key_digest_v1: [u8; 32],
    pub payload_digest_v1: [u8; 32],
    pub relocations: OwnerRelocationSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCheckedDomainCount {
    pub domain: OwnerCheckedRowDomain,
    pub rows: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCheckedConstructionReceipt {
    pub domain_counts: Box<[OwnerCheckedDomainCount]>,
    pub row_receipt_count: u32,
    pub relocation_count: u32,
    pub local_content_digest_v1: [u8; 32],
}

/// Canonical proof material closed by the owner-local checked-row builder.
///
/// Keeping the detailed receipts beside the compact construction receipt lets
/// the later non-checking linker consume exact CSR spans without reopening or
/// serializing the rich checked rows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCheckedReceiptSet {
    pub construction: OwnerCheckedConstructionReceipt,
    pub row_receipts: Box<[OwnerCheckedRowReceipt]>,
    pub relocations: Box<[OwnerCheckedRelocation]>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CheckedOwnerRows {
    pub scopes: Vec<OwnerScopeRow>,
    pub declarations: Vec<OwnerDeclarationRow>,
    pub statements: Vec<OwnerStatementRow>,
    pub expressions: Vec<OwnerExpressionRow>,
    pub callables: Vec<OwnerCallableRow>,
    pub context_formals: Vec<OwnerContextFormalRow>,
    pub calls: Vec<OwnerCallRow>,
    pub call_result_paths: Vec<OwnerCallResultPathRow>,
    pub pattern_bindings: Vec<OwnerPatternBindingRow>,
    pub resource_projection_seeds: Vec<OwnerResourceProjectionSeedRow>,
    pub sources: Vec<OwnerSourceRow>,
    pub states: Vec<OwnerStateRow>,
    pub lists: Vec<OwnerListRow>,
    pub occurrences: Vec<OwnerOccurrenceRow>,
    pub relocations: Vec<OwnerCheckedRelocation>,
    pub receipts: Vec<OwnerCheckedRowReceipt>,
}
