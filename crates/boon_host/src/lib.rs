use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use boon_document_model::{DocumentNodeId, Rect, SourceBindingId};

macro_rules! string_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            pub struct $name(pub String);
        )+
    };
}

string_ids!(SurfaceId, WindowId, RoleId);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogicalSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub surface: u64,
    pub width: f32,
    pub height: f32,
    pub scale: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceResizeEvent {
    pub surface: SurfaceId,
    pub logical_size: LogicalSize,
    pub scale: f64,
    pub physical_size: PhysicalSize,
    pub epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostEvent {
    Keyboard(KeyEvent),
    TextInput(TextInputEvent),
    Pointer(PointerEvent),
    Wheel(WheelEvent),
    Focus { surface: SurfaceId, focused: bool },
    CloseRequested { window: WindowId },
    Resize(SurfaceResizeEvent),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyEvent {
    pub surface: SurfaceId,
    pub key: String,
    pub pressed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextInputEvent {
    pub surface: SurfaceId,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerEvent {
    pub surface: SurfaceId,
    pub x: f32,
    pub y: f32,
    pub pressed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WheelEvent {
    pub surface: SurfaceId,
    pub delta_x: f32,
    pub delta_y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewportIntent {
    Scroll { delta_x: f32, delta_y: f32 },
    Resize { size: LogicalSize },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceIntent {
    pub binding: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitResolution {
    pub surface: SurfaceId,
    pub target: Option<String>,
    pub focused: Option<String>,
    pub viewport_intent: Option<ViewportIntent>,
    pub source_intents: Vec<SourceIntent>,
}

string_ids!(SemanticId);

impl SemanticId {
    pub fn from_document_node_id(node: &DocumentNodeId) -> Self {
        Self(format!("semantic:{}", node.0))
    }

    pub fn from_world_editor_node_id(node: &boon_scene_model::WorldSemanticEditorNodeId) -> Self {
        Self(format!("semantic:{}", node.0))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRole {
    Application,
    Group,
    Row,
    Text,
    Button,
    Checkbox,
    TextInput,
    Table,
    Cell,
    ScrollRegion,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticValue {
    Text { text: String },
    Bool { value: bool },
    Number { value: f64 },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticState {
    pub focused: bool,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticActions {
    pub focus: bool,
    pub press: bool,
    pub set_text: bool,
    pub increment: bool,
    pub decrement: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticRelations {
    pub parent: Option<SemanticId>,
    pub children: Vec<SemanticId>,
    pub controls: Vec<SemanticId>,
    pub labelled_by: Vec<SemanticId>,
    pub described_by: Vec<SemanticId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticNode {
    pub id: SemanticId,
    pub node: DocumentNodeId,
    pub role: SemanticRole,
    pub name: Option<String>,
    pub description: Option<String>,
    pub value: Option<SemanticValue>,
    pub state: SemanticState,
    pub actions: SemanticActions,
    pub relations: SemanticRelations,
    pub bounds: Option<Rect>,
    pub language: Option<String>,
    pub heading_level: Option<u8>,
    pub href: Option<String>,
    pub source_binding_id: Option<SourceBindingId>,
    pub source_path: Option<String>,
    pub source_intent: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticScene {
    pub root: Option<SemanticId>,
    pub nodes: BTreeMap<SemanticId, SemanticNode>,
    pub focused: Option<SemanticId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticPatch {
    pub operations: Vec<SemanticPatchOperation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticPatchOperation {
    UpsertNode { node: SemanticNode },
    RemoveNode { id: SemanticId },
    SetFocus { focused: Option<SemanticId> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticAction {
    Focus,
    Press,
    SetText,
    Increment,
    Decrement,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticInputEvent {
    Focus {
        semantic_id: SemanticId,
    },
    Press {
        semantic_id: SemanticId,
    },
    SetText {
        semantic_id: SemanticId,
        text: String,
    },
    ReplaceSelectedText {
        semantic_id: SemanticId,
        text: String,
    },
    Increment {
        semantic_id: SemanticId,
    },
    Decrement {
        semantic_id: SemanticId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceDispatch {
    pub semantic_id: SemanticId,
    pub node: DocumentNodeId,
    pub source_path: String,
    pub source_intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl SemanticScene {
    pub fn from_world_editor_tree(tree: &boon_scene_model::WorldSemanticEditorTree) -> Self {
        let mut scene = Self {
            root: Some(SemanticId::from_world_editor_node_id(&tree.root)),
            nodes: BTreeMap::new(),
            focused: tree
                .focused
                .as_ref()
                .map(SemanticId::from_world_editor_node_id),
        };
        for node in tree.nodes.values() {
            let semantic = semantic_node_from_world_editor_node(node, tree);
            scene.nodes.insert(semantic.id.clone(), semantic);
        }
        scene
    }

    pub fn diff(&self, next: &SemanticScene) -> SemanticPatch {
        let mut operations = Vec::new();
        for id in self.nodes.keys() {
            if !next.nodes.contains_key(id) {
                operations.push(SemanticPatchOperation::RemoveNode { id: id.clone() });
            }
        }
        for (id, node) in &next.nodes {
            if self.nodes.get(id) != Some(node) {
                operations.push(SemanticPatchOperation::UpsertNode { node: node.clone() });
            }
        }
        if self.focused != next.focused {
            operations.push(SemanticPatchOperation::SetFocus {
                focused: next.focused.clone(),
            });
        }
        SemanticPatch { operations }
    }

    pub fn source_dispatch_for_event(
        &self,
        event: SemanticInputEvent,
    ) -> Option<SemanticSourceDispatch> {
        let (semantic_id, action, text) = match event {
            SemanticInputEvent::Focus { semantic_id } => (semantic_id, SemanticAction::Focus, None),
            SemanticInputEvent::Press { semantic_id } => (semantic_id, SemanticAction::Press, None),
            SemanticInputEvent::SetText { semantic_id, text }
            | SemanticInputEvent::ReplaceSelectedText { semantic_id, text } => {
                (semantic_id, SemanticAction::SetText, Some(text))
            }
            SemanticInputEvent::Increment { semantic_id } => {
                (semantic_id, SemanticAction::Increment, None)
            }
            SemanticInputEvent::Decrement { semantic_id } => {
                (semantic_id, SemanticAction::Decrement, None)
            }
        };
        let node = self.nodes.get(&semantic_id)?;
        Some(SemanticSourceDispatch {
            semantic_id,
            node: node.node.clone(),
            source_path: semantic_source_for_action(node, &action)?,
            source_intent: node.source_intent.clone(),
            text,
        })
    }
}

fn semantic_node_from_world_editor_node(
    node: &boon_scene_model::WorldSemanticEditorNode,
    tree: &boon_scene_model::WorldSemanticEditorTree,
) -> SemanticNode {
    let id = SemanticId::from_world_editor_node_id(&node.id);
    let source_intent = world_editor_source_intent(node);
    let source_path = source_intent
        .as_ref()
        .map(|intent| world_editor_source_path(node, intent));
    SemanticNode {
        id,
        node: DocumentNodeId(format!("world:{}", node.id.0)),
        role: semantic_role_for_world_editor_role(&node.role, &node.actions),
        name: Some(node.label.clone()),
        description: world_editor_description(node),
        value: world_editor_value(node),
        state: SemanticState {
            focused: tree.focused.as_ref() == Some(&node.id),
            checked: None,
            disabled: !world_editor_node_enabled(node),
            selected: node.selected,
        },
        actions: SemanticActions {
            focus: node.actions.focus || node.actions.select || node.actions.export_3mf,
            press: node.actions.select || node.actions.toggle_visibility || node.actions.export_3mf,
            set_text: false,
            increment: false,
            decrement: false,
        },
        relations: SemanticRelations {
            parent: world_editor_parent_id(&node.id, tree)
                .map(SemanticId::from_world_editor_node_id),
            children: node
                .children
                .iter()
                .map(SemanticId::from_world_editor_node_id)
                .collect(),
            controls: Vec::new(),
            labelled_by: Vec::new(),
            described_by: Vec::new(),
        },
        bounds: None,
        language: None,
        heading_level: None,
        href: None,
        source_binding_id: source_path
            .as_ref()
            .map(|path| SourceBindingId(format!("source:{path}"))),
        source_path,
        source_intent,
    }
}

fn semantic_role_for_world_editor_role(
    role: &boon_scene_model::WorldSemanticEditorRole,
    actions: &boon_scene_model::WorldSemanticEditorActions,
) -> SemanticRole {
    match role {
        boon_scene_model::WorldSemanticEditorRole::Editor => SemanticRole::Application,
        boon_scene_model::WorldSemanticEditorRole::Viewport
        | boon_scene_model::WorldSemanticEditorRole::Assembly
        | boon_scene_model::WorldSemanticEditorRole::Parameters
        | boon_scene_model::WorldSemanticEditorRole::Manufacturing => SemanticRole::Group,
        boon_scene_model::WorldSemanticEditorRole::PartInstance
        | boon_scene_model::WorldSemanticEditorRole::Parameter
        | boon_scene_model::WorldSemanticEditorRole::Action
            if actions.select || actions.edit_parameter || actions.export_3mf =>
        {
            SemanticRole::Button
        }
        boon_scene_model::WorldSemanticEditorRole::PartInstance => SemanticRole::Row,
        boon_scene_model::WorldSemanticEditorRole::Parameter
        | boon_scene_model::WorldSemanticEditorRole::Status => SemanticRole::Text,
        boon_scene_model::WorldSemanticEditorRole::Action => SemanticRole::Button,
    }
}

fn world_editor_description(node: &boon_scene_model::WorldSemanticEditorNode) -> Option<String> {
    match node.role {
        boon_scene_model::WorldSemanticEditorRole::PartInstance => Some(format!(
            "part {:?}, feature {:?}, {:?}",
            node.part_id, node.feature_id, node.manufacturing_role
        )),
        boon_scene_model::WorldSemanticEditorRole::Action if node.actions.export_3mf => {
            Some("Export the prepared printable assembly as 3MF".to_owned())
        }
        _ => None,
    }
}

fn world_editor_value(node: &boon_scene_model::WorldSemanticEditorNode) -> Option<SemanticValue> {
    if node.role == boon_scene_model::WorldSemanticEditorRole::Status {
        Some(SemanticValue::Text {
            text: node.label.clone(),
        })
    } else if node.role == boon_scene_model::WorldSemanticEditorRole::PartInstance {
        Some(SemanticValue::Text {
            text: if node.visible { "visible" } else { "hidden" }.to_owned(),
        })
    } else {
        None
    }
}

fn world_editor_node_enabled(node: &boon_scene_model::WorldSemanticEditorNode) -> bool {
    node.actions.focus
        || node.actions.select
        || node.actions.toggle_visibility
        || node.actions.edit_parameter
        || node.actions.export_3mf
        || !node.children.is_empty()
}

fn world_editor_source_intent(node: &boon_scene_model::WorldSemanticEditorNode) -> Option<String> {
    if node.actions.export_3mf {
        Some("press".to_owned())
    } else if node.actions.select || node.actions.toggle_visibility {
        Some("select".to_owned())
    } else if node.actions.focus {
        Some("focus".to_owned())
    } else if node.actions.edit_parameter {
        Some("press".to_owned())
    } else {
        None
    }
}

fn world_editor_source_path(
    node: &boon_scene_model::WorldSemanticEditorNode,
    intent: &str,
) -> String {
    if node.actions.export_3mf {
        "world.manufacturing.export_3mf".to_owned()
    } else if let Some(instance) = node.instance {
        format!("world.instance.{}.{}", instance.0, intent)
    } else {
        format!(
            "world.editor.{}.{}",
            semantic_path_token(&node.id.0),
            intent
        )
    }
}

fn semantic_path_token(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn world_editor_parent_id<'a>(
    child: &boon_scene_model::WorldSemanticEditorNodeId,
    tree: &'a boon_scene_model::WorldSemanticEditorTree,
) -> Option<&'a boon_scene_model::WorldSemanticEditorNodeId> {
    tree.nodes
        .values()
        .find(|node| node.children.iter().any(|candidate| candidate == child))
        .map(|node| &node.id)
}

fn semantic_source_for_action(node: &SemanticNode, action: &SemanticAction) -> Option<String> {
    let intent = node.source_intent.as_deref()?;
    let matches_action = match action {
        SemanticAction::Focus => intent == "focus",
        SemanticAction::Press => matches!(
            intent,
            "press" | "click" | "source" | "activate" | "toggle" | "submit" | "open" | "select"
        ),
        SemanticAction::SetText => matches!(intent, "change" | "text" | "input"),
        SemanticAction::Increment => intent == "increment",
        SemanticAction::Decrement => intent == "decrement",
    };
    matches_action.then(|| node.source_path.clone()).flatten()
}
