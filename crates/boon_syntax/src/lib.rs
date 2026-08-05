use boon_contract::{SourceBundleDigestV1, SourceBundleError, normalize_source_path};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

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

/// Canonical syntax-owned registry for the public language-surface coverage
/// contract. Entries are sorted by `id`.
///
/// `Planned + Accept` means only that the current grammar accepts the fixture;
/// it does not claim the planned semantics exist. `Planned + Reject` reserves a
/// fail-closed spelling until the implementing phase lands atomically.
pub const LANGUAGE_FEATURE_REGISTRY: &[LanguageFeatureSpec] = &[
    LanguageFeatureSpec {
        id: "bits_fixed_width_literals",
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["BITS"],
        summary: "fixed-width BITS[N] literals, exact patterns, transforms, arithmetic, and byte boundaries",
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
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["MAP"],
        summary: "typed authoritative MAP literals with delimiter-safe entry arrows",
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
        stage: LanguageFeatureStage::Current,
        parse_expectation: LanguageFeatureParseExpectation::Accept,
        spellings: &["SET"],
        summary: "typed authoritative SET literals with canonical key-safe items",
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

/// Public read-only schema projected by an opaque parser-produced program.
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
    pub expressions: SharedAstExpressions,
    pub functions: Vec<String>,
    pub operators: Vec<String>,
    #[serde(skip)]
    pub occurrence_routes: Vec<Option<StableOccurrenceRoute>>,
    #[serde(skip)]
    pub expression_route_digests_v1: Vec<Option<[u8; 32]>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedSourceFile {
    pub path: String,
    pub source: String,
    pub start_line: usize,
    pub module: Option<String>,
}

/// Stable, project-scoped identity of one canonical source unit.
///
/// Identity follows the normalized project-relative path, not source content
/// or the unit's position in a canonical bundle. An edit therefore preserves
/// identity, insertion of an earlier-sorting unit cannot renumber existing
/// units, and a rename deliberately creates a different identity. The owning
/// compiler project supplies the outer project identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SourceUnitId(String);

impl SourceUnitId {
    pub fn from_path(path: &str) -> Result<Self, SourceBundleError> {
        normalize_source_path(path).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for SourceUnitId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Collision-checked project-local namespace for unit-local syntax arena ids.
///
/// The namespace is a lookup aid inside one `ProjectSyntaxSnapshot`
/// and is never a cross-revision compiler identity. Stable compiler keys use
/// [`SourceUnitId`] plus parser-owned structural routes instead. Keeping the
/// namespace in the parser artifact lets a project borrow unchanged unit
/// arenas directly without cloning and rebasing every node into one global
/// `ParsedProgram` arena.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SyntaxUnitNamespace(u16);

impl SyntaxUnitNamespace {
    /// The packed syntax-id layout reserves twelve namespace bits and nineteen
    /// unit-local bits below a tag bit. Namespace zero is deliberately
    /// reserved so malformed/default ids fail closed.
    pub const MAX: u16 = (1 << 12) - 1;

    #[doc(hidden)]
    pub fn __parser_new(value: u16) -> Option<Self> {
        (value > 0 && value <= Self::MAX).then_some(Self(value))
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

const PACKED_SYNTAX_LOCAL_BITS: u32 = 19;
const PACKED_SYNTAX_LOCAL_MASK: usize = (1usize << PACKED_SYNTAX_LOCAL_BITS) - 1;
// Keep the representation explicitly 32-bit: checked syntax references use
// `u32`, and a host-width tag would be truncated on 64-bit machines.
const PACKED_SYNTAX_TAG: usize = 1usize << 31;

/// Maximum number of expression or statement nodes in one unit-native syntax
/// arena. This is a representation bound, not a language collection bound.
pub const MAX_UNIT_SYNTAX_NODES: usize = PACKED_SYNTAX_LOCAL_MASK + 1;

/// Pack one unit-local arena index into a project-readable syntax lookup id.
///
/// This function is parser infrastructure. The packed value must not enter a
/// stable definition, occurrence, semantic, persistence, or artifact key.
#[doc(hidden)]
pub fn __parser_pack_syntax_node_id(namespace: SyntaxUnitNamespace, local: usize) -> Option<usize> {
    if local >= MAX_UNIT_SYNTAX_NODES {
        return None;
    }
    Some(PACKED_SYNTAX_TAG | (usize::from(namespace.get()) << PACKED_SYNTAX_LOCAL_BITS) | local)
}

/// Decode a unit-native syntax lookup id.
///
/// Untagged ids belong to the legacy assembled parser artifact and return
/// `None`; production project snapshots never mix the two representations.
#[doc(hidden)]
pub fn __parser_unpack_syntax_node_id(id: usize) -> Option<(SyntaxUnitNamespace, usize)> {
    if id > u32::MAX as usize || id & PACKED_SYNTAX_TAG == 0 {
        return None;
    }
    let namespace =
        ((id >> PACKED_SYNTAX_LOCAL_BITS) & usize::from(SyntaxUnitNamespace::MAX)) as u16;
    let namespace = SyntaxUnitNamespace::__parser_new(namespace)?;
    Some((namespace, id & PACKED_SYNTAX_LOCAL_MASK))
}

/// Body-insensitive kind used by parser-owned stable item routes.
///
/// This is deliberately smaller than [`AstStatementKind`]. A route identifies
/// an authored item across revisions; expression bodies, byte offsets, lines,
/// and revision-local dense statement ids are not part of that identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitItemKind {
    Function,
    Field,
    Source,
    Hold,
    List,
}

/// One structural segment in a parser-owned item route.
///
/// `names` contains the authored header names relevant to the statement kind.
/// `matching_sibling_ordinal` counts only preceding siblings with the same kind
/// and names, so inserting an unrelated sibling does not rekey the item.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableItemRouteSegment {
    pub kind: UnitItemKind,
    pub names: Vec<String>,
    pub matching_sibling_ordinal: usize,
}

/// Structural, body-insensitive route of one authored item inside a source
/// unit. The surrounding [`SourceUnitId`] supplies project-local unit identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct StableItemRoute(Vec<StableItemRouteSegment>);

impl StableItemRoute {
    #[doc(hidden)]
    pub fn __parser_from_segments(segments: Vec<StableItemRouteSegment>) -> Self {
        Self(segments)
    }

    pub fn segments(&self) -> &[StableItemRouteSegment] {
        &self.0
    }
}

/// Body-insensitive function parameter header retained by the unit item index.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnitItemParameter {
    pub name: String,
    pub kind: AstParameterKind,
    pub ordinal: usize,
}

/// One parser-owned item header. `local_statement_id` is an intra-artifact
/// lookup aid only; cross-revision consumers use `route`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnitItemIndexEntry {
    pub route: StableItemRoute,
    pub local_statement_id: usize,
    pub kind: UnitItemKind,
    pub names: Vec<String>,
    pub parameters: Vec<UnitItemParameter>,
}

impl UnitItemIndexEntry {
    pub fn is_definition(&self) -> bool {
        self.kind == UnitItemKind::Function
    }
}

/// Body-insensitive item headers emitted once with a parsed source unit.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct UnitItemIndex {
    pub entries: Vec<UnitItemIndexEntry>,
}

impl UnitItemIndex {
    pub fn owners(&self) -> impl Iterator<Item = &UnitItemIndexEntry> {
        self.entries.iter()
    }

    pub fn definitions(&self) -> impl Iterator<Item = &UnitItemIndexEntry> {
        self.entries.iter().filter(|entry| entry.is_definition())
    }
}

/// Stable compiler identity of one authored executable owner.
///
/// Fields, sources, holds, lists, and functions are all owners. This keeps
/// top-level document state and list initializers out of a monolithic program
/// root and gives persistent checking/lowering requests a body-insensitive key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableOwnerKey {
    pub source_unit_id: SourceUnitId,
    pub item_route: StableItemRoute,
}

/// Stable compiler identity of one independently checked syntax owner.
///
/// Every authored item is an owner, including fields, sources, holds, lists,
/// and functions. Each source unit also has one anonymous root owner for
/// top-level expression/block structure and file-scoped diagnostics. The unit
/// root is deliberately explicit rather than overloading a missing item route.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum StableCheckOwnerKey {
    UnitRoot(SourceUnitId),
    Item(StableOwnerKey),
}

impl StableCheckOwnerKey {
    pub fn source_unit_id(&self) -> &SourceUnitId {
        match self {
            Self::UnitRoot(source_unit_id) => source_unit_id,
            Self::Item(owner) => &owner.source_unit_id,
        }
    }
}

/// Unit-local route used by the parser's compact owner partition.
///
/// This route becomes stable only when paired with the surrounding
/// [`SourceUnitId`]. It never contains a dense statement/expression id.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum UnitOwnerRoute {
    UnitRoot,
    Item(StableItemRoute),
}

/// Typed unit-local statement lookup id retained by the parser owner index.
///
/// This is an intra-snapshot locator only. It must not enter a stable compiler,
/// semantic, persistence, or artifact identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct UnitLocalStatementId(u32);

impl UnitLocalStatementId {
    #[doc(hidden)]
    pub fn __parser_new(value: usize) -> Option<Self> {
        u32::try_from(value).ok().map(Self)
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Typed unit-local expression lookup id retained by the parser owner index.
///
/// Like [`UnitLocalStatementId`], this is revision-local lookup metadata, not
/// semantic identity. Stable expression identity is [`StableExpressionKey`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct UnitLocalExpressionId(u32);

impl UnitLocalExpressionId {
    #[doc(hidden)]
    pub fn __parser_new(value: usize) -> Option<Self> {
        u32::try_from(value).ok().map(Self)
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Compact owner slot used only inside one [`UnitOwnerIndex`].
///
/// Zero means that an arena expression is not reachable from a statement,
/// one means the anonymous unit root, and values above one identify an item
/// entry. The representation stays private so callers cannot mistake it for a
/// stable owner identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnitCheckOwnerSlot(u32);

impl UnitCheckOwnerSlot {
    #[doc(hidden)]
    pub const fn __parser_unrouted() -> Self {
        Self(0)
    }

    #[doc(hidden)]
    pub const fn __parser_unit_root() -> Self {
        Self(1)
    }

    #[doc(hidden)]
    pub fn __parser_item(item_index: usize) -> Option<Self> {
        u32::try_from(item_index)
            .ok()
            .and_then(|item| item.checked_add(2))
            .map(Self)
    }

    pub const fn is_routed(self) -> bool {
        self.0 != 0
    }

    pub const fn is_unit_root(self) -> bool {
        self.0 == 1
    }

    pub const fn item_index(self) -> Option<usize> {
        if self.0 >= 2 {
            Some((self.0 - 2) as usize)
        } else {
            None
        }
    }

    pub const fn owner_entry_index(self) -> Option<usize> {
        if self.is_unit_root() {
            Some(0)
        } else if let Some(item) = self.item_index() {
            Some(item + 1)
        } else {
            None
        }
    }
}

/// Direct nested-statement locator for one unit-local statement id.
///
/// Parent links avoid allocating an index path for every statement. Resolving
/// one locator walks only that statement's lexical depth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnitStatementLocator {
    parent: Option<UnitLocalStatementId>,
    child_index: u32,
}

/// Exact placement of a descendant owner boundary inside its parent owner.
///
/// The child item statement is not part of the parent's body. `parent` and
/// `child_index` identify where its public interface is observed among the
/// parent's retained statement tree without copying an AST path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitChildOwnerBoundary {
    pub route: StableItemRoute,
    parent: Option<UnitLocalStatementId>,
    child_index: u32,
}

impl UnitChildOwnerBoundary {
    #[doc(hidden)]
    pub fn __parser_new(
        route: StableItemRoute,
        parent: Option<UnitLocalStatementId>,
        child_index: usize,
    ) -> Option<Self> {
        Some(Self {
            route,
            parent,
            child_index: u32::try_from(child_index).ok()?,
        })
    }

    pub const fn parent(&self) -> Option<UnitLocalStatementId> {
        self.parent
    }

    pub const fn child_index(&self) -> usize {
        self.child_index as usize
    }
}

impl UnitStatementLocator {
    #[doc(hidden)]
    pub fn __parser_new(parent: Option<UnitLocalStatementId>, child_index: usize) -> Option<Self> {
        Some(Self {
            parent,
            child_index: u32::try_from(child_index).ok()?,
        })
    }

    pub const fn parent(self) -> Option<UnitLocalStatementId> {
        self.parent
    }

    pub const fn child_index(self) -> usize {
        self.child_index as usize
    }
}

/// One parser-owned syntax partition for an independently checked owner.
///
/// Statement order is preorder within the owner and excludes descendant owner
/// bodies. Expression order follows the parser arena and likewise excludes
/// descendant owners. `child_owners` records the exact boundaries where a
/// checker must consume another owner's public interface instead of descending
/// into its body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitOwnerIndexEntry {
    pub route: UnitOwnerRoute,
    pub statements: Box<[UnitLocalStatementId]>,
    pub expressions: Box<[UnitLocalExpressionId]>,
    pub child_owners: Box<[UnitChildOwnerBoundary]>,
}

/// Complete parser-owned owner partition for one source unit.
///
/// Entry zero is always [`UnitOwnerRoute::UnitRoot`]. Every later entry maps
/// one-for-one to the same-position entry in [`UnitItemIndex`]. Compact slot
/// columns allow constant-time owner lookup without cloning stable routes per
/// expression or rescanning the nested AST.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnitOwnerIndex {
    entries: Box<[UnitOwnerIndexEntry]>,
    statement_locators: Box<[UnitStatementLocator]>,
    statement_routes: Box<[StableStatementRoute]>,
    statement_owners: Box<[UnitCheckOwnerSlot]>,
    expression_owners: Box<[UnitCheckOwnerSlot]>,
    expression_parents: Box<[Option<UnitLocalExpressionId>]>,
}

impl UnitOwnerIndex {
    #[doc(hidden)]
    pub fn __parser_new(
        entries: Vec<UnitOwnerIndexEntry>,
        statement_locators: Vec<UnitStatementLocator>,
        statement_routes: Vec<StableStatementRoute>,
        statement_owners: Vec<UnitCheckOwnerSlot>,
        expression_owners: Vec<UnitCheckOwnerSlot>,
        expression_parents: Vec<Option<UnitLocalExpressionId>>,
    ) -> Self {
        Self {
            entries: entries.into_boxed_slice(),
            statement_locators: statement_locators.into_boxed_slice(),
            statement_routes: statement_routes.into_boxed_slice(),
            statement_owners: statement_owners.into_boxed_slice(),
            expression_owners: expression_owners.into_boxed_slice(),
            expression_parents: expression_parents.into_boxed_slice(),
        }
    }

    pub fn entries(&self) -> &[UnitOwnerIndexEntry] {
        &self.entries
    }

    pub fn entry(&self, route: &UnitOwnerRoute) -> Option<&UnitOwnerIndexEntry> {
        match route {
            UnitOwnerRoute::UnitRoot => self.entries.first(),
            UnitOwnerRoute::Item(route) => self
                .entries
                .iter()
                .skip(1)
                .find(|entry| {
                    matches!(&entry.route, UnitOwnerRoute::Item(candidate) if candidate == route)
                }),
        }
    }

    pub fn statement_count(&self) -> usize {
        self.statement_locators.len()
    }

    pub fn expression_count(&self) -> usize {
        self.expression_owners.len()
    }

    pub fn statement_locator(
        &self,
        statement: UnitLocalStatementId,
    ) -> Option<UnitStatementLocator> {
        self.statement_locators.get(statement.as_usize()).copied()
    }

    pub fn statement_route(
        &self,
        statement: UnitLocalStatementId,
    ) -> Option<&StableStatementRoute> {
        self.statement_routes.get(statement.as_usize())
    }

    pub fn statement_owner(&self, statement: UnitLocalStatementId) -> Option<UnitCheckOwnerSlot> {
        self.statement_owners
            .get(statement.as_usize())
            .copied()
            .filter(|owner| owner.is_routed())
    }

    pub fn expression_owner(
        &self,
        expression: UnitLocalExpressionId,
    ) -> Option<UnitCheckOwnerSlot> {
        self.expression_owners
            .get(expression.as_usize())
            .copied()
            .filter(|owner| owner.is_routed())
    }

    pub fn expression_parent(
        &self,
        expression: UnitLocalExpressionId,
    ) -> Option<UnitLocalExpressionId> {
        self.expression_parents
            .get(expression.as_usize())
            .copied()
            .flatten()
    }
}

/// Stable compiler identity of one authored definition.
///
/// This function-only key remains distinct from [`StableOwnerKey`]: callers
/// that accept a definition must not accidentally accept an arbitrary authored
/// state/list owner.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableDefinitionKey {
    pub source_unit_id: SourceUnitId,
    pub item_route: StableItemRoute,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StableStatementKind {
    Function,
    Field,
    Source,
    Hold,
    List,
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableStatementRouteSegment {
    pub kind: StableStatementKind,
    pub names: Vec<String>,
    pub matching_sibling_reverse_ordinal: usize,
}

/// Parser-owned structural location of one statement inside its nearest
/// stable authored item. Item-root statements use an empty `statement_route`;
/// the item owner itself supplies their identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableStatementRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StableItemRoute>,
    pub statement_route: Vec<StableStatementRouteSegment>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableStatementKey {
    pub source_unit_id: SourceUnitId,
    pub route: StableStatementRoute,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StableExpressionChildRole {
    TextDynamic,
    RecordField,
    FlushPayload,
    CallArgument,
    CallPass,
    PipeInput,
    PipeArgument,
    PipePass,
    PipeArm,
    DrainingInput,
    HoldInitial,
    LatestBranch,
    WhenInput,
    WhenArm,
    ThenInput,
    ThenOutput,
    InfixLeft,
    InfixRight,
    MatchOutput,
    BlockBinding,
    BlockResult,
    CollectionItem,
    ArrowLeft,
    ArrowOutput,
    MapKey,
    MapValue,
    MapEntry,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableExpressionRouteSegment {
    pub role: StableExpressionChildRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub matching_sibling_reverse_ordinal: usize,
}

/// Parser-owned structural location of one call/pipe occurrence inside its
/// nearest stable item owner. No source text, offsets, lines, or dense ids are
/// part of this route.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableOccurrenceRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StableItemRoute>,
    pub statement_route: Vec<StableStatementRouteSegment>,
    pub expression_route: Vec<StableExpressionRouteSegment>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableOccurrenceKey {
    pub source_unit_id: SourceUnitId,
    pub route: StableOccurrenceRoute,
}

/// Stable compiler identity for one syntax expression across source revisions.
///
/// The digest commits only the parser-owned structural route: authored item
/// owner, statement route, and expression parent/child roles. Source text,
/// byte/line positions, packed syntax ids, and checked dense slots are not
/// part of this identity. The parser collision-checks every digest inside its
/// source unit before publishing the opaque artifact.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StableExpressionKey {
    pub source_unit_id: SourceUnitId,
    pub route_digest_v1: [u8; 32],
}

/// Public read-only schema projected by an opaque parser-produced source unit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ParsedSourceUnitFields {
    pub source_unit_id: SourceUnitId,
    pub path: String,
    pub source: String,
    pub ast: AstProgram,
    pub declared_functions: Vec<String>,
    pub item_index: UnitItemIndex,
    #[serde(skip)]
    pub owner_index: UnitOwnerIndex,
    #[serde(skip)]
    pub occurrence_routes: Vec<Option<StableOccurrenceRoute>>,
    #[serde(skip)]
    pub expression_route_digests_v1: Vec<Option<[u8; 32]>>,
}

impl ParsedSourceFile {
    pub fn source_unit_id(&self) -> Result<SourceUnitId, SourceBundleError> {
        SourceUnitId::from_path(&self.path)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstProgram {
    pub tokens: Vec<AstToken>,
    pub lines: Vec<ParserLine>,
    pub items: Vec<ParserItem>,
    pub statements: Vec<AstStatement>,
    pub expressions: SharedAstExpressions,
}

/// Immutable, cheaply cloned ownership for a parsed expression arena.
///
/// Parser-produced programs use one shared allocation for the convenience
/// `expressions` field and `ast.expressions`. The wrapper retains slice indexing
/// and iteration while preventing either public view from mutating an arena
/// that may be shared by another view.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SharedAstExpressions {
    expressions: Arc<Vec<AstExpr>>,
}

impl SharedAstExpressions {
    pub fn as_slice(&self) -> &[AstExpr] {
        &self.expressions
    }

    /// Parser-internal copy-on-write access used while assembling an accepted
    /// artifact. This is not part of the supported syntax DTO surface.
    #[doc(hidden)]
    pub fn __parser_make_mut(&mut self) -> &mut [AstExpr] {
        Arc::make_mut(&mut self.expressions).as_mut_slice()
    }
}

impl From<Vec<AstExpr>> for SharedAstExpressions {
    fn from(expressions: Vec<AstExpr>) -> Self {
        Self {
            expressions: Arc::new(expressions),
        }
    }
}

impl std::ops::Deref for SharedAstExpressions {
    type Target = [AstExpr];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl AsRef<[AstExpr]> for SharedAstExpressions {
    fn as_ref(&self) -> &[AstExpr] {
        self.as_slice()
    }
}

impl<'a> IntoIterator for &'a SharedAstExpressions {
    type Item = &'a AstExpr;
    type IntoIter = std::slice::Iter<'a, AstExpr>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for SharedAstExpressions {
    type Item = AstExpr;
    type IntoIter = std::vec::IntoIter<AstExpr>;

    fn into_iter(self) -> Self::IntoIter {
        Arc::try_unwrap(self.expressions)
            .unwrap_or_else(|shared| shared.as_ref().clone())
            .into_iter()
    }
}

impl Serialize for SharedAstExpressions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_slice().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SharedAstExpressions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<AstExpr>::deserialize(deserializer).map(Self::from)
    }
}

impl AstProgram {
    pub fn semantic_tokens(&self) -> impl Iterator<Item = &AstToken> {
        let document_lines = self.document_line_mask();
        self.tokens.iter().filter(move |token| {
            !matches!(token.kind, AstTokenKind::Comment | AstTokenKind::String)
                && !document_lines.get(token.line).copied().unwrap_or(false)
        })
    }

    pub fn semantic_parser_lines(&self) -> impl Iterator<Item = &ParserLine> {
        let document_lines = self.document_line_mask();
        self.lines.iter().filter(move |line| {
            !line.symbols.is_empty() && !document_lines.get(line.line).copied().unwrap_or(false)
        })
    }

    pub fn semantic_parser_items(&self) -> impl Iterator<Item = &ParserItem> {
        let document_lines = self.document_line_mask();
        self.items
            .iter()
            .filter(move |item| !document_lines.get(item.line).copied().unwrap_or(false))
    }

    fn document_line_mask(&self) -> Vec<bool> {
        fn mark(statement: &AstStatement, lines: &mut [bool]) {
            if let Some(line) = lines.get_mut(statement.line) {
                *line = true;
            }
            for child in &statement.children {
                mark(child, lines);
            }
        }

        let Some(document) = self.statements.iter().find(|statement| {
            matches!(
                &statement.kind,
                AstStatementKind::Field { name } if name == "document"
            )
        }) else {
            return Vec::new();
        };
        let maximum_line = self
            .tokens
            .last()
            .map(|token| token.line)
            .unwrap_or(document.line)
            .max(document.line);
        let mut lines = vec![false; maximum_line.saturating_add(1)];
        mark(document, &mut lines);
        lines
    }
}

/// Immutable symbols shared by the logical-line and parsed-item syntax views.
///
/// Most physical lines become one parser item without changing their symbol
/// sequence. Sharing that sequence avoids cloning every token string merely
/// to retain both read-only views. Multiline normalization uses copy-on-write
/// mutation, so only the lines it actually joins allocate a distinct vector.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SharedParserSymbols(Arc<Vec<String>>);

impl SharedParserSymbols {
    pub fn new(symbols: Vec<String>) -> Self {
        Self(Arc::new(symbols))
    }

    pub fn ptr_eq(left: &Self, right: &Self) -> bool {
        Arc::ptr_eq(&left.0, &right.0)
    }

    pub fn into_owned(self) -> Vec<String> {
        Arc::unwrap_or_clone(self.0)
    }
}

impl std::ops::Deref for SharedParserSymbols {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for SharedParserSymbols {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

impl AsRef<[String]> for SharedParserSymbols {
    fn as_ref(&self) -> &[String] {
        self.0.as_slice()
    }
}

impl From<Vec<String>> for SharedParserSymbols {
    fn from(symbols: Vec<String>) -> Self {
        Self::new(symbols)
    }
}

impl FromIterator<String> for SharedParserSymbols {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

impl IntoIterator for SharedParserSymbols {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_owned().into_iter()
    }
}

impl<'a> IntoIterator for &'a SharedParserSymbols {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl PartialEq<Vec<String>> for SharedParserSymbols {
    fn eq(&self, other: &Vec<String>) -> bool {
        self.as_ref() == other.as_slice()
    }
}

impl Serialize for SharedParserSymbols {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_ref().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SharedParserSymbols {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<String>::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParserLine {
    pub line: usize,
    pub indent: usize,
    pub symbols: SharedParserSymbols,
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
    pub symbols: SharedParserSymbols,
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
    Bits {
        width: u32,
        radix: u32,
        digits: String,
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
    /// Delimiter-safe intermediate for `left => right`.
    ///
    /// Structural linking resolves this into either a selector `MatchArm` or a
    /// `MapEntry`; an unconsumed arrow is rejected before a parsed program can
    /// be produced.
    Arrow {
        left: usize,
        pattern: AstMatchPattern,
        output: Option<usize>,
    },
    MapEntry {
        key: usize,
        value: usize,
    },
    MapLiteral {
        #[serde(default)]
        entries: Vec<usize>,
    },
    SetLiteral {
        #[serde(default)]
        items: Vec<usize>,
    },
    BitsLiteral {
        width: u32,
        radix: u32,
        digits: String,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
