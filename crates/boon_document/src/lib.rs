pub use boon_document_model::{
    Axis, DocumentFrame, DocumentNode, DocumentNodeId, DocumentNodeKind, DocumentPatch,
    MaterializedRange, ScrollRootId, SourceBindingId, StyleEditorTypeHint, StyleMap, StylePatch,
    StyleRichTextSpan, StyleValue, TextValue,
};
pub mod render_scene;
use boon_host::Viewport;
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
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
            let binding = node.and_then(|node| node.source_binding.as_ref());
            let spatial_bucket = hit_bucket_for_point(hit.bounds.x, hit.bounds.y, bucket_size);
            let entry_index = table.entries.len();
            let entry = HitSideTableEntry {
                hit_id: hit.id.clone(),
                node: hit.node.clone(),
                source_binding_id: binding.map(|binding| binding.id.clone()),
                source_path: binding.map(|binding| binding.source_path.clone()),
                source_intent: binding.map(|binding| binding.intent.clone()),
                source_binding_refs: Vec::new(),
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
        self.entries
            .iter()
            .find(|entry| entry.source_path.as_deref() == Some(source_path))
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
    let source_binding = node.source_binding.as_ref();
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
            document_node.source_binding = Some(boon_document_model::SourceBinding {
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticId(pub String);

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticAction {
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
pub struct SemanticWebSourceDispatch {
    pub semantic_id: SemanticId,
    pub node: DocumentNodeId,
    pub source_path: String,
    pub source_intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

pub type SemanticSourceDispatch = SemanticWebSourceDispatch;

impl SemanticScene {
    pub fn from_document_layout(document: &DocumentFrame, layout: &LayoutFrame) -> Self {
        let mut display_by_node = BTreeMap::new();
        for item in &layout.display_list {
            display_by_node
                .entry(item.node.clone())
                .or_insert_with(|| item.clone());
        }

        let mut scene = Self {
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

impl SemanticId {
    pub fn from_document_node_id(node: &DocumentNodeId) -> Self {
        Self(format!("semantic:{}", node.0))
    }

    pub fn from_world_editor_node_id(node: &boon_scene_model::WorldSemanticEditorNodeId) -> Self {
        Self(format!("semantic:{}", node.0))
    }
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DocumentHotNodeId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DocumentHotNodeGeneration(pub u64);

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DocumentInternId(pub u32);

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
    pub source_binding: Option<DocumentInternId>,
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
        let source_binding = node.source_binding.as_ref().map(|binding| {
            self.source_bindings.intern(stable_source_binding_key(
                &binding.id.0,
                &binding.source_path,
                &binding.intent,
            ))
        });
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
                source_binding,
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
            let Some(binding) = &node.source_binding else {
                continue;
            };
            let interned = intern_index.nodes.get(&node_ref.id).ok_or_else(|| {
                PatchApplyError::StaleReference {
                    reference_kind: "document_intern_index",
                    id: node_id.clone(),
                }
            })?;
            let intern_id =
                interned
                    .source_binding
                    .ok_or_else(|| PatchApplyError::StaleReference {
                        reference_kind: "document_intern_index_source_binding",
                        id: node_id.clone(),
                    })?;
            let reference = DocumentTypedBindingRef {
                node: node_ref.id,
                ordinal: 0,
            };
            let route = DocumentTypedBindingRoute {
                source_path: binding.source_path.clone(),
                intent: binding.intent.clone(),
            };
            let typed = DocumentTypedBinding {
                node: node_ref,
                reference,
                binding_id: binding.id.clone(),
                route: route.clone(),
                intern_id,
            };
            index.nodes.insert(
                node_ref.id,
                DocumentTypedBindingNode {
                    node: node_ref,
                    bindings: vec![typed],
                },
            );
            index
                .by_binding_id
                .entry(binding.id.clone())
                .or_default()
                .push(reference);
            index.by_route.entry(route).or_default().push(reference);
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

        let Some(binding) = &node.source_binding else {
            return Ok(());
        };
        let interned =
            intern_index
                .nodes
                .get(&hot_ref.id)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "document_intern_index",
                    id: node_id.clone(),
                })?;
        let intern_id = interned
            .source_binding
            .ok_or_else(|| PatchApplyError::StaleReference {
                reference_kind: "document_intern_index_source_binding",
                id: node_id.clone(),
            })?;
        let reference = DocumentTypedBindingRef {
            node: hot_ref.id,
            ordinal: 0,
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
        self.nodes.insert(
            hot_ref.id,
            DocumentTypedBindingNode {
                node: hot_ref,
                bindings: vec![typed],
            },
        );
        self.by_binding_id
            .entry(binding.id.clone())
            .or_default()
            .push(reference);
        self.by_route.entry(route).or_default().push(reference);
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
            node.source_binding = Some(binding);
            Ok(PatchApplyReport {
                patch_kind: "set_binding",
                target: Some(id),
                invalidation: vec![
                    PatchInvalidationClass::Binding,
                    PatchInvalidationClass::SourceBinding,
                    PatchInvalidationClass::HitRegion,
                ],
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
            style: node.style.clone(),
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
        if node.source_binding.is_some() || hover_scope {
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
mod tests {
    use super::*;

    fn node(id: &str, kind: DocumentNodeKind, parent: Option<&str>) -> DocumentNode {
        let mut node = DocumentNode::new(id, kind);
        node.parent = parent.map(|parent| DocumentNodeId(parent.to_owned()));
        node
    }

    #[test]
    fn semantic_scene_derives_stable_roles_bounds_actions_and_patch() {
        let mut frame = DocumentFrame::empty("root");
        let mut title = node("title", DocumentNodeKind::Text, Some("root"));
        title.text = Some(TextValue {
            text: "Inbox".to_owned(),
        });
        title
            .style
            .insert("heading_level".to_owned(), StyleValue::Number(2.0));

        let mut button = node("save", DocumentNodeKind::Button, Some("root"));
        button.text = Some(TextValue {
            text: "Save".to_owned(),
        });
        button.source_binding = Some(boon_document_model::SourceBinding {
            id: SourceBindingId("source:save:press".to_owned()),
            source_path: "toolbar.save".to_owned(),
            intent: "press".to_owned(),
        });

        let mut checkbox = node("done", DocumentNodeKind::Checkbox, Some("root"));
        checkbox
            .style
            .insert("checked".to_owned(), StyleValue::Bool(true));
        checkbox.style.insert(
            "accessibility_label".to_owned(),
            StyleValue::Text("Done".to_owned()),
        );

        let mut input = node("filter", DocumentNodeKind::TextInput, Some("root"));
        input.text = Some(TextValue {
            text: "abc".to_owned(),
        });
        input.style.insert(
            "placeholder".to_owned(),
            StyleValue::Text("Filter".to_owned()),
        );
        frame.focus = Some(input.id.clone());

        frame.nodes.get_mut(&frame.root).unwrap().children.extend([
            title.id.clone(),
            button.id.clone(),
            checkbox.id.clone(),
            input.id.clone(),
        ]);
        frame.nodes.insert(title.id.clone(), title);
        frame.nodes.insert(button.id.clone(), button);
        frame.nodes.insert(checkbox.id.clone(), checkbox);
        frame.nodes.insert(input.id.clone(), input);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 320.0,
                height: 180.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let scene = SemanticScene::from_document_layout(&frame, &layout);
        assert_eq!(
            scene.root,
            Some(SemanticId("semantic:root".to_owned())),
            "root semantic id must be stable and document-derived"
        );
        assert_eq!(
            scene.focused,
            Some(SemanticId("semantic:filter".to_owned())),
            "document focus must project into the SemanticScene"
        );

        let button_semantic = scene
            .nodes
            .get(&SemanticId("semantic:save".to_owned()))
            .expect("button semantic node should exist");
        assert_eq!(button_semantic.role, SemanticRole::Button);
        assert_eq!(button_semantic.name.as_deref(), Some("Save"));
        assert!(button_semantic.actions.press);
        assert!(button_semantic.actions.focus);
        assert!(button_semantic.bounds.is_some());
        assert_eq!(
            button_semantic.source_binding_id,
            Some(SourceBindingId("source:save:press".to_owned()))
        );
        assert_eq!(button_semantic.source_path.as_deref(), Some("toolbar.save"));

        let checkbox_semantic = scene
            .nodes
            .get(&SemanticId("semantic:done".to_owned()))
            .expect("checkbox semantic node should exist");
        assert_eq!(checkbox_semantic.role, SemanticRole::Checkbox);
        assert_eq!(checkbox_semantic.name.as_deref(), Some("Done"));
        assert_eq!(checkbox_semantic.state.checked, Some(true));
        assert_eq!(
            checkbox_semantic.value,
            Some(SemanticValue::Bool { value: true })
        );

        let title_semantic = scene
            .nodes
            .get(&SemanticId("semantic:title".to_owned()))
            .expect("text semantic node should exist");
        assert_eq!(title_semantic.role, SemanticRole::Text);
        assert_eq!(title_semantic.heading_level, Some(2));

        let mut next = scene.clone();
        next.nodes.remove(&SemanticId("semantic:done".to_owned()));
        let mut changed_button = next
            .nodes
            .remove(&SemanticId("semantic:save".to_owned()))
            .unwrap();
        changed_button.name = Some("Save now".to_owned());
        next.nodes.insert(changed_button.id.clone(), changed_button);
        next.focused = Some(SemanticId("semantic:save".to_owned()));

        let patch = scene.diff(&next);
        assert!(patch.operations.iter().any(|operation| matches!(
            operation,
            SemanticPatchOperation::RemoveNode { id } if id.0 == "semantic:done"
        )));
        assert!(patch.operations.iter().any(|operation| matches!(
            operation,
            SemanticPatchOperation::UpsertNode { node } if node.id.0 == "semantic:save"
                && node.name.as_deref() == Some("Save now")
        )));
        assert!(patch.operations.iter().any(|operation| matches!(
            operation,
            SemanticPatchOperation::SetFocus { focused: Some(id) } if id.0 == "semantic:save"
        )));
    }

    #[test]
    fn semantic_scene_lowers_world_editor_tree_actions_for_accessibility() {
        let root = boon_scene_model::WorldSemanticEditorNodeId("world-editor:root".to_owned());
        let assembly =
            boon_scene_model::WorldSemanticEditorNodeId("world-editor:assembly".to_owned());
        let wheel =
            boon_scene_model::WorldSemanticEditorNodeId("world-editor:part:front-left".to_owned());
        let manufacturing =
            boon_scene_model::WorldSemanticEditorNodeId("world-editor:manufacturing".to_owned());
        let export = boon_scene_model::WorldSemanticEditorNodeId(
            "world-editor:manufacturing:export-3mf".to_owned(),
        );
        let mut nodes = std::collections::BTreeMap::new();
        nodes.insert(
            root.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: root.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::Editor,
                label: "Car editor".to_owned(),
                children: vec![assembly.clone(), manufacturing.clone()],
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: boon_scene_model::WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            assembly.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: assembly.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::Assembly,
                label: "Car assembly".to_owned(),
                children: vec![wheel.clone()],
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: boon_scene_model::WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            wheel.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: wheel.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::PartInstance,
                label: "Front-left wheel".to_owned(),
                children: Vec::new(),
                instance: Some(boon_scene_model::InstanceId(7)),
                part_id: Some(boon_scene_model::PartId(2)),
                feature_id: Some(boon_scene_model::FeatureId(22)),
                pick_id: Some(boon_scene_model::PickId(4)),
                manufacturing_role: None,
                physical_material: Some(boon_scene_model::PhysicalMaterialId(2)),
                selected: true,
                visible: true,
                exportable: true,
                actions: boon_scene_model::WorldSemanticEditorActions {
                    focus: true,
                    select: true,
                    ..boon_scene_model::WorldSemanticEditorActions::default()
                },
            },
        );
        nodes.insert(
            manufacturing.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: manufacturing.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::Manufacturing,
                label: "Manufacturing".to_owned(),
                children: vec![export.clone()],
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: true,
                actions: boon_scene_model::WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            export.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: export.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::Action,
                label: "Export 3MF".to_owned(),
                children: Vec::new(),
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: true,
                actions: boon_scene_model::WorldSemanticEditorActions {
                    focus: true,
                    export_3mf: true,
                    ..boon_scene_model::WorldSemanticEditorActions::default()
                },
            },
        );
        let mut tree = boon_scene_model::WorldSemanticEditorTree {
            root: root.clone(),
            focused: Some(wheel.clone()),
            nodes,
            metrics: boon_scene_model::WorldSemanticEditorTreeMetrics::default(),
        };
        tree.metrics = tree.compute_metrics();

        let scene = SemanticScene::from_world_editor_tree(&tree);
        let bridge = SemanticWebBridgeSnapshot::from_scene(&scene);
        let export_id = SemanticId::from_world_editor_node_id(&export);
        let wheel_id = SemanticId::from_world_editor_node_id(&wheel);
        let export_node = scene.nodes.get(&export_id).expect("export semantic node");
        let wheel_node = scene.nodes.get(&wheel_id).expect("wheel semantic node");

        assert_eq!(
            scene.root,
            Some(SemanticId::from_world_editor_node_id(&root))
        );
        assert_eq!(scene.focused, Some(wheel_id.clone()));
        assert_eq!(scene.nodes.len(), tree.nodes.len());
        assert_eq!(export_node.role, SemanticRole::Button);
        assert_eq!(export_node.name.as_deref(), Some("Export 3MF"));
        assert!(export_node.actions.press);
        assert_eq!(
            export_node.source_path.as_deref(),
            Some("world.manufacturing.export_3mf")
        );
        assert_eq!(export_node.source_intent.as_deref(), Some("press"));
        assert_eq!(wheel_node.role, SemanticRole::Button);
        assert!(wheel_node.state.selected);
        assert_eq!(
            wheel_node.source_path.as_deref(),
            Some("world.instance.7.select")
        );
        assert!(bridge.action_routes.iter().any(|route| {
            route.semantic_id == export_id
                && route.action == SemanticWebAction::Press
                && route.source_path.as_deref() == Some("world.manufacturing.export_3mf")
        }));
    }

    #[test]
    fn world_editor_tree_projects_to_source_bound_document_controls() {
        let root = boon_scene_model::WorldSemanticEditorNodeId("world-editor:root".to_owned());
        let assembly =
            boon_scene_model::WorldSemanticEditorNodeId("world-editor:assembly".to_owned());
        let wheel =
            boon_scene_model::WorldSemanticEditorNodeId("world-editor:part:front-left".to_owned());
        let manufacturing =
            boon_scene_model::WorldSemanticEditorNodeId("world-editor:manufacturing".to_owned());
        let export = boon_scene_model::WorldSemanticEditorNodeId(
            "world-editor:manufacturing:export-3mf".to_owned(),
        );
        let mut nodes = BTreeMap::new();
        nodes.insert(
            root.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: root.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::Editor,
                label: "Car editor".to_owned(),
                children: vec![assembly.clone(), manufacturing.clone()],
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: boon_scene_model::WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            assembly.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: assembly.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::Assembly,
                label: "Car assembly".to_owned(),
                children: vec![wheel.clone()],
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: boon_scene_model::WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            wheel.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: wheel.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::PartInstance,
                label: "Front-left wheel".to_owned(),
                children: Vec::new(),
                instance: Some(boon_scene_model::InstanceId(7)),
                part_id: Some(boon_scene_model::PartId(3)),
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: Some(boon_scene_model::PhysicalMaterialId(4)),
                selected: true,
                visible: true,
                exportable: true,
                actions: boon_scene_model::WorldSemanticEditorActions {
                    focus: true,
                    select: true,
                    ..boon_scene_model::WorldSemanticEditorActions::default()
                },
            },
        );
        nodes.insert(
            manufacturing.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: manufacturing.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::Manufacturing,
                label: "Manufacturing".to_owned(),
                children: vec![export.clone()],
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: boon_scene_model::WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            export.clone(),
            boon_scene_model::WorldSemanticEditorNode {
                id: export.clone(),
                role: boon_scene_model::WorldSemanticEditorRole::Action,
                label: "Export 3MF".to_owned(),
                children: Vec::new(),
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: true,
                actions: boon_scene_model::WorldSemanticEditorActions {
                    focus: true,
                    export_3mf: true,
                    ..boon_scene_model::WorldSemanticEditorActions::default()
                },
            },
        );
        let mut tree = boon_scene_model::WorldSemanticEditorTree {
            root: root.clone(),
            focused: Some(wheel.clone()),
            nodes,
            metrics: boon_scene_model::WorldSemanticEditorTreeMetrics::default(),
        };
        tree.metrics = tree.compute_metrics();

        let frame = document_frame_from_world_editor_tree(&tree);
        DocumentState::from_frame(frame.clone()).expect("document frame should validate");
        let derived =
            DocumentDerivedIndexBundle::from_frame(&frame).expect("derived indexes should build");
        let mut text = SimpleTextMeasurer;
        let layout = derived
            .try_layout(LayoutInput {
                document: &frame,
                viewport: Viewport {
                    surface: 1,
                    width: 640.0,
                    height: 360.0,
                    scale: 1.0,
                },
                text: &mut text,
                capabilities: RenderCapabilities::fake_portable(),
            })
            .expect("world editor document should layout");
        let hit_table = derived
            .try_hit_side_table(&frame, &layout)
            .expect("world editor document should produce typed hit table");
        let scene = SemanticScene::from_document_layout(&frame, &layout);
        let export_doc_id = document_node_id_from_world_editor_node_id(&export);
        let wheel_doc_id = document_node_id_from_world_editor_node_id(&wheel);
        let export_semantic_id = SemanticId::from_document_node_id(&export_doc_id);
        let wheel_semantic_id = SemanticId::from_document_node_id(&wheel_doc_id);

        assert_eq!(
            frame.root,
            document_node_id_from_world_editor_node_id(&root)
        );
        assert_eq!(frame.focus, Some(wheel_doc_id.clone()));
        assert_eq!(
            frame
                .nodes
                .get(&export_doc_id)
                .and_then(|node| node.source_binding.as_ref())
                .map(|binding| binding.source_path.as_str()),
            Some("world.manufacturing.export_3mf")
        );
        assert_eq!(
            frame
                .nodes
                .get(&wheel_doc_id)
                .and_then(|node| node.source_binding.as_ref())
                .map(|binding| binding.source_path.as_str()),
            Some("world.instance.7.select")
        );
        assert!(
            layout
                .hit_regions
                .iter()
                .any(|hit| hit.node == export_doc_id)
        );
        assert!(hit_table.entries.iter().any(|entry| {
            entry.node == export_doc_id
                && entry.source_path.as_deref() == Some("world.manufacturing.export_3mf")
                && !entry.source_binding_refs.is_empty()
        }));
        assert!(hit_table.entries.iter().any(|entry| {
            entry.node == wheel_doc_id
                && entry.source_path.as_deref() == Some("world.instance.7.select")
                && !entry.source_binding_refs.is_empty()
        }));
        assert_eq!(
            scene
                .source_dispatch_for_event(SemanticInputEvent::Press {
                    semantic_id: export_semantic_id,
                })
                .map(|dispatch| dispatch.source_path),
            Some("world.manufacturing.export_3mf".to_owned())
        );
        assert_eq!(
            scene
                .source_dispatch_for_event(SemanticInputEvent::Press {
                    semantic_id: wheel_semantic_id,
                })
                .map(|dispatch| dispatch.source_path),
            Some("world.instance.7.select".to_owned())
        );
    }

    #[test]
    fn semantic_dom_snapshot_exposes_minimal_web_semantics_not_visual_dom() {
        let mut scene = SemanticScene::default();
        scene.root = Some(SemanticId("semantic:root".to_owned()));
        scene.focused = Some(SemanticId("semantic:filter".to_owned()));
        scene.nodes.insert(
            SemanticId("semantic:root".to_owned()),
            SemanticNode {
                id: SemanticId("semantic:root".to_owned()),
                node: DocumentNodeId("root".to_owned()),
                role: SemanticRole::Application,
                name: Some("Boon app".to_owned()),
                description: None,
                value: None,
                state: SemanticState::default(),
                actions: SemanticActions::default(),
                relations: SemanticRelations::default(),
                bounds: None,
                language: Some("en".to_owned()),
                heading_level: None,
                href: None,
                source_binding_id: None,
                source_path: None,
                source_intent: None,
            },
        );
        scene.nodes.insert(
            SemanticId("semantic:save".to_owned()),
            SemanticNode {
                id: SemanticId("semantic:save".to_owned()),
                node: DocumentNodeId("save".to_owned()),
                role: SemanticRole::Button,
                name: Some("Save & <Close>".to_owned()),
                description: None,
                value: None,
                state: SemanticState::default(),
                actions: SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: SemanticRelations::default(),
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(SourceBindingId("source:save".to_owned())),
                source_path: Some("toolbar.save".to_owned()),
                source_intent: Some("press".to_owned()),
            },
        );
        scene.nodes.insert(
            SemanticId("semantic:done".to_owned()),
            SemanticNode {
                id: SemanticId("semantic:done".to_owned()),
                node: DocumentNodeId("done".to_owned()),
                role: SemanticRole::Checkbox,
                name: Some("Done".to_owned()),
                description: None,
                value: Some(SemanticValue::Bool { value: true }),
                state: SemanticState {
                    checked: Some(true),
                    ..SemanticState::default()
                },
                actions: SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: SemanticRelations::default(),
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: None,
                source_path: None,
                source_intent: None,
            },
        );
        scene.nodes.insert(
            SemanticId("semantic:filter".to_owned()),
            SemanticNode {
                id: SemanticId("semantic:filter".to_owned()),
                node: DocumentNodeId("filter".to_owned()),
                role: SemanticRole::TextInput,
                name: Some("Filter".to_owned()),
                description: None,
                value: Some(SemanticValue::Text {
                    text: "a\"b".to_owned(),
                }),
                state: SemanticState {
                    focused: true,
                    ..SemanticState::default()
                },
                actions: SemanticActions {
                    focus: true,
                    press: false,
                    set_text: true,
                    increment: false,
                    decrement: false,
                },
                relations: SemanticRelations::default(),
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(SourceBindingId("source:filter:change".to_owned())),
                source_path: Some("toolbar.filter".to_owned()),
                source_intent: Some("change".to_owned()),
            },
        );

        let snapshot = SemanticDomSnapshot::from_scene(&scene);
        let html = snapshot.to_html_fragment();

        assert_eq!(snapshot.metrics.semantic_node_count, 4);
        assert_eq!(snapshot.metrics.dom_node_count, 4);
        assert_eq!(snapshot.metrics.data_boon_id_count, 4);
        assert_eq!(snapshot.metrics.text_input_endpoint_count, 1);
        assert_eq!(snapshot.metrics.visual_dom_node_count, 0);
        assert!(html.contains("id=\"boon-semantic-save\""));
        assert!(html.contains("data-boon-id=\"semantic:save\""));
        assert!(html.contains("data-boon-source-binding-id=\"source:save\""));
        assert!(html.contains("data-boon-source-path=\"toolbar.save\""));
        assert!(html.contains("data-boon-action-press=\"true\""));
        assert!(html.contains("Save &amp; &lt;Close&gt;"));
        assert!(html.contains("type=\"checkbox\""));
        assert!(html.contains("aria-checked=\"true\""));
        assert!(html.contains("data-boon-ime-endpoint=\"true\""));
        assert!(html.contains("value=\"a&quot;b\""));
        assert!(html.contains("data-boon-focused=\"true\""));
        assert!(!html.contains("<canvas"));
        assert!(!html.contains("<style"));
        assert!(!html.contains("<svg"));
    }

    #[test]
    fn semantic_web_bridge_maps_ime_events_to_source_dispatch_without_visual_dom() {
        let mut scene = SemanticScene::default();
        scene.root = Some(SemanticId("semantic:root".to_owned()));
        scene.focused = Some(SemanticId("semantic:filter".to_owned()));
        scene.nodes.insert(
            SemanticId("semantic:filter".to_owned()),
            SemanticNode {
                id: SemanticId("semantic:filter".to_owned()),
                node: DocumentNodeId("filter".to_owned()),
                role: SemanticRole::TextInput,
                name: Some("Filter".to_owned()),
                description: None,
                value: Some(SemanticValue::Text {
                    text: "abc".to_owned(),
                }),
                state: SemanticState {
                    focused: true,
                    ..SemanticState::default()
                },
                actions: SemanticActions {
                    focus: true,
                    press: false,
                    set_text: true,
                    increment: false,
                    decrement: false,
                },
                relations: SemanticRelations::default(),
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(SourceBindingId("source:filter:change".to_owned())),
                source_path: Some("toolbar.filter".to_owned()),
                source_intent: Some("change".to_owned()),
            },
        );
        scene.nodes.insert(
            SemanticId("semantic:save".to_owned()),
            SemanticNode {
                id: SemanticId("semantic:save".to_owned()),
                node: DocumentNodeId("save".to_owned()),
                role: SemanticRole::Button,
                name: Some("Save".to_owned()),
                description: None,
                value: None,
                state: SemanticState::default(),
                actions: SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: SemanticRelations::default(),
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(SourceBindingId("source:save:press".to_owned())),
                source_path: Some("toolbar.save".to_owned()),
                source_intent: Some("press".to_owned()),
            },
        );
        scene.nodes.insert(
            SemanticId("semantic:zoom-in".to_owned()),
            SemanticNode {
                id: SemanticId("semantic:zoom-in".to_owned()),
                node: DocumentNodeId("zoom-in".to_owned()),
                role: SemanticRole::Button,
                name: Some("Zoom in".to_owned()),
                description: None,
                value: Some(SemanticValue::Number { value: 1.0 }),
                state: SemanticState::default(),
                actions: SemanticActions {
                    focus: true,
                    press: false,
                    set_text: false,
                    increment: true,
                    decrement: false,
                },
                relations: SemanticRelations::default(),
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(SourceBindingId("source:zoom:increment".to_owned())),
                source_path: Some("viewport.zoom".to_owned()),
                source_intent: Some("increment".to_owned()),
            },
        );
        scene.nodes.insert(
            SemanticId("semantic:zoom-out".to_owned()),
            SemanticNode {
                id: SemanticId("semantic:zoom-out".to_owned()),
                node: DocumentNodeId("zoom-out".to_owned()),
                role: SemanticRole::Button,
                name: Some("Zoom out".to_owned()),
                description: None,
                value: Some(SemanticValue::Number { value: -1.0 }),
                state: SemanticState::default(),
                actions: SemanticActions {
                    focus: true,
                    press: false,
                    set_text: false,
                    increment: false,
                    decrement: true,
                },
                relations: SemanticRelations::default(),
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(SourceBindingId("source:zoom:decrement".to_owned())),
                source_path: Some("viewport.zoom".to_owned()),
                source_intent: Some("decrement".to_owned()),
            },
        );

        let bridge = SemanticWebBridgeSnapshot::from_scene(&scene);
        let html = bridge.to_html_fragment();

        assert_eq!(bridge.metrics.semantic_node_count, 4);
        assert_eq!(bridge.metrics.dom_node_count, 4);
        assert_eq!(bridge.metrics.visual_dom_node_count, 0);
        assert_eq!(bridge.metrics.ime_endpoint_count, 1);
        assert_eq!(bridge.metrics.source_routed_action_count, 4);
        assert_eq!(bridge.ime_endpoints[0].dom_id, "boon-semantic-filter");
        assert_eq!(
            bridge.ime_endpoints[0].source_path.as_deref(),
            Some("toolbar.filter")
        );
        assert!(html.contains("data-boon-ime-endpoint=\"true\""));
        assert!(html.contains("data-boon-action-increment=\"true\""));
        assert!(html.contains("data-boon-action-decrement=\"true\""));
        assert!(!html.contains("<canvas"));
        assert!(!html.contains("<style"));
        assert!(!html.contains("<svg"));

        let text_dispatch = bridge
            .source_dispatch_for_event(SemanticWebInputEvent::SetText {
                semantic_id: SemanticId("semantic:filter".to_owned()),
                text: "next".to_owned(),
            })
            .expect("text input route should dispatch to a Boon source");
        assert_eq!(text_dispatch.source_path, "toolbar.filter");
        assert_eq!(text_dispatch.source_intent.as_deref(), Some("change"));
        assert_eq!(text_dispatch.text.as_deref(), Some("next"));

        let press_dispatch = bridge
            .source_dispatch_for_event(SemanticWebInputEvent::Press {
                semantic_id: SemanticId("semantic:save".to_owned()),
            })
            .expect("button route should dispatch to a Boon source");
        assert_eq!(press_dispatch.source_path, "toolbar.save");
        assert_eq!(press_dispatch.source_intent.as_deref(), Some("press"));
        assert_eq!(press_dispatch.text, None);

        let increment_dispatch = bridge
            .source_dispatch_for_event(SemanticWebInputEvent::Increment {
                semantic_id: SemanticId("semantic:zoom-in".to_owned()),
            })
            .expect("increment route should dispatch to a Boon source");
        assert_eq!(increment_dispatch.source_path, "viewport.zoom");
        assert_eq!(
            increment_dispatch.source_intent.as_deref(),
            Some("increment")
        );
        assert_eq!(increment_dispatch.text, None);

        let decrement_dispatch = bridge
            .source_dispatch_for_event(SemanticWebInputEvent::Decrement {
                semantic_id: SemanticId("semantic:zoom-out".to_owned()),
            })
            .expect("decrement route should dispatch to a Boon source");
        assert_eq!(decrement_dispatch.source_path, "viewport.zoom");
        assert_eq!(
            decrement_dispatch.source_intent.as_deref(),
            Some("decrement")
        );
        assert_eq!(decrement_dispatch.text, None);
    }

    #[test]
    fn document_patch_reports_text_and_layout_invalidation() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "label",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();

        let report = state
            .apply_patch(DocumentPatch::SetText {
                id: DocumentNodeId("label".to_owned()),
                text: TextValue {
                    text: "Updated".to_owned(),
                },
            })
            .unwrap();

        assert_eq!(report.patch_kind, "set_text");
        assert_eq!(report.target, Some(DocumentNodeId("label".to_owned())));
        assert!(report.invalidation.contains(&PatchInvalidationClass::Text));
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::Layout)
        );
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::HitRegion)
        );
        assert_eq!(report.node_count_after, 2);
        assert_eq!(
            state.frame().nodes[&DocumentNodeId("label".to_owned())]
                .text
                .as_ref()
                .unwrap()
                .text,
            "Updated"
        );
    }

    #[test]
    fn document_batch_commits_atomically_and_merges_dirty_facts() {
        let mut state = DocumentState::new("root");
        let mut style = StylePatch::new();
        style.insert(
            "color".to_owned(),
            Some(StyleValue::Text("blue".to_owned())),
        );

        let change_set = state
            .apply_batch(DocumentChangeBatch {
                patches: vec![
                    DocumentPatch::UpsertNode(node("label", DocumentNodeKind::Text, Some("root"))),
                    DocumentPatch::SetText {
                        id: DocumentNodeId("label".to_owned()),
                        text: TextValue {
                            text: "Ready".to_owned(),
                        },
                    },
                    DocumentPatch::SetStyle {
                        id: DocumentNodeId("label".to_owned()),
                        patch: style,
                    },
                ],
            })
            .unwrap();

        assert_eq!(change_set.patch_count, 3);
        assert_eq!(change_set.node_count_before, 1);
        assert_eq!(change_set.node_count_after, 2);
        assert_eq!(change_set.targets, vec![DocumentNodeId("label".to_owned())]);
        for class in [
            PatchInvalidationClass::Structure,
            PatchInvalidationClass::Text,
            PatchInvalidationClass::Style,
            PatchInvalidationClass::Layout,
            PatchInvalidationClass::PaintOnly,
            PatchInvalidationClass::HitRegion,
            PatchInvalidationClass::FullDocument,
        ] {
            assert!(
                change_set.invalidation.contains(&class),
                "missing merged invalidation class {class:?}"
            );
        }
        assert_eq!(
            state.frame().nodes[&DocumentNodeId("label".to_owned())]
                .text
                .as_ref()
                .unwrap()
                .text,
            "Ready"
        );
    }

    #[test]
    fn document_batch_rolls_back_when_later_patch_fails() {
        let mut state = DocumentState::new("root");
        let error = state
            .apply_batch(DocumentChangeBatch {
                patches: vec![
                    DocumentPatch::UpsertNode(node("label", DocumentNodeKind::Text, Some("root"))),
                    DocumentPatch::SetText {
                        id: DocumentNodeId("missing".to_owned()),
                        text: TextValue {
                            text: "Should not commit".to_owned(),
                        },
                    },
                ],
            })
            .unwrap_err();

        assert!(matches!(
            error,
            PatchApplyError::MissingTarget {
                patch_kind: "set_text",
                id
            } if id.0 == "missing"
        ));
        assert!(
            !state
                .frame()
                .nodes
                .contains_key(&DocumentNodeId("label".to_owned())),
            "the successful first patch must not commit when a later patch fails"
        );
        assert_eq!(state.frame().nodes.len(), 1);
    }

    #[test]
    fn document_hot_id_table_is_numeric_stable_and_debuggable() {
        let mut state = DocumentState::new("root");
        state
            .apply_batch(DocumentChangeBatch {
                patches: vec![
                    DocumentPatch::UpsertNode(node("zeta", DocumentNodeKind::Text, Some("root"))),
                    DocumentPatch::UpsertNode(node("alpha", DocumentNodeKind::Text, Some("root"))),
                    DocumentPatch::UpsertNode(node("panel", DocumentNodeKind::Stack, Some("root"))),
                ],
            })
            .unwrap();

        let table = DocumentHotIdTable::from_frame(state.frame()).unwrap();
        assert_eq!(table.root, DocumentHotNodeId(0));
        assert_eq!(
            table.hot_id(&DocumentNodeId("root".to_owned())),
            Some(DocumentHotNodeId(0))
        );
        assert_eq!(
            table.hot_id(&DocumentNodeId("alpha".to_owned())),
            Some(DocumentHotNodeId(1)),
            "non-root IDs should be assigned deterministically by stable node ID"
        );
        assert_eq!(
            table.debug_name(DocumentHotNodeId(3)),
            Some(&DocumentNodeId("zeta".to_owned()))
        );
        let encoded = serde_json::to_value(&table).expect("hot ID table should serialize");
        assert_eq!(encoded["root"], 0);
        assert_eq!(encoded["debug_names"]["node_names"]["0"], "root");
    }

    #[test]
    fn document_hot_id_table_carries_ids_and_generations_across_frames() {
        let mut state = DocumentState::new("root");
        state
            .apply_batch(DocumentChangeBatch {
                patches: vec![
                    DocumentPatch::UpsertNode(node("zeta", DocumentNodeKind::Text, Some("root"))),
                    DocumentPatch::UpsertNode(node("alpha", DocumentNodeKind::Text, Some("root"))),
                ],
            })
            .unwrap();
        let previous_frame = state.frame().clone();
        let previous_table = DocumentHotIdTable::from_frame(&previous_frame).unwrap();
        let root_ref = previous_table
            .hot_ref(&DocumentNodeId("root".to_owned()))
            .unwrap();
        let alpha_ref = previous_table
            .hot_ref(&DocumentNodeId("alpha".to_owned()))
            .unwrap();
        let zeta_ref = previous_table
            .hot_ref(&DocumentNodeId("zeta".to_owned()))
            .unwrap();

        state
            .apply_batch(DocumentChangeBatch {
                patches: vec![
                    DocumentPatch::SetText {
                        id: DocumentNodeId("alpha".to_owned()),
                        text: TextValue {
                            text: "changed".to_owned(),
                        },
                    },
                    DocumentPatch::RemoveNode {
                        id: DocumentNodeId("zeta".to_owned()),
                    },
                    DocumentPatch::UpsertNode(node("beta", DocumentNodeKind::Button, Some("root"))),
                ],
            })
            .unwrap();

        let next_table = DocumentHotIdTable::from_previous_frames(
            &previous_table,
            &previous_frame,
            state.frame(),
        )
        .unwrap();
        let next_root_ref = next_table
            .hot_ref(&DocumentNodeId("root".to_owned()))
            .unwrap();
        let next_alpha_ref = next_table
            .hot_ref(&DocumentNodeId("alpha".to_owned()))
            .unwrap();
        let beta_ref = next_table
            .hot_ref(&DocumentNodeId("beta".to_owned()))
            .unwrap();

        assert_eq!(next_root_ref.id, root_ref.id);
        assert_eq!(
            next_root_ref.generation,
            DocumentHotNodeGeneration(root_ref.generation.0 + 1)
        );
        assert_eq!(next_alpha_ref.id, alpha_ref.id);
        assert_eq!(
            next_alpha_ref.generation,
            DocumentHotNodeGeneration(alpha_ref.generation.0 + 1)
        );
        assert!(beta_ref.id.0 >= previous_table.next_id);
        assert_eq!(beta_ref.generation, DocumentHotNodeGeneration(1));
        assert_eq!(next_table.hot_id(&DocumentNodeId("zeta".to_owned())), None);
        assert_eq!(next_table.debug_name(zeta_ref.id), None);
    }

    #[test]
    fn document_intern_index_deduplicates_text_styles_materials_clips_and_bindings() {
        let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
        alpha.text = Some(TextValue {
            text: "shared".to_owned(),
        });
        alpha
            .style
            .insert("width".to_owned(), StyleValue::Number(120.0));
        alpha
            .style
            .insert("color".to_owned(), StyleValue::Text("red".to_owned()));
        alpha
            .style
            .insert("material".to_owned(), StyleValue::Text("flat".to_owned()));
        alpha.style.insert(
            "__clip_rect".to_owned(),
            StyleValue::Text("viewport".to_owned()),
        );
        alpha.source_binding = Some(boon_document_model::SourceBinding {
            id: SourceBindingId("title-binding".to_owned()),
            source_path: "todos[0].title".to_owned(),
            intent: "edit".to_owned(),
        });

        let mut beta = node("beta", DocumentNodeKind::Text, Some("root"));
        beta.text = Some(TextValue {
            text: "shared".to_owned(),
        });
        beta.style = alpha.style.clone();
        beta.style
            .insert("color".to_owned(), StyleValue::Text("blue".to_owned()));
        beta.source_binding = alpha.source_binding.clone();

        let mut state = DocumentState::new("root");
        state
            .apply_batch(DocumentChangeBatch {
                patches: vec![
                    DocumentPatch::UpsertNode(alpha),
                    DocumentPatch::UpsertNode(beta),
                ],
            })
            .unwrap();

        let hot_ids = DocumentHotIdTable::from_frame(state.frame()).unwrap();
        let index = DocumentInternIndex::from_frame(state.frame(), &hot_ids).unwrap();
        let alpha_hot = hot_ids.hot_id(&DocumentNodeId("alpha".to_owned())).unwrap();
        let beta_hot = hot_ids.hot_id(&DocumentNodeId("beta".to_owned())).unwrap();
        let alpha_refs = index.nodes.get(&alpha_hot).unwrap();
        let beta_refs = index.nodes.get(&beta_hot).unwrap();

        assert_eq!(alpha_refs.text, beta_refs.text);
        assert_eq!(index.texts.keys_by_id.len(), 1);
        assert_eq!(alpha_refs.layout_style, beta_refs.layout_style);
        assert_ne!(alpha_refs.paint_style, beta_refs.paint_style);
        assert_eq!(alpha_refs.material, beta_refs.material);
        assert_eq!(alpha_refs.clip, beta_refs.clip);
        assert_eq!(alpha_refs.source_binding, beta_refs.source_binding);
        assert_eq!(index.source_bindings.keys_by_id.len(), 1);

        let previous_hot_ids =
            DocumentHotIdTable::from_frame(&DocumentFrame::empty("root")).unwrap();
        let err = DocumentInternIndex::from_frame(state.frame(), &previous_hot_ids).unwrap_err();
        assert!(matches!(
            err,
            PatchApplyError::StaleReference {
                reference_kind: "hot_id_table",
                ..
            }
        ));
    }

    #[test]
    fn derived_index_bundle_incrementally_updates_nonstructural_nodes() {
        let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
        alpha.text = Some(TextValue {
            text: "before".to_owned(),
        });
        alpha
            .style
            .insert("width".to_owned(), StyleValue::Number(120.0));
        alpha.source_binding = Some(boon_document_model::SourceBinding {
            id: SourceBindingId("alpha-binding".to_owned()),
            source_path: "store.before".to_owned(),
            intent: "edit".to_owned(),
        });

        let mut state = DocumentState::new("root");
        state.apply_patch(DocumentPatch::UpsertNode(alpha)).unwrap();
        let previous_bundle = DocumentDerivedIndexBundle::from_frame(state.frame()).unwrap();
        let alpha_node = DocumentNodeId("alpha".to_owned());
        let alpha_hot = previous_bundle.hot_ids.hot_id(&alpha_node).unwrap();

        state
            .apply_batch(DocumentChangeBatch {
                patches: vec![
                    DocumentPatch::SetText {
                        id: alpha_node.clone(),
                        text: TextValue {
                            text: "after".to_owned(),
                        },
                    },
                    DocumentPatch::SetStyle {
                        id: alpha_node.clone(),
                        patch: BTreeMap::from([(
                            "width".to_owned(),
                            Some(StyleValue::Number(180.0)),
                        )]),
                    },
                    DocumentPatch::SetBinding {
                        id: alpha_node.clone(),
                        binding: boon_document_model::SourceBinding {
                            id: SourceBindingId("alpha-binding".to_owned()),
                            source_path: "store.after".to_owned(),
                            intent: "edit".to_owned(),
                        },
                    },
                ],
            })
            .unwrap();

        let changed_nodes = BTreeSet::from([alpha_node]);
        let incremental = DocumentDerivedIndexBundle::from_previous_nonstructural_patch(
            &previous_bundle,
            state.frame(),
            &changed_nodes,
        )
        .unwrap();
        let full = DocumentDerivedIndexBundle::from_frame(state.frame()).unwrap();
        let after_route = DocumentTypedBindingRoute {
            source_path: "store.after".to_owned(),
            intent: "edit".to_owned(),
        };
        let before_route = DocumentTypedBindingRoute {
            source_path: "store.before".to_owned(),
            intent: "edit".to_owned(),
        };

        assert_eq!(
            incremental
                .hot_ids
                .hot_id(&DocumentNodeId("alpha".to_owned())),
            Some(alpha_hot)
        );
        let incremental_key = &incremental
            .retained_layout_keys
            .entry(alpha_hot)
            .unwrap()
            .key;
        let full_key = &full.retained_layout_keys.entry(alpha_hot).unwrap().key;
        assert_eq!(incremental_key.kind, full_key.kind);
        assert_eq!(incremental_key.children, full_key.children);
        assert_eq!(incremental_key.materialized, full_key.materialized);
        assert_eq!(
            incremental
                .intern_index
                .layout_styles
                .key(incremental_key.layout_style),
            full.intern_index.layout_styles.key(full_key.layout_style)
        );
        assert_eq!(
            incremental
                .intern_index
                .text_styles
                .key(incremental_key.text_style),
            full.intern_index.text_styles.key(full_key.text_style)
        );
        assert_eq!(
            incremental_key
                .text
                .and_then(|id| incremental.intern_index.texts.key(id)),
            full_key.text.and_then(|id| full.intern_index.texts.key(id))
        );
        assert_eq!(
            incremental.typed_styles.record(alpha_hot),
            full.typed_styles.record(alpha_hot)
        );
        assert_eq!(
            incremental.typed_bindings.refs_for_route(&after_route),
            full.typed_bindings.refs_for_route(&after_route)
        );
        assert!(
            incremental
                .typed_bindings
                .refs_for_route(&before_route)
                .is_empty()
        );
    }

    #[test]
    fn retained_layout_keys_ignore_paint_only_changes_but_track_layout_inputs() {
        let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
        alpha.text = Some(TextValue {
            text: "shared".to_owned(),
        });
        alpha
            .style
            .insert("width".to_owned(), StyleValue::Number(120.0));
        alpha
            .style
            .insert("color".to_owned(), StyleValue::Text("red".to_owned()));

        let mut state = DocumentState::new("root");
        state.apply_patch(DocumentPatch::UpsertNode(alpha)).unwrap();
        let initial_frame = state.frame().clone();
        let initial_hot = DocumentHotIdTable::from_frame(&initial_frame).unwrap();
        let initial_intern = DocumentInternIndex::from_frame(&initial_frame, &initial_hot).unwrap();
        let initial_keys = DocumentRetainedLayoutKeyTable::from_frame(
            &initial_frame,
            &initial_hot,
            &initial_intern,
        )
        .unwrap();
        let alpha_hot = initial_hot
            .hot_id(&DocumentNodeId("alpha".to_owned()))
            .unwrap();
        let initial_alpha = initial_keys.entry(alpha_hot).unwrap().clone();

        state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("alpha".to_owned()),
                patch: BTreeMap::from([(
                    "color".to_owned(),
                    Some(StyleValue::Text("blue".to_owned())),
                )]),
            })
            .unwrap();
        let paint_frame = state.frame().clone();
        let paint_hot =
            DocumentHotIdTable::from_previous_frames(&initial_hot, &initial_frame, &paint_frame)
                .unwrap();
        let paint_intern =
            DocumentInternIndex::from_previous_frame(&initial_intern, &paint_frame, &paint_hot)
                .unwrap();
        let paint_keys =
            DocumentRetainedLayoutKeyTable::from_frame(&paint_frame, &paint_hot, &paint_intern)
                .unwrap();
        let paint_alpha = paint_keys.entry(alpha_hot).unwrap();

        assert_eq!(paint_alpha.node.id, initial_alpha.node.id);
        assert_ne!(paint_alpha.node.generation, initial_alpha.node.generation);
        assert_eq!(
            paint_alpha.key, initial_alpha.key,
            "paint-only style changes must not invalidate the retained layout key"
        );
        let paint_delta = paint_keys.diff_from(&initial_keys);
        assert!(
            paint_delta.reused.iter().any(|entry| entry.id == alpha_hot),
            "paint-only changes should reuse the retained layout entry"
        );
        assert!(
            paint_delta
                .dirty
                .iter()
                .all(|entry| entry.node != alpha_hot),
            "paint-only changes should not dirty the retained layout entry"
        );

        state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("alpha".to_owned()),
                patch: BTreeMap::from([("width".to_owned(), Some(StyleValue::Number(180.0)))]),
            })
            .unwrap();
        let layout_frame = state.frame().clone();
        let layout_hot =
            DocumentHotIdTable::from_previous_frames(&paint_hot, &paint_frame, &layout_frame)
                .unwrap();
        let layout_intern =
            DocumentInternIndex::from_previous_frame(&paint_intern, &layout_frame, &layout_hot)
                .unwrap();
        let layout_keys =
            DocumentRetainedLayoutKeyTable::from_frame(&layout_frame, &layout_hot, &layout_intern)
                .unwrap();

        assert_ne!(
            layout_keys.entry(alpha_hot).unwrap().key,
            initial_alpha.key,
            "layout-affecting style changes must update the retained layout key"
        );
        let layout_delta = layout_keys.diff_from(&paint_keys);
        let layout_dirty = layout_delta
            .dirty
            .iter()
            .find(|entry| entry.node == alpha_hot)
            .expect("layout-affecting style change should dirty alpha");
        assert_eq!(
            layout_dirty.reasons,
            vec![DocumentRetainedLayoutDirtyReason::LayoutStyle]
        );

        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "child",
                DocumentNodeKind::Button,
                Some("alpha"),
            )))
            .unwrap();
        let child_frame = state.frame().clone();
        let child_hot =
            DocumentHotIdTable::from_previous_frames(&layout_hot, &layout_frame, &child_frame)
                .unwrap();
        let child_intern =
            DocumentInternIndex::from_previous_frame(&layout_intern, &child_frame, &child_hot)
                .unwrap();
        let child_keys =
            DocumentRetainedLayoutKeyTable::from_frame(&child_frame, &child_hot, &child_intern)
                .unwrap();
        let child_id = child_hot
            .hot_id(&DocumentNodeId("child".to_owned()))
            .unwrap();
        assert!(
            child_keys
                .entry(alpha_hot)
                .unwrap()
                .key
                .children
                .contains(&child_id),
            "structural child changes must be represented in the retained layout key"
        );
        let child_delta = child_keys.diff_from(&layout_keys);
        let child_dirty = child_delta
            .dirty
            .iter()
            .find(|entry| entry.node == alpha_hot)
            .expect("child insertion should dirty the parent layout entry");
        assert_eq!(
            child_dirty.reasons,
            vec![DocumentRetainedLayoutDirtyReason::Children]
        );
        let child_added = child_delta
            .dirty
            .iter()
            .find(|entry| entry.node == child_id)
            .expect("new child should be an added layout entry");
        assert_eq!(
            child_added.reasons,
            vec![DocumentRetainedLayoutDirtyReason::Added]
        );

        state
            .apply_patch(DocumentPatch::RemoveNode {
                id: DocumentNodeId("child".to_owned()),
            })
            .unwrap();
        let removed_frame = state.frame().clone();
        let removed_hot =
            DocumentHotIdTable::from_previous_frames(&child_hot, &child_frame, &removed_frame)
                .unwrap();
        let removed_intern =
            DocumentInternIndex::from_previous_frame(&child_intern, &removed_frame, &removed_hot)
                .unwrap();
        let removed_keys = DocumentRetainedLayoutKeyTable::from_frame(
            &removed_frame,
            &removed_hot,
            &removed_intern,
        )
        .unwrap();
        let removed_delta = removed_keys.diff_from(&child_keys);
        let removed_child = removed_delta
            .removed
            .iter()
            .find(|entry| entry.node == child_id)
            .expect("removed child should be reported as removed");
        assert_eq!(
            removed_child.reasons,
            vec![DocumentRetainedLayoutDirtyReason::Removed]
        );

        let stale_err =
            DocumentRetainedLayoutKeyTable::from_frame(&child_frame, &initial_hot, &initial_intern)
                .unwrap_err();
        assert!(matches!(
            stale_err,
            PatchApplyError::StaleReference {
                reference_kind: "document_intern_index" | "hot_id_table" | "hot_id_table_child",
                ..
            }
        ));
    }

    #[test]
    fn retained_layout_cache_reuses_paint_only_geometry_and_refreshes_layout_dirty_nodes() {
        let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
        alpha.text = Some(TextValue {
            text: "shared".to_owned(),
        });
        alpha
            .style
            .insert("width".to_owned(), StyleValue::Number(120.0));
        alpha
            .style
            .insert("color".to_owned(), StyleValue::Text("red".to_owned()));

        let mut state = DocumentState::new("root");
        state.apply_patch(DocumentPatch::UpsertNode(alpha)).unwrap();

        let initial_frame = state.frame().clone();
        let initial_hot = DocumentHotIdTable::from_frame(&initial_frame).unwrap();
        let initial_intern = DocumentInternIndex::from_frame(&initial_frame, &initial_hot).unwrap();
        let initial_keys = DocumentRetainedLayoutKeyTable::from_frame(
            &initial_frame,
            &initial_hot,
            &initial_intern,
        )
        .unwrap();
        let mut text = SimpleTextMeasurer;
        let initial_layout = layout(LayoutInput {
            document: &initial_frame,
            viewport: Viewport {
                surface: 1,
                width: 500.0,
                height: 300.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let initial_cache = DocumentRetainedLayoutCache::from_layout_frame(
            &initial_frame,
            &initial_hot,
            &initial_keys,
            &initial_layout,
        )
        .unwrap();
        let alpha_hot = initial_hot
            .hot_id(&DocumentNodeId("alpha".to_owned()))
            .unwrap();
        let initial_geometry = initial_cache
            .entries
            .get(&alpha_hot)
            .unwrap()
            .geometry
            .clone();

        state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("alpha".to_owned()),
                patch: BTreeMap::from([(
                    "color".to_owned(),
                    Some(StyleValue::Text("blue".to_owned())),
                )]),
            })
            .unwrap();
        let paint_frame = state.frame().clone();
        let paint_hot =
            DocumentHotIdTable::from_previous_frames(&initial_hot, &initial_frame, &paint_frame)
                .unwrap();
        let paint_intern =
            DocumentInternIndex::from_previous_frame(&initial_intern, &paint_frame, &paint_hot)
                .unwrap();
        let paint_keys =
            DocumentRetainedLayoutKeyTable::from_frame(&paint_frame, &paint_hot, &paint_intern)
                .unwrap();
        let mut text = SimpleTextMeasurer;
        let paint_layout = layout(LayoutInput {
            document: &paint_frame,
            viewport: Viewport {
                surface: 1,
                width: 500.0,
                height: 300.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let paint_update = initial_cache
            .update_from_layout_frame(&paint_frame, &paint_hot, &paint_keys, &paint_layout)
            .unwrap();
        assert!(
            paint_update.refreshed.is_empty(),
            "paint-only changes should not refresh retained layout geometry"
        );
        assert_eq!(
            paint_update.cache.entries.get(&alpha_hot).unwrap().geometry,
            initial_geometry
        );
        assert_eq!(paint_update.patch.operations.len(), 1);
        assert!(matches!(
            &paint_update.patch.operations[0],
            DocumentRetainedLayoutPatchOperation::ReuseGeometry { node }
                if node.id == alpha_hot
        ));

        state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("alpha".to_owned()),
                patch: BTreeMap::from([("width".to_owned(), Some(StyleValue::Number(180.0)))]),
            })
            .unwrap();
        let layout_frame = state.frame().clone();
        let layout_hot =
            DocumentHotIdTable::from_previous_frames(&paint_hot, &paint_frame, &layout_frame)
                .unwrap();
        let layout_intern =
            DocumentInternIndex::from_previous_frame(&paint_intern, &layout_frame, &layout_hot)
                .unwrap();
        let layout_keys =
            DocumentRetainedLayoutKeyTable::from_frame(&layout_frame, &layout_hot, &layout_intern)
                .unwrap();
        let mut text = SimpleTextMeasurer;
        let measured_layout = layout(LayoutInput {
            document: &layout_frame,
            viewport: Viewport {
                surface: 1,
                width: 500.0,
                height: 300.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let layout_update = paint_update
            .cache
            .update_from_layout_frame(&layout_frame, &layout_hot, &layout_keys, &measured_layout)
            .unwrap();
        assert!(
            layout_update
                .refreshed
                .iter()
                .any(|entry| entry.id == alpha_hot),
            "layout-affecting changes should refresh retained layout geometry"
        );
        let upsert = layout_update
            .patch
            .operations
            .iter()
            .find_map(|operation| match operation {
                DocumentRetainedLayoutPatchOperation::UpsertGeometry {
                    node,
                    geometry,
                    reasons,
                    ..
                } if node.id == alpha_hot => Some((geometry, reasons)),
                _ => None,
            })
            .expect("layout-affecting update should emit an upsert geometry patch");
        assert_eq!(upsert.0.bounds.width, 180.0);
        assert_eq!(
            upsert.1,
            &vec![DocumentRetainedLayoutDirtyReason::LayoutStyle]
        );
        assert_eq!(
            layout_update
                .cache
                .entries
                .get(&alpha_hot)
                .unwrap()
                .geometry
                .bounds
                .width,
            180.0
        );
    }

    #[test]
    fn typed_style_index_extracts_known_hot_style_properties() {
        let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
        alpha
            .style
            .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
        alpha
            .style
            .insert("height".to_owned(), StyleValue::Text("auto".to_owned()));
        alpha
            .style
            .insert("min_width".to_owned(), StyleValue::Text("120".to_owned()));
        alpha
            .style
            .insert("gap".to_owned(), StyleValue::Number(8.0));
        alpha
            .style
            .insert("padding".to_owned(), StyleValue::Number(4.0));
        alpha
            .style
            .insert("padding_left".to_owned(), StyleValue::Number(10.0));
        alpha
            .style
            .insert("center".to_owned(), StyleValue::Bool(true));
        alpha
            .style
            .insert("align_x".to_owned(), StyleValue::Text("right".to_owned()));
        alpha
            .style
            .insert("color".to_owned(), StyleValue::Text("red".to_owned()));
        alpha
            .style
            .insert("opacity".to_owned(), StyleValue::Number(0.5));
        alpha.style.insert(
            "font_weight".to_owned(),
            StyleValue::Text("bold".to_owned()),
        );
        alpha
            .style
            .insert("line_height".to_owned(), StyleValue::Number(18.0));
        alpha
            .style
            .insert("material".to_owned(), StyleValue::Text("flat".to_owned()));
        alpha
            .style
            .insert("border_radius".to_owned(), StyleValue::Number(6.0));
        alpha
            .style
            .insert("__hover_scope".to_owned(), StyleValue::Bool(true));
        alpha
            .style
            .insert("__clip_x".to_owned(), StyleValue::Number(1.0));
        alpha
            .style
            .insert("__clip_y".to_owned(), StyleValue::Number(2.0));
        alpha
            .style
            .insert("__clip_width".to_owned(), StyleValue::Number(3.0));
        alpha
            .style
            .insert("__clip_height".to_owned(), StyleValue::Number(4.0));

        let mut state = DocumentState::new("root");
        state.apply_patch(DocumentPatch::UpsertNode(alpha)).unwrap();
        let hot_ids = DocumentHotIdTable::from_frame(state.frame()).unwrap();
        let alpha_hot = hot_ids.hot_id(&DocumentNodeId("alpha".to_owned())).unwrap();
        let typed = DocumentTypedStyleIndex::from_frame(state.frame(), &hot_ids).unwrap();
        let record = typed.record(alpha_hot).unwrap();

        assert_eq!(record.layout.width, Some(DocumentStyleDimension::Fill));
        assert_eq!(record.layout.height, Some(DocumentStyleDimension::Auto));
        assert_eq!(
            record.layout.min_width,
            Some(DocumentStyleDimension::Px { value: 120.0 })
        );
        assert_eq!(record.layout.gap, Some(8.0));
        assert_eq!(
            record.layout.padding,
            DocumentTypedEdgeSpacing {
                top: 4.0,
                right: 4.0,
                bottom: 4.0,
                left: 10.0,
            }
        );
        assert!(record.layout.center);
        assert_eq!(record.layout.align_x.as_deref(), Some("right"));
        assert_eq!(
            record.layout.clip,
            Some(Rect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            })
        );
        assert_eq!(record.paint.color.as_deref(), Some("red"));
        assert_eq!(record.paint.opacity, Some(0.5));
        assert_eq!(record.text.font_weight.as_deref(), Some("bold"));
        assert_eq!(record.text.line_height, Some(18.0));
        assert_eq!(record.material.material.as_deref(), Some("flat"));
        assert_eq!(record.material.border_radius, Some(6.0));
        assert!(record.pseudo.hover_scope);

        let previous_hot_ids =
            DocumentHotIdTable::from_frame(&DocumentFrame::empty("root")).unwrap();
        let err =
            DocumentTypedStyleIndex::from_frame(state.frame(), &previous_hot_ids).unwrap_err();
        assert!(matches!(
            err,
            PatchApplyError::StaleReference {
                reference_kind: "hot_id_table",
                ..
            }
        ));
    }

    #[test]
    fn typed_style_layout_path_matches_legacy_layout_for_covered_properties() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = node("row", DocumentNodeKind::Row, Some("root"));
        row.style
            .insert("width".to_owned(), StyleValue::Number(360.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(84.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(8.0));
        row.style
            .insert("padding".to_owned(), StyleValue::Number(6.0));
        row.style
            .insert("padding_left".to_owned(), StyleValue::Number(10.0));
        row.style
            .insert("center".to_owned(), StyleValue::Bool(true));
        row.children.push(DocumentNodeId("auto-button".to_owned()));
        row.children.push(DocumentNodeId("fill-panel".to_owned()));
        row.children.push(DocumentNodeId("field".to_owned()));

        let mut auto_button = node("auto-button", DocumentNodeKind::Button, Some("row"));
        auto_button.text = Some(TextValue {
            text: "Open".to_owned(),
        });
        auto_button
            .style
            .insert("width".to_owned(), StyleValue::Text("auto".to_owned()));
        auto_button
            .style
            .insert("height".to_owned(), StyleValue::Number(26.0));
        auto_button
            .style
            .insert("padding".to_owned(), StyleValue::Number(3.0));
        auto_button
            .style
            .insert("auto_padding".to_owned(), StyleValue::Number(12.0));
        auto_button
            .style
            .insert("size".to_owned(), StyleValue::Number(10.0));

        let mut fill_panel = node("fill-panel", DocumentNodeKind::Stack, Some("row"));
        fill_panel
            .style
            .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
        fill_panel
            .style
            .insert("height".to_owned(), StyleValue::Number(30.0));
        fill_panel
            .style
            .insert("__hover_scope".to_owned(), StyleValue::Bool(true));

        let mut field = node("field", DocumentNodeKind::TextInput, Some("row"));
        field
            .style
            .insert("width".to_owned(), StyleValue::Number(80.0));
        field
            .style
            .insert("height".to_owned(), StyleValue::Number(24.0));
        field.style.insert(
            "placeholder".to_owned(),
            StyleValue::Text("Find".to_owned()),
        );
        field
            .style
            .insert("size".to_owned(), StyleValue::Number(11.0));

        let mut overlay = node("overlay", DocumentNodeKind::Stack, Some("root"));
        overlay
            .style
            .insert("overlay_children".to_owned(), StyleValue::Bool(true));
        overlay
            .style
            .insert("padding".to_owned(), StyleValue::Number(5.0));
        overlay
            .style
            .insert("min_width".to_owned(), StyleValue::Number(100.0));
        overlay
            .style
            .insert("max_width".to_owned(), StyleValue::Number(140.0));
        overlay
            .children
            .push(DocumentNodeId("overlay-text".to_owned()));

        let mut overlay_text = node("overlay-text", DocumentNodeKind::Text, Some("overlay"));
        overlay_text.text = Some(TextValue {
            text: "Overlay".to_owned(),
        });
        overlay_text
            .style
            .insert("size".to_owned(), StyleValue::Number(9.0));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .extend([row.id.clone(), overlay.id.clone()]);
        frame.nodes.insert(row.id.clone(), row);
        frame.nodes.insert(auto_button.id.clone(), auto_button);
        frame.nodes.insert(fill_panel.id.clone(), fill_panel);
        frame.nodes.insert(field.id.clone(), field);
        frame.nodes.insert(overlay.id.clone(), overlay);
        frame.nodes.insert(overlay_text.id.clone(), overlay_text);

        let hot_ids = DocumentHotIdTable::from_frame(&frame).unwrap();
        let typed_styles = DocumentTypedStyleIndex::from_frame(&frame, &hot_ids).unwrap();
        let viewport = Viewport {
            surface: 1,
            width: 500.0,
            height: 240.0,
            scale: 1.0,
        };

        let mut legacy_text = SimpleTextMeasurer;
        let legacy = layout(LayoutInput {
            document: &frame,
            viewport,
            text: &mut legacy_text,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let mut typed_text = SimpleTextMeasurer;
        let typed = layout_with_typed_styles(
            LayoutInput {
                document: &frame,
                viewport,
                text: &mut typed_text,
                capabilities: RenderCapabilities::fake_portable(),
            },
            &hot_ids,
            &typed_styles,
        );

        assert_eq!(typed, legacy);
        assert!(
            typed
                .hit_regions
                .iter()
                .any(|hit| hit.node.0 == "fill-panel"),
            "typed pseudo styles should preserve hover hit-region emission"
        );

        let mut stale_styles = typed_styles.clone();
        let row_hot = hot_ids
            .hot_id(&DocumentNodeId("row".to_owned()))
            .expect("row should have a hot id");
        stale_styles.records.remove(&row_hot);
        let mut stale_text = SimpleTextMeasurer;
        let err = try_layout_with_typed_styles(
            LayoutInput {
                document: &frame,
                viewport,
                text: &mut stale_text,
                capabilities: RenderCapabilities::fake_portable(),
            },
            &hot_ids,
            &stale_styles,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            PatchApplyError::StaleReference {
                reference_kind: "typed_style_index",
                id
            } if id.0 == "row"
        ));
    }

    #[test]
    fn derived_index_bundle_builds_typed_layout_and_hit_indexes_together() {
        let mut frame = DocumentFrame::empty("root");
        let mut button = node("button", DocumentNodeKind::Button, Some("root"));
        button.text = Some(TextValue {
            text: "Press".to_owned(),
        });
        button
            .style
            .insert("width".to_owned(), StyleValue::Text("auto".to_owned()));
        button
            .style
            .insert("padding".to_owned(), StyleValue::Number(4.0));
        button
            .style
            .insert("size".to_owned(), StyleValue::Number(12.0));
        button.source_binding = Some(boon_document_model::SourceBinding {
            id: SourceBindingId("source:button:press".to_owned()),
            source_path: "controls.primary.press".to_owned(),
            intent: "press".to_owned(),
        });
        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(button.id.clone());
        frame.nodes.insert(button.id.clone(), button);

        let bundle = DocumentDerivedIndexBundle::from_frame(&frame).unwrap();
        let standalone_hot_ids = DocumentHotIdTable::from_frame(&frame).unwrap();
        let standalone_intern =
            DocumentInternIndex::from_frame(&frame, &standalone_hot_ids).unwrap();
        let standalone_styles =
            DocumentTypedStyleIndex::from_frame(&frame, &standalone_hot_ids).unwrap();
        let standalone_bindings =
            DocumentTypedBindingIndex::from_frame(&frame, &standalone_hot_ids, &standalone_intern)
                .unwrap();

        assert_eq!(bundle.hot_ids, standalone_hot_ids);
        assert_eq!(bundle.intern_index, standalone_intern);
        assert_eq!(bundle.typed_styles, standalone_styles);
        assert_eq!(bundle.typed_bindings, standalone_bindings);

        let viewport = Viewport {
            surface: 1,
            width: 240.0,
            height: 80.0,
            scale: 1.0,
        };
        let mut legacy_text = SimpleTextMeasurer;
        let legacy = layout(LayoutInput {
            document: &frame,
            viewport,
            text: &mut legacy_text,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let mut typed_text = SimpleTextMeasurer;
        let typed = bundle
            .try_layout(LayoutInput {
                document: &frame,
                viewport,
                text: &mut typed_text,
                capabilities: RenderCapabilities::fake_portable(),
            })
            .unwrap();
        assert_eq!(typed, legacy);

        let hit_table = bundle.try_hit_side_table(&frame, &typed).unwrap();
        let hit = hit_table
            .entry_for_source_path("controls.primary.press")
            .expect("typed bundle hit table should preserve source path lookup");
        let button_hot = bundle
            .hot_ids
            .hot_id(&DocumentNodeId("button".to_owned()))
            .unwrap();
        let retained_layout_cache = bundle.try_retained_layout_cache(&frame, &typed).unwrap();
        assert!(
            retained_layout_cache.entries.contains_key(&button_hot),
            "derived bundle should build retained layout geometry for hot document nodes"
        );
        assert_eq!(
            hit.source_binding_refs,
            vec![DocumentTypedBindingRef {
                node: button_hot,
                ordinal: 0,
            }]
        );
    }

    #[test]
    fn typed_binding_index_exposes_current_single_binding_as_multi_binding_shape() {
        let mut button = node("button", DocumentNodeKind::Button, Some("root"));
        button.source_binding = Some(boon_document_model::SourceBinding {
            id: SourceBindingId("source:button:press".to_owned()),
            source_path: "todos[0].done".to_owned(),
            intent: "toggle".to_owned(),
        });

        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(button))
            .unwrap();
        let hot_ids = DocumentHotIdTable::from_frame(state.frame()).unwrap();
        let intern_index = DocumentInternIndex::from_frame(state.frame(), &hot_ids).unwrap();
        let bindings =
            DocumentTypedBindingIndex::from_frame(state.frame(), &hot_ids, &intern_index).unwrap();
        let button_hot = hot_ids
            .hot_id(&DocumentNodeId("button".to_owned()))
            .unwrap();
        let binding = bindings
            .bindings_for_node(button_hot)
            .first()
            .expect("button should expose its compatibility source binding");

        assert_eq!(binding.reference.node, button_hot);
        assert_eq!(binding.reference.ordinal, 0);
        assert_eq!(binding.binding_id.0, "source:button:press");
        assert_eq!(binding.route.source_path, "todos[0].done");
        assert_eq!(binding.route.intent, "toggle");
        assert_eq!(
            Some(binding.intern_id),
            intern_index.nodes.get(&button_hot).unwrap().source_binding
        );
        assert_eq!(
            bindings.refs_for_binding_id(&SourceBindingId("source:button:press".to_owned())),
            &[binding.reference]
        );
        assert_eq!(
            bindings.refs_for_route(&DocumentTypedBindingRoute {
                source_path: "todos[0].done".to_owned(),
                intent: "toggle".to_owned(),
            }),
            &[binding.reference]
        );
        assert!(
            bindings
                .bindings_for_node(DocumentHotNodeId(999))
                .is_empty()
        );

        let stale_hot = DocumentHotIdTable::from_frame(&DocumentFrame::empty("root")).unwrap();
        let stale_hot_err =
            DocumentTypedBindingIndex::from_frame(state.frame(), &stale_hot, &intern_index)
                .unwrap_err();
        assert!(matches!(
            stale_hot_err,
            PatchApplyError::StaleReference {
                reference_kind: "hot_id_table",
                ..
            }
        ));

        let stale_intern =
            DocumentInternIndex::from_frame(&DocumentFrame::empty("root"), &stale_hot).unwrap();
        let stale_intern_err =
            DocumentTypedBindingIndex::from_frame(state.frame(), &hot_ids, &stale_intern)
                .unwrap_err();
        assert!(matches!(
            stale_intern_err,
            PatchApplyError::StaleReference {
                reference_kind: "document_intern_index",
                ..
            }
        ));
    }

    #[test]
    fn style_identity_splits_layout_paint_material_font_and_pseudo_state() {
        let mut base = StyleMap::new();
        base.insert("width".to_owned(), StyleValue::Number(120.0));
        base.insert("color".to_owned(), StyleValue::Text("red".to_owned()));
        base.insert("material".to_owned(), StyleValue::Text("flat".to_owned()));
        base.insert(
            "font_weight".to_owned(),
            StyleValue::Text("bold".to_owned()),
        );
        base.insert("__hover_scope".to_owned(), StyleValue::Bool(true));

        let identity = computed_style_identity(&base);
        let same_identity = computed_style_identity(&base);
        assert_eq!(identity, same_identity);

        let mut paint_change = base.clone();
        paint_change.insert("color".to_owned(), StyleValue::Text("blue".to_owned()));
        let paint_identity = computed_style_identity(&paint_change);
        assert_ne!(identity.style_id, paint_identity.style_id);
        assert_eq!(identity.layout_id, paint_identity.layout_id);
        assert_ne!(identity.paint_id, paint_identity.paint_id);
        assert_eq!(identity.material_id, paint_identity.material_id);
        assert_eq!(identity.font_id, paint_identity.font_id);
        assert_eq!(identity.pseudo_state_id, paint_identity.pseudo_state_id);

        let mut layout_change = base.clone();
        layout_change.insert("width".to_owned(), StyleValue::Number(180.0));
        let layout_identity = computed_style_identity(&layout_change);
        assert_ne!(identity.layout_id, layout_identity.layout_id);
        assert_eq!(identity.paint_id, layout_identity.paint_id);
    }

    #[test]
    fn typed_hit_side_table_carries_route_identity_and_bucket_index() {
        let mut frame = DocumentFrame::empty("root");
        let mut scroll = node("scroll", DocumentNodeKind::ScrollRoot, Some("root"));
        scroll
            .style
            .insert("height".to_owned(), StyleValue::Number(120.0));
        scroll.materialized.push(MaterializedRange {
            axis: Axis::Vertical,
            visible: 0..4,
            overscan: 0..8,
        });
        let mut button = node("row-button", DocumentNodeKind::Button, Some("scroll"));
        button
            .style
            .insert("width".to_owned(), StyleValue::Number(80.0));
        button
            .style
            .insert("height".to_owned(), StyleValue::Number(24.0));
        button
            .style
            .insert("row_key".to_owned(), StyleValue::Number(42.0));
        button.style.insert(
            "row_generation".to_owned(),
            StyleValue::Text("7".to_owned()),
        );
        button.source_binding = Some(boon_document_model::SourceBinding {
            id: SourceBindingId("source:row-button:press".to_owned()),
            source_path: "rows.press".to_owned(),
            intent: "press".to_owned(),
        });
        frame
            .nodes
            .get_mut(&DocumentNodeId("root".to_owned()))
            .unwrap()
            .children
            .push(DocumentNodeId("scroll".to_owned()));
        scroll
            .children
            .push(DocumentNodeId("row-button".to_owned()));
        frame
            .nodes
            .insert(DocumentNodeId("scroll".to_owned()), scroll);
        frame
            .nodes
            .insert(DocumentNodeId("row-button".to_owned()), button);
        frame.scroll_roots.insert(
            ScrollRootId("scroll".to_owned()),
            boon_document_model::ScrollState { x: 0.0, y: 0.0 },
        );

        let mut measurer = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: boon_host::Viewport {
                surface: 1,
                width: 320.0,
                height: 240.0,
                scale: 1.0,
            },
            text: &mut measurer,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let hot_ids = DocumentHotIdTable::from_frame(&frame).unwrap();
        let intern_index = DocumentInternIndex::from_frame(&frame, &hot_ids).unwrap();
        let typed_bindings =
            DocumentTypedBindingIndex::from_frame(&frame, &hot_ids, &intern_index).unwrap();
        let table = HitSideTable::try_from_document_layout_with_typed_bindings(
            &frame,
            &hot_ids,
            &typed_bindings,
            &layout,
            64.0,
        )
        .unwrap();

        let entry = table
            .entry_for_source_path("rows.press")
            .expect("source path should have a typed hit entry");
        let row_button_hot = hot_ids
            .hot_id(&DocumentNodeId("row-button".to_owned()))
            .unwrap();
        assert_eq!(entry.node, DocumentNodeId("row-button".to_owned()));
        assert_eq!(
            entry.source_binding_id,
            Some(SourceBindingId("source:row-button:press".to_owned()))
        );
        assert_eq!(entry.source_intent.as_deref(), Some("press"));
        assert_eq!(
            entry.source_binding_refs,
            vec![DocumentTypedBindingRef {
                node: row_button_hot,
                ordinal: 0,
            }]
        );
        assert_eq!(entry.scroll_root, Some(ScrollRootId("scroll".to_owned())));
        assert_eq!(entry.row_key, Some(42));
        assert_eq!(entry.row_generation, Some(7));
        assert_eq!(entry.z_depth, 0);
        assert!(
            table
                .bucket_indices(entry.spatial_bucket)
                .is_some_and(|bucket| !bucket.is_empty())
        );
        let hit = table
            .hit_test(entry.bounds.x + 1.0, entry.bounds.y + 1.0)
            .expect("typed hit side table should route by point");
        assert_eq!(hit.source_path.as_deref(), Some("rows.press"));
        assert_eq!(hit.source_binding_refs, entry.source_binding_refs);
    }

    #[test]
    fn style_patch_reports_precise_invalidation_classes() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "label",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();

        let mut patch = StylePatch::new();
        patch.insert("width".to_owned(), Some(StyleValue::Number(240.0)));
        patch.insert(
            "background_color".to_owned(),
            Some(StyleValue::Text("black".to_owned())),
        );
        patch.insert("__hover_scope".to_owned(), Some(StyleValue::Bool(true)));
        patch.insert(
            "source_intent".to_owned(),
            Some(StyleValue::Text("activate".to_owned())),
        );

        let report = state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("label".to_owned()),
                patch,
            })
            .unwrap();

        for class in [
            PatchInvalidationClass::Style,
            PatchInvalidationClass::Layout,
            PatchInvalidationClass::LayoutOnly,
            PatchInvalidationClass::PaintOnly,
            PatchInvalidationClass::ConditionalStructure,
            PatchInvalidationClass::SourceBinding,
            PatchInvalidationClass::HitRegion,
        ] {
            assert!(
                report.invalidation.contains(&class),
                "missing invalidation class {class:?}"
            );
        }
    }

    #[test]
    fn style_patch_unknown_keys_fail_toward_full_document_invalidation() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "label",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();

        let mut patch = StylePatch::new();
        patch.insert(
            "future_renderer_knob".to_owned(),
            Some(StyleValue::Text("unknown".to_owned())),
        );

        let report = state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("label".to_owned()),
                patch,
            })
            .unwrap();

        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::FullDocument),
            "unknown style keys must invalidate conservatively"
        );
    }

    #[test]
    fn document_patch_missing_targets_fail_closed() {
        let mut state = DocumentState::new("root");

        let text_error = state
            .apply_patch(DocumentPatch::SetText {
                id: DocumentNodeId("missing".to_owned()),
                text: TextValue {
                    text: "Lost".to_owned(),
                },
            })
            .unwrap_err();
        assert!(matches!(
            text_error,
            PatchApplyError::MissingTarget {
                patch_kind: "set_text",
                id
            } if id.0 == "missing"
        ));

        let style_error = state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("missing".to_owned()),
                patch: StylePatch::new(),
            })
            .unwrap_err();
        assert!(matches!(
            style_error,
            PatchApplyError::MissingTarget {
                patch_kind: "set_style",
                id
            } if id.0 == "missing"
        ));

        let materialized_error = state
            .apply_patch(DocumentPatch::SetListMaterialization {
                id: DocumentNodeId("missing".to_owned()),
                materialized: MaterializedRange {
                    axis: Axis::Vertical,
                    visible: 0..1,
                    overscan: 0..2,
                },
            })
            .unwrap_err();
        assert!(matches!(
            materialized_error,
            PatchApplyError::MissingTarget {
                patch_kind: "set_list_materialization",
                id
            } if id.0 == "missing"
        ));
    }

    #[test]
    fn owned_frame_batch_matches_stateful_batch_patch_result() {
        let mut initial = DocumentState::new("root");
        initial
            .apply_patch(DocumentPatch::UpsertNode(node(
                "title",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();
        let batch = DocumentChangeBatch {
            patches: vec![
                DocumentPatch::SetText {
                    id: DocumentNodeId("title".to_owned()),
                    text: TextValue {
                        text: "Ready".to_owned(),
                    },
                },
                DocumentPatch::SetStyle {
                    id: DocumentNodeId("title".to_owned()),
                    patch: BTreeMap::from([(
                        "color".to_owned(),
                        Some(StyleValue::Text("green".to_owned())),
                    )]),
                },
            ],
        };

        let mut stateful = DocumentState::from_frame(initial.frame().clone()).unwrap();
        let stateful_change_set = stateful.apply_batch(batch.clone()).unwrap();
        let (owned_frame, owned_change_set) =
            DocumentState::apply_batch_to_owned_frame(initial.into_frame(), batch).unwrap();

        assert_eq!(owned_frame, stateful.into_frame());
        assert_eq!(owned_change_set, stateful_change_set);
    }

    #[test]
    fn trusted_nonstructural_owned_frame_batch_matches_stateful_batch_patch_result() {
        let mut initial = DocumentState::new("root");
        initial
            .apply_patch(DocumentPatch::UpsertNode(node(
                "title",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();
        let batch = DocumentChangeBatch {
            patches: vec![
                DocumentPatch::SetText {
                    id: DocumentNodeId("title".to_owned()),
                    text: TextValue {
                        text: "Ready".to_owned(),
                    },
                },
                DocumentPatch::SetStyle {
                    id: DocumentNodeId("title".to_owned()),
                    patch: BTreeMap::from([(
                        "color".to_owned(),
                        Some(StyleValue::Text("green".to_owned())),
                    )]),
                },
            ],
        };

        let mut stateful = DocumentState::from_frame(initial.frame().clone()).unwrap();
        let stateful_change_set = stateful.apply_batch(batch.clone()).unwrap();
        let (owned_frame, owned_change_set) =
            DocumentState::apply_nonstructural_batch_to_valid_owned_frame(
                initial.into_frame(),
                batch,
            )
            .unwrap();

        assert_eq!(owned_frame, stateful.into_frame());
        assert_eq!(owned_change_set, stateful_change_set);
    }

    #[test]
    fn trusted_nonstructural_owned_frame_batch_rejects_structural_patch() {
        let initial = DocumentState::new("root");
        let error = DocumentState::apply_nonstructural_batch_to_valid_owned_frame(
            initial.into_frame(),
            DocumentChangeBatch {
                patches: vec![DocumentPatch::UpsertNode(node(
                    "title",
                    DocumentNodeKind::Text,
                    Some("root"),
                ))],
            },
        )
        .unwrap_err();

        assert!(matches!(
            error,
            PatchApplyError::UnsupportedTrustedNonstructuralPatch {
                patch_kind: "upsert_node"
            }
        ));
    }

    #[test]
    fn materialization_patch_reports_logical_counts_and_stable_keys() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "virtual-list",
                DocumentNodeKind::ScrollRoot,
                Some("root"),
            )))
            .unwrap();

        let report = state
            .apply_patch(DocumentPatch::SetListMaterialization {
                id: DocumentNodeId("virtual-list".to_owned()),
                materialized: MaterializedRange {
                    axis: Axis::Vertical,
                    visible: 10..20,
                    overscan: 8..24,
                },
            })
            .unwrap();
        let materialization = report
            .materialization
            .expect("materialization patch should report protocol metadata");

        assert_eq!(materialization.node.0, "virtual-list");
        assert_eq!(materialization.visible, 10..20);
        assert_eq!(materialization.overscan, 8..24);
        assert_eq!(materialization.logical_item_count, 24);
        assert_eq!(materialization.materialized_item_count, 16);
        assert_eq!(
            materialization.stable_key_prefix,
            "materialized:virtual-list:y"
        );
        assert_eq!(
            materialization.first_stable_key.as_deref(),
            Some("materialized:virtual-list:y:8")
        );
        assert_eq!(
            materialization.last_stable_key.as_deref(),
            Some("materialized:virtual-list:y:23")
        );
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::Materialization)
        );
    }

    #[test]
    fn materialization_layout_demands_visible_overscan_and_stable_keys() {
        let frame = fixture_frame_with_virtualized_table();
        let mut measurer = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 640.0,
                height: 480.0,
                scale: 1.0,
            },
            text: &mut measurer,
            capabilities: RenderCapabilities::fake_portable(),
        });

        assert_eq!(layout.demands.len(), 2);
        assert_eq!(layout.materialization.len(), 2);
        let vertical = layout
            .demands
            .iter()
            .find(|demand| demand.axis == Axis::Vertical)
            .expect("vertical materialization demand should exist");
        assert_eq!(vertical.visible, 0..20);
        assert_eq!(vertical.overscan, 0..28);
        assert_eq!(vertical.logical_item_count, 28);
        assert_eq!(vertical.materialized_item_count, 28);
        assert_eq!(vertical.stable_key_prefix, "materialized:virtual-table:y");
        assert_eq!(
            vertical.last_stable_key.as_deref(),
            Some("materialized:virtual-table:y:27")
        );
        assert_eq!(layout.metrics.materialized_range_count, 2);
    }

    #[test]
    fn document_upsert_rejects_orphaned_children_and_bad_parent_links() {
        let mut state = DocumentState::new("root");
        let mut parent = node("parent", DocumentNodeKind::Stack, Some("root"));
        parent
            .children
            .push(DocumentNodeId("missing-child".to_owned()));
        let orphan_error = state
            .apply_patch(DocumentPatch::UpsertNode(parent))
            .unwrap_err();
        assert!(matches!(
            orphan_error,
            PatchApplyError::OrphanedChild { parent, child }
                if parent.0 == "parent" && child.0 == "missing-child"
        ));

        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "parent",
                DocumentNodeKind::Stack,
                Some("root"),
            )))
            .unwrap();
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "child",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();
        let mut parent = node("parent", DocumentNodeKind::Stack, Some("root"));
        parent.children.push(DocumentNodeId("child".to_owned()));
        let link_error = state
            .apply_patch(DocumentPatch::UpsertNode(parent))
            .unwrap_err();
        assert!(matches!(
            link_error,
            PatchApplyError::InvalidParentChildLink {
                parent,
                child,
                actual_parent: Some(actual_parent),
            } if parent.0 == "parent" && child.0 == "child" && actual_parent.0 == "root"
        ));
    }

    #[test]
    fn document_remove_node_removes_subtree_and_detaches_parent() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "panel",
                DocumentNodeKind::Stack,
                Some("root"),
            )))
            .unwrap();
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "label",
                DocumentNodeKind::Text,
                Some("panel"),
            )))
            .unwrap();

        let report = state
            .apply_patch(DocumentPatch::RemoveNode {
                id: DocumentNodeId("panel".to_owned()),
            })
            .unwrap();

        assert_eq!(report.patch_kind, "remove_node");
        assert_eq!(
            report.removed_nodes,
            vec![
                DocumentNodeId("panel".to_owned()),
                DocumentNodeId("label".to_owned())
            ]
        );
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::Structure)
        );
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::HitRegion)
        );
        assert!(
            !state
                .frame()
                .nodes
                .contains_key(&DocumentNodeId("panel".to_owned()))
        );
        assert!(
            !state
                .frame()
                .nodes
                .contains_key(&DocumentNodeId("label".to_owned()))
        );
        assert!(
            state.frame().nodes[&DocumentNodeId("root".to_owned())]
                .children
                .is_empty()
        );
    }

    #[test]
    fn structural_child_patches_reorder_move_and_remove_precisely() {
        let mut state = DocumentState::new("root");
        for (id, kind, parent) in [
            ("left", DocumentNodeKind::Stack, "root"),
            ("right", DocumentNodeKind::Stack, "root"),
            ("a", DocumentNodeKind::Text, "left"),
            ("b", DocumentNodeKind::Text, "left"),
            ("c", DocumentNodeKind::Text, "left"),
            ("nested", DocumentNodeKind::Text, "c"),
        ] {
            state
                .apply_patch(DocumentPatch::UpsertNode(node(id, kind, Some(parent))))
                .unwrap();
        }

        let reorder = state
            .apply_patch(DocumentPatch::InsertChild {
                parent: DocumentNodeId("left".to_owned()),
                child: DocumentNodeId("c".to_owned()),
                index: 0,
            })
            .unwrap();
        assert_eq!(reorder.patch_kind, "insert_child");
        assert_eq!(
            state.frame().nodes[&DocumentNodeId("left".to_owned())].children,
            vec![
                DocumentNodeId("c".to_owned()),
                DocumentNodeId("a".to_owned()),
                DocumentNodeId("b".to_owned()),
            ]
        );
        assert!(
            reorder
                .invalidation
                .contains(&PatchInvalidationClass::Structure)
        );
        assert!(
            !reorder
                .invalidation
                .contains(&PatchInvalidationClass::FullDocument),
            "precise child reorders should not force full-document invalidation"
        );

        let moved = state
            .apply_patch(DocumentPatch::MoveChild {
                child: DocumentNodeId("b".to_owned()),
                new_parent: DocumentNodeId("right".to_owned()),
                index: 0,
            })
            .unwrap();
        assert_eq!(moved.patch_kind, "move_child");
        assert_eq!(
            state.frame().nodes[&DocumentNodeId("b".to_owned())].parent,
            Some(DocumentNodeId("right".to_owned()))
        );
        assert_eq!(
            state.frame().nodes[&DocumentNodeId("right".to_owned())].children,
            vec![DocumentNodeId("b".to_owned())]
        );
        assert_eq!(
            state.frame().nodes[&DocumentNodeId("left".to_owned())].children,
            vec![
                DocumentNodeId("c".to_owned()),
                DocumentNodeId("a".to_owned())
            ]
        );

        let removed = state
            .apply_patch(DocumentPatch::RemoveChild {
                parent: DocumentNodeId("left".to_owned()),
                child: DocumentNodeId("c".to_owned()),
            })
            .unwrap();
        assert_eq!(removed.patch_kind, "remove_child");
        assert_eq!(
            removed.removed_nodes,
            vec![
                DocumentNodeId("c".to_owned()),
                DocumentNodeId("nested".to_owned())
            ]
        );
        assert!(
            !state
                .frame()
                .nodes
                .contains_key(&DocumentNodeId("nested".to_owned()))
        );
    }

    #[test]
    fn structural_child_patches_reject_cycles_and_bad_indices() {
        let mut state = DocumentState::new("root");
        for (id, kind, parent) in [
            ("panel", DocumentNodeKind::Stack, "root"),
            ("child", DocumentNodeKind::Stack, "panel"),
            ("leaf", DocumentNodeKind::Text, "child"),
        ] {
            state
                .apply_patch(DocumentPatch::UpsertNode(node(id, kind, Some(parent))))
                .unwrap();
        }

        let cycle = state
            .apply_patch(DocumentPatch::MoveChild {
                child: DocumentNodeId("panel".to_owned()),
                new_parent: DocumentNodeId("leaf".to_owned()),
                index: 0,
            })
            .unwrap_err();
        assert!(matches!(
            cycle,
            PatchApplyError::Cycle { id } if id.0 == "panel"
        ));

        let bad_index = state
            .apply_patch(DocumentPatch::InsertChild {
                parent: DocumentNodeId("panel".to_owned()),
                child: DocumentNodeId("child".to_owned()),
                index: 9,
            })
            .unwrap_err();
        assert!(matches!(
            bad_index,
            PatchApplyError::ChildIndexOutOfBounds {
                parent,
                index: 9,
                child_count: 0
            } if parent.0 == "panel"
        ));
        assert_eq!(
            state.frame().nodes[&DocumentNodeId("panel".to_owned())].children,
            vec![DocumentNodeId("child".to_owned())],
            "failed reorders must not mutate committed state"
        );
    }

    #[test]
    fn document_remove_root_is_explicit_error() {
        let mut state = DocumentState::new("root");
        let error = state
            .apply_patch(DocumentPatch::RemoveNode {
                id: DocumentNodeId("root".to_owned()),
            })
            .unwrap_err();
        assert!(matches!(
            error,
            PatchApplyError::CannotRemoveRoot { id } if id.0 == "root"
        ));
    }

    #[test]
    fn layout_rejects_stale_focus_and_orphan_child_references() {
        let mut frame = DocumentFrame::empty("root");
        frame.focus = Some(DocumentNodeId("missing-focus".to_owned()));
        let mut text = SimpleTextMeasurer;
        let error = try_layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 100.0,
                height: 100.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        })
        .unwrap_err();
        assert!(matches!(
            error,
            PatchApplyError::StaleReference {
                reference_kind: "focus",
                id
            } if id.0 == "missing-focus"
        ));

        let mut frame = DocumentFrame::empty("root");
        frame
            .nodes
            .get_mut(&DocumentNodeId("root".to_owned()))
            .unwrap()
            .children
            .push(DocumentNodeId("missing-child".to_owned()));
        let mut text = SimpleTextMeasurer;
        let error = try_layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 100.0,
                height: 100.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        })
        .unwrap_err();
        assert!(matches!(
            error,
            PatchApplyError::OrphanedChild { parent, child }
                if parent.0 == "root" && child.0 == "missing-child"
        ));
    }

    #[test]
    fn row_fill_uses_remaining_width_after_fixed_siblings() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
        row.parent = Some(frame.root.clone());
        row.style
            .insert("width".to_owned(), StyleValue::Number(300.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(40.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(8.0));
        row.children.push(DocumentNodeId("fixed".to_owned()));
        row.children.push(DocumentNodeId("fill".to_owned()));

        let mut fixed = DocumentNode::new("fixed", DocumentNodeKind::Text);
        fixed.parent = Some(row.id.clone());
        fixed
            .style
            .insert("width".to_owned(), StyleValue::Number(50.0));
        fixed
            .style
            .insert("height".to_owned(), StyleValue::Number(20.0));

        let mut fill = DocumentNode::new("fill", DocumentNodeKind::Text);
        fill.parent = Some(row.id.clone());
        fill.style
            .insert("width".to_owned(), StyleValue::Text("fill".to_owned()));
        fill.style
            .insert("height".to_owned(), StyleValue::Number(20.0));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(row.id.clone());
        frame.nodes.insert(row.id.clone(), row);
        frame.nodes.insert(fixed.id.clone(), fixed);
        frame.nodes.insert(fill.id.clone(), fill);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let fixed = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "fixed")
            .expect("fixed child should be laid out");
        let fill = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "fill")
            .expect("fill child should be laid out");

        assert_eq!(fixed.bounds.width, 50.0);
        assert_eq!(fill.bounds.x, 58.0);
        assert_eq!(fill.bounds.width, 242.0);
        assert!(fill.bounds.x + fill.bounds.width <= 300.0);
    }

    #[test]
    fn layout_subtree_matches_whole_frame_row_geometry() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
        row.parent = Some(frame.root.clone());
        row.style
            .insert("width".to_owned(), StyleValue::Number(300.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(80.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(0.0));
        row.children.push(DocumentNodeId("panel".to_owned()));
        row.children.push(DocumentNodeId("sibling".to_owned()));

        let mut panel = DocumentNode::new("panel", DocumentNodeKind::Stack);
        panel.parent = Some(row.id.clone());
        panel
            .style
            .insert("width".to_owned(), StyleValue::Number(180.0));
        panel
            .style
            .insert("height".to_owned(), StyleValue::Text("fill".to_owned()));
        panel
            .style
            .insert("padding".to_owned(), StyleValue::Number(10.0));
        panel
            .style
            .insert("gap".to_owned(), StyleValue::Number(4.0));
        panel.children.push(DocumentNodeId("header".to_owned()));

        let mut header = DocumentNode::new("header", DocumentNodeKind::Row);
        header.parent = Some(panel.id.clone());
        header
            .style
            .insert("width".to_owned(), StyleValue::Text("fill".to_owned()));
        header
            .style
            .insert("height".to_owned(), StyleValue::Number(20.0));

        let mut sibling = DocumentNode::new("sibling", DocumentNodeKind::Stack);
        sibling.parent = Some(row.id.clone());
        sibling
            .style
            .insert("width".to_owned(), StyleValue::Number(50.0));
        sibling
            .style
            .insert("height".to_owned(), StyleValue::Text("fill".to_owned()));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(row.id.clone());
        frame.nodes.insert(row.id.clone(), row);
        frame.nodes.insert(panel.id.clone(), panel);
        frame.nodes.insert(header.id.clone(), header);
        frame.nodes.insert(sibling.id.clone(), sibling);

        let mut full_text = SimpleTextMeasurer;
        let full = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 100.0,
                scale: 1.0,
            },
            text: &mut full_text,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let mut subtree_text = SimpleTextMeasurer;
        let subtree = layout_subtree(LayoutSubtreeInput {
            document: &frame,
            root: &DocumentNodeId("row".to_owned()),
            x: 0.0,
            y: 0.0,
            available_width: 300.0,
            available_height: 80.0,
            text: &mut subtree_text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        for id in ["row", "panel", "header", "sibling"] {
            let full_bounds = full
                .display_list
                .iter()
                .find(|item| item.node.0 == id)
                .unwrap()
                .bounds;
            let subtree_bounds = subtree
                .display_list
                .iter()
                .find(|item| item.node.0 == id)
                .unwrap()
                .bounds;
            assert_eq!(subtree_bounds, full_bounds, "bounds differ for {id}");
        }
        assert_eq!(subtree.metrics.node_count, 4);
        assert_eq!(subtree.metrics.display_item_count, 4);
    }

    #[test]
    fn row_multiple_fill_children_share_remaining_width() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
        row.parent = Some(frame.root.clone());
        row.style
            .insert("width".to_owned(), StyleValue::Number(330.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(40.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(15.0));

        for id in ["left", "middle", "right"] {
            row.children.push(DocumentNodeId(id.to_owned()));
            let mut child = DocumentNode::new(id, DocumentNodeKind::Stack);
            child.parent = Some(row.id.clone());
            child
                .style
                .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
            child
                .style
                .insert("height".to_owned(), StyleValue::Number(20.0));
            frame.nodes.insert(child.id.clone(), child);
        }

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(row.id.clone());
        frame.nodes.insert(row.id.clone(), row);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 330.0,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let child = |id: &str| {
            layout
                .display_list
                .iter()
                .find(|item| item.node.0 == id)
                .unwrap_or_else(|| panic!("child `{id}` should be laid out"))
        };
        let left = child("left");
        let middle = child("middle");
        let right = child("right");

        assert_eq!(left.bounds.width, 100.0);
        assert_eq!(middle.bounds.width, 100.0);
        assert_eq!(right.bounds.width, 100.0);
        assert_eq!(middle.bounds.x, 115.0);
        assert_eq!(right.bounds.x, 230.0);
        assert!(right.bounds.x + right.bounds.width <= 330.0);
    }

    #[test]
    fn button_with_element_label_shrinks_to_label_child() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
        row.parent = Some(frame.root.clone());
        row.style
            .insert("width".to_owned(), StyleValue::Number(300.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(10.0));
        row.children.push(DocumentNodeId("one-button".to_owned()));
        row.children.push(DocumentNodeId("two-button".to_owned()));

        let mut one_button = DocumentNode::new("one-button", DocumentNodeKind::Button);
        one_button.parent = Some(row.id.clone());
        one_button
            .children
            .push(DocumentNodeId("one-label".to_owned()));

        let mut one_label = DocumentNode::new("one-label", DocumentNodeKind::Text);
        one_label.parent = Some(one_button.id.clone());
        one_label.text = Some(TextValue {
            text: "One".to_owned(),
        });

        let mut two_button = DocumentNode::new("two-button", DocumentNodeKind::Button);
        two_button.parent = Some(row.id.clone());
        two_button
            .children
            .push(DocumentNodeId("two-label".to_owned()));

        let mut two_label = DocumentNode::new("two-label", DocumentNodeKind::Text);
        two_label.parent = Some(two_button.id.clone());
        two_label.text = Some(TextValue {
            text: "Two".to_owned(),
        });

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(row.id.clone());
        frame.nodes.insert(row.id.clone(), row);
        frame.nodes.insert(one_button.id.clone(), one_button);
        frame.nodes.insert(one_label.id.clone(), one_label);
        frame.nodes.insert(two_button.id.clone(), two_button);
        frame.nodes.insert(two_label.id.clone(), two_label);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let one_button = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "one-button")
            .expect("first button should be laid out");
        let one_label = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "one-label")
            .expect("first label should be laid out");
        let two_button = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "two-button")
            .expect("second button should be laid out");

        assert_eq!(one_label.bounds.width, 42.0);
        assert_eq!(one_button.bounds.width, one_label.bounds.width);
        assert_eq!(two_button.bounds.x, one_button.bounds.width + 10.0);
        assert!(two_button.bounds.x + two_button.bounds.width < 300.0);
    }

    #[test]
    fn checkbox_size_wins_over_accessibility_label_text() {
        let mut frame = DocumentFrame::empty("root");

        let mut checkbox = DocumentNode::new("checkbox", DocumentNodeKind::Checkbox);
        checkbox.parent = Some(frame.root.clone());
        checkbox
            .style
            .insert("size".to_owned(), StyleValue::Number(40.0));
        checkbox.text = Some(TextValue {
            text: "Reference[element:todo.title]".to_owned(),
        });

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(checkbox.id.clone());
        frame.nodes.insert(checkbox.id.clone(), checkbox);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let checkbox = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "checkbox")
            .expect("checkbox should be laid out");

        assert_eq!(checkbox.bounds.width, 40.0);
        assert_eq!(checkbox.bounds.height, 40.0);
    }

    #[test]
    fn inherited_font_size_does_not_force_stack_box_size() {
        let mut frame = DocumentFrame::empty("root");

        let mut stack = DocumentNode::new("stack", DocumentNodeKind::Stack);
        stack.parent = Some(frame.root.clone());
        stack
            .style
            .insert("size".to_owned(), StyleValue::Number(14.0));
        stack.children.push(DocumentNodeId("child".to_owned()));

        let mut child = DocumentNode::new("child", DocumentNodeKind::Text);
        child.parent = Some(stack.id.clone());
        child
            .style
            .insert("width".to_owned(), StyleValue::Number(100.0));
        child
            .style
            .insert("height".to_owned(), StyleValue::Number(50.0));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(stack.id.clone());
        frame.nodes.insert(stack.id.clone(), stack);
        frame.nodes.insert(child.id.clone(), child);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 100.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let stack = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "stack")
            .expect("stack should be laid out");

        assert_eq!(stack.bounds.height, 50.0);
    }

    #[test]
    fn stack_overlay_children_share_parent_origin() {
        let mut frame = DocumentFrame::empty("root");

        let mut stack = DocumentNode::new("stack", DocumentNodeKind::Stack);
        stack.parent = Some(frame.root.clone());
        stack
            .style
            .insert("width".to_owned(), StyleValue::Number(300.0));
        stack
            .style
            .insert("height".to_owned(), StyleValue::Number(180.0));
        stack
            .style
            .insert("overlay_children".to_owned(), StyleValue::Bool(true));
        stack.children.push(DocumentNodeId("content".to_owned()));
        stack.children.push(DocumentNodeId("modal".to_owned()));

        let mut content = DocumentNode::new("content", DocumentNodeKind::Stack);
        content.parent = Some(stack.id.clone());
        content
            .style
            .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
        content
            .style
            .insert("height".to_owned(), StyleValue::Text("Fill".to_owned()));

        let mut modal = DocumentNode::new("modal", DocumentNodeKind::Stack);
        modal.parent = Some(stack.id.clone());
        modal
            .style
            .insert("width".to_owned(), StyleValue::Number(120.0));
        modal
            .style
            .insert("height".to_owned(), StyleValue::Number(60.0));
        modal
            .style
            .insert("center".to_owned(), StyleValue::Bool(true));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(stack.id.clone());
        frame.nodes.insert(stack.id.clone(), stack);
        frame.nodes.insert(content.id.clone(), content);
        frame.nodes.insert(modal.id.clone(), modal);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 180.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let content = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "content")
            .expect("content layer should be laid out");
        let modal = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "modal")
            .expect("modal layer should be laid out");

        assert_eq!(content.bounds.x, 0.0);
        assert_eq!(content.bounds.y, 0.0);
        assert_eq!(content.bounds.height, 180.0);
        assert_eq!(modal.bounds.x, 90.0);
        assert_eq!(modal.bounds.y, 0.0);
        assert_eq!(modal.bounds.height, 60.0);
    }

    #[test]
    fn materialized_scroll_node_marks_descendants_with_clip_rect() {
        let mut frame = DocumentFrame::empty("root");

        let mut scroll = DocumentNode::new("scroll", DocumentNodeKind::Stack);
        scroll.parent = Some(frame.root.clone());
        scroll
            .style
            .insert("width".to_owned(), StyleValue::Number(200.0));
        scroll
            .style
            .insert("height".to_owned(), StyleValue::Number(80.0));
        scroll.materialized.push(MaterializedRange {
            axis: Axis::Vertical,
            visible: 0..4,
            overscan: 0..8,
        });
        scroll.children.push(DocumentNodeId("row".to_owned()));

        let mut row = DocumentNode::new("row", DocumentNodeKind::Text);
        row.parent = Some(scroll.id.clone());
        row.text = Some(TextValue {
            text: "oversized row".to_owned(),
        });
        row.style
            .insert("width".to_owned(), StyleValue::Number(200.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(160.0));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(scroll.id.clone());
        frame.nodes.insert(scroll.id.clone(), scroll);
        frame.nodes.insert(row.id.clone(), row);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 200.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let row = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "row")
            .expect("scroll child should be laid out");

        assert_eq!(style_spacing(&row.style, "__clip_x"), Some(0.0));
        assert_eq!(style_spacing(&row.style, "__clip_y"), Some(0.0));
        assert_eq!(style_spacing(&row.style, "__clip_width"), Some(200.0));
        assert_eq!(style_spacing(&row.style, "__clip_height"), Some(80.0));
    }
}
