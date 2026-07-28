use boon_contract::{CanonicalSourceBundleV1, SourceBundleDigestV1, SourceBundleUnit};
use chumsky::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StandardRootKind {
    ProgramRole,
    Runtime,
    Library,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramRoleRoot {
    Client,
    Session,
    Server,
}

impl ProgramRoleRoot {
    pub const ALL: [Self; 3] = [Self::Client, Self::Session, Self::Server];

    pub const fn namespace(self) -> &'static str {
        match self {
            Self::Client => "Client",
            Self::Session => "Session",
            Self::Server => "Server",
        }
    }

    const fn standard_root(self) -> StandardRoot {
        StandardRoot {
            name: self.namespace(),
            kind: StandardRootKind::ProgramRole,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StandardRoot {
    pub name: &'static str,
    pub kind: StandardRootKind,
}

/// The single source of truth for names owned by the language and standard
/// library. Application modules and root declarations may not shadow them.
pub const STANDARD_ROOTS: &[StandardRoot] = &[
    StandardRoot {
        name: "Bool",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Browser",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Bytes",
        kind: StandardRootKind::Library,
    },
    ProgramRoleRoot::Client.standard_root(),
    StandardRoot {
        name: "Clock",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Content",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Crypto",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "DevelopmentPasskey",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Directory",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Document",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Element",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Field",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "File",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Http",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Light",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "List",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Log",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Number",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Passkey",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Random",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Router",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Scene",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Secret",
        kind: StandardRootKind::Library,
    },
    ProgramRoleRoot::Server.standard_root(),
    ProgramRoleRoot::Session.standard_root(),
    StandardRoot {
        name: "SessionInfo",
        kind: StandardRootKind::Runtime,
    },
    StandardRoot {
        name: "Text",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Timer",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Ulid",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Url",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Wellen",
        kind: StandardRootKind::Library,
    },
    StandardRoot {
        name: "Widget",
        kind: StandardRootKind::Library,
    },
];

pub fn standard_root_kind(name: &str) -> Option<StandardRootKind> {
    STANDARD_ROOTS
        .iter()
        .find_map(|root| (root.name == name).then_some(root.kind))
}

pub fn program_role_root(name: &str) -> Option<ProgramRoleRoot> {
    ProgramRoleRoot::ALL
        .into_iter()
        .find(|role| role.namespace() == name)
}

pub fn is_program_role_root(name: &str) -> bool {
    program_role_root(name).is_some()
}

pub fn is_reserved_standard_root(name: &str) -> bool {
    standard_root_kind(name).is_some()
}

pub fn canonical_value_path(parts: &[String]) -> String {
    match parts.split_first() {
        Some((root, suffix)) if is_program_role_root(root) && !suffix.is_empty() => {
            format!("{root}/{}", suffix.join("."))
        }
        _ => parts.join("."),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProgramKind {
    Generic,
}

impl ProgramKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
        }
    }
}

/// Delivery status for a public Boon language-surface feature.
///
/// This is deliberately independent from parser acceptance: some planned
/// semantics reuse syntax that the current parser already accepts, while
/// planned syntax with no implementation must continue to fail closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageFeatureStage {
    Current,
    Planned,
}

impl LanguageFeatureStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Planned => "planned",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageFeatureParseExpectation {
    Accept,
    Reject,
}

impl LanguageFeatureParseExpectation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageFeatureSpec {
    pub id: &'static str,
    pub stage: LanguageFeatureStage,
    pub parse_expectation: LanguageFeatureParseExpectation,
    pub spellings: &'static [&'static str],
    pub summary: &'static str,
}

/// Canonical parser-owned registry for the public language-surface coverage
/// contract. Entries are sorted by `id`.
///
/// `Planned + Accept` means only that the current grammar accepts the fixture;
/// it does not claim the planned semantics exist. `Planned + Reject` reserves a
/// fail-closed spelling until the implementing phase lands atomically.
pub const LANGUAGE_FEATURE_REGISTRY: &[LanguageFeatureSpec] = &[
    LanguageFeatureSpec {
        id: "bits_fixed_width_literals",
        stage: LanguageFeatureStage::Planned,
        parse_expectation: LanguageFeatureParseExpectation::Reject,
        spellings: &["BITS"],
        summary: "fixed-width BITS[N] literals are planned and rejected today",
    },
    LanguageFeatureSpec {
        id: "closed_truth_tags",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["True", "False"],
        summary: "True and False are ordinary members of a closed Tag set",
    },
    LanguageFeatureSpec {
        id: "distributed_role_paths",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["Client/", "Session/", "Server/"],
        summary: "slash-qualified Client, Session, and Server values and calls",
    },
    LanguageFeatureSpec {
        id: "exact_number_value_algebra",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["integer literal", "decimal literal"],
        summary: "integer, decimal, exponent, and fraction results use canonical exact rationals",
    },
    LanguageFeatureSpec {
        id: "flush_control",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["FLUSH"],
        summary: "typed fail-fast FLUSH control with lexical boundary unwrapping",
    },
    LanguageFeatureSpec {
        id: "immutable_bytes",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["BYTES", "Bytes/"],
        summary: "immutable dynamic, inferred, and fixed-size byte values",
    },
    LanguageFeatureSpec {
        id: "map_literals",
        stage: LanguageFeatureStage::Planned,
        parse_expectation: LanguageFeatureParseExpectation::Reject,
        spellings: &["MAP"],
        summary: "authoritative MAP literals are planned and rejected today",
    },
    LanguageFeatureSpec {
        id: "one_based_positions",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["position:", "from:", "count:"],
        summary: "LIST, TEXT, and BYTES positions are one-based; BITS joins the same contract when its value phase lands",
    },
    LanguageFeatureSpec {
        id: "reactive_temporal_operators",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["SOURCE", "HOLD", "LATEST", "THEN", "WHEN", "WHILE"],
        summary: "current reactive sources, state, merge, event, and selection operators",
    },
    LanguageFeatureSpec {
        id: "record_and_list_literals",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["[field: value]", "LIST"],
        summary: "structural record and ordered LIST literals",
    },
    LanguageFeatureSpec {
        id: "set_literals",
        stage: LanguageFeatureStage::Planned,
        parse_expectation: LanguageFeatureParseExpectation::Reject,
        spellings: &["SET"],
        summary: "authoritative SET literals are planned and rejected today",
    },
    LanguageFeatureSpec {
        id: "structured_out_and_pass",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["OUT", "PASS", "PASSED"],
        summary: "structured output bindings and a separate final PASS context",
    },
    LanguageFeatureSpec {
        id: "tags_presence_and_fault_algebra",
        stage: LanguageFeatureStage::Planned,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["Tag", "Tag[field: value]"],
        summary: "tag syntax parses today, while the unified presence and fault algebra is planned",
    },
    LanguageFeatureSpec {
        id: "typed_list_pipelines",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["List/map", "List/filter", "List/sort", "List/page"],
        summary: "typed LIST pipelines with structured row bindings",
    },
    LanguageFeatureSpec {
        id: "where_contracts",
        stage: LanguageFeatureStage::Planned,
        parse_expectation: LanguageFeatureParseExpectation::Reject,
        spellings: &["WHERE"],
        summary: "authored WHERE proof obligations are planned and rejected today",
    },
];

pub fn language_feature(id: &str) -> Option<&'static LanguageFeatureSpec> {
    LANGUAGE_FEATURE_REGISTRY
        .binary_search_by_key(&id, |feature| feature.id)
        .ok()
        .map(|index| &LANGUAGE_FEATURE_REGISTRY[index])
}

/// Opaque parser-produced syntax artifact.
///
/// Its fields remain readable through [`ParsedProgramFields`], but callers
/// cannot mutate them through this wrapper or construct an accepted
/// `ParsedProgram` from deserialized fields.
///
/// ```compile_fail
/// use boon_parser::{ParsedProgram, ParsedProgramFields};
///
/// let fields: ParsedProgramFields = serde_json::from_str("{}").unwrap();
/// let _ = ParsedProgram { fields };
/// ```
///
/// ```compile_fail
/// use boon_parser::ParsedProgram;
///
/// let _: ParsedProgram = serde_json::from_str("{}").unwrap();
/// ```
///
/// ```compile_fail
/// use boon_parser::ParsedProgram;
///
/// let _: ParsedProgram = Default::default();
/// ```
///
/// ```compile_fail
/// let mut parsed = boon_parser::parse_source("main.bn", "value: 1").unwrap();
/// parsed.path.clear();
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ParsedProgram {
    #[serde(flatten)]
    fields: ParsedProgramFields,
}

/// Public read-only schema projected by an opaque [`ParsedProgram`].
///
/// Deserializing or constructing this DTO never creates a parser-produced
/// artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedProgramFields {
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub path: String,
    pub source: String,
    pub files: Vec<ParsedSourceFile>,
    pub kind: ProgramKind,
    pub ast: AstProgram,
    pub expressions: Vec<AstExpr>,
    pub functions: Vec<String>,
    pub operators: Vec<String>,
}

impl std::ops::Deref for ParsedProgram {
    type Target = ParsedProgramFields;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl ParsedProgram {
    fn from_parser_fields(fields: ParsedProgramFields) -> Self {
        Self { fields }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedSourceFile {
    pub path: String,
    pub source: String,
    pub start_line: usize,
    pub module: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstProgram {
    pub tokens: Vec<AstToken>,
    pub lines: Vec<ParserLine>,
    pub items: Vec<ParserItem>,
    pub statements: Vec<AstStatement>,
    pub expressions: Vec<AstExpr>,
}

impl AstProgram {
    pub fn semantic_tokens(&self) -> impl Iterator<Item = &AstToken> {
        self.tokens.iter().filter(|token| {
            !matches!(token.kind, AstTokenKind::Comment | AstTokenKind::String)
                && !self.line_is_document(token.line)
        })
    }

    pub fn semantic_parser_lines(&self) -> impl Iterator<Item = &ParserLine> {
        self.lines
            .iter()
            .filter(|line| !line.symbols.is_empty() && !self.line_is_document(line.line))
    }

    pub fn semantic_parser_items(&self) -> impl Iterator<Item = &ParserItem> {
        self.items
            .iter()
            .filter(|item| !self.line_is_document(item.line))
    }

    fn line_is_document(&self, line: usize) -> bool {
        self.statements
            .iter()
            .find(|statement| {
                matches!(
                    &statement.kind,
                    AstStatementKind::Field { name } if name == "document"
                )
            })
            .is_some_and(|statement| statement_contains_line(statement, line))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParserLine {
    pub line: usize,
    pub indent: usize,
    pub symbols: Vec<String>,
    pub symbol_spans: Vec<(usize, usize)>,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParserItem {
    pub line: usize,
    pub indent: usize,
    pub start: usize,
    pub end: usize,
    pub symbols: Vec<String>,
    pub symbol_spans: Vec<(usize, usize)>,
    pub field: Option<String>,
    pub example: Option<String>,
    pub function: Option<String>,
    pub source_event: Option<String>,
    pub hold: Option<String>,
    pub list_capacity: Option<usize>,
    pub is_list: bool,
    pub opens_scope: bool,
    pub closes_scope: bool,
    pub operators: Vec<String>,
}

impl ParserItem {
    pub fn has_lexeme(&self, lexeme: &str) -> bool {
        self.symbols.iter().any(|candidate| candidate == lexeme)
    }

    pub fn contains_sequence(&self, sequence: &[&str]) -> bool {
        if sequence.is_empty() {
            return true;
        }
        self.symbols.windows(sequence.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(sequence.iter().copied())
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstStatement {
    pub id: usize,
    pub line: usize,
    pub indent: usize,
    pub start: usize,
    pub end: usize,
    pub kind: AstStatementKind,
    pub expr: Option<usize>,
    pub children: Vec<AstStatement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstStatementKind {
    Function {
        name: String,
        parameters: Vec<AstParameter>,
    },
    Field {
        name: String,
    },
    Source {
        field: Option<String>,
        event: Option<String>,
    },
    Hold {
        field: Option<String>,
        name: Option<String>,
    },
    List {
        field: Option<String>,
        capacity: Option<usize>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstExpr {
    pub id: usize,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_input: Option<usize>,
    pub kind: AstExprKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AstTextSegment {
    Static { value: String },
    Dynamic { value: usize },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AstMatchPattern {
    Wildcard,
    Number {
        value: String,
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
    Invalid {
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstExprKind {
    Identifier(String),
    Path(Vec<String>),
    Drain {
        path: AstDrainPath,
    },
    StringLiteral(String),
    TextLiteral(String),
    TextTemplate {
        segments: Vec<AstTextSegment>,
    },
    Number(String),
    ByteLiteral {
        radix: u8,
        digits: String,
        value: u8,
    },
    Tag(String),
    TaggedObject {
        tag: String,
        fields: Vec<AstRecordField>,
    },
    /// Private fail-fast control. The payload is ordinary Boon data, but the
    /// `FLUSH` carrier itself never enters the public value algebra.
    Flush {
        payload: Option<usize>,
    },
    Source,
    Call {
        function: String,
        args: Vec<AstCallArg>,
        pass: Option<AstPassContext>,
    },
    Pipe {
        input: usize,
        op: String,
        args: Vec<AstCallArg>,
        pass: Option<AstPassContext>,
        #[serde(default)]
        arms: Vec<usize>,
    },
    Draining {
        input: usize,
    },
    Hold {
        initial: usize,
        name: String,
    },
    Latest {
        #[serde(default)]
        branches: Vec<usize>,
    },
    When {
        input: usize,
        #[serde(default)]
        arms: Vec<usize>,
    },
    Then {
        input: usize,
        output: Option<usize>,
    },
    Infix {
        left: usize,
        op: String,
        right: usize,
    },
    MatchArm {
        pattern: AstMatchPattern,
        output: Option<usize>,
    },
    Block {
        #[serde(default)]
        bindings: Vec<AstBlockBinding>,
        result: Option<usize>,
    },
    Object(Vec<AstRecordField>),
    ListLiteral {
        capacity: Option<usize>,
        #[serde(default)]
        items: Vec<usize>,
    },
    BytesLiteral {
        size: BytesSizeSyntax,
        #[serde(default)]
        items: Vec<usize>,
    },
    Delimiter,
    Unknown(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstDrainPath {
    Binding {
        name: String,
    },
    Field {
        binding: String,
        fields: Vec<String>,
    },
    Passed {
        fields: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BytesSizeSyntax {
    Dynamic,
    Infer,
    Fixed(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstParameterKind {
    Value,
    Out,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstParameter {
    pub name: String,
    pub kind: AstParameterKind,
    pub ordinal: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstCallArgKind {
    BareBinding,
    Named,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstCallArg {
    pub kind: AstCallArgKind,
    pub name: String,
    pub value: usize,
    pub start: usize,
    pub end: usize,
}

impl AstCallArg {
    pub fn named_name(&self) -> Option<&str> {
        (self.kind == AstCallArgKind::Named).then_some(self.name.as_str())
    }

    pub fn is_bare_binding(&self) -> bool {
        self.kind == AstCallArgKind::BareBinding
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstPassContext {
    pub value: usize,
    pub start: usize,
    pub end: usize,
    pub final_clause: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstBlockBinding {
    pub name: String,
    pub statement: usize,
    pub value: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstRecordField {
    pub name: String,
    pub value: usize,
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub spread: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstToken {
    pub kind: AstTokenKind,
    pub lexeme: String,
    pub line: usize,
    pub column: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstTokenKind {
    Identifier,
    Number,
    String,
    Comment,
    Operator,
    Symbol,
    Newline,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentAst {
    pub root: AstStatement,
    pub expressions: Vec<AstExpr>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse_source(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<ParsedProgram, ParseError> {
    let path = path.into();
    let source = source.into();
    let bundle = CanonicalSourceBundleV1::new(
        &path,
        [SourceBundleUnit::new(path.as_str(), source.as_str())],
    )
    .map_err(|error| source_bundle_parse_error(&path, error))?;
    parse_canonical_source_bundle(bundle)
}

/// Parses a project whose first argument is the logical project-relative path
/// of the entrypoint unit.
pub fn parse_project(
    entrypoint: impl Into<String>,
    files: impl IntoIterator<Item = (String, String)>,
) -> Result<ParsedProgram, ParseError> {
    let entrypoint = entrypoint.into();
    let files = files.into_iter().collect::<Vec<_>>();
    let bundle = CanonicalSourceBundleV1::new(
        &entrypoint,
        files
            .iter()
            .map(|(path, source)| SourceBundleUnit::new(path.as_str(), source.as_str())),
    )
    .map_err(|error| source_bundle_parse_error(&entrypoint, error))?;
    parse_canonical_source_bundle(bundle)
}

fn source_bundle_parse_error(
    entrypoint: &str,
    error: boon_contract::SourceBundleError,
) -> ParseError {
    ParseError {
        path: entrypoint.to_owned(),
        line: None,
        column: None,
        message: format!("invalid source bundle: {error}"),
    }
}

fn parse_canonical_source_bundle(
    bundle: CanonicalSourceBundleV1<'_>,
) -> Result<ParsedProgram, ParseError> {
    let entrypoint = bundle.entrypoint().to_owned();
    let source_bundle_digest_v1 = bundle.digest();
    let mut parsed_files = Vec::new();
    let mut source = String::new();
    let mut next_line = 1usize;
    for (index, unit) in bundle.units().iter().enumerate() {
        let file_path = unit.path();
        let file_source = unit.source();
        reject_reserved_module_path(file_path)?;
        let start_line = next_line;
        source.push_str(file_source);
        if index + 1 < bundle.units().len() && !file_source.ends_with('\n') {
            source.push('\n');
        }
        next_line += file_source.lines().count().max(1);
        parsed_files.push(ParsedSourceFile {
            module: module_name_for_project_file(&entrypoint, file_path),
            path: file_path.to_owned(),
            source: file_source.to_owned(),
            start_line,
        });
    }
    parse_combined_source(
        entrypoint.clone(),
        source,
        parsed_files.clone(),
        source_bundle_digest_v1,
    )
    .map_err(|error| project_source_error(error, &entrypoint, &parsed_files))
}

fn project_source_error(
    mut error: ParseError,
    project_path: &str,
    files: &[ParsedSourceFile],
) -> ParseError {
    if error.path != project_path {
        return error;
    }
    let Some(global_line) = error.line else {
        return error;
    };
    let Some(file) = files
        .iter()
        .filter(|file| file.start_line <= global_line)
        .max_by_key(|file| file.start_line)
    else {
        return error;
    };
    error.path.clone_from(&file.path);
    error.line = Some(
        global_line
            .saturating_sub(file.start_line)
            .saturating_add(1),
    );
    error
}

fn parse_combined_source(
    path: String,
    source: String,
    files: Vec<ParsedSourceFile>,
    source_bundle_digest_v1: SourceBundleDigestV1,
) -> Result<ParsedProgram, ParseError> {
    let mut ast = parse_ast(&path, &source)?;
    namespace_project_modules(&mut ast, &files);
    validate_source_syntax(&path, &ast)?;
    validate_balanced_brackets(&path, &ast)?;
    validate_list_capacities(&path, &ast)?;
    validate_no_reducer_style_update(&path, &ast)?;
    let kind = detect_program_kind();
    validate_no_hidden_identity_leak(&path, &ast)?;
    Ok(ParsedProgram::from_parser_fields(ParsedProgramFields {
        source_bundle_digest_v1,
        expressions: ast.expressions.clone(),
        functions: collect_functions(&ast),
        operators: collect_operators(&ast),
        path,
        source,
        files,
        kind,
        ast,
    }))
}

fn module_name_for_project_file(entry_path: &str, file_path: &str) -> Option<String> {
    if entry_path == file_path {
        return None;
    }
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|stem| stem.to_str())?;
    if stem.chars().next().is_some_and(char::is_uppercase) {
        Some(stem.to_owned())
    } else {
        None
    }
}

fn reject_reserved_module_path(path: &str) -> Result<(), ParseError> {
    let module = std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str());
    if module.is_some_and(is_reserved_standard_root) {
        return Err(ParseError {
            path: path.to_owned(),
            line: None,
            column: None,
            message: format!(
                "`{}` is a reserved Boon standard namespace and cannot be declared by an application",
                module.unwrap_or_default()
            ),
        });
    }
    Ok(())
}

fn namespace_project_modules(ast: &mut AstProgram, files: &[ParsedSourceFile]) {
    let ranges = files
        .iter()
        .filter_map(|file| {
            let module = file.module.as_ref()?;
            let line_count = file.source.lines().count().max(1);
            Some((
                file.start_line..file.start_line + line_count,
                module.clone(),
            ))
        })
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return;
    }
    let mut functions_by_module = std::collections::BTreeMap::<String, Vec<String>>::new();
    collect_module_functions(&ast.statements, &ranges, &mut functions_by_module);
    namespace_statement_functions(&mut ast.statements, &ranges, &functions_by_module);
    namespace_expr_functions(&mut ast.expressions, &ranges, &functions_by_module);
    namespace_parser_items(&mut ast.items, &ranges, &functions_by_module);
}

fn module_for_line(line: usize, ranges: &[(std::ops::Range<usize>, String)]) -> Option<&str> {
    ranges
        .iter()
        .find(|(range, _)| range.contains(&line))
        .map(|(_, module)| module.as_str())
}

fn collect_module_functions(
    statements: &[AstStatement],
    ranges: &[(std::ops::Range<usize>, String)],
    functions_by_module: &mut std::collections::BTreeMap<String, Vec<String>>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &statement.kind
            && let Some(module) = module_for_line(statement.line, ranges)
        {
            functions_by_module
                .entry(module.to_owned())
                .or_default()
                .push(name.clone());
        }
        collect_module_functions(&statement.children, ranges, functions_by_module);
    }
}

fn module_function_name(
    module: &str,
    function: &str,
    functions_by_module: &std::collections::BTreeMap<String, Vec<String>>,
) -> Option<String> {
    if function.contains('/') {
        return None;
    }
    functions_by_module
        .get(module)
        .is_some_and(|functions| functions.iter().any(|name| name == function))
        .then(|| format!("{module}/{function}"))
}

fn namespace_statement_functions(
    statements: &mut [AstStatement],
    ranges: &[(std::ops::Range<usize>, String)],
    functions_by_module: &std::collections::BTreeMap<String, Vec<String>>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &mut statement.kind
            && let Some(module) = module_for_line(statement.line, ranges)
            && !name.contains('/')
        {
            *name = format!("{module}/{name}");
        }
        namespace_statement_functions(&mut statement.children, ranges, functions_by_module);
        let _ = functions_by_module;
    }
}

fn namespace_expr_functions(
    expressions: &mut [AstExpr],
    ranges: &[(std::ops::Range<usize>, String)],
    functions_by_module: &std::collections::BTreeMap<String, Vec<String>>,
) {
    for expr in expressions {
        let Some(module) = module_for_line(expr.line, ranges) else {
            continue;
        };
        match &mut expr.kind {
            AstExprKind::Call { function, .. } => {
                if let Some(namespaced) =
                    module_function_name(module, function, functions_by_module)
                {
                    *function = namespaced;
                }
            }
            AstExprKind::Pipe { op, .. } => {
                if let Some(namespaced) = module_function_name(module, op, functions_by_module) {
                    *op = namespaced;
                }
            }
            _ => {}
        }
    }
}

fn namespace_parser_items(
    items: &mut [ParserItem],
    ranges: &[(std::ops::Range<usize>, String)],
    functions_by_module: &std::collections::BTreeMap<String, Vec<String>>,
) {
    for item in items {
        let Some(module) = module_for_line(item.line, ranges) else {
            continue;
        };
        if let Some(function) = &mut item.function
            && !function.contains('/')
        {
            *function = format!("{module}/{function}");
        }
        for operator in &mut item.operators {
            if let Some(namespaced) = module_function_name(module, operator, functions_by_module) {
                *operator = namespaced;
            }
        }
    }
}

pub fn parsed_document(program: &ParsedProgram) -> Option<DocumentAst> {
    document_statement(&program.ast)
        .cloned()
        .map(|root| DocumentAst {
            root,
            expressions: program.ast.expressions.clone(),
        })
}

pub fn parsed_scene(program: &ParsedProgram) -> Option<DocumentAst> {
    scene_statement(&program.ast)
        .cloned()
        .map(|root| DocumentAst {
            root,
            expressions: program.ast.expressions.clone(),
        })
}

pub fn format_source(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<String, ParseError> {
    let path = path.into();
    let source = source.into();
    parse_source(path, source.clone())?;
    Ok(format_source_text(&source))
}

pub fn format_source_unit(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<String, ParseError> {
    let path = path.into();
    let source = source.into();
    let ast = parse_ast(&path, &source)?;
    validate_source_syntax(&path, &ast)?;
    validate_balanced_brackets(&path, &ast)?;
    validate_list_capacities(&path, &ast)?;
    Ok(format_source_text(&source))
}

fn format_source_text(source: &str) -> String {
    let mut formatted_lines = Vec::new();
    let mut previous_blank = false;
    for line in source.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.is_empty() {
            if !previous_blank {
                formatted_lines.push(String::new());
            }
            previous_blank = true;
            continue;
        }
        previous_blank = false;
        let content = trimmed_end.trim_start_matches([' ', '\t']);
        let raw_indent_columns = trimmed_end
            .chars()
            .take_while(|character| *character == ' ' || *character == '\t')
            .map(|character| if character == '\t' { 4 } else { 1 })
            .sum::<usize>();
        let indent_columns = if raw_indent_columns > 0 {
            // Parser-gated indentation normalization: every non-empty source line
            // keeps its block depth, but mixed/two-space indentation is rewritten to
            // the canonical four-column editor grid after the parser has accepted
            // the source.
            raw_indent_columns.div_ceil(4) * 4
        } else {
            raw_indent_columns
        };
        formatted_lines.push(format!("{}{}", " ".repeat(indent_columns), content));
    }
    formatted_lines = compact_format_bracket_blocks(formatted_lines);
    while formatted_lines.last().is_some_and(|line| line.is_empty()) {
        formatted_lines.pop();
    }
    let mut formatted = formatted_lines.join("\n");
    formatted.push('\n');
    formatted
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FormatNode {
    Blank,
    Line {
        indent: usize,
        content: String,
    },
    BracketBlock {
        indent: usize,
        prefix: String,
        children: Vec<FormatNode>,
    },
}

fn compact_format_bracket_blocks(lines: Vec<String>) -> Vec<String> {
    let mut index = 0;
    let nodes = parse_format_nodes(&lines, &mut index, None);
    let mut formatted = Vec::new();
    render_format_nodes(&nodes, &mut formatted);
    formatted
}

fn parse_format_nodes(
    lines: &[String],
    index: &mut usize,
    close_indent: Option<usize>,
) -> Vec<FormatNode> {
    let mut nodes = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.is_empty() {
            nodes.push(FormatNode::Blank);
            *index += 1;
            continue;
        }
        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        let content = line[indent..].to_owned();
        if close_indent == Some(indent) && content == "]" {
            *index += 1;
            break;
        }
        if let Some(prefix) = format_bracket_block_prefix(&content) {
            *index += 1;
            let children = parse_format_nodes(lines, index, Some(indent));
            nodes.push(FormatNode::BracketBlock {
                indent,
                prefix,
                children,
            });
        } else {
            nodes.push(FormatNode::Line { indent, content });
            *index += 1;
        }
    }
    nodes
}

fn format_bracket_block_prefix(content: &str) -> Option<String> {
    let prefix = content.strip_suffix('[')?.trim_end();
    if prefix.contains("--") {
        return None;
    }
    Some(prefix.to_owned())
}

fn render_format_nodes(nodes: &[FormatNode], output: &mut Vec<String>) {
    let nonblank = nodes
        .iter()
        .filter(|node| !matches!(node, FormatNode::Blank))
        .collect::<Vec<_>>();
    let object_of_objects = nonblank.len() > 1
        && nonblank
            .iter()
            .all(|node| matches!(node, FormatNode::BracketBlock { .. }));
    let mut previous_multiline = false;
    let mut rendered_any = false;
    for node in nodes {
        if matches!(node, FormatNode::Blank) {
            if !object_of_objects {
                push_format_blank(output);
            }
            continue;
        }
        let multiline = format_node_inline_text(node).is_none();
        if object_of_objects && rendered_any && (previous_multiline || multiline) {
            push_format_blank(output);
        }
        render_format_node(node, output);
        previous_multiline = multiline;
        rendered_any = true;
    }
}

fn render_format_node(node: &FormatNode, output: &mut Vec<String>) {
    match node {
        FormatNode::Blank => push_format_blank(output),
        FormatNode::Line { indent, content } => {
            output.push(format!("{}{}", " ".repeat(*indent), content));
        }
        FormatNode::BracketBlock {
            indent,
            prefix,
            children,
        } => {
            if let Some(inline) = format_node_inline_text(node) {
                output.push(format!("{}{}", " ".repeat(*indent), inline));
                return;
            }
            output.push(format!("{}{}", " ".repeat(*indent), bracket_open(prefix)));
            render_format_nodes(children, output);
            output.push(format!("{}]", " ".repeat(*indent)));
        }
    }
}

fn push_format_blank(output: &mut Vec<String>) {
    if output.last().is_some_and(|line| line.is_empty()) {
        return;
    }
    output.push(String::new());
}

fn format_node_inline_text(node: &FormatNode) -> Option<String> {
    const MAX_INLINE_CHARS: usize = 96;
    let text = format_node_inline_text_unbounded(node)?;
    (text.chars().count() <= MAX_INLINE_CHARS).then_some(text)
}

fn format_node_inline_text_unbounded(node: &FormatNode) -> Option<String> {
    match node {
        FormatNode::Blank => None,
        FormatNode::Line { content, .. } => {
            if content.starts_with("--") {
                None
            } else {
                Some(content.clone())
            }
        }
        FormatNode::BracketBlock {
            prefix, children, ..
        } => {
            let nonblank = children
                .iter()
                .filter(|child| !matches!(child, FormatNode::Blank))
                .collect::<Vec<_>>();
            match nonblank.as_slice() {
                [] => Some(bracket_inline(prefix, "")),
                [child] => {
                    let child = format_node_inline_text_unbounded(child)?;
                    Some(bracket_inline(prefix, &child))
                }
                _ => None,
            }
        }
    }
}

fn bracket_open(prefix: &str) -> String {
    if prefix.is_empty() {
        "[".to_owned()
    } else {
        format!("{prefix} [")
    }
}

fn bracket_inline(prefix: &str, inner: &str) -> String {
    if prefix.is_empty() {
        format!("[{inner}]")
    } else if inner.is_empty() {
        format!("{prefix} []")
    } else {
        format!("{prefix} [{inner}]")
    }
}

pub fn parse_ast(path: &str, source: &str) -> Result<AstProgram, ParseError> {
    let tokens = lex_source(path, source)?;
    let text_body_line_ranges = text_literal_body_line_ranges(&tokens);
    let lines = parser_lines(&tokens);
    let item_lines = merge_multiline_bytes_lines(&lines, &text_body_line_ranges);
    let item_lines = merge_multiline_drain_lines(&item_lines, &text_body_line_ranges);
    let item_lines = merge_multiline_call_expression_lines(&item_lines, &text_body_line_ranges);
    let items = parser_items(&item_lines, &text_body_line_ranges);
    let mut expressions = Vec::new();
    let mut statements = ast_statement_tree(&items, &mut expressions, source);
    link_multiline_expression_structure(&mut statements, &mut expressions);
    normalize_unlinked_unary_negation(&mut expressions);
    validate_pipeline_inputs(path, source, &expressions)?;
    let ast = AstProgram {
        tokens,
        lines,
        items,
        statements,
        expressions,
    };
    validate_match_patterns(path, &ast)?;
    Ok(ast)
}

fn link_multiline_expression_structure(
    statements: &mut [AstStatement],
    expressions: &mut Vec<AstExpr>,
) {
    link_multiline_expression_structure_with_input(statements, expressions, None);
}

fn link_multiline_expression_structure_with_input(
    statements: &mut [AstStatement],
    expressions: &mut Vec<AstExpr>,
    mut previous: Option<usize>,
) {
    for statement in statements.iter_mut() {
        if let Some(target) = statement_pipeline_continuation_target(statement, expressions)
            && let Some(input) = previous
            && let Some(expression) = expressions.get_mut(target)
        {
            expression.linked_input = Some(input);
            if let AstExprKind::Infix { left, .. } = &mut expression.kind {
                *left = input;
            }
        }
        let child_input = statement_child_pipeline_input(statement, expressions);
        link_multiline_expression_structure_with_input(
            &mut statement.children,
            expressions,
            child_input,
        );
        if let Some(output) = leading_pipeline_continuation_result(&statement.children, expressions)
        {
            replace_statement_inline_output(statement, expressions, output);
        }
        materialize_statement_structure(statement, expressions);
        let value = statement_value_expression(statement, expressions);
        if let Some(value) = value {
            previous = Some(value);
        }
    }
}

fn statement_pipeline_continuation_target(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    if matches!(
        statement.kind,
        AstStatementKind::Function { .. }
            | AstStatementKind::Field { .. }
            | AstStatementKind::Source { field: Some(_), .. }
            | AstStatementKind::Hold { field: Some(_), .. }
            | AstStatementKind::List { field: Some(_), .. }
    ) {
        return None;
    }
    pipeline_placeholder_target(statement.expr?, expressions)
}

fn normalize_unlinked_unary_negation(expressions: &mut [AstExpr]) {
    let zero_expressions = expressions
        .iter()
        .filter_map(|expression| {
            let AstExprKind::Infix { left, op, .. } = &expression.kind else {
                return None;
            };
            (expression.linked_input.is_none()
                && op == "-"
                && expressions
                    .get(*left)
                    .is_some_and(|left| matches!(left.kind, AstExprKind::Delimiter)))
            .then_some(*left)
        })
        .collect::<BTreeSet<_>>();
    for expression in zero_expressions {
        if let Some(expression) = expressions.get_mut(expression) {
            expression.kind = AstExprKind::Number("0".to_owned());
        }
    }
}

fn statement_is_pipeline_continuation(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    statement_pipeline_continuation_target(statement, expressions).is_some()
}

fn pipeline_placeholder_target(expr_id: usize, expressions: &[AstExpr]) -> Option<usize> {
    let expression = expressions.get(expr_id)?;
    if expression.linked_input.is_some() && matches!(expression.kind, AstExprKind::Infix { .. }) {
        return Some(expr_id);
    }
    let input = match &expression.kind {
        AstExprKind::Pipe { input, .. }
        | AstExprKind::Then { input, .. }
        | AstExprKind::When { input, .. }
        | AstExprKind::Draining { input }
        | AstExprKind::Hold { initial: input, .. } => *input,
        AstExprKind::Infix { left, .. } => *left,
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => return pipeline_placeholder_target(*output, expressions),
        _ => return None,
    };
    if expressions
        .get(input)
        .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
    {
        Some(expr_id)
    } else {
        pipeline_placeholder_target(input, expressions)
    }
}

fn statement_child_pipeline_input(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    let owner = statement_structure_owner(statement.expr?, expressions);
    match &expressions.get(owner)?.kind {
        AstExprKind::MatchArm { output, .. } | AstExprKind::Then { output, .. } => *output,
        AstExprKind::Block { .. }
        | AstExprKind::Object(_)
        | AstExprKind::ListLiteral { .. }
        | AstExprKind::BytesLiteral { .. }
        | AstExprKind::Flush { .. }
        | AstExprKind::Hold { .. }
        | AstExprKind::Latest { .. }
        | AstExprKind::When { .. } => None,
        AstExprKind::Pipe { op, .. } if op == "WHILE" => None,
        _ => Some(owner),
    }
}

fn leading_pipeline_continuation_result(
    statements: &[AstStatement],
    expressions: &[AstExpr],
) -> Option<usize> {
    let mut result = None;
    for statement in statements {
        if statement_pipeline_continuation_target(statement, expressions).is_none() {
            break;
        }
        result = statement_value_expression(statement, expressions);
    }
    result
}

fn replace_statement_inline_output(
    statement: &AstStatement,
    expressions: &mut [AstExpr],
    output: usize,
) {
    let Some(expr_id) = statement.expr else {
        return;
    };
    let owner = statement_structure_owner(expr_id, expressions);
    let Some(expression) = expressions.get_mut(owner) else {
        return;
    };
    match &mut expression.kind {
        AstExprKind::MatchArm {
            output: arm_output, ..
        }
        | AstExprKind::Then {
            output: arm_output, ..
        } if arm_output.is_some() => *arm_output = Some(output),
        _ => {}
    }
}

fn validate_pipeline_inputs(
    path: &str,
    source: &str,
    expressions: &[AstExpr],
) -> Result<(), ParseError> {
    for expression in expressions {
        let input = match &expression.kind {
            AstExprKind::Pipe { input, .. }
            | AstExprKind::Then { input, .. }
            | AstExprKind::When { input, .. }
            | AstExprKind::Draining { input }
            | AstExprKind::Hold { initial: input, .. }
            | AstExprKind::Infix { left: input, .. } => *input,
            _ => continue,
        };
        if expressions
            .get(input)
            .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
            && expression.linked_input.is_none()
        {
            let line_start = source
                .get(..expression.start)
                .and_then(|prefix| prefix.rfind('\n'))
                .map_or(0, |newline| newline + 1);
            let column = source
                .get(line_start..expression.start)
                .map_or(1, |prefix| prefix.chars().count() + 1);
            return Err(error(
                path,
                expression.line,
                column,
                "pipeline continuation has no preceding value in its lexical sequence",
            ));
        }
    }
    Ok(())
}

fn materialize_statement_structure(statement: &mut AstStatement, expressions: &mut [AstExpr]) {
    let child_values = statement_sequence_values(&statement.children, expressions);
    let child_result = child_values.last().copied();
    let child_arms = statement
        .children
        .iter()
        .filter_map(|child| child.expr)
        .filter(|expr_id| {
            expressions
                .get(*expr_id)
                .is_some_and(|expr| matches!(expr.kind, AstExprKind::MatchArm { .. }))
        })
        .collect::<Vec<_>>();

    let Some(expr_id) = statement.expr else {
        return;
    };
    let expr_id = statement_structure_owner(expr_id, expressions);
    let block_bindings = statement
        .children
        .iter()
        .filter_map(|child| {
            let name = statement_binding_name(child)?.to_owned();
            Some(AstBlockBinding {
                name,
                statement: child.id,
                value: statement_value_expression(child, expressions)?,
                start: child.start,
                end: child.end,
            })
        })
        .collect::<Vec<_>>();
    let record_fields = statement
        .children
        .iter()
        .enumerate()
        .filter_map(|(index, child)| {
            let (name, spread) = match &child.kind {
                AstStatementKind::Spread => (format!("__spread_{index}"), true),
                _ => (statement_binding_name(child)?.to_owned(), false),
            };
            Some(AstRecordField {
                name,
                value: statement_value_expression(child, expressions)?,
                start: child.start,
                end: child.end,
                spread,
            })
        })
        .collect::<Vec<_>>();

    let Some(expression) = expressions.get_mut(expr_id) else {
        return;
    };
    match &mut expression.kind {
        AstExprKind::When { arms, .. } => {
            if arms.is_empty() {
                *arms = child_arms;
            }
        }
        AstExprKind::Latest { branches } => {
            if branches.is_empty() {
                *branches = child_values;
            }
        }
        AstExprKind::Pipe { op, arms, .. } if op == "WHILE" => {
            if arms.is_empty() {
                *arms = child_arms;
            }
        }
        AstExprKind::Then { output, .. } | AstExprKind::MatchArm { output, .. } => {
            if output.is_none() {
                *output = child_result;
            }
        }
        AstExprKind::Flush { payload } => {
            if payload.is_none() {
                *payload = child_result;
            }
        }
        AstExprKind::Block { bindings, result } => {
            if bindings.is_empty() {
                *bindings = block_bindings;
            }
            if result.is_none() {
                *result = child_result;
            }
        }
        AstExprKind::Object(fields) => {
            if fields.is_empty() {
                *fields = record_fields;
            }
        }
        AstExprKind::ListLiteral { items, .. } if items.is_empty() => {
            *items = child_values;
        }
        _ => {}
    }
}

fn statement_structure_owner(expr_id: usize, expressions: &[AstExpr]) -> usize {
    let Some(expression) = expressions.get(expr_id) else {
        return expr_id;
    };
    match &expression.kind {
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } if expression_owns_statement_children(*output, expressions) => {
            statement_structure_owner(*output, expressions)
        }
        _ => expr_id,
    }
}

fn expression_owns_statement_children(expr_id: usize, expressions: &[AstExpr]) -> bool {
    expressions.get(expr_id).is_some_and(|expression| {
        matches!(
            &expression.kind,
            AstExprKind::Block { .. }
                | AstExprKind::Object(_)
                | AstExprKind::ListLiteral { .. }
                | AstExprKind::BytesLiteral { .. }
                | AstExprKind::Flush { .. }
                | AstExprKind::Hold { .. }
                | AstExprKind::Latest { .. }
                | AstExprKind::When { .. }
                | AstExprKind::Then { .. }
        ) || matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "WHILE")
    })
}

fn statement_sequence_values(statements: &[AstStatement], expressions: &[AstExpr]) -> Vec<usize> {
    let mut values = Vec::new();
    for statement in statements {
        let Some(value) = statement_value_expression(statement, expressions) else {
            continue;
        };
        if statement_is_pipeline_continuation(statement, expressions) && !values.is_empty() {
            *values.last_mut().expect("non-empty values") = value;
        } else {
            values.push(value);
        }
    }
    values
}

fn statement_value_expression(statement: &AstStatement, expressions: &[AstExpr]) -> Option<usize> {
    if statement
        .expr
        .and_then(|expr_id| expressions.get(expr_id))
        .is_some_and(|expr| {
            matches!(
                &expr.kind,
                AstExprKind::Block { .. }
                    | AstExprKind::Object(_)
                    | AstExprKind::ListLiteral { .. }
                    | AstExprKind::BytesLiteral { .. }
                    | AstExprKind::Flush { .. }
                    | AstExprKind::Hold { .. }
                    | AstExprKind::Latest { .. }
                    | AstExprKind::When { .. }
                    | AstExprKind::Then { .. }
                    | AstExprKind::MatchArm { .. }
            ) || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "WHILE")
        })
    {
        return statement.expr;
    }
    statement_sequence_values(&statement.children, expressions)
        .last()
        .copied()
        .or(statement.expr)
}

fn statement_binding_name(statement: &AstStatement) -> Option<&str> {
    match &statement.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::Source {
            field: Some(name), ..
        }
        | AstStatementKind::Hold {
            field: Some(name), ..
        }
        | AstStatementKind::List {
            field: Some(name), ..
        } => Some(name),
        AstStatementKind::Function { .. }
        | AstStatementKind::Source { field: None, .. }
        | AstStatementKind::Hold { field: None, .. }
        | AstStatementKind::List { field: None, .. }
        | AstStatementKind::Block
        | AstStatementKind::Spread
        | AstStatementKind::Expression => None,
    }
}

/// Tokenizes Boon source with the same lexer used by the parser.
///
/// Editor tooling uses this surface so highlighting and semantic inspection do
/// not need a second, drifting approximation of the language grammar.
pub fn lex_source(path: &str, source: &str) -> Result<Vec<AstToken>, ParseError> {
    let source_index = SourceIndex::new(source);
    let spanned = token_parser()
        .repeated()
        .then_ignore(end())
        .parse(source)
        .map_err(|errors| {
            let message = errors
                .into_iter()
                .next()
                .map(|error| format!("syntax error near {:?}", error.span()))
                .unwrap_or_else(|| "syntax error".to_owned());
            ParseError {
                path: path.to_owned(),
                line: None,
                column: None,
                message,
            }
        })?;
    Ok(spanned
        .into_iter()
        .map(|(kind, span)| {
            let start = source_index.char_offset_to_byte(span.start);
            let end = source_index.char_offset_to_byte(span.end);
            let (line, column) = source_index.line_column(source, start);
            let raw_lexeme = source.get(start..end).unwrap_or_default();
            let lexeme = match kind {
                AstTokenKind::String | AstTokenKind::Comment | AstTokenKind::Newline => raw_lexeme,
                _ => raw_lexeme.trim_matches(|ch| matches!(ch, ' ' | '\t' | '\r')),
            };
            AstToken {
                kind,
                lexeme: lexeme.to_owned(),
                line,
                column,
                start,
                end,
            }
        })
        .collect())
}

fn document_statement(ast: &AstProgram) -> Option<&AstStatement> {
    ast.statements.iter().find(|statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "document"
        )
    })
}

fn scene_statement(ast: &AstProgram) -> Option<&AstStatement> {
    ast.statements.iter().find(|statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "scene"
        )
    })
}

fn statement_contains_line(statement: &AstStatement, line: usize) -> bool {
    statement.line == line
        || statement
            .children
            .iter()
            .any(|child| statement_contains_line(child, line))
}

fn token_parser() -> impl Parser<char, (AstTokenKind, std::ops::Range<usize>), Error = Simple<char>>
{
    let horizontal_space = one_of(" \t\r").repeated().ignored();
    let ident_start = filter(|ch: &char| ch.is_ascii_alphabetic() || *ch == '_');
    let ident_tail =
        filter(|ch: &char| ch.is_ascii_alphanumeric() || matches!(*ch, '_' | '-' | '/'));
    let identifier = ident_start
        .then(ident_tail.repeated())
        .to(AstTokenKind::Identifier);
    let number = text::int(10).to(AstTokenKind::Number);
    let string = just('"')
        .ignore_then(
            choice((
                just('\\').ignore_then(any()).ignored(),
                none_of('"').ignored(),
            ))
            .repeated(),
        )
        .then_ignore(just('"'))
        .to(AstTokenKind::String);
    let comment = just("--")
        .ignore_then(none_of('\n').repeated())
        .to(AstTokenKind::Comment);
    let operator = choice((
        just("=>").ignored(),
        just("|>").ignored(),
        just("==").ignored(),
        just(">=").ignored(),
        just("<=").ignored(),
        just("!=").ignored(),
        one_of("><=|+-%*/").ignored(),
    ))
    .to(AstTokenKind::Operator);
    let symbol = one_of("[]{}():,.$#").to(AstTokenKind::Symbol);
    let newline = just('\n').to(AstTokenKind::Newline);
    let unknown = any().to(AstTokenKind::Unknown);

    choice((
        string, comment, identifier, number, operator, symbol, newline, unknown,
    ))
    .padded_by(horizontal_space)
    .map_with_span(|kind, span| (kind, span))
}

struct SourceIndex {
    char_to_byte: Vec<usize>,
    line_starts: Vec<usize>,
}

impl SourceIndex {
    fn new(source: &str) -> Self {
        let mut char_to_byte = source
            .char_indices()
            .map(|(byte, _)| byte)
            .collect::<Vec<_>>();
        char_to_byte.push(source.len());
        let mut line_starts = vec![0];
        for (byte, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(byte + ch.len_utf8());
            }
        }
        Self {
            char_to_byte,
            line_starts,
        }
    }

    fn char_offset_to_byte(&self, char_offset: usize) -> usize {
        self.char_to_byte
            .get(char_offset)
            .copied()
            .unwrap_or_else(|| *self.char_to_byte.last().unwrap_or(&0))
    }

    fn line_column(&self, source: &str, byte_index: usize) -> (usize, usize) {
        let line_index = self
            .line_starts
            .partition_point(|start| *start <= byte_index);
        let line = line_index.max(1);
        let line_start = self.line_starts[line - 1];
        let column = source
            .get(line_start..byte_index)
            .map(|slice| slice.chars().count() + 1)
            .unwrap_or(1);
        (line, column)
    }
}

fn parser_lines(tokens: &[AstToken]) -> Vec<ParserLine> {
    let mut lines = Vec::new();
    let mut current_line = None;
    let mut indent = 0usize;
    let mut start = 0usize;
    let mut end = 0usize;
    let mut symbols = Vec::new();
    let mut symbol_spans = Vec::new();
    for token in tokens {
        if current_line != Some(token.line) {
            if let Some(line) = current_line {
                lines.push(ParserLine {
                    line,
                    indent,
                    symbols: std::mem::take(&mut symbols),
                    symbol_spans: std::mem::take(&mut symbol_spans),
                    start,
                    end,
                });
            }
            current_line = Some(token.line);
            indent = token.column.saturating_sub(1);
            start = token.start;
        }
        end = token.end;
        if !matches!(token.kind, AstTokenKind::Comment | AstTokenKind::Newline)
            && !token.lexeme.is_empty()
        {
            symbols.push(token.lexeme.clone());
            symbol_spans.push((token.start, token.end));
        }
    }
    if let Some(line) = current_line {
        lines.push(ParserLine {
            line,
            indent,
            symbols,
            symbol_spans,
            start,
            end,
        });
    }
    lines
}

fn merge_multiline_bytes_lines(
    lines: &[ParserLine],
    text_body_line_ranges: &[(usize, usize)],
) -> Vec<ParserLine> {
    merge_multiline_braced_lines(lines, text_body_line_ranges, unclosed_bytes_body_open)
}

fn merge_multiline_drain_lines(
    lines: &[ParserLine],
    text_body_line_ranges: &[(usize, usize)],
) -> Vec<ParserLine> {
    merge_multiline_braced_lines(lines, text_body_line_ranges, unclosed_drain_body_open)
}

fn merge_multiline_call_expression_lines(
    lines: &[ParserLine],
    text_body_line_ranges: &[(usize, usize)],
) -> Vec<ParserLine> {
    let mut merged = Vec::new();
    let mut index = 0usize;
    while let Some(line) = lines.get(index) {
        if line_is_in_ranges(line.line, text_body_line_ranges) {
            merged.push(line.clone());
            index += 1;
            continue;
        }
        let Some(open) = line.symbols.iter().position(|symbol| symbol == "(") else {
            merged.push(line.clone());
            index += 1;
            continue;
        };
        if matching_close(&line.symbols, open).is_some() {
            merged.push(line.clone());
            index += 1;
            continue;
        }

        let mut end_index = index;
        let mut candidate = line.symbols.clone();
        let mut outer_close = None;
        while outer_close.is_none() {
            end_index += 1;
            let Some(next) = lines.get(end_index) else {
                break;
            };
            if line_is_in_ranges(next.line, text_body_line_ranges) {
                break;
            }
            candidate.extend(next.symbols.iter().cloned());
            outer_close = matching_close(&candidate, open);
        }
        let Some(_) = outer_close else {
            merged.push(line.clone());
            index += 1;
            continue;
        };
        let mut expression_line = line.clone();
        for next in &lines[index + 1..=end_index] {
            if multiline_expression_needs_separator(&expression_line.symbols, &next.symbols) {
                let separator = expression_line
                    .symbol_spans
                    .last()
                    .map(|(_, end)| (*end, *end))
                    .unwrap_or((expression_line.end, expression_line.end));
                expression_line.symbols.push(",".to_owned());
                expression_line.symbol_spans.push(separator);
            }
            expression_line.end = next.end;
            expression_line.symbols.extend(next.symbols.iter().cloned());
            expression_line
                .symbol_spans
                .extend(next.symbol_spans.iter().copied());
        }
        merged.push(expression_line);
        index = end_index + 1;
    }
    merged
}

fn multiline_expression_needs_separator(current: &[String], next: &[String]) -> bool {
    let Some(previous) = current.last().map(String::as_str) else {
        return false;
    };
    let Some(next) = next.first().map(String::as_str) else {
        return false;
    };
    !matches!(previous, "(" | "[" | "{" | ":" | "," | "|>" | "=>")
        && !matches!(next, ")" | "]" | "}" | "," | "|>" | "=>")
}

fn merge_multiline_braced_lines(
    lines: &[ParserLine],
    text_body_line_ranges: &[(usize, usize)],
    unclosed_body_open: fn(&[String]) -> Option<usize>,
) -> Vec<ParserLine> {
    let mut merged = Vec::new();
    let mut index = 0usize;
    while let Some(line) = lines.get(index) {
        if line_is_in_ranges(line.line, text_body_line_ranges) {
            merged.push(line.clone());
            index += 1;
            continue;
        }
        let Some(body_open) = unclosed_body_open(&line.symbols) else {
            merged.push(line.clone());
            index += 1;
            continue;
        };
        let mut current = line.clone();
        index += 1;
        while matching_close(&current.symbols, body_open).is_none() {
            let Some(next) = lines.get(index) else {
                break;
            };
            if line_is_in_ranges(next.line, text_body_line_ranges) {
                break;
            }
            current.end = next.end;
            current.symbols.extend(next.symbols.iter().cloned());
            current
                .symbol_spans
                .extend(next.symbol_spans.iter().copied());
            index += 1;
        }
        merged.push(current);
    }
    merged
}

fn line_is_in_ranges(line: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| line >= *start && line <= *end)
}

fn unclosed_bytes_body_open(symbols: &[String]) -> Option<usize> {
    let bytes = symbols.iter().position(|symbol| symbol == "BYTES")?;
    let body_open = match symbols.get(bytes + 1).map(String::as_str) {
        Some("{") => bytes + 1,
        Some("[") => {
            let size_close = matching_close(symbols, bytes + 1)?;
            (symbols.get(size_close + 1).map(String::as_str) == Some("{"))
                .then_some(size_close + 1)?
        }
        _ => return None,
    };
    matching_close(symbols, body_open)
        .is_none()
        .then_some(body_open)
}

fn unclosed_drain_body_open(symbols: &[String]) -> Option<usize> {
    let drain = symbols.iter().position(|symbol| symbol == "DRAIN")?;
    let body_open =
        (symbols.get(drain + 1).map(String::as_str) == Some("{")).then_some(drain + 1)?;
    matching_close(symbols, body_open)
        .is_none()
        .then_some(body_open)
}

fn parser_items(lines: &[ParserLine], text_body_line_ranges: &[(usize, usize)]) -> Vec<ParserItem> {
    lines
        .iter()
        .filter(|line| {
            !text_body_line_ranges
                .iter()
                .any(|(start, end)| line.line >= *start && line.line <= *end)
        })
        .filter(|line| !line.symbols.is_empty())
        .map(parser_item)
        .collect()
}

fn parser_item(line: &ParserLine) -> ParserItem {
    let symbols = line.symbols.clone();
    let symbol_spans = line.symbol_spans.clone();
    let field = ast_field_name(&symbols).map(ToOwned::to_owned);
    let function = (symbols.first().map(String::as_str) == Some("FUNCTION"))
        .then(|| symbols.get(1).cloned())
        .flatten();
    let source_event = ast_insource_slice_event(&symbols).map(ToOwned::to_owned);
    let operators = ast_expression_operators(&symbols);
    let is_list = symbols.iter().any(|lexeme| is_list_constructor(lexeme))
        && find_top_level_pipe(&symbols).is_none();
    ParserItem {
        line: line.line,
        indent: line.indent,
        start: line.start,
        end: line.end,
        source_event,
        hold: ast_hold_name(&symbols).map(ToOwned::to_owned),
        list_capacity: ast_list_capacity(&symbols),
        opens_scope: ast_opens_scope(&symbols),
        closes_scope: symbols.len() == 1
            && matches!(symbols.first().map(String::as_str), Some("}" | "]" | ")")),
        operators,
        symbols,
        symbol_spans,
        field,
        example: None,
        function,
        is_list,
    }
}

fn is_list_constructor(lexeme: &str) -> bool {
    matches!(lexeme, "LIST" | "List/range")
}

fn ast_statement_tree(
    items: &[ParserItem],
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<AstStatement> {
    let mut index = 0usize;
    let mut next_id = 0usize;
    ast_statement_block(items, &mut index, 0, expressions, &mut next_id, source)
}

fn ast_statement_block(
    items: &[ParserItem],
    index: &mut usize,
    min_indent: usize,
    expressions: &mut Vec<AstExpr>,
    next_id: &mut usize,
    source: &str,
) -> Vec<AstStatement> {
    let mut statements = Vec::new();
    while let Some(item) = items.get(*index) {
        if item.indent < min_indent {
            break;
        }
        if item.closes_scope {
            *index += 1;
            continue;
        }
        let indent = item.indent;
        let mut statement = ast_statement(item, expressions, *next_id, source);
        *next_id += 1;
        *index += 1;
        if item.opens_scope || items.get(*index).is_some_and(|next| next.indent > indent) {
            statement.children =
                ast_statement_block(items, index, indent + 1, expressions, next_id, source);
        }
        statements.push(statement);
    }
    statements
}

fn ast_statement(
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    id: usize,
    source: &str,
) -> AstStatement {
    let is_semantic_block = item.symbols.first().map(String::as_str) == Some("BLOCK")
        && item.symbols.last().map(String::as_str) == Some("{");
    let kind = if let Some(function) = item.function.clone() {
        AstStatementKind::Function {
            name: function,
            parameters: ast_function_parameters(&item.symbols, item),
        }
    } else if item.has_lexeme("SOURCE") {
        AstStatementKind::Source {
            field: item.field.clone(),
            event: item.source_event.clone(),
        }
    } else if item.has_lexeme("HOLD") {
        AstStatementKind::Hold {
            field: item.field.clone(),
            name: item.hold.clone(),
        }
    } else if item.is_list {
        AstStatementKind::List {
            field: item.field.clone(),
            capacity: item.list_capacity,
        }
    } else if let Some(field) = item.field.clone() {
        AstStatementKind::Field { name: field }
    } else if is_semantic_block
        || matches!(item.symbols.as_slice(), [one] if matches!(one.as_str(), "[" | "{" | "(" | "]" | "}" | ")"))
    {
        AstStatementKind::Block
    } else if item
        .symbols
        .starts_with(&[".".to_owned(), ".".to_owned(), ".".to_owned()])
    {
        AstStatementKind::Spread
    } else {
        AstStatementKind::Expression
    };
    let expr = if matches!(kind, AstStatementKind::Function { .. }) {
        None
    } else if matches!(kind, AstStatementKind::Block) && !is_semantic_block {
        (item.symbols.first().map(String::as_str) == Some("[")).then(|| {
            push_ast_expr(
                item,
                expressions,
                AstExprKind::Object(Vec::new()),
                item.start,
                item.end,
            )
        })
    } else if item.field.is_some()
        && item.symbols.get(1).map(String::as_str) == Some(":")
        && item.symbols.get(2).map(String::as_str) == Some("[")
        && item.symbols.len() == 3
    {
        Some(push_ast_expr(
            item,
            expressions,
            AstExprKind::Object(Vec::new()),
            item.start,
            item.end,
        ))
    } else {
        let expr_tokens = statement_expression_tokens(item);
        (!expr_tokens.is_empty()).then(|| parse_ast_expr(&expr_tokens, item, expressions, source))
    };
    AstStatement {
        id,
        line: item.line,
        indent: item.indent,
        start: item.start,
        end: item.end,
        kind,
        expr,
        children: Vec::new(),
    }
}

fn statement_expression_tokens(item: &ParserItem) -> Vec<String> {
    if item.field.is_some() && item.symbols.get(1).map(String::as_str) == Some(":") {
        if matches!(
            item.symbols.get(2).map(String::as_str),
            Some("[") | Some("{")
        ) && item.symbols.len() == 3
        {
            return Vec::new();
        }
        return item.symbols[2..].to_vec();
    }
    item.symbols.clone()
}

fn parse_ast_expr(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> usize {
    let kind = ast_expr_kind(tokens, item, expressions, source);
    let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
    push_ast_expr(item, expressions, kind, start, end)
}

fn push_ast_expr(
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    kind: AstExprKind,
    start: usize,
    end: usize,
) -> usize {
    let id = expressions.len();
    expressions.push(AstExpr {
        id,
        line: item.line,
        start,
        end,
        linked_input: None,
        kind,
    });
    id
}

fn span_for_tokens(tokens: &[String], item: &ParserItem) -> Option<(usize, usize)> {
    if tokens.is_empty() {
        return None;
    }
    item.symbols
        .windows(tokens.len())
        .position(|window| window == tokens)
        .and_then(|start_index| {
            let end_index = start_index + tokens.len() - 1;
            Some((
                item.symbol_spans.get(start_index)?.0,
                item.symbol_spans.get(end_index)?.1,
            ))
        })
}

fn ast_expr_kind(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> AstExprKind {
    if tokens.is_empty() {
        return AstExprKind::Delimiter;
    }
    if tokens.len() > 3 && tokens[0] == "." && tokens[1] == "." && tokens[2] == "." {
        return ast_expr_kind(&tokens[3..], item, expressions, source);
    }
    if tokens
        .iter()
        .all(|token| matches!(token.as_str(), "[" | "]" | "{" | "}" | "(" | ")"))
    {
        return AstExprKind::Delimiter;
    }
    if tokens.first().map(String::as_str) == Some("(")
        && matching_close(tokens, 0) == Some(tokens.len() - 1)
    {
        return ast_expr_kind(&tokens[1..tokens.len() - 1], item, expressions, source);
    }
    if tokens == ["SOURCE"] {
        return AstExprKind::Source;
    }
    if tokens == ["True"] {
        return AstExprKind::Tag("True".to_owned());
    }
    if tokens == ["False"] {
        return AstExprKind::Tag("False".to_owned());
    }
    if let Some(number) = ast_number_literal(tokens) {
        return AstExprKind::Number(number);
    }
    if let Some((radix, digits, value)) = ast_byte_literal(tokens, item, source) {
        return AstExprKind::ByteLiteral {
            radix,
            digits,
            value,
        };
    }
    if let Some(arrow) = find_top_level_token(tokens, "=>") {
        let pattern = &tokens[..arrow];
        return AstExprKind::MatchArm {
            pattern: ast_match_pattern(pattern, item, source),
            output: (!tokens[arrow + 1..].is_empty())
                .then(|| parse_ast_expr(&tokens[arrow + 1..], item, expressions, source)),
        };
    }
    if let Some(value) = string_literal_value(tokens) {
        return AstExprKind::StringLiteral(value);
    }
    if let Some(text) = parsed_text_literal(tokens, item, source) {
        if let Some(segments) =
            ast_text_template_segments(&text.value, text.value_start, item, expressions, source)
        {
            return AstExprKind::TextTemplate { segments };
        }
        return AstExprKind::TextLiteral(text.value);
    }
    if tokens == ["Text/empty", "(", ")"] {
        return AstExprKind::TextLiteral(String::new());
    }
    if tokens.first().map(String::as_str) == Some("FLUSH") {
        let payload = if tokens.get(1).map(String::as_str) == Some("{")
            && matching_close(tokens, 1) == Some(tokens.len() - 1)
        {
            (!tokens[2..tokens.len() - 1].is_empty())
                .then(|| parse_ast_expr(&tokens[2..tokens.len() - 1], item, expressions, source))
        } else {
            None
        };
        return AstExprKind::Flush { payload };
    }
    if tokens.first().map(String::as_str) == Some("BLOCK")
        && tokens.last().map(String::as_str) == Some("{")
    {
        return AstExprKind::Block {
            bindings: Vec::new(),
            result: None,
        };
    }
    if let Some(path) = ast_drain_path(tokens) {
        return AstExprKind::Drain { path };
    }
    if let Some(pipe) = find_top_level_pipe(tokens) {
        return ast_pipe_expr_kind(tokens, pipe, item, expressions, source);
    }
    if tokens.first().map(String::as_str) == Some("LATEST") {
        return AstExprKind::Latest {
            branches: ast_latest_branches(tokens, item, expressions, source),
        };
    }
    if tokens.first().map(String::as_str) == Some("LIST") {
        return AstExprKind::ListLiteral {
            capacity: ast_list_capacity(tokens),
            items: ast_list_items(tokens, item, expressions, source),
        };
    }
    if let Some((size, items)) = ast_bytes_literal(tokens, item, expressions, source) {
        return AstExprKind::BytesLiteral { size, items };
    }
    if tokens.first().map(String::as_str) == Some("[")
        && tokens.last().map(String::as_str) == Some("]")
    {
        return AstExprKind::Object(ast_record_fields(tokens, item, expressions, source));
    }
    if tokens.first().map(String::as_str) == Some("[")
        && tokens.get(2).map(String::as_str) == Some(":")
        && tokens.len() > 3
    {
        let value = parse_ast_expr(&tokens[3..], item, expressions, source);
        let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
        return AstExprKind::Object(vec![AstRecordField {
            name: tokens[1].clone(),
            value,
            start,
            end,
            spread: false,
        }]);
    }
    if tokens.len() >= 3
        && tokens.get(1).map(String::as_str) == Some("[")
        && tokens.last().map(String::as_str) == Some("]")
        && tokens
            .first()
            .is_some_and(|token| value_starts_uppercase_identifier(token))
    {
        return AstExprKind::TaggedObject {
            tag: tokens[0].clone(),
            fields: ast_record_fields(&tokens[1..], item, expressions, source),
        };
    }
    if let Some((op, right)) = split_leading_infix(tokens) {
        let left = parse_ast_expr(&[], item, expressions, source);
        let right = parse_ast_expr(right, item, expressions, source);
        return AstExprKind::Infix {
            left,
            op: op.to_owned(),
            right,
        };
    }
    if let Some((left, op, right)) = split_infix(tokens) {
        let left = parse_ast_expr(left, item, expressions, source);
        let right = parse_ast_expr(right, item, expressions, source);
        return AstExprKind::Infix {
            left,
            op: op.to_owned(),
            right,
        };
    }
    if let Some((input_tokens, field)) = split_postfix_field_access(tokens) {
        let input = parse_ast_expr(input_tokens, item, expressions, source);
        return AstExprKind::Pipe {
            input,
            op: format!("Field/{field}"),
            args: Vec::new(),
            pass: None,
            arms: Vec::new(),
        };
    }
    if let Some((function, args, pass)) = ast_call(tokens, item, expressions, source) {
        return AstExprKind::Call {
            function,
            args,
            pass,
        };
    }
    if tokens.len() == 1 && split_role_value_head(&tokens[0]).is_some() {
        AstExprKind::Path(path_segments(tokens))
    } else if tokens.len() == 1 && is_name(&tokens[0]) {
        let token = tokens[0].clone();
        if value_starts_uppercase_identifier(&token) {
            AstExprKind::Tag(token)
        } else {
            AstExprKind::Identifier(token)
        }
    } else if tokens.iter().any(|token| token == ".") {
        AstExprKind::Path(path_segments(tokens))
    } else {
        AstExprKind::Unknown(tokens.to_vec())
    }
}

fn ast_match_pattern(tokens: &[String], item: &ParserItem, source: &str) -> AstMatchPattern {
    if tokens == ["__"] {
        return AstMatchPattern::Wildcard;
    }
    if tokens == ["True"] {
        return AstMatchPattern::Tag {
            name: "True".to_owned(),
            fields: Vec::new(),
        };
    }
    if tokens == ["False"] {
        return AstMatchPattern::Tag {
            name: "False".to_owned(),
            fields: Vec::new(),
        };
    }
    if tokens == ["NaN"] {
        return AstMatchPattern::Invalid {
            message: "`NaN` is not a Number or a valid match pattern; handle typed parse and find results instead".to_owned(),
        };
    }
    if let Some(value) = ast_number_literal(tokens) {
        return AstMatchPattern::Number { value };
    }
    if let Some(value) = string_literal_value(tokens) {
        return AstMatchPattern::Text { value };
    }
    if let Some(value) = text_literal_value(tokens, item, source) {
        return AstMatchPattern::Text { value };
    }
    if tokens.len() == 1 && matches!(tokens[0].as_str(), "FLUSH" | "FLUSHED" | "SKIP" | "SOURCE") {
        return AstMatchPattern::Invalid {
            message: "private flow-control states cannot be matched as public values".to_owned(),
        };
    }
    if tokens.len() == 1
        && matches!(
            tokens[0].as_str(),
            "BITS" | "BYTES" | "LIST" | "MAP" | "NUMBER" | "SET" | "TEXT"
        )
    {
        return AstMatchPattern::Invalid {
            message: invalid_match_pattern_message(tokens),
        };
    }
    if let Some(name) = tokens.first().filter(|_| tokens.len() == 1) {
        if value_starts_uppercase_identifier(name) {
            return AstMatchPattern::Tag {
                name: name.clone(),
                fields: Vec::new(),
            };
        }
        if is_name(name) {
            return AstMatchPattern::Binding { name: name.clone() };
        }
    }
    if let Some(name) = tokens.first().filter(|name| {
        value_starts_uppercase_identifier(name)
            && !matches!(
                name.as_str(),
                "BITS" | "BYTES" | "LIST" | "MAP" | "NUMBER" | "SET" | "TEXT"
            )
    }) && tokens.get(1).map(String::as_str) == Some("[")
        && matching_close(tokens, 1) == tokens.len().checked_sub(1)
    {
        return match ast_tag_pattern_fields(&tokens[2..tokens.len() - 1]) {
            Ok(fields) => AstMatchPattern::Tag {
                name: name.clone(),
                fields,
            },
            Err(message) => AstMatchPattern::Invalid { message },
        };
    }
    AstMatchPattern::Invalid {
        message: invalid_match_pattern_message(tokens),
    }
}

fn ast_tag_pattern_fields(tokens: &[String]) -> Result<Vec<String>, String> {
    if tokens.is_empty() {
        return Err(
            "tag payload patterns require at least one lowercase payload field binding".to_owned(),
        );
    }
    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "[" | "]" | "{" | "}" | ":" | "=>"))
    {
        return Err(
            "tag payload patterns do not support renaming, nesting, or comparison; use `Tag[field, ...]`"
                .to_owned(),
        );
    }
    if tokens.len() % 2 == 0 {
        return Err(
            "tag payload patterns must use `Tag[field, ...]` with comma-separated lowercase payload field bindings"
                .to_owned(),
        );
    }
    let mut fields = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if index % 2 == 1 {
            if token != "," {
                return Err(
                    "tag payload patterns do not support renaming, nesting, or comparison; use `Tag[field, ...]`"
                        .to_owned(),
                );
            }
            continue;
        }
        if !is_name(token)
            || !token
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_lowercase())
        {
            return Err(format!(
                "tag payload binding `{token}` must be a lowercase field name"
            ));
        }
        if fields.contains(token) {
            return Err(format!(
                "tag payload field `{token}` cannot be bound more than once"
            ));
        }
        fields.push(token.clone());
    }
    Ok(fields)
}

fn invalid_match_pattern_message(tokens: &[String]) -> String {
    let rendered = tokens.join(" ");
    match tokens.first().map(String::as_str) {
        None => "match arms require a pattern before `=>`".to_owned(),
        Some("[") => {
            "object patterns are unsupported; match an ordinary Tag and bind explicit payload fields"
                .to_owned()
        }
        Some("LIST") => {
            "LIST patterns are unsupported; use explicit list operations before matching".to_owned()
        }
        Some("MAP") => {
            "MAP patterns are unsupported; use explicit map operations before matching".to_owned()
        }
        Some("SET") => {
            "SET patterns are unsupported; use explicit set operations before matching".to_owned()
        }
        Some("BITS") => {
            "BITS patterns are unavailable until fixed-width BITS literals are implemented"
                .to_owned()
        }
        Some("BYTES" | "NUMBER" | "TEXT") => {
            "runtime type patterns are unsupported; match an exact literal or an ordinary Tag"
                .to_owned()
        }
        Some("{") => {
            "dynamic comparison patterns are unsupported; compare explicitly inside the arm"
                .to_owned()
        }
        Some(name) if value_starts_uppercase_identifier(name) => {
            "tag payload patterns must use `Tag[field, ...]` with lowercase payload field bindings"
                .to_owned()
        }
        _ => format!(
            "unsupported match pattern `{rendered}`; use `__`, a lowercase whole-value binding, an exact literal, a bare Tag, or `Tag[field, ...]`"
        ),
    }
}

fn ast_drain_path(tokens: &[String]) -> Option<AstDrainPath> {
    if tokens.first().map(String::as_str) != Some("DRAIN")
        || tokens.get(1).map(String::as_str) != Some("{")
        || matching_close(tokens, 1) != Some(tokens.len().checked_sub(1)?)
    {
        return None;
    }
    drain_path_from_symbols(&tokens[2..tokens.len() - 1])
}

fn drain_path_from_symbols(symbols: &[String]) -> Option<AstDrainPath> {
    if symbols.is_empty() || symbols.len().is_multiple_of(2) {
        return None;
    }
    let mut segments = Vec::with_capacity(symbols.len().div_ceil(2));
    for (index, symbol) in symbols.iter().enumerate() {
        if index % 2 == 1 {
            if symbol != "." {
                return None;
            }
            continue;
        }
        if !is_drain_path_segment(symbol) {
            return None;
        }
        segments.push(symbol.clone());
    }
    let root = segments.remove(0);
    if root == "PASSED" {
        return (!segments.is_empty()).then_some(AstDrainPath::Passed { fields: segments });
    }
    if value_starts_uppercase_identifier(&root) {
        return None;
    }
    if segments.is_empty() {
        Some(AstDrainPath::Binding { name: root })
    } else {
        Some(AstDrainPath::Field {
            binding: root,
            fields: segments,
        })
    }
}

fn is_drain_path_segment(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && is_name(value)
}

fn ast_number_literal(tokens: &[String]) -> Option<String> {
    match tokens {
        [value] if value.chars().all(|ch| ch.is_ascii_digit()) => Some(value.clone()),
        [left, dot, right]
            if dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && right.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            Some(format!("{left}.{right}"))
        }
        [left, dot, right @ ..]
            if dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && !right.is_empty()
                && right
                    .iter()
                    .all(|part| part.chars().all(|ch| ch.is_ascii_digit())) =>
        {
            Some(format!("{left}.{}", right.join("")))
        }
        [minus, value] if minus == "-" && value.chars().all(|ch| ch.is_ascii_digit()) => {
            Some(format!("-{value}"))
        }
        [minus, left, dot, right]
            if minus == "-"
                && dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && right.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            Some(format!("-{left}.{right}"))
        }
        [minus, left, dot, right @ ..]
            if minus == "-"
                && dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && !right.is_empty()
                && right
                    .iter()
                    .all(|part| part.chars().all(|ch| ch.is_ascii_digit())) =>
        {
            Some(format!("-{left}.{}", right.join("")))
        }
        _ => None,
    }
}

fn ast_byte_literal(
    tokens: &[String],
    item: &ParserItem,
    source: &str,
) -> Option<(u8, String, u8)> {
    let [base, suffix] = tokens else {
        return None;
    };
    let adjacent_text = format!("{base}{suffix}");
    let adjacent_in_source = source
        .get(item.start..item.end)
        .is_some_and(|item_source| item_source.contains(&adjacent_text));
    if !adjacent_in_source {
        return None;
    }
    parse_byte_literal_parts(base, suffix).ok()
}

fn parse_byte_literal_parts(base: &str, suffix: &str) -> Result<(u8, String, u8), String> {
    let radix = match base {
        "2" => 2,
        "8" => 8,
        "10" => 10,
        "16" => 16,
        _ => {
            return Err("byte literal base must be one of `2`, `8`, `10`, or `16`".to_owned());
        }
    };
    let Some(digits) = suffix.strip_prefix('u') else {
        return Err("byte literal must use explicit base notation such as `16uFF`".to_owned());
    };
    if digits.is_empty() {
        return Err("byte literal must include digits after `u`".to_owned());
    }
    if !digits.chars().all(|ch| ch.is_digit(radix as u32)) {
        return Err(format!(
            "byte literal `{base}u{digits}` contains digits outside base {radix}"
        ));
    }
    let value = u16::from_str_radix(digits, radix as u32).map_err(|_| {
        format!("byte literal `{base}u{digits}` could not be parsed in base {radix}")
    })?;
    if value > u8::MAX as u16 {
        return Err(format!(
            "byte literal `{base}u{digits}` evaluates to {value}, but bytes must be 0..255"
        ));
    }
    Ok((radix, digits.to_owned(), value as u8))
}

fn ast_pipe_expr_kind(
    tokens: &[String],
    pipe: usize,
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> AstExprKind {
    let input = parse_ast_expr(&tokens[..pipe], item, expressions, source);
    let op = tokens
        .get(pipe + 1)
        .cloned()
        .unwrap_or_else(|| "pipe".to_owned());
    if op == "DRAINING" && pipe + 2 == tokens.len() {
        return AstExprKind::Draining { input };
    }
    if op == "HOLD" {
        let name = tokens
            .get(pipe + 2)
            .cloned()
            .unwrap_or_else(|| "hold".to_owned());
        push_inline_hold_latest_exprs(&tokens[pipe + 3..], item, expressions, source);
        return AstExprKind::Hold {
            initial: input,
            name,
        };
    }
    if op == "WHEN" {
        let arms = parse_inline_match_arms(&tokens[pipe + 1..], item, expressions, source);
        return AstExprKind::When { input, arms };
    }
    if op == "THEN" {
        return AstExprKind::Then {
            input,
            output: ast_operator_block_expr(&tokens[pipe + 1..], item, expressions, source),
        };
    }
    let (args, pass) = ast_call_args_after_operator(&tokens[pipe + 1..], item, expressions, source);
    let arms = if op == "WHILE" {
        parse_inline_match_arms(&tokens[pipe + 1..], item, expressions, source)
    } else {
        Vec::new()
    };
    AstExprKind::Pipe {
        input,
        op,
        args,
        pass,
        arms,
    }
}

fn ast_record_fields(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<AstRecordField> {
    split_top_level(&tokens[1..tokens.len() - 1], ",")
        .into_iter()
        .enumerate()
        .filter_map(|(index, part)| {
            if part.starts_with(&[".".to_owned(), ".".to_owned(), ".".to_owned()]) && part.len() > 3
            {
                let (start, end) = span_for_tokens(&part, item).unwrap_or((item.start, item.end));
                return Some(AstRecordField {
                    name: format!("__spread_{index}"),
                    value: parse_ast_expr(&part[3..], item, expressions, source),
                    start,
                    end,
                    spread: true,
                });
            }
            if part.len() < 3 || part.get(1).map(String::as_str) != Some(":") {
                return None;
            }
            let (start, end) = span_for_tokens(&part, item).unwrap_or((item.start, item.end));
            Some(AstRecordField {
                name: part[0].clone(),
                value: parse_ast_expr(&part[2..], item, expressions, source),
                start,
                end,
                spread: false,
            })
        })
        .collect()
}

fn ast_call(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<(String, Vec<AstCallArg>, Option<AstPassContext>)> {
    let open = tokens.iter().position(|token| token == "(")?;
    if open == 0 {
        return None;
    }
    let function = tokens[..open].join("");
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    let arg_tokens = if close > open {
        &tokens[open + 1..close]
    } else {
        &[]
    };
    let (args, pass) = ast_call_parts(arg_tokens, item, expressions, source);
    Some((function, args, pass))
}

fn ast_call_args_after_operator(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> (Vec<AstCallArg>, Option<AstPassContext>) {
    let Some(open) = tokens.iter().position(|token| token == "(") else {
        return (Vec::new(), None);
    };
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    let arg_tokens = if close > open {
        &tokens[open + 1..close]
    } else {
        &[]
    };
    ast_call_parts(arg_tokens, item, expressions, source)
}

fn ast_call_parts(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> (Vec<AstCallArg>, Option<AstPassContext>) {
    let mut args = Vec::new();
    let mut pass = None;
    let parts = split_top_level(tokens, ",");
    for (index, part) in parts.iter().enumerate() {
        if part.first().map(String::as_str) == Some("PASS")
            && part.get(1).map(String::as_str) == Some(":")
        {
            let (start, end) = span_for_tokens(part, item).unwrap_or((item.start, item.end));
            let value = parse_ast_expr(&part[2..], item, expressions, source);
            if pass.is_none() {
                pass = Some(AstPassContext {
                    value,
                    start,
                    end,
                    final_clause: index + 1 == parts.len(),
                });
            } else {
                args.push(AstCallArg {
                    kind: AstCallArgKind::Named,
                    name: "PASS".to_owned(),
                    value,
                    start,
                    end,
                });
            }
        } else if let Some(arg) = ast_call_arg(part, item, expressions, source) {
            args.push(arg);
        }
    }
    (args, pass)
}

fn ast_operator_block_expr(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<usize> {
    let open = tokens.iter().position(|token| token == "{")?;
    let close = matching_close(tokens, open)?;
    (close > open + 1).then(|| parse_ast_expr(&tokens[open + 1..close], item, expressions, source))
}

fn push_inline_hold_latest_exprs(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) {
    let Some(latest) = tokens.iter().position(|token| token == "LATEST") else {
        return;
    };
    let _ = parse_ast_expr(&tokens[latest..], item, expressions, source);
}

fn ast_latest_branches(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return Vec::new();
    };
    let Some(close) = matching_close(tokens, open) else {
        return Vec::new();
    };
    if close <= open + 1 {
        return Vec::new();
    }
    let inner = &tokens[open + 1..close];
    let Some(token_offset) = item
        .symbols
        .windows(tokens.len())
        .position(|window| window == tokens)
        .map(|offset| offset + open + 1)
    else {
        return Vec::new();
    };
    let first_offset = item.symbol_spans.get(token_offset).map(|span| span.0);
    let base_indent = first_offset.map_or(0, |offset| source_line_indent(source, offset));
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut depth = 0_usize;
    for (index, token) in inner.iter().enumerate() {
        let token = token.as_str();
        if depth == 0 && token == "," {
            if start < index {
                ranges.push(start..index);
            }
            start = index + 1;
            continue;
        }
        if index > start
            && depth == 0
            && !latest_continuation_token(token)
            && latest_token_starts_peer_line(
                item,
                source,
                token_offset + index - 1,
                token_offset + index,
                base_indent,
            )
        {
            ranges.push(start..index);
            start = index;
        }
        match token {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if start < inner.len() {
        ranges.push(start..inner.len());
    }
    ranges
        .into_iter()
        .filter(|range| range.start < range.end)
        .map(|range| parse_ast_expr(&inner[range], item, expressions, source))
        .collect()
}

fn latest_token_starts_peer_line(
    item: &ParserItem,
    source: &str,
    previous: usize,
    current: usize,
    base_indent: usize,
) -> bool {
    let Some(previous_end) = item.symbol_spans.get(previous).map(|span| span.1) else {
        return false;
    };
    let Some(current_start) = item.symbol_spans.get(current).map(|span| span.0) else {
        return false;
    };
    source
        .get(previous_end..current_start)
        .is_some_and(|gap| gap.contains('\n'))
        && source_line_indent(source, current_start) <= base_indent
}

fn latest_continuation_token(token: &str) -> bool {
    matches!(
        token,
        "|>" | "+" | "-" | "*" | "/" | "%" | "==" | "!=" | ">" | "<" | ">=" | "<="
    )
}

fn source_line_indent(source: &str, offset: usize) -> usize {
    let line_start = source
        .get(..offset)
        .and_then(|prefix| prefix.rfind('\n'))
        .map_or(0, |newline| newline + 1);
    source
        .get(line_start..offset)
        .unwrap_or_default()
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .count()
}

fn parse_inline_match_arms(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return Vec::new();
    };
    let Some(close) = matching_close(tokens, open) else {
        return Vec::new();
    };
    if close <= open + 1 {
        return Vec::new();
    }
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .filter(|part| part.iter().any(|token| token == "=>"))
        .map(|part| parse_ast_expr(&part, item, expressions, source))
        .collect()
}

fn ast_call_arg(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<AstCallArg> {
    if tokens.is_empty() {
        return None;
    }
    if tokens.get(1).map(String::as_str) == Some(":") {
        let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
        return Some(AstCallArg {
            kind: AstCallArgKind::Named,
            name: tokens[0].clone(),
            value: parse_ast_expr(&tokens[2..], item, expressions, source),
            start,
            end,
        });
    }
    let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
    let name = if tokens.len() == 1 && is_name(&tokens[0]) {
        tokens[0].clone()
    } else {
        String::new()
    };
    Some(AstCallArg {
        kind: AstCallArgKind::BareBinding,
        name,
        value: parse_ast_expr(tokens, item, expressions, source),
        start,
        end,
    })
}

fn ast_function_parameters(tokens: &[String], item: &ParserItem) -> Vec<AstParameter> {
    let Some(open) = tokens.iter().position(|token| token == "(") else {
        return Vec::new();
    };
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .enumerate()
        .filter_map(|(ordinal, part)| {
            let name = part.first()?.clone();
            let kind = if part.get(1).map(String::as_str) == Some(":")
                && part.get(2).map(String::as_str) == Some("OUT")
            {
                AstParameterKind::Out
            } else {
                AstParameterKind::Value
            };
            let (start, end) = span_for_tokens(&part, item).unwrap_or((item.start, item.end));
            Some(AstParameter {
                name,
                kind,
                ordinal,
                start,
                end,
            })
        })
        .collect()
}

fn find_top_level_pipe(tokens: &[String]) -> Option<usize> {
    let mut depth = 0i32;
    let mut pipe = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == "|>" && depth == 0 {
            pipe = Some(index);
        }
    }
    pipe
}

fn find_top_level_token(tokens: &[String], needle: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == needle && depth == 0 {
            return Some(index);
        }
    }
    None
}

fn split_infix(tokens: &[String]) -> Option<(&[String], &str, &[String])> {
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            token
                if is_infix_operator(token)
                    && depth == 0
                    && index > 0
                    && index + 1 < tokens.len() =>
            {
                return Some((&tokens[..index], token, &tokens[index + 1..]));
            }
            _ => {}
        }
    }
    None
}

fn split_leading_infix(tokens: &[String]) -> Option<(&str, &[String])> {
    let (operator, right) = tokens.split_first()?;
    (!right.is_empty() && is_infix_operator(operator)).then_some((operator, right))
}

fn is_infix_operator(token: &str) -> bool {
    matches!(
        token,
        "==" | ">" | "<" | ">=" | "<=" | "!=" | "+" | "-" | "*" | "/" | "%"
    )
}

fn split_postfix_field_access(tokens: &[String]) -> Option<(&[String], String)> {
    let mut depth = 0i32;
    let mut dot = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            "." if depth == 0 && index > 0 && index + 1 < tokens.len() => dot = Some(index),
            _ => {}
        }
    }
    let dot = dot?;
    let input = &tokens[..dot];
    if !input.iter().any(|token| token == ")") {
        return None;
    }
    let field_tokens = &tokens[dot + 1..];
    if field_tokens.is_empty()
        || field_tokens.iter().any(|token| token == ".")
        || !field_tokens.iter().all(|token| is_name(token))
    {
        return None;
    }
    Some((input, field_tokens.join("")))
}

fn matching_close(tokens: &[String], open: usize) -> Option<usize> {
    let close_token = match tokens.get(open).map(String::as_str)? {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        _ => return None,
    };
    let mut stack = vec![close_token];
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        match token.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop() != Some(token.as_str()) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(tokens: &[String], separator: &str) -> Vec<Vec<String>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0i32;
    for token in tokens {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == separator && depth == 0 {
            groups.push(std::mem::take(&mut current));
        } else {
            current.push(token.clone());
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn path_segments(tokens: &[String]) -> Vec<String> {
    let mut segments = Vec::new();
    for token in tokens.iter().filter(|token| token.as_str() != ".") {
        if let Some((role, first)) = split_role_value_head(token) {
            segments.push(role.to_owned());
            segments.push(first.to_owned());
        } else if is_name(token) {
            segments.push(token.clone());
        }
    }
    segments
}

fn split_role_value_head(value: &str) -> Option<(&str, &str)> {
    let (role, first) = value.split_once('/')?;
    (is_program_role_root(role) && is_name(first)).then_some((role, first))
}

struct ParsedTextLiteral {
    value: String,
    value_start: usize,
}

fn text_literal_value(tokens: &[String], item: &ParserItem, source: &str) -> Option<String> {
    parsed_text_literal(tokens, item, source).map(|text| text.value)
}

fn parsed_text_literal(
    tokens: &[String],
    item: &ParserItem,
    source: &str,
) -> Option<ParsedTextLiteral> {
    if tokens.first().map(String::as_str) != Some("TEXT")
        || tokens.get(1).map(String::as_str) != Some("{")
    {
        return None;
    }
    if tokens == ["TEXT", "{"] {
        return text_literal_source_value_from_start(item.start, source);
    }
    let close = tokens.iter().rposition(|token| token == "}")?;
    if close + 1 != tokens.len() {
        return None;
    }
    if let Some(text) = text_literal_source_value(tokens, item, source) {
        return Some(text);
    }
    Some(ParsedTextLiteral {
        value: join_text_literal_tokens(&tokens[2..close]),
        value_start: span_for_tokens(tokens, item)
            .and_then(|(start, end)| source.get(start..end).map(|slice| (start, slice)))
            .and_then(|(start, slice)| slice.find('{').map(|open| start + open + 1))
            .unwrap_or(item.start),
    })
}

fn text_literal_source_value(
    tokens: &[String],
    item: &ParserItem,
    source: &str,
) -> Option<ParsedTextLiteral> {
    let (start, end) = span_for_tokens(tokens, item)?;
    let slice = source.get(start..end)?;
    parsed_text_literal_source_slice(slice, start)
}

fn parsed_text_literal_source_slice(slice: &str, slice_start: usize) -> Option<ParsedTextLiteral> {
    let text_start = slice.find("TEXT")?;
    let open = text_start + slice[text_start..].find('{')?;
    let content_start = open + 1;
    let mut depth = 1i32;
    for (offset, ch) in slice[content_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let raw = &slice[content_start..content_start + offset];
                    let value = raw.trim();
                    let leading = raw.len() - raw.trim_start().len();
                    return Some(ParsedTextLiteral {
                        value: value.to_owned(),
                        value_start: slice_start + content_start + leading,
                    });
                }
            }
            _ => {}
        }
    }
    None
}

fn text_literal_source_value_from_start(start: usize, source: &str) -> Option<ParsedTextLiteral> {
    let slice = source.get(start..)?;
    parsed_text_literal_source_slice(slice, start)
}

fn ast_text_template_segments(
    text: &str,
    text_start: usize,
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<Vec<AstTextSegment>> {
    let mut cursor = 0usize;
    let mut segments = Vec::new();
    while let Some(relative_open) = text[cursor..].find('{') {
        let open = cursor + relative_open;
        if open > cursor {
            segments.push(AstTextSegment::Static {
                value: text[cursor..open].to_owned(),
            });
        }
        let Some(relative_close) = text[open + 1..].find('}') else {
            let raw = text[open + 1..].trim();
            let leading = text[open + 1..].len() - text[open + 1..].trim_start().len();
            let start = text_start + open + 1 + leading;
            let value = push_ast_expr_at_source_span(
                item,
                expressions,
                AstExprKind::Unknown(vec![raw.to_owned()]),
                start,
                start + raw.len(),
                source,
            );
            segments.push(AstTextSegment::Dynamic { value });
            cursor = text.len();
            break;
        };
        let close = open + 1 + relative_close;
        let inner = &text[open + 1..close];
        let trimmed = inner.trim();
        let raw = trimmed.trim_start_matches('$');
        let parts = raw.split('.').collect::<Vec<_>>();
        let kind = if parts.len() == 1 && parts[0].contains('/') {
            let path = path_segments(&[parts[0].to_owned()]);
            if path.is_empty() {
                AstExprKind::Unknown(vec![raw.to_owned()])
            } else {
                AstExprKind::Path(path)
            }
        } else if parts.len() == 1 && is_name(parts[0]) {
            AstExprKind::Identifier(parts[0].to_owned())
        } else if !parts.is_empty() && parts.iter().all(|part| is_name(part)) {
            AstExprKind::Path(parts.into_iter().map(str::to_owned).collect())
        } else {
            AstExprKind::Unknown(vec![raw.to_owned()])
        };
        let leading = inner.len() - inner.trim_start().len();
        let dollar_prefix = trimmed.len() - raw.len();
        let start = text_start + open + 1 + leading + dollar_prefix;
        let value =
            push_ast_expr_at_source_span(item, expressions, kind, start, start + raw.len(), source);
        segments.push(AstTextSegment::Dynamic { value });
        cursor = close + 1;
    }
    if segments.is_empty() {
        return None;
    }
    if cursor < text.len() {
        segments.push(AstTextSegment::Static {
            value: text[cursor..].to_owned(),
        });
    }
    Some(segments)
}

fn push_ast_expr_at_source_span(
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    kind: AstExprKind,
    start: usize,
    end: usize,
    source: &str,
) -> usize {
    let value = push_ast_expr(item, expressions, kind, start, end);
    expressions[value].line = source
        .get(..start)
        .map(|prefix| prefix.bytes().filter(|byte| *byte == b'\n').count() + 1)
        .unwrap_or(item.line);
    value
}

fn string_literal_value(tokens: &[String]) -> Option<String> {
    if tokens.len() != 1 {
        return None;
    }
    tokens[0]
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(unescape_string_literal)
}

fn unescape_string_literal(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                output.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn join_text_literal_tokens(tokens: &[String]) -> String {
    let mut output = String::new();
    let mut previous = "";
    for token in tokens {
        if output.is_empty() {
            output.push_str(token);
        } else if text_literal_needs_space(previous, token) {
            output.push(' ');
            output.push_str(token);
        } else {
            output.push_str(token);
        }
        previous = token;
    }
    output
}

fn text_literal_needs_space(previous: &str, current: &str) -> bool {
    if matches!(
        current,
        "[" | "(" | "{" | "]" | ")" | "}" | "," | "." | ":" | ";" | "%"
    ) {
        return false;
    }
    if matches!(previous, "[" | "(" | "{" | "." | ":" | "#" | "/" | "%") {
        return false;
    }
    if previous.chars().all(|ch| ch.is_ascii_digit())
        && current
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, 'x' | 'X'))
    {
        return false;
    }
    true
}

fn value_starts_uppercase_identifier(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn ast_field_name(symbols: &[String]) -> Option<&str> {
    if symbols.get(1).map(String::as_str) != Some(":") {
        return None;
    }
    let name = symbols.first()?.as_str();
    is_name(name).then_some(name)
}

fn ast_insource_slice_event(symbols: &[String]) -> Option<&str> {
    let open = symbols.iter().position(|lexeme| lexeme == "[")?;
    let event = symbols.get(open + 1)?.as_str();
    (symbols.get(open + 2).map(String::as_str) == Some(":")
        && symbols.iter().any(|lexeme| lexeme == "SOURCE")
        && is_name(event))
    .then_some(event)
}

fn ast_hold_name(symbols: &[String]) -> Option<&str> {
    let hold = symbols.iter().position(|lexeme| lexeme == "HOLD")?;
    symbols
        .get(hold + 1)
        .map(String::as_str)
        .filter(|name| is_name(name))
}

fn ast_list_capacity(symbols: &[String]) -> Option<usize> {
    let list = symbols.iter().position(|lexeme| lexeme == "LIST")?;
    (symbols.get(list + 1).map(String::as_str) == Some("["))
        .then(|| symbols.get(list + 2))?
        .and_then(|value| value.parse().ok())
}

fn ast_list_items(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return Vec::new();
    };
    let Some(close) = matching_close(tokens, open) else {
        return Vec::new();
    };
    if close <= open + 1 {
        return Vec::new();
    }
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| parse_ast_expr(&part, item, expressions, source))
        .collect()
}

fn ast_bytes_literal(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<(BytesSizeSyntax, Vec<usize>)> {
    if tokens.first().map(String::as_str) != Some("BYTES") {
        return None;
    }
    let (size, body_open) = match tokens.get(1).map(String::as_str) {
        Some("{") => (BytesSizeSyntax::Dynamic, 1),
        Some("[") => {
            let close = matching_close(tokens, 1)?;
            let size_tokens = &tokens[2..close];
            let size = match size_tokens {
                [value] if value == "__" => BytesSizeSyntax::Infer,
                [value] => BytesSizeSyntax::Fixed(value.parse().ok()?),
                _ => return None,
            };
            if tokens.get(close + 1).map(String::as_str) != Some("{") {
                return None;
            }
            (size, close + 1)
        }
        _ => return None,
    };
    let body_close = matching_close(tokens, body_open)?;
    if body_close + 1 != tokens.len() {
        return None;
    }
    let items = if body_close <= body_open + 1 {
        Vec::new()
    } else {
        split_top_level(&tokens[body_open + 1..body_close], ",")
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(|part| parse_ast_expr(&part, item, expressions, source))
            .collect()
    };
    Some((size, items))
}

fn ast_opens_scope(symbols: &[String]) -> bool {
    matches!(
        symbols.last().map(String::as_str),
        Some(":") | Some("[") | Some("{")
    ) || symbols.windows(2).any(|window| {
        window[0] == ":" && window[1] == "[" && !symbols.iter().any(|lexeme| lexeme == "]")
    })
}

fn ast_expression_operators(symbols: &[String]) -> Vec<String> {
    let refs = symbols.iter().map(String::as_str).collect::<Vec<_>>();
    expression_operators(&refs)
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn detect_program_kind() -> ProgramKind {
    ProgramKind::Generic
}

fn validate_source_syntax(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    let example_source = path.contains("/examples/") || path.starts_with("examples/");
    let mut text_literal_spans = ast
        .expressions
        .iter()
        .filter_map(|expr| {
            matches!(expr.kind, AstExprKind::TextLiteral(_)).then_some((expr.start, expr.end))
        })
        .collect::<Vec<_>>();
    text_literal_spans.extend(text_literal_token_spans(&ast.tokens));
    for token in &ast.tokens {
        if matches!(token.kind, AstTokenKind::String | AstTokenKind::Comment)
            || text_literal_spans
                .iter()
                .any(|(start, end)| token.start >= *start && token.end <= *end)
        {
            continue;
        }
        if token.lexeme == "EXAMPLE" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`EXAMPLE` is not Boon syntax; put example identity in the manifest/dev metadata",
            ));
        }
        if token.lexeme == "#" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`#` comments are not supported in Boon source; use `--` comments",
            ));
        }
        if token.lexeme == "LINK" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`LINK` is not supported in boon-circuit examples; declare input ports with `SOURCE`",
            ));
        }
        if let Some(feature) = LANGUAGE_FEATURE_REGISTRY.iter().find(|feature| {
            feature.stage == LanguageFeatureStage::Planned
                && feature.parse_expectation == LanguageFeatureParseExpectation::Reject
                && feature.spellings.contains(&token.lexeme.as_str())
        }) {
            return Err(error(
                path,
                token.line,
                token.column,
                &format!(
                    "`{}` belongs to planned language feature `{}` and is rejected until that feature is implemented",
                    token.lexeme, feature.id
                ),
            ));
        }
        if example_source && matches!(token.lexeme.as_str(), "bg" | "fill" | "true" | "false") {
            return Err(error(
                path,
                token.line,
                token.column,
                "Boon examples must use canonical names such as `background`, `Fill`, `True`, and `False`",
            ));
        }
    }
    for item in &ast.items {
        for (index, window) in item.symbols.windows(2).enumerate() {
            if matches!(window, [pipe, op] if pipe == "|>" && op == "LINK") {
                return Err(error(
                    path,
                    item.line,
                    item.indent + 1,
                    "`|> LINK` is not supported; bind source ports through an Element constructor's `element.events` field",
                ));
            }
            if matches!(window, [pipe, op] if pipe == "|>" && op == "SOURCE") {
                return Err(error(
                    path,
                    item.line,
                    item.symbol_spans
                        .get(index + 1)
                        .map_or(item.indent + 1, |(start, _)| {
                            start.saturating_sub(item.start) + 1
                        }),
                    "`|> SOURCE { ... }` routing was removed; bind the source directly through the Element constructor's `element.events` field",
                ));
            }
            let op = window[1].as_str();
            if window[0] == "|>"
                && pipeline_operator_requires_call_parentheses(op)
                && item.symbols.get(index + 2).map(String::as_str) != Some("(")
            {
                return Err(error(
                    path,
                    item.line,
                    item.symbol_spans
                        .get(index + 1)
                        .map_or(item.indent + 1, |(start, _)| {
                            start.saturating_sub(item.start) + 1
                        }),
                    &format!("pipeline function `{op}` must be called with parentheses: `{op}()`"),
                ));
            }
        }
    }
    validate_reserved_standard_namespaces(path, &ast.statements)?;
    validate_role_qualified_value_syntax(path, &ast.tokens)?;
    validate_function_parameter_syntax(path, ast)?;
    validate_call_entry_syntax(path, ast)?;
    if example_source && let Some(document) = document_statement(ast) {
        let document_is_canonical = document.expr.is_some_and(|expr_id| {
            ast.expressions.get(expr_id).is_some_and(|expr| {
                matches!(&expr.kind, AstExprKind::Call { function, .. } if function == "Document/new")
            })
        });
        if !document_is_canonical || statement_has_field(document, "kind") {
            return Err(error(
                path,
                document.line,
                document.indent + 1,
                "example documents must use `Document/new(root: Element/...)`",
            ));
        }
    }
    validate_drain_syntax(path, ast, &text_literal_spans)?;
    validate_bytes_syntax(path, ast, &text_literal_spans)?;
    Ok(())
}

fn validate_match_patterns(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    for expression in &ast.expressions {
        let AstExprKind::MatchArm {
            pattern: AstMatchPattern::Invalid { message },
            ..
        } = &expression.kind
        else {
            continue;
        };
        let column = ast
            .tokens
            .iter()
            .find(|token| token.line == expression.line && token.start >= expression.start)
            .map_or(1, |token| token.column);
        return Err(error(path, expression.line, column, message));
    }
    Ok(())
}

fn validate_function_parameter_syntax(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    for item in ast.items.iter().filter(|item| item.function.is_some()) {
        let Some(open) = item.symbols.iter().position(|symbol| symbol == "(") else {
            continue;
        };
        let close = matching_close(&item.symbols, open).unwrap_or(item.symbols.len());
        let mut names = BTreeSet::new();
        for part in split_top_level(&item.symbols[open + 1..close], ",") {
            if part.is_empty() {
                continue;
            }
            let valid = matches!(part.as_slice(), [name] if is_name(name))
                || matches!(part.as_slice(), [name, colon, out] if is_name(name) && colon == ":" && out == "OUT");
            let name = part.first().map(String::as_str).unwrap_or_default();
            let column = item
                .symbol_spans
                .iter()
                .zip(&item.symbols)
                .find_map(|((start, _), symbol)| (symbol == name).then_some(*start))
                .map_or(item.indent + 1, |start| {
                    start.saturating_sub(item.start) + 1
                });
            if !valid {
                return Err(error(
                    path,
                    item.line,
                    column,
                    "function parameters must be `name` or `name: OUT`",
                ));
            }
            if matches!(name, "PASS" | "PASSED" | "OUT") {
                return Err(error(
                    path,
                    item.line,
                    column,
                    "`PASS`, `PASSED`, and `OUT` are reserved and cannot be function parameter names",
                ));
            }
            if !names.insert(name.to_owned()) {
                return Err(error(
                    path,
                    item.line,
                    column,
                    &format!("duplicate function parameter `{name}`"),
                ));
            }
        }
    }
    Ok(())
}

fn validate_call_entry_syntax(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    for expr in &ast.expressions {
        let (function, args, pass) = match &expr.kind {
            AstExprKind::Call {
                function,
                args,
                pass,
            } => (function.as_str(), args.as_slice(), pass.as_ref()),
            AstExprKind::Pipe { op, args, pass, .. } => {
                (op.as_str(), args.as_slice(), pass.as_ref())
            }
            _ => continue,
        };
        let mut names = BTreeSet::new();
        for arg in args {
            let column = parser_column_for_offset(ast, expr.line, arg.start);
            if arg.is_bare_binding() && arg.name.is_empty() {
                return Err(error(
                    path,
                    expr.line,
                    column,
                    &format!(
                        "bare entry in `{function}` must be one identifier naming a declared OUT parameter; ordinary arguments use `name: expression`"
                    ),
                ));
            }
            if arg.name == "PASS" {
                return Err(error(
                    path,
                    expr.line,
                    column,
                    "`PASS` may appear only once as the final call clause",
                ));
            }
            if !names.insert(arg.name.as_str()) {
                return Err(error(
                    path,
                    expr.line,
                    column,
                    &format!("duplicate call entry `{}` in `{function}`", arg.name),
                ));
            }
        }
        if let Some(pass) = pass
            && !pass.final_clause
        {
            return Err(error(
                path,
                expr.line,
                parser_column_for_offset(ast, expr.line, pass.start),
                "`PASS` must be the final call clause",
            ));
        }
    }
    Ok(())
}

fn parser_column_for_offset(ast: &AstProgram, line: usize, offset: usize) -> usize {
    ast.lines
        .iter()
        .find(|candidate| candidate.line == line)
        .map_or(1, |candidate| offset.saturating_sub(candidate.start) + 1)
}

fn pipeline_operator_requires_call_parentheses(operator: &str) -> bool {
    !matches!(
        operator,
        "HOLD" | "LATEST" | "WHEN" | "WHILE" | "THEN" | "DRAIN" | "DRAINING" | "SOURCE"
    )
}

fn validate_reserved_standard_namespaces(
    path: &str,
    statements: &[AstStatement],
) -> Result<(), ParseError> {
    for statement in statements {
        if let AstStatementKind::Field { name } = &statement.kind
            && is_reserved_standard_root(name)
        {
            return Err(error(
                path,
                statement.line,
                statement.indent + 1,
                &format!(
                    "`{name}` is a reserved Boon standard root and cannot be declared by an application"
                ),
            ));
        }
    }
    validate_reserved_standard_function_names(path, statements)
}

fn validate_reserved_standard_function_names(
    path: &str,
    statements: &[AstStatement],
) -> Result<(), ParseError> {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &statement.kind
            && name
                .split('/')
                .next()
                .is_some_and(is_reserved_standard_root)
        {
            return Err(error(
                path,
                statement.line,
                statement.indent + 1,
                &format!(
                    "`{name}` is in a reserved Boon standard namespace and cannot be declared by an application"
                ),
            ));
        }
        validate_reserved_standard_function_names(path, &statement.children)?;
    }
    Ok(())
}

fn validate_role_qualified_value_syntax(path: &str, tokens: &[AstToken]) -> Result<(), ParseError> {
    for window in tokens.windows(2) {
        let [role, separator] = window else {
            continue;
        };
        if role.kind == AstTokenKind::Identifier
            && is_program_role_root(&role.lexeme)
            && separator.lexeme == "."
        {
            return Err(error(
                path,
                role.line,
                role.column,
                &format!(
                    "qualified role values use `{}/value.field`, not `{}.value.field`",
                    role.lexeme, role.lexeme
                ),
            ));
        }
    }
    Ok(())
}

fn validate_drain_syntax(
    path: &str,
    ast: &AstProgram,
    text_literal_spans: &[(usize, usize)],
) -> Result<(), ParseError> {
    let tokens = ast
        .tokens
        .iter()
        .filter(|token| {
            if matches!(token.kind, AstTokenKind::Comment | AstTokenKind::Newline) {
                return false;
            }
            let containing_text = text_literal_spans
                .iter()
                .find(|(start, end)| token.start >= *start && token.end <= *end);
            containing_text.is_none_or(|(start, _)| token.start == *start)
        })
        .collect::<Vec<_>>();

    let mut index = 0usize;
    while let Some(token) = tokens.get(index) {
        if token.lexeme != "DRAIN" {
            index += 1;
            continue;
        }
        if tokens.get(index + 1).map(|token| token.lexeme.as_str()) != Some("{") {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` requires a `{ path }` body",
            ));
        }
        let Some(close) = matching_semantic_brace(&tokens, index + 1) else {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` is missing closing `}`",
            ));
        };
        let body = tokens[index + 2..close]
            .iter()
            .map(|token| token.lexeme.clone())
            .collect::<Vec<_>>();
        if drain_path_from_symbols(&body).is_none() {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` body must contain exactly one named binding, field path, or `PASSED` path",
            ));
        }
        index = close + 1;
    }

    let depths = semantic_token_depths(&tokens);
    for (index, token) in tokens.iter().enumerate() {
        if token.lexeme != "DRAINING" {
            continue;
        }
        if index
            .checked_sub(1)
            .and_then(|index| tokens.get(index))
            .is_none_or(|previous| previous.lexeme != "|>")
            || !draining_pipe_has_input(&tokens, index)
        {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAINING` must be used as terminal `input |> DRAINING` syntax",
            ));
        }
        if draining_has_trailing_pipeline_syntax(&tokens, &depths, index) {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAINING` must be terminal in its pipeline",
            ));
        }
    }
    Ok(())
}

fn matching_semantic_brace(tokens: &[&AstToken], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.lexeme.as_str() {
            "{" => depth += 1,
            "}" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn semantic_token_depths(tokens: &[&AstToken]) -> Vec<i32> {
    let mut depth = 0i32;
    tokens
        .iter()
        .map(|token| {
            let current = depth;
            match token.lexeme.as_str() {
                "[" | "{" | "(" => depth += 1,
                "]" | "}" | ")" => depth -= 1,
                _ => {}
            }
            current
        })
        .collect()
}

fn draining_pipe_has_input(tokens: &[&AstToken], draining: usize) -> bool {
    let Some(pipe) = draining.checked_sub(1) else {
        return false;
    };
    let Some(previous) = pipe.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return false;
    };
    !matches!(
        previous.lexeme.as_str(),
        ":" | "," | "(" | "[" | "{" | "=>" | "|>"
    )
}

fn draining_has_trailing_pipeline_syntax(
    tokens: &[&AstToken],
    depths: &[i32],
    draining: usize,
) -> bool {
    let Some(marker) = tokens.get(draining) else {
        return false;
    };
    let marker_depth = depths.get(draining).copied().unwrap_or_default();
    let Some(next) = tokens.get(draining + 1) else {
        return false;
    };
    if next.line == marker.line {
        return match next.lexeme.as_str() {
            "," => marker_depth == 0,
            ")" | "]" | "}" => false,
            _ => true,
        };
    }
    let next_depth = depths.get(draining + 1).copied().unwrap_or_default();
    if matches!(next.lexeme.as_str(), ")" | "]" | "}") && next_depth <= marker_depth {
        return false;
    }
    next.lexeme == "|>" && next_depth == marker_depth
}

fn validate_bytes_syntax(
    path: &str,
    ast: &AstProgram,
    text_literal_spans: &[(usize, usize)],
) -> Result<(), ParseError> {
    for window in ast.tokens.windows(2) {
        let [base_token, suffix_token] = window else {
            continue;
        };
        if base_token.line != suffix_token.line
            || base_token.end != suffix_token.start
            || matches!(
                base_token.kind,
                AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
            )
            || matches!(
                suffix_token.kind,
                AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
            )
            || text_literal_spans
                .iter()
                .any(|(start, end)| base_token.start >= *start && base_token.end <= *end)
            || text_literal_spans
                .iter()
                .any(|(start, end)| suffix_token.start >= *start && suffix_token.end <= *end)
        {
            continue;
        }
        if !matches!(base_token.kind, AstTokenKind::Number) || !suffix_token.lexeme.starts_with('u')
        {
            continue;
        }
        parse_byte_literal_parts(&base_token.lexeme, &suffix_token.lexeme)
            .map_err(|message| error(path, base_token.line, base_token.column, message.as_str()))?;
    }
    for item in &ast.items {
        validate_bytes_item_syntax(path, ast, item, text_literal_spans)?;
    }
    Ok(())
}

fn validate_bytes_item_syntax(
    path: &str,
    _ast: &AstProgram,
    item: &ParserItem,
    text_literal_spans: &[(usize, usize)],
) -> Result<(), ParseError> {
    for bytes_index in item
        .symbols
        .iter()
        .enumerate()
        .filter_map(|(index, symbol)| (symbol == "BYTES").then_some(index))
    {
        let token_start = item
            .symbol_spans
            .get(bytes_index)
            .map(|(start, _)| *start)
            .unwrap_or(item.start);
        if text_literal_spans
            .iter()
            .any(|(start, end)| token_start >= *start && token_start <= *end)
        {
            continue;
        }
        match item.symbols.get(bytes_index + 1).map(String::as_str) {
            Some("{") => {
                validate_bytes_body_consumption(path, item, bytes_index + 1)?;
            }
            Some("[") => {
                let Some(close) = matching_close(&item.symbols, bytes_index + 1) else {
                    return Err(error(
                        path,
                        item.line,
                        item.indent + 1,
                        "BYTES size is missing closing `]`",
                    ));
                };
                let size_tokens = &item.symbols[bytes_index + 2..close];
                let valid_size = matches!(size_tokens, [value] if value == "__" || value.parse::<usize>().is_ok());
                if !valid_size {
                    return Err(error(
                        path,
                        item.line,
                        item.indent + 1,
                        "BYTES size must be `__` or a non-negative decimal integer in v1",
                    ));
                }
                if item.symbols.get(close + 1).map(String::as_str) != Some("{") {
                    return Err(error(
                        path,
                        item.line,
                        item.indent + 1,
                        "BYTES constructor requires a `{ ... }` body",
                    ));
                }
                validate_bytes_body_consumption(path, item, close + 1)?;
            }
            _ => {
                return Err(error(
                    path,
                    item.line,
                    item.indent + 1,
                    "BYTES constructor requires a `{ ... }` body",
                ));
            }
        }
    }
    Ok(())
}

fn validate_bytes_body_consumption(
    path: &str,
    item: &ParserItem,
    body_open: usize,
) -> Result<(), ParseError> {
    let Some(body_close) = matching_close(&item.symbols, body_open) else {
        return Err(error(
            path,
            item.line,
            item.indent + 1,
            "BYTES constructor is missing closing `}`",
        ));
    };
    let Some(next) = item.symbols.get(body_close + 1) else {
        return Ok(());
    };
    if matches!(next.as_str(), "," | ")" | "]" | "}" | "|>") {
        return Ok(());
    }
    Err(error(
        path,
        item.line,
        item.indent + 1,
        format!("BYTES constructor has unexpected trailing token `{next}`").as_str(),
    ))
}

fn text_literal_token_spans(tokens: &[AstToken]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        if tokens[index].lexeme == "TEXT" && tokens[index + 1].lexeme == "{" {
            let start = tokens[index].start;
            let mut depth = 0i32;
            let mut cursor = index + 1;
            while cursor < tokens.len() {
                match tokens[cursor].lexeme.as_str() {
                    "{" => depth += 1,
                    "}" => {
                        depth -= 1;
                        if depth == 0 {
                            spans.push((start, tokens[cursor].end));
                            index = cursor;
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        index += 1;
    }
    spans
}

fn text_literal_body_line_ranges(tokens: &[AstToken]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        if tokens[index].lexeme == "TEXT" && tokens[index + 1].lexeme == "{" {
            let start_line = tokens[index].line;
            let mut depth = 0i32;
            let mut cursor = index + 1;
            while cursor < tokens.len() {
                match tokens[cursor].lexeme.as_str() {
                    "{" => depth += 1,
                    "}" => {
                        depth -= 1;
                        if depth == 0 {
                            let end_line = tokens[cursor].line;
                            if end_line > start_line {
                                ranges.push((start_line + 1, end_line));
                            }
                            index = cursor;
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        index += 1;
    }
    ranges
}

fn statement_has_field(statement: &AstStatement, needle: &str) -> bool {
    matches!(&statement.kind, AstStatementKind::Field { name } if name == needle)
        || statement
            .children
            .iter()
            .any(|child| statement_has_field(child, needle))
}

fn validate_balanced_brackets(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    let mut stack = Vec::new();
    for token in ast.tokens.iter().filter(|token| {
        !matches!(
            token.kind,
            AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
        )
    }) {
        match token.lexeme.as_str() {
            "[" | "{" | "(" => stack.push((token.lexeme.as_str(), token.line, token.column)),
            "]" if stack.pop().map(|(ch, _, _)| ch) != Some("[") => {
                return Err(error(path, token.line, token.column, "unbalanced `]`"));
            }
            "}" if stack.pop().map(|(ch, _, _)| ch) != Some("{") => {
                return Err(error(path, token.line, token.column, "unbalanced `}`"));
            }
            ")" if stack.pop().map(|(ch, _, _)| ch) != Some("(") => {
                return Err(error(path, token.line, token.column, "unbalanced `)`"));
            }
            "]" | "}" | ")" => {}
            _ => {}
        }
    }
    if stack.is_empty() {
        Ok(())
    } else {
        let (ch, line, column) = stack
            .last()
            .copied()
            .expect("stack is known to be nonempty");
        Err(ParseError {
            path: path.to_owned(),
            line: Some(line),
            column: Some(column),
            message: format!("unclosed `{ch}` at line {line}, column {column}"),
        })
    }
}

fn validate_list_capacities(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    for line in ast.semantic_parser_lines() {
        let Some(list_index) = line.symbols.iter().position(|lexeme| lexeme == "LIST") else {
            continue;
        };
        if line.symbols.get(list_index + 1).map(String::as_str) != Some("[") {
            continue;
        }
        let capacity_column = ast_token_for_parser_line_symbol(ast, line, list_index + 2)
            .map(|token| token.column)
            .unwrap_or(line.indent + 1);
        let Some(close_offset) = line.symbols[list_index + 2..]
            .iter()
            .position(|lexeme| lexeme == "]")
        else {
            return Err(error(
                path,
                line.line,
                capacity_column,
                "LIST capacity is missing closing `]`",
            ));
        };
        let capacity_parts = &line.symbols[list_index + 2..list_index + 2 + close_offset];
        if capacity_parts.len() != 1
            || capacity_parts
                .first()
                .is_none_or(|capacity| capacity.is_empty())
        {
            return Err(error(
                path,
                line.line,
                capacity_column,
                "LIST capacity must be a positive integer",
            ));
        }
        match capacity_parts[0].parse::<usize>() {
            Ok(value) if value > 0 => {}
            _ => {
                return Err(error(
                    path,
                    line.line,
                    capacity_column,
                    "LIST capacity must be a positive integer",
                ));
            }
        }
    }
    Ok(())
}

fn validate_no_reducer_style_update(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    if ast.semantic_parser_items().any(reducer_update_signature) {
        return Err(ParseError {
            path: path.to_owned(),
            line: None,
            column: None,
            message: "central reducer `FUNCTION update(state, event)` is not allowed; define local HOLD equations for each value".to_owned(),
        });
    }
    let has_event_source_when = ast
        .semantic_parser_items()
        .any(|item| item.contains_sequence(&["event", ".", "source", "|>", "WHEN"]));
    let has_state_pipe = ast
        .semantic_parser_items()
        .any(|item| item.contains_sequence(&["state", "|>"]));
    if has_event_source_when && has_state_pipe {
        return Err(ParseError {
            path: path.to_owned(),
            line: None,
            column: None,
            message: "global event-source reducer over `state` is not allowed; each value must declare its own sources".to_owned(),
        });
    }
    Ok(())
}

fn reducer_update_signature(item: &ParserItem) -> bool {
    item.function.as_deref() == Some("update")
        && item.has_lexeme("state")
        && item.has_lexeme("event")
}

fn validate_no_hidden_identity_leak(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    for token in ast.semantic_tokens() {
        if let Some(needle) = hidden_runtime_identity_token(&token.lexeme) {
            return Err(ParseError {
                path: path.to_owned(),
                line: Some(token.line),
                column: Some(token.column),
                message: format!("Boon source exposes hidden runtime identity `{needle}`"),
            });
        }
    }
    for item in ast.semantic_parser_items() {
        if item.field.as_deref() == Some("alive") {
            return Err(ParseError {
                path: path.to_owned(),
                line: Some(item.line),
                column: None,
                message: format!(
                    "Boon source exposes app-visible liveness field `alive` at line {}",
                    item.line
                ),
            });
        }
    }
    Ok(())
}

fn hidden_runtime_identity_token(value: &str) -> Option<&'static str> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("$boon") {
        return Some("$boon");
    }
    let tokens = lower
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty());
    const FORBIDDEN: &[&str] = &[
        "runtime_key",
        "item_key",
        "row_key",
        "hidden_key",
        "hidden_keys",
        "hidden_generation",
        "target_key",
        "target_generation",
        "source_id",
        "bind_epoch",
        "listkey",
        "slot",
    ];
    tokens.into_iter().find_map(|token| {
        FORBIDDEN
            .iter()
            .copied()
            .find(|forbidden| token == *forbidden)
    })
}

fn expression_operators(symbols: &[&str]) -> Vec<String> {
    let mut operators = Vec::new();
    for lexeme in symbols {
        if is_operator_lexeme(lexeme) && !operators.iter().any(|operator| operator == lexeme) {
            operators.push((*lexeme).to_owned());
        }
    }
    operators
}

fn is_operator_lexeme(lexeme: &str) -> bool {
    matches!(
        lexeme,
        "SOURCE"
            | "DRAIN"
            | "DRAINING"
            | "HOLD"
            | "THEN"
            | "WHEN"
            | "WHILE"
            | "LATEST"
            | "LIST"
            | "BLOCK"
            | "List/map"
            | "List/append"
            | "List/range"
            | "List/get"
            | "List/find"
            | "List/filter"
            | "List/move_field_first"
            | "List/move_field_last"
            | "List/chunk"
            | "List/remove"
            | "List/retain"
            | "List/count"
            | "List/length"
            | "List/sum"
            | "List/every"
            | "List/any"
            | "List/is_not_empty"
            | "List/latest"
            | "Text/empty"
            | "Text/join"
            | "Text/concat"
            | "Text/time_range_label"
            | "Text/trim"
            | "Text/to_lowercase"
            | "Text/to_uppercase"
            | "Text/slice"
            | "Text/length"
            | "Text/find"
            | "Text/contains"
            | "Text/starts_with"
            | "Text/all_chars_in"
            | "Text/to_number"
            | "Text/to_bytes"
            | "Text/is_empty"
            | "Text/is_not_empty"
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
            | "Bytes/from_hex"
            | "Bytes/to_hex"
            | "Bytes/from_base64"
            | "Bytes/to_base64"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
            | "File/read_bytes"
            | "File/read_text"
            | "File/write_bytes"
            | "Number/bit_width"
            | "Number/to_text"
            | "Number/to_codepoint_text"
            | "Number/to_ascii_text"
            | "Number/interpolate"
            | "Number/project_width"
            | "Number/project_offset"
            | "Number/project_time"
            | "Bool/not"
            | "Bool/and"
            | "Bool/toggle"
            | "Timer/interval"
            | "Router/route"
            | "Router/go_to"
            | "Ulid/generate"
            | "Light/directional"
            | "Light/ambient"
            | "Light/spot"
    )
}

fn ast_token_for_parser_line_symbol<'a>(
    ast: &'a AstProgram,
    line: &ParserLine,
    lexeme_index: usize,
) -> Option<&'a AstToken> {
    ast.semantic_tokens()
        .filter(|token| token.line == line.line)
        .nth(lexeme_index)
}

fn collect_functions(ast: &AstProgram) -> Vec<String> {
    ast.semantic_parser_items()
        .filter_map(|item| item.function.clone())
        .collect()
}

fn collect_operators(ast: &AstProgram) -> Vec<String> {
    let mut operators = Vec::new();
    for token in ast.semantic_tokens() {
        if is_operator_lexeme(&token.lexeme)
            && !operators.iter().any(|operator| operator == &token.lexeme)
        {
            operators.push(token.lexeme.clone());
        }
    }
    operators
}

fn error(path: &str, line: usize, column: usize, message: &str) -> ParseError {
    ParseError {
        path: path.to_owned(),
        line: Some(line),
        column: Some(column),
        message: format!("{message} at line {line}, column {column}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn true_and_false_use_the_canonical_tag_ast_and_match_pattern() {
        let parsed = parse_source(
            "truth-tags.bn",
            r#"
truth: True
selected:
    truth |> WHEN {
        True => False
        False => True
    }
"#,
        )
        .unwrap();

        let expression_tags = parsed
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                AstExprKind::Tag(tag) => Some(tag.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(expression_tags.contains(&"True"));
        assert!(expression_tags.contains(&"False"));

        let pattern_tags = parsed
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                AstExprKind::MatchArm {
                    pattern: AstMatchPattern::Tag { name, .. },
                    ..
                } => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(pattern_tags, ["True", "False"]);

        let artifact = serde_json::to_string(&parsed).unwrap();
        assert!(!artifact.contains("\"kind\":\"bool\""));
        assert!(!artifact.contains("\"kind\":\"enum\""));
        assert!(!artifact.contains("\"kind\":\"record\""));
    }

    #[test]
    fn match_patterns_preserve_only_the_supported_typed_surface() {
        let parsed = parse_source(
            "typed-match-patterns.bn",
            r#"
selected:
    candidate |> WHEN {
        __ => 0
        whole => whole
        42 => 1
        TEXT { exact } => 2
        Ready => 3
        Found[value] => value
        InvalidNumber[reason, position] => position
    }
"#,
        )
        .unwrap();

        let patterns = parsed
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                AstExprKind::MatchArm { pattern, .. } => Some(pattern),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            patterns,
            [
                &AstMatchPattern::Wildcard,
                &AstMatchPattern::Binding {
                    name: "whole".to_owned(),
                },
                &AstMatchPattern::Number {
                    value: "42".to_owned(),
                },
                &AstMatchPattern::Text {
                    value: "exact".to_owned(),
                },
                &AstMatchPattern::Tag {
                    name: "Ready".to_owned(),
                    fields: Vec::new(),
                },
                &AstMatchPattern::Tag {
                    name: "Found".to_owned(),
                    fields: vec!["value".to_owned()],
                },
                &AstMatchPattern::Tag {
                    name: "InvalidNumber".to_owned(),
                    fields: vec!["reason".to_owned(), "position".to_owned()],
                },
            ]
        );
        let artifact = serde_json::to_string(&parsed).unwrap();
        assert!(!artifact.contains("\"pattern\":["));
        assert!(!artifact.contains("\"kind\":\"unknown\""));
    }

    #[test]
    fn unsupported_compound_match_patterns_fail_closed_with_targeted_errors() {
        let cases = [
            ("[field: value]", "object patterns are unsupported"),
            ("LIST { value }", "LIST patterns are unsupported"),
            ("NUMBER", "runtime type patterns are unsupported"),
            ("SKIP", "private flow-control states cannot be matched"),
            ("NaN", "is not a Number or a valid match pattern"),
            (
                "Found[value: renamed]",
                "do not support renaming, nesting, or comparison",
            ),
            (
                "Outer[Inner[value]]",
                "do not support renaming, nesting, or comparison",
            ),
            ("Found[value, value]", "cannot be bound more than once"),
            ("Found[Value]", "must be a lowercase field name"),
            ("{expected}", "dynamic comparison patterns are unsupported"),
        ];

        for (pattern, expected) in cases {
            let error = parse_source(
                "invalid-match-pattern.bn",
                format!(
                    "selected:\n    candidate |> WHEN {{\n        {pattern} => 1\n        __ => 0\n    }}\n"
                ),
            )
            .unwrap_err();
            assert!(
                error.message.contains(expected),
                "pattern `{pattern}` produced `{}`",
                error.message
            );
        }
    }

    #[test]
    fn parsed_program_digest_is_order_independent_and_uses_normalized_paths() {
        let forward = parse_project(
            "app/main.bn",
            [
                ("app/main.bn".to_owned(), "main_value: 1\n".to_owned()),
                ("app/helper.bn".to_owned(), "helper_value: 2\n".to_owned()),
            ],
        )
        .unwrap();
        let reverse = parse_project(
            "app\\main.bn",
            [
                ("app\\helper.bn".to_owned(), "helper_value: 2\n".to_owned()),
                ("app\\main.bn".to_owned(), "main_value: 1\n".to_owned()),
            ],
        )
        .unwrap();

        assert_eq!(
            forward.source_bundle_digest_v1,
            reverse.source_bundle_digest_v1
        );
        assert_eq!(forward, reverse);
        assert_eq!(forward.path, "app/main.bn");
        assert_eq!(
            forward
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["app/helper.bn", "app/main.bn"]
        );
    }

    #[test]
    fn parsed_program_digest_and_parser_input_preserve_exact_source_text() {
        let without_newline = parse_source("main.bn", "value: 1").unwrap();
        let with_newline = parse_source("main.bn", "value: 1\n").unwrap();

        assert_eq!(without_newline.source, "value: 1");
        assert_eq!(without_newline.files[0].source, "value: 1");
        assert_eq!(with_newline.source, "value: 1\n");
        assert_ne!(
            without_newline.source_bundle_digest_v1,
            with_newline.source_bundle_digest_v1
        );
    }

    #[test]
    fn parse_project_rejects_missing_and_ambiguous_entrypoints() {
        let missing = parse_project(
            "missing.bn",
            [("main.bn".to_owned(), "value: 1\n".to_owned())],
        )
        .unwrap_err();
        assert_eq!(missing.path, "missing.bn");
        assert!(missing.message.contains("entrypoint `missing.bn`"));
        assert!(missing.message.contains("not one of its units"));

        let ambiguous = parse_project(
            "app/main.bn",
            [
                ("app/main.bn".to_owned(), "value: 1\n".to_owned()),
                ("app\\main.bn".to_owned(), "value: 2\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert_eq!(ambiguous.path, "app/main.bn");
        assert!(
            ambiguous
                .message
                .contains("duplicate normalized path `app/main.bn`")
        );
    }

    #[test]
    fn parsed_program_serialization_includes_unforgeable_provenance_fields() {
        let parsed = parse_source("app/main.bn", "value: 1\n").unwrap();
        let serialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(
            serialized["source_bundle_digest_v1"],
            parsed.source_bundle_digest_v1.to_string()
        );
        assert_eq!(serialized["path"], "app/main.bn");

        let fields: ParsedProgramFields = serde_json::from_value(serialized).unwrap();
        assert_eq!(
            fields.source_bundle_digest_v1,
            parsed.source_bundle_digest_v1
        );
        assert_eq!(fields.path, parsed.path);
    }

    #[test]
    fn multiline_selectors_retain_owned_match_arms() {
        let parsed = parse_ast(
            "structured-selectors.bn",
            r#"
when_value:
    selector |> WHEN {
        Ready =>
            selector.value
            |> Number/abs()
        fallback => BLOCK {
            copied: fallback
            copied
        }
    }

while_value:
    selector |> WHILE {
        Ready => selector.value
        fallback => fallback
    }
"#,
        )
        .unwrap();

        let when = parsed
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, AstExprKind::When { .. }))
            .expect("WHEN expression");
        let AstExprKind::When { arms, .. } = &when.kind else {
            unreachable!();
        };
        assert_eq!(arms.len(), 2);
        assert!(arms.iter().all(|arm| matches!(
            parsed.expressions[*arm].kind,
            AstExprKind::MatchArm {
                output: Some(_),
                ..
            }
        )));

        let ready_output = arms
            .iter()
            .find_map(|arm| match &parsed.expressions[*arm].kind {
                AstExprKind::MatchArm {
                    pattern: AstMatchPattern::Tag { name, .. },
                    output: Some(output),
                } if name == "Ready" => Some(*output),
                _ => None,
            });
        let ready_output = ready_output.expect("Ready output");
        let ready = &parsed.expressions[ready_output];
        assert!(matches!(&ready.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs"));
        let linked_input = ready.linked_input.expect("multiline pipeline input");
        assert!(matches!(
            &parsed.expressions[linked_input].kind,
            AstExprKind::Path(parts) if parts == &["selector", "value"]
        ));

        let fallback_output = arms
            .iter()
            .find_map(|arm| match &parsed.expressions[*arm].kind {
                AstExprKind::MatchArm {
                    pattern: AstMatchPattern::Binding { name },
                    output: Some(output),
                } if name == "fallback" => Some(*output),
                _ => None,
            })
            .expect("fallback output");
        let AstExprKind::Block { bindings, result } = &parsed.expressions[fallback_output].kind
        else {
            panic!("fallback output must remain a BLOCK");
        };
        assert_eq!(
            bindings
                .iter()
                .map(|binding| binding.name.as_str())
                .collect::<Vec<_>>(),
            ["copied"]
        );
        assert!(matches!(
            parsed.expressions[result.expect("fallback BLOCK result")].kind,
            AstExprKind::Identifier(ref name) if name == "copied"
        ));

        let while_arms = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::Pipe { op, arms, .. } if op == "WHILE" => Some(arms),
                _ => None,
            });
        assert_eq!(while_arms.expect("WHILE arms").len(), 2);
    }

    #[test]
    fn expression_block_retains_bindings_result_and_multiline_children() {
        let parsed = parse_ast(
            "structured-block.bn",
            r#"
FUNCTION calculate(input) {
    BLOCK {
        answer: normalized
        normalized:
            input
            |> Number/abs()
        answer
    }
}

FUNCTION row(input) {
    [
        value: input
    ]
}

rows: LIST {
    [value: 1]
    [value: 2]
}
"#,
        )
        .unwrap();

        let block = parsed
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, AstExprKind::Block { .. }))
            .expect("BLOCK expression");
        let AstExprKind::Block { bindings, result } = &block.kind else {
            unreachable!();
        };
        assert_eq!(
            bindings
                .iter()
                .map(|binding| binding.name.as_str())
                .collect::<Vec<_>>(),
            ["answer", "normalized"]
        );
        assert!(bindings.iter().all(|binding| binding.statement > 0));
        assert!(matches!(
            parsed.expressions[result.expect("BLOCK result")].kind,
            AstExprKind::Identifier(ref name) if name == "answer"
        ));
        let normalized = bindings
            .iter()
            .find(|binding| binding.name == "normalized")
            .expect("normalized binding");
        assert!(matches!(
            parsed.expressions[normalized.value].kind,
            AstExprKind::Pipe { ref op, .. } if op == "Number/abs"
        ));
        assert!(parsed.expressions[normalized.value].linked_input.is_some());

        let record = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::Object(fields) if fields.iter().any(|field| field.name == "value") => {
                    Some(fields)
                }
                _ => None,
            })
            .expect("multiline record fields");
        assert_eq!(record.len(), 1);
        let list_items = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::ListLiteral { items, .. } if items.len() == 2 => Some(items),
                _ => None,
            });
        assert_eq!(list_items.expect("multiline list items").len(), 2);
    }

    #[test]
    fn hanging_indented_pipeline_retains_exact_input_chain() {
        let parsed = parse_ast(
            "hanging-indented-pipeline.bn",
            r#"
value:
    input
        |> Number/abs()
        |> Number/floor()
"#,
        )
        .unwrap();

        let input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "input")
            })
            .expect("pipeline source");
        let abs = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs")
            })
            .expect("first continuation");
        let floor = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/floor")
            })
            .expect("second continuation");

        assert_eq!(abs.linked_input, Some(input.id));
        assert_eq!(floor.linked_input, Some(abs.id));
    }

    #[test]
    fn named_leading_minus_is_unary_not_a_sibling_continuation() {
        let parsed = parse_ast(
            "named-negative.bn",
            r#"
first: 10
second: -5 / 2 |> Number/round(to: 1, using: NearestEven)
third: 1 |> Number/round(to: -0.1, using: TowardZero)
"#,
        )
        .unwrap();

        let second = parsed
            .statements
            .iter()
            .find(|statement| {
                matches!(&statement.kind, AstStatementKind::Field { name } if name == "second")
            })
            .and_then(|statement| statement.expr)
            .expect("second field expression");
        let AstExprKind::Pipe { input, .. } = &parsed.expressions[second].kind else {
            panic!("second field must retain its rounding call");
        };
        let AstExprKind::Infix { left, op, right: _ } = &parsed.expressions[*input].kind else {
            panic!("second field must retain unary negation");
        };
        assert_eq!(op, "-");
        assert_eq!(parsed.expressions[*input].linked_input, None);
        assert!(matches!(
            &parsed.expressions[*left].kind,
            AstExprKind::Number(value) if value == "0"
        ));

        let third = parsed
            .statements
            .iter()
            .find(|statement| {
                matches!(&statement.kind, AstStatementKind::Field { name } if name == "third")
            })
            .and_then(|statement| statement.expr)
            .expect("third field expression");
        let AstExprKind::Pipe { args, .. } = &parsed.expressions[third].kind else {
            panic!("third field must retain its rounding call");
        };
        let quantum = args
            .iter()
            .find(|argument| argument.name == "to")
            .map(|argument| argument.value)
            .expect("rounding quantum");
        assert_eq!(parsed.expressions[quantum].linked_input, None);
        match &parsed.expressions[quantum].kind {
            AstExprKind::Number(value) => assert_eq!(value, "-0.1"),
            AstExprKind::Infix { left, op, right: _ } => {
                assert_eq!(op, "-");
                assert!(matches!(
                    &parsed.expressions[*left].kind,
                    AstExprKind::Number(value) if value == "0"
                ));
            }
            _ => panic!("negative call argument must retain unary negation"),
        }
    }

    #[test]
    fn one_line_continuation_links_the_innermost_placeholder_operator() {
        let parsed = parse_ast(
            "one-line-continuation.bn",
            r#"
value:
    input
    |> Number/abs() |> Number/floor()
"#,
        )
        .unwrap();

        let input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "input")
            })
            .expect("pipeline source");
        let abs = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs")
            })
            .expect("placeholder-bearing operator");
        let floor = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/floor")
            })
            .expect("outer operator");

        assert_eq!(abs.linked_input, Some(input.id));
        assert_eq!(floor.linked_input, None);
        assert!(matches!(
            floor.kind,
            AstExprKind::Pipe { input, .. } if input == abs.id
        ));
    }

    #[test]
    fn every_special_pipeline_continuation_receives_its_exact_input() {
        let parsed = parse_ast(
            "special-continuations.bn",
            r#"
when_value:
    when_input
    |> WHEN {
        True => 1
        False => 0
    }

then_value:
    then_input
    |> THEN { 1 }

hold_value:
    hold_input
    |> HOLD held { LATEST {} }

draining_value:
    draining_input
    |> DRAINING

while_value:
    while_input
    |> WHILE {
        True => 1
        False => 0
    }
"#,
        )
        .unwrap();

        let expected = [
            ("when_input", "WHEN"),
            ("then_input", "THEN"),
            ("hold_input", "HOLD"),
            ("draining_input", "DRAINING"),
            ("while_input", "WHILE"),
        ];
        for (input_name, operator) in expected {
            let expression = parsed
                .expressions
                .iter()
                .find(|expression| match (&expression.kind, operator) {
                    (AstExprKind::When { .. }, "WHEN")
                    | (AstExprKind::Then { .. }, "THEN")
                    | (AstExprKind::Hold { .. }, "HOLD")
                    | (AstExprKind::Draining { .. }, "DRAINING") => true,
                    (AstExprKind::Pipe { op, .. }, "WHILE") => op == "WHILE",
                    _ => false,
                })
                .unwrap_or_else(|| panic!("{operator} continuation"));
            let linked = expression
                .linked_input
                .unwrap_or_else(|| panic!("{operator} linked input"));
            assert!(matches!(
                &parsed.expressions[linked].kind,
                AstExprKind::Identifier(name) if name == input_name
            ));
        }
    }

    #[test]
    fn inline_match_arm_and_then_outputs_extend_to_the_final_continuation() {
        let parsed = parse_ast(
            "inline-output-continuations.bn",
            r#"
matched:
    selector |> WHEN {
        Ready => match_input
            |> Number/abs()
        __ => 0
    }

triggered:
    trigger |> THEN { then_input }
        |> Number/floor()
"#,
        )
        .unwrap();

        let abs = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs")
            })
            .expect("match-arm continuation");
        let match_input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "match_input")
            })
            .expect("match-arm input");
        assert_eq!(abs.linked_input, Some(match_input.id));
        assert!(parsed.expressions.iter().any(|expression| matches!(
            expression.kind,
            AstExprKind::MatchArm { output: Some(output), .. } if output == abs.id
        )));

        let floor = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/floor")
            })
            .expect("THEN output continuation");
        let then_input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "then_input")
            })
            .expect("THEN output input");
        assert_eq!(floor.linked_input, Some(then_input.id));
        assert!(parsed.expressions.iter().any(|expression| matches!(
            expression.kind,
            AstExprKind::Then { output: Some(output), .. } if output == floor.id
        )));
    }

    #[test]
    fn orphan_pipeline_continuation_is_rejected_by_the_parser() {
        let error = parse_ast(
            "orphan-continuation.bn",
            r#"
value: |> Number/abs()
"#,
        )
        .expect_err("orphan continuation must fail");

        assert!(
            error
                .message
                .contains("pipeline continuation has no preceding value")
        );
    }

    #[test]
    fn every_cells_pipeline_placeholder_is_parser_linked() {
        let files = [
            ("cell.bn", include_str!("../../../examples/cells/cell.bn")),
            (
                "columns.bn",
                include_str!("../../../examples/cells/columns.bn"),
            ),
            (
                "defaults.bn",
                include_str!("../../../examples/cells/defaults.bn"),
            ),
            (
                "formula.bn",
                include_str!("../../../examples/cells/formula.bn"),
            ),
            ("model.bn", include_str!("../../../examples/cells/model.bn")),
            ("store.bn", include_str!("../../../examples/cells/store.bn")),
            ("view.bn", include_str!("../../../examples/cells/view.bn")),
        ];
        let mut placeholder_count = 0usize;
        for (path, source) in files {
            let parsed = parse_ast(path, source).unwrap_or_else(|error| panic!("{path}: {error}"));
            for expression in &parsed.expressions {
                let input = match &expression.kind {
                    AstExprKind::Pipe { input, .. }
                    | AstExprKind::Then { input, .. }
                    | AstExprKind::When { input, .. }
                    | AstExprKind::Draining { input }
                    | AstExprKind::Hold { initial: input, .. } => *input,
                    _ => continue,
                };
                if parsed
                    .expressions
                    .get(input)
                    .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
                {
                    placeholder_count += 1;
                    assert!(
                        expression.linked_input.is_some(),
                        "{path}: expression {}",
                        expression.id
                    );
                }
            }
        }
        assert!(placeholder_count > 0);
    }

    #[test]
    fn grouped_infix_expression_preserves_the_inner_operation() {
        let parsed = parse_source(
            "grouped-infix.bn",
            r#"
store: [
    value: (input.value + 1) * 2
]
"#,
        )
        .unwrap();
        let multiply = parsed
            .expressions
            .iter()
            .find(|expr| matches!(&expr.kind, AstExprKind::Infix { op, .. } if op == "*"))
            .expect("outer multiplication is parsed");
        let AstExprKind::Infix { left, .. } = multiply.kind else {
            unreachable!();
        };
        assert!(matches!(
            parsed.expressions[left].kind,
            AstExprKind::Infix { ref op, .. } if op == "+"
        ));
    }

    #[test]
    fn comparison_pipeline_feeds_when_with_the_comparison_result() {
        let parsed = parse_source(
            "comparison-when.bn",
            r#"
store: [
    left: [value: 1]
    right: [value: 1]
    result:
        left
        == right
        |> WHEN {
            True => TEXT { equal }
            False => TEXT { different }
        }
]
"#,
        )
        .unwrap();
        let when = parsed
            .expressions
            .iter()
            .find(|expr| matches!(expr.kind, AstExprKind::When { .. }))
            .expect("WHEN expression");
        let AstExprKind::When { input, .. } = when.kind else {
            unreachable!();
        };
        let input = when.linked_input.unwrap_or(input);
        assert!(
            matches!(
                parsed.expressions[input].kind,
                AstExprKind::Infix { ref op, .. } if op == "=="
            ),
            "WHEN input: {:#?}",
            parsed.expressions[input]
        );
    }

    #[test]
    fn nested_grouped_expression_preserves_every_operation() {
        let parsed = parse_source(
            "nested-grouped-infix.bn",
            r#"
store: [
    value: ((input.value + 1) * 2)
]
"#,
        )
        .unwrap();
        let multiply = parsed
            .expressions
            .iter()
            .find(|expr| matches!(&expr.kind, AstExprKind::Infix { op, .. } if op == "*"))
            .expect("outer groups are unwrapped");
        let AstExprKind::Infix { left, .. } = multiply.kind else {
            unreachable!();
        };
        assert!(matches!(
            parsed.expressions[left].kind,
            AstExprKind::Infix { ref op, .. } if op == "+"
        ));
    }

    #[test]
    fn text_interpolation_creates_structured_children_with_exact_spans() {
        let source = r#"
store: [value: 7]
label: TEXT {
    prefix
    {store.value}
    suffix
}
"#;
        let parsed = parse_source("text-interpolation-span.bn", source).unwrap();
        let dynamic = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::TextTemplate { segments } => {
                    segments.iter().find_map(|segment| match segment {
                        AstTextSegment::Static { .. } => None,
                        AstTextSegment::Dynamic { value } => Some(*value),
                    })
                }
                _ => None,
            })
            .expect("structured text interpolation child");
        let dynamic = &parsed.expressions[dynamic];

        assert_eq!(&source[dynamic.start..dynamic.end], "store.value");
        assert_eq!(dynamic.line, 5);
        assert!(matches!(
            &dynamic.kind,
            AstExprKind::Path(path) if path == &["store", "value"]
        ));
    }

    #[test]
    fn multiline_list_map_retains_bare_binding_and_named_call() {
        let parsed = parse_source(
            "multiline-dynamic-map.bn",
            r#"
store: [
    input_rows:
        True |> WHEN {
            True => LIST { [id: TEXT { a }] }
            False => LIST {}
        }
    mapped_rows:
        input_rows
        |> List/map(row, new:
            new_row(row: row)
        )
]

FUNCTION new_row(row) {
    [
        select: SOURCE
        id: row.id
    ]
}
"#,
        )
        .unwrap();
        let args = parsed
            .expressions
            .iter()
            .find_map(|expr| match &expr.kind {
                AstExprKind::Pipe { op, args, .. } if op == "List/map" => Some(args),
                _ => None,
            })
            .expect("multiline List/map arguments");
        let row_arg = args
            .iter()
            .find(|arg| arg.is_bare_binding())
            .expect("bare row binding");
        assert_eq!(row_arg.name, "row");
        assert!(matches!(
            parsed.expressions[row_arg.value].kind,
            AstExprKind::Identifier(ref name) if name == "row"
        ));
        let new_arg = args
            .iter()
            .find(|arg| arg.named_name() == Some("new"))
            .expect("multiline new argument is normalized in the AST");
        assert!(matches!(
            parsed.expressions[new_arg.value].kind,
            AstExprKind::Call { ref function, .. } if function == "new_row"
        ));
    }

    #[test]
    fn multiline_then_uses_the_final_pipeline_expression_as_its_output() {
        let parsed = parse_source(
            "multiline-then-pipeline.bn",
            r#"
store: [
    submit: SOURCE
    normalized:
        submit |> THEN {
            submit.text
                |> Text/trim()
                |> Text/to_uppercase()
        }
]
"#,
        )
        .unwrap();
        let output = parsed
            .expressions
            .iter()
            .find_map(|expr| match &expr.kind {
                AstExprKind::Then {
                    output: Some(output),
                    ..
                } => Some(*output),
                _ => None,
            })
            .expect("multiline THEN has an output");
        assert!(matches!(
            parsed.expressions[output].kind,
            AstExprKind::Pipe { ref op, .. } if op == "Text/to_uppercase"
        ));
    }

    #[test]
    fn multiline_when_arm_uses_the_final_pipeline_expression_as_its_output() {
        let parsed = parse_source(
            "multiline-when-arm-pipeline.bn",
            r#"
store: [
    status:
        result |> WHEN {
            Ready =>
                result.name
                |> Text/concat(with: TEXT { ready }, separator: " ")
            __ => TEXT { pending }
        }
]
"#,
        )
        .unwrap();
        let concat = parsed
            .expressions
            .iter()
            .find(|expr| matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "Text/concat"))
            .expect("concat continuation exists");
        assert!(parsed.expressions.iter().any(|expr| {
            matches!(expr.kind, AstExprKind::MatchArm { output: Some(output), .. } if output == concat.id)
        }));
    }

    #[test]
    fn multiline_when_arm_call_pipeline_uses_the_final_operator_as_its_output() {
        let parsed = parse_source(
            "multiline-when-arm-call-pipeline.bn",
            r#"
store: [
    rows: LIST { [id: TEXT { one }, values: LIST {}] }
    selected:
        result |> WHEN {
            Ready =>
                rows
                |> List/find(item, if: item.id == TEXT { one })
                |> WHEN {
                    Found[value] => value.values
                    NotFound => LIST {}
                }
                |> List/map(item, new: item)
            __ => LIST {}
        }
]
"#,
        )
        .unwrap();
        let map = parsed
            .expressions
            .iter()
            .find(|expr| matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "List/map"))
            .expect("map continuation exists");
        let ready_output = parsed.expressions.iter().find_map(|expr| match &expr.kind {
            AstExprKind::MatchArm {
                pattern: AstMatchPattern::Tag { name, .. },
                output: Some(output),
            } if name == "Ready" => Some(*output),
            _ => None,
        });
        assert_eq!(ready_output, Some(map.id));
    }

    #[test]
    fn list_literal_ast_preserves_scalar_record_empty_and_capacity_forms() {
        let parsed = parse_source(
            "scalar-list-value.bn",
            r#"
store: [
    selected: TEXT { alpha }
    selected_ids: LIST { selected }
    optional_selected_ids:
        True |> WHEN {
            True => LIST {}
            False => LIST { selected }
        }
    rows: LIST { [id: selected] }
    optional_rows:
        True |> WHEN {
            True => LIST {}
            False => LIST { [id: selected] }
    }
    empty_rows: LIST {}
    bounded_rows: LIST[4] {}
]
"#,
        )
        .unwrap();
        let lists = parsed
            .expressions
            .iter()
            .filter_map(|expr| match &expr.kind {
                AstExprKind::ListLiteral { capacity, items } => Some((*capacity, items.as_slice())),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(lists.iter().any(|(_, items)| {
            items.len() == 1
                && matches!(
                    parsed.expressions[items[0]].kind,
                    AstExprKind::Identifier(ref name) if name == "selected"
                )
        }));
        assert!(lists.iter().any(|(_, items)| {
            items.len() == 1
                && matches!(
                    &parsed.expressions[items[0]].kind,
                    AstExprKind::Object(fields)
                        if fields.iter().any(|field| field.name == "id")
                )
        }));
        assert!(
            lists
                .iter()
                .any(|(capacity, items)| capacity.is_none() && items.is_empty())
        );
        fn contains_bounded_rows(statements: &[AstStatement]) -> bool {
            statements.iter().any(|statement| {
                matches!(
                    &statement.kind,
                    AstStatementKind::List {
                        field: Some(field),
                        capacity: Some(4),
                    } if field == "bounded_rows"
                ) || contains_bounded_rows(&statement.children)
            })
        }
        assert!(contains_bounded_rows(&parsed.ast.statements));
    }

    #[test]
    fn function_parameters_and_call_entries_preserve_out_and_pass_structure() {
        let parsed = parse_source(
            "out-structure.bn",
            r#"
FUNCTION map(list, item: OUT, new) {
    new
}

items: LIST { [value: 1] }
mapped:
    map(
        list: items
        item
        new: item.value
        PASS: [theme: theme]
    )
"#,
        )
        .unwrap();

        let parameters = parsed
            .ast
            .statements
            .iter()
            .find_map(|statement| match &statement.kind {
                AstStatementKind::Function { name, parameters } if name == "map" => {
                    Some(parameters)
                }
                _ => None,
            })
            .expect("map parameters");
        assert_eq!(
            parameters
                .iter()
                .map(|parameter| (parameter.name.as_str(), parameter.kind))
                .collect::<Vec<_>>(),
            vec![
                ("list", AstParameterKind::Value),
                ("item", AstParameterKind::Out),
                ("new", AstParameterKind::Value),
            ]
        );

        let (args, pass) = parsed
            .expressions
            .iter()
            .find_map(|expr| match &expr.kind {
                AstExprKind::Call {
                    function,
                    args,
                    pass,
                } if function == "map" => Some((args, pass.as_ref())),
                _ => None,
            })
            .expect("map call");
        assert_eq!(args.len(), 3);
        assert_eq!(args[0].named_name(), Some("list"));
        assert!(args[1].is_bare_binding());
        assert_eq!(args[1].name, "item");
        assert_eq!(args[2].named_name(), Some("new"));
        assert!(pass.is_some());
    }

    #[test]
    fn pass_must_be_unique_and_final() {
        for source in [
            "value: render(PASS: [theme: theme], value: 1)",
            "value: render(value: 1, PASS: [theme: theme], PASS: [theme: other])",
        ] {
            let error = parse_source("invalid-pass.bn", source).unwrap_err();
            assert!(error.message.contains("`PASS`"), "{error}");
        }
        parse_source(
            "valid-pass.bn",
            "value: render(value: 1, PASS: [theme: theme])",
        )
        .unwrap();
        parse_source(
            "valid-multiline-pass.bn",
            "value:\n    render(\n        value: 1\n        PASS: [theme: theme]\n    )\n",
        )
        .unwrap();
        let error = parse_source(
            "invalid-multiline-pass.bn",
            "value:\n    render(\n        PASS: [theme: theme]\n        value: 1\n    )\n",
        )
        .unwrap_err();
        assert!(error.message.contains("final call clause"), "{error}");
    }

    #[test]
    fn ordinary_positional_call_entries_are_rejected() {
        let error = parse_source("positional.bn", "value: render(1)").unwrap_err();
        assert!(error.message.contains("ordinary arguments use"), "{error}");
    }

    #[test]
    fn function_parameter_kinds_are_exact() {
        let error =
            parse_source("invalid-parameter.bn", "FUNCTION map(item: EACH) { item }").unwrap_err();
        assert!(error.message.contains("`name` or `name: OUT`"), "{error}");
    }

    #[test]
    fn application_code_cannot_shadow_canonical_standard_roots() {
        let field_error = parse_source("app.bn", "SessionInfo: 1\n").unwrap_err();
        assert!(field_error.message.contains("reserved Boon standard root"));

        let function_error =
            parse_source("app.bn", "FUNCTION List/custom(value) { value }\n").unwrap_err();
        assert!(
            function_error
                .message
                .contains("reserved Boon standard namespace")
        );

        let projection_error =
            parse_source("app.bn", "FUNCTION Field/custom(value) { value }\n").unwrap_err();
        assert!(
            projection_error
                .message
                .contains("reserved Boon standard namespace")
        );

        let module_error = parse_project(
            "RUN.bn",
            [
                ("RUN.bn".to_owned(), "store: [ready: True]\n".to_owned()),
                ("File.bn".to_owned(), "value: 1\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert!(
            module_error
                .message
                .contains("reserved Boon standard namespace")
        );

        for role in ["Client", "Session", "Server"] {
            let source = format!("{role}: 1\n");
            let error = parse_source("app.bn", source).unwrap_err();
            assert!(error.message.contains("reserved Boon standard root"));
        }
    }

    #[test]
    fn canonical_standard_root_registry_is_sorted_unique_and_role_complete() {
        for roots in STANDARD_ROOTS.windows(2) {
            assert!(
                roots[0].name < roots[1].name,
                "standard root `{}` must sort before unique root `{}`",
                roots[0].name,
                roots[1].name
            );
        }
        for role in ProgramRoleRoot::ALL {
            assert_eq!(program_role_root(role.namespace()), Some(role));
            assert_eq!(
                standard_root_kind(role.namespace()),
                Some(StandardRootKind::ProgramRole)
            );
        }
        for root in STANDARD_ROOTS
            .iter()
            .filter(|root| root.kind == StandardRootKind::ProgramRole)
        {
            assert!(
                program_role_root(root.name).is_some(),
                "program-role root `{}` is missing from ProgramRoleRoot",
                root.name
            );
        }
    }

    #[test]
    fn canonical_language_feature_registry_is_sorted_unique_and_honest() {
        assert!(!LANGUAGE_FEATURE_REGISTRY.is_empty());
        for features in LANGUAGE_FEATURE_REGISTRY.windows(2) {
            assert!(
                features[0].id < features[1].id,
                "language feature `{}` must sort before unique feature `{}`",
                features[0].id,
                features[1].id
            );
        }
        for feature in LANGUAGE_FEATURE_REGISTRY {
            assert!(
                !feature.id.is_empty()
                    && feature
                        .id
                        .chars()
                        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'),
                "invalid language feature id `{}`",
                feature.id
            );
            assert!(
                !feature.spellings.is_empty(),
                "{} has no spellings",
                feature.id
            );
            assert!(!feature.summary.is_empty(), "{} has no summary", feature.id);
            assert_eq!(language_feature(feature.id), Some(feature));
            if feature.stage == LanguageFeatureStage::Current {
                assert_eq!(
                    feature.parse_expectation,
                    LanguageFeatureParseExpectation::Accept,
                    "current feature `{}` cannot claim parser rejection",
                    feature.id
                );
            }
        }
        assert_eq!(language_feature("not_a_language_feature"), None);
    }

    #[test]
    fn planned_unimplemented_spellings_fail_closed_outside_comments_and_text() {
        for feature in LANGUAGE_FEATURE_REGISTRY.iter().filter(|feature| {
            feature.stage == LanguageFeatureStage::Planned
                && feature.parse_expectation == LanguageFeatureParseExpectation::Reject
        }) {
            for spelling in feature.spellings {
                let source = format!("value: {spelling} {{}}\n");
                let error = parse_source("planned-language-surface.bn", source).unwrap_err();
                assert!(
                    error.message.contains(feature.id),
                    "unexpected `{spelling}` error for {}: {error}",
                    feature.id
                );
            }
        }

        parse_source(
            "planned-words-in-text.bn",
            "-- BITS FLUSH MAP SET WHERE remain ordinary comment text\n\
             note: TEXT { BITS FLUSH MAP SET WHERE }\n",
        )
        .unwrap();
    }

    #[test]
    fn obsolete_dotted_role_qualification_is_rejected_for_every_role_root() {
        for role in ProgramRoleRoot::ALL {
            let namespace = role.namespace();
            let source = format!("value: {namespace}.foo\n");
            let error = parse_source("obsolete-role-path.bn", source).unwrap_err();
            assert!(
                error
                    .message
                    .contains(&format!("use `{namespace}/value.field`")),
                "unexpected {namespace} diagnostic: {error}"
            );
        }
    }
}
