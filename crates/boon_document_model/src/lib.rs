use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::ops::Range;

mod map;

pub use map::*;

pub const SENSITIVE_INPUT_STYLE_KEY: &str = "sensitive";
pub const SENSITIVE_INPUT_REDACTED_VALUE: &str = "redacted";
pub const SENSITIVE_INPUT_REDACTED_GLYPHS: &str = "••••••••";

macro_rules! string_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            pub struct $name(pub String);
        )+
    };
}

string_ids!(
    DocumentNodeId,
    SourceBindingId,
    ScrollRootId,
    TextInputId,
    MapOverlayId,
    MapHitIdentity,
    MapTileSourceId,
);

macro_rules! route_usize_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub usize);

            impl $name {
                pub const fn as_usize(self) -> usize {
                    self.0
                }
            }
        )+
    };
}

route_usize_ids!(ListId, PlanStaticOwnerId, SourceId);

impl PlanStaticOwnerId {
    /// The structural owner for executable work outside every repeated scope.
    pub const ROOT: Self = Self(usize::MAX);

    pub const fn is_root(self) -> bool {
        self.0 == Self::ROOT.0
    }
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct OwnerInstanceRow {
    pub list: ListId,
    pub key: u64,
    pub generation: u64,
}

impl Debug for OwnerInstanceRow {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("OwnerInstanceRow(..)")
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct OwnerInstanceRoute {
    pub static_owner: PlanStaticOwnerId,
    pub ancestors: Vec<OwnerInstanceRow>,
}

impl Debug for OwnerInstanceRoute {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("OwnerInstanceRoute(..)")
    }
}

impl OwnerInstanceRoute {
    pub fn new(
        static_owner: PlanStaticOwnerId,
        ancestors: impl IntoIterator<Item = OwnerInstanceRow>,
    ) -> Result<Self, &'static str> {
        let ancestors = ancestors.into_iter().collect::<Vec<_>>();
        let owner = Self {
            static_owner,
            ancestors,
        };
        owner.validate()?;
        Ok(owner)
    }

    pub fn root() -> Self {
        Self {
            static_owner: PlanStaticOwnerId::ROOT,
            ancestors: Vec::new(),
        }
    }

    pub fn leaf(&self) -> Option<OwnerInstanceRow> {
        self.ancestors.last().copied()
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.static_owner.is_root() && !self.ancestors.is_empty() {
            return Err("root owner instance cannot have ancestor rows");
        }
        if self
            .ancestors
            .iter()
            .any(|ancestor| ancestor.generation == 0)
        {
            return Err("owner instance row generations must be positive");
        }
        Ok(())
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SourceRouteToken {
    pub program_revision: u64,
    pub owner: OwnerInstanceRoute,
    pub source: SourceId,
    /// Zero denotes a non-row route; a row route repeats its leaf generation.
    pub row_generation: u64,
    pub binding_epoch: u64,
}

impl Debug for SourceRouteToken {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("SourceRouteToken(..)")
    }
}

impl SourceRouteToken {
    pub fn new(
        program_revision: u64,
        owner: OwnerInstanceRoute,
        source: SourceId,
        binding_epoch: u64,
    ) -> Result<Self, &'static str> {
        let row_generation = owner.leaf().map_or(0, |row| row.generation);
        let route = Self {
            program_revision,
            owner,
            source,
            row_generation,
            binding_epoch,
        };
        route.validate()?;
        Ok(route)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.program_revision == 0 {
            return Err("source route program revision must be positive");
        }
        if self.binding_epoch == 0 {
            return Err("source route binding epoch must be positive");
        }
        self.owner.validate()?;
        let expected_generation = self.owner.leaf().map_or(0, |row| row.generation);
        if self.row_generation != expected_generation {
            return Err("source route row generation does not match its owner");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentNodeKind {
    Root,
    Stack,
    Row,
    Text,
    Button,
    Checkbox,
    TextInput,
    EmbeddedProgram,
    EmbeddedMedia,
    MapViewport,
    Table,
    TableCell,
    ScrollRoot,
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramRole {
    #[default]
    Client,
    Session,
    Server,
}

impl ProgramRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Session => "session",
            Self::Server => "server",
        }
    }

    pub const fn namespace(self) -> &'static str {
        match self {
            Self::Client => "Client",
            Self::Session => "Session",
            Self::Server => "Server",
        }
    }

    /// Distributed application data may cross only one adjacent island edge.
    /// Same-role references stay unqualified and Client never reaches Server
    /// directly in either direction.
    pub const fn can_depend_on(self, producer: Self) -> bool {
        matches!(
            (self, producer),
            (Self::Client, Self::Session)
                | (Self::Session, Self::Client)
                | (Self::Session, Self::Server)
                | (Self::Server, Self::Session)
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramCapabilityProfile {
    #[default]
    PublicClient,
    TrustedSession,
    TrustedServer,
}

impl ProgramCapabilityProfile {
    pub fn name(self) -> &'static str {
        match self {
            Self::PublicClient => "public_client",
            Self::TrustedSession => "trusted_session",
            Self::TrustedServer => "trusted_server",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramArtifactRetention {
    #[default]
    Ephemeral,
    Replaceable,
    Archive,
}

impl ProgramArtifactRetention {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::Replaceable => "replaceable",
            Self::Archive => "archive",
        }
    }
}

pub const EMBEDDED_PROGRAM_ENTRY_PATH: &str = "RUN.bn";
pub const EMBEDDED_PROGRAM_ARTIFACT_FORMAT: &str = "boon.embedded-program-artifact.v1";

#[derive(Clone, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedProgramSourceUnit {
    pub path: String,
    #[serde(default)]
    pub source: String,
}

impl Debug for EmbeddedProgramSourceUnit {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbeddedProgramSourceUnit")
            .field("path", &self.path)
            .field("source_bytes", &self.source.len())
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct EmbeddedProgramDescriptor {
    pub source: String,
    pub revision: u64,
    pub artifact_id: String,
    pub artifact_retention: ProgramArtifactRetention,
    pub support_sources: Vec<EmbeddedProgramSourceUnit>,
    pub bootstrap_source: String,
    pub bootstrap_artifact_id: String,
    pub bootstrap_support_sources: Vec<EmbeddedProgramSourceUnit>,
    pub bootstrap_revision: u64,
    pub role: ProgramRole,
    pub capability_profile: ProgramCapabilityProfile,
    pub session_key: String,
    pub mount: bool,
}

impl Default for EmbeddedProgramDescriptor {
    fn default() -> Self {
        Self {
            source: String::new(),
            revision: 0,
            artifact_id: String::new(),
            artifact_retention: ProgramArtifactRetention::default(),
            support_sources: Vec::new(),
            bootstrap_source: String::new(),
            bootstrap_artifact_id: String::new(),
            bootstrap_support_sources: Vec::new(),
            bootstrap_revision: 0,
            role: ProgramRole::Client,
            capability_profile: ProgramCapabilityProfile::default(),
            session_key: String::new(),
            mount: true,
        }
    }
}

impl Debug for EmbeddedProgramDescriptor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let source_bundle_digest_v1 = self.source_bundle_digest_v1().ok().flatten();
        let bootstrap_source_bundle_digest_v1 =
            self.bootstrap_source_bundle_digest_v1().ok().flatten();
        formatter
            .debug_struct("EmbeddedProgramDescriptor")
            .field("source_bundle_digest_v1", &source_bundle_digest_v1)
            .field("source_bytes", &self.source.len())
            .field("revision", &self.revision)
            .field("artifact_id", &self.artifact_id)
            .field("artifact_retention", &self.artifact_retention)
            .field("support_sources", &self.support_sources)
            .field(
                "bootstrap_source_bundle_digest_v1",
                &bootstrap_source_bundle_digest_v1,
            )
            .field("bootstrap_source_bytes", &self.bootstrap_source.len())
            .field("bootstrap_artifact_id", &self.bootstrap_artifact_id)
            .field("bootstrap_support_sources", &self.bootstrap_support_sources)
            .field("bootstrap_revision", &self.bootstrap_revision)
            .field("role", &self.role)
            .field("capability_profile", &self.capability_profile)
            .field("session_key", &self.session_key)
            .field("mount", &self.mount)
            .finish()
    }
}

impl EmbeddedProgramDescriptor {
    pub fn canonical_source_bundle_v1(
        &self,
    ) -> Result<Option<boon_contract::CanonicalSourceBundleV1<'_>>, boon_contract::SourceBundleError>
    {
        canonical_embedded_program_source_bundle_v1(&self.source, &self.support_sources)
    }

    pub fn canonical_bootstrap_source_bundle_v1(
        &self,
    ) -> Result<Option<boon_contract::CanonicalSourceBundleV1<'_>>, boon_contract::SourceBundleError>
    {
        canonical_embedded_program_source_bundle_v1(
            &self.bootstrap_source,
            &self.bootstrap_support_sources,
        )
    }

    pub fn source_bundle_digest_v1(
        &self,
    ) -> Result<Option<boon_contract::SourceBundleDigestV1>, boon_contract::SourceBundleError> {
        self.canonical_source_bundle_v1()
            .map(|bundle| bundle.map(|bundle| bundle.digest()))
    }

    pub fn bootstrap_source_bundle_digest_v1(
        &self,
    ) -> Result<Option<boon_contract::SourceBundleDigestV1>, boon_contract::SourceBundleError> {
        self.canonical_bootstrap_source_bundle_v1()
            .map(|bundle| bundle.map(|bundle| bundle.digest()))
    }
}

fn canonical_embedded_program_source_bundle_v1<'a>(
    source: &'a str,
    support_sources: &'a [EmbeddedProgramSourceUnit],
) -> Result<Option<boon_contract::CanonicalSourceBundleV1<'a>>, boon_contract::SourceBundleError> {
    if source.is_empty() {
        return Ok(None);
    }
    boon_contract::CanonicalSourceBundleV1::new(
        EMBEDDED_PROGRAM_ENTRY_PATH,
        std::iter::once(boon_contract::SourceBundleUnit::new(
            EMBEDDED_PROGRAM_ENTRY_PATH,
            source,
        ))
        .chain(
            support_sources
                .iter()
                .map(|unit| boon_contract::SourceBundleUnit::new(&unit.path, &unit.source)),
        ),
    )
    .map(Some)
}

#[derive(Serialize)]
struct EmbeddedProgramArtifactV1<'a> {
    format: &'static str,
    current: EmbeddedProgramRevisionArtifactV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    bootstrap: Option<EmbeddedProgramRevisionArtifactV1>,
    session_key: &'a str,
    mount: bool,
}

#[derive(Serialize)]
struct EmbeddedProgramRevisionArtifactV1 {
    revision: u64,
    role: ProgramRole,
    capability_profile: ProgramCapabilityProfile,
    artifact_retention: ProgramArtifactRetention,
    payload: EmbeddedProgramPayloadArtifactV1,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum EmbeddedProgramPayloadArtifactV1 {
    Source {
        source_bundle_digest_v1: boon_contract::SourceBundleDigestV1,
        entrypoint: String,
        units: Vec<EmbeddedProgramUnitArtifactV1>,
    },
    ContentArtifact {
        content_artifact_id: String,
    },
    Missing,
    Invalid {
        error: String,
    },
}

#[derive(Serialize)]
struct EmbeddedProgramUnitArtifactV1 {
    path: String,
    source_bytes: usize,
}

fn embedded_program_payload_artifact_v1(
    source: &str,
    artifact_id: &str,
    support_sources: &[EmbeddedProgramSourceUnit],
) -> EmbeddedProgramPayloadArtifactV1 {
    let has_source = !source.is_empty();
    let has_artifact = !artifact_id.trim().is_empty();
    if has_source && has_artifact {
        return EmbeddedProgramPayloadArtifactV1::Invalid {
            error: "source and content artifact identity are mutually exclusive".to_owned(),
        };
    }
    if !support_sources.is_empty() && !has_source {
        return EmbeddedProgramPayloadArtifactV1::Invalid {
            error: "support sources require a source payload".to_owned(),
        };
    }
    if has_source {
        return match canonical_embedded_program_source_bundle_v1(source, support_sources) {
            Ok(Some(bundle)) => EmbeddedProgramPayloadArtifactV1::Source {
                source_bundle_digest_v1: bundle.digest(),
                entrypoint: bundle.entrypoint().to_owned(),
                units: bundle
                    .units()
                    .iter()
                    .map(|unit| EmbeddedProgramUnitArtifactV1 {
                        path: unit.path().to_owned(),
                        source_bytes: unit.source().len(),
                    })
                    .collect(),
            },
            Ok(None) => EmbeddedProgramPayloadArtifactV1::Missing,
            Err(error) => EmbeddedProgramPayloadArtifactV1::Invalid {
                error: bounded_embedded_program_error(error.to_string()),
            },
        };
    }
    if has_artifact {
        return EmbeddedProgramPayloadArtifactV1::ContentArtifact {
            content_artifact_id: artifact_id.trim().to_owned(),
        };
    }
    EmbeddedProgramPayloadArtifactV1::Missing
}

fn bounded_embedded_program_error(error: String) -> String {
    const MAX_BYTES: usize = 512;
    if error.len() <= MAX_BYTES {
        return error;
    }
    let mut bounded = String::new();
    for character in error.chars() {
        if bounded.len() + character.len_utf8() + 3 > MAX_BYTES {
            break;
        }
        bounded.push(character);
    }
    bounded.push_str("...");
    bounded
}

impl Serialize for EmbeddedProgramDescriptor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let current = EmbeddedProgramRevisionArtifactV1 {
            revision: self.revision,
            role: self.role,
            capability_profile: self.capability_profile,
            artifact_retention: self.artifact_retention,
            payload: embedded_program_payload_artifact_v1(
                &self.source,
                &self.artifact_id,
                &self.support_sources,
            ),
        };
        let has_bootstrap = self.bootstrap_revision > 0
            || !self.bootstrap_source.is_empty()
            || !self.bootstrap_artifact_id.trim().is_empty()
            || !self.bootstrap_support_sources.is_empty();
        EmbeddedProgramArtifactV1 {
            format: EMBEDDED_PROGRAM_ARTIFACT_FORMAT,
            current,
            bootstrap: has_bootstrap.then(|| EmbeddedProgramRevisionArtifactV1 {
                revision: self.bootstrap_revision,
                role: self.role,
                capability_profile: self.capability_profile,
                artifact_retention: ProgramArtifactRetention::Ephemeral,
                payload: embedded_program_payload_artifact_v1(
                    &self.bootstrap_source,
                    &self.bootstrap_artifact_id,
                    &self.bootstrap_support_sources,
                ),
            }),
            session_key: &self.session_key,
            mount: self.mount,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EmbeddedProgramDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            #[serde(default)]
            source: String,
            revision: u64,
            #[serde(default)]
            artifact_id: String,
            #[serde(default)]
            artifact_retention: ProgramArtifactRetention,
            #[serde(default)]
            support_sources: Vec<EmbeddedProgramSourceUnit>,
            #[serde(default)]
            bootstrap_source: String,
            #[serde(default)]
            bootstrap_artifact_id: String,
            #[serde(default)]
            bootstrap_support_sources: Vec<EmbeddedProgramSourceUnit>,
            #[serde(default)]
            bootstrap_revision: u64,
            #[serde(default)]
            role: ProgramRole,
            capability_profile: ProgramCapabilityProfile,
            #[serde(default)]
            session_key: String,
            #[serde(default = "default_true")]
            mount: bool,
        }

        let input = Input::deserialize(deserializer)?;
        Ok(Self {
            source: input.source,
            revision: input.revision,
            artifact_id: input.artifact_id,
            artifact_retention: input.artifact_retention,
            support_sources: input.support_sources,
            bootstrap_source: input.bootstrap_source,
            bootstrap_artifact_id: input.bootstrap_artifact_id,
            bootstrap_support_sources: input.bootstrap_support_sources,
            bootstrap_revision: input.bootstrap_revision,
            role: input.role,
            capability_profile: input.capability_profile,
            session_key: input.session_key,
            mount: input.mount,
        })
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleRichTextSpan {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleEditorTypeHint {
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub start: usize,
    #[serde(default)]
    pub end: usize,
    #[serde(default)]
    pub anchor_column: usize,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub compact_label: String,
    #[serde(default)]
    pub detail_label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StyleValue {
    Text(String),
    Number(f64),
    Bool(bool),
    RichTextSpans(Vec<StyleRichTextSpan>),
    EditorTypeHints(Vec<StyleEditorTypeHint>),
}

impl StyleValue {
    /// Decodes the canonical boolean forms accepted at the document boundary.
    /// Boon closed tags are serialized as `True`/`False` text, while retained
    /// host state and patches may already carry a typed boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Text(value) if value.eq_ignore_ascii_case("true") => Some(true),
            Self::Text(value) if value.eq_ignore_ascii_case("false") => Some(false),
            Self::Number(value) => Some(*value != 0.0),
            Self::Text(_) | Self::RichTextSpans(_) | Self::EditorTypeHints(_) => None,
        }
    }
}

impl Serialize for StyleValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            StyleValue::Text(value) => serializer.serialize_str(value),
            StyleValue::Number(value) => serializer.serialize_f64(*value),
            StyleValue::Bool(value) => serializer.serialize_bool(*value),
            StyleValue::RichTextSpans(spans) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("kind", "rich_text_spans")?;
                map.serialize_entry("spans", spans)?;
                map.end()
            }
            StyleValue::EditorTypeHints(hints) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("kind", "editor_type_hints")?;
                map.serialize_entry("hints", hints)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for StyleValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match StyleValueRepr::deserialize(deserializer)? {
            StyleValueRepr::Text(value) => Ok(Self::Text(value)),
            StyleValueRepr::Number(value) => Ok(Self::Number(value)),
            StyleValueRepr::Bool(value) => Ok(Self::Bool(value)),
            StyleValueRepr::Typed(TypedStyleValue::RichTextSpans { spans }) => {
                Ok(Self::RichTextSpans(spans))
            }
            StyleValueRepr::Typed(TypedStyleValue::EditorTypeHints { hints }) => {
                Ok(Self::EditorTypeHints(hints))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StyleValueRepr {
    Text(String),
    Number(f64),
    Bool(bool),
    Typed(TypedStyleValue),
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TypedStyleValue {
    RichTextSpans { spans: Vec<StyleRichTextSpan> },
    EditorTypeHints { hints: Vec<StyleEditorTypeHint> },
}

pub type StyleMap = BTreeMap<String, StyleValue>;
pub type StylePatch = BTreeMap<String, Option<StyleValue>>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutStylePatch {
    pub patch: StylePatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaintStylePatch {
    pub patch: StylePatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStylePatch {
    pub patch: StylePatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialStylePatch {
    pub patch: StylePatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeBatch<T> {
    pub epoch: u64,
    pub changes: Vec<T>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextValue {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceBinding {
    pub id: SourceBindingId,
    pub source_path: String,
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<SourceRouteToken>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollState {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextInputFocusRequest {
    pub input_id: TextInputId,
    pub line: u64,
    pub column: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaterializedRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialization: Option<u64>,
    pub axis: Axis,
    pub visible: Range<u64>,
    pub overscan: Range<u64>,
    pub logical_item_count: u64,
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct DocumentNode {
    pub id: DocumentNodeId,
    pub kind: DocumentNodeKind,
    pub parent: Option<DocumentNodeId>,
    pub children: Vec<DocumentNodeId>,
    pub text: Option<TextValue>,
    pub style: StyleMap,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_viewport: Option<Box<MapViewportDescriptor>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedded_program: Option<EmbeddedProgramDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_bindings: Vec<SourceBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_input_id: Option<TextInputId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_focus: Option<TextInputFocusRequest>,
    pub scroll: Option<ScrollState>,
    pub materialized: Vec<MaterializedRange>,
}

impl DocumentNode {
    pub fn new(id: impl Into<String>, kind: DocumentNodeKind) -> Self {
        let embedded_program = matches!(kind, DocumentNodeKind::EmbeddedProgram)
            .then(EmbeddedProgramDescriptor::default);
        Self {
            id: DocumentNodeId(id.into()),
            kind,
            parent: None,
            children: Vec::new(),
            text: None,
            style: StyleMap::new(),
            map_viewport: None,
            embedded_program,
            source_bindings: Vec::new(),
            text_input_id: None,
            activation_focus: None,
            scroll: None,
            materialized: Vec::new(),
        }
    }

    pub fn source_bindings(&self) -> impl Iterator<Item = &SourceBinding> {
        self.source_bindings.iter()
    }

    pub fn primary_source_binding(&self) -> Option<&SourceBinding> {
        self.source_bindings.first()
    }

    pub fn has_source_binding(&self) -> bool {
        !self.source_bindings.is_empty()
    }

    pub fn set_primary_source_binding(&mut self, binding: SourceBinding) {
        if let Some(primary) = self.source_bindings.first_mut() {
            *primary = binding;
        } else {
            self.source_bindings.push(binding);
        }
    }

    pub fn is_sensitive_text_input(&self) -> bool {
        matches!(self.kind, DocumentNodeKind::TextInput)
            && style_flag(&self.style, SENSITIVE_INPUT_STYLE_KEY)
    }

    /// Returns a fixed presentation that is independent of the draft's length.
    pub fn presentation_text(&self, focused: bool) -> Option<String> {
        if self.is_sensitive_text_input() {
            return (focused
                || self
                    .text
                    .as_ref()
                    .is_some_and(|value| !value.text.is_empty()))
            .then(|| SENSITIVE_INPUT_REDACTED_GLYPHS.to_owned());
        }
        self.text.as_ref().map(|value| value.text.clone())
    }

    pub fn artifact_text(&self) -> Option<Cow<'_, TextValue>> {
        self.text.as_ref().map(|text| {
            if self.is_sensitive_text_input() {
                Cow::Owned(TextValue {
                    text: SENSITIVE_INPUT_REDACTED_VALUE.to_owned(),
                })
            } else {
                Cow::Borrowed(text)
            }
        })
    }

    pub fn artifact_style(&self) -> Cow<'_, StyleMap> {
        if !self.is_sensitive_text_input() {
            return Cow::Borrowed(&self.style);
        }
        let mut style = self.style.clone();
        for key in ["text", "value", "display_value", "contents"] {
            if style.contains_key(key) {
                style.insert(
                    key.to_owned(),
                    StyleValue::Text(SENSITIVE_INPUT_REDACTED_VALUE.to_owned()),
                );
            }
        }
        style.remove("selection_start");
        style.remove("selection_end");
        if style.contains_key("caret_column") {
            style.insert(
                "caret_column".to_owned(),
                StyleValue::Number(SENSITIVE_INPUT_REDACTED_GLYPHS.chars().count() as f64),
            );
        }
        Cow::Owned(style)
    }
}

impl Debug for DocumentNode {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DocumentNode")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("parent", &self.parent)
            .field("children", &self.children)
            .field("text", &self.artifact_text())
            .field("style", &self.artifact_style())
            .field("map_viewport", &self.map_viewport)
            .field("embedded_program", &self.embedded_program)
            .field("source_bindings", &self.source_bindings)
            .field("text_input_id", &self.text_input_id)
            .field("activation_focus", &self.activation_focus)
            .field("scroll", &self.scroll)
            .field("materialized", &self.materialized)
            .finish()
    }
}

impl Serialize for DocumentNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Artifact<'a> {
            id: &'a DocumentNodeId,
            kind: &'a DocumentNodeKind,
            parent: &'a Option<DocumentNodeId>,
            children: &'a [DocumentNodeId],
            text: Option<Cow<'a, TextValue>>,
            style: Cow<'a, StyleMap>,
            #[serde(skip_serializing_if = "Option::is_none")]
            map_viewport: &'a Option<Box<MapViewportDescriptor>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            embedded_program: &'a Option<EmbeddedProgramDescriptor>,
            #[serde(default, skip_serializing_if = "<[SourceBinding]>::is_empty")]
            source_bindings: &'a [SourceBinding],
            #[serde(skip_serializing_if = "Option::is_none")]
            text_input_id: &'a Option<TextInputId>,
            #[serde(skip_serializing_if = "Option::is_none")]
            activation_focus: &'a Option<TextInputFocusRequest>,
            scroll: &'a Option<ScrollState>,
            materialized: &'a [MaterializedRange],
        }

        Artifact {
            id: &self.id,
            kind: &self.kind,
            parent: &self.parent,
            children: &self.children,
            text: self.artifact_text(),
            style: self.artifact_style(),
            map_viewport: &self.map_viewport,
            embedded_program: &self.embedded_program,
            source_bindings: &self.source_bindings,
            text_input_id: &self.text_input_id,
            activation_focus: &self.activation_focus,
            scroll: &self.scroll,
            materialized: &self.materialized,
        }
        .serialize(serializer)
    }
}

fn style_flag(style: &StyleMap, key: &str) -> bool {
    style
        .get(key)
        .and_then(StyleValue::as_bool)
        .unwrap_or(false)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum DocumentPatch {
    UpsertNode(DocumentNode),
    RemoveNode {
        id: DocumentNodeId,
    },
    InsertChild {
        parent: DocumentNodeId,
        child: DocumentNodeId,
        index: usize,
    },
    RemoveChild {
        parent: DocumentNodeId,
        child: DocumentNodeId,
    },
    MoveChild {
        child: DocumentNodeId,
        new_parent: DocumentNodeId,
        index: usize,
    },
    SetText {
        id: DocumentNodeId,
        text: TextValue,
    },
    SetStyle {
        id: DocumentNodeId,
        patch: StylePatch,
    },
    SetEmbeddedProgram {
        id: DocumentNodeId,
        program: EmbeddedProgramDescriptor,
    },
    SetBinding {
        id: DocumentNodeId,
        binding: SourceBinding,
    },
    SetBindingAt {
        id: DocumentNodeId,
        ordinal: u32,
        binding: SourceBinding,
    },
    SetTextInputFocus {
        id: DocumentNodeId,
        text_input_id: Option<TextInputId>,
        activation_focus: Option<TextInputFocusRequest>,
    },
    SetScroll {
        id: DocumentNodeId,
        scroll: ScrollState,
    },
    SetListMaterialization {
        id: DocumentNodeId,
        materialized: MaterializedRange,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiSemanticChange {
    InsertNode {
        parent: DocumentNodeId,
        index: usize,
        node: Box<DocumentNode>,
    },
    RemoveSubtree {
        id: DocumentNodeId,
    },
    MoveNode {
        id: DocumentNodeId,
        parent: DocumentNodeId,
        index: usize,
    },
    SetText {
        id: DocumentNodeId,
        text: TextValue,
    },
    SetStyle {
        id: DocumentNodeId,
        patch: StylePatch,
    },
    SetLayoutStyle {
        id: DocumentNodeId,
        patch: LayoutStylePatch,
    },
    SetPaintStyle {
        id: DocumentNodeId,
        patch: PaintStylePatch,
    },
    SetTextStyle {
        id: DocumentNodeId,
        patch: TextStylePatch,
    },
    SetMaterialStyle {
        id: DocumentNodeId,
        patch: MaterialStylePatch,
    },
    SetBinding {
        id: DocumentNodeId,
        binding: SourceBinding,
    },
    SetBindingAt {
        id: DocumentNodeId,
        ordinal: u32,
        binding: SourceBinding,
    },
    SetVisibility {
        id: DocumentNodeId,
        visible: bool,
    },
    SetScroll {
        id: DocumentNodeId,
        scroll: ScrollState,
    },
    SetListWindow {
        id: DocumentNodeId,
        materialized: MaterializedRange,
    },
}

impl UiSemanticChange {
    pub fn into_document_patches(self) -> Vec<DocumentPatch> {
        match self {
            UiSemanticChange::InsertNode {
                parent,
                index,
                mut node,
            } => {
                node.parent = Some(parent.clone());
                let child = node.id.clone();
                vec![
                    DocumentPatch::UpsertNode(*node),
                    DocumentPatch::InsertChild {
                        parent,
                        child,
                        index,
                    },
                ]
            }
            UiSemanticChange::RemoveSubtree { id } => vec![DocumentPatch::RemoveNode { id }],
            UiSemanticChange::MoveNode { id, parent, index } => vec![DocumentPatch::MoveChild {
                child: id,
                new_parent: parent,
                index,
            }],
            UiSemanticChange::SetText { id, text } => {
                vec![DocumentPatch::SetText { id, text }]
            }
            UiSemanticChange::SetStyle { id, patch } => {
                vec![DocumentPatch::SetStyle { id, patch }]
            }
            UiSemanticChange::SetLayoutStyle { id, patch } => {
                vec![DocumentPatch::SetStyle {
                    id,
                    patch: patch.patch,
                }]
            }
            UiSemanticChange::SetPaintStyle { id, patch } => {
                vec![DocumentPatch::SetStyle {
                    id,
                    patch: patch.patch,
                }]
            }
            UiSemanticChange::SetTextStyle { id, patch } => {
                vec![DocumentPatch::SetStyle {
                    id,
                    patch: patch.patch,
                }]
            }
            UiSemanticChange::SetMaterialStyle { id, patch } => {
                vec![DocumentPatch::SetStyle {
                    id,
                    patch: patch.patch,
                }]
            }
            UiSemanticChange::SetBinding { id, binding } => {
                vec![DocumentPatch::SetBinding { id, binding }]
            }
            UiSemanticChange::SetBindingAt {
                id,
                ordinal,
                binding,
            } => {
                vec![DocumentPatch::SetBindingAt {
                    id,
                    ordinal,
                    binding,
                }]
            }
            UiSemanticChange::SetVisibility { id, visible } => {
                let mut patch = StylePatch::new();
                patch.insert("visible".to_owned(), Some(StyleValue::Bool(visible)));
                vec![DocumentPatch::SetStyle { id, patch }]
            }
            UiSemanticChange::SetScroll { id, scroll } => {
                vec![DocumentPatch::SetScroll { id, scroll }]
            }
            UiSemanticChange::SetListWindow { id, materialized } => {
                vec![DocumentPatch::SetListMaterialization { id, materialized }]
            }
        }
    }
}

impl From<ChangeBatch<UiSemanticChange>> for ChangeBatch<DocumentPatch> {
    fn from(batch: ChangeBatch<UiSemanticChange>) -> Self {
        Self {
            epoch: batch.epoch,
            changes: batch
                .changes
                .into_iter()
                .flat_map(UiSemanticChange::into_document_patches)
                .collect(),
        }
    }
}

impl From<ChangeBatch<DocumentPatch>> for Vec<DocumentPatch> {
    fn from(batch: ChangeBatch<DocumentPatch>) -> Self {
        batch.changes
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentFrame {
    pub root: DocumentNodeId,
    pub nodes: BTreeMap<DocumentNodeId, DocumentNode>,
    pub focus: Option<DocumentNodeId>,
    pub scroll_roots: BTreeMap<ScrollRootId, ScrollState>,
}

impl DocumentFrame {
    pub fn empty(root: impl Into<String>) -> Self {
        let root = DocumentNodeId(root.into());
        let root_node = DocumentNode::new(root.0.clone(), DocumentNodeKind::Root);
        let mut nodes = BTreeMap::new();
        nodes.insert(root.clone(), root_node);
        Self {
            root,
            nodes,
            focus: None,
            scroll_roots: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests;
