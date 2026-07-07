use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::ops::Range;

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
    Table,
    TableCell,
    ScrollRoot,
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
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::String(value) => Ok(StyleValue::Text(value)),
            serde_json::Value::Number(value) => value
                .as_f64()
                .map(StyleValue::Number)
                .ok_or_else(|| serde::de::Error::custom("style number must fit f64")),
            serde_json::Value::Bool(value) => Ok(StyleValue::Bool(value)),
            serde_json::Value::Object(mut value) => {
                let kind = value
                    .remove("kind")
                    .and_then(|kind| kind.as_str().map(str::to_owned))
                    .ok_or_else(|| serde::de::Error::custom("typed style value needs kind"))?;
                match kind.as_str() {
                    "rich_text_spans" => {
                        serde_json::from_value(value.remove("spans").ok_or_else(|| {
                            serde::de::Error::custom("rich_text_spans needs spans")
                        })?)
                        .map(StyleValue::RichTextSpans)
                        .map_err(serde::de::Error::custom)
                    }
                    "editor_type_hints" => {
                        serde_json::from_value(value.remove("hints").ok_or_else(|| {
                            serde::de::Error::custom("editor_type_hints needs hints")
                        })?)
                        .map(StyleValue::EditorTypeHints)
                        .map_err(serde::de::Error::custom)
                    }
                    _ => Err(serde::de::Error::custom(format!(
                        "unknown typed style value kind `{kind}`"
                    ))),
                }
            }
            _ => Err(serde::de::Error::custom(
                "style value must be a string, number, bool, or typed style object",
            )),
        }
    }
}

impl StyleValue {
    pub fn from_legacy_rich_text_spans_json(payload: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<Vec<StyleRichTextSpan>>(payload).map(Self::RichTextSpans)
    }

    pub fn from_legacy_editor_type_hints_json(payload: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<Vec<StyleEditorTypeHint>>(payload).map(Self::EditorTypeHints)
    }
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
    pub axis: Axis,
    pub visible: Range<u64>,
    pub overscan: Range<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentNode {
    pub id: DocumentNodeId,
    pub kind: DocumentNodeKind,
    pub parent: Option<DocumentNodeId>,
    pub children: Vec<DocumentNodeId>,
    pub text: Option<TextValue>,
    pub style: StyleMap,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_bindings: Vec<SourceBinding>,
    pub scroll: Option<ScrollState>,
    pub materialized: Vec<MaterializedRange>,
}

impl DocumentNode {
    pub fn new(id: impl Into<String>, kind: DocumentNodeKind) -> Self {
        Self {
            id: DocumentNodeId(id.into()),
            kind,
            parent: None,
            children: Vec::new(),
            text: None,
            style: StyleMap::new(),
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
mod tests {
    use super::*;

    #[test]
    fn typed_style_payloads_round_trip_as_typed_objects() {
        let rich_text = StyleValue::RichTextSpans(vec![StyleRichTextSpan {
            text: "SOURCE".to_owned(),
            source_text: Some("SOURCE".to_owned()),
            color: Some("#ff0000".to_owned()),
            font_style: Some("italic".to_owned()),
            font_weight: Some("bold".to_owned()),
        }]);
        let hints = StyleValue::EditorTypeHints(vec![StyleEditorTypeHint {
            line: 2,
            start: 4,
            end: 8,
            anchor_column: 12,
            category: "return".to_owned(),
            compact_label: "TEXT".to_owned(),
            detail_label: "TEXT value".to_owned(),
        }]);

        for value in [rich_text, hints] {
            let encoded = serde_json::to_value(&value).expect("style value should serialize");
            assert!(
                encoded.get("kind").is_some(),
                "typed style payloads must use tagged objects"
            );
            let decoded: StyleValue =
                serde_json::from_value(encoded).expect("typed style value should deserialize");
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn legacy_typed_style_payload_strings_decode_only_through_explicit_helpers() {
        let rich_text_payload = serde_json::to_string(&vec![StyleRichTextSpan {
            text: "SOURCE".to_owned(),
            source_text: Some("SOURCE".to_owned()),
            color: Some("#ff0000".to_owned()),
            font_style: Some("italic".to_owned()),
            font_weight: Some("bold".to_owned()),
        }])
        .unwrap();
        let decoded_scalar: StyleValue =
            serde_json::from_value(serde_json::Value::String(rich_text_payload.clone()))
                .expect("legacy scalar string should remain a text style");
        assert!(matches!(decoded_scalar, StyleValue::Text(_)));

        let decoded_rich_text =
            StyleValue::from_legacy_rich_text_spans_json(&rich_text_payload).unwrap();
        assert!(matches!(decoded_rich_text, StyleValue::RichTextSpans(_)));

        let hint_payload = serde_json::to_string(&vec![StyleEditorTypeHint {
            line: 2,
            start: 4,
            end: 8,
            anchor_column: 12,
            category: "return".to_owned(),
            compact_label: "TEXT".to_owned(),
            detail_label: "TEXT value".to_owned(),
        }])
        .unwrap();
        let decoded_hints = StyleValue::from_legacy_editor_type_hints_json(&hint_payload).unwrap();
        assert!(matches!(decoded_hints, StyleValue::EditorTypeHints(_)));
    }

    #[test]
    fn typed_ui_style_changes_lower_to_compatible_style_patches() {
        let node = DocumentNodeId("node".to_owned());
        let typed_changes = vec![
            UiSemanticChange::SetLayoutStyle {
                id: node.clone(),
                patch: LayoutStylePatch {
                    patch: BTreeMap::from([("width".to_owned(), Some(StyleValue::Number(120.0)))]),
                },
            },
            UiSemanticChange::SetPaintStyle {
                id: node.clone(),
                patch: PaintStylePatch {
                    patch: BTreeMap::from([(
                        "background".to_owned(),
                        Some(StyleValue::Text("#fff".to_owned())),
                    )]),
                },
            },
            UiSemanticChange::SetTextStyle {
                id: node.clone(),
                patch: TextStylePatch {
                    patch: BTreeMap::from([(
                        "font_weight".to_owned(),
                        Some(StyleValue::Text("bold".to_owned())),
                    )]),
                },
            },
            UiSemanticChange::SetMaterialStyle {
                id: node.clone(),
                patch: MaterialStylePatch {
                    patch: BTreeMap::from([(
                        "material".to_owned(),
                        Some(StyleValue::Text("glass".to_owned())),
                    )]),
                },
            },
        ];
        let batch: ChangeBatch<DocumentPatch> = ChangeBatch {
            epoch: 11,
            changes: typed_changes,
        }
        .into();

        assert_eq!(batch.epoch, 11);
        assert_eq!(batch.changes.len(), 4);
        for patch in batch.changes {
            assert!(
                matches!(patch, DocumentPatch::SetStyle { id, .. } if id == node),
                "typed style semantic changes should preserve compatible SetStyle lowering"
            );
        }
    }
}
