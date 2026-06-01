use serde::{Deserialize, Serialize};
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
#[serde(untagged)]
pub enum StyleValue {
    Text(String),
    Number(f64),
    Bool(bool),
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
