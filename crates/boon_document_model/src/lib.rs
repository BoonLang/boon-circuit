use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DocumentNodeId(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SourceBindingId(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ScrollRootId(pub String);

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
    pub source_binding: Option<SourceBinding>,
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
            source_binding: None,
            scroll: None,
            materialized: Vec::new(),
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
}
