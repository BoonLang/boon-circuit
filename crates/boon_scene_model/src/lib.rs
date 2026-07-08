use std::collections::BTreeMap;

macro_rules! numeric_ids {
    ($ty:ty; $($name:ident),+ $(,)?) => {
        $(
            #[derive(
                Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
            )]
            pub struct $name(pub $ty);
        )+
    };
}

numeric_ids!(
    u64;
    CameraId,
    LightId,
    InstanceId,
    GeometryLogicalId,
    GeometryRevision,
    AppearanceMaterialId,
    PhysicalMaterialId,
    PartId,
    FeatureId,
);
numeric_ids!(u32; PickId);

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldScene {
    pub cameras: BTreeMap<CameraId, Camera>,
    pub lights: BTreeMap<LightId, Light>,
    pub geometries: BTreeMap<GeometryLogicalId, GeometryResource>,
    pub appearances: BTreeMap<AppearanceMaterialId, AppearanceMaterial>,
    pub instances: BTreeMap<InstanceId, ModelInstance>,
    pub semantics: BTreeMap<InstanceId, WorldSemanticBinding>,
    #[serde(default)]
    pub selection: Option<WorldSelection>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Camera {
    pub id: CameraId,
    pub projection: CameraProjection,
    pub transform: Transform3D,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum CameraProjection {
    Perspective {
        vertical_fov_degrees: f32,
        near: f32,
        far: f32,
    },
    Orthographic {
        vertical_size: f32,
        near: f32,
        far: f32,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Light {
    pub id: LightId,
    pub kind: LightKind,
    pub color: [f32; 3],
    pub intensity: f32,
    pub transform: Transform3D,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum LightKind {
    Directional,
    Point { radius: f32 },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct GeometryResource {
    pub id: GeometryLogicalId,
    pub revision: GeometryRevision,
    pub kind: GeometryKind,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum GeometryKind {
    SharedPrimitive(PrimitiveGeometry),
    IndexedMeshSummary {
        vertex_count: u32,
        index_count: u32,
        bounds: Bounds3D,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize)]
pub struct SurfaceChunkId {
    pub geometry: GeometryLogicalId,
    pub spatial_key: String,
    pub lod: u8,
    pub tolerance_class: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SurfaceChunk {
    pub id: SurfaceChunkId,
    pub bounds: Bounds3D,
    pub lod: u8,
    pub error_bound: f64,
    pub geometry_revision: GeometryRevision,
    pub source_features: Vec<FeatureId>,
    pub representation: SurfaceRepresentation,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum SurfaceRepresentation {
    IndexedMeshSummary { vertex_count: u32, index_count: u32 },
    IndexedMesh(IndexedMeshChunk),
    DirectedDualGridSummary { cell_count: u32 },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct IndexedMeshChunk {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PrimitiveGeometry {
    Cube {
        size: [f32; 3],
    },
    Sphere {
        radius: f32,
        sectors: u16,
        stacks: u16,
    },
    Cylinder {
        radius: f32,
        height: f32,
        segments: u16,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AppearanceMaterial {
    pub id: AppearanceMaterialId,
    pub base_color: [f32; 4],
    pub roughness: f32,
    pub metallic: f32,
    pub emissive: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ModelInstance {
    pub id: InstanceId,
    pub geometry: GeometryLogicalId,
    pub geometry_revision: GeometryRevision,
    pub transform: Transform3D,
    pub appearance: AppearanceMaterialId,
    pub part_id: PartId,
    pub feature_id: FeatureId,
    pub pick_id: PickId,
    pub visibility: Visibility,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldSemanticBinding {
    pub instance: InstanceId,
    pub semantic_id: String,
    pub label: String,
    pub part_id: PartId,
    pub feature_id: FeatureId,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldSelection {
    pub instance: InstanceId,
    pub pick_id: PickId,
    pub part_id: PartId,
    pub feature_id: FeatureId,
    pub semantic_id: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Transform3D {
    pub translation: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldOrbitCameraDrag {
    pub target: [f32; 3],
    pub yaw_delta_radians: f32,
    pub pitch_delta_radians: f32,
    pub min_distance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldPointerOrbitDrag {
    pub target: [f32; 3],
    pub delta_pixels: [f32; 2],
    pub viewport_size: [f32; 2],
    pub yaw_radians_per_viewport: f32,
    pub pitch_radians_per_viewport: f32,
    pub min_distance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum WorldHostPointerPhase {
    Press,
    Move,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldHostPointerOrbitEvent {
    pub phase: WorldHostPointerPhase,
    pub position_pixels: [f32; 2],
    pub viewport_size: [f32; 2],
    pub primary_button_down: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldHostPointerOrbitController {
    pub target: [f32; 3],
    pub yaw_radians_per_viewport: f32,
    pub pitch_radians_per_viewport: f32,
    pub min_distance: f32,
    active_camera: Option<CameraId>,
    last_position_pixels: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Bounds3D {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Visibility {
    Visible,
    Hidden,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldPatch {
    pub operations: Vec<WorldPatchOperation>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum WorldPatchOperation {
    SetCameraTransform {
        camera: CameraId,
        transform: Transform3D,
    },
    SetTransform {
        instance: InstanceId,
        transform: Transform3D,
    },
    SetAppearanceMaterial {
        material: AppearanceMaterialId,
        base_color: [f32; 4],
    },
    SetInstanceAppearance {
        instance: InstanceId,
        appearance: AppearanceMaterialId,
    },
    SetVisibility {
        instance: InstanceId,
        visibility: Visibility,
    },
    SetSelection(Option<WorldSelection>),
    UpsertGeometry(GeometryResource),
    UpsertInstance(ModelInstance),
    RemoveInstance(InstanceId),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldPatchReport {
    pub operation_count: usize,
    #[serde(default)]
    pub camera_transform_update_count: usize,
    pub transform_update_count: usize,
    pub material_update_count: usize,
    pub instance_appearance_update_count: usize,
    pub visibility_update_count: usize,
    pub selection_update_count: usize,
    pub geometry_update_count: usize,
    pub instance_upsert_count: usize,
    pub instance_remove_count: usize,
    pub geometry_rebuild_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldPickTarget {
    pub instance: InstanceId,
    pub geometry: GeometryLogicalId,
    pub geometry_revision: GeometryRevision,
    pub part_id: PartId,
    pub feature_id: FeatureId,
    pub semantic_id: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldSceneMetrics {
    pub camera_count: usize,
    pub light_count: usize,
    pub geometry_count: usize,
    pub appearance_count: usize,
    pub instance_count: usize,
    pub semantic_binding_count: usize,
    pub pickable_instance_count: usize,
    pub selected_instance_count: usize,
    pub shared_geometry_instance_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct SolidVisualCompileReport {
    pub input_solid_graph_count: usize,
    pub input_part_count: usize,
    pub input_instance_count: usize,
    pub visual_geometry_count: usize,
    pub visual_instance_count: usize,
    pub proxy_mesh_count: usize,
    pub retained_chunk_count: usize,
    pub retained_chunk_id_count: usize,
    pub exact_mesh_count: usize,
    pub proxy_bounds_chunk_count: usize,
    pub csg_subset_chunk_count: usize,
    pub generated_mesh_count: usize,
    pub generated_vertex_count: usize,
    pub generated_index_count: usize,
    pub adaptive_chunk_count: usize,
    pub manufacturing_mesh_used: bool,
    pub visual_compiler_status: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SolidVisualScene {
    pub scene: WorldScene,
    pub chunks: Vec<SurfaceChunk>,
    pub report: SolidVisualCompileReport,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SolidVisualMeshQuality {
    Exact,
    Adaptive { error_bound: f64 },
    CsgSubset { error_bound: f64 },
    Proxy { error_bound: f64 },
}

impl SolidVisualMeshQuality {
    fn is_exact(self) -> bool {
        matches!(self, Self::Exact)
    }

    fn is_adaptive(self) -> bool {
        matches!(self, Self::Adaptive { .. })
    }

    fn is_csg_subset(self) -> bool {
        matches!(self, Self::CsgSubset { .. })
    }

    fn is_proxy(self) -> bool {
        matches!(self, Self::Proxy { .. })
    }

    fn tolerance_class(self) -> &'static str {
        match self {
            Self::Exact => "exact-primitive",
            Self::Adaptive { .. } => "adaptive-rounded-box",
            Self::CsgSubset { .. } => "csg-subset-composite",
            Self::Proxy { .. } => "proxy-bounds",
        }
    }

    fn error_bound(self) -> f64 {
        match self {
            Self::Exact => 0.0,
            Self::Adaptive { error_bound }
            | Self::CsgSubset { error_bound }
            | Self::Proxy { error_bound } => error_bound,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldManufacturingExportPreparation {
    pub status: WorldManufacturingExportStatus,
    pub printable_part_count: usize,
    pub printable_instance_count: usize,
    pub excluded_visual_only_instance_count: usize,
    pub selected_instance: Option<InstanceId>,
    pub selected_part: Option<PartId>,
    pub selected_physical_material: Option<PhysicalMaterialId>,
    pub selected_part_exportable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum WorldManufacturingExportStatus {
    ReadyNoSelection,
    ReadySelectedPrintable,
    SelectionNotPrintable,
    NoPrintableParts,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldEditorSourceAction {
    pub source_path: String,
    pub source_intent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldEditorActionOutcome {
    pub action: WorldEditorActionKind,
    pub patch: Option<WorldPatch>,
    pub export_preparation: Option<WorldManufacturingExportPreparation>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum WorldEditorActionKind {
    SelectInstance { instance: InstanceId },
    Export3Mf,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldEditorSession {
    pub scene: WorldScene,
    pub last_action: Option<WorldEditorActionKind>,
    pub last_patch_report: Option<WorldPatchReport>,
    pub last_export_preparation: Option<WorldManufacturingExportPreparation>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorldEditorSessionActionReport {
    pub outcome: WorldEditorActionOutcome,
    pub patch_report: Option<WorldPatchReport>,
    pub selected_instance_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize)]
pub struct WorldSemanticEditorNodeId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldSemanticEditorTree {
    pub root: WorldSemanticEditorNodeId,
    pub focused: Option<WorldSemanticEditorNodeId>,
    pub nodes: BTreeMap<WorldSemanticEditorNodeId, WorldSemanticEditorNode>,
    pub metrics: WorldSemanticEditorTreeMetrics,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldSemanticPickRoute {
    pub pick_id: PickId,
    pub selection: WorldSelection,
    pub focused_node: WorldSemanticEditorNodeId,
    pub semantic_id: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldSemanticEditorNode {
    pub id: WorldSemanticEditorNodeId,
    pub role: WorldSemanticEditorRole,
    pub label: String,
    pub children: Vec<WorldSemanticEditorNodeId>,
    pub instance: Option<InstanceId>,
    pub part_id: Option<PartId>,
    pub feature_id: Option<FeatureId>,
    pub pick_id: Option<PickId>,
    pub manufacturing_role: Option<boon_solid_model::ManufacturingRole>,
    pub physical_material: Option<PhysicalMaterialId>,
    pub selected: bool,
    pub visible: bool,
    pub exportable: bool,
    pub actions: WorldSemanticEditorActions,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum WorldSemanticEditorRole {
    Editor,
    Viewport,
    Assembly,
    PartInstance,
    Parameters,
    Parameter,
    Manufacturing,
    Status,
    Action,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldSemanticEditorActions {
    pub focus: bool,
    pub select: bool,
    pub toggle_visibility: bool,
    pub edit_parameter: bool,
    pub export_3mf: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WorldSemanticEditorTreeMetrics {
    pub node_count: usize,
    pub part_instance_node_count: usize,
    pub selectable_node_count: usize,
    pub selected_node_count: usize,
    pub visible_part_node_count: usize,
    pub hidden_part_node_count: usize,
    pub printable_part_node_count: usize,
    pub visual_only_part_node_count: usize,
    pub exportable_action_count: usize,
    pub parameter_node_count: usize,
    pub manufacturing_node_count: usize,
}

impl Transform3D {
    pub const IDENTITY: Self = Self {
        translation: [0.0, 0.0, 0.0],
        rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };

    pub fn with_rotation_z_degrees(mut self, degrees: f32) -> Self {
        let radians = degrees.to_radians();
        let half = radians * 0.5;
        self.rotation_xyzw = [0.0, 0.0, half.sin(), half.cos()];
        self
    }
}

impl WorldOrbitCameraDrag {
    pub fn around_origin(yaw_delta_radians: f32, pitch_delta_radians: f32) -> Self {
        Self {
            target: [0.0, 0.0, 0.0],
            yaw_delta_radians,
            pitch_delta_radians,
            min_distance: 0.01,
        }
    }
}

impl WorldPointerOrbitDrag {
    pub fn around_origin(delta_pixels: [f32; 2], viewport_size: [f32; 2]) -> Self {
        Self {
            target: [0.0, 0.0, 0.0],
            delta_pixels,
            viewport_size,
            yaw_radians_per_viewport: std::f32::consts::PI,
            pitch_radians_per_viewport: std::f32::consts::PI * 0.5,
            min_distance: 0.01,
        }
    }

    pub fn to_orbit_drag(self) -> Result<WorldOrbitCameraDrag, String> {
        if !is_finite_vec3(self.target)
            || !self.delta_pixels.iter().all(|value| value.is_finite())
            || !self.viewport_size.iter().all(|value| value.is_finite())
            || !self.yaw_radians_per_viewport.is_finite()
            || !self.pitch_radians_per_viewport.is_finite()
            || !self.min_distance.is_finite()
        {
            return Err("pointer orbit drag contains non-finite values".to_owned());
        }
        let viewport_width = self.viewport_size[0].max(1.0);
        let viewport_height = self.viewport_size[1].max(1.0);
        Ok(WorldOrbitCameraDrag {
            target: self.target,
            yaw_delta_radians: self.delta_pixels[0] / viewport_width
                * self.yaw_radians_per_viewport,
            pitch_delta_radians: self.delta_pixels[1] / viewport_height
                * self.pitch_radians_per_viewport,
            min_distance: self.min_distance,
        })
    }
}

impl Default for WorldHostPointerOrbitController {
    fn default() -> Self {
        Self {
            target: [0.0, 0.0, 0.0],
            yaw_radians_per_viewport: std::f32::consts::PI,
            pitch_radians_per_viewport: std::f32::consts::PI * 0.5,
            min_distance: 0.01,
            active_camera: None,
            last_position_pixels: None,
        }
    }
}

impl WorldHostPointerOrbitController {
    pub fn around_origin() -> Self {
        Self::default()
    }

    pub fn active_camera(&self) -> Option<CameraId> {
        self.active_camera
    }

    pub fn reset(&mut self) {
        self.active_camera = None;
        self.last_position_pixels = None;
    }

    pub fn handle_event(
        &mut self,
        scene: &WorldScene,
        camera: CameraId,
        event: WorldHostPointerOrbitEvent,
    ) -> Result<Option<WorldPatch>, String> {
        event.validate()?;
        match event.phase {
            WorldHostPointerPhase::Press => {
                if event.primary_button_down {
                    self.active_camera = Some(camera);
                    self.last_position_pixels = Some(event.position_pixels);
                } else {
                    self.reset();
                }
                Ok(None)
            }
            WorldHostPointerPhase::Move => {
                if self.active_camera != Some(camera) || !event.primary_button_down {
                    return Ok(None);
                }
                let Some(previous) = self.last_position_pixels.replace(event.position_pixels)
                else {
                    return Ok(None);
                };
                let delta_pixels = [
                    event.position_pixels[0] - previous[0],
                    event.position_pixels[1] - previous[1],
                ];
                if delta_pixels == [0.0, 0.0] {
                    return Ok(None);
                }
                scene
                    .orbit_camera_pointer_drag(
                        camera,
                        WorldPointerOrbitDrag {
                            target: self.target,
                            delta_pixels,
                            viewport_size: event.viewport_size,
                            yaw_radians_per_viewport: self.yaw_radians_per_viewport,
                            pitch_radians_per_viewport: self.pitch_radians_per_viewport,
                            min_distance: self.min_distance,
                        },
                    )
                    .map(Some)
            }
            WorldHostPointerPhase::Release => {
                self.reset();
                Ok(None)
            }
        }
    }
}

impl WorldHostPointerOrbitEvent {
    pub fn press(position_pixels: [f32; 2], viewport_size: [f32; 2]) -> Self {
        Self {
            phase: WorldHostPointerPhase::Press,
            position_pixels,
            viewport_size,
            primary_button_down: true,
        }
    }

    pub fn drag(position_pixels: [f32; 2], viewport_size: [f32; 2]) -> Self {
        Self {
            phase: WorldHostPointerPhase::Move,
            position_pixels,
            viewport_size,
            primary_button_down: true,
        }
    }

    pub fn release(position_pixels: [f32; 2], viewport_size: [f32; 2]) -> Self {
        Self {
            phase: WorldHostPointerPhase::Release,
            position_pixels,
            viewport_size,
            primary_button_down: false,
        }
    }

    fn validate(self) -> Result<(), String> {
        if !self.position_pixels.iter().all(|value| value.is_finite())
            || !self.viewport_size.iter().all(|value| value.is_finite())
        {
            return Err("host pointer orbit event contains non-finite values".to_owned());
        }
        Ok(())
    }
}

impl WorldSemanticEditorTree {
    pub fn compute_metrics(&self) -> WorldSemanticEditorTreeMetrics {
        let mut metrics = WorldSemanticEditorTreeMetrics {
            node_count: self.nodes.len(),
            ..WorldSemanticEditorTreeMetrics::default()
        };
        for node in self.nodes.values() {
            if node.role == WorldSemanticEditorRole::PartInstance {
                metrics.part_instance_node_count += 1;
            }
            if node.actions.select {
                metrics.selectable_node_count += 1;
            }
            if node.selected {
                metrics.selected_node_count += 1;
            }
            if node.role == WorldSemanticEditorRole::PartInstance && node.visible {
                metrics.visible_part_node_count += 1;
            }
            if node.role == WorldSemanticEditorRole::PartInstance && !node.visible {
                metrics.hidden_part_node_count += 1;
            }
            if node.manufacturing_role == Some(boon_solid_model::ManufacturingRole::PrintableSolid)
            {
                metrics.printable_part_node_count += 1;
            }
            if node.manufacturing_role == Some(boon_solid_model::ManufacturingRole::VisualOnly) {
                metrics.visual_only_part_node_count += 1;
            }
            if node.actions.export_3mf && node.exportable {
                metrics.exportable_action_count += 1;
            }
            if node.role == WorldSemanticEditorRole::Parameter {
                metrics.parameter_node_count += 1;
            }
            if matches!(
                node.role,
                WorldSemanticEditorRole::Manufacturing
                    | WorldSemanticEditorRole::Status
                    | WorldSemanticEditorRole::Action
            ) {
                metrics.manufacturing_node_count += 1;
            }
        }
        metrics
    }

    pub fn node_with_label(&self, label: &str) -> Option<&WorldSemanticEditorNode> {
        self.nodes.values().find(|node| node.label == label)
    }
}

impl WorldScene {
    pub fn hello_cube_fixture() -> Self {
        let camera_id = CameraId(1);
        let light_id = LightId(1);
        let geometry_id = GeometryLogicalId(1);
        let geometry_revision = GeometryRevision(1);
        let material_id = AppearanceMaterialId(1);
        let instance_id = InstanceId(1);
        let part_id = PartId(1);
        let feature_id = FeatureId(1);

        let mut cameras = BTreeMap::new();
        cameras.insert(
            camera_id,
            Camera {
                id: camera_id,
                projection: CameraProjection::Perspective {
                    vertical_fov_degrees: 55.0,
                    near: 0.01,
                    far: 1_000.0,
                },
                transform: Transform3D {
                    translation: [0.0, 1.5, 6.0],
                    rotation_xyzw: [-0.12, 0.0, 0.0, 0.9927739],
                    scale: [1.0, 1.0, 1.0],
                },
            },
        );

        let mut lights = BTreeMap::new();
        lights.insert(
            light_id,
            Light {
                id: light_id,
                kind: LightKind::Directional,
                color: [1.0, 0.96, 0.9],
                intensity: 3.0,
                transform: Transform3D {
                    translation: [2.0, 4.0, 3.0],
                    ..Transform3D::IDENTITY
                },
            },
        );

        let mut geometries = BTreeMap::new();
        geometries.insert(
            geometry_id,
            GeometryResource {
                id: geometry_id,
                revision: geometry_revision,
                kind: GeometryKind::SharedPrimitive(PrimitiveGeometry::Cube {
                    size: [1.0, 1.0, 1.0],
                }),
            },
        );

        let mut appearances = BTreeMap::new();
        appearances.insert(
            material_id,
            AppearanceMaterial {
                id: material_id,
                base_color: [0.2, 0.55, 0.95, 1.0],
                roughness: 0.62,
                metallic: 0.0,
                emissive: [0.0, 0.0, 0.0],
            },
        );

        let mut instances = BTreeMap::new();
        instances.insert(
            instance_id,
            ModelInstance {
                id: instance_id,
                geometry: geometry_id,
                geometry_revision,
                transform: Transform3D::IDENTITY,
                appearance: material_id,
                part_id,
                feature_id,
                pick_id: PickId(1),
                visibility: Visibility::Visible,
            },
        );

        let mut semantics = BTreeMap::new();
        semantics.insert(
            instance_id,
            WorldSemanticBinding {
                instance: instance_id,
                semantic_id: "world:cube".to_owned(),
                label: "Hello cube".to_owned(),
                part_id,
                feature_id,
            },
        );

        Self {
            cameras,
            lights,
            geometries,
            appearances,
            instances,
            semantics,
            selection: None,
        }
    }

    pub fn visual_proxy_from_solid_model(
        bundle: &boon_solid_model::SolidModelBundle,
    ) -> Result<(Self, SolidVisualCompileReport), String> {
        let visual = Self::visual_proxy_with_chunks_from_solid_model(bundle)?;
        Ok((visual.scene, visual.report))
    }

    pub fn visual_proxy_with_chunks_from_solid_model(
        bundle: &boon_solid_model::SolidModelBundle,
    ) -> Result<SolidVisualScene, String> {
        let mut scene = Self {
            cameras: default_visual_proxy_cameras(),
            lights: default_visual_proxy_lights(),
            geometries: BTreeMap::new(),
            appearances: BTreeMap::new(),
            instances: BTreeMap::new(),
            semantics: BTreeMap::new(),
            selection: None,
        };
        let mut chunks = Vec::new();
        let mut exact_mesh_count = 0;
        let mut adaptive_chunk_count = 0;
        let mut csg_subset_chunk_count = 0;
        let mut proxy_bounds_chunk_count = 0;

        for (geometry_id, graph) in &bundle.solids {
            let root = graph.nodes.get(&graph.root).ok_or_else(|| {
                format!(
                    "solid visual proxy compiler cannot find root {:?} for geometry {:?}",
                    graph.root, geometry_id
                )
            })?;
            let bounds = bounds3d_from_solid_bounds(root.bounds);
            let node_count = graph.nodes.len().max(1);
            let scene_geometry_id = GeometryLogicalId(geometry_id.0);
            let (mesh, mesh_quality) = mesh_chunk_from_solid_root(graph, root, bounds);
            if mesh_quality.is_exact() {
                exact_mesh_count += 1;
            }
            if mesh_quality.is_adaptive() {
                adaptive_chunk_count += 1;
            }
            if mesh_quality.is_csg_subset() {
                csg_subset_chunk_count += 1;
            }
            if mesh_quality.is_proxy() {
                proxy_bounds_chunk_count += 1;
            }
            let vertex_count = mesh.vertices.len() as u32;
            let index_count = mesh.indices.len() as u32;
            scene.geometries.insert(
                scene_geometry_id,
                GeometryResource {
                    id: scene_geometry_id,
                    revision: GeometryRevision(1),
                    kind: GeometryKind::IndexedMeshSummary {
                        vertex_count,
                        index_count,
                        bounds,
                    },
                },
            );
            chunks.push(SurfaceChunk {
                id: SurfaceChunkId {
                    geometry: scene_geometry_id,
                    spatial_key: "root".to_owned(),
                    lod: 0,
                    tolerance_class: mesh_quality.tolerance_class().to_owned(),
                },
                bounds,
                lod: 0,
                error_bound: mesh_quality.error_bound(),
                geometry_revision: GeometryRevision(1),
                source_features: graph_source_features(graph),
                representation: SurfaceRepresentation::IndexedMesh(mesh),
            });
            let _ = node_count;
        }

        for part in bundle.assembly.parts.values() {
            let appearance = scene_appearance_id_for_solid_part(part);
            scene
                .appearances
                .entry(appearance)
                .or_insert_with(|| solid_part_appearance(appearance, part.manufacturing_role));
        }

        for (index, instance) in bundle.assembly.instances.iter().enumerate() {
            let part = bundle.assembly.parts.get(&instance.part).ok_or_else(|| {
                format!(
                    "solid visual proxy compiler cannot find part {:?} for instance {:?}",
                    instance.part, instance.id
                )
            })?;
            let geometry = GeometryLogicalId(part.geometry.0);
            if !scene.geometries.contains_key(&geometry) {
                return Err(format!(
                    "solid visual proxy compiler cannot find geometry {:?} for part {:?}",
                    geometry, part.id
                ));
            }
            let instance_id = InstanceId(instance.id.0);
            let feature_id = FeatureId(part.root.0);
            let scene_part_id = PartId(part.id.0);
            scene.instances.insert(
                instance_id,
                ModelInstance {
                    id: instance_id,
                    geometry,
                    geometry_revision: GeometryRevision(1),
                    transform: transform3d_from_solid_transform(instance.transform),
                    appearance: scene_appearance_id_for_solid_part(part),
                    part_id: scene_part_id,
                    feature_id,
                    pick_id: PickId((index.saturating_add(1)).min(u32::MAX as usize) as u32),
                    visibility: Visibility::Visible,
                },
            );
            scene.semantics.insert(
                instance_id,
                WorldSemanticBinding {
                    instance: instance_id,
                    semantic_id: format!("solid:part:{}:instance:{}", part.id.0, instance.id.0),
                    label: instance.label.clone(),
                    part_id: scene_part_id,
                    feature_id,
                },
            );
        }

        let report = SolidVisualCompileReport {
            input_solid_graph_count: bundle.solids.len(),
            input_part_count: bundle.assembly.parts.len(),
            input_instance_count: bundle.assembly.instances.len(),
            visual_geometry_count: scene.geometries.len(),
            visual_instance_count: scene.instances.len(),
            proxy_mesh_count: scene.geometries.len(),
            retained_chunk_count: chunks.len(),
            retained_chunk_id_count: chunks
                .iter()
                .map(|chunk| &chunk.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            exact_mesh_count,
            proxy_bounds_chunk_count,
            csg_subset_chunk_count,
            generated_mesh_count: chunks
                .iter()
                .filter(|chunk| {
                    matches!(chunk.representation, SurfaceRepresentation::IndexedMesh(_))
                })
                .count(),
            generated_vertex_count: chunks.iter().map(surface_chunk_vertex_count).sum(),
            generated_index_count: chunks.iter().map(surface_chunk_index_count).sum(),
            adaptive_chunk_count,
            manufacturing_mesh_used: false,
            visual_compiler_status: visual_compiler_status(
                exact_mesh_count,
                adaptive_chunk_count,
                csg_subset_chunk_count,
                proxy_bounds_chunk_count,
            ),
        };
        Ok(SolidVisualScene {
            scene,
            chunks,
            report,
        })
    }

    pub fn metrics(&self) -> WorldSceneMetrics {
        let mut geometry_uses = BTreeMap::<GeometryLogicalId, usize>::new();
        let mut pickable_instance_count = 0;
        for instance in self.instances.values() {
            *geometry_uses.entry(instance.geometry).or_insert(0) += 1;
            if instance.pick_id.0 != 0 && instance.visibility == Visibility::Visible {
                pickable_instance_count += 1;
            }
        }
        WorldSceneMetrics {
            camera_count: self.cameras.len(),
            light_count: self.lights.len(),
            geometry_count: self.geometries.len(),
            appearance_count: self.appearances.len(),
            instance_count: self.instances.len(),
            semantic_binding_count: self.semantics.len(),
            pickable_instance_count,
            selected_instance_count: usize::from(self.selection.is_some()),
            shared_geometry_instance_count: geometry_uses.values().filter(|uses| **uses > 1).sum(),
        }
    }

    pub fn selection_for_pick(&self, pick_id: PickId) -> Option<WorldSelection> {
        let target = self.pick_target(pick_id)?;
        Some(WorldSelection {
            instance: target.instance,
            pick_id,
            part_id: target.part_id,
            feature_id: target.feature_id,
            semantic_id: target.semantic_id,
            label: target.label,
        })
    }

    pub fn selection_for_instance(&self, instance: InstanceId) -> Option<WorldSelection> {
        let target = self.instances.get(&instance)?;
        if target.visibility != Visibility::Visible || target.pick_id.0 == 0 {
            return None;
        }
        self.selection_for_pick(target.pick_id)
    }

    pub fn pick_target(&self, pick_id: PickId) -> Option<WorldPickTarget> {
        let instance = self.instances.values().find(|instance| {
            instance.pick_id == pick_id && instance.visibility == Visibility::Visible
        })?;
        let semantic = self.semantics.get(&instance.id);
        Some(WorldPickTarget {
            instance: instance.id,
            geometry: instance.geometry,
            geometry_revision: instance.geometry_revision,
            part_id: instance.part_id,
            feature_id: instance.feature_id,
            semantic_id: semantic.map(|binding| binding.semantic_id.clone()),
            label: semantic.map(|binding| binding.label.clone()),
        })
    }

    pub fn manufacturing_export_preparation(
        &self,
        bundle: &boon_solid_model::SolidModelBundle,
    ) -> Result<WorldManufacturingExportPreparation, String> {
        let mut printable_part_count = 0;
        for part in bundle.assembly.parts.values() {
            if part.manufacturing_role == boon_solid_model::ManufacturingRole::PrintableSolid {
                printable_part_count += 1;
            }
        }

        let mut printable_instance_count = 0;
        let mut excluded_visual_only_instance_count = 0;
        for instance in self.instances.values() {
            let part = bundle
                .assembly
                .parts
                .get(&boon_solid_model::PartId(instance.part_id.0))
                .ok_or_else(|| {
                    format!(
                        "world export preparation cannot find solid part {:?}",
                        instance.part_id
                    )
                })?;
            match part.manufacturing_role {
                boon_solid_model::ManufacturingRole::PrintableSolid => {
                    printable_instance_count += 1;
                }
                boon_solid_model::ManufacturingRole::VisualOnly => {
                    excluded_visual_only_instance_count += 1;
                }
                boon_solid_model::ManufacturingRole::VoidModifier
                | boon_solid_model::ManufacturingRole::SupportModifier
                | boon_solid_model::ManufacturingRole::InfillModifier
                | boon_solid_model::ManufacturingRole::Reference => {}
            }
        }

        let Some(selection) = &self.selection else {
            return Ok(WorldManufacturingExportPreparation {
                status: if printable_part_count == 0 {
                    WorldManufacturingExportStatus::NoPrintableParts
                } else {
                    WorldManufacturingExportStatus::ReadyNoSelection
                },
                printable_part_count,
                printable_instance_count,
                excluded_visual_only_instance_count,
                selected_instance: None,
                selected_part: None,
                selected_physical_material: None,
                selected_part_exportable: false,
            });
        };

        let selected_instance = self.instances.get(&selection.instance).ok_or_else(|| {
            format!(
                "world export preparation cannot find selected instance {:?}",
                selection.instance
            )
        })?;
        if selected_instance.pick_id != selection.pick_id {
            return Err(format!(
                "world export preparation selected pick id {:?} does not match instance pick id {:?}",
                selection.pick_id, selected_instance.pick_id
            ));
        }
        let selected_part_id = boon_solid_model::PartId(selected_instance.part_id.0);
        let selected_part = bundle
            .assembly
            .parts
            .get(&selected_part_id)
            .ok_or_else(|| {
                format!(
                    "world export preparation cannot find selected solid part {:?}",
                    selected_instance.part_id
                )
            })?;
        let selected_part_exportable =
            selected_part.manufacturing_role == boon_solid_model::ManufacturingRole::PrintableSolid;
        let selected_physical_material = selected_part
            .physical_material
            .map(|material| PhysicalMaterialId(material.0));
        let status = if printable_part_count == 0 {
            WorldManufacturingExportStatus::NoPrintableParts
        } else if selected_part_exportable {
            WorldManufacturingExportStatus::ReadySelectedPrintable
        } else {
            WorldManufacturingExportStatus::SelectionNotPrintable
        };

        Ok(WorldManufacturingExportPreparation {
            status,
            printable_part_count,
            printable_instance_count,
            excluded_visual_only_instance_count,
            selected_instance: Some(selection.instance),
            selected_part: Some(selected_instance.part_id),
            selected_physical_material,
            selected_part_exportable,
        })
    }

    pub fn editor_source_action_outcome(
        &self,
        bundle: &boon_solid_model::SolidModelBundle,
        action: &WorldEditorSourceAction,
    ) -> Result<WorldEditorActionOutcome, String> {
        match (action.source_path.as_str(), action.source_intent.as_deref()) {
            ("world.manufacturing.export_3mf", Some("press" | "click" | "source" | "activate"))
            | ("world.manufacturing.export_3mf", None) => Ok(WorldEditorActionOutcome {
                action: WorldEditorActionKind::Export3Mf,
                patch: None,
                export_preparation: Some(self.manufacturing_export_preparation(bundle)?),
            }),
            (source_path, Some("select" | "press" | "click" | "source" | "activate")) => {
                let Some(instance) = instance_id_from_editor_source_path(source_path, "select")
                else {
                    return Err(format!(
                        "unsupported world editor source action path `{}` with intent {:?}",
                        action.source_path, action.source_intent
                    ));
                };
                let selection = self.selection_for_instance(instance).ok_or_else(|| {
                    format!("world editor source action cannot select instance {instance:?}")
                })?;
                Ok(WorldEditorActionOutcome {
                    action: WorldEditorActionKind::SelectInstance { instance },
                    patch: Some(WorldPatch {
                        operations: vec![WorldPatchOperation::SetSelection(Some(selection))],
                    }),
                    export_preparation: None,
                })
            }
            _ => Err(format!(
                "unsupported world editor source action path `{}` with intent {:?}",
                action.source_path, action.source_intent
            )),
        }
    }

    pub fn semantic_editor_tree_from_solid_model(
        &self,
        bundle: &boon_solid_model::SolidModelBundle,
        editor_label: impl Into<String>,
    ) -> Result<WorldSemanticEditorTree, String> {
        let root = WorldSemanticEditorNodeId("world-editor:root".to_owned());
        let viewport = WorldSemanticEditorNodeId("world-editor:viewport".to_owned());
        let assembly = WorldSemanticEditorNodeId("world-editor:assembly".to_owned());
        let parameters = WorldSemanticEditorNodeId("world-editor:parameters".to_owned());
        let manufacturing = WorldSemanticEditorNodeId("world-editor:manufacturing".to_owned());
        let validation_status =
            WorldSemanticEditorNodeId("world-editor:manufacturing:validation".to_owned());
        let export_3mf =
            WorldSemanticEditorNodeId("world-editor:manufacturing:export-3mf".to_owned());
        let mut nodes = BTreeMap::new();
        let mut assembly_children = Vec::new();

        for instance in &bundle.assembly.instances {
            let world_instance_id = InstanceId(instance.id.0);
            let world_instance = self.instances.get(&world_instance_id).ok_or_else(|| {
                format!(
                    "world semantic editor tree cannot find scene instance {:?}",
                    world_instance_id
                )
            })?;
            let part = bundle.assembly.parts.get(&instance.part).ok_or_else(|| {
                format!(
                    "world semantic editor tree cannot find part {:?} for instance {:?}",
                    instance.part, instance.id
                )
            })?;
            let semantic = self.semantics.get(&world_instance_id);
            let node_id = WorldSemanticEditorNodeId(format!(
                "world-editor:assembly:instance:{}",
                instance.id.0
            ));
            let selected = self
                .selection
                .as_ref()
                .is_some_and(|selection| selection.instance == world_instance_id);
            assembly_children.push(node_id.clone());
            nodes.insert(
                node_id.clone(),
                WorldSemanticEditorNode {
                    id: node_id,
                    role: WorldSemanticEditorRole::PartInstance,
                    label: semantic
                        .map(|binding| binding.label.clone())
                        .unwrap_or_else(|| instance.label.clone()),
                    children: Vec::new(),
                    instance: Some(world_instance_id),
                    part_id: Some(world_instance.part_id),
                    feature_id: Some(world_instance.feature_id),
                    pick_id: Some(world_instance.pick_id),
                    manufacturing_role: Some(part.manufacturing_role),
                    physical_material: part
                        .physical_material
                        .map(|material| PhysicalMaterialId(material.0)),
                    selected,
                    visible: world_instance.visibility == Visibility::Visible,
                    exportable: part.manufacturing_role
                        == boon_solid_model::ManufacturingRole::PrintableSolid,
                    actions: WorldSemanticEditorActions {
                        focus: true,
                        select: world_instance.visibility == Visibility::Visible,
                        toggle_visibility: true,
                        ..WorldSemanticEditorActions::default()
                    },
                },
            );
        }

        let parameter_children = vec![
            WorldSemanticEditorNodeId("world-editor:parameters:body-length".to_owned()),
            WorldSemanticEditorNodeId("world-editor:parameters:wheel-radius".to_owned()),
            WorldSemanticEditorNodeId("world-editor:parameters:paint".to_owned()),
        ];
        for (id, label) in [
            (&parameter_children[0], "Body length"),
            (&parameter_children[1], "Wheel radius"),
            (&parameter_children[2], "Paint"),
        ] {
            nodes.insert(
                id.clone(),
                WorldSemanticEditorNode {
                    id: id.clone(),
                    role: WorldSemanticEditorRole::Parameter,
                    label: label.to_owned(),
                    children: Vec::new(),
                    instance: None,
                    part_id: None,
                    feature_id: None,
                    pick_id: None,
                    manufacturing_role: None,
                    physical_material: None,
                    selected: false,
                    visible: true,
                    exportable: false,
                    actions: WorldSemanticEditorActions {
                        focus: true,
                        edit_parameter: true,
                        ..WorldSemanticEditorActions::default()
                    },
                },
            );
        }

        let export_preparation = self.manufacturing_export_preparation(bundle)?;
        nodes.insert(
            root.clone(),
            WorldSemanticEditorNode {
                id: root.clone(),
                role: WorldSemanticEditorRole::Editor,
                label: editor_label.into(),
                children: vec![
                    viewport.clone(),
                    assembly.clone(),
                    parameters.clone(),
                    manufacturing.clone(),
                ],
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            viewport.clone(),
            WorldSemanticEditorNode {
                id: viewport,
                role: WorldSemanticEditorRole::Viewport,
                label: "3D viewport".to_owned(),
                children: Vec::new(),
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: WorldSemanticEditorActions {
                    focus: true,
                    ..WorldSemanticEditorActions::default()
                },
            },
        );
        nodes.insert(
            assembly.clone(),
            WorldSemanticEditorNode {
                id: assembly,
                role: WorldSemanticEditorRole::Assembly,
                label: "Car assembly".to_owned(),
                children: assembly_children,
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            parameters.clone(),
            WorldSemanticEditorNode {
                id: parameters,
                role: WorldSemanticEditorRole::Parameters,
                label: "Parameters".to_owned(),
                children: parameter_children,
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            manufacturing.clone(),
            WorldSemanticEditorNode {
                id: manufacturing,
                role: WorldSemanticEditorRole::Manufacturing,
                label: "Manufacturing".to_owned(),
                children: vec![validation_status.clone(), export_3mf.clone()],
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            validation_status.clone(),
            WorldSemanticEditorNode {
                id: validation_status,
                role: WorldSemanticEditorRole::Status,
                label: format!(
                    "Validation ready: {} printable instances, {} visual-only excluded",
                    export_preparation.printable_instance_count,
                    export_preparation.excluded_visual_only_instance_count
                ),
                children: Vec::new(),
                instance: None,
                part_id: None,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: None,
                selected: false,
                visible: true,
                exportable: false,
                actions: WorldSemanticEditorActions::default(),
            },
        );
        nodes.insert(
            export_3mf.clone(),
            WorldSemanticEditorNode {
                id: export_3mf,
                role: WorldSemanticEditorRole::Action,
                label: "Export 3MF".to_owned(),
                children: Vec::new(),
                instance: None,
                part_id: export_preparation.selected_part,
                feature_id: None,
                pick_id: None,
                manufacturing_role: None,
                physical_material: export_preparation.selected_physical_material,
                selected: false,
                visible: true,
                exportable: !matches!(
                    export_preparation.status,
                    WorldManufacturingExportStatus::NoPrintableParts
                        | WorldManufacturingExportStatus::SelectionNotPrintable
                ),
                actions: WorldSemanticEditorActions {
                    focus: true,
                    export_3mf: true,
                    ..WorldSemanticEditorActions::default()
                },
            },
        );

        let focused = self.selection.as_ref().map(|selection| {
            WorldSemanticEditorNodeId(format!(
                "world-editor:assembly:instance:{}",
                selection.instance.0
            ))
        });
        let mut tree = WorldSemanticEditorTree {
            root,
            focused,
            nodes,
            metrics: WorldSemanticEditorTreeMetrics::default(),
        };
        tree.metrics = tree.compute_metrics();
        Ok(tree)
    }

    pub fn semantic_editor_route_for_pick(
        &self,
        bundle: &boon_solid_model::SolidModelBundle,
        editor_label: impl Into<String>,
        pick_id: PickId,
    ) -> Result<Option<WorldSemanticPickRoute>, String> {
        let Some(selection) = self.selection_for_pick(pick_id) else {
            return Ok(None);
        };
        let mut selected_scene = self.clone();
        selected_scene.selection = Some(selection.clone());
        let tree = selected_scene.semantic_editor_tree_from_solid_model(bundle, editor_label)?;
        let Some(focused_node) = tree.focused.clone() else {
            return Ok(None);
        };
        let node = tree.nodes.get(&focused_node).ok_or_else(|| {
            format!(
                "semantic editor focus node {:?} is missing from tree",
                focused_node
            )
        })?;
        if node.pick_id != Some(pick_id) || node.instance != Some(selection.instance) {
            return Ok(None);
        }
        if !node.selected || !node.actions.focus || !node.actions.select {
            return Ok(None);
        }
        Ok(Some(WorldSemanticPickRoute {
            pick_id,
            selection,
            focused_node,
            semantic_id: node
                .instance
                .and_then(|instance| self.semantics.get(&instance))
                .map(|binding| binding.semantic_id.clone()),
            label: Some(node.label.clone()),
        }))
    }

    pub fn semantic_editor_routes_for_feature(
        &self,
        bundle: &boon_solid_model::SolidModelBundle,
        editor_label: impl Into<String> + Clone,
        feature_id: FeatureId,
    ) -> Result<Vec<WorldSemanticPickRoute>, String> {
        let mut routes = Vec::new();
        for instance in self.instances.values() {
            if instance.visibility != Visibility::Visible
                || instance.feature_id != feature_id
                || instance.pick_id.0 == 0
            {
                continue;
            }
            if let Some(route) =
                self.semantic_editor_route_for_pick(bundle, editor_label.clone(), instance.pick_id)?
            {
                routes.push(route);
            }
        }
        Ok(routes)
    }

    pub fn orbit_camera_drag(
        &self,
        camera: CameraId,
        drag: WorldOrbitCameraDrag,
    ) -> Result<WorldPatch, String> {
        let camera_model = self
            .cameras
            .get(&camera)
            .ok_or_else(|| format!("unknown world camera {camera:?}"))?;
        let transform = orbit_camera_transform(camera_model.transform, drag)?;
        Ok(WorldPatch {
            operations: vec![WorldPatchOperation::SetCameraTransform { camera, transform }],
        })
    }

    pub fn orbit_camera_pointer_drag(
        &self,
        camera: CameraId,
        drag: WorldPointerOrbitDrag,
    ) -> Result<WorldPatch, String> {
        self.orbit_camera_drag(camera, drag.to_orbit_drag()?)
    }

    pub fn diff(old: &Self, new: &Self) -> WorldPatch {
        let mut operations = Vec::new();
        for (camera_id, old_camera) in &old.cameras {
            if let Some(new_camera) = new.cameras.get(camera_id) {
                if old_camera.transform != new_camera.transform {
                    operations.push(WorldPatchOperation::SetCameraTransform {
                        camera: *camera_id,
                        transform: new_camera.transform,
                    });
                }
            }
        }
        for (geometry_id, new_geometry) in &new.geometries {
            match old.geometries.get(geometry_id) {
                Some(old_geometry) if old_geometry != new_geometry => {
                    operations.push(WorldPatchOperation::UpsertGeometry(new_geometry.clone()));
                }
                None => operations.push(WorldPatchOperation::UpsertGeometry(new_geometry.clone())),
                Some(_) => {}
            }
        }
        for (instance_id, old_instance) in &old.instances {
            match new.instances.get(instance_id) {
                Some(new_instance) => {
                    if old_instance.transform != new_instance.transform {
                        operations.push(WorldPatchOperation::SetTransform {
                            instance: *instance_id,
                            transform: new_instance.transform,
                        });
                    }
                    if old_instance.appearance != new_instance.appearance {
                        operations.push(WorldPatchOperation::SetInstanceAppearance {
                            instance: *instance_id,
                            appearance: new_instance.appearance,
                        });
                    }
                    if old_instance.visibility != new_instance.visibility {
                        operations.push(WorldPatchOperation::SetVisibility {
                            instance: *instance_id,
                            visibility: new_instance.visibility,
                        });
                    }
                    if old_instance.geometry != new_instance.geometry {
                        operations.push(WorldPatchOperation::UpsertInstance(new_instance.clone()));
                    }
                }
                None => operations.push(WorldPatchOperation::RemoveInstance(*instance_id)),
            }
        }
        for (instance_id, new_instance) in &new.instances {
            if !old.instances.contains_key(instance_id) {
                operations.push(WorldPatchOperation::UpsertInstance(new_instance.clone()));
            }
        }
        for (material_id, old_material) in &old.appearances {
            if let Some(new_material) = new.appearances.get(material_id) {
                if old_material.base_color != new_material.base_color {
                    operations.push(WorldPatchOperation::SetAppearanceMaterial {
                        material: *material_id,
                        base_color: new_material.base_color,
                    });
                }
            }
        }
        if old.selection != new.selection {
            operations.push(WorldPatchOperation::SetSelection(new.selection.clone()));
        }
        WorldPatch { operations }
    }

    pub fn apply_patch(&mut self, patch: &WorldPatch) -> Result<WorldPatchReport, String> {
        let mut report = WorldPatchReport {
            operation_count: patch.operations.len(),
            ..WorldPatchReport::default()
        };
        for operation in &patch.operations {
            match operation {
                WorldPatchOperation::SetCameraTransform { camera, transform } => {
                    let target = self
                        .cameras
                        .get_mut(camera)
                        .ok_or_else(|| format!("unknown world camera {camera:?}"))?;
                    target.transform = *transform;
                    report.camera_transform_update_count += 1;
                }
                WorldPatchOperation::SetTransform {
                    instance,
                    transform,
                } => {
                    let target = self
                        .instances
                        .get_mut(instance)
                        .ok_or_else(|| format!("unknown world instance {instance:?}"))?;
                    target.transform = *transform;
                    report.transform_update_count += 1;
                }
                WorldPatchOperation::SetAppearanceMaterial {
                    material,
                    base_color,
                } => {
                    let target = self
                        .appearances
                        .get_mut(material)
                        .ok_or_else(|| format!("unknown appearance material {material:?}"))?;
                    target.base_color = *base_color;
                    report.material_update_count += 1;
                }
                WorldPatchOperation::SetInstanceAppearance {
                    instance,
                    appearance,
                } => {
                    if !self.appearances.contains_key(appearance) {
                        return Err(format!("unknown appearance material {appearance:?}"));
                    }
                    let target = self
                        .instances
                        .get_mut(instance)
                        .ok_or_else(|| format!("unknown world instance {instance:?}"))?;
                    target.appearance = *appearance;
                    report.instance_appearance_update_count += 1;
                }
                WorldPatchOperation::SetVisibility {
                    instance,
                    visibility,
                } => {
                    let target = self
                        .instances
                        .get_mut(instance)
                        .ok_or_else(|| format!("unknown world instance {instance:?}"))?;
                    target.visibility = *visibility;
                    report.visibility_update_count += 1;
                }
                WorldPatchOperation::SetSelection(selection) => {
                    if let Some(selection) = selection {
                        let target = self.instances.get(&selection.instance).ok_or_else(|| {
                            format!("unknown selected world instance {:?}", selection.instance)
                        })?;
                        if target.pick_id != selection.pick_id {
                            return Err(format!(
                                "selected world instance {:?} has pick id {:?}, not {:?}",
                                selection.instance, target.pick_id, selection.pick_id
                            ));
                        }
                        if target.part_id != selection.part_id {
                            return Err(format!(
                                "selected world instance {:?} has part {:?}, not {:?}",
                                selection.instance, target.part_id, selection.part_id
                            ));
                        }
                        if target.feature_id != selection.feature_id {
                            return Err(format!(
                                "selected world instance {:?} has feature {:?}, not {:?}",
                                selection.instance, target.feature_id, selection.feature_id
                            ));
                        }
                    }
                    self.selection = selection.clone();
                    report.selection_update_count += 1;
                }
                WorldPatchOperation::UpsertGeometry(geometry) => {
                    self.geometries.insert(geometry.id, geometry.clone());
                    for instance in self.instances.values_mut() {
                        if instance.geometry == geometry.id {
                            instance.geometry_revision = geometry.revision;
                        }
                    }
                    report.geometry_update_count += 1;
                    report.geometry_rebuild_count += 1;
                }
                WorldPatchOperation::UpsertInstance(instance) => {
                    if !self.geometries.contains_key(&instance.geometry) {
                        return Err(format!("unknown geometry {:?}", instance.geometry));
                    }
                    if !self.appearances.contains_key(&instance.appearance) {
                        return Err(format!(
                            "unknown appearance material {:?}",
                            instance.appearance
                        ));
                    }
                    self.instances.insert(instance.id, instance.clone());
                    report.instance_upsert_count += 1;
                    report.geometry_rebuild_count += 1;
                }
                WorldPatchOperation::RemoveInstance(instance) => {
                    self.instances.remove(instance);
                    self.semantics.remove(instance);
                    report.instance_remove_count += 1;
                }
            }
        }
        Ok(report)
    }
}

fn default_visual_proxy_cameras() -> BTreeMap<CameraId, Camera> {
    let mut cameras = BTreeMap::new();
    cameras.insert(
        CameraId(1),
        Camera {
            id: CameraId(1),
            projection: CameraProjection::Perspective {
                vertical_fov_degrees: 55.0,
                near: 0.01,
                far: 10_000.0,
            },
            transform: Transform3D {
                translation: [0.0, 80.0, 180.0],
                rotation_xyzw: [-0.24, 0.0, 0.0, 0.9707721],
                scale: [1.0, 1.0, 1.0],
            },
        },
    );
    cameras
}

fn default_visual_proxy_lights() -> BTreeMap<LightId, Light> {
    let mut lights = BTreeMap::new();
    lights.insert(
        LightId(1),
        Light {
            id: LightId(1),
            kind: LightKind::Directional,
            color: [1.0, 0.97, 0.92],
            intensity: 3.0,
            transform: Transform3D {
                translation: [100.0, 150.0, 120.0],
                ..Transform3D::IDENTITY
            },
        },
    );
    lights
}

impl WorldEditorSession {
    pub fn new(scene: WorldScene) -> Self {
        Self {
            scene,
            last_action: None,
            last_patch_report: None,
            last_export_preparation: None,
        }
    }

    pub fn handle_source_action(
        &mut self,
        bundle: &boon_solid_model::SolidModelBundle,
        action: &WorldEditorSourceAction,
    ) -> Result<WorldEditorSessionActionReport, String> {
        let outcome = self.scene.editor_source_action_outcome(bundle, action)?;
        let patch_report = if let Some(patch) = outcome.patch.as_ref() {
            Some(self.scene.apply_patch(patch)?)
        } else {
            None
        };
        self.last_action = Some(outcome.action.clone());
        self.last_patch_report = patch_report.clone();
        self.last_export_preparation = outcome.export_preparation.clone();

        Ok(WorldEditorSessionActionReport {
            outcome,
            patch_report,
            selected_instance_count: self.scene.metrics().selected_instance_count,
        })
    }

    pub fn semantic_editor_tree(
        &self,
        bundle: &boon_solid_model::SolidModelBundle,
        root_label: &str,
    ) -> Result<WorldSemanticEditorTree, String> {
        self.scene
            .semantic_editor_tree_from_solid_model(bundle, root_label)
    }
}

fn instance_id_from_editor_source_path(source_path: &str, action: &str) -> Option<InstanceId> {
    let prefix = "world.instance.";
    let suffix = format!(".{action}");
    let value = source_path.strip_prefix(prefix)?.strip_suffix(&suffix)?;
    value.parse::<u64>().ok().map(InstanceId)
}

fn bounds3d_from_solid_bounds(bounds: boon_solid_model::Aabb64) -> Bounds3D {
    Bounds3D {
        min: [
            finite_f32(bounds.min.x),
            finite_f32(bounds.min.y),
            finite_f32(bounds.min.z),
        ],
        max: [
            finite_f32(bounds.max.x),
            finite_f32(bounds.max.y),
            finite_f32(bounds.max.z),
        ],
    }
}

fn transform3d_from_solid_transform(transform: boon_solid_model::Mat4d) -> Transform3D {
    Transform3D {
        translation: [
            finite_f32(transform.columns[3][0]),
            finite_f32(transform.columns[3][1]),
            finite_f32(transform.columns[3][2]),
        ],
        ..Transform3D::IDENTITY
    }
}

fn scene_appearance_id_for_solid_part(
    part: &boon_solid_model::PartDefinition,
) -> AppearanceMaterialId {
    part.appearance
        .map(|appearance| AppearanceMaterialId(appearance.0))
        .unwrap_or(AppearanceMaterialId(part.id.0.max(1)))
}

fn solid_part_appearance(
    id: AppearanceMaterialId,
    role: boon_solid_model::ManufacturingRole,
) -> AppearanceMaterial {
    let base_color = match role {
        boon_solid_model::ManufacturingRole::PrintableSolid => [0.3, 0.62, 0.9, 1.0],
        boon_solid_model::ManufacturingRole::VisualOnly => [0.7, 0.7, 0.75, 0.45],
        boon_solid_model::ManufacturingRole::VoidModifier => [0.95, 0.25, 0.2, 0.35],
        boon_solid_model::ManufacturingRole::SupportModifier => [0.45, 0.75, 0.35, 0.55],
        boon_solid_model::ManufacturingRole::InfillModifier => [0.9, 0.65, 0.25, 0.55],
        boon_solid_model::ManufacturingRole::Reference => [0.55, 0.55, 0.6, 0.35],
    };
    AppearanceMaterial {
        id,
        base_color,
        roughness: 0.7,
        metallic: 0.0,
        emissive: [0.0, 0.0, 0.0],
    }
}

fn graph_source_features(graph: &boon_solid_model::SolidGraph) -> Vec<FeatureId> {
    graph
        .nodes
        .values()
        .map(|node| FeatureId(node.feature_id.0))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn visual_compiler_status(
    exact_mesh_count: usize,
    adaptive_chunk_count: usize,
    csg_subset_chunk_count: usize,
    proxy_bounds_chunk_count: usize,
) -> String {
    match (
        exact_mesh_count > 0,
        adaptive_chunk_count > 0,
        csg_subset_chunk_count > 0,
        proxy_bounds_chunk_count > 0,
    ) {
        (true, false, false, false) => "retained-exact-primitive-mesh-no-csg".to_owned(),
        (false, true, false, false) => "retained-adaptive-rounded-box-mesh-no-csg".to_owned(),
        (false, false, true, false) => "retained-csg-subset-composite-mesh-no-full-csg".to_owned(),
        (false, false, false, true) => "retained-generated-bounds-mesh-no-csg".to_owned(),
        (true, true, false, false) => "retained-mixed-exact-and-adaptive-mesh-no-csg".to_owned(),
        (true, false, true, false) => {
            "retained-mixed-exact-and-csg-subset-mesh-no-full-csg".to_owned()
        }
        (false, true, true, false) => {
            "retained-mixed-adaptive-and-csg-subset-mesh-no-full-csg".to_owned()
        }
        (true, false, false, true) => "retained-mixed-exact-and-bounds-mesh-no-csg".to_owned(),
        (false, true, false, true) => "retained-mixed-adaptive-and-bounds-mesh-no-csg".to_owned(),
        (false, false, true, true) => {
            "retained-mixed-csg-subset-and-bounds-mesh-no-full-csg".to_owned()
        }
        (true, true, true, false) => {
            "retained-mixed-exact-adaptive-and-csg-subset-mesh-no-full-csg".to_owned()
        }
        (true, true, false, true) => {
            "retained-mixed-exact-adaptive-and-bounds-mesh-no-csg".to_owned()
        }
        (true, false, true, true) => {
            "retained-mixed-exact-csg-subset-and-bounds-mesh-no-full-csg".to_owned()
        }
        (false, true, true, true) => {
            "retained-mixed-adaptive-csg-subset-and-bounds-mesh-no-full-csg".to_owned()
        }
        (true, true, true, true) => {
            "retained-mixed-exact-adaptive-csg-subset-and-bounds-mesh-no-full-csg".to_owned()
        }
        (false, false, false, false) => "retained-empty-mesh-no-csg".to_owned(),
    }
}

fn mesh_chunk_from_solid_root(
    graph: &boon_solid_model::SolidGraph,
    root: &boon_solid_model::SolidNode,
    bounds: Bounds3D,
) -> (IndexedMeshChunk, SolidVisualMeshQuality) {
    match &root.op {
        boon_solid_model::SolidOp::Box { .. } => (
            mesh_chunk_from_bounds(bounds),
            SolidVisualMeshQuality::Exact,
        ),
        boon_solid_model::SolidOp::RoundedBox { radius, .. } => {
            if let Some((mesh, error_bound)) =
                mesh_chunk_from_rounded_box_bounds(bounds, *radius, 6)
            {
                (mesh, SolidVisualMeshQuality::Adaptive { error_bound })
            } else {
                (
                    mesh_chunk_from_bounds(bounds),
                    SolidVisualMeshQuality::Proxy {
                        error_bound: graph.tolerance.linear_error,
                    },
                )
            }
        }
        boon_solid_model::SolidOp::Sphere { radius } => (
            mesh_chunk_from_sphere_bounds(bounds, *radius as f32, 32, 16),
            SolidVisualMeshQuality::Exact,
        ),
        boon_solid_model::SolidOp::Cylinder { radius, height } => (
            mesh_chunk_from_cylinder_bounds(bounds, *radius as f32, *height as f32, 32),
            SolidVisualMeshQuality::Exact,
        ),
        boon_solid_model::SolidOp::Cone {
            radius0,
            radius1,
            height,
        } => (
            mesh_chunk_from_cone_bounds(
                bounds,
                *radius0 as f32,
                *radius1 as f32,
                *height as f32,
                32,
            ),
            SolidVisualMeshQuality::Exact,
        ),
        boon_solid_model::SolidOp::Torus {
            major_radius,
            minor_radius,
        } => (
            mesh_chunk_from_torus_bounds(
                bounds,
                *major_radius as f32,
                *minor_radius as f32,
                32,
                12,
            ),
            SolidVisualMeshQuality::Exact,
        ),
        boon_solid_model::SolidOp::Extrude { profile, height } => {
            if let Some(mesh) = mesh_chunk_from_extrude_profile(graph, *profile, *height, bounds) {
                (mesh, SolidVisualMeshQuality::Exact)
            } else {
                (
                    mesh_chunk_from_bounds(bounds),
                    SolidVisualMeshQuality::Proxy {
                        error_bound: graph.tolerance.linear_error,
                    },
                )
            }
        }
        boon_solid_model::SolidOp::Revolve { profile, axis } => {
            if let Some(mesh) = mesh_chunk_from_revolve_profile(graph, *profile, *axis, 32) {
                (mesh, SolidVisualMeshQuality::Exact)
            } else {
                (
                    mesh_chunk_from_bounds(bounds),
                    SolidVisualMeshQuality::Proxy {
                        error_bound: graph.tolerance.linear_error,
                    },
                )
            }
        }
        boon_solid_model::SolidOp::Loft { profiles } => {
            if let Some(mesh) = mesh_chunk_from_loft_profiles(graph, profiles) {
                (mesh, SolidVisualMeshQuality::Exact)
            } else {
                (
                    mesh_chunk_from_bounds(bounds),
                    SolidVisualMeshQuality::Proxy {
                        error_bound: graph.tolerance.linear_error,
                    },
                )
            }
        }
        boon_solid_model::SolidOp::Shell { child, thickness } => {
            if let Some(mesh) = mesh_chunk_from_box_like_shell(graph, *child, *thickness) {
                (mesh, SolidVisualMeshQuality::Exact)
            } else {
                (
                    mesh_chunk_from_bounds(bounds),
                    SolidVisualMeshQuality::Proxy {
                        error_bound: graph.tolerance.linear_error,
                    },
                )
            }
        }
        boon_solid_model::SolidOp::Transform { child, transform } => {
            if let Some((mesh, quality)) =
                mesh_chunk_from_translated_child(graph, *child, *transform)
            {
                (mesh, quality)
            } else {
                (
                    mesh_chunk_from_bounds(bounds),
                    SolidVisualMeshQuality::Proxy {
                        error_bound: graph.tolerance.linear_error,
                    },
                )
            }
        }
        boon_solid_model::SolidOp::Difference { base, tools } => {
            if let Some((mesh, error_bound)) =
                mesh_chunk_from_supported_difference(graph, *base, tools)
            {
                (mesh, SolidVisualMeshQuality::CsgSubset { error_bound })
            } else {
                (
                    mesh_chunk_from_bounds(bounds),
                    SolidVisualMeshQuality::Proxy {
                        error_bound: graph.tolerance.linear_error,
                    },
                )
            }
        }
        boon_solid_model::SolidOp::Intersection { children } => {
            if let Some((mesh, error_bound)) =
                mesh_chunk_from_supported_intersection(graph, children)
            {
                (mesh, SolidVisualMeshQuality::CsgSubset { error_bound })
            } else {
                (
                    mesh_chunk_from_bounds(bounds),
                    SolidVisualMeshQuality::Proxy {
                        error_bound: graph.tolerance.linear_error,
                    },
                )
            }
        }
        _ => (
            mesh_chunk_from_bounds(bounds),
            SolidVisualMeshQuality::Proxy {
                error_bound: graph.tolerance.linear_error,
            },
        ),
    }
}

fn mesh_chunk_from_translated_child(
    graph: &boon_solid_model::SolidGraph,
    child: boon_solid_model::SolidNodeId,
    transform: boon_solid_model::Mat4d,
) -> Option<(IndexedMeshChunk, SolidVisualMeshQuality)> {
    let translation = translation_from_transform_only(transform)?;
    let child_node = graph.nodes.get(&child)?;
    let child_bounds = bounds3d_from_solid_bounds(child_node.bounds);
    let (mut mesh, quality) = mesh_chunk_from_solid_root(graph, child_node, child_bounds);
    for vertex in &mut mesh.vertices {
        vertex.position[0] += translation[0];
        vertex.position[1] += translation[1];
        vertex.position[2] += translation[2];
    }
    Some((mesh, quality))
}

fn translation_from_transform_only(transform: boon_solid_model::Mat4d) -> Option<[f32; 3]> {
    let columns = transform.columns;
    let identity_basis = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    for column in 0..3 {
        for row in 0..4 {
            if !matrix_value_close(columns[column][row], identity_basis[column][row]) {
                return None;
            }
        }
    }
    if !matrix_value_close(columns[3][3], 1.0) {
        return None;
    }
    let translation = [columns[3][0], columns[3][1], columns[3][2]];
    if translation
        .iter()
        .all(|value| value.is_finite() && *value >= f32::MIN as f64 && *value <= f32::MAX as f64)
    {
        Some([
            translation[0] as f32,
            translation[1] as f32,
            translation[2] as f32,
        ])
    } else {
        None
    }
}

fn matrix_value_close(value: f64, expected: f64) -> bool {
    (value - expected).abs() <= 1.0e-9
}

#[derive(Clone, Debug)]
struct VisualPrism2D {
    outer: Vec<[f32; 2]>,
    holes: Vec<Vec<[f32; 2]>>,
    min_z: f32,
    max_z: f32,
    error_bound: f64,
}

#[derive(Clone, Debug)]
struct VisualHole2D {
    center: [f32; 2],
    ring: Vec<[f32; 2]>,
    min_z: f32,
    max_z: f32,
    error_bound: f64,
}

fn mesh_chunk_from_supported_difference(
    graph: &boon_solid_model::SolidGraph,
    base: boon_solid_model::SolidNodeId,
    tools: &[boon_solid_model::SolidNodeId],
) -> Option<(IndexedMeshChunk, f64)> {
    let prisms = visual_prisms_from_solid_node(graph, base)?;
    let holes = tools
        .iter()
        .filter_map(|tool| visual_hole_from_solid_node(graph, *tool))
        .collect::<Vec<_>>();
    if prisms.is_empty() || holes.is_empty() {
        return None;
    }
    let mut used_hole_count = 0;
    let mut error_bound = 0.0_f64;
    let mut output_prisms = Vec::new();
    for prism in prisms {
        let mut pending_prisms = vec![prism];
        for hole in &holes {
            let mut next_prisms = Vec::new();
            for mut prism in pending_prisms {
                error_bound = error_bound.max(prism.error_bound);
                if !visual_hole_overlaps_prism(hole, &prism) {
                    next_prisms.push(prism);
                    continue;
                }
                if hole.ring.len() < 3 || !point_in_ring(hole.center, &prism.outer) {
                    return None;
                }
                if hole.min_z <= prism.min_z && hole.max_z >= prism.max_z {
                    error_bound = error_bound.max(hole.error_bound);
                    prism.holes.push(hole.ring.clone());
                    used_hole_count += 1;
                    next_prisms.push(prism);
                    continue;
                }
                if hole.max_z >= prism.max_z && hole.min_z > prism.min_z && hole.min_z < prism.max_z
                {
                    error_bound = error_bound.max(hole.error_bound);
                    let mut bottom = prism.clone();
                    bottom.max_z = hole.min_z;
                    if bottom.min_z < bottom.max_z {
                        next_prisms.push(bottom);
                    }
                    prism.min_z = hole.min_z;
                    prism.holes.push(hole.ring.clone());
                    next_prisms.push(prism);
                    used_hole_count += 1;
                    continue;
                }
                return None;
            }
            pending_prisms = next_prisms;
        }
        output_prisms.extend(pending_prisms);
    }
    if used_hole_count == 0 {
        return None;
    }
    let mesh = mesh_chunk_from_visual_prisms(&output_prisms)?;
    Some((mesh, error_bound.max(graph.tolerance.linear_error)))
}

fn visual_hole_overlaps_prism(hole: &VisualHole2D, prism: &VisualPrism2D) -> bool {
    hole.max_z > prism.min_z && hole.min_z < prism.max_z && point_in_ring(hole.center, &prism.outer)
}

fn mesh_chunk_from_supported_intersection(
    graph: &boon_solid_model::SolidGraph,
    children: &[boon_solid_model::SolidNodeId],
) -> Option<(IndexedMeshChunk, f64)> {
    if children.is_empty() {
        return None;
    }

    let mut overlap: Option<Bounds3D> = None;
    let mut error_bound = 0.0_f64;
    for child in children {
        let (bounds, child_error_bound) = box_like_visual_bounds_from_solid_node(graph, *child)?;
        if bounds.min[0] >= bounds.max[0]
            || bounds.min[1] >= bounds.max[1]
            || bounds.min[2] >= bounds.max[2]
        {
            return None;
        }
        error_bound = error_bound.max(child_error_bound);
        overlap = Some(match overlap {
            Some(current) => Bounds3D {
                min: [
                    current.min[0].max(bounds.min[0]),
                    current.min[1].max(bounds.min[1]),
                    current.min[2].max(bounds.min[2]),
                ],
                max: [
                    current.max[0].min(bounds.max[0]),
                    current.max[1].min(bounds.max[1]),
                    current.max[2].min(bounds.max[2]),
                ],
            },
            None => bounds,
        });
    }

    let bounds = overlap?;
    if bounds.min[0] >= bounds.max[0]
        || bounds.min[1] >= bounds.max[1]
        || bounds.min[2] >= bounds.max[2]
    {
        return None;
    }
    let mesh = mesh_chunk_from_rectangular_prism(
        bounds.min[0],
        bounds.max[0],
        bounds.min[1],
        bounds.max[1],
        bounds.min[2],
        bounds.max[2],
    )?;
    Some((mesh, error_bound.max(graph.tolerance.linear_error)))
}

fn box_like_visual_bounds_from_solid_node(
    graph: &boon_solid_model::SolidGraph,
    node_id: boon_solid_model::SolidNodeId,
) -> Option<(Bounds3D, f64)> {
    let node = graph.nodes.get(&node_id)?;
    match &node.op {
        boon_solid_model::SolidOp::Box { .. } => {
            Some((bounds3d_from_solid_bounds(node.bounds), 0.0))
        }
        boon_solid_model::SolidOp::RoundedBox { radius, .. } => {
            let error_bound = if radius.is_finite() && *radius > 0.0 {
                *radius
            } else {
                graph.tolerance.linear_error
            };
            Some((bounds3d_from_solid_bounds(node.bounds), error_bound))
        }
        boon_solid_model::SolidOp::Transform { child, transform } => {
            let translation = translation_from_transform_only(*transform)?;
            let (mut bounds, error_bound) = box_like_visual_bounds_from_solid_node(graph, *child)?;
            bounds.min[0] += translation[0];
            bounds.min[1] += translation[1];
            bounds.min[2] += translation[2];
            bounds.max[0] += translation[0];
            bounds.max[1] += translation[1];
            bounds.max[2] += translation[2];
            Some((bounds, error_bound))
        }
        _ => None,
    }
}

fn visual_prisms_from_solid_node(
    graph: &boon_solid_model::SolidGraph,
    node_id: boon_solid_model::SolidNodeId,
) -> Option<Vec<VisualPrism2D>> {
    let node = graph.nodes.get(&node_id)?;
    match &node.op {
        boon_solid_model::SolidOp::Box { .. } => {
            let bounds = bounds3d_from_solid_bounds(node.bounds);
            Some(vec![VisualPrism2D {
                outer: rectangle_ring_2d(bounds),
                holes: Vec::new(),
                min_z: bounds.min[2],
                max_z: bounds.max[2],
                error_bound: 0.0,
            }])
        }
        boon_solid_model::SolidOp::RoundedBox { radius, .. } => {
            let bounds = bounds3d_from_solid_bounds(node.bounds);
            let (outer, error_bound) = rounded_box_ring_2d(bounds, *radius, 6)?;
            Some(vec![VisualPrism2D {
                outer,
                holes: Vec::new(),
                min_z: bounds.min[2],
                max_z: bounds.max[2],
                error_bound,
            }])
        }
        boon_solid_model::SolidOp::Union { children } => {
            let mut prisms = Vec::new();
            for child in children {
                prisms.extend(visual_prisms_from_solid_node(graph, *child)?);
            }
            Some(prisms)
        }
        boon_solid_model::SolidOp::Transform { child, transform } => {
            let translation = translation_from_transform_only(*transform)?;
            let mut prisms = visual_prisms_from_solid_node(graph, *child)?;
            for prism in &mut prisms {
                translate_ring_2d(&mut prism.outer, translation);
                for hole in &mut prism.holes {
                    translate_ring_2d(hole, translation);
                }
                prism.min_z += translation[2];
                prism.max_z += translation[2];
            }
            Some(prisms)
        }
        _ => None,
    }
}

fn visual_hole_from_solid_node(
    graph: &boon_solid_model::SolidGraph,
    node_id: boon_solid_model::SolidNodeId,
) -> Option<VisualHole2D> {
    let node = graph.nodes.get(&node_id)?;
    match &node.op {
        boon_solid_model::SolidOp::Cylinder { radius, .. } => {
            let bounds = bounds3d_from_solid_bounds(node.bounds);
            let radius = *radius as f32;
            if !radius.is_finite() || radius <= f32::EPSILON {
                return None;
            }
            let center = [
                (bounds.min[0] + bounds.max[0]) * 0.5,
                (bounds.min[1] + bounds.max[1]) * 0.5,
            ];
            Some(VisualHole2D {
                center,
                ring: circle_ring_2d(center, radius, 32),
                min_z: bounds.min[2],
                max_z: bounds.max[2],
                error_bound: circle_segment_error(radius as f64, 32),
            })
        }
        boon_solid_model::SolidOp::Box { .. } => {
            let bounds = bounds3d_from_solid_bounds(node.bounds);
            Some(VisualHole2D {
                center: [
                    (bounds.min[0] + bounds.max[0]) * 0.5,
                    (bounds.min[1] + bounds.max[1]) * 0.5,
                ],
                ring: rectangle_ring_2d(bounds),
                min_z: bounds.min[2],
                max_z: bounds.max[2],
                error_bound: 0.0,
            })
        }
        boon_solid_model::SolidOp::Transform { child, transform } => {
            let translation = translation_from_transform_only(*transform)?;
            let mut hole = visual_hole_from_solid_node(graph, *child)?;
            hole.center[0] += translation[0];
            hole.center[1] += translation[1];
            translate_ring_2d(&mut hole.ring, translation);
            hole.min_z += translation[2];
            hole.max_z += translation[2];
            Some(hole)
        }
        _ => None,
    }
}

fn mesh_chunk_from_visual_prisms(prisms: &[VisualPrism2D]) -> Option<IndexedMeshChunk> {
    let mut mesh = IndexedMeshChunk {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    for prism in prisms {
        if prism.outer.len() < 3 || prism.min_z >= prism.max_z {
            return None;
        }
        let (vertices_2d, hole_indices, rings) = prism_to_earcut_vertices(prism);
        let triangles = earcutr::earcut(&vertices_2d, &hole_indices, 2).ok()?;
        if triangles.is_empty() {
            return None;
        }
        let points = vertices_2d
            .chunks_exact(2)
            .map(|coords| [coords[0] as f32, coords[1] as f32])
            .collect::<Vec<_>>();
        for triangle in triangles.chunks_exact(3) {
            let a = points[triangle[0]];
            let b = points[triangle[1]];
            let c = points[triangle[2]];
            push_oriented_visual_cap(&mut mesh, a, b, c, prism.max_z, true);
            push_oriented_visual_cap(&mut mesh, a, c, b, prism.min_z, false);
        }
        if let Some(outer) = rings.first() {
            push_visual_ring_walls(&mut mesh, &points, outer, prism.min_z, prism.max_z, false);
        }
        for hole in rings.iter().skip(1) {
            push_visual_ring_walls(&mut mesh, &points, hole, prism.min_z, prism.max_z, true);
        }
    }
    (!mesh.vertices.is_empty() && !mesh.indices.is_empty()).then_some(mesh)
}

fn prism_to_earcut_vertices(prism: &VisualPrism2D) -> (Vec<f64>, Vec<usize>, Vec<Vec<usize>>) {
    let mut vertices = Vec::new();
    let mut hole_indices = Vec::new();
    let mut rings = Vec::new();
    push_visual_ring_for_earcut(&prism.outer, &mut vertices, &mut rings);
    for hole in &prism.holes {
        hole_indices.push(vertices.len() / 2);
        push_visual_ring_for_earcut(hole, &mut vertices, &mut rings);
    }
    (vertices, hole_indices, rings)
}

fn push_visual_ring_for_earcut(
    ring_points: &[[f32; 2]],
    vertices: &mut Vec<f64>,
    rings: &mut Vec<Vec<usize>>,
) {
    let mut ring = Vec::new();
    for point in ring_points {
        ring.push(vertices.len() / 2);
        vertices.push(point[0] as f64);
        vertices.push(point[1] as f64);
    }
    rings.push(ring);
}

fn push_oriented_visual_cap(
    mesh: &mut IndexedMeshChunk,
    a: [f32; 2],
    b: [f32; 2],
    c: [f32; 2],
    z: f32,
    top: bool,
) {
    let a = [a[0], a[1], z];
    let b = [b[0], b[1], z];
    let c = [c[0], c[1], z];
    let normal = normal_for_triangle(a, b, c);
    if (top && normal[2] < 0.0) || (!top && normal[2] > 0.0) {
        push_visual_triangle(mesh, a, c, b);
    } else {
        push_visual_triangle(mesh, a, b, c);
    }
}

fn push_visual_ring_walls(
    mesh: &mut IndexedMeshChunk,
    points: &[[f32; 2]],
    ring: &[usize],
    z0: f32,
    z1: f32,
    hole: bool,
) {
    if ring.len() < 2 {
        return;
    }
    for index in 0..ring.len() {
        let next = (index + 1) % ring.len();
        let a = points[ring[index]];
        let b = points[ring[next]];
        let a0 = [a[0], a[1], z0];
        let b0 = [b[0], b[1], z0];
        let a1 = [a[0], a[1], z1];
        let b1 = [b[0], b[1], z1];
        if hole {
            push_visual_triangle(mesh, b0, a0, a1);
            push_visual_triangle(mesh, b0, a1, b1);
        } else {
            push_visual_triangle(mesh, a0, b0, b1);
            push_visual_triangle(mesh, a0, b1, a1);
        }
    }
}

fn push_visual_triangle(mesh: &mut IndexedMeshChunk, a: [f32; 3], b: [f32; 3], c: [f32; 3]) {
    let normal = normal_for_triangle(a, b, c);
    let base = mesh.vertices.len() as u32;
    mesh.vertices.push(MeshVertex {
        position: a,
        normal,
    });
    mesh.vertices.push(MeshVertex {
        position: b,
        normal,
    });
    mesh.vertices.push(MeshVertex {
        position: c,
        normal,
    });
    mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
}

fn normal_for_triangle(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    normalize3(normal).unwrap_or([0.0, 0.0, 1.0])
}

fn rectangle_ring_2d(bounds: Bounds3D) -> Vec<[f32; 2]> {
    vec![
        [bounds.min[0], bounds.min[1]],
        [bounds.max[0], bounds.min[1]],
        [bounds.max[0], bounds.max[1]],
        [bounds.min[0], bounds.max[1]],
    ]
}

fn rounded_box_ring_2d(
    bounds: Bounds3D,
    radius: f64,
    corner_segments: u16,
) -> Option<(Vec<[f32; 2]>, f64)> {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    if min_x >= max_x || min_y >= max_y || min_z >= max_z {
        return None;
    }
    let half_x = (max_x - min_x) * 0.5;
    let half_y = (max_y - min_y) * 0.5;
    let radius = (radius as f32).min(half_x).min(half_y);
    if !radius.is_finite() || radius <= f32::EPSILON {
        return None;
    }
    let corner_segments = corner_segments.max(3);
    let corner_specs = [
        (max_x - radius, min_y + radius, -std::f32::consts::FRAC_PI_2),
        (max_x - radius, max_y - radius, 0.0),
        (min_x + radius, max_y - radius, std::f32::consts::FRAC_PI_2),
        (min_x + radius, min_y + radius, std::f32::consts::PI),
    ];
    let mut perimeter = Vec::with_capacity(usize::from(corner_segments) * 4 + 1);
    for (corner_index, (cx, cy, start_angle)) in corner_specs.into_iter().enumerate() {
        for step in 0..=corner_segments {
            if corner_index > 0 && step == 0 {
                continue;
            }
            let angle = start_angle
                + std::f32::consts::FRAC_PI_2 * f32::from(step) / f32::from(corner_segments);
            perimeter.push([cx + radius * angle.cos(), cy + radius * angle.sin()]);
        }
    }
    if perimeter.len() < 8 {
        return None;
    }
    let error_bound = rounded_segment_error(radius as f64, corner_segments);
    Some((perimeter, error_bound))
}

fn circle_ring_2d(center: [f32; 2], radius: f32, segments: u16) -> Vec<[f32; 2]> {
    let segments = segments.max(8);
    (0..segments)
        .map(|index| {
            let angle = std::f32::consts::TAU * f32::from(index) / f32::from(segments);
            [
                center[0] + angle.cos() * radius,
                center[1] + angle.sin() * radius,
            ]
        })
        .collect()
}

fn translate_ring_2d(ring: &mut [[f32; 2]], translation: [f32; 3]) {
    for point in ring {
        point[0] += translation[0];
        point[1] += translation[1];
    }
}

fn point_in_ring(point: [f32; 2], ring: &[[f32; 2]]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = ring.len() - 1;
    for current in 0..ring.len() {
        let a = ring[current];
        let b = ring[previous];
        if ((a[1] > point[1]) != (b[1] > point[1]))
            && (point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0])
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn rounded_segment_error(radius: f64, corner_segments: u16) -> f64 {
    let angle_step = std::f64::consts::FRAC_PI_2 / f64::from(corner_segments.max(1));
    radius * (1.0 - (angle_step * 0.5).cos())
}

fn circle_segment_error(radius: f64, segments: u16) -> f64 {
    let angle_step = std::f64::consts::TAU / f64::from(segments.max(3));
    radius * (1.0 - (angle_step * 0.5).cos())
}

fn mesh_chunk_from_bounds(bounds: Bounds3D) -> IndexedMeshChunk {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    let positions = [
        [min_x, min_y, min_z],
        [max_x, min_y, min_z],
        [max_x, max_y, min_z],
        [min_x, max_y, min_z],
        [min_x, min_y, max_z],
        [max_x, min_y, max_z],
        [max_x, max_y, max_z],
        [min_x, max_y, max_z],
    ];
    let center = [
        (min_x + max_x) * 0.5,
        (min_y + max_y) * 0.5,
        (min_z + max_z) * 0.5,
    ];
    let vertices = positions
        .into_iter()
        .map(|position| MeshVertex {
            position,
            normal: normal_from_center(position, center),
        })
        .collect();
    IndexedMeshChunk {
        vertices,
        indices: vec![
            0, 1, 2, 0, 2, 3, // back
            4, 6, 5, 4, 7, 6, // front
            0, 4, 5, 0, 5, 1, // bottom
            3, 2, 6, 3, 6, 7, // top
            1, 5, 6, 1, 6, 2, // right
            0, 3, 7, 0, 7, 4, // left
        ],
    }
}

fn mesh_chunk_from_rounded_box_bounds(
    bounds: Bounds3D,
    radius: f64,
    corner_segments: u16,
) -> Option<(IndexedMeshChunk, f64)> {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    if min_x >= max_x || min_y >= max_y || min_z >= max_z {
        return None;
    }
    let half_x = (max_x - min_x) * 0.5;
    let half_y = (max_y - min_y) * 0.5;
    let radius = (radius as f32).min(half_x).min(half_y);
    if !radius.is_finite() || radius <= f32::EPSILON {
        return None;
    }
    let corner_segments = corner_segments.max(3);
    let center = [(min_x + max_x) * 0.5, (min_y + max_y) * 0.5];
    let corner_specs = [
        (max_x - radius, min_y + radius, -std::f32::consts::FRAC_PI_2),
        (max_x - radius, max_y - radius, 0.0),
        (min_x + radius, max_y - radius, std::f32::consts::FRAC_PI_2),
        (min_x + radius, min_y + radius, std::f32::consts::PI),
    ];
    let mut perimeter = Vec::with_capacity(usize::from(corner_segments) * 4 + 1);
    for (corner_index, (cx, cy, start_angle)) in corner_specs.into_iter().enumerate() {
        for step in 0..=corner_segments {
            if corner_index > 0 && step == 0 {
                continue;
            }
            let angle = start_angle
                + std::f32::consts::FRAC_PI_2 * f32::from(step) / f32::from(corner_segments);
            perimeter.push([cx + radius * angle.cos(), cy + radius * angle.sin()]);
        }
    }
    if perimeter.len() < 8 {
        return None;
    }

    let mut vertices = Vec::with_capacity(perimeter.len() * 2 + 2);
    for point in &perimeter {
        let normal = normal_from_center([point[0], point[1], 0.0], [center[0], center[1], 0.0]);
        vertices.push(MeshVertex {
            position: [point[0], point[1], min_z],
            normal,
        });
        vertices.push(MeshVertex {
            position: [point[0], point[1], max_z],
            normal,
        });
    }
    let bottom_center = vertices.len() as u32;
    vertices.push(MeshVertex {
        position: [center[0], center[1], min_z],
        normal: [0.0, 0.0, -1.0],
    });
    let top_center = vertices.len() as u32;
    vertices.push(MeshVertex {
        position: [center[0], center[1], max_z],
        normal: [0.0, 0.0, 1.0],
    });

    let mut indices = Vec::with_capacity(perimeter.len() * 12);
    for index in 0..perimeter.len() as u32 {
        let next = (index + 1) % perimeter.len() as u32;
        let bottom = index * 2;
        let top = bottom + 1;
        let next_bottom = next * 2;
        let next_top = next_bottom + 1;
        indices.extend_from_slice(&[bottom, next_bottom, top, top, next_bottom, next_top]);
        indices.extend_from_slice(&[bottom_center, bottom, next_bottom]);
        indices.extend_from_slice(&[top_center, next_top, top]);
    }

    let angle_step = std::f64::consts::FRAC_PI_2 / f64::from(corner_segments);
    let error_bound = f64::from(radius) * (1.0 - (angle_step * 0.5).cos());
    Some((IndexedMeshChunk { vertices, indices }, error_bound))
}

fn mesh_chunk_from_cylinder_bounds(
    bounds: Bounds3D,
    radius: f32,
    height: f32,
    segments: u16,
) -> IndexedMeshChunk {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    let center_x = (min_x + max_x) * 0.5;
    let center_y = (min_y + max_y) * 0.5;
    let center_z = (min_z + max_z) * 0.5;
    let radius = if radius.is_finite() && radius > 0.0 {
        radius
    } else {
        ((max_x - min_x).abs().min((max_y - min_y).abs())) * 0.5
    };
    let half_height = if height.is_finite() && height > 0.0 {
        height * 0.5
    } else {
        (max_z - min_z).abs() * 0.5
    };
    let min_z = center_z - half_height;
    let max_z = center_z + half_height;
    let segments = segments.max(8);
    let mut vertices = Vec::with_capacity(segments as usize * 2 + 2);
    for index in 0..segments {
        let angle = std::f32::consts::TAU * f32::from(index) / f32::from(segments);
        let normal = [angle.cos(), angle.sin(), 0.0];
        let x = center_x + normal[0] * radius;
        let y = center_y + normal[1] * radius;
        vertices.push(MeshVertex {
            position: [x, y, min_z],
            normal,
        });
        vertices.push(MeshVertex {
            position: [x, y, max_z],
            normal,
        });
    }
    let bottom_center = vertices.len() as u32;
    vertices.push(MeshVertex {
        position: [center_x, center_y, min_z],
        normal: [0.0, 0.0, -1.0],
    });
    let top_center = vertices.len() as u32;
    vertices.push(MeshVertex {
        position: [center_x, center_y, max_z],
        normal: [0.0, 0.0, 1.0],
    });

    let mut indices = Vec::with_capacity(segments as usize * 12);
    for index in 0..u32::from(segments) {
        let next = (index + 1) % u32::from(segments);
        let bottom = index * 2;
        let top = bottom + 1;
        let next_bottom = next * 2;
        let next_top = next_bottom + 1;
        indices.extend_from_slice(&[bottom, next_bottom, top, top, next_bottom, next_top]);
        indices.extend_from_slice(&[bottom_center, bottom, next_bottom]);
        indices.extend_from_slice(&[top_center, next_top, top]);
    }
    IndexedMeshChunk { vertices, indices }
}

fn mesh_chunk_from_sphere_bounds(
    bounds: Bounds3D,
    radius: f32,
    segments: u16,
    rings: u16,
) -> IndexedMeshChunk {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    let center = [
        (min_x + max_x) * 0.5,
        (min_y + max_y) * 0.5,
        (min_z + max_z) * 0.5,
    ];
    let fallback_radius = ((max_x - min_x)
        .abs()
        .min((max_y - min_y).abs())
        .min((max_z - min_z).abs()))
        * 0.5;
    let radius = if radius.is_finite() && radius > 0.0 {
        radius
    } else {
        fallback_radius.max(0.001)
    };
    let segments = segments.max(8);
    let rings = rings.max(4);
    let mut vertices =
        Vec::with_capacity(2 + (rings.saturating_sub(1) as usize * segments as usize));
    let top_index = 0_u32;
    vertices.push(MeshVertex {
        position: [center[0], center[1], center[2] + radius],
        normal: [0.0, 0.0, 1.0],
    });
    for ring in 1..rings {
        let v = f32::from(ring) / f32::from(rings);
        let polar = std::f32::consts::PI * v;
        let z = polar.cos();
        let ring_radius = polar.sin();
        for segment in 0..segments {
            let angle = std::f32::consts::TAU * f32::from(segment) / f32::from(segments);
            let normal = [ring_radius * angle.cos(), ring_radius * angle.sin(), z];
            vertices.push(MeshVertex {
                position: [
                    center[0] + normal[0] * radius,
                    center[1] + normal[1] * radius,
                    center[2] + normal[2] * radius,
                ],
                normal,
            });
        }
    }
    let bottom_index = vertices.len() as u32;
    vertices.push(MeshVertex {
        position: [center[0], center[1], center[2] - radius],
        normal: [0.0, 0.0, -1.0],
    });

    let ring_start = |ring: u32| 1 + (ring - 1) * u32::from(segments);
    let mut indices = Vec::with_capacity(usize::from(segments) * usize::from(rings) * 6);
    let first_ring = ring_start(1);
    for segment in 0..u32::from(segments) {
        let next = (segment + 1) % u32::from(segments);
        indices.extend_from_slice(&[top_index, first_ring + next, first_ring + segment]);
    }
    for ring in 1..u32::from(rings.saturating_sub(1)) {
        let current = ring_start(ring);
        let next_ring = ring_start(ring + 1);
        for segment in 0..u32::from(segments) {
            let next = (segment + 1) % u32::from(segments);
            let a = current + segment;
            let b = next_ring + segment;
            let c = next_ring + next;
            let d = current + next;
            indices.extend_from_slice(&[a, b, d, d, b, c]);
        }
    }
    let last_ring = ring_start(u32::from(rings.saturating_sub(1)));
    for segment in 0..u32::from(segments) {
        let next = (segment + 1) % u32::from(segments);
        indices.extend_from_slice(&[bottom_index, last_ring + segment, last_ring + next]);
    }
    IndexedMeshChunk { vertices, indices }
}

fn mesh_chunk_from_cone_bounds(
    bounds: Bounds3D,
    radius0: f32,
    radius1: f32,
    height: f32,
    segments: u16,
) -> IndexedMeshChunk {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    let center_x = (min_x + max_x) * 0.5;
    let center_y = (min_y + max_y) * 0.5;
    let center_z = (min_z + max_z) * 0.5;
    let fallback_radius = ((max_x - min_x).abs().min((max_y - min_y).abs())) * 0.5;
    let radius0 = if radius0.is_finite() && radius0 >= 0.0 {
        radius0
    } else {
        fallback_radius
    };
    let radius1 = if radius1.is_finite() && radius1 >= 0.0 {
        radius1
    } else {
        0.0
    };
    let half_height = if height.is_finite() && height > 0.0 {
        height * 0.5
    } else {
        (max_z - min_z).abs() * 0.5
    };
    let min_z = center_z - half_height;
    let max_z = center_z + half_height;
    let height = (max_z - min_z).abs().max(0.001);
    let segments = segments.max(8);
    let mut vertices = Vec::with_capacity(segments as usize * 2 + 2);
    let side_z = (radius0 - radius1) / height;
    for index in 0..segments {
        let angle = std::f32::consts::TAU * f32::from(index) / f32::from(segments);
        let radial = [angle.cos(), angle.sin()];
        let normal = normal_from_center([radial[0], radial[1], side_z], [0.0, 0.0, 0.0]);
        vertices.push(MeshVertex {
            position: [
                center_x + radial[0] * radius0,
                center_y + radial[1] * radius0,
                min_z,
            ],
            normal,
        });
        vertices.push(MeshVertex {
            position: [
                center_x + radial[0] * radius1,
                center_y + radial[1] * radius1,
                max_z,
            ],
            normal,
        });
    }
    let bottom_center = vertices.len() as u32;
    vertices.push(MeshVertex {
        position: [center_x, center_y, min_z],
        normal: [0.0, 0.0, -1.0],
    });
    let top_center = vertices.len() as u32;
    vertices.push(MeshVertex {
        position: [center_x, center_y, max_z],
        normal: [0.0, 0.0, 1.0],
    });

    let mut indices = Vec::with_capacity(segments as usize * 12);
    for index in 0..u32::from(segments) {
        let next = (index + 1) % u32::from(segments);
        let bottom = index * 2;
        let top = bottom + 1;
        let next_bottom = next * 2;
        let next_top = next_bottom + 1;
        indices.extend_from_slice(&[bottom, next_bottom, top, top, next_bottom, next_top]);
        if radius0 > f32::EPSILON {
            indices.extend_from_slice(&[bottom_center, bottom, next_bottom]);
        }
        if radius1 > f32::EPSILON {
            indices.extend_from_slice(&[top_center, next_top, top]);
        }
    }
    IndexedMeshChunk { vertices, indices }
}

fn mesh_chunk_from_torus_bounds(
    bounds: Bounds3D,
    major_radius: f32,
    minor_radius: f32,
    major_segments: u16,
    minor_segments: u16,
) -> IndexedMeshChunk {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    let center = [
        (min_x + max_x) * 0.5,
        (min_y + max_y) * 0.5,
        (min_z + max_z) * 0.5,
    ];
    let fallback_outer = ((max_x - min_x).abs().min((max_y - min_y).abs())) * 0.5;
    let minor_radius = if minor_radius.is_finite() && minor_radius > 0.0 {
        minor_radius
    } else {
        ((max_z - min_z).abs() * 0.5).max(0.001)
    };
    let major_radius = if major_radius.is_finite() && major_radius > 0.0 {
        major_radius
    } else {
        (fallback_outer - minor_radius).max(minor_radius)
    };
    let major_segments = major_segments.max(8);
    let minor_segments = minor_segments.max(6);
    let mut vertices = Vec::with_capacity(major_segments as usize * minor_segments as usize);
    for major in 0..major_segments {
        let major_angle = std::f32::consts::TAU * f32::from(major) / f32::from(major_segments);
        let major_dir = [major_angle.cos(), major_angle.sin()];
        for minor in 0..minor_segments {
            let minor_angle = std::f32::consts::TAU * f32::from(minor) / f32::from(minor_segments);
            let radial = major_radius + minor_radius * minor_angle.cos();
            let normal = [
                major_dir[0] * minor_angle.cos(),
                major_dir[1] * minor_angle.cos(),
                minor_angle.sin(),
            ];
            vertices.push(MeshVertex {
                position: [
                    center[0] + major_dir[0] * radial,
                    center[1] + major_dir[1] * radial,
                    center[2] + minor_radius * minor_angle.sin(),
                ],
                normal,
            });
        }
    }

    let mut indices = Vec::with_capacity(major_segments as usize * minor_segments as usize * 6);
    let minor_count = u32::from(minor_segments);
    for major in 0..u32::from(major_segments) {
        let next_major = (major + 1) % u32::from(major_segments);
        for minor in 0..minor_count {
            let next_minor = (minor + 1) % minor_count;
            let a = major * minor_count + minor;
            let b = next_major * minor_count + minor;
            let c = next_major * minor_count + next_minor;
            let d = major * minor_count + next_minor;
            indices.extend_from_slice(&[a, b, d, d, b, c]);
        }
    }
    IndexedMeshChunk { vertices, indices }
}

fn mesh_chunk_from_extrude_profile(
    graph: &boon_solid_model::SolidGraph,
    profile: boon_solid_model::ProfileId,
    height: f64,
    bounds: Bounds3D,
) -> Option<IndexedMeshChunk> {
    if !height.is_finite() || height <= 0.0 {
        return None;
    }
    let profile = graph.profiles.get(&profile)?;
    if !profile.closed || profile.segment_count != 4 {
        return None;
    }
    if profile.bounds.min.x >= profile.bounds.max.x || profile.bounds.min.y >= profile.bounds.max.y
    {
        return None;
    }
    let min_z = bounds.min[2];
    let max_z = bounds.max[2];
    if min_z >= max_z {
        return None;
    }

    mesh_chunk_from_rectangular_prism(
        profile.bounds.min.x as f32,
        profile.bounds.max.x as f32,
        profile.bounds.min.y as f32,
        profile.bounds.max.y as f32,
        min_z,
        max_z,
    )
}

fn mesh_chunk_from_revolve_profile(
    graph: &boon_solid_model::SolidGraph,
    profile: boon_solid_model::ProfileId,
    axis: boon_solid_model::Axis3d,
    segments: u16,
) -> Option<IndexedMeshChunk> {
    if !axis_is_default_z(axis) {
        return None;
    }
    let profile = graph.profiles.get(&profile)?;
    if !profile.closed || profile.segment_count != 4 {
        return None;
    }
    let inner_radius = profile.bounds.min.x.abs().min(profile.bounds.max.x.abs()) as f32;
    let outer_radius = profile.bounds.min.x.abs().max(profile.bounds.max.x.abs()) as f32;
    let min_z = profile.bounds.min.z.min(profile.bounds.max.z) as f32;
    let max_z = profile.bounds.min.z.max(profile.bounds.max.z) as f32;
    if !inner_radius.is_finite()
        || !outer_radius.is_finite()
        || inner_radius <= f32::EPSILON
        || inner_radius >= outer_radius
        || min_z >= max_z
    {
        return None;
    }

    let segments = segments.max(8);
    let mut vertices = Vec::with_capacity(segments as usize * 4);
    for index in 0..segments {
        let angle = std::f32::consts::TAU * f32::from(index) / f32::from(segments);
        let dir = [angle.cos(), angle.sin()];
        let outer_normal = [dir[0], dir[1], 0.0];
        let inner_normal = [-dir[0], -dir[1], 0.0];
        vertices.push(MeshVertex {
            position: [
                axis.origin.x as f32 + dir[0] * outer_radius,
                axis.origin.y as f32 + dir[1] * outer_radius,
                min_z,
            ],
            normal: outer_normal,
        });
        vertices.push(MeshVertex {
            position: [
                axis.origin.x as f32 + dir[0] * outer_radius,
                axis.origin.y as f32 + dir[1] * outer_radius,
                max_z,
            ],
            normal: outer_normal,
        });
        vertices.push(MeshVertex {
            position: [
                axis.origin.x as f32 + dir[0] * inner_radius,
                axis.origin.y as f32 + dir[1] * inner_radius,
                min_z,
            ],
            normal: inner_normal,
        });
        vertices.push(MeshVertex {
            position: [
                axis.origin.x as f32 + dir[0] * inner_radius,
                axis.origin.y as f32 + dir[1] * inner_radius,
                max_z,
            ],
            normal: inner_normal,
        });
    }

    let mut indices = Vec::with_capacity(segments as usize * 24);
    for index in 0..u32::from(segments) {
        let next = (index + 1) % u32::from(segments);
        let outer_bottom = index * 4;
        let outer_top = outer_bottom + 1;
        let inner_bottom = outer_bottom + 2;
        let inner_top = outer_bottom + 3;
        let next_outer_bottom = next * 4;
        let next_outer_top = next_outer_bottom + 1;
        let next_inner_bottom = next_outer_bottom + 2;
        let next_inner_top = next_outer_bottom + 3;

        indices.extend_from_slice(&[
            outer_bottom,
            next_outer_bottom,
            outer_top,
            outer_top,
            next_outer_bottom,
            next_outer_top,
        ]);
        indices.extend_from_slice(&[
            inner_bottom,
            inner_top,
            next_inner_bottom,
            inner_top,
            next_inner_top,
            next_inner_bottom,
        ]);
        indices.extend_from_slice(&[
            outer_top,
            next_outer_top,
            inner_top,
            inner_top,
            next_outer_top,
            next_inner_top,
        ]);
        indices.extend_from_slice(&[
            outer_bottom,
            inner_bottom,
            next_outer_bottom,
            inner_bottom,
            next_inner_bottom,
            next_outer_bottom,
        ]);
    }

    Some(IndexedMeshChunk { vertices, indices })
}

fn mesh_chunk_from_loft_profiles(
    graph: &boon_solid_model::SolidGraph,
    profiles: &[boon_solid_model::ProfileId],
) -> Option<IndexedMeshChunk> {
    if profiles.len() != 2 {
        return None;
    }
    let bottom = graph.profiles.get(&profiles[0])?;
    let top = graph.profiles.get(&profiles[1])?;
    if !bottom.closed || !top.closed || bottom.segment_count != 4 || top.segment_count != 4 {
        return None;
    }
    if bottom.bounds.min.x >= bottom.bounds.max.x
        || bottom.bounds.min.y >= bottom.bounds.max.y
        || top.bounds.min.x >= top.bounds.max.x
        || top.bounds.min.y >= top.bounds.max.y
    {
        return None;
    }
    let bottom_z = bottom.bounds.min.z.min(bottom.bounds.max.z) as f32;
    let top_z = top.bounds.min.z.max(top.bounds.max.z) as f32;
    if bottom_z >= top_z {
        return None;
    }

    let positions = [
        [
            bottom.bounds.min.x as f32,
            bottom.bounds.min.y as f32,
            bottom_z,
        ],
        [
            bottom.bounds.max.x as f32,
            bottom.bounds.min.y as f32,
            bottom_z,
        ],
        [
            bottom.bounds.max.x as f32,
            bottom.bounds.max.y as f32,
            bottom_z,
        ],
        [
            bottom.bounds.min.x as f32,
            bottom.bounds.max.y as f32,
            bottom_z,
        ],
        [top.bounds.min.x as f32, top.bounds.min.y as f32, top_z],
        [top.bounds.max.x as f32, top.bounds.min.y as f32, top_z],
        [top.bounds.max.x as f32, top.bounds.max.y as f32, top_z],
        [top.bounds.min.x as f32, top.bounds.max.y as f32, top_z],
    ];
    mesh_chunk_from_prism_positions(positions)
}

fn mesh_chunk_from_rectangular_prism(
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    min_z: f32,
    max_z: f32,
) -> Option<IndexedMeshChunk> {
    mesh_chunk_from_prism_positions([
        [min_x, min_y, min_z],
        [max_x, min_y, min_z],
        [max_x, max_y, min_z],
        [min_x, max_y, min_z],
        [min_x, min_y, max_z],
        [max_x, min_y, max_z],
        [max_x, max_y, max_z],
        [min_x, max_y, max_z],
    ])
}

fn mesh_chunk_from_prism_positions(positions: [[f32; 3]; 8]) -> Option<IndexedMeshChunk> {
    let center = [
        positions.iter().map(|position| position[0]).sum::<f32>() / positions.len() as f32,
        positions.iter().map(|position| position[1]).sum::<f32>() / positions.len() as f32,
        positions.iter().map(|position| position[2]).sum::<f32>() / positions.len() as f32,
    ];
    let vertices = positions
        .into_iter()
        .map(|position| MeshVertex {
            position,
            normal: normal_from_center(position, center),
        })
        .collect();
    Some(IndexedMeshChunk {
        vertices,
        indices: vec![
            0, 2, 1, 0, 3, 2, // bottom
            4, 5, 6, 4, 6, 7, // top
            0, 1, 5, 0, 5, 4, // front
            1, 2, 6, 1, 6, 5, // right
            2, 3, 7, 2, 7, 6, // back
            3, 0, 4, 3, 4, 7, // left
        ],
    })
}

fn mesh_chunk_from_box_like_shell(
    graph: &boon_solid_model::SolidGraph,
    child: boon_solid_model::SolidNodeId,
    thickness: f64,
) -> Option<IndexedMeshChunk> {
    if !thickness.is_finite() || thickness <= 0.0 {
        return None;
    }
    let child = graph.nodes.get(&child)?;
    if !matches!(
        child.op,
        boon_solid_model::SolidOp::Box { .. } | boon_solid_model::SolidOp::RoundedBox { .. }
    ) {
        return None;
    }
    let outer = bounds3d_from_solid_bounds(child.bounds);
    let thickness = thickness as f32;
    if !thickness.is_finite() || thickness <= 0.0 {
        return None;
    }
    let inner = Bounds3D {
        min: [
            outer.min[0] + thickness,
            outer.min[1] + thickness,
            outer.min[2] + thickness,
        ],
        max: [
            outer.max[0] - thickness,
            outer.max[1] - thickness,
            outer.max[2] - thickness,
        ],
    };
    if inner.min[0] >= inner.max[0] || inner.min[1] >= inner.max[1] || inner.min[2] >= inner.max[2]
    {
        return None;
    }

    let mut vertices = Vec::with_capacity(48);
    let mut indices = Vec::with_capacity(72);
    append_box_faces(&mut vertices, &mut indices, outer, false);
    append_box_faces(&mut vertices, &mut indices, inner, true);
    Some(IndexedMeshChunk { vertices, indices })
}

fn append_box_faces(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    bounds: Bounds3D,
    inward: bool,
) {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    let faces = [
        (
            [0.0, 0.0, -1.0],
            [
                [min_x, min_y, min_z],
                [min_x, max_y, min_z],
                [max_x, max_y, min_z],
                [max_x, min_y, min_z],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [min_x, min_y, max_z],
                [max_x, min_y, max_z],
                [max_x, max_y, max_z],
                [min_x, max_y, max_z],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [min_x, min_y, min_z],
                [max_x, min_y, min_z],
                [max_x, min_y, max_z],
                [min_x, min_y, max_z],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [max_x, min_y, min_z],
                [max_x, max_y, min_z],
                [max_x, max_y, max_z],
                [max_x, min_y, max_z],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [max_x, max_y, min_z],
                [min_x, max_y, min_z],
                [min_x, max_y, max_z],
                [max_x, max_y, max_z],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [min_x, max_y, min_z],
                [min_x, min_y, min_z],
                [min_x, min_y, max_z],
                [min_x, max_y, max_z],
            ],
        ),
    ];

    for (normal, positions) in faces {
        let base = vertices.len() as u32;
        let normal = if inward {
            [-normal[0], -normal[1], -normal[2]]
        } else {
            normal
        };
        vertices.extend(
            positions
                .into_iter()
                .map(|position| MeshVertex { position, normal }),
        );
        if inward {
            indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        } else {
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

fn axis_is_default_z(axis: boon_solid_model::Axis3d) -> bool {
    axis.origin.x.abs() <= f64::EPSILON
        && axis.origin.y.abs() <= f64::EPSILON
        && axis.direction.x.abs() <= f64::EPSILON
        && axis.direction.y.abs() <= f64::EPSILON
        && (axis.direction.z.abs() - 1.0).abs() <= f64::EPSILON
}

fn normal_from_center(position: [f32; 3], center: [f32; 3]) -> [f32; 3] {
    let vector = [
        position[0] - center[0],
        position[1] - center[1],
        position[2] - center[2],
    ];
    let length = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        [vector[0] / length, vector[1] / length, vector[2] / length]
    }
}

fn surface_chunk_vertex_count(chunk: &SurfaceChunk) -> usize {
    match &chunk.representation {
        SurfaceRepresentation::IndexedMeshSummary { vertex_count, .. } => *vertex_count as usize,
        SurfaceRepresentation::IndexedMesh(mesh) => mesh.vertices.len(),
        SurfaceRepresentation::DirectedDualGridSummary { .. } => 0,
    }
}

fn surface_chunk_index_count(chunk: &SurfaceChunk) -> usize {
    match &chunk.representation {
        SurfaceRepresentation::IndexedMeshSummary { index_count, .. } => *index_count as usize,
        SurfaceRepresentation::IndexedMesh(mesh) => mesh.indices.len(),
        SurfaceRepresentation::DirectedDualGridSummary { .. } => 0,
    }
}

fn finite_f32(value: f64) -> f32 {
    if value.is_finite() {
        value.clamp(f32::MIN as f64, f32::MAX as f64) as f32
    } else {
        0.0
    }
}

fn orbit_camera_transform(
    transform: Transform3D,
    drag: WorldOrbitCameraDrag,
) -> Result<Transform3D, String> {
    if !is_finite_vec3(drag.target)
        || !drag.yaw_delta_radians.is_finite()
        || !drag.pitch_delta_radians.is_finite()
        || !drag.min_distance.is_finite()
    {
        return Err("orbit camera drag contains non-finite values".to_owned());
    }
    let offset = sub3(transform.translation, drag.target);
    let mut distance = length3(offset);
    let min_distance = drag.min_distance.max(0.001);
    if distance < min_distance {
        distance = min_distance;
    }

    let horizontal = (offset[0] * offset[0] + offset[2] * offset[2]).sqrt();
    let yaw = offset[0].atan2(offset[2]) + drag.yaw_delta_radians;
    let pitch_limit = std::f32::consts::FRAC_PI_2 - 0.01;
    let pitch =
        (offset[1].atan2(horizontal) + drag.pitch_delta_radians).clamp(-pitch_limit, pitch_limit);
    let cos_pitch = pitch.cos();
    let new_offset = [
        distance * cos_pitch * yaw.sin(),
        distance * pitch.sin(),
        distance * cos_pitch * yaw.cos(),
    ];
    let translation = add3(drag.target, new_offset);
    Ok(Transform3D {
        translation,
        rotation_xyzw: look_at_rotation_xyzw(translation, drag.target),
        scale: transform.scale,
    })
}

fn look_at_rotation_xyzw(eye: [f32; 3], target: [f32; 3]) -> [f32; 4] {
    let forward = normalize3(sub3(target, eye)).unwrap_or([0.0, 0.0, -1.0]);
    let world_up = [0.0, 1.0, 0.0];
    let mut right = normalize3(cross3(forward, world_up));
    if right.is_none() {
        right = normalize3(cross3(forward, [1.0, 0.0, 0.0]));
    }
    let right = right.unwrap_or([1.0, 0.0, 0.0]);
    let up = cross3(right, forward);
    quat_from_mat3(
        [right[0], up[0], -forward[0]],
        [right[1], up[1], -forward[1]],
        [right[2], up[2], -forward[2]],
    )
}

fn quat_from_mat3(row0: [f32; 3], row1: [f32; 3], row2: [f32; 3]) -> [f32; 4] {
    let trace = row0[0] + row1[1] + row2[2];
    let quat = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (row2[1] - row1[2]) / s,
            (row0[2] - row2[0]) / s,
            (row1[0] - row0[1]) / s,
            0.25 * s,
        ]
    } else if row0[0] > row1[1] && row0[0] > row2[2] {
        let s = (1.0 + row0[0] - row1[1] - row2[2]).sqrt() * 2.0;
        [
            0.25 * s,
            (row0[1] + row1[0]) / s,
            (row0[2] + row2[0]) / s,
            (row2[1] - row1[2]) / s,
        ]
    } else if row1[1] > row2[2] {
        let s = (1.0 + row1[1] - row0[0] - row2[2]).sqrt() * 2.0;
        [
            (row0[1] + row1[0]) / s,
            0.25 * s,
            (row1[2] + row2[1]) / s,
            (row0[2] - row2[0]) / s,
        ]
    } else {
        let s = (1.0 + row2[2] - row0[0] - row1[1]).sqrt() * 2.0;
        [
            (row0[2] + row2[0]) / s,
            (row1[2] + row2[1]) / s,
            0.25 * s,
            (row1[0] - row0[1]) / s,
        ]
    };
    normalize4(quat).unwrap_or(Transform3D::IDENTITY.rotation_xyzw)
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length3(value: [f32; 3]) -> f32 {
    (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt()
}

fn normalize3(value: [f32; 3]) -> Option<[f32; 3]> {
    let length = length3(value);
    if length > f32::EPSILON && length.is_finite() {
        Some([value[0] / length, value[1] / length, value[2] / length])
    } else {
        None
    }
}

fn normalize4(value: [f32; 4]) -> Option<[f32; 4]> {
    let length =
        (value[0] * value[0] + value[1] * value[1] + value[2] * value[2] + value[3] * value[3])
            .sqrt();
    if length > f32::EPSILON && length.is_finite() {
        Some([
            value[0] / length,
            value[1] / length,
            value[2] / length,
            value[3] / length,
        ])
    } else {
        None
    }
}

fn is_finite_vec3(value: [f32; 3]) -> bool {
    value.iter().all(|component| component.is_finite())
}

#[cfg(test)]
mod tests;
