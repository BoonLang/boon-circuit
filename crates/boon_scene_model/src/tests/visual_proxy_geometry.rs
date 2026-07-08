#[test]
fn solid_model_visual_proxy_preserves_shared_geometry_and_identity() {
    let bundle = boon_solid_model::SolidModelBundle::printable_bracket_fixture();
    let (scene, report) = WorldScene::visual_proxy_from_solid_model(&bundle)
        .expect("solid fixture should compile to visual proxy scene");
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("solid fixture should compile to retained visual chunks");
    let metrics = scene.metrics();

    assert_eq!(report.input_solid_graph_count, 1);
    assert_eq!(report.input_instance_count, 2);
    assert_eq!(report.visual_geometry_count, 1);
    assert_eq!(report.visual_instance_count, 2);
    assert_eq!(report.proxy_mesh_count, 1);
    assert_eq!(report.retained_chunk_count, 1);
    assert_eq!(report.retained_chunk_id_count, 1);
    assert_eq!(report.exact_mesh_count, 0);
    assert_eq!(report.proxy_bounds_chunk_count, 0);
    assert_eq!(report.csg_subset_chunk_count, 1);
    assert_eq!(report.generated_mesh_count, 1);
    assert!(report.generated_vertex_count > 8);
    assert!(report.generated_index_count > 36);
    assert_eq!(report.adaptive_chunk_count, 0);
    assert!(!report.manufacturing_mesh_used);
    assert_eq!(
        report.visual_compiler_status,
        "retained-csg-subset-composite-mesh-no-full-csg"
    );
    assert_eq!(metrics.geometry_count, 1);
    assert_eq!(metrics.instance_count, 2);
    assert_eq!(metrics.shared_geometry_instance_count, 2);
    assert_eq!(visual.chunks.len(), 1);
    assert_eq!(visual.chunks[0].id.geometry, GeometryLogicalId(1));
    assert_eq!(visual.chunks[0].id.spatial_key, "root");
    assert_eq!(visual.chunks[0].id.tolerance_class, "csg-subset-composite");
    assert_eq!(visual.chunks[0].geometry_revision, GeometryRevision(1));
    assert!(!visual.chunks[0].source_features.is_empty());
    match &visual.chunks[0].representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert!(mesh.vertices.len() > 8);
            assert!(mesh.indices.len() > 36);
        }
        other => panic!("expected retained indexed mesh payload, got {other:?}"),
    }

    let first_pick = scene.pick_target(PickId(1)).expect("first pick target");
    assert_eq!(first_pick.geometry, GeometryLogicalId(1));
    assert_eq!(first_pick.part_id, PartId(1));
    assert!(first_pick.semantic_id.is_some());
}

#[test]
fn box_intersection_visual_proxy_uses_retained_csg_subset_mesh() {
    let bundle = boon_solid_model::SolidModelBundle::box_intersection_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("box intersection should compile to retained visual chunks");
    let chunk = visual
        .chunks
        .first()
        .expect("retained box-intersection chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-csg-subset-composite-mesh-no-full-csg"
    );
    assert_eq!(visual.report.retained_chunk_count, 1);
    assert_eq!(visual.report.csg_subset_chunk_count, 1);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(visual.report.exact_mesh_count, 0);
    assert!(!visual.report.manufacturing_mesh_used);
    assert_eq!(chunk.id.geometry, GeometryLogicalId(70));
    assert_eq!(chunk.id.tolerance_class, "csg-subset-composite");
    assert!(chunk.error_bound > 0.0);
    assert!(chunk.source_features.contains(&FeatureId(3)));
    match &chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 8);
            assert_eq!(mesh.indices.len(), 36);
        }
        other => panic!("expected retained indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn curved_intersection_visual_proxy_stays_proxy_until_general_csg_exists() {
    let bundle = boon_solid_model::SolidModelBundle::curved_intersection_negative_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("unsupported curved intersection should still have proxy bounds");
    let chunk = visual
        .chunks
        .first()
        .expect("retained curved-intersection proxy chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-generated-bounds-mesh-no-csg"
    );
    assert_eq!(visual.report.retained_chunk_count, 1);
    assert_eq!(visual.report.csg_subset_chunk_count, 0);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 1);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(chunk.id.tolerance_class, "proxy-bounds");
    assert!(chunk.error_bound > 0.0);
    assert!(!visual.report.manufacturing_mesh_used);
}

#[test]
fn box_slot_difference_visual_proxy_uses_rectangular_hole_csg_subset_mesh() {
    let bundle = boon_solid_model::SolidModelBundle::box_slot_difference_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("box slot difference should compile to retained visual chunks");
    let chunk = visual
        .chunks
        .first()
        .expect("retained box-slot-difference chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-csg-subset-composite-mesh-no-full-csg"
    );
    assert_eq!(visual.report.retained_chunk_count, 1);
    assert_eq!(visual.report.csg_subset_chunk_count, 1);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(visual.report.exact_mesh_count, 0);
    assert!(!visual.report.manufacturing_mesh_used);
    assert_eq!(chunk.id.geometry, GeometryLogicalId(71));
    assert_eq!(chunk.id.tolerance_class, "csg-subset-composite");
    assert!(chunk.source_features.contains(&FeatureId(1)));
    assert!(chunk.source_features.contains(&FeatureId(2)));
    assert!(chunk.source_features.contains(&FeatureId(3)));
    match &chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert!(mesh.vertices.len() > 8);
            assert!(mesh.indices.len() > 36);
        }
        other => panic!("expected retained indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn parametric_car_visual_proxy_uses_exact_mesh_for_shared_wheel_cylinder() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let wheel_instance = visual
        .scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let wheel_geometry = visual.scene.instances[&wheel_instance].geometry;
    let wheel_chunk = visual
        .chunks
        .iter()
        .find(|chunk| chunk.id.geometry == wheel_geometry)
        .expect("retained wheel chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-mixed-exact-and-adaptive-mesh-no-csg"
    );
    assert_eq!(visual.report.exact_mesh_count, 1);
    assert_eq!(visual.report.adaptive_chunk_count, 2);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 3);
    assert_eq!(wheel_chunk.id.tolerance_class, "exact-primitive");
    assert_eq!(wheel_chunk.error_bound, 0.0);
    match &wheel_chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 66);
            assert_eq!(mesh.indices.len(), 384);
        }
        other => panic!("expected retained exact indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn revolved_ring_visual_proxy_uses_exact_retained_annulus_mesh() {
    let bundle = boon_solid_model::SolidModelBundle::revolved_ring_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("revolved ring should compile to visual proxy scene");
    let chunk = visual.chunks.first().expect("retained revolve chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-exact-primitive-mesh-no-csg"
    );
    assert_eq!(visual.report.exact_mesh_count, 1);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(chunk.id.tolerance_class, "exact-primitive");
    assert_eq!(chunk.error_bound, 0.0);
    match &chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 128);
            assert_eq!(mesh.indices.len(), 768);
        }
        other => panic!("expected retained exact indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn lofted_rectangle_visual_proxy_uses_exact_retained_tapered_mesh() {
    let bundle = boon_solid_model::SolidModelBundle::lofted_rectangle_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("lofted rectangle should compile to visual proxy scene");
    let chunk = visual.chunks.first().expect("retained loft chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-exact-primitive-mesh-no-csg"
    );
    assert_eq!(visual.report.exact_mesh_count, 1);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(chunk.id.tolerance_class, "exact-primitive");
    assert_eq!(chunk.error_bound, 0.0);
    match &chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 8);
            assert_eq!(mesh.indices.len(), 36);
            assert!(mesh.vertices[0].position[0].abs() > mesh.vertices[4].position[0].abs());
        }
        other => panic!("expected retained exact indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn extruded_rectangle_visual_proxy_uses_exact_retained_prism_mesh() {
    let bundle = boon_solid_model::SolidModelBundle::extruded_rectangle_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("extruded rectangle should compile to visual proxy scene");
    let chunk = visual.chunks.first().expect("retained extrude chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-exact-primitive-mesh-no-csg"
    );
    assert_eq!(visual.report.exact_mesh_count, 1);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(chunk.id.tolerance_class, "exact-primitive");
    assert_eq!(chunk.error_bound, 0.0);
    match &chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 8);
            assert_eq!(mesh.indices.len(), 36);
            let min_z = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(f32::INFINITY, f32::min);
            let max_z = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!(min_z, -6.0);
            assert_eq!(max_z, 6.0);
        }
        other => panic!("expected retained exact indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn box_like_shell_visual_proxy_uses_exact_retained_hollow_mesh() {
    let bundle = boon_solid_model::SolidModelBundle::shell_box_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("shell box should compile to visual proxy scene");
    let chunk = visual.chunks.first().expect("retained shell chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-exact-primitive-mesh-no-csg"
    );
    assert_eq!(visual.report.exact_mesh_count, 1);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(chunk.id.tolerance_class, "exact-primitive");
    assert_eq!(chunk.error_bound, 0.0);
    assert!(chunk.source_features.contains(&FeatureId(1)));
    assert!(chunk.source_features.contains(&FeatureId(2)));
    match &chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 48);
            assert_eq!(mesh.indices.len(), 72);
            let min_abs_x = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[0].abs())
                .fold(f32::INFINITY, f32::min);
            let max_abs_x = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[0].abs())
                .fold(0.0, f32::max);
            assert_eq!(min_abs_x, 18.0);
            assert_eq!(max_abs_x, 20.0);
            assert!(
                mesh.vertices
                    .iter()
                    .any(|vertex| vertex.position[0] == 18.0 && vertex.normal[0] == -1.0)
            );
        }
        other => panic!("expected retained exact indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn rounded_box_visual_proxy_uses_adaptive_retained_mesh_not_exact_claim() {
    let geometry = boon_solid_model::GeometryLogicalId(1);
    let bundle = boon_solid_model::SolidModelBundle {
        solids: [(
            geometry,
            boon_solid_model::SolidGraph {
                nodes: [(
                    boon_solid_model::SolidNodeId(1),
                    boon_solid_model::SolidNode {
                        id: boon_solid_model::SolidNodeId(1),
                        logical_id: geometry,
                        op: boon_solid_model::SolidOp::RoundedBox {
                            size: boon_solid_model::Vec3d::new(20.0, 12.0, 6.0),
                            radius: 2.0,
                        },
                        bounds: boon_solid_model::Aabb64::from_center_size(
                            boon_solid_model::Vec3d::ZERO,
                            boon_solid_model::Vec3d::new(20.0, 12.0, 6.0),
                        ),
                        feature_id: boon_solid_model::FeatureId(1),
                        physical_region: boon_solid_model::RegionId(1),
                    },
                )]
                .into_iter()
                .collect(),
                root: boon_solid_model::SolidNodeId(1),
                units: boon_solid_model::Units::Millimeter,
                tolerance: boon_solid_model::ManufacturingTolerance::default(),
                profiles: std::collections::BTreeMap::new(),
                curves: std::collections::BTreeMap::new(),
            },
        )]
        .into_iter()
        .collect(),
        assembly: boon_solid_model::AssemblyGraph {
            id: boon_solid_model::AssemblyId(1),
            parts: [(
                boon_solid_model::PartId(1),
                boon_solid_model::PartDefinition {
                    id: boon_solid_model::PartId(1),
                    geometry,
                    root: boon_solid_model::SolidNodeId(1),
                    appearance: Some(boon_solid_model::AppearanceMaterialId(1)),
                    physical_material: Some(boon_solid_model::PhysicalMaterialId(1)),
                    manufacturing_role: boon_solid_model::ManufacturingRole::PrintableSolid,
                },
            )]
            .into_iter()
            .collect(),
            instances: vec![boon_solid_model::PartInstance {
                id: boon_solid_model::PartInstanceId(1),
                part: boon_solid_model::PartId(1),
                transform: boon_solid_model::Mat4d::IDENTITY,
                label: "Rounded box".to_owned(),
            }],
            constraints: Vec::new(),
        },
    };
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("rounded box should compile to adaptive visual proxy scene");
    let chunk = visual.chunks.first().expect("retained rounded-box chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-adaptive-rounded-box-mesh-no-csg"
    );
    assert_eq!(visual.report.exact_mesh_count, 0);
    assert_eq!(visual.report.adaptive_chunk_count, 1);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(chunk.id.tolerance_class, "adaptive-rounded-box");
    assert!(chunk.error_bound > 0.0);
    match &chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 52);
            assert_eq!(mesh.indices.len(), 300);
        }
        other => panic!("expected retained adaptive indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn translated_rounded_box_visual_proxy_preserves_adaptive_mesh_quality() {
    let geometry = boon_solid_model::GeometryLogicalId(1);
    let size = boon_solid_model::Vec3d::new(20.0, 12.0, 6.0);
    let translation = boon_solid_model::Vec3d::new(4.0, 5.0, 6.0);
    let child_bounds =
        boon_solid_model::Aabb64::from_center_size(boon_solid_model::Vec3d::ZERO, size);
    let translated_bounds = child_bounds.translate(translation);
    let bundle = boon_solid_model::SolidModelBundle {
        solids: [(
            geometry,
            boon_solid_model::SolidGraph {
                nodes: [
                    (
                        boon_solid_model::SolidNodeId(1),
                        boon_solid_model::SolidNode {
                            id: boon_solid_model::SolidNodeId(1),
                            logical_id: geometry,
                            op: boon_solid_model::SolidOp::RoundedBox { size, radius: 2.0 },
                            bounds: child_bounds,
                            feature_id: boon_solid_model::FeatureId(1),
                            physical_region: boon_solid_model::RegionId(1),
                        },
                    ),
                    (
                        boon_solid_model::SolidNodeId(2),
                        boon_solid_model::SolidNode {
                            id: boon_solid_model::SolidNodeId(2),
                            logical_id: geometry,
                            op: boon_solid_model::SolidOp::Transform {
                                child: boon_solid_model::SolidNodeId(1),
                                transform: boon_solid_model::Mat4d::translation(translation),
                            },
                            bounds: translated_bounds,
                            feature_id: boon_solid_model::FeatureId(2),
                            physical_region: boon_solid_model::RegionId(2),
                        },
                    ),
                ]
                .into_iter()
                .collect(),
                root: boon_solid_model::SolidNodeId(2),
                units: boon_solid_model::Units::Millimeter,
                tolerance: boon_solid_model::ManufacturingTolerance::default(),
                profiles: std::collections::BTreeMap::new(),
                curves: std::collections::BTreeMap::new(),
            },
        )]
        .into_iter()
        .collect(),
        assembly: boon_solid_model::AssemblyGraph {
            id: boon_solid_model::AssemblyId(1),
            parts: [(
                boon_solid_model::PartId(1),
                boon_solid_model::PartDefinition {
                    id: boon_solid_model::PartId(1),
                    geometry,
                    root: boon_solid_model::SolidNodeId(2),
                    appearance: Some(boon_solid_model::AppearanceMaterialId(1)),
                    physical_material: Some(boon_solid_model::PhysicalMaterialId(1)),
                    manufacturing_role: boon_solid_model::ManufacturingRole::PrintableSolid,
                },
            )]
            .into_iter()
            .collect(),
            instances: vec![boon_solid_model::PartInstance {
                id: boon_solid_model::PartInstanceId(1),
                part: boon_solid_model::PartId(1),
                transform: boon_solid_model::Mat4d::IDENTITY,
                label: "Translated rounded box".to_owned(),
            }],
            constraints: Vec::new(),
        },
    };
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("translated rounded box should compile to adaptive visual proxy scene");
    let chunk = visual
        .chunks
        .first()
        .expect("retained translated rounded-box chunk");

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-adaptive-rounded-box-mesh-no-csg"
    );
    assert_eq!(visual.report.exact_mesh_count, 0);
    assert_eq!(visual.report.adaptive_chunk_count, 1);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert_eq!(visual.report.generated_mesh_count, 1);
    assert_eq!(chunk.id.tolerance_class, "adaptive-rounded-box");
    assert!(chunk.error_bound > 0.0);
    match &chunk.representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 52);
            assert_eq!(mesh.indices.len(), 300);
            let min_x = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(f32::INFINITY, f32::min);
            let max_x = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(f32::NEG_INFINITY, f32::max);
            let min_z = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(f32::INFINITY, f32::min);
            let max_z = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!(min_x, -6.0);
            assert_eq!(max_x, 14.0);
            assert_eq!(min_z, 3.0);
            assert_eq!(max_z, 9.0);
        }
        other => panic!("expected retained adaptive indexed mesh payload, got {other:?}"),
    }
}

#[test]
fn curved_solid_visual_proxy_uses_exact_meshes_for_sphere_cone_and_torus() {
    let cone_geometry = boon_solid_model::GeometryLogicalId(1);
    let torus_geometry = boon_solid_model::GeometryLogicalId(2);
    let sphere_geometry = boon_solid_model::GeometryLogicalId(3);
    let mut solids = std::collections::BTreeMap::new();
    solids.insert(
        cone_geometry,
        boon_solid_model::SolidGraph {
            nodes: [(
                boon_solid_model::SolidNodeId(1),
                boon_solid_model::SolidNode {
                    id: boon_solid_model::SolidNodeId(1),
                    logical_id: cone_geometry,
                    op: boon_solid_model::SolidOp::Cone {
                        radius0: 8.0,
                        radius1: 3.0,
                        height: 20.0,
                    },
                    bounds: boon_solid_model::Aabb64::from_cylinder_z(
                        boon_solid_model::Vec3d::ZERO,
                        8.0,
                        20.0,
                    ),
                    feature_id: boon_solid_model::FeatureId(1),
                    physical_region: boon_solid_model::RegionId(1),
                },
            )]
            .into_iter()
            .collect(),
            root: boon_solid_model::SolidNodeId(1),
            units: boon_solid_model::Units::Millimeter,
            tolerance: boon_solid_model::ManufacturingTolerance::default(),
            profiles: std::collections::BTreeMap::new(),
            curves: std::collections::BTreeMap::new(),
        },
    );
    solids.insert(
        torus_geometry,
        boon_solid_model::SolidGraph {
            nodes: [(
                boon_solid_model::SolidNodeId(1),
                boon_solid_model::SolidNode {
                    id: boon_solid_model::SolidNodeId(1),
                    logical_id: torus_geometry,
                    op: boon_solid_model::SolidOp::Torus {
                        major_radius: 12.0,
                        minor_radius: 2.0,
                    },
                    bounds: boon_solid_model::Aabb64::from_center_size(
                        boon_solid_model::Vec3d::ZERO,
                        boon_solid_model::Vec3d::new(28.0, 28.0, 4.0),
                    ),
                    feature_id: boon_solid_model::FeatureId(2),
                    physical_region: boon_solid_model::RegionId(2),
                },
            )]
            .into_iter()
            .collect(),
            root: boon_solid_model::SolidNodeId(1),
            units: boon_solid_model::Units::Millimeter,
            tolerance: boon_solid_model::ManufacturingTolerance::default(),
            profiles: std::collections::BTreeMap::new(),
            curves: std::collections::BTreeMap::new(),
        },
    );
    solids.insert(
        sphere_geometry,
        boon_solid_model::SolidGraph {
            nodes: [(
                boon_solid_model::SolidNodeId(1),
                boon_solid_model::SolidNode {
                    id: boon_solid_model::SolidNodeId(1),
                    logical_id: sphere_geometry,
                    op: boon_solid_model::SolidOp::Sphere { radius: 6.0 },
                    bounds: boon_solid_model::Aabb64::from_center_size(
                        boon_solid_model::Vec3d::ZERO,
                        boon_solid_model::Vec3d::new(12.0, 12.0, 12.0),
                    ),
                    feature_id: boon_solid_model::FeatureId(3),
                    physical_region: boon_solid_model::RegionId(3),
                },
            )]
            .into_iter()
            .collect(),
            root: boon_solid_model::SolidNodeId(1),
            units: boon_solid_model::Units::Millimeter,
            tolerance: boon_solid_model::ManufacturingTolerance::default(),
            profiles: std::collections::BTreeMap::new(),
            curves: std::collections::BTreeMap::new(),
        },
    );
    let bundle = boon_solid_model::SolidModelBundle {
        solids,
        assembly: boon_solid_model::AssemblyGraph {
            id: boon_solid_model::AssemblyId(1),
            parts: [
                (
                    boon_solid_model::PartId(1),
                    boon_solid_model::PartDefinition {
                        id: boon_solid_model::PartId(1),
                        geometry: cone_geometry,
                        root: boon_solid_model::SolidNodeId(1),
                        appearance: Some(boon_solid_model::AppearanceMaterialId(1)),
                        physical_material: Some(boon_solid_model::PhysicalMaterialId(1)),
                        manufacturing_role: boon_solid_model::ManufacturingRole::PrintableSolid,
                    },
                ),
                (
                    boon_solid_model::PartId(2),
                    boon_solid_model::PartDefinition {
                        id: boon_solid_model::PartId(2),
                        geometry: torus_geometry,
                        root: boon_solid_model::SolidNodeId(1),
                        appearance: Some(boon_solid_model::AppearanceMaterialId(2)),
                        physical_material: Some(boon_solid_model::PhysicalMaterialId(1)),
                        manufacturing_role: boon_solid_model::ManufacturingRole::PrintableSolid,
                    },
                ),
                (
                    boon_solid_model::PartId(3),
                    boon_solid_model::PartDefinition {
                        id: boon_solid_model::PartId(3),
                        geometry: sphere_geometry,
                        root: boon_solid_model::SolidNodeId(1),
                        appearance: Some(boon_solid_model::AppearanceMaterialId(3)),
                        physical_material: Some(boon_solid_model::PhysicalMaterialId(1)),
                        manufacturing_role: boon_solid_model::ManufacturingRole::PrintableSolid,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            instances: vec![
                boon_solid_model::PartInstance {
                    id: boon_solid_model::PartInstanceId(1),
                    part: boon_solid_model::PartId(1),
                    transform: boon_solid_model::Mat4d::IDENTITY,
                    label: "Cone".to_owned(),
                },
                boon_solid_model::PartInstance {
                    id: boon_solid_model::PartInstanceId(2),
                    part: boon_solid_model::PartId(2),
                    transform: boon_solid_model::Mat4d::translation(boon_solid_model::Vec3d::new(
                        32.0, 0.0, 0.0,
                    )),
                    label: "Torus".to_owned(),
                },
                boon_solid_model::PartInstance {
                    id: boon_solid_model::PartInstanceId(3),
                    part: boon_solid_model::PartId(3),
                    transform: boon_solid_model::Mat4d::translation(boon_solid_model::Vec3d::new(
                        -28.0, 0.0, 0.0,
                    )),
                    label: "Sphere".to_owned(),
                },
            ],
            constraints: Vec::new(),
        },
    };

    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("curved fixture should compile to visual proxy scene");
    let chunk_by_geometry = visual
        .chunks
        .iter()
        .map(|chunk| (chunk.id.geometry, chunk))
        .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(
        visual.report.visual_compiler_status,
        "retained-exact-primitive-mesh-no-csg"
    );
    assert_eq!(visual.report.exact_mesh_count, 3);
    assert_eq!(visual.report.retained_chunk_count, 3);
    assert_eq!(visual.report.generated_mesh_count, 3);
    assert_eq!(visual.report.adaptive_chunk_count, 0);
    assert_eq!(visual.report.proxy_bounds_chunk_count, 0);
    assert!(!visual.report.manufacturing_mesh_used);
    assert_eq!(
        chunk_by_geometry[&GeometryLogicalId(1)].id.tolerance_class,
        "exact-primitive"
    );
    assert_eq!(
        chunk_by_geometry[&GeometryLogicalId(2)].id.tolerance_class,
        "exact-primitive"
    );
    assert_eq!(
        chunk_by_geometry[&GeometryLogicalId(3)].id.tolerance_class,
        "exact-primitive"
    );
    match &chunk_by_geometry[&GeometryLogicalId(1)].representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 66);
            assert_eq!(mesh.indices.len(), 384);
        }
        other => panic!("expected exact cone mesh, got {other:?}"),
    }
    match &chunk_by_geometry[&GeometryLogicalId(2)].representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 384);
            assert_eq!(mesh.indices.len(), 2304);
        }
        other => panic!("expected exact torus mesh, got {other:?}"),
    }
    match &chunk_by_geometry[&GeometryLogicalId(3)].representation {
        SurfaceRepresentation::IndexedMesh(mesh) => {
            assert_eq!(mesh.vertices.len(), 482);
            assert_eq!(mesh.indices.len(), 2880);
        }
        other => panic!("expected exact sphere mesh, got {other:?}"),
    }
}

