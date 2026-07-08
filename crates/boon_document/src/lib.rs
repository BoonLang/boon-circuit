pub use boon_document_model::{
    Axis, ChangeBatch, DocumentFrame, DocumentNode, DocumentNodeId, DocumentNodeKind,
    DocumentPatch, LayoutStylePatch, MaterialStylePatch, MaterializedRange, PaintStylePatch, Rect,
    ScrollRootId, SourceBindingId, StyleEditorTypeHint, StyleMap, StylePatch, StyleRichTextSpan,
    StyleValue, TextStylePatch, TextValue, UiSemanticChange,
};
pub mod render_scene;
use boon_host::Viewport;
pub use boon_host::{
    SemanticAction, SemanticActions, SemanticId, SemanticInputEvent, SemanticNode, SemanticPatch,
    SemanticPatchOperation, SemanticRelations, SemanticRole, SemanticScene, SemanticSourceDispatch,
    SemanticState, SemanticValue,
};
pub use render_scene::{
    RenderFontStyle, RenderFontWeight, RenderQuadBatch, RenderRichTextSpan, RenderScene,
    RenderSceneItem, RenderSceneMetrics, RenderScenePaintPatch, RenderScenePatch,
    RenderScenePatchOperation, RenderScenePatchReport, RenderTextAlign, RenderTextPlacementKey,
    RenderTextRun, RenderTextShapeKey, RenderTextVerticalAlign, RenderTextureRef,
    RenderVisualPrimitive, RenderVisualPrimitiveKind, RetainedRenderChunkDescriptor,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

pub trait TextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32) -> TextMetrics;

    fn measure_styled(
        &mut self,
        text: &str,
        font_size: f32,
        style: &BTreeMap<String, StyleValue>,
    ) -> TextMetrics {
        let _ = style;
        self.measure(text, font_size)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderCapabilities {
    pub max_texture_dimension_2d: u32,
    pub supports_instancing: bool,
    pub supports_clip_rects: bool,
    pub text_backend_class: String,
}

impl RenderCapabilities {
    pub fn fake_portable() -> Self {
        Self {
            max_texture_dimension_2d: 4096,
            supports_instancing: true,
            supports_clip_rects: true,
            text_backend_class: "fake-portable".to_owned(),
        }
    }
}

pub struct LayoutInput<'a> {
    pub document: &'a DocumentFrame,
    pub viewport: Viewport,
    pub text: &'a mut dyn TextMeasurer,
    pub capabilities: RenderCapabilities,
}

pub struct LayoutSubtreeInput<'a> {
    pub document: &'a DocumentFrame,
    pub root: &'a DocumentNodeId,
    pub x: f32,
    pub y: f32,
    pub available_width: f32,
    pub available_height: f32,
    pub text: &'a mut dyn TextMeasurer,
    pub capabilities: RenderCapabilities,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutFrame {
    pub display_list: Vec<DisplayItem>,
    pub hit_regions: Vec<HitRegion>,
    pub scroll_regions: Vec<ScrollRegion>,
    pub accessibility: AccessibilityTree,
    pub demands: Vec<LayoutDemand>,
    pub materialization: Vec<MaterializationReport>,
    pub metrics: LayoutMetrics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayItem {
    pub node: DocumentNodeId,
    pub kind: DocumentNodeKind,
    pub bounds: Rect,
    pub text: Option<String>,
    pub style_identity: ComputedStyleIdentity,
    pub style: BTreeMap<String, StyleValue>,
    pub focused: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComputedStyleIdentity {
    pub style_id: u64,
    pub layout_id: u64,
    pub paint_id: u64,
    pub material_id: u64,
    pub font_id: u64,
    pub pseudo_state_id: u64,
}

impl ComputedStyleIdentity {
    pub fn from_style(style: &BTreeMap<String, StyleValue>) -> Self {
        computed_style_identity(style)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitRegion {
    pub id: String,
    pub node: DocumentNodeId,
    pub bounds: Rect,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HitSideTable {
    pub bucket_size: f32,
    pub entries: Vec<HitSideTableEntry>,
    pub buckets: BTreeMap<String, Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitSideTableEntry {
    pub hit_id: String,
    pub node: DocumentNodeId,
    pub source_binding_id: Option<SourceBindingId>,
    pub source_path: Option<String>,
    pub source_intent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_binding_refs: Vec<DocumentTypedBindingRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_routes: Vec<DocumentTypedBindingRoute>,
    pub bounds: Rect,
    pub z_depth: u32,
    pub scroll_root: Option<ScrollRootId>,
    pub row_key: Option<u64>,
    pub row_generation: Option<u64>,
    pub spatial_bucket: HitSpatialBucket,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct HitSpatialBucket {
    pub x: i32,
    pub y: i32,
}

impl HitSideTable {
    pub const DEFAULT_BUCKET_SIZE: f32 = 128.0;

    pub fn from_document_layout(document: &DocumentFrame, layout: &LayoutFrame) -> Self {
        Self::from_document_layout_with_bucket_size(document, layout, Self::DEFAULT_BUCKET_SIZE)
    }

    pub fn from_document_layout_with_bucket_size(
        document: &DocumentFrame,
        layout: &LayoutFrame,
        bucket_size: f32,
    ) -> Self {
        let bucket_size = if bucket_size.is_finite() && bucket_size > 0.0 {
            bucket_size
        } else {
            Self::DEFAULT_BUCKET_SIZE
        };
        let mut table = Self {
            bucket_size,
            entries: Vec::with_capacity(layout.hit_regions.len()),
            buckets: BTreeMap::new(),
        };
        for (index, hit) in layout.hit_regions.iter().enumerate() {
            let node = document.nodes.get(&hit.node);
            let binding = node.and_then(|node| node.primary_source_binding());
            let spatial_bucket = hit_bucket_for_point(hit.bounds.x, hit.bounds.y, bucket_size);
            let entry_index = table.entries.len();
            let entry = HitSideTableEntry {
                hit_id: hit.id.clone(),
                node: hit.node.clone(),
                source_binding_id: binding.map(|binding| binding.id.clone()),
                source_path: binding.map(|binding| binding.source_path.clone()),
                source_intent: binding.map(|binding| binding.intent.clone()),
                source_binding_refs: Vec::new(),
                source_routes: binding
                    .map(|binding| {
                        vec![DocumentTypedBindingRoute {
                            source_path: binding.source_path.clone(),
                            intent: binding.intent.clone(),
                        }]
                    })
                    .unwrap_or_default(),
                bounds: hit.bounds,
                z_depth: index as u32,
                scroll_root: scroll_root_for_node(document, &hit.node),
                row_key: node.and_then(|node| {
                    style_u64_any(&node.style, &["row_key", "target_key", "__row_key"])
                }),
                row_generation: node.and_then(|node| {
                    style_u64_any(
                        &node.style,
                        &[
                            "row_generation",
                            "target_generation",
                            "generation",
                            "__row_generation",
                        ],
                    )
                }),
                spatial_bucket,
            };
            table.push_entry(entry, entry_index);
        }
        table
    }

    pub fn try_from_document_layout_with_typed_bindings(
        document: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
        typed_bindings: &DocumentTypedBindingIndex,
        layout: &LayoutFrame,
        bucket_size: f32,
    ) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(document)?;
        let bucket_size = if bucket_size.is_finite() && bucket_size > 0.0 {
            bucket_size
        } else {
            Self::DEFAULT_BUCKET_SIZE
        };
        let mut table = Self {
            bucket_size,
            entries: Vec::with_capacity(layout.hit_regions.len()),
            buckets: BTreeMap::new(),
        };
        for (index, hit) in layout.hit_regions.iter().enumerate() {
            let node =
                document
                    .nodes
                    .get(&hit.node)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "hit_region_node",
                        id: hit.node.clone(),
                    })?;
            let hot = hot_ids
                .hot_id(&hit.node)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "hot_id_table",
                    id: hit.node.clone(),
                })?;
            let bindings = typed_bindings.bindings_for_node(hot);
            let primary = bindings.first();
            let spatial_bucket = hit_bucket_for_point(hit.bounds.x, hit.bounds.y, bucket_size);
            let entry_index = table.entries.len();
            let entry = HitSideTableEntry {
                hit_id: hit.id.clone(),
                node: hit.node.clone(),
                source_binding_id: primary.map(|binding| binding.binding_id.clone()),
                source_path: primary.map(|binding| binding.route.source_path.clone()),
                source_intent: primary.map(|binding| binding.route.intent.clone()),
                source_binding_refs: bindings.iter().map(|binding| binding.reference).collect(),
                source_routes: bindings
                    .iter()
                    .map(|binding| binding.route.clone())
                    .collect(),
                bounds: hit.bounds,
                z_depth: index as u32,
                scroll_root: scroll_root_for_node(document, &hit.node),
                row_key: style_u64_any(&node.style, &["row_key", "target_key", "__row_key"]),
                row_generation: style_u64_any(
                    &node.style,
                    &[
                        "row_generation",
                        "target_generation",
                        "generation",
                        "__row_generation",
                    ],
                ),
                spatial_bucket,
            };
            table.push_entry(entry, entry_index);
        }
        Ok(table)
    }

    fn push_entry(&mut self, entry: HitSideTableEntry, entry_index: usize) {
        for bucket in buckets_for_rect(entry.bounds, self.bucket_size) {
            self.buckets
                .entry(hit_bucket_key(bucket))
                .or_default()
                .push(entry_index);
        }
        self.entries.push(entry);
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<&HitSideTableEntry> {
        let bucket = hit_bucket_for_point(x, y, self.bucket_size);
        let candidates = self.buckets.get(&hit_bucket_key(bucket))?;
        candidates
            .iter()
            .filter_map(|index| self.entries.get(*index).map(|entry| (*index, entry)))
            .filter(|(_, entry)| rect_contains(entry.bounds, x, y))
            .min_by(|left, right| compare_typed_hit_priority(left, right, x, y))
            .map(|(_, entry)| entry)
    }

    pub fn entry_for_source_path(&self, source_path: &str) -> Option<&HitSideTableEntry> {
        self.entries.iter().find(|entry| {
            entry.source_path.as_deref() == Some(source_path)
                || entry
                    .source_routes
                    .iter()
                    .any(|route| route.source_path == source_path)
        })
    }

    pub fn bucket_indices(&self, bucket: HitSpatialBucket) -> Option<&Vec<usize>> {
        self.buckets.get(&hit_bucket_key(bucket))
    }

    pub fn candidate_indices_at(&self, x: f32, y: f32) -> Option<&Vec<usize>> {
        self.bucket_indices(hit_bucket_for_point(x, y, self.bucket_size))
    }
}

fn hit_bucket_for_point(x: f32, y: f32, bucket_size: f32) -> HitSpatialBucket {
    HitSpatialBucket {
        x: (x / bucket_size).floor() as i32,
        y: (y / bucket_size).floor() as i32,
    }
}

fn hit_bucket_key(bucket: HitSpatialBucket) -> String {
    format!("{},{}", bucket.x, bucket.y)
}

fn buckets_for_rect(rect: Rect, bucket_size: f32) -> Vec<HitSpatialBucket> {
    let min = hit_bucket_for_point(rect.x, rect.y, bucket_size);
    let max = hit_bucket_for_point(
        rect.x + rect.width.max(0.0),
        rect.y + rect.height.max(0.0),
        bucket_size,
    );
    let mut buckets = Vec::new();
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            buckets.push(HitSpatialBucket { x, y });
        }
    }
    buckets
}

fn compare_typed_hit_priority(
    left: &(usize, &HitSideTableEntry),
    right: &(usize, &HitSideTableEntry),
    x: f32,
    y: f32,
) -> std::cmp::Ordering {
    let left_live = left.1.source_path.is_some();
    let right_live = right.1.source_path.is_some();
    right_live
        .cmp(&left_live)
        .then_with(|| {
            rect_center_distance2(left.1.bounds, x, y).total_cmp(&rect_center_distance2(
                right.1.bounds,
                x,
                y,
            ))
        })
        .then_with(|| rect_area(left.1.bounds).total_cmp(&rect_area(right.1.bounds)))
        .then_with(|| right.1.z_depth.cmp(&left.1.z_depth))
        .then_with(|| right.0.cmp(&left.0))
}

fn rect_contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

fn rect_area(rect: Rect) -> f32 {
    rect.width.max(0.0) * rect.height.max(0.0)
}

fn rect_center_distance2(rect: Rect, x: f32, y: f32) -> f32 {
    let center_x = rect.x + rect.width / 2.0;
    let center_y = rect.y + rect.height / 2.0;
    let dx = center_x - x;
    let dy = center_y - y;
    dx * dx + dy * dy
}

fn scroll_root_for_node(document: &DocumentFrame, node: &DocumentNodeId) -> Option<ScrollRootId> {
    let mut current = Some(node.clone());
    while let Some(id) = current {
        let scroll_root = ScrollRootId(id.0.clone());
        if document.scroll_roots.contains_key(&scroll_root) {
            return Some(scroll_root);
        }
        current = document.nodes.get(&id).and_then(|node| node.parent.clone());
    }
    None
}

fn style_u64_any(style: &StyleMap, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| style_u64(style, key))
}

fn style_u64(style: &StyleMap, key: &str) -> Option<u64> {
    match style.get(key)? {
        StyleValue::Number(value) if *value >= 0.0 => Some(*value as u64),
        StyleValue::Text(value) => value.parse::<u64>().ok(),
        StyleValue::Bool(_)
        | StyleValue::Number(_)
        | StyleValue::RichTextSpans(_)
        | StyleValue::EditorTypeHints(_) => None,
    }
}

fn semantic_node_from_document_node(
    document: &DocumentFrame,
    node: &DocumentNode,
    item: Option<&DisplayItem>,
) -> SemanticNode {
    let id = SemanticId::from_document_node_id(&node.id);
    let checked = semantic_style_bool(&node.style, "checked");
    let focused = document.focus.as_ref() == Some(&node.id)
        || item.is_some_and(|item| item.focused)
        || semantic_style_bool(&node.style, "__focused") == Some(true)
        || semantic_style_bool(&node.style, "focus") == Some(true);
    let source_binding = node.primary_source_binding();
    let source_intent = source_binding.map(|binding| binding.intent.clone());
    let role = semantic_role_for_document_kind(&node.kind);
    let actions = semantic_actions_for_node(&node.kind, source_intent.as_deref());
    let value = semantic_value_for_node(node, item);
    SemanticNode {
        id,
        node: node.id.clone(),
        role,
        name: semantic_name_for_node(node, item),
        description: semantic_style_text_any(
            &node.style,
            &[
                "accessibility_description",
                "aria_description",
                "description",
            ],
        ),
        value,
        state: SemanticState {
            focused,
            checked,
            disabled: semantic_style_bool(&node.style, "disabled").unwrap_or(false),
            selected: semantic_style_bool(&node.style, "selected").unwrap_or(false),
        },
        actions,
        relations: SemanticRelations {
            parent: node.parent.as_ref().map(SemanticId::from_document_node_id),
            children: node
                .children
                .iter()
                .map(SemanticId::from_document_node_id)
                .collect(),
            controls: Vec::new(),
            labelled_by: Vec::new(),
            described_by: Vec::new(),
        },
        bounds: item.map(|item| item.bounds),
        language: semantic_style_text_any(&node.style, &["language", "lang"]),
        heading_level: style_u64(&node.style, "heading_level").and_then(|value| {
            u8::try_from(value)
                .ok()
                .filter(|value| (1..=6).contains(value))
        }),
        href: semantic_style_text_any(&node.style, &["href", "url"]),
        source_binding_id: source_binding.map(|binding| binding.id.clone()),
        source_path: source_binding.map(|binding| binding.source_path.clone()),
        source_intent,
    }
}

fn semantic_role_for_document_kind(kind: &DocumentNodeKind) -> SemanticRole {
    match kind {
        DocumentNodeKind::Root => SemanticRole::Application,
        DocumentNodeKind::Stack => SemanticRole::Group,
        DocumentNodeKind::Row => SemanticRole::Row,
        DocumentNodeKind::Text => SemanticRole::Text,
        DocumentNodeKind::Button => SemanticRole::Button,
        DocumentNodeKind::Checkbox => SemanticRole::Checkbox,
        DocumentNodeKind::TextInput => SemanticRole::TextInput,
        DocumentNodeKind::Table => SemanticRole::Table,
        DocumentNodeKind::TableCell => SemanticRole::Cell,
        DocumentNodeKind::ScrollRoot => SemanticRole::ScrollRegion,
    }
}

fn semantic_actions_for_node(
    kind: &DocumentNodeKind,
    source_intent: Option<&str>,
) -> SemanticActions {
    let press_intent = source_intent.is_some_and(|intent| {
        matches!(
            intent,
            "press" | "activate" | "toggle" | "submit" | "open" | "select"
        )
    });
    SemanticActions {
        focus: matches!(
            kind,
            DocumentNodeKind::Button | DocumentNodeKind::Checkbox | DocumentNodeKind::TextInput
        ) || press_intent,
        press: matches!(kind, DocumentNodeKind::Button | DocumentNodeKind::Checkbox)
            || press_intent,
        set_text: matches!(kind, DocumentNodeKind::TextInput),
        increment: false,
        decrement: false,
    }
}

fn semantic_name_for_node(node: &DocumentNode, item: Option<&DisplayItem>) -> Option<String> {
    semantic_style_text_any(
        &node.style,
        &[
            "accessibility_label",
            "aria_label",
            "label",
            "title",
            "placeholder",
        ],
    )
    .or_else(|| node.text.as_ref().map(|text| text.text.clone()))
    .or_else(|| item.and_then(|item| item.text.clone()))
}

fn semantic_value_for_node(
    node: &DocumentNode,
    item: Option<&DisplayItem>,
) -> Option<SemanticValue> {
    match node.kind {
        DocumentNodeKind::Checkbox => {
            semantic_style_bool(&node.style, "checked").map(|value| SemanticValue::Bool { value })
        }
        DocumentNodeKind::TextInput => node
            .text
            .as_ref()
            .map(|text| text.text.clone())
            .or_else(|| item.and_then(|item| item.text.clone()))
            .or_else(|| semantic_style_text_any(&node.style, &["value"]))
            .map(|text| SemanticValue::Text { text }),
        DocumentNodeKind::Text => node.text.as_ref().map(|text| SemanticValue::Text {
            text: text.text.clone(),
        }),
        _ => None,
    }
}

pub fn document_frame_from_world_editor_tree(
    tree: &boon_scene_model::WorldSemanticEditorTree,
) -> DocumentFrame {
    let root_id = document_node_id_from_world_editor_node_id(&tree.root);
    let mut frame = DocumentFrame::empty(root_id.0.clone());
    frame.focus = tree
        .focused
        .as_ref()
        .map(document_node_id_from_world_editor_node_id);

    if let Some(root) = tree.nodes.get(&tree.root) {
        if let Some(root_node) = frame.nodes.get_mut(&root_id) {
            root_node.style.insert(
                "accessibility_label".to_owned(),
                StyleValue::Text(root.label.clone()),
            );
            root_node.style.insert(
                "semantic_source".to_owned(),
                StyleValue::Text(root.id.0.clone()),
            );
            root_node.children = root
                .children
                .iter()
                .map(document_node_id_from_world_editor_node_id)
                .collect();
        }
    }

    for node in tree.nodes.values() {
        if node.id == tree.root {
            continue;
        }
        let id = document_node_id_from_world_editor_node_id(&node.id);
        let mut document_node =
            DocumentNode::new(id.0.clone(), document_kind_for_world_editor_node(node));
        document_node.parent =
            world_editor_parent_id(&node.id, tree).map(document_node_id_from_world_editor_node_id);
        document_node.children = node
            .children
            .iter()
            .map(document_node_id_from_world_editor_node_id)
            .collect();
        document_node.text = Some(TextValue {
            text: node.label.clone(),
        });
        document_node.style.insert(
            "accessibility_label".to_owned(),
            StyleValue::Text(node.label.clone()),
        );
        document_node.style.insert(
            "semantic_source".to_owned(),
            StyleValue::Text(node.id.0.clone()),
        );
        if node.selected {
            document_node
                .style
                .insert("selected".to_owned(), StyleValue::Bool(true));
        }
        if !node.visible {
            document_node
                .style
                .insert("visible".to_owned(), StyleValue::Bool(false));
        }
        if let Some(intent) = world_editor_source_intent(node) {
            let source_path = world_editor_source_path(node, &intent);
            document_node.set_primary_source_binding(boon_document_model::SourceBinding {
                id: SourceBindingId(format!("source:{source_path}:{intent}")),
                source_path,
                intent,
            });
        }
        frame.nodes.insert(id, document_node);
    }
    frame
}

fn document_kind_for_world_editor_node(
    node: &boon_scene_model::WorldSemanticEditorNode,
) -> DocumentNodeKind {
    match node.role {
        boon_scene_model::WorldSemanticEditorRole::Editor => DocumentNodeKind::Root,
        boon_scene_model::WorldSemanticEditorRole::Viewport
        | boon_scene_model::WorldSemanticEditorRole::Assembly
        | boon_scene_model::WorldSemanticEditorRole::Parameters
        | boon_scene_model::WorldSemanticEditorRole::Manufacturing => DocumentNodeKind::Stack,
        boon_scene_model::WorldSemanticEditorRole::PartInstance
        | boon_scene_model::WorldSemanticEditorRole::Parameter
        | boon_scene_model::WorldSemanticEditorRole::Action
            if node.actions.select || node.actions.edit_parameter || node.actions.export_3mf =>
        {
            DocumentNodeKind::Button
        }
        boon_scene_model::WorldSemanticEditorRole::PartInstance => DocumentNodeKind::Row,
        boon_scene_model::WorldSemanticEditorRole::Parameter
        | boon_scene_model::WorldSemanticEditorRole::Status => DocumentNodeKind::Text,
        boon_scene_model::WorldSemanticEditorRole::Action => DocumentNodeKind::Button,
    }
}

fn document_node_id_from_world_editor_node_id(
    node: &boon_scene_model::WorldSemanticEditorNodeId,
) -> DocumentNodeId {
    DocumentNodeId(format!("world-doc:{}", node.0))
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

fn semantic_style_text_any(style: &StyleMap, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match style.get(*key)? {
        StyleValue::Text(value) if !value.is_empty() => Some(value.clone()),
        StyleValue::Number(value) => Some(value.to_string()),
        StyleValue::Bool(value) => Some(value.to_string()),
        StyleValue::Text(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    })
}

fn semantic_style_bool(style: &StyleMap, key: &str) -> Option<bool> {
    match style.get(key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(value) => Some(*value != 0.0),
        StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollRegion {
    pub id: String,
    pub node: DocumentNodeId,
    pub axis: Axis,
    pub bounds: Rect,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityTree {
    pub node_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticDomSnapshot {
    pub root: Option<SemanticId>,
    pub nodes: Vec<SemanticDomNode>,
    pub metrics: SemanticDomMetrics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticDomNode {
    pub semantic_id: SemanticId,
    pub tag: String,
    pub role: Option<String>,
    pub attributes: BTreeMap<String, String>,
    pub text: Option<String>,
    pub focus_proxy: bool,
    pub source_binding_id: Option<SourceBindingId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDomMetrics {
    pub semantic_node_count: usize,
    pub dom_node_count: usize,
    pub interactive_node_count: usize,
    pub text_input_endpoint_count: usize,
    pub visual_dom_node_count: usize,
    pub data_boon_id_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticWebBridgeSnapshot {
    pub dom: SemanticDomSnapshot,
    pub ime_endpoints: Vec<SemanticWebImeEndpoint>,
    pub action_routes: Vec<SemanticWebActionRoute>,
    pub metrics: SemanticWebBridgeMetrics,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticWebImeEndpoint {
    pub semantic_id: SemanticId,
    pub node: DocumentNodeId,
    pub dom_id: String,
    pub value: String,
    pub source_binding_id: Option<SourceBindingId>,
    pub source_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticWebActionRoute {
    pub semantic_id: SemanticId,
    pub node: DocumentNodeId,
    pub action: SemanticWebAction,
    pub dom_event: String,
    pub source_binding_id: Option<SourceBindingId>,
    pub source_path: Option<String>,
    pub source_intent: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticWebAction {
    Focus,
    Press,
    SetText,
    Increment,
    Decrement,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticWebBridgeMetrics {
    pub semantic_node_count: usize,
    pub dom_node_count: usize,
    pub visual_dom_node_count: usize,
    pub ime_endpoint_count: usize,
    pub action_route_count: usize,
    pub source_routed_action_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticWebInputEvent {
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

pub type SemanticWebSourceDispatch = SemanticSourceDispatch;

pub fn semantic_scene_from_document_layout(
    document: &DocumentFrame,
    layout: &LayoutFrame,
) -> SemanticScene {
    let mut display_by_node = BTreeMap::new();
    for item in &layout.display_list {
        display_by_node
            .entry(item.node.clone())
            .or_insert_with(|| item.clone());
    }

    let mut scene = SemanticScene {
        root: document
            .nodes
            .contains_key(&document.root)
            .then(|| SemanticId::from_document_node_id(&document.root)),
        nodes: BTreeMap::new(),
        focused: None,
    };
    for node in document.nodes.values() {
        let item = display_by_node.get(&node.id);
        let semantic = semantic_node_from_document_node(document, node, item);
        if semantic.state.focused {
            scene.focused = Some(semantic.id.clone());
        }
        scene.nodes.insert(semantic.id.clone(), semantic);
    }
    scene
}

pub fn semantic_node_from_document_layout(
    document: &DocumentFrame,
    layout: &LayoutFrame,
    node_id: &DocumentNodeId,
) -> Option<SemanticNode> {
    let node = document.nodes.get(node_id)?;
    let item = layout
        .display_list
        .iter()
        .find(|item| item.node == *node_id);
    Some(semantic_node_from_document_node(document, node, item))
}

impl SemanticWebBridgeSnapshot {
    pub fn from_scene(scene: &SemanticScene) -> Self {
        let dom = SemanticDomSnapshot::from_scene(scene);
        let mut ime_endpoints = Vec::new();
        let mut action_routes = Vec::new();
        for node in scene.nodes.values() {
            let dom_id = semantic_web_dom_id(&node.id);
            if node.actions.focus || node.state.focused {
                action_routes.push(semantic_web_action_route(
                    node,
                    SemanticWebAction::Focus,
                    "focus",
                ));
            }
            if node.actions.press {
                action_routes.push(semantic_web_action_route(
                    node,
                    SemanticWebAction::Press,
                    "click",
                ));
            }
            if node.actions.set_text {
                ime_endpoints.push(SemanticWebImeEndpoint {
                    semantic_id: node.id.clone(),
                    node: node.node.clone(),
                    dom_id,
                    value: semantic_text_value(node).unwrap_or_default(),
                    source_binding_id: node.source_binding_id.clone(),
                    source_path: node.source_path.clone(),
                });
                action_routes.push(semantic_web_action_route(
                    node,
                    SemanticWebAction::SetText,
                    "input",
                ));
            }
            if node.actions.increment {
                action_routes.push(semantic_web_action_route(
                    node,
                    SemanticWebAction::Increment,
                    "input",
                ));
            }
            if node.actions.decrement {
                action_routes.push(semantic_web_action_route(
                    node,
                    SemanticWebAction::Decrement,
                    "input",
                ));
            }
        }
        let metrics = SemanticWebBridgeMetrics {
            semantic_node_count: scene.nodes.len(),
            dom_node_count: dom.metrics.dom_node_count,
            visual_dom_node_count: dom.metrics.visual_dom_node_count,
            ime_endpoint_count: ime_endpoints.len(),
            action_route_count: action_routes.len(),
            source_routed_action_count: action_routes
                .iter()
                .filter(|route| route.source_path.is_some())
                .count(),
        };
        Self {
            dom,
            ime_endpoints,
            action_routes,
            metrics,
        }
    }

    pub fn to_html_fragment(&self) -> String {
        self.dom.to_html_fragment()
    }

    pub fn source_dispatch_for_event(
        &self,
        event: SemanticWebInputEvent,
    ) -> Option<SemanticWebSourceDispatch> {
        let (semantic_id, action, text) = match event {
            SemanticWebInputEvent::Focus { semantic_id } => {
                (semantic_id, SemanticWebAction::Focus, None)
            }
            SemanticWebInputEvent::Press { semantic_id } => {
                (semantic_id, SemanticWebAction::Press, None)
            }
            SemanticWebInputEvent::SetText { semantic_id, text }
            | SemanticWebInputEvent::ReplaceSelectedText { semantic_id, text } => {
                (semantic_id, SemanticWebAction::SetText, Some(text))
            }
            SemanticWebInputEvent::Increment { semantic_id } => {
                (semantic_id, SemanticWebAction::Increment, None)
            }
            SemanticWebInputEvent::Decrement { semantic_id } => {
                (semantic_id, SemanticWebAction::Decrement, None)
            }
        };
        let route = self
            .action_routes
            .iter()
            .find(|route| route.semantic_id == semantic_id && route.action == action)?;
        Some(SemanticWebSourceDispatch {
            semantic_id: route.semantic_id.clone(),
            node: route.node.clone(),
            source_path: route.source_path.clone()?,
            source_intent: route.source_intent.clone(),
            text,
        })
    }
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

fn semantic_web_action_route(
    node: &SemanticNode,
    action: SemanticWebAction,
    dom_event: &str,
) -> SemanticWebActionRoute {
    let source_path = semantic_web_source_for_action(node, &action);
    let source_intent = source_path
        .as_ref()
        .and_then(|_| node.source_intent.clone());
    SemanticWebActionRoute {
        semantic_id: node.id.clone(),
        node: node.node.clone(),
        action,
        dom_event: dom_event.to_owned(),
        source_binding_id: source_path
            .as_ref()
            .and_then(|_| node.source_binding_id.clone()),
        source_path,
        source_intent,
    }
}

fn semantic_web_source_for_action(
    node: &SemanticNode,
    action: &SemanticWebAction,
) -> Option<String> {
    semantic_source_for_action(node, &semantic_action_from_web_action(action))
}

fn semantic_action_from_web_action(action: &SemanticWebAction) -> SemanticAction {
    match action {
        SemanticWebAction::Focus => SemanticAction::Focus,
        SemanticWebAction::Press => SemanticAction::Press,
        SemanticWebAction::SetText => SemanticAction::SetText,
        SemanticWebAction::Increment => SemanticAction::Increment,
        SemanticWebAction::Decrement => SemanticAction::Decrement,
    }
}

fn semantic_web_dom_id(id: &SemanticId) -> String {
    let mut dom_id = String::from("boon-");
    for character in id.0.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            dom_id.push(character);
        } else {
            dom_id.push('-');
        }
    }
    dom_id
}

impl SemanticDomSnapshot {
    pub fn from_scene(scene: &SemanticScene) -> Self {
        let mut nodes = Vec::with_capacity(scene.nodes.len());
        for node in scene.nodes.values() {
            nodes.push(SemanticDomNode::from_semantic_node(
                node,
                scene.focused.as_ref() == Some(&node.id),
            ));
        }
        let metrics = SemanticDomMetrics {
            semantic_node_count: scene.nodes.len(),
            dom_node_count: nodes.len(),
            interactive_node_count: nodes
                .iter()
                .filter(|node| {
                    node.attributes.contains_key("data-boon-action-press")
                        || node.attributes.contains_key("data-boon-action-set-text")
                        || node.attributes.contains_key("data-boon-action-increment")
                        || node.attributes.contains_key("data-boon-action-decrement")
                        || node.attributes.contains_key("tabindex")
                })
                .count(),
            text_input_endpoint_count: nodes
                .iter()
                .filter(|node| {
                    node.attributes.get("data-boon-ime-endpoint") == Some(&"true".to_owned())
                })
                .count(),
            visual_dom_node_count: 0,
            data_boon_id_count: nodes
                .iter()
                .filter(|node| node.attributes.contains_key("data-boon-id"))
                .count(),
        };
        Self {
            root: scene.root.clone(),
            nodes,
            metrics,
        }
    }

    pub fn to_html_fragment(&self) -> String {
        let mut html = String::new();
        for node in &self.nodes {
            node.push_html(&mut html);
        }
        html
    }
}

impl SemanticDomNode {
    fn from_semantic_node(node: &SemanticNode, focused: bool) -> Self {
        let mut attributes = BTreeMap::new();
        attributes.insert("data-boon-id".to_owned(), node.id.0.clone());
        attributes.insert("data-boon-node".to_owned(), node.node.0.clone());
        attributes.insert("id".to_owned(), semantic_web_dom_id(&node.id));
        if let Some(name) = &node.name {
            attributes.insert("aria-label".to_owned(), name.clone());
        }
        if let Some(description) = &node.description {
            attributes.insert("aria-description".to_owned(), description.clone());
        }
        if node.state.disabled {
            attributes.insert("aria-disabled".to_owned(), "true".to_owned());
        }
        if node.state.selected {
            attributes.insert("aria-selected".to_owned(), "true".to_owned());
        }
        if focused || node.state.focused {
            attributes.insert("data-boon-focused".to_owned(), "true".to_owned());
            attributes.insert("tabindex".to_owned(), "0".to_owned());
        }
        if node.actions.press {
            attributes.insert("data-boon-action-press".to_owned(), "true".to_owned());
        }
        if node.actions.set_text {
            attributes.insert("data-boon-action-set-text".to_owned(), "true".to_owned());
            attributes.insert("data-boon-ime-endpoint".to_owned(), "true".to_owned());
        }
        if node.actions.increment {
            attributes.insert("data-boon-action-increment".to_owned(), "true".to_owned());
        }
        if node.actions.decrement {
            attributes.insert("data-boon-action-decrement".to_owned(), "true".to_owned());
        }
        if let Some(binding) = &node.source_binding_id {
            attributes.insert("data-boon-source-binding-id".to_owned(), binding.0.clone());
        }
        if let Some(path) = &node.source_path {
            attributes.insert("data-boon-source-path".to_owned(), path.clone());
        }
        if let Some(intent) = &node.source_intent {
            attributes.insert("data-boon-source-intent".to_owned(), intent.clone());
        }
        if let Some(language) = &node.language {
            attributes.insert("lang".to_owned(), language.clone());
        }
        if let Some(level) = node.heading_level {
            attributes.insert("aria-level".to_owned(), level.to_string());
        }

        let (tag, role, text) = semantic_dom_shape(node, &mut attributes);
        Self {
            semantic_id: node.id.clone(),
            tag,
            role,
            attributes,
            text,
            focus_proxy: focused || node.state.focused || node.actions.set_text,
            source_binding_id: node.source_binding_id.clone(),
        }
    }

    fn push_html(&self, html: &mut String) {
        html.push('<');
        html.push_str(&self.tag);
        if let Some(role) = &self.role {
            push_html_attr(html, "role", role);
        }
        for (name, value) in &self.attributes {
            push_html_attr(html, name, value);
        }
        if self.tag == "input" {
            html.push_str(">");
            return;
        }
        html.push('>');
        if let Some(text) = &self.text {
            push_html_text(html, text);
        }
        html.push_str("</");
        html.push_str(&self.tag);
        html.push('>');
    }
}

fn semantic_dom_shape(
    node: &SemanticNode,
    attributes: &mut BTreeMap<String, String>,
) -> (String, Option<String>, Option<String>) {
    match node.role {
        SemanticRole::Application => (
            "main".to_owned(),
            Some("application".to_owned()),
            node.name.clone(),
        ),
        SemanticRole::Group => (
            "section".to_owned(),
            Some("group".to_owned()),
            node.name.clone(),
        ),
        SemanticRole::Row => ("div".to_owned(), Some("row".to_owned()), node.name.clone()),
        SemanticRole::Text => ("span".to_owned(), None, semantic_text_value(node)),
        SemanticRole::Button => ("button".to_owned(), None, node.name.clone()),
        SemanticRole::Checkbox => {
            attributes.insert("type".to_owned(), "checkbox".to_owned());
            let checked = node.state.checked.unwrap_or(false);
            attributes.insert("aria-checked".to_owned(), checked.to_string());
            if checked {
                attributes.insert("checked".to_owned(), "checked".to_owned());
            }
            ("input".to_owned(), None, None)
        }
        SemanticRole::TextInput => {
            attributes.insert("type".to_owned(), "text".to_owned());
            if let Some(text) = semantic_text_value(node) {
                attributes.insert("value".to_owned(), text);
            }
            ("input".to_owned(), None, None)
        }
        SemanticRole::Table => ("table".to_owned(), None, node.name.clone()),
        SemanticRole::Cell => ("div".to_owned(), Some("cell".to_owned()), node.name.clone()),
        SemanticRole::ScrollRegion => (
            "section".to_owned(),
            Some("region".to_owned()),
            node.name.clone(),
        ),
    }
}

fn semantic_text_value(node: &SemanticNode) -> Option<String> {
    match &node.value {
        Some(SemanticValue::Text { text }) => Some(text.clone()),
        Some(SemanticValue::Bool { value }) => Some(value.to_string()),
        Some(SemanticValue::Number { value }) => Some(value.to_string()),
        None => node.name.clone(),
    }
}

fn push_html_attr(html: &mut String, name: &str, value: &str) {
    html.push(' ');
    html.push_str(name);
    html.push_str("=\"");
    push_html_attr_value(html, value);
    html.push('"');
}

fn push_html_attr_value(html: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => html.push_str("&amp;"),
            '"' => html.push_str("&quot;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            _ => html.push(ch),
        }
    }
}

fn push_html_text(html: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            _ => html.push(ch),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutDemand {
    pub node: DocumentNodeId,
    pub axis: Axis,
    pub visible: Range<u64>,
    pub overscan: Range<u64>,
    pub logical_item_count: u64,
    pub materialized_item_count: u64,
    pub stable_key_prefix: String,
    pub first_stable_key: Option<String>,
    pub last_stable_key: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaterializationReport {
    pub node: DocumentNodeId,
    pub axis: Axis,
    pub visible: Range<u64>,
    pub overscan: Range<u64>,
    pub logical_item_count: u64,
    pub materialized_item_count: u64,
    pub stable_key_prefix: String,
    pub first_stable_key: Option<String>,
    pub last_stable_key: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LayoutMetrics {
    pub node_count: usize,
    pub display_item_count: usize,
    pub materialized_range_count: usize,
    pub native_capability_required: bool,
}

#[derive(Clone, Debug)]
pub struct DocumentState {
    frame: DocumentFrame,
}

macro_rules! document_numeric_ids {
    ($ty:ty; $($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            pub struct $name(pub $ty);
        )+
    };
}

document_numeric_ids!(u32; DocumentHotNodeId);
document_numeric_ids!(u64; DocumentHotNodeGeneration);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DocumentHotNodeRef {
    pub id: DocumentHotNodeId,
    pub generation: DocumentHotNodeGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentDebugNameTable {
    pub node_names: BTreeMap<DocumentHotNodeId, DocumentNodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentHotIdTable {
    pub root: DocumentHotNodeId,
    pub ids_by_node: BTreeMap<DocumentNodeId, DocumentHotNodeId>,
    pub generations: BTreeMap<DocumentHotNodeId, DocumentHotNodeGeneration>,
    pub debug_names: DocumentDebugNameTable,
    pub next_id: u32,
}

document_numeric_ids!(u32; DocumentInternId);

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentInternTable {
    pub ids_by_key: BTreeMap<String, DocumentInternId>,
    pub keys_by_id: BTreeMap<DocumentInternId, String>,
    pub next_id: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentInternedNode {
    pub node: DocumentHotNodeRef,
    pub text: Option<DocumentInternId>,
    pub layout_style: DocumentInternId,
    pub paint_style: DocumentInternId,
    pub text_style: DocumentInternId,
    pub material: DocumentInternId,
    pub clip: DocumentInternId,
    pub source_bindings: Vec<DocumentInternId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentInternIndex {
    pub texts: DocumentInternTable,
    pub layout_styles: DocumentInternTable,
    pub paint_styles: DocumentInternTable,
    pub text_styles: DocumentInternTable,
    pub materials: DocumentInternTable,
    pub clips: DocumentInternTable,
    pub source_bindings: DocumentInternTable,
    pub nodes: BTreeMap<DocumentHotNodeId, DocumentInternedNode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentMaterializationWindowKey {
    pub axis: Axis,
    pub visible: Range<u64>,
    pub overscan: Range<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutKey {
    pub kind: DocumentNodeKind,
    pub layout_style: DocumentInternId,
    pub text_style: DocumentInternId,
    pub text: Option<DocumentInternId>,
    pub children: Vec<DocumentHotNodeId>,
    pub materialized: Vec<DocumentMaterializationWindowKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutEntry {
    pub node: DocumentHotNodeRef,
    pub key: DocumentRetainedLayoutKey,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutKeyTable {
    pub entries: BTreeMap<DocumentHotNodeId, DocumentRetainedLayoutEntry>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentRetainedLayoutDirtyReason {
    Added,
    Removed,
    Geometry,
    Kind,
    LayoutStyle,
    TextStyle,
    Text,
    Children,
    Materialization,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutDirtyEntry {
    pub node: DocumentHotNodeId,
    pub previous: Option<DocumentHotNodeRef>,
    pub current: Option<DocumentHotNodeRef>,
    pub reasons: Vec<DocumentRetainedLayoutDirtyReason>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutDelta {
    pub reused: Vec<DocumentHotNodeRef>,
    pub dirty: Vec<DocumentRetainedLayoutDirtyEntry>,
    pub removed: Vec<DocumentRetainedLayoutDirtyEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutGeometry {
    pub bounds: Rect,
    pub display_index: usize,
    pub hit_region_count: usize,
    pub scroll_region_count: usize,
    pub materialization_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutCacheEntry {
    pub node: DocumentHotNodeRef,
    pub key: DocumentRetainedLayoutKey,
    pub geometry: DocumentRetainedLayoutGeometry,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutCache {
    pub entries: BTreeMap<DocumentHotNodeId, DocumentRetainedLayoutCacheEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutCacheUpdate {
    pub cache: DocumentRetainedLayoutCache,
    pub delta: DocumentRetainedLayoutDelta,
    pub refreshed: Vec<DocumentHotNodeRef>,
    pub patch: DocumentRetainedLayoutPatch,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentRetainedLayoutPatch {
    pub operations: Vec<DocumentRetainedLayoutPatchOperation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DocumentRetainedLayoutPatchOperation {
    ReuseGeometry {
        node: DocumentHotNodeRef,
    },
    UpsertGeometry {
        node: DocumentHotNodeRef,
        key: DocumentRetainedLayoutKey,
        geometry: DocumentRetainedLayoutGeometry,
        reasons: Vec<DocumentRetainedLayoutDirtyReason>,
    },
    RemoveGeometry {
        node: DocumentHotNodeRef,
        reasons: Vec<DocumentRetainedLayoutDirtyReason>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentStyleDimension {
    Px { value: f32 },
    Fill,
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedEdgeSpacing {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedLayoutStyle {
    pub width: Option<DocumentStyleDimension>,
    pub height: Option<DocumentStyleDimension>,
    pub min_width: Option<DocumentStyleDimension>,
    pub max_width: Option<DocumentStyleDimension>,
    pub min_height: Option<DocumentStyleDimension>,
    pub max_height: Option<DocumentStyleDimension>,
    pub gap: Option<f32>,
    pub size: Option<f32>,
    pub box_size: Option<f32>,
    pub auto_padding: Option<f32>,
    pub center: bool,
    pub align_x: Option<String>,
    pub overlay_children: bool,
    pub placeholder: Option<String>,
    pub padding: DocumentTypedEdgeSpacing,
    pub clip: Option<Rect>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedPaintStyle {
    pub color: Option<String>,
    pub background: Option<String>,
    pub background_color: Option<String>,
    pub border_color: Option<String>,
    pub opacity: Option<f32>,
    pub relief: Option<String>,
    pub depth: Option<f32>,
    pub shadow: Option<String>,
    pub outline: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedTextStyle {
    pub size: Option<f32>,
    pub font: Option<String>,
    pub font_family: Option<String>,
    pub font_weight: Option<String>,
    pub font_style: Option<String>,
    pub line_height: Option<f32>,
    pub letter_spacing: Option<f32>,
    pub text_align: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedMaterialStyle {
    pub material: Option<String>,
    pub texture: Option<String>,
    pub image: Option<String>,
    pub shader: Option<String>,
    pub border_radius: Option<f32>,
    pub clip: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedPseudoStyle {
    pub hover_scope: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedStyleRecord {
    pub node: DocumentHotNodeRef,
    pub identity: ComputedStyleIdentity,
    pub layout: DocumentTypedLayoutStyle,
    pub paint: DocumentTypedPaintStyle,
    pub text: DocumentTypedTextStyle,
    pub material: DocumentTypedMaterialStyle,
    pub pseudo: DocumentTypedPseudoStyle,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedStyleIndex {
    pub records: BTreeMap<DocumentHotNodeId, DocumentTypedStyleRecord>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DocumentTypedBindingRef {
    pub node: DocumentHotNodeId,
    pub ordinal: u32,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DocumentTypedBindingRoute {
    pub source_path: String,
    pub intent: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedBinding {
    pub node: DocumentHotNodeRef,
    pub reference: DocumentTypedBindingRef,
    pub binding_id: SourceBindingId,
    pub route: DocumentTypedBindingRoute,
    pub intern_id: DocumentInternId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedBindingNode {
    pub node: DocumentHotNodeRef,
    pub bindings: Vec<DocumentTypedBinding>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentTypedBindingIndex {
    pub nodes: BTreeMap<DocumentHotNodeId, DocumentTypedBindingNode>,
    pub by_binding_id: BTreeMap<SourceBindingId, Vec<DocumentTypedBindingRef>>,
    pub by_route: BTreeMap<DocumentTypedBindingRoute, Vec<DocumentTypedBindingRef>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentDerivedIndexBundle {
    pub hot_ids: DocumentHotIdTable,
    pub intern_index: DocumentInternIndex,
    pub retained_layout_keys: DocumentRetainedLayoutKeyTable,
    pub typed_styles: DocumentTypedStyleIndex,
    pub typed_bindings: DocumentTypedBindingIndex,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentChangeBatch {
    pub patches: Vec<DocumentPatch>,
}

impl From<ChangeBatch<UiSemanticChange>> for DocumentChangeBatch {
    fn from(batch: ChangeBatch<UiSemanticChange>) -> Self {
        let document_batch: ChangeBatch<DocumentPatch> = batch.into();
        Self {
            patches: document_batch.changes,
        }
    }
}

impl From<ChangeBatch<DocumentPatch>> for DocumentChangeBatch {
    fn from(batch: ChangeBatch<DocumentPatch>) -> Self {
        Self {
            patches: batch.changes,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DocumentChangeSet {
    pub patch_count: usize,
    pub reports: Vec<PatchApplyReport>,
    pub targets: Vec<DocumentNodeId>,
    pub invalidation: Vec<PatchInvalidationClass>,
    pub removed_nodes: Vec<DocumentNodeId>,
    pub node_count_before: usize,
    pub node_count_after: usize,
    pub materialization: Vec<MaterializationReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchInvalidationClass {
    Structure,
    Text,
    Style,
    Binding,
    Scroll,
    Materialization,
    Layout,
    HitRegion,
    PaintOnly,
    LayoutOnly,
    SourceBinding,
    ListStructure,
    ConditionalStructure,
    ScrollOffsetOnly,
    MaterializationOnly,
    FullDocument,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PatchApplyReport {
    pub patch_kind: &'static str,
    pub target: Option<DocumentNodeId>,
    pub invalidation: Vec<PatchInvalidationClass>,
    pub removed_nodes: Vec<DocumentNodeId>,
    pub node_count_after: usize,
    pub materialization: Option<MaterializationReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatchApplyError {
    MissingTarget {
        patch_kind: &'static str,
        id: DocumentNodeId,
    },
    MissingParent {
        id: DocumentNodeId,
        parent: DocumentNodeId,
    },
    CannotRemoveRoot {
        id: DocumentNodeId,
    },
    DuplicateChild {
        parent: DocumentNodeId,
        child: DocumentNodeId,
    },
    OrphanedChild {
        parent: DocumentNodeId,
        child: DocumentNodeId,
    },
    InvalidParentChildLink {
        parent: DocumentNodeId,
        child: DocumentNodeId,
        actual_parent: Option<DocumentNodeId>,
    },
    ChildIndexOutOfBounds {
        parent: DocumentNodeId,
        index: usize,
        child_count: usize,
    },
    OrphanedNode {
        id: DocumentNodeId,
        parent: Option<DocumentNodeId>,
    },
    Cycle {
        id: DocumentNodeId,
    },
    StaleReference {
        reference_kind: &'static str,
        id: DocumentNodeId,
    },
    UnsupportedTrustedNonstructuralPatch {
        patch_kind: &'static str,
    },
}

impl fmt::Display for PatchApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTarget { patch_kind, id } => {
                write!(f, "{patch_kind} target `{}` does not exist", id.0)
            }
            Self::MissingParent { id, parent } => {
                write!(f, "node `{}` parent `{}` does not exist", id.0, parent.0)
            }
            Self::CannotRemoveRoot { id } => write!(f, "cannot remove root node `{}`", id.0),
            Self::DuplicateChild { parent, child } => write!(
                f,
                "node `{}` lists child `{}` more than once",
                parent.0, child.0
            ),
            Self::OrphanedChild { parent, child } => {
                write!(
                    f,
                    "node `{}` references missing child `{}`",
                    parent.0, child.0
                )
            }
            Self::InvalidParentChildLink {
                parent,
                child,
                actual_parent,
            } => write!(
                f,
                "node `{}` references child `{}` whose parent is {:?}",
                parent.0,
                child.0,
                actual_parent.as_ref().map(|id| id.0.as_str())
            ),
            Self::ChildIndexOutOfBounds {
                parent,
                index,
                child_count,
            } => write!(
                f,
                "node `{}` cannot insert child at index {} with {} children",
                parent.0, index, child_count
            ),
            Self::OrphanedNode { id, parent } => write!(
                f,
                "node `{}` has parent {:?} but is not reachable from the root",
                id.0,
                parent.as_ref().map(|id| id.0.as_str())
            ),
            Self::Cycle { id } => write!(f, "node `{}` participates in a parent cycle", id.0),
            Self::StaleReference { reference_kind, id } => {
                write!(f, "{reference_kind} references stale node `{}`", id.0)
            }
            Self::UnsupportedTrustedNonstructuralPatch { patch_kind } => write!(
                f,
                "{patch_kind} cannot be applied through trusted nonstructural document patching"
            ),
        }
    }
}

impl std::error::Error for PatchApplyError {}

impl DocumentHotIdTable {
    pub fn from_frame(frame: &DocumentFrame) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(frame)?;
        let mut ids_by_node = BTreeMap::new();
        let mut node_names = BTreeMap::new();
        let mut generations = BTreeMap::new();
        let root = DocumentHotNodeId(0);
        ids_by_node.insert(frame.root.clone(), root);
        node_names.insert(root, frame.root.clone());
        generations.insert(root, DocumentHotNodeGeneration(1));
        let mut next = 1_u32;
        for id in frame.nodes.keys().filter(|id| *id != &frame.root) {
            let hot = DocumentHotNodeId(next);
            next = next.saturating_add(1);
            ids_by_node.insert(id.clone(), hot);
            node_names.insert(hot, id.clone());
            generations.insert(hot, DocumentHotNodeGeneration(1));
        }
        Ok(Self {
            root,
            ids_by_node,
            generations,
            debug_names: DocumentDebugNameTable { node_names },
            next_id: next,
        })
    }

    pub fn from_previous_frames(
        previous: &Self,
        previous_frame: &DocumentFrame,
        frame: &DocumentFrame,
    ) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(previous_frame)?;
        validate_frame_integrity(frame)?;

        let mut ids_by_node = BTreeMap::new();
        let mut node_names = BTreeMap::new();
        let mut generations = BTreeMap::new();
        let root = DocumentHotNodeId(0);
        let mut next = previous.next_id.max(
            previous
                .debug_names
                .node_names
                .keys()
                .map(|id| id.0.saturating_add(1))
                .max()
                .unwrap_or(1),
        );

        let assign =
            |id: &DocumentNodeId,
             hot: DocumentHotNodeId,
             next: &mut u32,
             ids_by_node: &mut BTreeMap<DocumentNodeId, DocumentHotNodeId>,
             node_names: &mut BTreeMap<DocumentHotNodeId, DocumentNodeId>,
             generations: &mut BTreeMap<DocumentHotNodeId, DocumentHotNodeGeneration>| {
                let current_node = frame
                    .nodes
                    .get(id)
                    .expect("validated frame node key should resolve");
                let previous_generation = previous
                    .generations
                    .get(&hot)
                    .copied()
                    .unwrap_or(DocumentHotNodeGeneration(1));
                let generation = match previous_frame.nodes.get(id) {
                    Some(previous_node) if previous_node == current_node => previous_generation,
                    Some(_) => DocumentHotNodeGeneration(previous_generation.0.saturating_add(1)),
                    None => DocumentHotNodeGeneration(1),
                };

                ids_by_node.insert(id.clone(), hot);
                node_names.insert(hot, id.clone());
                generations.insert(hot, generation);
                *next = (*next).max(hot.0.saturating_add(1));
            };

        assign(
            &frame.root,
            root,
            &mut next,
            &mut ids_by_node,
            &mut node_names,
            &mut generations,
        );

        for id in frame.nodes.keys().filter(|id| *id != &frame.root) {
            let hot = previous.ids_by_node.get(id).copied().unwrap_or_else(|| {
                let hot = DocumentHotNodeId(next);
                next = next.saturating_add(1);
                hot
            });
            assign(
                id,
                hot,
                &mut next,
                &mut ids_by_node,
                &mut node_names,
                &mut generations,
            );
        }

        Ok(Self {
            root,
            ids_by_node,
            generations,
            debug_names: DocumentDebugNameTable { node_names },
            next_id: next,
        })
    }

    pub fn hot_id(&self, id: &DocumentNodeId) -> Option<DocumentHotNodeId> {
        self.ids_by_node.get(id).copied()
    }

    pub fn hot_ref(&self, id: &DocumentNodeId) -> Option<DocumentHotNodeRef> {
        let id = self.hot_id(id)?;
        let generation = self.generation(id)?;
        Some(DocumentHotNodeRef { id, generation })
    }

    pub fn generation(&self, id: DocumentHotNodeId) -> Option<DocumentHotNodeGeneration> {
        self.generations.get(&id).copied()
    }

    pub fn debug_name(&self, id: DocumentHotNodeId) -> Option<&DocumentNodeId> {
        self.debug_names.node_names.get(&id)
    }
}

impl DocumentInternTable {
    pub fn intern(&mut self, key: String) -> DocumentInternId {
        if let Some(id) = self.ids_by_key.get(&key) {
            return *id;
        }
        let id = DocumentInternId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.ids_by_key.insert(key.clone(), id);
        self.keys_by_id.insert(id, key);
        id
    }

    pub fn key(&self, id: DocumentInternId) -> Option<&str> {
        self.keys_by_id.get(&id).map(String::as_str)
    }
}

impl DocumentInternIndex {
    pub fn from_frame(
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
    ) -> Result<Self, PatchApplyError> {
        Self::from_seeded_frame(Self::default(), frame, hot_ids)
    }

    pub fn from_previous_frame(
        previous: &Self,
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
    ) -> Result<Self, PatchApplyError> {
        let mut index = previous.clone();
        index.nodes.clear();
        Self::from_seeded_frame(index, frame, hot_ids)
    }

    fn from_seeded_frame(
        mut index: Self,
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
    ) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(frame)?;
        for (node_id, node) in &frame.nodes {
            let hot_ref =
                hot_ids
                    .hot_ref(node_id)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "hot_id_table",
                        id: node_id.clone(),
                    })?;
            index.update_node(node, hot_ref);
        }
        Ok(index)
    }

    fn update_node(&mut self, node: &DocumentNode, hot_ref: DocumentHotNodeRef) {
        let text = node
            .text
            .as_ref()
            .map(|text| self.texts.intern(text.text.clone()));
        let layout_style = self.layout_styles.intern(stable_style_intern_key(
            &node.style,
            StyleHashCategory::Layout,
        ));
        let paint_style = self.paint_styles.intern(stable_style_intern_key(
            &node.style,
            StyleHashCategory::Paint,
        ));
        let text_style = self.text_styles.intern(stable_style_intern_key(
            &node.style,
            StyleHashCategory::Font,
        ));
        let material = self.materials.intern(stable_style_intern_key(
            &node.style,
            StyleHashCategory::Material,
        ));
        let clip = self.clips.intern(stable_style_intern_key(
            &node.style,
            StyleHashCategory::Clip,
        ));
        let source_bindings = node
            .source_bindings()
            .map(|binding| {
                self.source_bindings.intern(stable_source_binding_key(
                    &binding.id.0,
                    &binding.source_path,
                    &binding.intent,
                ))
            })
            .collect::<Vec<_>>();
        self.nodes.insert(
            hot_ref.id,
            DocumentInternedNode {
                node: hot_ref,
                text,
                layout_style,
                paint_style,
                text_style,
                material,
                clip,
                source_bindings,
            },
        );
    }
}

impl DocumentRetainedLayoutKeyTable {
    pub fn from_frame(
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
        intern_index: &DocumentInternIndex,
    ) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(frame)?;
        let mut table = Self::default();
        for (node_id, node) in &frame.nodes {
            let node_ref =
                hot_ids
                    .hot_ref(node_id)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "hot_id_table",
                        id: node_id.clone(),
                    })?;
            let interned = intern_index.nodes.get(&node_ref.id).ok_or_else(|| {
                PatchApplyError::StaleReference {
                    reference_kind: "document_intern_index",
                    id: node_id.clone(),
                }
            })?;
            let mut children = Vec::with_capacity(node.children.len());
            for child in &node.children {
                children.push(hot_ids.hot_id(child).ok_or_else(|| {
                    PatchApplyError::StaleReference {
                        reference_kind: "hot_id_table_child",
                        id: child.clone(),
                    }
                })?);
            }
            let materialized = node
                .materialized
                .iter()
                .map(|range| DocumentMaterializationWindowKey {
                    axis: range.axis,
                    visible: range.visible.clone(),
                    overscan: range.overscan.clone(),
                })
                .collect();
            table.entries.insert(
                node_ref.id,
                DocumentRetainedLayoutEntry {
                    node: node_ref,
                    key: DocumentRetainedLayoutKey {
                        kind: node.kind.clone(),
                        layout_style: interned.layout_style,
                        text_style: interned.text_style,
                        text: interned.text,
                        children,
                        materialized,
                    },
                },
            );
        }
        Ok(table)
    }

    fn update_node(
        &mut self,
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
        intern_index: &DocumentInternIndex,
        node_id: &DocumentNodeId,
    ) -> Result<(), PatchApplyError> {
        let node = frame
            .nodes
            .get(node_id)
            .ok_or_else(|| PatchApplyError::StaleReference {
                reference_kind: "document_frame",
                id: node_id.clone(),
            })?;
        let node_ref = hot_ids
            .hot_ref(node_id)
            .ok_or_else(|| PatchApplyError::StaleReference {
                reference_kind: "hot_id_table",
                id: node_id.clone(),
            })?;
        let interned = intern_index.nodes.get(&node_ref.id).ok_or_else(|| {
            PatchApplyError::StaleReference {
                reference_kind: "document_intern_index",
                id: node_id.clone(),
            }
        })?;
        let mut children = Vec::with_capacity(node.children.len());
        for child in &node.children {
            children.push(hot_ids.hot_id(child).ok_or_else(|| {
                PatchApplyError::StaleReference {
                    reference_kind: "hot_id_table_child",
                    id: child.clone(),
                }
            })?);
        }
        let materialized = node
            .materialized
            .iter()
            .map(|range| DocumentMaterializationWindowKey {
                axis: range.axis,
                visible: range.visible.clone(),
                overscan: range.overscan.clone(),
            })
            .collect();
        self.entries.insert(
            node_ref.id,
            DocumentRetainedLayoutEntry {
                node: node_ref,
                key: DocumentRetainedLayoutKey {
                    kind: node.kind.clone(),
                    layout_style: interned.layout_style,
                    text_style: interned.text_style,
                    text: interned.text,
                    children,
                    materialized,
                },
            },
        );
        Ok(())
    }

    pub fn entry(&self, id: DocumentHotNodeId) -> Option<&DocumentRetainedLayoutEntry> {
        self.entries.get(&id)
    }

    pub fn diff_from(&self, previous: &Self) -> DocumentRetainedLayoutDelta {
        let mut delta = DocumentRetainedLayoutDelta::default();
        for (id, current) in &self.entries {
            match previous.entries.get(id) {
                Some(previous_entry) => {
                    let reasons = retained_layout_dirty_reasons(&previous_entry.key, &current.key);
                    if reasons.is_empty() {
                        delta.reused.push(current.node);
                    } else {
                        delta.dirty.push(DocumentRetainedLayoutDirtyEntry {
                            node: *id,
                            previous: Some(previous_entry.node),
                            current: Some(current.node),
                            reasons,
                        });
                    }
                }
                None => delta.dirty.push(DocumentRetainedLayoutDirtyEntry {
                    node: *id,
                    previous: None,
                    current: Some(current.node),
                    reasons: vec![DocumentRetainedLayoutDirtyReason::Added],
                }),
            }
        }
        for (id, previous_entry) in &previous.entries {
            if !self.entries.contains_key(id) {
                delta.removed.push(DocumentRetainedLayoutDirtyEntry {
                    node: *id,
                    previous: Some(previous_entry.node),
                    current: None,
                    reasons: vec![DocumentRetainedLayoutDirtyReason::Removed],
                });
            }
        }
        delta
    }
}

fn retained_layout_dirty_reasons(
    previous: &DocumentRetainedLayoutKey,
    current: &DocumentRetainedLayoutKey,
) -> Vec<DocumentRetainedLayoutDirtyReason> {
    let mut reasons = Vec::new();
    if previous.kind != current.kind {
        reasons.push(DocumentRetainedLayoutDirtyReason::Kind);
    }
    if previous.layout_style != current.layout_style {
        reasons.push(DocumentRetainedLayoutDirtyReason::LayoutStyle);
    }
    if previous.text_style != current.text_style {
        reasons.push(DocumentRetainedLayoutDirtyReason::TextStyle);
    }
    if previous.text != current.text {
        reasons.push(DocumentRetainedLayoutDirtyReason::Text);
    }
    if previous.children != current.children {
        reasons.push(DocumentRetainedLayoutDirtyReason::Children);
    }
    if previous.materialized != current.materialized {
        reasons.push(DocumentRetainedLayoutDirtyReason::Materialization);
    }
    reasons
}

impl DocumentRetainedLayoutCache {
    pub fn from_layout_frame(
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
        key_table: &DocumentRetainedLayoutKeyTable,
        layout: &LayoutFrame,
    ) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(frame)?;
        let mut cache = Self::default();
        for (index, item) in layout.display_list.iter().enumerate() {
            let node_id = &item.node;
            let node = frame
                .nodes
                .get(node_id)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "layout_frame_display_item",
                    id: node_id.clone(),
                })?;
            let node_ref =
                hot_ids
                    .hot_ref(node_id)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "hot_id_table",
                        id: node_id.clone(),
                    })?;
            let key_entry =
                key_table
                    .entry(node_ref.id)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "retained_layout_key_table",
                        id: node_id.clone(),
                    })?;
            cache.entries.insert(
                node_ref.id,
                DocumentRetainedLayoutCacheEntry {
                    node: node_ref,
                    key: key_entry.key.clone(),
                    geometry: DocumentRetainedLayoutGeometry {
                        bounds: item.bounds,
                        display_index: index,
                        hit_region_count: layout
                            .hit_regions
                            .iter()
                            .filter(|hit| hit.node == node.id)
                            .count(),
                        scroll_region_count: layout
                            .scroll_regions
                            .iter()
                            .filter(|scroll| scroll.node == node.id)
                            .count(),
                        materialization_count: layout
                            .materialization
                            .iter()
                            .filter(|report| report.node == node.id)
                            .count(),
                    },
                },
            );
        }
        Ok(cache)
    }

    pub fn update_from_layout_frame(
        &self,
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
        key_table: &DocumentRetainedLayoutKeyTable,
        layout: &LayoutFrame,
    ) -> Result<DocumentRetainedLayoutCacheUpdate, PatchApplyError> {
        let previous_keys = self.key_table();
        let measured = Self::from_layout_frame(frame, hot_ids, key_table, layout)?;
        let delta = measured.key_table().diff_from(&previous_keys);
        let mut cache = Self::default();
        let mut refreshed = Vec::new();
        let mut patch = DocumentRetainedLayoutPatch::default();
        for (id, measured_entry) in measured.entries {
            if delta.reused.iter().any(|entry| entry.id == id) {
                if let Some(previous_entry) = self.entries.get(&id) {
                    patch
                        .operations
                        .push(DocumentRetainedLayoutPatchOperation::ReuseGeometry {
                            node: measured_entry.node,
                        });
                    cache.entries.insert(
                        id,
                        DocumentRetainedLayoutCacheEntry {
                            node: measured_entry.node,
                            key: measured_entry.key,
                            geometry: previous_entry.geometry.clone(),
                        },
                    );
                    continue;
                }
            }
            let reasons = delta
                .dirty
                .iter()
                .find(|entry| entry.node == id)
                .map(|entry| entry.reasons.clone())
                .unwrap_or_default();
            refreshed.push(measured_entry.node);
            patch
                .operations
                .push(DocumentRetainedLayoutPatchOperation::UpsertGeometry {
                    node: measured_entry.node,
                    key: measured_entry.key.clone(),
                    geometry: measured_entry.geometry.clone(),
                    reasons,
                });
            cache.entries.insert(id, measured_entry);
        }
        for removed in &delta.removed {
            if let Some(previous) = removed.previous {
                patch
                    .operations
                    .push(DocumentRetainedLayoutPatchOperation::RemoveGeometry {
                        node: previous,
                        reasons: removed.reasons.clone(),
                    });
            }
        }
        Ok(DocumentRetainedLayoutCacheUpdate {
            cache,
            delta,
            refreshed,
            patch,
        })
    }

    pub fn update_nodes_from_layout_frame(
        &self,
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
        key_table: &DocumentRetainedLayoutKeyTable,
        layout: &LayoutFrame,
        changed_nodes: &BTreeSet<DocumentNodeId>,
    ) -> Result<Option<DocumentRetainedLayoutCacheUpdate>, PatchApplyError> {
        if changed_nodes.is_empty() {
            let mut delta = DocumentRetainedLayoutDelta::default();
            delta
                .reused
                .extend(self.entries.values().map(|entry| entry.node));
            return Ok(Some(DocumentRetainedLayoutCacheUpdate {
                cache: self.clone(),
                delta,
                refreshed: Vec::new(),
                patch: DocumentRetainedLayoutPatch::default(),
            }));
        }

        let mut cache = self.clone();
        let mut delta = DocumentRetainedLayoutDelta {
            reused: self.entries.values().map(|entry| entry.node).collect(),
            dirty: Vec::new(),
            removed: Vec::new(),
        };
        let mut refreshed = Vec::new();
        let mut patch = DocumentRetainedLayoutPatch::default();

        for node_id in changed_nodes {
            let hot_ref =
                hot_ids
                    .hot_ref(node_id)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "hot_id_table",
                        id: node_id.clone(),
                    })?;
            let Some(measured_entry) =
                retained_layout_cache_entry_for_node(frame, hot_ids, key_table, layout, node_id)?
            else {
                return Ok(None);
            };
            let previous_entry = self.entries.get(&hot_ref.id);
            let mut reasons = previous_entry
                .map(|previous| retained_layout_dirty_reasons(&previous.key, &measured_entry.key))
                .unwrap_or_else(|| vec![DocumentRetainedLayoutDirtyReason::Added]);
            if previous_entry.is_some_and(|previous| previous.geometry != measured_entry.geometry)
                && !reasons.contains(&DocumentRetainedLayoutDirtyReason::Geometry)
            {
                reasons.push(DocumentRetainedLayoutDirtyReason::Geometry);
            }
            if reasons.is_empty() {
                continue;
            }

            delta.reused.retain(|entry| entry.id != hot_ref.id);
            delta.dirty.push(DocumentRetainedLayoutDirtyEntry {
                node: hot_ref.id,
                previous: previous_entry.map(|entry| entry.node),
                current: Some(measured_entry.node),
                reasons: reasons.clone(),
            });
            refreshed.push(measured_entry.node);
            patch
                .operations
                .push(DocumentRetainedLayoutPatchOperation::UpsertGeometry {
                    node: measured_entry.node,
                    key: measured_entry.key.clone(),
                    geometry: measured_entry.geometry.clone(),
                    reasons,
                });
            cache.entries.insert(hot_ref.id, measured_entry);
        }

        Ok(Some(DocumentRetainedLayoutCacheUpdate {
            cache,
            delta,
            refreshed,
            patch,
        }))
    }

    pub fn key_table(&self) -> DocumentRetainedLayoutKeyTable {
        DocumentRetainedLayoutKeyTable {
            entries: self
                .entries
                .iter()
                .map(|(id, entry)| {
                    (
                        *id,
                        DocumentRetainedLayoutEntry {
                            node: entry.node,
                            key: entry.key.clone(),
                        },
                    )
                })
                .collect(),
        }
    }
}

fn retained_layout_cache_entry_for_node(
    frame: &DocumentFrame,
    hot_ids: &DocumentHotIdTable,
    key_table: &DocumentRetainedLayoutKeyTable,
    layout: &LayoutFrame,
    node_id: &DocumentNodeId,
) -> Result<Option<DocumentRetainedLayoutCacheEntry>, PatchApplyError> {
    let Some((display_index, item)) = layout
        .display_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.node == *node_id)
    else {
        return Ok(None);
    };
    let node = frame
        .nodes
        .get(node_id)
        .ok_or_else(|| PatchApplyError::StaleReference {
            reference_kind: "layout_frame_display_item",
            id: node_id.clone(),
        })?;
    let node_ref = hot_ids
        .hot_ref(node_id)
        .ok_or_else(|| PatchApplyError::StaleReference {
            reference_kind: "hot_id_table",
            id: node_id.clone(),
        })?;
    let key_entry =
        key_table
            .entry(node_ref.id)
            .ok_or_else(|| PatchApplyError::StaleReference {
                reference_kind: "retained_layout_key_table",
                id: node_id.clone(),
            })?;
    Ok(Some(DocumentRetainedLayoutCacheEntry {
        node: node_ref,
        key: key_entry.key.clone(),
        geometry: DocumentRetainedLayoutGeometry {
            bounds: item.bounds,
            display_index,
            hit_region_count: layout
                .hit_regions
                .iter()
                .filter(|hit| hit.node == node.id)
                .count(),
            scroll_region_count: layout
                .scroll_regions
                .iter()
                .filter(|scroll| scroll.node == node.id)
                .count(),
            materialization_count: layout
                .materialization
                .iter()
                .filter(|report| report.node == node.id)
                .count(),
        },
    }))
}

impl DocumentTypedStyleIndex {
    pub fn from_frame(
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
    ) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(frame)?;
        let mut index = Self::default();
        for (node_id, node) in &frame.nodes {
            let node_ref =
                hot_ids
                    .hot_ref(node_id)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "hot_id_table",
                        id: node_id.clone(),
                    })?;
            index.records.insert(
                node_ref.id,
                DocumentTypedStyleRecord {
                    node: node_ref,
                    identity: computed_style_identity(&node.style),
                    layout: typed_layout_style(&node.style),
                    paint: typed_paint_style(&node.style),
                    text: typed_text_style(&node.style),
                    material: typed_material_style(&node.style),
                    pseudo: typed_pseudo_style(&node.style),
                },
            );
        }
        Ok(index)
    }

    fn update_node(
        &mut self,
        node_id: &DocumentNodeId,
        node: &DocumentNode,
        hot_ref: DocumentHotNodeRef,
    ) {
        let _ = node_id;
        self.records.insert(
            hot_ref.id,
            DocumentTypedStyleRecord {
                node: hot_ref,
                identity: computed_style_identity(&node.style),
                layout: typed_layout_style(&node.style),
                paint: typed_paint_style(&node.style),
                text: typed_text_style(&node.style),
                material: typed_material_style(&node.style),
                pseudo: typed_pseudo_style(&node.style),
            },
        );
    }

    pub fn record(&self, id: DocumentHotNodeId) -> Option<&DocumentTypedStyleRecord> {
        self.records.get(&id)
    }
}

impl DocumentTypedBindingIndex {
    pub fn from_frame(
        frame: &DocumentFrame,
        hot_ids: &DocumentHotIdTable,
        intern_index: &DocumentInternIndex,
    ) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(frame)?;
        let mut index = Self::default();
        for (node_id, node) in &frame.nodes {
            let node_ref =
                hot_ids
                    .hot_ref(node_id)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "hot_id_table",
                        id: node_id.clone(),
                    })?;
            index.insert_node_bindings(node_id, node, node_ref, intern_index)?;
        }
        Ok(index)
    }

    fn update_node(
        &mut self,
        node_id: &DocumentNodeId,
        node: &DocumentNode,
        hot_ref: DocumentHotNodeRef,
        intern_index: &DocumentInternIndex,
    ) -> Result<(), PatchApplyError> {
        if let Some(previous_node) = self.nodes.remove(&hot_ref.id) {
            for binding in previous_node.bindings {
                if let Some(refs) = self.by_binding_id.get_mut(&binding.binding_id) {
                    refs.retain(|reference| *reference != binding.reference);
                    if refs.is_empty() {
                        self.by_binding_id.remove(&binding.binding_id);
                    }
                }
                if let Some(refs) = self.by_route.get_mut(&binding.route) {
                    refs.retain(|reference| *reference != binding.reference);
                    if refs.is_empty() {
                        self.by_route.remove(&binding.route);
                    }
                }
            }
        }

        self.insert_node_bindings(node_id, node, hot_ref, intern_index)
    }

    fn insert_node_bindings(
        &mut self,
        node_id: &DocumentNodeId,
        node: &DocumentNode,
        hot_ref: DocumentHotNodeRef,
        intern_index: &DocumentInternIndex,
    ) -> Result<(), PatchApplyError> {
        if node.source_bindings().next().is_none() {
            return Ok(());
        }
        let interned =
            intern_index
                .nodes
                .get(&hot_ref.id)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "document_intern_index",
                    id: node_id.clone(),
                })?;

        let mut bindings = Vec::new();
        for (ordinal, binding) in node.source_bindings().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(|_| PatchApplyError::StaleReference {
                reference_kind: "document_typed_binding_ordinal",
                id: node_id.clone(),
            })?;
            let intern_id = *interned
                .source_bindings
                .get(ordinal as usize)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "document_intern_index_source_binding",
                    id: node_id.clone(),
                })?;
            let reference = DocumentTypedBindingRef {
                node: hot_ref.id,
                ordinal,
            };
            let route = DocumentTypedBindingRoute {
                source_path: binding.source_path.clone(),
                intent: binding.intent.clone(),
            };
            let typed = DocumentTypedBinding {
                node: hot_ref,
                reference,
                binding_id: binding.id.clone(),
                route: route.clone(),
                intern_id,
            };
            self.by_binding_id
                .entry(binding.id.clone())
                .or_default()
                .push(reference);
            self.by_route.entry(route).or_default().push(reference);
            bindings.push(typed);
        }

        if !bindings.is_empty() {
            self.nodes.insert(
                hot_ref.id,
                DocumentTypedBindingNode {
                    node: hot_ref,
                    bindings,
                },
            );
        }
        Ok(())
    }

    pub fn bindings_for_node(&self, node: DocumentHotNodeId) -> &[DocumentTypedBinding] {
        self.nodes
            .get(&node)
            .map(|entry| entry.bindings.as_slice())
            .unwrap_or(&[])
    }

    pub fn refs_for_binding_id(&self, id: &SourceBindingId) -> &[DocumentTypedBindingRef] {
        self.by_binding_id.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn refs_for_route(&self, route: &DocumentTypedBindingRoute) -> &[DocumentTypedBindingRef] {
        self.by_route.get(route).map(Vec::as_slice).unwrap_or(&[])
    }
}

impl DocumentDerivedIndexBundle {
    pub fn from_frame(frame: &DocumentFrame) -> Result<Self, PatchApplyError> {
        let hot_ids = DocumentHotIdTable::from_frame(frame)?;
        let intern_index = DocumentInternIndex::from_frame(frame, &hot_ids)?;
        let retained_layout_keys =
            DocumentRetainedLayoutKeyTable::from_frame(frame, &hot_ids, &intern_index)?;
        let typed_styles = DocumentTypedStyleIndex::from_frame(frame, &hot_ids)?;
        let typed_bindings = DocumentTypedBindingIndex::from_frame(frame, &hot_ids, &intern_index)?;
        Ok(Self {
            hot_ids,
            intern_index,
            retained_layout_keys,
            typed_styles,
            typed_bindings,
        })
    }

    pub fn from_previous_nonstructural_patch(
        previous: &Self,
        frame: &DocumentFrame,
        changed_nodes: &BTreeSet<DocumentNodeId>,
    ) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(frame)?;
        if previous.hot_ids.ids_by_node.len() != frame.nodes.len() {
            return Err(PatchApplyError::StaleReference {
                reference_kind: "hot_id_table_node_count",
                id: frame.root.clone(),
            });
        }
        for node_id in frame.nodes.keys() {
            if !previous.hot_ids.ids_by_node.contains_key(node_id) {
                return Err(PatchApplyError::StaleReference {
                    reference_kind: "hot_id_table",
                    id: node_id.clone(),
                });
            }
        }

        let mut next = previous.clone();
        for node_id in changed_nodes {
            let node = frame
                .nodes
                .get(node_id)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "document_frame",
                    id: node_id.clone(),
                })?;
            let hot_ref =
                next.hot_ids
                    .hot_ref(node_id)
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "hot_id_table",
                        id: node_id.clone(),
                    })?;
            next.intern_index.update_node(node, hot_ref);
            next.retained_layout_keys.update_node(
                frame,
                &next.hot_ids,
                &next.intern_index,
                node_id,
            )?;
            next.typed_styles.update_node(node_id, node, hot_ref);
            next.typed_bindings
                .update_node(node_id, node, hot_ref, &next.intern_index)?;
        }
        Ok(next)
    }

    pub fn try_layout<'a>(
        &'a self,
        input: LayoutInput<'a>,
    ) -> Result<LayoutFrame, PatchApplyError> {
        try_layout_with_typed_styles(input, &self.hot_ids, &self.typed_styles)
    }

    pub fn try_hit_side_table(
        &self,
        document: &DocumentFrame,
        layout: &LayoutFrame,
    ) -> Result<HitSideTable, PatchApplyError> {
        self.try_hit_side_table_with_bucket_size(
            document,
            layout,
            HitSideTable::DEFAULT_BUCKET_SIZE,
        )
    }

    pub fn try_hit_side_table_with_bucket_size(
        &self,
        document: &DocumentFrame,
        layout: &LayoutFrame,
        bucket_size: f32,
    ) -> Result<HitSideTable, PatchApplyError> {
        HitSideTable::try_from_document_layout_with_typed_bindings(
            document,
            &self.hot_ids,
            &self.typed_bindings,
            layout,
            bucket_size,
        )
    }

    pub fn try_render_scene(
        &self,
        layout: &LayoutFrame,
        width: u32,
        height: u32,
        columns: &mut impl render_scene::RenderTextColumnMeasurer,
    ) -> Result<RenderScene, PatchApplyError> {
        render_scene::lower_layout_frame_to_render_scene_with_retained_keys(
            layout,
            &self.hot_ids,
            &self.retained_layout_keys,
            width,
            height,
            columns,
        )
    }

    pub fn try_retained_layout_cache(
        &self,
        document: &DocumentFrame,
        layout: &LayoutFrame,
    ) -> Result<DocumentRetainedLayoutCache, PatchApplyError> {
        DocumentRetainedLayoutCache::from_layout_frame(
            document,
            &self.hot_ids,
            &self.retained_layout_keys,
            layout,
        )
    }

    pub fn try_retained_layout_cache_update(
        &self,
        previous: &DocumentRetainedLayoutCache,
        document: &DocumentFrame,
        layout: &LayoutFrame,
    ) -> Result<DocumentRetainedLayoutCacheUpdate, PatchApplyError> {
        previous.update_from_layout_frame(
            document,
            &self.hot_ids,
            &self.retained_layout_keys,
            layout,
        )
    }

    pub fn try_retained_layout_cache_update_for_nodes(
        &self,
        previous: &DocumentRetainedLayoutCache,
        document: &DocumentFrame,
        layout: &LayoutFrame,
        changed_nodes: &BTreeSet<DocumentNodeId>,
    ) -> Result<Option<DocumentRetainedLayoutCacheUpdate>, PatchApplyError> {
        previous.update_nodes_from_layout_frame(
            document,
            &self.hot_ids,
            &self.retained_layout_keys,
            layout,
            changed_nodes,
        )
    }
}

impl DocumentState {
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            frame: DocumentFrame::empty(root),
        }
    }

    pub fn from_frame(frame: DocumentFrame) -> Result<Self, PatchApplyError> {
        validate_frame_integrity(&frame)?;
        Ok(Self { frame })
    }

    pub fn frame(&self) -> &DocumentFrame {
        &self.frame
    }

    pub fn into_frame(self) -> DocumentFrame {
        self.frame
    }

    pub fn apply_patch(
        &mut self,
        patch: DocumentPatch,
    ) -> Result<PatchApplyReport, PatchApplyError> {
        let change_set = self.apply_batch(DocumentChangeBatch {
            patches: vec![patch],
        })?;
        Ok(change_set
            .reports
            .into_iter()
            .next()
            .expect("single patch batch must produce one report"))
    }

    pub fn apply_batch(
        &mut self,
        batch: DocumentChangeBatch,
    ) -> Result<DocumentChangeSet, PatchApplyError> {
        validate_frame_integrity(&self.frame)?;
        let node_count_before = self.frame.nodes.len();
        let mut next_frame = self.frame.clone();
        let mut reports = Vec::with_capacity(batch.patches.len());
        for patch in batch.patches {
            reports.push(apply_document_patch_unchecked(&mut next_frame, patch)?);
        }
        validate_frame_integrity(&next_frame)?;
        let change_set =
            document_change_set_from_reports(reports, node_count_before, next_frame.nodes.len());
        self.frame = next_frame;
        Ok(change_set)
    }

    pub fn apply_ui_semantic_batch(
        &mut self,
        batch: ChangeBatch<UiSemanticChange>,
    ) -> Result<DocumentChangeSet, PatchApplyError> {
        validate_frame_integrity(&self.frame)?;
        let node_count_before = self.frame.nodes.len();
        let mut next_frame = self.frame.clone();
        let reports = apply_ui_semantic_changes_unchecked(&mut next_frame, batch.changes)?;
        validate_frame_integrity(&next_frame)?;
        let change_set =
            document_change_set_from_reports(reports, node_count_before, next_frame.nodes.len());
        self.frame = next_frame;
        Ok(change_set)
    }

    pub fn apply_batch_to_owned_frame(
        mut frame: DocumentFrame,
        batch: DocumentChangeBatch,
    ) -> Result<(DocumentFrame, DocumentChangeSet), PatchApplyError> {
        validate_frame_integrity(&frame)?;
        let node_count_before = frame.nodes.len();
        let mut reports = Vec::with_capacity(batch.patches.len());
        for patch in batch.patches {
            reports.push(apply_document_patch_unchecked(&mut frame, patch)?);
        }
        validate_frame_integrity(&frame)?;
        let change_set =
            document_change_set_from_reports(reports, node_count_before, frame.nodes.len());
        Ok((frame, change_set))
    }

    pub fn apply_ui_semantic_batch_to_owned_frame(
        frame: DocumentFrame,
        batch: ChangeBatch<UiSemanticChange>,
    ) -> Result<(DocumentFrame, DocumentChangeSet), PatchApplyError> {
        validate_frame_integrity(&frame)?;
        Self::apply_ui_semantic_batch_to_valid_owned_frame(frame, batch)
    }

    pub fn apply_ui_semantic_batch_to_valid_owned_frame(
        mut frame: DocumentFrame,
        batch: ChangeBatch<UiSemanticChange>,
    ) -> Result<(DocumentFrame, DocumentChangeSet), PatchApplyError> {
        let node_count_before = frame.nodes.len();
        let reports = apply_ui_semantic_changes_unchecked(&mut frame, batch.changes)?;
        let change_set =
            document_change_set_from_reports(reports, node_count_before, frame.nodes.len());
        Ok((frame, change_set))
    }

    pub fn apply_nonstructural_batch_to_valid_owned_frame(
        mut frame: DocumentFrame,
        batch: DocumentChangeBatch,
    ) -> Result<(DocumentFrame, DocumentChangeSet), PatchApplyError> {
        for patch in &batch.patches {
            if let Some(patch_kind) = document_patch_structural_kind(patch) {
                return Err(PatchApplyError::UnsupportedTrustedNonstructuralPatch { patch_kind });
            }
        }
        let node_count_before = frame.nodes.len();
        let mut reports = Vec::with_capacity(batch.patches.len());
        for patch in batch.patches {
            reports.push(apply_document_patch_unchecked(&mut frame, patch)?);
        }
        let change_set =
            document_change_set_from_reports(reports, node_count_before, frame.nodes.len());
        Ok((frame, change_set))
    }
}

fn apply_ui_semantic_changes_unchecked(
    frame: &mut DocumentFrame,
    changes: Vec<UiSemanticChange>,
) -> Result<Vec<PatchApplyReport>, PatchApplyError> {
    let mut reports = Vec::with_capacity(changes.len());
    for change in changes {
        match change {
            UiSemanticChange::SetLayoutStyle { id, patch } => {
                reports.push(apply_typed_style_patch_unchecked(
                    frame,
                    id,
                    patch.patch,
                    "set_layout_style",
                )?);
            }
            UiSemanticChange::SetPaintStyle { id, patch } => {
                reports.push(apply_typed_style_patch_unchecked(
                    frame,
                    id,
                    patch.patch,
                    "set_paint_style",
                )?);
            }
            UiSemanticChange::SetTextStyle { id, patch } => {
                reports.push(apply_typed_style_patch_unchecked(
                    frame,
                    id,
                    patch.patch,
                    "set_text_style",
                )?);
            }
            UiSemanticChange::SetMaterialStyle { id, patch } => {
                reports.push(apply_typed_style_patch_unchecked(
                    frame,
                    id,
                    patch.patch,
                    "set_material_style",
                )?);
            }
            other => {
                for patch in other.into_document_patches() {
                    reports.push(apply_document_patch_unchecked(frame, patch)?);
                }
            }
        }
    }
    Ok(reports)
}

fn apply_typed_style_patch_unchecked(
    frame: &mut DocumentFrame,
    id: DocumentNodeId,
    patch: StylePatch,
    patch_kind: &'static str,
) -> Result<PatchApplyReport, PatchApplyError> {
    let node = required_node_mut(frame, patch_kind, &id)?;
    let changed_keys = apply_style_patch(&mut node.style, patch);
    let invalidation = style_patch_invalidation(&changed_keys);
    Ok(PatchApplyReport {
        patch_kind,
        target: Some(id),
        invalidation,
        removed_nodes: Vec::new(),
        node_count_after: frame.nodes.len(),
        materialization: None,
    })
}

fn document_patch_structural_kind(patch: &DocumentPatch) -> Option<&'static str> {
    match patch {
        DocumentPatch::UpsertNode(_) => Some("upsert_node"),
        DocumentPatch::RemoveNode { .. } => Some("remove_node"),
        DocumentPatch::InsertChild { .. } => Some("insert_child"),
        DocumentPatch::RemoveChild { .. } => Some("remove_child"),
        DocumentPatch::MoveChild { .. } => Some("move_child"),
        DocumentPatch::SetText { .. }
        | DocumentPatch::SetStyle { .. }
        | DocumentPatch::SetBinding { .. }
        | DocumentPatch::SetBindingAt { .. }
        | DocumentPatch::SetScroll { .. }
        | DocumentPatch::SetListMaterialization { .. } => None,
    }
}

fn apply_document_patch_unchecked(
    frame: &mut DocumentFrame,
    patch: DocumentPatch,
) -> Result<PatchApplyReport, PatchApplyError> {
    match patch {
        DocumentPatch::UpsertNode(node) => {
            let target = node.id.clone();
            apply_upsert_node(frame, node)?;
            Ok(PatchApplyReport {
                patch_kind: "upsert_node",
                target: Some(target),
                invalidation: vec![
                    PatchInvalidationClass::Structure,
                    PatchInvalidationClass::ListStructure,
                    PatchInvalidationClass::ConditionalStructure,
                    PatchInvalidationClass::Layout,
                    PatchInvalidationClass::LayoutOnly,
                    PatchInvalidationClass::HitRegion,
                    PatchInvalidationClass::FullDocument,
                ],
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::RemoveNode { id } => {
            let removed_nodes = remove_subtree(frame, &id)?;
            Ok(PatchApplyReport {
                patch_kind: "remove_node",
                target: Some(id),
                invalidation: vec![
                    PatchInvalidationClass::Structure,
                    PatchInvalidationClass::ListStructure,
                    PatchInvalidationClass::Binding,
                    PatchInvalidationClass::SourceBinding,
                    PatchInvalidationClass::Scroll,
                    PatchInvalidationClass::ScrollOffsetOnly,
                    PatchInvalidationClass::Materialization,
                    PatchInvalidationClass::MaterializationOnly,
                    PatchInvalidationClass::Layout,
                    PatchInvalidationClass::LayoutOnly,
                    PatchInvalidationClass::HitRegion,
                    PatchInvalidationClass::FullDocument,
                ],
                removed_nodes,
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::InsertChild {
            parent,
            child,
            index,
        } => {
            reorder_child(frame, &parent, &child, index, "insert_child")?;
            Ok(PatchApplyReport {
                patch_kind: "insert_child",
                target: Some(parent),
                invalidation: structural_child_invalidation(),
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::RemoveChild { parent, child } => {
            validate_parent_child_link(frame, &parent, &child, "remove_child")?;
            let removed_nodes = remove_subtree(frame, &child)?;
            Ok(PatchApplyReport {
                patch_kind: "remove_child",
                target: Some(parent),
                invalidation: structural_child_invalidation(),
                removed_nodes,
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::MoveChild {
            child,
            new_parent,
            index,
        } => {
            move_child(frame, &child, &new_parent, index)?;
            Ok(PatchApplyReport {
                patch_kind: "move_child",
                target: Some(new_parent),
                invalidation: structural_child_invalidation(),
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::SetText { id, text } => {
            let node = required_node_mut(frame, "set_text", &id)?;
            node.text = Some(text);
            Ok(PatchApplyReport {
                patch_kind: "set_text",
                target: Some(id),
                invalidation: vec![
                    PatchInvalidationClass::Text,
                    PatchInvalidationClass::PaintOnly,
                    PatchInvalidationClass::Layout,
                    PatchInvalidationClass::LayoutOnly,
                    PatchInvalidationClass::HitRegion,
                ],
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::SetStyle { id, patch } => {
            let node = required_node_mut(frame, "set_style", &id)?;
            let changed_keys = apply_style_patch(&mut node.style, patch);
            let invalidation = style_patch_invalidation(&changed_keys);
            Ok(PatchApplyReport {
                patch_kind: "set_style",
                target: Some(id),
                invalidation,
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::SetBinding { id, binding } => {
            let node = required_node_mut(frame, "set_binding", &id)?;
            node.set_primary_source_binding(binding);
            Ok(PatchApplyReport {
                patch_kind: "set_binding",
                target: Some(id),
                invalidation: source_binding_invalidation(),
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::SetBindingAt {
            id,
            ordinal,
            binding,
        } => {
            let node = required_node_mut(frame, "set_binding_at", &id)?;
            apply_source_binding_at(node, ordinal, binding)?;
            Ok(PatchApplyReport {
                patch_kind: "set_binding_at",
                target: Some(id),
                invalidation: source_binding_invalidation(),
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::SetScroll { id, scroll } => {
            let node = required_node_mut(frame, "set_scroll", &id)?;
            node.scroll = Some(scroll);
            Ok(PatchApplyReport {
                patch_kind: "set_scroll",
                target: Some(id),
                invalidation: vec![
                    PatchInvalidationClass::Scroll,
                    PatchInvalidationClass::ScrollOffsetOnly,
                    PatchInvalidationClass::Layout,
                    PatchInvalidationClass::LayoutOnly,
                ],
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: None,
            })
        }
        DocumentPatch::SetListMaterialization { id, materialized } => {
            let node = required_node_mut(frame, "set_list_materialization", &id)?;
            let report = materialization_report(node, &materialized);
            node.materialized.push(materialized);
            Ok(PatchApplyReport {
                patch_kind: "set_list_materialization",
                target: Some(id),
                invalidation: vec![
                    PatchInvalidationClass::Materialization,
                    PatchInvalidationClass::MaterializationOnly,
                    PatchInvalidationClass::Layout,
                    PatchInvalidationClass::LayoutOnly,
                    PatchInvalidationClass::HitRegion,
                ],
                removed_nodes: Vec::new(),
                node_count_after: frame.nodes.len(),
                materialization: Some(report),
            })
        }
    }
}

fn structural_child_invalidation() -> Vec<PatchInvalidationClass> {
    vec![
        PatchInvalidationClass::Structure,
        PatchInvalidationClass::ListStructure,
        PatchInvalidationClass::ConditionalStructure,
        PatchInvalidationClass::Layout,
        PatchInvalidationClass::LayoutOnly,
        PatchInvalidationClass::HitRegion,
    ]
}

fn source_binding_invalidation() -> Vec<PatchInvalidationClass> {
    vec![
        PatchInvalidationClass::Binding,
        PatchInvalidationClass::SourceBinding,
        PatchInvalidationClass::HitRegion,
    ]
}

fn apply_source_binding_at(
    node: &mut DocumentNode,
    ordinal: u32,
    binding: boon_document_model::SourceBinding,
) -> Result<(), PatchApplyError> {
    if ordinal == 0 {
        if node.source_bindings.is_empty() {
            return Err(PatchApplyError::StaleReference {
                reference_kind: "source_binding_at",
                id: node.id.clone(),
            });
        }
        node.source_bindings[0] = binding;
        return Ok(());
    }
    let index = usize::try_from(ordinal).map_err(|_| PatchApplyError::StaleReference {
        reference_kind: "source_binding_at",
        id: node.id.clone(),
    })?;
    let Some(slot) = node.source_bindings.get_mut(index) else {
        return Err(PatchApplyError::StaleReference {
            reference_kind: "source_binding_at",
            id: node.id.clone(),
        });
    };
    *slot = binding;
    Ok(())
}

fn document_change_set_from_reports(
    reports: Vec<PatchApplyReport>,
    node_count_before: usize,
    node_count_after: usize,
) -> DocumentChangeSet {
    let mut targets = Vec::new();
    let mut invalidation = Vec::new();
    let mut removed_nodes = Vec::new();
    let mut materialization = Vec::new();
    for report in &reports {
        if let Some(target) = &report.target
            && !targets.contains(target)
        {
            targets.push(target.clone());
        }
        for class in &report.invalidation {
            push_unique_invalidation(&mut invalidation, class.clone());
        }
        for removed in &report.removed_nodes {
            if !removed_nodes.contains(removed) {
                removed_nodes.push(removed.clone());
            }
        }
        if let Some(report) = &report.materialization {
            materialization.push(report.clone());
        }
    }
    DocumentChangeSet {
        patch_count: reports.len(),
        reports,
        targets,
        invalidation,
        removed_nodes,
        node_count_before,
        node_count_after,
        materialization,
    }
}

fn required_node_mut<'a>(
    frame: &'a mut DocumentFrame,
    patch_kind: &'static str,
    id: &DocumentNodeId,
) -> Result<&'a mut DocumentNode, PatchApplyError> {
    frame
        .nodes
        .get_mut(id)
        .ok_or_else(|| PatchApplyError::MissingTarget {
            patch_kind,
            id: id.clone(),
        })
}

fn apply_upsert_node(frame: &mut DocumentFrame, node: DocumentNode) -> Result<(), PatchApplyError> {
    let id = node.id.clone();
    if id == frame.root && node.parent.is_some() {
        return Err(PatchApplyError::InvalidParentChildLink {
            parent: id.clone(),
            child: id,
            actual_parent: node.parent,
        });
    }
    if id != frame.root {
        let parent = node
            .parent
            .clone()
            .ok_or_else(|| PatchApplyError::OrphanedNode {
                id: id.clone(),
                parent: None,
            })?;
        if !frame.nodes.contains_key(&parent) {
            return Err(PatchApplyError::MissingParent {
                id: id.clone(),
                parent,
            });
        }
    }
    validate_child_refs(frame, &node)?;

    if let Some(old_parent) = frame.nodes.get(&id).and_then(|old| old.parent.clone())
        && old_parent != node.parent.clone().unwrap_or_else(|| frame.root.clone())
        && let Some(parent) = frame.nodes.get_mut(&old_parent)
    {
        parent.children.retain(|child| child != &id);
    }
    let parent = node.parent.clone();
    frame.nodes.insert(id.clone(), node);
    if let Some(parent_id) = parent
        && let Some(parent) = frame.nodes.get_mut(&parent_id)
        && !parent.children.contains(&id)
    {
        parent.children.push(id);
    }
    Ok(())
}

fn validate_child_refs(frame: &DocumentFrame, node: &DocumentNode) -> Result<(), PatchApplyError> {
    let mut seen = BTreeSet::new();
    for child in &node.children {
        if !seen.insert(child.clone()) {
            return Err(PatchApplyError::DuplicateChild {
                parent: node.id.clone(),
                child: child.clone(),
            });
        }
        let Some(child_node) = frame.nodes.get(child) else {
            return Err(PatchApplyError::OrphanedChild {
                parent: node.id.clone(),
                child: child.clone(),
            });
        };
        if child_node.parent.as_ref() != Some(&node.id) {
            return Err(PatchApplyError::InvalidParentChildLink {
                parent: node.id.clone(),
                child: child.clone(),
                actual_parent: child_node.parent.clone(),
            });
        }
    }
    Ok(())
}

fn remove_subtree(
    frame: &mut DocumentFrame,
    id: &DocumentNodeId,
) -> Result<Vec<DocumentNodeId>, PatchApplyError> {
    if id == &frame.root {
        return Err(PatchApplyError::CannotRemoveRoot { id: id.clone() });
    }
    if !frame.nodes.contains_key(id) {
        return Err(PatchApplyError::MissingTarget {
            patch_kind: "remove_node",
            id: id.clone(),
        });
    }
    let mut removed = Vec::new();
    collect_subtree(frame, id, &mut removed)?;
    if let Some(parent_id) = frame.nodes.get(id).and_then(|node| node.parent.clone())
        && let Some(parent) = frame.nodes.get_mut(&parent_id)
    {
        parent.children.retain(|child| child != id);
    }
    for node_id in &removed {
        frame.nodes.remove(node_id);
    }
    if frame
        .focus
        .as_ref()
        .is_some_and(|focus| removed.contains(focus))
    {
        frame.focus = None;
    }
    Ok(removed)
}

fn reorder_child(
    frame: &mut DocumentFrame,
    parent: &DocumentNodeId,
    child: &DocumentNodeId,
    index: usize,
    patch_kind: &'static str,
) -> Result<(), PatchApplyError> {
    validate_parent_child_link(frame, parent, child, patch_kind)?;
    let parent_node =
        frame
            .nodes
            .get_mut(parent)
            .ok_or_else(|| PatchApplyError::MissingTarget {
                patch_kind,
                id: parent.clone(),
            })?;
    parent_node.children.retain(|candidate| candidate != child);
    if index > parent_node.children.len() {
        return Err(PatchApplyError::ChildIndexOutOfBounds {
            parent: parent.clone(),
            index,
            child_count: parent_node.children.len(),
        });
    }
    parent_node.children.insert(index, child.clone());
    Ok(())
}

fn move_child(
    frame: &mut DocumentFrame,
    child: &DocumentNodeId,
    new_parent: &DocumentNodeId,
    index: usize,
) -> Result<(), PatchApplyError> {
    if child == &frame.root {
        return Err(PatchApplyError::CannotRemoveRoot { id: child.clone() });
    }
    if !frame.nodes.contains_key(child) {
        return Err(PatchApplyError::MissingTarget {
            patch_kind: "move_child",
            id: child.clone(),
        });
    }
    if !frame.nodes.contains_key(new_parent) {
        return Err(PatchApplyError::MissingTarget {
            patch_kind: "move_child",
            id: new_parent.clone(),
        });
    }
    validate_move_does_not_create_cycle(frame, child, new_parent)?;
    let old_parent = frame
        .nodes
        .get(child)
        .and_then(|node| node.parent.clone())
        .ok_or_else(|| PatchApplyError::OrphanedNode {
            id: child.clone(),
            parent: None,
        })?;
    if let Some(parent_node) = frame.nodes.get_mut(&old_parent) {
        parent_node.children.retain(|candidate| candidate != child);
    }
    let new_parent_node =
        frame
            .nodes
            .get_mut(new_parent)
            .ok_or_else(|| PatchApplyError::MissingTarget {
                patch_kind: "move_child",
                id: new_parent.clone(),
            })?;
    if index > new_parent_node.children.len() {
        return Err(PatchApplyError::ChildIndexOutOfBounds {
            parent: new_parent.clone(),
            index,
            child_count: new_parent_node.children.len(),
        });
    }
    new_parent_node.children.insert(index, child.clone());
    if let Some(child_node) = frame.nodes.get_mut(child) {
        child_node.parent = Some(new_parent.clone());
    }
    Ok(())
}

fn validate_parent_child_link(
    frame: &DocumentFrame,
    parent: &DocumentNodeId,
    child: &DocumentNodeId,
    patch_kind: &'static str,
) -> Result<(), PatchApplyError> {
    let parent_node = frame
        .nodes
        .get(parent)
        .ok_or_else(|| PatchApplyError::MissingTarget {
            patch_kind,
            id: parent.clone(),
        })?;
    let child_node = frame
        .nodes
        .get(child)
        .ok_or_else(|| PatchApplyError::MissingTarget {
            patch_kind,
            id: child.clone(),
        })?;
    if child_node.parent.as_ref() != Some(parent) || !parent_node.children.contains(child) {
        return Err(PatchApplyError::InvalidParentChildLink {
            parent: parent.clone(),
            child: child.clone(),
            actual_parent: child_node.parent.clone(),
        });
    }
    Ok(())
}

fn validate_move_does_not_create_cycle(
    frame: &DocumentFrame,
    child: &DocumentNodeId,
    new_parent: &DocumentNodeId,
) -> Result<(), PatchApplyError> {
    let mut current = new_parent.clone();
    loop {
        if &current == child {
            return Err(PatchApplyError::Cycle { id: child.clone() });
        }
        if current == frame.root {
            return Ok(());
        }
        let Some(node) = frame.nodes.get(&current) else {
            return Err(PatchApplyError::MissingTarget {
                patch_kind: "move_child",
                id: current,
            });
        };
        let Some(parent) = node.parent.clone() else {
            return Err(PatchApplyError::OrphanedNode {
                id: node.id.clone(),
                parent: None,
            });
        };
        current = parent;
    }
}

fn collect_subtree(
    frame: &DocumentFrame,
    id: &DocumentNodeId,
    removed: &mut Vec<DocumentNodeId>,
) -> Result<(), PatchApplyError> {
    let node = frame
        .nodes
        .get(id)
        .ok_or_else(|| PatchApplyError::MissingTarget {
            patch_kind: "remove_node",
            id: id.clone(),
        })?;
    removed.push(id.clone());
    for child in &node.children {
        collect_subtree(frame, child, removed)?;
    }
    Ok(())
}

fn validate_frame_integrity(frame: &DocumentFrame) -> Result<(), PatchApplyError> {
    let root = frame
        .nodes
        .get(&frame.root)
        .ok_or_else(|| PatchApplyError::MissingTarget {
            patch_kind: "frame_root",
            id: frame.root.clone(),
        })?;
    if root.parent.is_some() {
        return Err(PatchApplyError::InvalidParentChildLink {
            parent: frame.root.clone(),
            child: frame.root.clone(),
            actual_parent: root.parent.clone(),
        });
    }
    for node in frame.nodes.values() {
        validate_child_refs(frame, node)?;
        if node.id != frame.root {
            let Some(parent) = node.parent.as_ref() else {
                return Err(PatchApplyError::OrphanedNode {
                    id: node.id.clone(),
                    parent: None,
                });
            };
            let Some(parent_node) = frame.nodes.get(parent) else {
                return Err(PatchApplyError::MissingParent {
                    id: node.id.clone(),
                    parent: parent.clone(),
                });
            };
            if !parent_node.children.contains(&node.id) {
                return Err(PatchApplyError::InvalidParentChildLink {
                    parent: parent.clone(),
                    child: node.id.clone(),
                    actual_parent: node.parent.clone(),
                });
            }
            validate_parent_chain_reaches_root(frame, &node.id)?;
        }
    }
    if let Some(focus) = &frame.focus
        && !frame.nodes.contains_key(focus)
    {
        return Err(PatchApplyError::StaleReference {
            reference_kind: "focus",
            id: focus.clone(),
        });
    }
    Ok(())
}

fn validate_parent_chain_reaches_root(
    frame: &DocumentFrame,
    id: &DocumentNodeId,
) -> Result<(), PatchApplyError> {
    let mut seen = BTreeSet::new();
    let mut current = id.clone();
    while current != frame.root {
        if !seen.insert(current.clone()) {
            return Err(PatchApplyError::Cycle { id: current });
        }
        let Some(node) = frame.nodes.get(&current) else {
            return Err(PatchApplyError::MissingTarget {
                patch_kind: "parent_chain",
                id: current,
            });
        };
        let Some(parent) = node.parent.clone() else {
            return Err(PatchApplyError::OrphanedNode {
                id: node.id.clone(),
                parent: None,
            });
        };
        current = parent;
    }
    Ok(())
}

pub fn try_layout(input: LayoutInput<'_>) -> Result<LayoutFrame, PatchApplyError> {
    validate_frame_integrity(input.document)?;
    Ok(layout_unchecked(input))
}

pub fn layout(input: LayoutInput<'_>) -> LayoutFrame {
    try_layout(input).expect("document layout frame failed integrity validation")
}

pub fn try_layout_with_typed_styles<'a>(
    input: LayoutInput<'a>,
    hot_ids: &'a DocumentHotIdTable,
    typed_styles: &'a DocumentTypedStyleIndex,
) -> Result<LayoutFrame, PatchApplyError> {
    validate_frame_integrity(input.document)?;
    validate_typed_style_context(input.document, hot_ids, typed_styles)?;
    Ok(layout_unchecked_with_typed_styles(
        input,
        Some(TypedLayoutStyleContext {
            hot_ids,
            typed_styles,
        }),
    ))
}

pub fn layout_with_typed_styles<'a>(
    input: LayoutInput<'a>,
    hot_ids: &'a DocumentHotIdTable,
    typed_styles: &'a DocumentTypedStyleIndex,
) -> LayoutFrame {
    try_layout_with_typed_styles(input, hot_ids, typed_styles)
        .expect("typed document layout frame failed integrity validation")
}

pub fn try_layout_subtree(input: LayoutSubtreeInput<'_>) -> Result<LayoutFrame, PatchApplyError> {
    validate_frame_integrity(input.document)?;
    Ok(layout_subtree_unchecked(input))
}

pub fn layout_subtree(input: LayoutSubtreeInput<'_>) -> LayoutFrame {
    try_layout_subtree(input).expect("document subtree layout frame failed integrity validation")
}

fn layout_unchecked(input: LayoutInput<'_>) -> LayoutFrame {
    layout_unchecked_with_typed_styles(input, None)
}

fn layout_unchecked_with_typed_styles<'a>(
    input: LayoutInput<'a>,
    typed_styles: Option<TypedLayoutStyleContext<'a>>,
) -> LayoutFrame {
    let mut builder = LayoutBuilder {
        document: input.document,
        text: input.text,
        typed_styles,
        display_list: Vec::new(),
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        demands: Vec::new(),
        materialization: Vec::new(),
        materialized_range_count: 0,
    };
    if let Some(root) = input.document.nodes.get(&input.document.root).cloned() {
        let mut cursor_y = 0.0;
        for child in root.children {
            let rect = builder.layout_node(
                &child,
                0.0,
                cursor_y,
                input.viewport.width,
                input.viewport.height,
            );
            cursor_y += rect.height;
        }
    }

    LayoutFrame {
        accessibility: AccessibilityTree {
            node_count: input.document.nodes.len(),
        },
        metrics: LayoutMetrics {
            node_count: input.document.nodes.len(),
            display_item_count: builder.display_list.len(),
            materialized_range_count: builder.materialized_range_count,
            native_capability_required: false,
        },
        display_list: builder.display_list,
        hit_regions: builder.hit_regions,
        scroll_regions: builder.scroll_regions,
        demands: builder.demands,
        materialization: builder.materialization,
    }
}

fn layout_subtree_unchecked(input: LayoutSubtreeInput<'_>) -> LayoutFrame {
    let mut builder = LayoutBuilder {
        document: input.document,
        text: input.text,
        typed_styles: None,
        display_list: Vec::new(),
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        demands: Vec::new(),
        materialization: Vec::new(),
        materialized_range_count: 0,
    };
    let subtree_node_count = document_subtree_node_count(input.document, input.root);
    builder.layout_node(
        input.root,
        input.x,
        input.y,
        input.available_width,
        input.available_height,
    );

    LayoutFrame {
        accessibility: AccessibilityTree {
            node_count: subtree_node_count,
        },
        metrics: LayoutMetrics {
            node_count: subtree_node_count,
            display_item_count: builder.display_list.len(),
            materialized_range_count: builder.materialized_range_count,
            native_capability_required: false,
        },
        display_list: builder.display_list,
        hit_regions: builder.hit_regions,
        scroll_regions: builder.scroll_regions,
        demands: builder.demands,
        materialization: builder.materialization,
    }
}

fn validate_typed_style_context(
    document: &DocumentFrame,
    hot_ids: &DocumentHotIdTable,
    typed_styles: &DocumentTypedStyleIndex,
) -> Result<(), PatchApplyError> {
    for node_id in document.nodes.keys() {
        let hot_ref = hot_ids
            .hot_ref(node_id)
            .ok_or_else(|| PatchApplyError::StaleReference {
                reference_kind: "typed_style_hot_id_table",
                id: node_id.clone(),
            })?;
        let record =
            typed_styles
                .record(hot_ref.id)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "typed_style_index",
                    id: node_id.clone(),
                })?;
        if record.node != hot_ref {
            return Err(PatchApplyError::StaleReference {
                reference_kind: "typed_style_generation",
                id: node_id.clone(),
            });
        }
    }
    Ok(())
}

fn document_subtree_node_count(document: &DocumentFrame, root: &DocumentNodeId) -> usize {
    let mut count = 0usize;
    let mut stack = vec![root.clone()];
    while let Some(id) = stack.pop() {
        let Some(node) = document.nodes.get(&id) else {
            continue;
        };
        count = count.saturating_add(1);
        stack.extend(node.children.iter().cloned());
    }
    count
}

#[derive(Clone, Copy)]
struct TypedLayoutStyleContext<'a> {
    hot_ids: &'a DocumentHotIdTable,
    typed_styles: &'a DocumentTypedStyleIndex,
}

struct LayoutBuilder<'a, 'b> {
    document: &'a DocumentFrame,
    text: &'b mut dyn TextMeasurer,
    typed_styles: Option<TypedLayoutStyleContext<'a>>,
    display_list: Vec<DisplayItem>,
    hit_regions: Vec<HitRegion>,
    scroll_regions: Vec<ScrollRegion>,
    demands: Vec<LayoutDemand>,
    materialization: Vec<MaterializationReport>,
    materialized_range_count: usize,
}

impl LayoutBuilder<'_, '_> {
    fn typed_style_record(&self, node: &DocumentNode) -> Option<DocumentTypedStyleRecord> {
        let typed_styles = self.typed_styles?;
        let hot_id = typed_styles.hot_ids.hot_id(&node.id)?;
        typed_styles.typed_styles.record(hot_id).cloned()
    }

    fn typed_layout_style(&self, node: &DocumentNode) -> Option<DocumentTypedLayoutStyle> {
        self.typed_style_record(node).map(|record| record.layout)
    }

    fn node_layout_dimension_is_fill(&self, node: &DocumentNode, key: &str) -> bool {
        let typed_layout = self.typed_layout_style(node);
        layout_dimension_is_fill(&node.style, typed_layout.as_ref(), key)
    }

    fn preferred_row_child_width(&mut self, node: &DocumentNode) -> Option<f32> {
        let typed_layout = self.typed_layout_style(node);
        preferred_row_child_width_with_typed(node, typed_layout.as_ref(), self.text)
    }

    fn layout_node(
        &mut self,
        id: &DocumentNodeId,
        x: f32,
        y: f32,
        available_width: f32,
        available_height: f32,
    ) -> Rect {
        let Some(node) = self.document.nodes.get(id).cloned() else {
            return Rect {
                x,
                y,
                width: 0.0,
                height: 0.0,
            };
        };
        let typed_record = self.typed_style_record(&node);
        let typed_layout = typed_record.as_ref().map(|record| &record.layout);
        let padding = layout_edges(&node.style, typed_layout, "padding");
        let gap = layout_spacing(&node.style, typed_layout, "gap").unwrap_or(0.0);
        let box_size = match node.kind {
            DocumentNodeKind::Checkbox => layout_spacing(&node.style, typed_layout, "box_size")
                .or_else(|| layout_spacing(&node.style, typed_layout, "size")),
            DocumentNodeKind::Button | DocumentNodeKind::Stack | DocumentNodeKind::TableCell
                if node.text.is_none() =>
            {
                layout_spacing(&node.style, typed_layout, "box_size")
            }
            _ => None,
        };
        let auto_width = layout_dimension_is_auto(&node.style, typed_layout, "width");
        let explicit_width =
            layout_dimension(&node.style, typed_layout, "width", available_width).or(box_size);
        let explicit_height =
            layout_dimension(&node.style, typed_layout, "height", available_height).or(box_size);
        let text = node.text.as_ref().map(|value| value.text.clone());
        let measurement_text = text
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                matches!(node.kind, DocumentNodeKind::TextInput)
                    .then(|| layout_text(&node.style, typed_layout, "placeholder"))
                    .flatten()
            });
        let font_size = layout_spacing(&node.style, typed_layout, "size").unwrap_or(14.0);
        let mut measured = measurement_text
            .filter(|value| !value.is_empty())
            .map(|value| self.text.measure_styled(value, font_size, &node.style))
            .unwrap_or(TextMetrics {
                width: 0.0,
                height: 0.0,
            });
        if matches!(node.kind, DocumentNodeKind::Text)
            && (node.style.contains_key("relief") || node.style.contains_key("depth"))
            && measured.width > 0.0
        {
            measured.width += 8.0;
        }
        let shrink_to_child_width = explicit_width.is_none()
            && text.is_none()
            && !node.children.is_empty()
            && matches!(
                node.kind,
                DocumentNodeKind::Button | DocumentNodeKind::Checkbox
            );
        let mut width = if auto_width {
            let auto_padding = layout_spacing(&node.style, typed_layout, "auto_padding")
                .unwrap_or_else(|| font_size * 0.9);
            (measured.width + auto_padding + padding.horizontal()).max(1.0)
        } else if shrink_to_child_width {
            padding.horizontal().max(1.0)
        } else {
            explicit_width
                .unwrap_or_else(|| measured.width.max(available_width))
                .max(1.0)
        };
        width =
            constrain_layout_dimension(width, &node.style, typed_layout, "width", available_width);
        let mut height =
            explicit_height.unwrap_or_else(|| measured.height.max(24.0) + padding.vertical());
        height = constrain_layout_dimension(
            height,
            &node.style,
            typed_layout,
            "height",
            available_height,
        );
        let style_identity = typed_record
            .as_ref()
            .map(|record| record.identity)
            .unwrap_or_else(|| computed_style_identity(&node.style));
        let mut display_style = node.style.clone();
        if matches!(node.kind, DocumentNodeKind::TextInput)
            && !display_style.contains_key("placeholder")
            && let Some(placeholder) = layout_text(&node.style, typed_layout, "placeholder")
        {
            display_style.insert(
                "placeholder".to_owned(),
                StyleValue::Text(placeholder.to_owned()),
            );
        }
        let centered = layout_bool(&node.style, typed_layout, "center").unwrap_or(false);
        let align_x = layout_text(&node.style, typed_layout, "align_x").unwrap_or_default();
        let mut node_x = if centered && width < available_width {
            x + (available_width - width) / 2.0
        } else if align_x.eq_ignore_ascii_case("right") && width < available_width {
            x + available_width - width
        } else {
            x
        };
        let display_index = self.display_list.len();
        self.display_list.push(DisplayItem {
            node: node.id.clone(),
            kind: node.kind.clone(),
            bounds: Rect {
                x: node_x,
                y,
                width,
                height,
            },
            text,
            style_identity,
            style: display_style,
            focused: self.document.focus.as_ref() == Some(&node.id),
        });
        let subtree_display_start = self.display_list.len();
        let subtree_hit_start = self.hit_regions.len();
        let subtree_scroll_start = self.scroll_regions.len();

        if !node.children.is_empty() {
            let content_x = node_x + padding.left;
            let content_y = y + padding.top;
            let content_width = (width - padding.horizontal()).max(1.0);
            match node.kind {
                DocumentNodeKind::Row => {
                    let display_start = self.display_list.len();
                    let hit_start = self.hit_regions.len();
                    let scroll_start = self.scroll_regions.len();
                    let child_count = node.children.len();
                    let fill_child_count = node
                        .children
                        .iter()
                        .filter(|child| {
                            self.document.nodes.get(child).is_some_and(|child| {
                                self.node_layout_dimension_is_fill(child, "width")
                            })
                        })
                        .count();
                    let row_gap_total = if child_count > 0 {
                        gap * child_count.saturating_sub(1) as f32
                    } else {
                        0.0
                    };
                    let fixed_child_width: f32 = if fill_child_count > 0 {
                        let mut fixed_child_width = 0.0;
                        for child in &node.children {
                            let Some(child_node) = self.document.nodes.get(child).cloned() else {
                                continue;
                            };
                            if self.node_layout_dimension_is_fill(&child_node, "width") {
                                continue;
                            }
                            fixed_child_width +=
                                self.preferred_row_child_width(&child_node).unwrap_or(0.0);
                        }
                        fixed_child_width
                    } else {
                        0.0
                    };
                    let fill_child_width = if fill_child_count > 0 {
                        ((content_width - row_gap_total - fixed_child_width)
                            / fill_child_count as f32)
                            .max(1.0)
                    } else {
                        0.0
                    };
                    let mut cursor_x = content_x;
                    let mut max_child_height: f32 = 0.0;
                    for child in &node.children {
                        let child_available_width = self
                            .document
                            .nodes
                            .get(child)
                            .cloned()
                            .and_then(|child_node| {
                                self.node_layout_dimension_is_fill(&child_node, "width")
                                    .then_some(fill_child_width)
                                    .or_else(|| {
                                        (fill_child_count > 0)
                                            .then(|| self.preferred_row_child_width(&child_node))
                                            .flatten()
                                    })
                            })
                            .unwrap_or_else(|| (content_x + content_width - cursor_x).max(1.0))
                            .max(1.0);
                        let child_rect = self.layout_node(
                            child,
                            cursor_x,
                            content_y,
                            child_available_width,
                            (height - padding.vertical()).max(1.0),
                        );
                        cursor_x += child_rect.width + gap;
                        max_child_height = max_child_height.max(child_rect.height);
                    }
                    if layout_bool(&node.style, typed_layout, "center").unwrap_or(false) {
                        let total_child_width = (cursor_x - content_x - gap).max(0.0);
                        let offset_x = ((content_width - total_child_width) / 2.0).max(0.0);
                        if offset_x > f32::EPSILON {
                            for item in &mut self.display_list[display_start..] {
                                item.bounds.x += offset_x;
                            }
                            for hit in &mut self.hit_regions[hit_start..] {
                                hit.bounds.x += offset_x;
                            }
                            for scroll in &mut self.scroll_regions[scroll_start..] {
                                scroll.bounds.x += offset_x;
                            }
                        }
                    }
                    if explicit_height.is_none() {
                        height = (max_child_height + padding.vertical()).max(24.0);
                    }
                }
                _ if layout_bool(&node.style, typed_layout, "overlay_children")
                    .unwrap_or(false) =>
                {
                    let mut max_child_width: f32 = 0.0;
                    let mut max_child_height: f32 = 0.0;
                    for child in &node.children {
                        let child_rect = self.layout_node(
                            child,
                            content_x,
                            content_y,
                            content_width,
                            (height - padding.vertical()).max(1.0),
                        );
                        max_child_width = max_child_width.max(child_rect.width);
                        max_child_height = max_child_height.max(child_rect.height);
                    }
                    if explicit_width.is_none() {
                        width = constrain_layout_dimension(
                            max_child_width.max(width).max(1.0) + padding.horizontal(),
                            &node.style,
                            typed_layout,
                            "width",
                            available_width,
                        );
                    }
                    if explicit_height.is_none() {
                        height = constrain_layout_dimension(
                            (max_child_height + padding.vertical()).max(24.0),
                            &node.style,
                            typed_layout,
                            "height",
                            available_height,
                        );
                    }
                }
                _ => {
                    let mut cursor_y = content_y;
                    let mut max_child_width: f32 = 0.0;
                    for child in &node.children {
                        let child_rect = self.layout_node(
                            child,
                            content_x,
                            cursor_y,
                            content_width,
                            (content_y + height - cursor_y).max(1.0),
                        );
                        cursor_y += child_rect.height + gap;
                        max_child_width = max_child_width.max(child_rect.width);
                    }
                    if explicit_width.is_none() {
                        width = if shrink_to_child_width {
                            let padded_button_safety =
                                if matches!(node.kind, DocumentNodeKind::Button)
                                    && padding.horizontal() > 0.0
                                {
                                    16.0
                                } else {
                                    0.0
                                };
                            (max_child_width + padding.horizontal() + padded_button_safety).max(1.0)
                        } else {
                            constrain_layout_dimension(
                                max_child_width.max(width).max(1.0) + padding.horizontal(),
                                &node.style,
                                typed_layout,
                                "width",
                                available_width,
                            )
                        };
                    }
                    if explicit_height.is_none() {
                        height = constrain_layout_dimension(
                            (cursor_y - y - gap).max(24.0) + padding.bottom,
                            &node.style,
                            typed_layout,
                            "height",
                            available_height,
                        );
                    }
                }
            }
        }

        let final_node_x = if centered && width < available_width {
            x + (available_width - width) / 2.0
        } else if align_x.eq_ignore_ascii_case("right") && width < available_width {
            x + available_width - width
        } else {
            x
        };
        let node_delta_x = final_node_x - node_x;
        if node_delta_x.abs() > f32::EPSILON {
            node_x = final_node_x;
            for item in &mut self.display_list[display_index..] {
                item.bounds.x += node_delta_x;
            }
            for hit in &mut self.hit_regions[subtree_hit_start..] {
                hit.bounds.x += node_delta_x;
            }
            for scroll in &mut self.scroll_regions[subtree_scroll_start..] {
                scroll.bounds.x += node_delta_x;
            }
        }

        let rect = Rect {
            x: node_x,
            y,
            width,
            height,
        };
        self.display_list[display_index].bounds = rect;
        if !node.materialized.is_empty() {
            apply_clip_to_display_items(&mut self.display_list[subtree_display_start..], rect);
        }
        let hover_scope = typed_record
            .as_ref()
            .map(|record| record.pseudo.hover_scope)
            .unwrap_or_else(|| style_bool(&node.style, "__hover_scope") == Some(true));
        if node.has_source_binding() || hover_scope {
            self.hit_regions.push(HitRegion {
                id: format!("hit:{}", node.id.0),
                node: node.id.clone(),
                bounds: rect,
            });
        }
        for range in &node.materialized {
            self.materialized_range_count += 1;
            let report = materialization_report(&node, range);
            self.scroll_regions.push(ScrollRegion {
                id: format!("scroll:{}", node.id.0),
                node: node.id.clone(),
                axis: range.axis,
                bounds: rect,
            });
            self.demands.push(demand_from_report(&report));
            self.materialization.push(report);
        }
        rect
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeSpacing {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl EdgeSpacing {
    fn horizontal(self) -> f32 {
        self.left + self.right
    }

    fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

fn style_edges(style: &BTreeMap<String, StyleValue>, prefix: &str) -> EdgeSpacing {
    let all = style_spacing(style, prefix).unwrap_or(0.0);
    EdgeSpacing {
        top: style_spacing(style, &format!("{prefix}_top")).unwrap_or(all),
        right: style_spacing(style, &format!("{prefix}_right")).unwrap_or(all),
        bottom: style_spacing(style, &format!("{prefix}_bottom")).unwrap_or(all),
        left: style_spacing(style, &format!("{prefix}_left")).unwrap_or(all),
    }
}

fn style_spacing(style: &BTreeMap<String, StyleValue>, key: &str) -> Option<f32> {
    match style.get(key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) => value
            .split(',')
            .next()
            .and_then(|value| value.trim().parse::<f32>().ok()),
        StyleValue::Bool(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

fn style_bool(style: &BTreeMap<String, StyleValue>, key: &str) -> Option<bool> {
    match style.get(key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => {
            None
        }
    }
}

fn apply_clip_to_display_items(items: &mut [DisplayItem], clip: Rect) {
    for item in items {
        let clip = item_clip_rect(item)
            .and_then(|existing| rect_intersection(existing, clip))
            .unwrap_or(clip);
        item.style
            .insert("__clip_x".to_owned(), StyleValue::Number(f64::from(clip.x)));
        item.style
            .insert("__clip_y".to_owned(), StyleValue::Number(f64::from(clip.y)));
        item.style.insert(
            "__clip_width".to_owned(),
            StyleValue::Number(f64::from(clip.width)),
        );
        item.style.insert(
            "__clip_height".to_owned(),
            StyleValue::Number(f64::from(clip.height)),
        );
        item.style_identity = computed_style_identity(&item.style);
    }
}

fn item_clip_rect(item: &DisplayItem) -> Option<Rect> {
    Some(Rect {
        x: style_spacing(&item.style, "__clip_x")?,
        y: style_spacing(&item.style, "__clip_y")?,
        width: style_spacing(&item.style, "__clip_width")?,
        height: style_spacing(&item.style, "__clip_height")?,
    })
}

fn rect_intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn style_text<'a>(style: &'a BTreeMap<String, StyleValue>, key: &str) -> Option<&'a str> {
    match style.get(key)? {
        StyleValue::Text(value) => Some(value.as_str()),
        StyleValue::Bool(_)
        | StyleValue::Number(_)
        | StyleValue::RichTextSpans(_)
        | StyleValue::EditorTypeHints(_) => None,
    }
}

fn style_dimension(
    style: &BTreeMap<String, StyleValue>,
    key: &str,
    fill_width: f32,
) -> Option<f32> {
    match style.get(key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) if value == "Fill" || value == "fill" => Some(fill_width),
        StyleValue::Text(value) => value.parse::<f32>().ok(),
        StyleValue::Bool(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

fn style_dimension_is_fill(style: &BTreeMap<String, StyleValue>, key: &str) -> bool {
    matches!(
        style.get(key),
        Some(StyleValue::Text(value)) if value.eq_ignore_ascii_case("fill")
    )
}

fn layout_edges(
    style: &BTreeMap<String, StyleValue>,
    typed_layout: Option<&DocumentTypedLayoutStyle>,
    prefix: &str,
) -> EdgeSpacing {
    if prefix == "padding" {
        if let Some(typed_layout) = typed_layout {
            return typed_edge_spacing_to_layout(typed_layout.padding);
        }
    }
    style_edges(style, prefix)
}

fn layout_spacing(
    style: &BTreeMap<String, StyleValue>,
    typed_layout: Option<&DocumentTypedLayoutStyle>,
    key: &str,
) -> Option<f32> {
    typed_layout
        .and_then(|typed_layout| match key {
            "gap" => typed_layout.gap,
            "size" => typed_layout.size,
            "box_size" => typed_layout.box_size,
            "auto_padding" => typed_layout.auto_padding,
            _ => None,
        })
        .or_else(|| style_spacing(style, key))
}

fn layout_bool(
    style: &BTreeMap<String, StyleValue>,
    typed_layout: Option<&DocumentTypedLayoutStyle>,
    key: &str,
) -> Option<bool> {
    typed_layout
        .and_then(|typed_layout| match key {
            "center" => Some(typed_layout.center),
            "overlay_children" => Some(typed_layout.overlay_children),
            _ => None,
        })
        .or_else(|| style_bool(style, key))
}

fn layout_text<'a>(
    style: &'a BTreeMap<String, StyleValue>,
    typed_layout: Option<&'a DocumentTypedLayoutStyle>,
    key: &str,
) -> Option<&'a str> {
    typed_layout
        .and_then(|typed_layout| match key {
            "align_x" => typed_layout.align_x.as_deref(),
            "placeholder" => typed_layout.placeholder.as_deref(),
            _ => None,
        })
        .or_else(|| style_text(style, key))
}

fn layout_dimension(
    style: &BTreeMap<String, StyleValue>,
    typed_layout: Option<&DocumentTypedLayoutStyle>,
    key: &str,
    fill_extent: f32,
) -> Option<f32> {
    typed_layout
        .and_then(|typed_layout| typed_layout_dimension(typed_layout, key))
        .and_then(|dimension| layout_dimension_value(dimension, fill_extent))
        .or_else(|| style_dimension(style, key, fill_extent))
}

fn layout_dimension_is_auto(
    style: &BTreeMap<String, StyleValue>,
    typed_layout: Option<&DocumentTypedLayoutStyle>,
    key: &str,
) -> bool {
    typed_layout
        .and_then(|typed_layout| typed_layout_dimension(typed_layout, key))
        .is_some_and(|dimension| matches!(dimension, DocumentStyleDimension::Auto))
        || style_text(style, key).is_some_and(|value| value.eq_ignore_ascii_case("auto"))
}

fn layout_dimension_is_fill(
    style: &BTreeMap<String, StyleValue>,
    typed_layout: Option<&DocumentTypedLayoutStyle>,
    key: &str,
) -> bool {
    typed_layout
        .and_then(|typed_layout| typed_layout_dimension(typed_layout, key))
        .is_some_and(|dimension| matches!(dimension, DocumentStyleDimension::Fill))
        || style_dimension_is_fill(style, key)
}

fn typed_layout_dimension(
    typed_layout: &DocumentTypedLayoutStyle,
    key: &str,
) -> Option<DocumentStyleDimension> {
    match key {
        "width" => typed_layout.width,
        "height" => typed_layout.height,
        "min_width" => typed_layout.min_width,
        "max_width" => typed_layout.max_width,
        "min_height" => typed_layout.min_height,
        "max_height" => typed_layout.max_height,
        _ => None,
    }
}

fn layout_dimension_value(dimension: DocumentStyleDimension, fill_extent: f32) -> Option<f32> {
    match dimension {
        DocumentStyleDimension::Px { value } => Some(value),
        DocumentStyleDimension::Fill => Some(fill_extent),
        DocumentStyleDimension::Auto => None,
    }
}

fn typed_edge_spacing_to_layout(spacing: DocumentTypedEdgeSpacing) -> EdgeSpacing {
    EdgeSpacing {
        top: spacing.top,
        right: spacing.right,
        bottom: spacing.bottom,
        left: spacing.left,
    }
}

fn typed_layout_style(style: &BTreeMap<String, StyleValue>) -> DocumentTypedLayoutStyle {
    DocumentTypedLayoutStyle {
        width: typed_style_dimension(style, "width"),
        height: typed_style_dimension(style, "height"),
        min_width: typed_style_dimension(style, "min_width"),
        max_width: typed_style_dimension(style, "max_width"),
        min_height: typed_style_dimension(style, "min_height"),
        max_height: typed_style_dimension(style, "max_height"),
        gap: style_spacing(style, "gap"),
        size: style_spacing(style, "size"),
        box_size: style_spacing(style, "box_size"),
        auto_padding: style_spacing(style, "auto_padding"),
        center: style_bool(style, "center").unwrap_or(false),
        align_x: style_text(style, "align_x").map(str::to_owned),
        overlay_children: style_bool(style, "overlay_children").unwrap_or(false),
        placeholder: style_text(style, "placeholder").map(str::to_owned),
        padding: typed_edge_spacing(style, "padding"),
        clip: typed_clip_rect(style),
    }
}

fn typed_paint_style(style: &BTreeMap<String, StyleValue>) -> DocumentTypedPaintStyle {
    DocumentTypedPaintStyle {
        color: style_text(style, "color").map(str::to_owned),
        background: style_text(style, "background").map(str::to_owned),
        background_color: style_text(style, "background_color").map(str::to_owned),
        border_color: style_text(style, "border_color").map(str::to_owned),
        opacity: style_spacing(style, "opacity"),
        relief: style_text(style, "relief").map(str::to_owned),
        depth: style_spacing(style, "depth"),
        shadow: style_text(style, "shadow").map(str::to_owned),
        outline: style_text(style, "outline").map(str::to_owned),
    }
}

fn typed_text_style(style: &BTreeMap<String, StyleValue>) -> DocumentTypedTextStyle {
    DocumentTypedTextStyle {
        size: style_spacing(style, "size"),
        font: style_text(style, "font").map(str::to_owned),
        font_family: style_text(style, "font_family").map(str::to_owned),
        font_weight: style_text(style, "font_weight").map(str::to_owned),
        font_style: style_text(style, "font_style").map(str::to_owned),
        line_height: style_spacing(style, "line_height"),
        letter_spacing: style_spacing(style, "letter_spacing"),
        text_align: style_text(style, "text_align").map(str::to_owned),
    }
}

fn typed_material_style(style: &BTreeMap<String, StyleValue>) -> DocumentTypedMaterialStyle {
    DocumentTypedMaterialStyle {
        material: style_text(style, "material").map(str::to_owned),
        texture: style_text(style, "texture").map(str::to_owned),
        image: style_text(style, "image").map(str::to_owned),
        shader: style_text(style, "shader").map(str::to_owned),
        border_radius: style_spacing(style, "border_radius"),
        clip: style_text(style, "clip").map(str::to_owned),
    }
}

fn typed_pseudo_style(style: &BTreeMap<String, StyleValue>) -> DocumentTypedPseudoStyle {
    DocumentTypedPseudoStyle {
        hover_scope: style_bool(style, "__hover_scope").unwrap_or(false),
    }
}

fn typed_style_dimension(
    style: &BTreeMap<String, StyleValue>,
    key: &str,
) -> Option<DocumentStyleDimension> {
    match style.get(key)? {
        StyleValue::Number(value) => Some(DocumentStyleDimension::Px {
            value: *value as f32,
        }),
        StyleValue::Text(value) if value.eq_ignore_ascii_case("fill") => {
            Some(DocumentStyleDimension::Fill)
        }
        StyleValue::Text(value) if value.eq_ignore_ascii_case("auto") => {
            Some(DocumentStyleDimension::Auto)
        }
        StyleValue::Text(value) => value
            .parse::<f32>()
            .ok()
            .map(|value| DocumentStyleDimension::Px { value }),
        StyleValue::Bool(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

fn typed_edge_spacing(
    style: &BTreeMap<String, StyleValue>,
    prefix: &str,
) -> DocumentTypedEdgeSpacing {
    let spacing = style_edges(style, prefix);
    DocumentTypedEdgeSpacing {
        top: spacing.top,
        right: spacing.right,
        bottom: spacing.bottom,
        left: spacing.left,
    }
}

fn typed_clip_rect(style: &BTreeMap<String, StyleValue>) -> Option<Rect> {
    Some(Rect {
        x: style_spacing(style, "__clip_x")?,
        y: style_spacing(style, "__clip_y")?,
        width: style_spacing(style, "__clip_width")?,
        height: style_spacing(style, "__clip_height")?,
    })
}

fn preferred_row_child_width_with_typed(
    node: &DocumentNode,
    typed_layout: Option<&DocumentTypedLayoutStyle>,
    text: &mut dyn TextMeasurer,
) -> Option<f32> {
    let padding = layout_edges(&node.style, typed_layout, "padding");
    let box_size = match node.kind {
        DocumentNodeKind::Checkbox => layout_spacing(&node.style, typed_layout, "box_size")
            .or_else(|| layout_spacing(&node.style, typed_layout, "size")),
        DocumentNodeKind::Button | DocumentNodeKind::Stack | DocumentNodeKind::TableCell
            if node.text.is_none() =>
        {
            layout_spacing(&node.style, typed_layout, "box_size")
        }
        _ => None,
    };
    let font_size = layout_spacing(&node.style, typed_layout, "size").unwrap_or(14.0);
    if layout_dimension_is_auto(&node.style, typed_layout, "width") {
        let auto_padding = layout_spacing(&node.style, typed_layout, "auto_padding")
            .unwrap_or_else(|| font_size * 0.9);
        let measured_width = row_child_measurement_text(node, typed_layout)
            .map(|value| text.measure_styled(value, font_size, &node.style).width)
            .unwrap_or(0.0);
        return Some((measured_width + auto_padding + padding.horizontal()).max(1.0));
    }
    layout_dimension(&node.style, typed_layout, "width", 0.0)
        .or(box_size)
        .or_else(|| {
            row_child_measurement_text(node, typed_layout).map(|value| {
                let mut measured_width = text.measure_styled(value, font_size, &node.style).width;
                if matches!(node.kind, DocumentNodeKind::Text)
                    && (node.style.contains_key("relief") || node.style.contains_key("depth"))
                    && measured_width > 0.0
                {
                    measured_width += 8.0;
                }
                (measured_width + padding.horizontal()).max(1.0)
            })
        })
}

fn row_child_measurement_text<'a>(
    node: &'a DocumentNode,
    typed_layout: Option<&'a DocumentTypedLayoutStyle>,
) -> Option<&'a str> {
    node.text
        .as_ref()
        .map(|value| value.text.as_str())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            matches!(node.kind, DocumentNodeKind::TextInput)
                .then(|| layout_text(&node.style, typed_layout, "placeholder"))
                .flatten()
                .filter(|value| !value.is_empty())
        })
}

fn constrain_layout_dimension(
    value: f32,
    style: &BTreeMap<String, StyleValue>,
    typed_layout: Option<&DocumentTypedLayoutStyle>,
    key: &str,
    fill_extent: f32,
) -> f32 {
    let min = layout_dimension(style, typed_layout, &format!("min_{key}"), fill_extent);
    let max = layout_dimension(style, typed_layout, &format!("max_{key}"), fill_extent);
    let mut constrained = value;
    if let Some(min) = min {
        constrained = constrained.max(min);
    }
    if let Some(max) = max {
        constrained = constrained.min(max);
    }
    constrained.max(1.0)
}

#[derive(Default)]
pub struct SimpleTextMeasurer;

impl TextMeasurer for SimpleTextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32) -> TextMetrics {
        TextMetrics {
            width: text.chars().count() as f32 * font_size,
            height: font_size * 1.4,
        }
    }
}

pub fn fixture_frame_with_virtualized_table() -> DocumentFrame {
    let mut frame = DocumentFrame::empty("root");
    let mut table = DocumentNode::new(
        "virtual-table",
        boon_document_model::DocumentNodeKind::Table,
    );
    table.parent = Some(frame.root.clone());
    table.text = Some(TextValue {
        text: "Virtualized logical table".to_owned(),
    });
    table.materialized.push(MaterializedRange {
        axis: Axis::Vertical,
        visible: 0..20,
        overscan: 0..28,
    });
    table.materialized.push(MaterializedRange {
        axis: Axis::Horizontal,
        visible: 0..8,
        overscan: 0..12,
    });
    if let Some(root) = frame.nodes.get_mut(&frame.root) {
        root.children.push(table.id.clone());
    }
    frame.nodes.insert(table.id.clone(), table);
    frame
}

fn apply_style_patch(
    style: &mut BTreeMap<String, StyleValue>,
    patch: StylePatch,
) -> BTreeSet<String> {
    let mut changed_keys = BTreeSet::new();
    for (key, value) in patch {
        match value {
            Some(value) => {
                if style.get(&key) != Some(&value) {
                    changed_keys.insert(key.clone());
                    style.insert(key, value);
                }
            }
            None => {
                if style.remove(&key).is_some() {
                    changed_keys.insert(key);
                }
            }
        }
    }
    changed_keys
}

fn style_patch_invalidation(changed_keys: &BTreeSet<String>) -> Vec<PatchInvalidationClass> {
    let mut invalidation = vec![PatchInvalidationClass::Style];
    if changed_keys.is_empty() {
        push_unique_invalidation(&mut invalidation, PatchInvalidationClass::PaintOnly);
        push_unique_invalidation(&mut invalidation, PatchInvalidationClass::LayoutOnly);
        push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
        return invalidation;
    }
    for key in changed_keys {
        if !style_key_is_known(key) {
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::FullDocument);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::Layout);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::LayoutOnly);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::PaintOnly);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
            continue;
        }
        if style_key_affects_layout(key) {
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::Layout);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::LayoutOnly);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
        }
        if style_key_affects_paint(key) || style_key_affects_material(key) {
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::PaintOnly);
        }
        if style_key_affects_pseudo_state(key) {
            push_unique_invalidation(
                &mut invalidation,
                PatchInvalidationClass::ConditionalStructure,
            );
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
        }
        if style_key_affects_source_binding(key) {
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::SourceBinding);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
        }
    }
    if invalidation.len() == 1 {
        push_unique_invalidation(&mut invalidation, PatchInvalidationClass::PaintOnly);
    }
    invalidation
}

fn push_unique_invalidation(
    invalidation: &mut Vec<PatchInvalidationClass>,
    class: PatchInvalidationClass,
) {
    if !invalidation.contains(&class) {
        invalidation.push(class);
    }
}

fn computed_style_identity(style: &BTreeMap<String, StyleValue>) -> ComputedStyleIdentity {
    ComputedStyleIdentity {
        style_id: stable_style_hash(style, StyleHashCategory::All),
        layout_id: stable_style_hash(style, StyleHashCategory::Layout),
        paint_id: stable_style_hash(style, StyleHashCategory::Paint),
        material_id: stable_style_hash(style, StyleHashCategory::Material),
        font_id: stable_style_hash(style, StyleHashCategory::Font),
        pseudo_state_id: stable_style_hash(style, StyleHashCategory::PseudoState),
    }
}

#[derive(Clone, Copy)]
enum StyleHashCategory {
    All,
    Layout,
    Paint,
    Material,
    Font,
    PseudoState,
    Clip,
}

fn stable_style_hash(style: &BTreeMap<String, StyleValue>, category: StyleHashCategory) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    stable_hash_bytes(&mut hash, b"boon-style-v1");
    for (key, value) in style {
        if !style_key_in_hash_category(key, category) {
            continue;
        }
        stable_hash_bytes(&mut hash, key.as_bytes());
        stable_hash_bytes(&mut hash, &[0]);
        stable_hash_style_value(&mut hash, value);
        stable_hash_bytes(&mut hash, &[0xff]);
    }
    hash
}

fn stable_hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn stable_hash_style_value(hash: &mut u64, value: &StyleValue) {
    match value {
        StyleValue::Text(value) => {
            stable_hash_bytes(hash, b"text:");
            stable_hash_bytes(hash, value.as_bytes());
        }
        StyleValue::Number(value) => {
            stable_hash_bytes(hash, b"number:");
            stable_hash_bytes(hash, &value.to_bits().to_le_bytes());
        }
        StyleValue::Bool(value) => {
            stable_hash_bytes(hash, b"bool:");
            stable_hash_bytes(hash, &[*value as u8]);
        }
        StyleValue::RichTextSpans(spans) => {
            stable_hash_bytes(hash, b"rich_text_spans:");
            stable_hash_bytes(hash, &spans.len().to_le_bytes());
            for span in spans {
                stable_hash_bytes(hash, span.text.as_bytes());
                stable_hash_optional_text(hash, span.source_text.as_deref());
                stable_hash_optional_text(hash, span.color.as_deref());
                stable_hash_optional_text(hash, span.font_style.as_deref());
                stable_hash_optional_text(hash, span.font_weight.as_deref());
            }
        }
        StyleValue::EditorTypeHints(hints) => {
            stable_hash_bytes(hash, b"editor_type_hints:");
            stable_hash_bytes(hash, &hints.len().to_le_bytes());
            for hint in hints {
                stable_hash_bytes(hash, &hint.line.to_le_bytes());
                stable_hash_bytes(hash, &hint.start.to_le_bytes());
                stable_hash_bytes(hash, &hint.end.to_le_bytes());
                stable_hash_bytes(hash, &hint.anchor_column.to_le_bytes());
                stable_hash_bytes(hash, hint.category.as_bytes());
                stable_hash_bytes(hash, hint.compact_label.as_bytes());
                stable_hash_bytes(hash, hint.detail_label.as_bytes());
            }
        }
    }
}

fn stable_hash_optional_text(hash: &mut u64, value: Option<&str>) {
    match value {
        Some(value) => {
            stable_hash_bytes(hash, &[1]);
            stable_hash_bytes(hash, value.as_bytes());
        }
        None => stable_hash_bytes(hash, &[0]),
    }
}

fn stable_style_intern_key(
    style: &BTreeMap<String, StyleValue>,
    category: StyleHashCategory,
) -> String {
    let mut key = String::new();
    key.push_str("boon-style-v1:");
    key.push_str(style_hash_category_name(category));
    key.push(':');
    for (name, value) in style {
        if !style_key_in_hash_category(name, category) {
            continue;
        }
        push_key_text(&mut key, name);
        key.push('=');
        push_style_value_key(&mut key, value);
        key.push(';');
    }
    key
}

fn stable_source_binding_key(id: &str, source_path: &str, intent: &str) -> String {
    let mut key = String::from("boon-binding-v1:");
    push_key_text(&mut key, id);
    key.push('|');
    push_key_text(&mut key, source_path);
    key.push('|');
    push_key_text(&mut key, intent);
    key
}

fn style_hash_category_name(category: StyleHashCategory) -> &'static str {
    match category {
        StyleHashCategory::All => "all",
        StyleHashCategory::Layout => "layout",
        StyleHashCategory::Paint => "paint",
        StyleHashCategory::Material => "material",
        StyleHashCategory::Font => "font",
        StyleHashCategory::PseudoState => "pseudo_state",
        StyleHashCategory::Clip => "clip",
    }
}

fn push_key_text(key: &mut String, value: &str) {
    key.push_str(&value.len().to_string());
    key.push(':');
    key.push_str(value);
}

fn push_style_value_key(key: &mut String, value: &StyleValue) {
    match value {
        StyleValue::Text(value) => {
            key.push_str("text(");
            push_key_text(key, value);
            key.push(')');
        }
        StyleValue::Number(value) => {
            key.push_str("number(");
            key.push_str(&format!("{:016x}", value.to_bits()));
            key.push(')');
        }
        StyleValue::Bool(value) => {
            key.push_str("bool(");
            key.push_str(if *value { "1" } else { "0" });
            key.push(')');
        }
        StyleValue::RichTextSpans(spans) => {
            key.push_str("rich_text_spans(");
            key.push_str(&spans.len().to_string());
            for span in spans {
                key.push('|');
                push_key_text(key, &span.text);
                key.push('|');
                push_optional_key_text(key, span.source_text.as_deref());
                key.push('|');
                push_optional_key_text(key, span.color.as_deref());
                key.push('|');
                push_optional_key_text(key, span.font_style.as_deref());
                key.push('|');
                push_optional_key_text(key, span.font_weight.as_deref());
            }
            key.push(')');
        }
        StyleValue::EditorTypeHints(hints) => {
            key.push_str("editor_type_hints(");
            key.push_str(&hints.len().to_string());
            for hint in hints {
                key.push('|');
                key.push_str(&hint.line.to_string());
                key.push(',');
                key.push_str(&hint.start.to_string());
                key.push(',');
                key.push_str(&hint.end.to_string());
                key.push(',');
                key.push_str(&hint.anchor_column.to_string());
                key.push(',');
                push_key_text(key, &hint.category);
                key.push(',');
                push_key_text(key, &hint.compact_label);
                key.push(',');
                push_key_text(key, &hint.detail_label);
            }
            key.push(')');
        }
    }
}

fn push_optional_key_text(key: &mut String, value: Option<&str>) {
    match value {
        Some(value) => {
            key.push_str("some(");
            push_key_text(key, value);
            key.push(')');
        }
        None => key.push_str("none"),
    }
}

fn style_key_in_hash_category(key: &str, category: StyleHashCategory) -> bool {
    match category {
        StyleHashCategory::All => true,
        StyleHashCategory::Layout => style_key_affects_layout(key),
        StyleHashCategory::Paint => style_key_affects_paint(key),
        StyleHashCategory::Material => style_key_affects_material(key),
        StyleHashCategory::Font => style_key_affects_font(key),
        StyleHashCategory::PseudoState => style_key_affects_pseudo_state(key),
        StyleHashCategory::Clip => style_key_affects_clip(key),
    }
}

fn style_key_is_known(key: &str) -> bool {
    style_key_affects_layout(key)
        || style_key_affects_paint(key)
        || style_key_affects_material(key)
        || style_key_affects_font(key)
        || style_key_affects_pseudo_state(key)
        || style_key_affects_source_binding(key)
}

fn style_key_affects_layout(key: &str) -> bool {
    key == "width"
        || key == "height"
        || key == "min_width"
        || key == "max_width"
        || key == "min_height"
        || key == "max_height"
        || key == "gap"
        || key == "size"
        || key == "box_size"
        || key == "auto_padding"
        || key == "center"
        || key == "align_x"
        || key == "overlay_children"
        || key == "placeholder"
        || key.starts_with("padding")
        || key.starts_with("__clip_")
}

fn style_key_affects_paint(key: &str) -> bool {
    key == "color"
        || key == "background"
        || key == "background_color"
        || key == "border_color"
        || key == "opacity"
        || key == "relief"
        || key == "depth"
        || key == "shadow"
        || key == "outline"
        || key.starts_with("__selected_")
}

fn style_key_affects_material(key: &str) -> bool {
    key == "material"
        || key == "texture"
        || key == "image"
        || key == "shader"
        || key == "border_radius"
        || key == "clip"
}

fn style_key_affects_clip(key: &str) -> bool {
    key == "clip" || key.starts_with("__clip_")
}

fn style_key_affects_font(key: &str) -> bool {
    key == "size"
        || key == "font"
        || key == "font_family"
        || key == "font_weight"
        || key == "font_style"
        || key == "line_height"
        || key == "letter_spacing"
        || key == "text_align"
}

fn style_key_affects_pseudo_state(key: &str) -> bool {
    key == "__hover_scope"
        || key == "hover"
        || key == "focus"
        || key == "active"
        || key == "disabled"
        || key == "selected"
        || key == "checked"
}

fn style_key_affects_source_binding(key: &str) -> bool {
    key == "source_intent" || key == "source_binding" || key == "__source_binding"
}

fn materialization_report(
    node: &DocumentNode,
    materialized: &MaterializedRange,
) -> MaterializationReport {
    let logical_item_count = materialized
        .overscan
        .end
        .max(materialized.visible.end)
        .max(materialized.overscan.start)
        .max(materialized.visible.start);
    let materialized_item_count = materialized
        .overscan
        .end
        .saturating_sub(materialized.overscan.start);
    let stable_key_prefix = stable_materialization_key_prefix(node, materialized.axis);
    MaterializationReport {
        node: node.id.clone(),
        axis: materialized.axis,
        visible: materialized.visible.clone(),
        overscan: materialized.overscan.clone(),
        logical_item_count,
        materialized_item_count,
        first_stable_key: (materialized_item_count > 0)
            .then(|| format!("{}:{}", stable_key_prefix, materialized.overscan.start)),
        last_stable_key: (materialized_item_count > 0).then(|| {
            format!(
                "{}:{}",
                stable_key_prefix,
                materialized.overscan.end.saturating_sub(1)
            )
        }),
        stable_key_prefix,
    }
}

fn stable_materialization_key_prefix(node: &DocumentNode, axis: Axis) -> String {
    let axis = match axis {
        Axis::Horizontal => "x",
        Axis::Vertical => "y",
    };
    format!("materialized:{}:{axis}", node.id.0)
}

fn demand_from_report(report: &MaterializationReport) -> LayoutDemand {
    LayoutDemand {
        node: report.node.clone(),
        axis: report.axis,
        visible: report.visible.clone(),
        overscan: report.overscan.clone(),
        logical_item_count: report.logical_item_count,
        materialized_item_count: report.materialized_item_count,
        stable_key_prefix: report.stable_key_prefix.clone(),
        first_stable_key: report.first_stable_key.clone(),
        last_stable_key: report.last_stable_key.clone(),
    }
}

#[cfg(test)]
mod tests;
