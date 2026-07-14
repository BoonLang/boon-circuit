use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::ops::Range;

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

string_ids!(DocumentNodeId, SourceBindingId, ScrollRootId);

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
    Table,
    TableCell,
    ScrollRoot,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramCapabilityProfile {
    #[default]
    PublicDocument,
}

impl ProgramCapabilityProfile {
    pub fn name(self) -> &'static str {
        match self {
            Self::PublicDocument => "public_document",
        }
    }
}

#[derive(Clone, Default, Eq, PartialEq, Deserialize)]
pub struct EmbeddedProgramDescriptor {
    #[serde(default)]
    pub source: String,
    pub source_digest: String,
    pub revision: u64,
    pub capability_profile: ProgramCapabilityProfile,
}

impl Debug for EmbeddedProgramDescriptor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbeddedProgramDescriptor")
            .field("source_digest", &self.source_digest)
            .field("source_bytes", &self.source.len())
            .field("revision", &self.revision)
            .field("capability_profile", &self.capability_profile)
            .finish()
    }
}

impl Serialize for EmbeddedProgramDescriptor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Artifact<'a> {
            source_digest: &'a str,
            source_bytes: usize,
            revision: u64,
            capability_profile: ProgramCapabilityProfile,
        }

        Artifact {
            source_digest: &self.source_digest,
            source_bytes: self.source.len(),
            revision: self.revision,
            capability_profile: self.capability_profile,
        }
        .serialize(serializer)
    }
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
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollState {
    pub x: f32,
    pub y: f32,
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
    pub embedded_program: Option<EmbeddedProgramDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_bindings: Vec<SourceBinding>,
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
            embedded_program,
            source_bindings: Vec::new(),
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
            .field("embedded_program", &self.embedded_program)
            .field("source_bindings", &self.source_bindings)
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
            embedded_program: &'a Option<EmbeddedProgramDescriptor>,
            #[serde(default, skip_serializing_if = "<[SourceBinding]>::is_empty")]
            source_bindings: &'a [SourceBinding],
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
            embedded_program: &self.embedded_program,
            source_bindings: &self.source_bindings,
            scroll: &self.scroll,
            materialized: &self.materialized,
        }
        .serialize(serializer)
    }
}

fn style_flag(style: &StyleMap, key: &str) -> bool {
    match style.get(key) {
        Some(StyleValue::Bool(value)) => *value,
        Some(StyleValue::Text(value)) => value.eq_ignore_ascii_case("true"),
        Some(StyleValue::Number(value)) => *value != 0.0,
        Some(StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_)) | None => false,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
        node: DocumentNode,
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
                    DocumentPatch::UpsertNode(node),
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
