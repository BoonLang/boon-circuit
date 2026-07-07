use std::collections::{BTreeMap, BTreeSet};

macro_rules! u64_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(
                Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
            )]
            pub struct $name(pub u64);
        )+
    };
}

u64_ids!(
    SolidNodeId,
    GeometryLogicalId,
    FeatureId,
    RegionId,
    ProfileId,
    CurveId,
    EvaluatorId,
    ImportedSolidId,
    AppearanceMaterialId,
    PhysicalMaterialId,
    PartId,
    PartInstanceId,
    AssemblyId,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Units {
    Millimeter,
    Meter,
    Inch,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Vec3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Aabb64 {
    pub min: Vec3d,
    pub max: Vec3d,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Mat4d {
    pub columns: [[f64; 4]; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Axis3d {
    pub origin: Vec3d,
    pub direction: Vec3d,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SolidGraph {
    pub nodes: BTreeMap<SolidNodeId, SolidNode>,
    pub root: SolidNodeId,
    pub units: Units,
    pub tolerance: ManufacturingTolerance,
    pub profiles: BTreeMap<ProfileId, ProfileSummary>,
    pub curves: BTreeMap<CurveId, CurveSummary>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SolidNode {
    pub id: SolidNodeId,
    pub logical_id: GeometryLogicalId,
    pub op: SolidOp,
    pub bounds: Aabb64,
    pub feature_id: FeatureId,
    pub physical_region: RegionId,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum SolidOp {
    Box {
        size: Vec3d,
    },
    RoundedBox {
        size: Vec3d,
        radius: f64,
    },
    Sphere {
        radius: f64,
    },
    Cylinder {
        radius: f64,
        height: f64,
    },
    Cone {
        radius0: f64,
        radius1: f64,
        height: f64,
    },
    Torus {
        major_radius: f64,
        minor_radius: f64,
    },
    Extrude {
        profile: ProfileId,
        height: f64,
    },
    Revolve {
        profile: ProfileId,
        axis: Axis3d,
    },
    Sweep {
        profile: ProfileId,
        path: CurveId,
    },
    Loft {
        profiles: Vec<ProfileId>,
    },
    Union {
        children: Vec<SolidNodeId>,
    },
    Intersection {
        children: Vec<SolidNodeId>,
    },
    Difference {
        base: SolidNodeId,
        tools: Vec<SolidNodeId>,
    },
    Transform {
        child: SolidNodeId,
        transform: Mat4d,
    },
    Offset {
        child: SolidNodeId,
        distance: f64,
    },
    Shell {
        child: SolidNodeId,
        thickness: f64,
    },
    SmoothUnion {
        a: SolidNodeId,
        b: SolidNodeId,
        radius: f64,
    },
    Functional {
        evaluator: EvaluatorId,
    },
    ImportedSolid {
        source: ImportedSolidId,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ProfileSummary {
    pub bounds: Aabb64,
    pub segment_count: u32,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CurveSummary {
    pub bounds: Aabb64,
    pub segment_count: u32,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ManufacturingTolerance {
    pub linear_error: f64,
    pub z_error: f64,
    pub minimum_feature: f64,
    pub integer_grid: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AssemblyGraph {
    pub id: AssemblyId,
    pub parts: BTreeMap<PartId, PartDefinition>,
    pub instances: Vec<PartInstance>,
    pub constraints: Vec<AssemblyConstraint>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PartDefinition {
    pub id: PartId,
    pub geometry: GeometryLogicalId,
    pub root: SolidNodeId,
    pub appearance: Option<AppearanceMaterialId>,
    pub physical_material: Option<PhysicalMaterialId>,
    pub manufacturing_role: ManufacturingRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum ManufacturingRole {
    PrintableSolid,
    VisualOnly,
    VoidModifier,
    SupportModifier,
    InfillModifier,
    Reference,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PartInstance {
    pub id: PartInstanceId,
    pub part: PartId,
    pub transform: Mat4d,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum AssemblyConstraint {
    Fixed {
        instance: PartInstanceId,
    },
    Coincident {
        a: PartInstanceId,
        b: PartInstanceId,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SolidModelBundle {
    pub solids: BTreeMap<GeometryLogicalId, SolidGraph>,
    pub assembly: AssemblyGraph,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct SolidGraphMetrics {
    pub node_count: usize,
    pub primitive_node_count: usize,
    pub boolean_node_count: usize,
    pub transform_node_count: usize,
    pub subtractive_cylinder_count: usize,
    pub physical_region_count: usize,
    pub max_depth: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct AssemblyMetrics {
    pub part_count: usize,
    pub instance_count: usize,
    pub printable_part_count: usize,
    pub visual_only_part_count: usize,
    pub void_modifier_part_count: usize,
    pub physical_material_part_count: usize,
    pub shared_geometry_instance_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct SolidValidationReport {
    pub status: SolidValidationStatus,
    pub diagnostics: Vec<SolidDiagnostic>,
    pub printable_closed_region_count: usize,
    pub unresolved_reference_count: usize,
    pub minimum_feature_violation_count: usize,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum SolidValidationStatus {
    #[default]
    Pass,
    Fail,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct SolidDiagnostic {
    pub code: String,
    pub node: Option<SolidNodeId>,
    pub message: String,
}

impl Vec3d {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn translate(self, by: Vec3d) -> Self {
        Self {
            x: self.x + by.x,
            y: self.y + by.y,
            z: self.z + by.z,
        }
    }
}

impl Aabb64 {
    pub const EMPTY: Self = Self {
        min: Vec3d {
            x: f64::INFINITY,
            y: f64::INFINITY,
            z: f64::INFINITY,
        },
        max: Vec3d {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
            z: f64::NEG_INFINITY,
        },
    };

    pub fn from_center_size(center: Vec3d, size: Vec3d) -> Self {
        let half = Vec3d::new(size.x * 0.5, size.y * 0.5, size.z * 0.5);
        Self {
            min: Vec3d::new(center.x - half.x, center.y - half.y, center.z - half.z),
            max: Vec3d::new(center.x + half.x, center.y + half.y, center.z + half.z),
        }
    }

    pub fn from_cylinder_z(center: Vec3d, radius: f64, height: f64) -> Self {
        let radius = radius.abs();
        let half_height = height.abs() * 0.5;
        Self {
            min: Vec3d::new(center.x - radius, center.y - radius, center.z - half_height),
            max: Vec3d::new(center.x + radius, center.y + radius, center.z + half_height),
        }
    }

    pub fn union(self, other: Aabb64) -> Self {
        Self {
            min: Vec3d::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: Vec3d::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        }
    }

    pub fn translate(self, by: Vec3d) -> Self {
        Self {
            min: self.min.translate(by),
            max: self.max.translate(by),
        }
    }
}

impl Mat4d {
    pub const IDENTITY: Self = Self {
        columns: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    pub fn translation(by: Vec3d) -> Self {
        let mut transform = Self::IDENTITY;
        transform.columns[3] = [by.x, by.y, by.z, 1.0];
        transform
    }
}

impl Default for ManufacturingTolerance {
    fn default() -> Self {
        Self {
            linear_error: 0.03,
            z_error: 0.10,
            minimum_feature: 0.40,
            integer_grid: 0.001,
        }
    }
}

impl SolidModelBundle {
    pub fn printable_bracket_fixture() -> Self {
        printable_bracket_fixture_with_hole_diameter(6.4)
    }

    pub fn minimum_feature_negative_fixture() -> Self {
        printable_bracket_fixture_with_hole_diameter(0.20)
    }

    pub fn shell_box_fixture() -> Self {
        let geometry = GeometryLogicalId(1);
        let bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(40.0, 24.0, 16.0));
        let mut nodes = BTreeMap::new();
        insert_node(
            &mut nodes,
            SolidNode {
                id: SolidNodeId(1),
                logical_id: geometry,
                op: SolidOp::Box {
                    size: Vec3d::new(40.0, 24.0, 16.0),
                },
                bounds,
                feature_id: FeatureId(1),
                physical_region: RegionId(1),
            },
        );
        insert_node(
            &mut nodes,
            SolidNode {
                id: SolidNodeId(2),
                logical_id: geometry,
                op: SolidOp::Shell {
                    child: SolidNodeId(1),
                    thickness: 2.0,
                },
                bounds,
                feature_id: FeatureId(2),
                physical_region: RegionId(1),
            },
        );

        let mut solids = BTreeMap::new();
        solids.insert(
            geometry,
            SolidGraph {
                nodes,
                root: SolidNodeId(2),
                units: Units::Millimeter,
                tolerance: ManufacturingTolerance::default(),
                profiles: BTreeMap::new(),
                curves: BTreeMap::new(),
            },
        );
        let mut parts = BTreeMap::new();
        parts.insert(
            PartId(1),
            PartDefinition {
                id: PartId(1),
                geometry,
                root: SolidNodeId(2),
                appearance: Some(AppearanceMaterialId(1)),
                physical_material: Some(PhysicalMaterialId(1)),
                manufacturing_role: ManufacturingRole::PrintableSolid,
            },
        );
        SolidModelBundle {
            solids,
            assembly: AssemblyGraph {
                id: AssemblyId(1),
                parts,
                instances: vec![PartInstance {
                    id: PartInstanceId(1),
                    part: PartId(1),
                    transform: Mat4d::IDENTITY,
                    label: "Shell box".to_owned(),
                }],
                constraints: vec![AssemblyConstraint::Fixed {
                    instance: PartInstanceId(1),
                }],
            },
        }
    }

    pub fn extruded_rectangle_fixture() -> Self {
        let geometry = GeometryLogicalId(1);
        let profile = ProfileId(1);
        let profile_bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(36.0, 18.0, 0.0));
        let bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(36.0, 18.0, 12.0));
        let mut nodes = BTreeMap::new();
        insert_node(
            &mut nodes,
            SolidNode {
                id: SolidNodeId(1),
                logical_id: geometry,
                op: SolidOp::Extrude {
                    profile,
                    height: 12.0,
                },
                bounds,
                feature_id: FeatureId(1),
                physical_region: RegionId(1),
            },
        );
        let mut profiles = BTreeMap::new();
        profiles.insert(
            profile,
            ProfileSummary {
                bounds: profile_bounds,
                segment_count: 4,
                closed: true,
            },
        );

        let mut solids = BTreeMap::new();
        solids.insert(
            geometry,
            SolidGraph {
                nodes,
                root: SolidNodeId(1),
                units: Units::Millimeter,
                tolerance: ManufacturingTolerance::default(),
                profiles,
                curves: BTreeMap::new(),
            },
        );
        let mut parts = BTreeMap::new();
        parts.insert(
            PartId(1),
            PartDefinition {
                id: PartId(1),
                geometry,
                root: SolidNodeId(1),
                appearance: Some(AppearanceMaterialId(1)),
                physical_material: Some(PhysicalMaterialId(1)),
                manufacturing_role: ManufacturingRole::PrintableSolid,
            },
        );
        SolidModelBundle {
            solids,
            assembly: AssemblyGraph {
                id: AssemblyId(1),
                parts,
                instances: vec![PartInstance {
                    id: PartInstanceId(1),
                    part: PartId(1),
                    transform: Mat4d::IDENTITY,
                    label: "Extruded rectangle".to_owned(),
                }],
                constraints: vec![AssemblyConstraint::Fixed {
                    instance: PartInstanceId(1),
                }],
            },
        }
    }

    pub fn revolved_ring_fixture() -> Self {
        let geometry = GeometryLogicalId(1);
        let profile = ProfileId(1);
        let profile_bounds = Aabb64 {
            min: Vec3d::new(4.0, 0.0, -6.0),
            max: Vec3d::new(9.0, 0.0, 6.0),
        };
        let bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(18.0, 18.0, 12.0));
        let mut nodes = BTreeMap::new();
        insert_node(
            &mut nodes,
            SolidNode {
                id: SolidNodeId(1),
                logical_id: geometry,
                op: SolidOp::Revolve {
                    profile,
                    axis: Axis3d {
                        origin: Vec3d::ZERO,
                        direction: Vec3d::new(0.0, 0.0, 1.0),
                    },
                },
                bounds,
                feature_id: FeatureId(1),
                physical_region: RegionId(1),
            },
        );
        let mut profiles = BTreeMap::new();
        profiles.insert(
            profile,
            ProfileSummary {
                bounds: profile_bounds,
                segment_count: 4,
                closed: true,
            },
        );

        let mut solids = BTreeMap::new();
        solids.insert(
            geometry,
            SolidGraph {
                nodes,
                root: SolidNodeId(1),
                units: Units::Millimeter,
                tolerance: ManufacturingTolerance::default(),
                profiles,
                curves: BTreeMap::new(),
            },
        );
        let mut parts = BTreeMap::new();
        parts.insert(
            PartId(1),
            PartDefinition {
                id: PartId(1),
                geometry,
                root: SolidNodeId(1),
                appearance: Some(AppearanceMaterialId(1)),
                physical_material: Some(PhysicalMaterialId(1)),
                manufacturing_role: ManufacturingRole::PrintableSolid,
            },
        );
        SolidModelBundle {
            solids,
            assembly: AssemblyGraph {
                id: AssemblyId(1),
                parts,
                instances: vec![PartInstance {
                    id: PartInstanceId(1),
                    part: PartId(1),
                    transform: Mat4d::IDENTITY,
                    label: "Revolved ring".to_owned(),
                }],
                constraints: vec![AssemblyConstraint::Fixed {
                    instance: PartInstanceId(1),
                }],
            },
        }
    }

    pub fn lofted_rectangle_fixture() -> Self {
        let geometry = GeometryLogicalId(1);
        let bottom_profile = ProfileId(1);
        let top_profile = ProfileId(2);
        let bottom_bounds =
            Aabb64::from_center_size(Vec3d::new(0.0, 0.0, -6.0), Vec3d::new(36.0, 20.0, 0.0));
        let top_bounds =
            Aabb64::from_center_size(Vec3d::new(0.0, 0.0, 6.0), Vec3d::new(18.0, 10.0, 0.0));
        let bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(36.0, 20.0, 12.0));
        let mut nodes = BTreeMap::new();
        insert_node(
            &mut nodes,
            SolidNode {
                id: SolidNodeId(1),
                logical_id: geometry,
                op: SolidOp::Loft {
                    profiles: vec![bottom_profile, top_profile],
                },
                bounds,
                feature_id: FeatureId(1),
                physical_region: RegionId(1),
            },
        );
        let mut profiles = BTreeMap::new();
        profiles.insert(
            bottom_profile,
            ProfileSummary {
                bounds: bottom_bounds,
                segment_count: 4,
                closed: true,
            },
        );
        profiles.insert(
            top_profile,
            ProfileSummary {
                bounds: top_bounds,
                segment_count: 4,
                closed: true,
            },
        );

        let mut solids = BTreeMap::new();
        solids.insert(
            geometry,
            SolidGraph {
                nodes,
                root: SolidNodeId(1),
                units: Units::Millimeter,
                tolerance: ManufacturingTolerance::default(),
                profiles,
                curves: BTreeMap::new(),
            },
        );
        let mut parts = BTreeMap::new();
        parts.insert(
            PartId(1),
            PartDefinition {
                id: PartId(1),
                geometry,
                root: SolidNodeId(1),
                appearance: Some(AppearanceMaterialId(1)),
                physical_material: Some(PhysicalMaterialId(1)),
                manufacturing_role: ManufacturingRole::PrintableSolid,
            },
        );
        SolidModelBundle {
            solids,
            assembly: AssemblyGraph {
                id: AssemblyId(1),
                parts,
                instances: vec![PartInstance {
                    id: PartInstanceId(1),
                    part: PartId(1),
                    transform: Mat4d::IDENTITY,
                    label: "Lofted rectangle".to_owned(),
                }],
                constraints: vec![AssemblyConstraint::Fixed {
                    instance: PartInstanceId(1),
                }],
            },
        }
    }

    pub fn box_intersection_fixture() -> Self {
        box_intersection_fixture_with_curved_child(false)
    }

    pub fn curved_intersection_negative_fixture() -> Self {
        box_intersection_fixture_with_curved_child(true)
    }

    pub fn box_slot_difference_fixture() -> Self {
        box_slot_difference_fixture()
    }

    pub fn box_pocket_difference_fixture() -> Self {
        box_pocket_difference_fixture()
    }

    pub fn visual_only_fixture() -> Self {
        let mut bundle = printable_bracket_fixture_with_hole_diameter(6.4);
        for part in bundle.assembly.parts.values_mut() {
            part.manufacturing_role = ManufacturingRole::VisualOnly;
            part.physical_material = None;
        }
        bundle
    }

    pub fn curved_primitives_fixture() -> Self {
        let mut solids = BTreeMap::new();
        let mut parts = BTreeMap::new();
        let specs = [
            (
                GeometryLogicalId(1),
                PartId(1),
                SolidOp::Sphere { radius: 6.0 },
                Aabb64::from_center_size(Vec3d::new(-28.0, 0.0, 0.0), Vec3d::new(12.0, 12.0, 12.0)),
                "Sphere",
            ),
            (
                GeometryLogicalId(2),
                PartId(2),
                SolidOp::Cone {
                    radius0: 8.0,
                    radius1: 3.0,
                    height: 20.0,
                },
                Aabb64::from_center_size(Vec3d::new(0.0, 0.0, 0.0), Vec3d::new(16.0, 16.0, 20.0)),
                "Cone",
            ),
            (
                GeometryLogicalId(3),
                PartId(3),
                SolidOp::Torus {
                    major_radius: 12.0,
                    minor_radius: 2.0,
                },
                Aabb64::from_center_size(Vec3d::new(34.0, 0.0, 0.0), Vec3d::new(28.0, 28.0, 4.0)),
                "Torus",
            ),
        ];
        for (geometry, part, op, bounds, _label) in specs.iter() {
            solids.insert(
                *geometry,
                single_node_graph(
                    *geometry,
                    op.clone(),
                    *bounds,
                    FeatureId(part.0),
                    RegionId(part.0),
                ),
            );
            parts.insert(
                *part,
                PartDefinition {
                    id: *part,
                    geometry: *geometry,
                    root: SolidNodeId(1),
                    appearance: Some(AppearanceMaterialId(part.0)),
                    physical_material: Some(PhysicalMaterialId(1)),
                    manufacturing_role: ManufacturingRole::PrintableSolid,
                },
            );
        }
        let instances = specs
            .iter()
            .map(|(_geometry, part, _op, _bounds, label)| PartInstance {
                id: PartInstanceId(part.0),
                part: *part,
                transform: Mat4d::IDENTITY,
                label: (*label).to_owned(),
            })
            .collect();
        SolidModelBundle {
            solids,
            assembly: AssemblyGraph {
                id: AssemblyId(1),
                parts,
                instances,
                constraints: vec![AssemblyConstraint::Fixed {
                    instance: PartInstanceId(1),
                }],
            },
        }
    }

    pub fn unsupported_loft_fixture() -> Self {
        let geometry = GeometryLogicalId(1);
        let mut nodes = BTreeMap::new();
        insert_node(
            &mut nodes,
            SolidNode {
                id: SolidNodeId(1),
                logical_id: geometry,
                op: SolidOp::Loft {
                    profiles: Vec::new(),
                },
                bounds: Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(20.0, 20.0, 20.0)),
                feature_id: FeatureId(1),
                physical_region: RegionId(1),
            },
        );
        let mut solids = BTreeMap::new();
        solids.insert(
            geometry,
            SolidGraph {
                nodes,
                root: SolidNodeId(1),
                units: Units::Millimeter,
                tolerance: ManufacturingTolerance::default(),
                profiles: BTreeMap::new(),
                curves: BTreeMap::new(),
            },
        );
        let mut parts = BTreeMap::new();
        parts.insert(
            PartId(1),
            PartDefinition {
                id: PartId(1),
                geometry,
                root: SolidNodeId(1),
                appearance: Some(AppearanceMaterialId(1)),
                physical_material: Some(PhysicalMaterialId(1)),
                manufacturing_role: ManufacturingRole::PrintableSolid,
            },
        );
        SolidModelBundle {
            solids,
            assembly: AssemblyGraph {
                id: AssemblyId(1),
                parts,
                instances: vec![PartInstance {
                    id: PartInstanceId(1),
                    part: PartId(1),
                    transform: Mat4d::IDENTITY,
                    label: "Unsupported loft".to_owned(),
                }],
                constraints: vec![AssemblyConstraint::Fixed {
                    instance: PartInstanceId(1),
                }],
            },
        }
    }

    pub fn thin_shell_wall_thickness_negative_fixture() -> Self {
        let geometry = GeometryLogicalId(1);
        let mut nodes = BTreeMap::new();
        insert_node(
            &mut nodes,
            SolidNode {
                id: SolidNodeId(1),
                logical_id: geometry,
                op: SolidOp::Box {
                    size: Vec3d::new(20.0, 20.0, 20.0),
                },
                bounds: Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(20.0, 20.0, 20.0)),
                feature_id: FeatureId(1),
                physical_region: RegionId(1),
            },
        );
        insert_node(
            &mut nodes,
            SolidNode {
                id: SolidNodeId(2),
                logical_id: geometry,
                op: SolidOp::Shell {
                    child: SolidNodeId(1),
                    thickness: 0.2,
                },
                bounds: Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(20.0, 20.0, 20.0)),
                feature_id: FeatureId(2),
                physical_region: RegionId(1),
            },
        );
        let mut solids = BTreeMap::new();
        solids.insert(
            geometry,
            SolidGraph {
                nodes,
                root: SolidNodeId(2),
                units: Units::Millimeter,
                tolerance: ManufacturingTolerance::default(),
                profiles: BTreeMap::new(),
                curves: BTreeMap::new(),
            },
        );
        let mut parts = BTreeMap::new();
        parts.insert(
            PartId(1),
            PartDefinition {
                id: PartId(1),
                geometry,
                root: SolidNodeId(2),
                appearance: Some(AppearanceMaterialId(1)),
                physical_material: Some(PhysicalMaterialId(1)),
                manufacturing_role: ManufacturingRole::PrintableSolid,
            },
        );
        SolidModelBundle {
            solids,
            assembly: AssemblyGraph {
                id: AssemblyId(1),
                parts,
                instances: vec![PartInstance {
                    id: PartInstanceId(1),
                    part: PartId(1),
                    transform: Mat4d::IDENTITY,
                    label: "Thin shell".to_owned(),
                }],
                constraints: vec![AssemblyConstraint::Fixed {
                    instance: PartInstanceId(1),
                }],
            },
        }
    }

    pub fn material_region_conflict_fixture() -> Self {
        let mut bundle = printable_bracket_fixture_with_hole_diameter(6.4);
        let Some(base_part) = bundle.assembly.parts.get(&PartId(1)).cloned() else {
            return bundle;
        };
        let conflicting_part = PartDefinition {
            id: PartId(2),
            appearance: Some(AppearanceMaterialId(2)),
            physical_material: Some(PhysicalMaterialId(2)),
            ..base_part
        };
        bundle
            .assembly
            .parts
            .insert(conflicting_part.id, conflicting_part);
        bundle.assembly.instances.push(PartInstance {
            id: PartInstanceId(3),
            part: PartId(2),
            transform: Mat4d::IDENTITY,
            label: "Bracket conflicting material".to_owned(),
        });
        bundle
    }

    pub fn parametric_car_fixture() -> Self {
        let body_geometry = GeometryLogicalId(1);
        let wheel_geometry = GeometryLogicalId(2);
        let window_geometry = GeometryLogicalId(3);
        let mut solids = BTreeMap::new();
        solids.insert(
            body_geometry,
            single_node_graph(
                body_geometry,
                SolidOp::RoundedBox {
                    size: Vec3d::new(118.0, 38.0, 18.0),
                    radius: 3.0,
                },
                Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(118.0, 38.0, 18.0)),
                FeatureId(1),
                RegionId(1),
            ),
        );
        solids.insert(
            wheel_geometry,
            single_node_graph(
                wheel_geometry,
                SolidOp::Cylinder {
                    radius: 6.0,
                    height: 8.0,
                },
                Aabb64::from_cylinder_z(Vec3d::ZERO, 6.0, 8.0),
                FeatureId(2),
                RegionId(2),
            ),
        );
        solids.insert(
            window_geometry,
            single_node_graph(
                window_geometry,
                SolidOp::RoundedBox {
                    size: Vec3d::new(52.0, 32.0, 6.0),
                    radius: 2.0,
                },
                Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(52.0, 32.0, 6.0)),
                FeatureId(3),
                RegionId(3),
            ),
        );

        let mut parts = BTreeMap::new();
        parts.insert(
            PartId(1),
            PartDefinition {
                id: PartId(1),
                geometry: body_geometry,
                root: SolidNodeId(1),
                appearance: Some(AppearanceMaterialId(1)),
                physical_material: Some(PhysicalMaterialId(1)),
                manufacturing_role: ManufacturingRole::PrintableSolid,
            },
        );
        parts.insert(
            PartId(2),
            PartDefinition {
                id: PartId(2),
                geometry: wheel_geometry,
                root: SolidNodeId(1),
                appearance: Some(AppearanceMaterialId(2)),
                physical_material: Some(PhysicalMaterialId(2)),
                manufacturing_role: ManufacturingRole::PrintableSolid,
            },
        );
        parts.insert(
            PartId(3),
            PartDefinition {
                id: PartId(3),
                geometry: window_geometry,
                root: SolidNodeId(1),
                appearance: Some(AppearanceMaterialId(3)),
                physical_material: None,
                manufacturing_role: ManufacturingRole::VisualOnly,
            },
        );

        let wheel_positions = [
            (PartInstanceId(3), "Front-left wheel", -36.0, -30.0),
            (PartInstanceId(4), "Front-right wheel", -36.0, 30.0),
            (PartInstanceId(5), "Rear-left wheel", 36.0, -30.0),
            (PartInstanceId(6), "Rear-right wheel", 36.0, 30.0),
        ];
        let mut instances = vec![
            PartInstance {
                id: PartInstanceId(1),
                part: PartId(1),
                transform: Mat4d::IDENTITY,
                label: "Car body".to_owned(),
            },
            PartInstance {
                id: PartInstanceId(2),
                part: PartId(3),
                transform: Mat4d::translation(Vec3d::new(0.0, 0.0, 12.0)),
                label: "Visual-only windows".to_owned(),
            },
        ];
        instances.extend(
            wheel_positions
                .into_iter()
                .map(|(id, label, x, y)| PartInstance {
                    id,
                    part: PartId(2),
                    transform: Mat4d::translation(Vec3d::new(x, y, -6.0)),
                    label: label.to_owned(),
                }),
        );

        SolidModelBundle {
            solids,
            assembly: AssemblyGraph {
                id: AssemblyId(2),
                parts,
                instances,
                constraints: vec![AssemblyConstraint::Fixed {
                    instance: PartInstanceId(1),
                }],
            },
        }
    }

    pub fn validate(&self) -> SolidValidationReport {
        let mut report = SolidValidationReport::default();
        for graph in self.solids.values() {
            validate_graph(graph, &mut report);
        }
        validate_assembly(self, &mut report);
        if report.unresolved_reference_count > 0 || report.minimum_feature_violation_count > 0 {
            report.status = SolidValidationStatus::Fail;
        }
        report
    }

    pub fn metrics(&self) -> (SolidGraphMetrics, AssemblyMetrics) {
        let graph_metrics =
            self.solids
                .values()
                .fold(SolidGraphMetrics::default(), |mut total, graph| {
                    let metrics = graph.metrics();
                    total.node_count += metrics.node_count;
                    total.primitive_node_count += metrics.primitive_node_count;
                    total.boolean_node_count += metrics.boolean_node_count;
                    total.transform_node_count += metrics.transform_node_count;
                    total.subtractive_cylinder_count += metrics.subtractive_cylinder_count;
                    total.physical_region_count += metrics.physical_region_count;
                    total.max_depth = total.max_depth.max(metrics.max_depth);
                    total
                });
        (graph_metrics, self.assembly.metrics())
    }
}

impl SolidGraph {
    pub fn metrics(&self) -> SolidGraphMetrics {
        let mut metrics = SolidGraphMetrics {
            node_count: self.nodes.len(),
            max_depth: self.depth(self.root, &mut BTreeSet::new()),
            ..SolidGraphMetrics::default()
        };
        let mut physical_regions = BTreeSet::new();
        for node in self.nodes.values() {
            physical_regions.insert(node.physical_region);
            match node.op {
                SolidOp::Box { .. }
                | SolidOp::RoundedBox { .. }
                | SolidOp::Sphere { .. }
                | SolidOp::Cylinder { .. }
                | SolidOp::Cone { .. }
                | SolidOp::Torus { .. }
                | SolidOp::Extrude { .. }
                | SolidOp::Revolve { .. }
                | SolidOp::Sweep { .. }
                | SolidOp::Loft { .. }
                | SolidOp::Functional { .. }
                | SolidOp::ImportedSolid { .. } => {
                    metrics.primitive_node_count += 1;
                }
                SolidOp::Union { .. }
                | SolidOp::Intersection { .. }
                | SolidOp::Difference { .. }
                | SolidOp::SmoothUnion { .. } => {
                    metrics.boolean_node_count += 1;
                }
                SolidOp::Transform { .. } | SolidOp::Offset { .. } | SolidOp::Shell { .. } => {
                    metrics.transform_node_count += 1;
                }
            }
        }
        for node in self.nodes.values() {
            if let SolidOp::Difference { tools, .. } = &node.op {
                metrics.subtractive_cylinder_count += tools
                    .iter()
                    .filter(|tool| self.subtraction_tool_is_cylinder(**tool, &mut BTreeSet::new()))
                    .count();
            }
        }
        metrics.physical_region_count = physical_regions.len();
        metrics
    }

    fn depth(&self, id: SolidNodeId, seen: &mut BTreeSet<SolidNodeId>) -> usize {
        if !seen.insert(id) {
            return 0;
        }
        let Some(node) = self.nodes.get(&id) else {
            return 0;
        };
        let child_depth = match &node.op {
            SolidOp::Union { children } | SolidOp::Intersection { children } => children
                .iter()
                .map(|child| self.depth(*child, seen))
                .max()
                .unwrap_or(0),
            SolidOp::Difference { base, tools } => tools
                .iter()
                .chain(std::iter::once(base))
                .map(|child| self.depth(*child, seen))
                .max()
                .unwrap_or(0),
            SolidOp::Transform { child, .. }
            | SolidOp::Offset { child, .. }
            | SolidOp::Shell { child, .. } => self.depth(*child, seen),
            SolidOp::SmoothUnion { a, b, .. } => self.depth(*a, seen).max(self.depth(*b, seen)),
            _ => 0,
        };
        seen.remove(&id);
        child_depth + 1
    }

    fn subtraction_tool_is_cylinder(
        &self,
        id: SolidNodeId,
        seen: &mut BTreeSet<SolidNodeId>,
    ) -> bool {
        if !seen.insert(id) {
            return false;
        }
        let result = self.nodes.get(&id).is_some_and(|node| match node.op {
            SolidOp::Cylinder { .. } => true,
            SolidOp::Transform { child, .. }
            | SolidOp::Offset { child, .. }
            | SolidOp::Shell { child, .. } => self.subtraction_tool_is_cylinder(child, seen),
            _ => false,
        });
        seen.remove(&id);
        result
    }
}

impl AssemblyGraph {
    pub fn metrics(&self) -> AssemblyMetrics {
        let mut metrics = AssemblyMetrics {
            part_count: self.parts.len(),
            instance_count: self.instances.len(),
            ..AssemblyMetrics::default()
        };
        for part in self.parts.values() {
            match part.manufacturing_role {
                ManufacturingRole::PrintableSolid => metrics.printable_part_count += 1,
                ManufacturingRole::VisualOnly => metrics.visual_only_part_count += 1,
                ManufacturingRole::VoidModifier => metrics.void_modifier_part_count += 1,
                ManufacturingRole::SupportModifier
                | ManufacturingRole::InfillModifier
                | ManufacturingRole::Reference => {}
            }
            if part.physical_material.is_some() {
                metrics.physical_material_part_count += 1;
            }
        }
        let mut geometry_uses = BTreeMap::<GeometryLogicalId, usize>::new();
        for instance in &self.instances {
            if let Some(part) = self.parts.get(&instance.part) {
                *geometry_uses.entry(part.geometry).or_default() += 1;
            }
        }
        metrics.shared_geometry_instance_count = geometry_uses
            .values()
            .filter(|count| **count > 1)
            .map(|count| *count)
            .sum();
        metrics
    }
}

fn box_intersection_fixture_with_curved_child(curved_child: bool) -> SolidModelBundle {
    let geometry = GeometryLogicalId(70);
    let first_bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(40.0, 24.0, 8.0));
    let second_bounds =
        Aabb64::from_center_size(Vec3d::new(8.0, 0.0, 0.0), Vec3d::new(30.0, 30.0, 8.0));
    let root_bounds = Aabb64 {
        min: Vec3d::new(
            first_bounds.min.x.max(second_bounds.min.x),
            first_bounds.min.y.max(second_bounds.min.y),
            first_bounds.min.z.max(second_bounds.min.z),
        ),
        max: Vec3d::new(
            first_bounds.max.x.min(second_bounds.max.x),
            first_bounds.max.y.min(second_bounds.max.y),
            first_bounds.max.z.min(second_bounds.max.z),
        ),
    };

    let mut nodes = BTreeMap::new();
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(1),
            logical_id: geometry,
            op: SolidOp::Box {
                size: Vec3d::new(40.0, 24.0, 8.0),
            },
            bounds: first_bounds,
            feature_id: FeatureId(1),
            physical_region: RegionId(1),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(2),
            logical_id: geometry,
            op: if curved_child {
                SolidOp::Cylinder {
                    radius: 12.0,
                    height: 8.0,
                }
            } else {
                SolidOp::RoundedBox {
                    size: Vec3d::new(30.0, 30.0, 8.0),
                    radius: 2.0,
                }
            },
            bounds: second_bounds,
            feature_id: FeatureId(2),
            physical_region: RegionId(1),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(3),
            logical_id: geometry,
            op: SolidOp::Intersection {
                children: vec![SolidNodeId(1), SolidNodeId(2)],
            },
            bounds: root_bounds,
            feature_id: FeatureId(3),
            physical_region: RegionId(1),
        },
    );

    let mut solids = BTreeMap::new();
    solids.insert(
        geometry,
        SolidGraph {
            nodes,
            root: SolidNodeId(3),
            units: Units::Millimeter,
            tolerance: ManufacturingTolerance::default(),
            profiles: BTreeMap::new(),
            curves: BTreeMap::new(),
        },
    );
    let mut parts = BTreeMap::new();
    parts.insert(
        PartId(70),
        PartDefinition {
            id: PartId(70),
            geometry,
            root: SolidNodeId(3),
            appearance: Some(AppearanceMaterialId(70)),
            physical_material: Some(PhysicalMaterialId(70)),
            manufacturing_role: ManufacturingRole::PrintableSolid,
        },
    );

    SolidModelBundle {
        solids,
        assembly: AssemblyGraph {
            id: AssemblyId(70),
            parts,
            instances: vec![PartInstance {
                id: PartInstanceId(70),
                part: PartId(70),
                transform: Mat4d::IDENTITY,
                label: if curved_child {
                    "Unsupported curved intersection".to_owned()
                } else {
                    "Box intersection".to_owned()
                },
            }],
            constraints: vec![AssemblyConstraint::Fixed {
                instance: PartInstanceId(70),
            }],
        },
    }
}

fn box_slot_difference_fixture() -> SolidModelBundle {
    let geometry = GeometryLogicalId(71);
    let base_bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(42.0, 26.0, 8.0));
    let slot_bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(12.0, 8.0, 10.0));

    let mut nodes = BTreeMap::new();
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(1),
            logical_id: geometry,
            op: SolidOp::Box {
                size: Vec3d::new(42.0, 26.0, 8.0),
            },
            bounds: base_bounds,
            feature_id: FeatureId(1),
            physical_region: RegionId(1),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(2),
            logical_id: geometry,
            op: SolidOp::Box {
                size: Vec3d::new(12.0, 8.0, 10.0),
            },
            bounds: slot_bounds,
            feature_id: FeatureId(2),
            physical_region: RegionId(2),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(3),
            logical_id: geometry,
            op: SolidOp::Difference {
                base: SolidNodeId(1),
                tools: vec![SolidNodeId(2)],
            },
            bounds: base_bounds,
            feature_id: FeatureId(3),
            physical_region: RegionId(1),
        },
    );

    let mut solids = BTreeMap::new();
    solids.insert(
        geometry,
        SolidGraph {
            nodes,
            root: SolidNodeId(3),
            units: Units::Millimeter,
            tolerance: ManufacturingTolerance::default(),
            profiles: BTreeMap::new(),
            curves: BTreeMap::new(),
        },
    );
    let mut parts = BTreeMap::new();
    parts.insert(
        PartId(71),
        PartDefinition {
            id: PartId(71),
            geometry,
            root: SolidNodeId(3),
            appearance: Some(AppearanceMaterialId(71)),
            physical_material: Some(PhysicalMaterialId(71)),
            manufacturing_role: ManufacturingRole::PrintableSolid,
        },
    );

    SolidModelBundle {
        solids,
        assembly: AssemblyGraph {
            id: AssemblyId(71),
            parts,
            instances: vec![PartInstance {
                id: PartInstanceId(71),
                part: PartId(71),
                transform: Mat4d::IDENTITY,
                label: "Box slot difference".to_owned(),
            }],
            constraints: vec![AssemblyConstraint::Fixed {
                instance: PartInstanceId(71),
            }],
        },
    }
}

fn box_pocket_difference_fixture() -> SolidModelBundle {
    let geometry = GeometryLogicalId(72);
    let base_bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(42.0, 26.0, 8.0));
    let pocket_bounds =
        Aabb64::from_center_size(Vec3d::new(0.0, 0.0, 1.0), Vec3d::new(12.0, 8.0, 6.0));

    let mut nodes = BTreeMap::new();
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(1),
            logical_id: geometry,
            op: SolidOp::Box {
                size: Vec3d::new(42.0, 26.0, 8.0),
            },
            bounds: base_bounds,
            feature_id: FeatureId(1),
            physical_region: RegionId(1),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(2),
            logical_id: geometry,
            op: SolidOp::Box {
                size: Vec3d::new(12.0, 8.0, 6.0),
            },
            bounds: pocket_bounds,
            feature_id: FeatureId(2),
            physical_region: RegionId(2),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(3),
            logical_id: geometry,
            op: SolidOp::Difference {
                base: SolidNodeId(1),
                tools: vec![SolidNodeId(2)],
            },
            bounds: base_bounds,
            feature_id: FeatureId(3),
            physical_region: RegionId(1),
        },
    );

    let mut solids = BTreeMap::new();
    solids.insert(
        geometry,
        SolidGraph {
            nodes,
            root: SolidNodeId(3),
            units: Units::Millimeter,
            tolerance: ManufacturingTolerance::default(),
            profiles: BTreeMap::new(),
            curves: BTreeMap::new(),
        },
    );
    let mut parts = BTreeMap::new();
    parts.insert(
        PartId(72),
        PartDefinition {
            id: PartId(72),
            geometry,
            root: SolidNodeId(3),
            appearance: Some(AppearanceMaterialId(72)),
            physical_material: Some(PhysicalMaterialId(72)),
            manufacturing_role: ManufacturingRole::PrintableSolid,
        },
    );

    SolidModelBundle {
        solids,
        assembly: AssemblyGraph {
            id: AssemblyId(72),
            parts,
            instances: vec![PartInstance {
                id: PartInstanceId(72),
                part: PartId(72),
                transform: Mat4d::IDENTITY,
                label: "Box pocket difference".to_owned(),
            }],
            constraints: vec![AssemblyConstraint::Fixed {
                instance: PartInstanceId(72),
            }],
        },
    }
}

fn printable_bracket_fixture_with_hole_diameter(hole_diameter: f64) -> SolidModelBundle {
    let geometry = GeometryLogicalId(1);
    let base_bounds = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(70.0, 34.0, 6.0));
    let upright_local = Aabb64::from_center_size(Vec3d::ZERO, Vec3d::new(70.0, 7.0, 42.0));
    let upright_translation = Vec3d::new(0.0, -13.5, 18.0);
    let upright_bounds = upright_local.translate(upright_translation);
    let left_hole_bounds =
        Aabb64::from_cylinder_z(Vec3d::new(-23.0, 0.0, 0.0), hole_diameter * 0.5, 8.0);
    let right_hole_bounds =
        Aabb64::from_cylinder_z(Vec3d::new(23.0, 0.0, 0.0), hole_diameter * 0.5, 8.0);
    let union_bounds = base_bounds.union(upright_bounds);
    let root_bounds = union_bounds;

    let mut nodes = BTreeMap::new();
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(1),
            logical_id: geometry,
            op: SolidOp::RoundedBox {
                size: Vec3d::new(70.0, 34.0, 6.0),
                radius: 2.0,
            },
            bounds: base_bounds,
            feature_id: FeatureId(1),
            physical_region: RegionId(1),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(2),
            logical_id: geometry,
            op: SolidOp::RoundedBox {
                size: Vec3d::new(70.0, 7.0, 42.0),
                radius: 2.0,
            },
            bounds: upright_local,
            feature_id: FeatureId(2),
            physical_region: RegionId(1),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(3),
            logical_id: geometry,
            op: SolidOp::Transform {
                child: SolidNodeId(2),
                transform: Mat4d::translation(upright_translation),
            },
            bounds: upright_bounds,
            feature_id: FeatureId(2),
            physical_region: RegionId(1),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(4),
            logical_id: geometry,
            op: SolidOp::Union {
                children: vec![SolidNodeId(1), SolidNodeId(3)],
            },
            bounds: union_bounds,
            feature_id: FeatureId(3),
            physical_region: RegionId(1),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(5),
            logical_id: geometry,
            op: SolidOp::Cylinder {
                radius: hole_diameter * 0.5,
                height: 8.0,
            },
            bounds: left_hole_bounds,
            feature_id: FeatureId(4),
            physical_region: RegionId(2),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(6),
            logical_id: geometry,
            op: SolidOp::Cylinder {
                radius: hole_diameter * 0.5,
                height: 8.0,
            },
            bounds: right_hole_bounds,
            feature_id: FeatureId(5),
            physical_region: RegionId(3),
        },
    );
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(7),
            logical_id: geometry,
            op: SolidOp::Difference {
                base: SolidNodeId(4),
                tools: vec![SolidNodeId(5), SolidNodeId(6)],
            },
            bounds: root_bounds,
            feature_id: FeatureId(6),
            physical_region: RegionId(1),
        },
    );

    let graph = SolidGraph {
        nodes,
        root: SolidNodeId(7),
        units: Units::Millimeter,
        tolerance: ManufacturingTolerance::default(),
        profiles: BTreeMap::new(),
        curves: BTreeMap::new(),
    };
    let mut solids = BTreeMap::new();
    solids.insert(geometry, graph);

    let part = PartDefinition {
        id: PartId(1),
        geometry,
        root: SolidNodeId(7),
        appearance: Some(AppearanceMaterialId(1)),
        physical_material: Some(PhysicalMaterialId(1)),
        manufacturing_role: ManufacturingRole::PrintableSolid,
    };
    let mut parts = BTreeMap::new();
    parts.insert(part.id, part);
    let assembly = AssemblyGraph {
        id: AssemblyId(1),
        parts,
        instances: vec![
            PartInstance {
                id: PartInstanceId(1),
                part: PartId(1),
                transform: Mat4d::IDENTITY,
                label: "Bracket".to_owned(),
            },
            PartInstance {
                id: PartInstanceId(2),
                part: PartId(1),
                transform: Mat4d::translation(Vec3d::new(90.0, 0.0, 0.0)),
                label: "Bracket duplicate".to_owned(),
            },
        ],
        constraints: vec![AssemblyConstraint::Fixed {
            instance: PartInstanceId(1),
        }],
    };
    SolidModelBundle { solids, assembly }
}

fn insert_node(nodes: &mut BTreeMap<SolidNodeId, SolidNode>, node: SolidNode) {
    nodes.insert(node.id, node);
}

fn single_node_graph(
    geometry: GeometryLogicalId,
    op: SolidOp,
    bounds: Aabb64,
    feature_id: FeatureId,
    physical_region: RegionId,
) -> SolidGraph {
    let mut nodes = BTreeMap::new();
    insert_node(
        &mut nodes,
        SolidNode {
            id: SolidNodeId(1),
            logical_id: geometry,
            op,
            bounds,
            feature_id,
            physical_region,
        },
    );
    SolidGraph {
        nodes,
        root: SolidNodeId(1),
        units: Units::Millimeter,
        tolerance: ManufacturingTolerance::default(),
        profiles: BTreeMap::new(),
        curves: BTreeMap::new(),
    }
}

fn validate_graph(graph: &SolidGraph, report: &mut SolidValidationReport) {
    if !graph.nodes.contains_key(&graph.root) {
        push_diagnostic(
            report,
            "missing-root",
            Some(graph.root),
            "solid graph root is missing",
        );
    }
    for node in graph.nodes.values() {
        match &node.op {
            SolidOp::Union { children } | SolidOp::Intersection { children } => {
                for child in children {
                    check_ref(graph, report, *child, node.id);
                }
            }
            SolidOp::Difference { base, tools } => {
                check_ref(graph, report, *base, node.id);
                for tool in tools {
                    check_ref(graph, report, *tool, node.id);
                }
            }
            SolidOp::Transform { child, .. }
            | SolidOp::Offset { child, .. }
            | SolidOp::Shell { child, .. } => check_ref(graph, report, *child, node.id),
            SolidOp::SmoothUnion { a, b, .. } => {
                check_ref(graph, report, *a, node.id);
                check_ref(graph, report, *b, node.id);
            }
            SolidOp::Extrude { profile, .. } | SolidOp::Revolve { profile, .. } => {
                check_profile_ref(graph, report, *profile, node.id);
            }
            SolidOp::Sweep { profile, path } => {
                check_profile_ref(graph, report, *profile, node.id);
                check_curve_ref(graph, report, *path, node.id);
            }
            SolidOp::Loft { profiles } => {
                for profile in profiles {
                    check_profile_ref(graph, report, *profile, node.id);
                }
            }
            SolidOp::Cylinder { radius, .. } => {
                if radius * 2.0 < graph.tolerance.minimum_feature {
                    report.minimum_feature_violation_count += 1;
                    push_diagnostic(
                        report,
                        "minimum-feature",
                        Some(node.id),
                        format!(
                            "cylinder diameter {} is below minimum feature {}",
                            radius * 2.0,
                            graph.tolerance.minimum_feature
                        ),
                    );
                }
            }
            _ => {}
        }
    }
}

fn validate_assembly(bundle: &SolidModelBundle, report: &mut SolidValidationReport) {
    for part in bundle.assembly.parts.values() {
        let graph = bundle.solids.get(&part.geometry);
        if graph.is_none() {
            push_diagnostic(
                report,
                "missing-part-geometry",
                Some(part.root),
                format!("part {:?} references missing solid graph", part.id),
            );
            continue;
        }
        if graph.is_some_and(|graph| !graph.nodes.contains_key(&part.root)) {
            push_diagnostic(
                report,
                "missing-part-root",
                Some(part.root),
                format!("part {:?} references missing solid root", part.id),
            );
        }
        if part.manufacturing_role == ManufacturingRole::PrintableSolid {
            report.printable_closed_region_count += 1;
            if part.physical_material.is_none() {
                push_diagnostic(
                    report,
                    "missing-physical-material",
                    Some(part.root),
                    format!("printable part {:?} lacks a physical material", part.id),
                );
            }
        }
    }
    for instance in &bundle.assembly.instances {
        if !bundle.assembly.parts.contains_key(&instance.part) {
            push_diagnostic(
                report,
                "missing-instance-part",
                None,
                format!("instance {:?} references missing part", instance.id),
            );
        }
    }
}

fn check_profile_ref(
    graph: &SolidGraph,
    report: &mut SolidValidationReport,
    referenced: ProfileId,
    owner: SolidNodeId,
) {
    if !graph.profiles.contains_key(&referenced) {
        report.unresolved_reference_count += 1;
        push_diagnostic(
            report,
            "missing-profile",
            Some(owner),
            format!(
                "node {:?} references missing profile {:?}",
                owner, referenced
            ),
        );
    }
}

fn check_curve_ref(
    graph: &SolidGraph,
    report: &mut SolidValidationReport,
    referenced: CurveId,
    owner: SolidNodeId,
) {
    if !graph.curves.contains_key(&referenced) {
        report.unresolved_reference_count += 1;
        push_diagnostic(
            report,
            "missing-curve",
            Some(owner),
            format!("node {:?} references missing curve {:?}", owner, referenced),
        );
    }
}

fn check_ref(
    graph: &SolidGraph,
    report: &mut SolidValidationReport,
    referenced: SolidNodeId,
    owner: SolidNodeId,
) {
    if !graph.nodes.contains_key(&referenced) {
        report.unresolved_reference_count += 1;
        push_diagnostic(
            report,
            "missing-solid-node",
            Some(owner),
            format!("node {:?} references missing child {:?}", owner, referenced),
        );
    }
}

fn push_diagnostic(
    report: &mut SolidValidationReport,
    code: impl Into<String>,
    node: Option<SolidNodeId>,
    message: impl Into<String>,
) {
    report.diagnostics.push(SolidDiagnostic {
        code: code.into(),
        node,
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_bracket_fixture_keeps_holes_material_and_role_identity() {
        let bundle = SolidModelBundle::printable_bracket_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(validation.printable_closed_region_count, 1);
        assert_eq!(validation.minimum_feature_violation_count, 0);
        assert_eq!(validation.visual_mesh_used_for_manufacturing, false);
        assert_eq!(solid_metrics.subtractive_cylinder_count, 2);
        assert_eq!(solid_metrics.boolean_node_count, 2);
        assert_eq!(assembly_metrics.printable_part_count, 1);
        assert_eq!(assembly_metrics.physical_material_part_count, 1);
    }

    #[test]
    fn repeated_part_instances_share_one_solid_graph() {
        let bundle = SolidModelBundle::printable_bracket_fixture();
        let (_solid_metrics, assembly_metrics) = bundle.metrics();

        assert_eq!(bundle.solids.len(), 1);
        assert_eq!(assembly_metrics.instance_count, 2);
        assert_eq!(assembly_metrics.shared_geometry_instance_count, 2);
    }

    #[test]
    fn below_minimum_feature_reports_diagnostic_without_erasing_feature() {
        let bundle = SolidModelBundle::minimum_feature_negative_fixture();
        let (solid_metrics, _assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Fail);
        assert_eq!(validation.minimum_feature_violation_count, 2);
        assert_eq!(solid_metrics.subtractive_cylinder_count, 2);
        assert!(
            validation
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "minimum-feature")
        );
    }

    #[test]
    fn box_slot_difference_fixture_keeps_boolean_features() {
        let bundle = SolidModelBundle::box_slot_difference_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(validation.printable_closed_region_count, 1);
        assert_eq!(solid_metrics.primitive_node_count, 2);
        assert_eq!(solid_metrics.boolean_node_count, 1);
        assert_eq!(assembly_metrics.printable_part_count, 1);
        assert_eq!(assembly_metrics.physical_material_part_count, 1);
    }

    #[test]
    fn box_pocket_difference_fixture_keeps_boolean_features() {
        let bundle = SolidModelBundle::box_pocket_difference_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(validation.printable_closed_region_count, 1);
        assert_eq!(solid_metrics.primitive_node_count, 2);
        assert_eq!(solid_metrics.boolean_node_count, 1);
        assert_eq!(assembly_metrics.printable_part_count, 1);
        assert_eq!(assembly_metrics.physical_material_part_count, 1);
    }

    #[test]
    fn visual_only_fixture_is_not_printable_material() {
        let bundle = SolidModelBundle::visual_only_fixture();
        let (_solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(validation.printable_closed_region_count, 0);
        assert_eq!(assembly_metrics.printable_part_count, 0);
        assert_eq!(assembly_metrics.visual_only_part_count, 1);
    }

    #[test]
    fn shell_box_fixture_keeps_shell_node_printable() {
        let bundle = SolidModelBundle::shell_box_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(validation.printable_closed_region_count, 1);
        assert_eq!(validation.visual_mesh_used_for_manufacturing, false);
        assert_eq!(solid_metrics.primitive_node_count, 1);
        assert_eq!(solid_metrics.transform_node_count, 1);
        assert_eq!(assembly_metrics.printable_part_count, 1);
        assert_eq!(assembly_metrics.instance_count, 1);
    }

    #[test]
    fn extruded_rectangle_fixture_keeps_profile_reference_printable() {
        let bundle = SolidModelBundle::extruded_rectangle_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();
        let graph = bundle
            .solids
            .values()
            .next()
            .expect("fixture should contain a solid graph");

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(validation.printable_closed_region_count, 1);
        assert_eq!(validation.visual_mesh_used_for_manufacturing, false);
        assert_eq!(solid_metrics.primitive_node_count, 1);
        assert_eq!(graph.profiles.len(), 1);
        assert_eq!(assembly_metrics.printable_part_count, 1);
        assert_eq!(assembly_metrics.instance_count, 1);
    }

    #[test]
    fn revolved_ring_fixture_keeps_profile_reference_printable() {
        let bundle = SolidModelBundle::revolved_ring_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();
        let graph = bundle
            .solids
            .values()
            .next()
            .expect("fixture should contain a solid graph");

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(validation.printable_closed_region_count, 1);
        assert_eq!(validation.visual_mesh_used_for_manufacturing, false);
        assert_eq!(solid_metrics.primitive_node_count, 1);
        assert_eq!(graph.profiles.len(), 1);
        assert_eq!(assembly_metrics.printable_part_count, 1);
        assert_eq!(assembly_metrics.instance_count, 1);
    }

    #[test]
    fn lofted_rectangle_fixture_keeps_two_profile_references_printable() {
        let bundle = SolidModelBundle::lofted_rectangle_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();
        let graph = bundle
            .solids
            .values()
            .next()
            .expect("fixture should contain a solid graph");

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(validation.printable_closed_region_count, 1);
        assert_eq!(validation.visual_mesh_used_for_manufacturing, false);
        assert_eq!(solid_metrics.primitive_node_count, 1);
        assert_eq!(graph.profiles.len(), 2);
        assert_eq!(assembly_metrics.printable_part_count, 1);
        assert_eq!(assembly_metrics.instance_count, 1);
    }

    #[test]
    fn curved_primitives_fixture_keeps_printable_supported_roots() {
        let bundle = SolidModelBundle::curved_primitives_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(solid_metrics.primitive_node_count, 3);
        assert_eq!(assembly_metrics.printable_part_count, 3);
        assert_eq!(assembly_metrics.instance_count, 3);
    }

    #[test]
    fn unsupported_loft_fixture_validates_but_requires_manufacturing_diagnostic() {
        let bundle = SolidModelBundle::unsupported_loft_fixture();
        let (solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(solid_metrics.primitive_node_count, 1);
        assert_eq!(assembly_metrics.printable_part_count, 1);
    }

    #[test]
    fn material_region_conflict_fixture_keeps_overlapping_printable_materials() {
        let bundle = SolidModelBundle::material_region_conflict_fixture();
        let (_solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(assembly_metrics.printable_part_count, 2);
        assert_eq!(assembly_metrics.physical_material_part_count, 2);
        assert_eq!(assembly_metrics.instance_count, 3);
        assert_eq!(assembly_metrics.shared_geometry_instance_count, 3);
    }

    #[test]
    fn parametric_car_fixture_keeps_shared_wheel_identity_and_visual_windows() {
        let bundle = SolidModelBundle::parametric_car_fixture();
        let (_solid_metrics, assembly_metrics) = bundle.metrics();
        let validation = bundle.validate();

        assert_eq!(validation.status, SolidValidationStatus::Pass);
        assert_eq!(assembly_metrics.part_count, 3);
        assert_eq!(assembly_metrics.instance_count, 6);
        assert_eq!(assembly_metrics.printable_part_count, 2);
        assert_eq!(assembly_metrics.visual_only_part_count, 1);
        assert_eq!(assembly_metrics.physical_material_part_count, 2);
        assert_eq!(assembly_metrics.shared_geometry_instance_count, 4);
    }
}
