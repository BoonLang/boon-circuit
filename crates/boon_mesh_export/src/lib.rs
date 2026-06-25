use std::collections::{BTreeMap, BTreeSet};

use boon_manufacturing::{
    GridPoint2D, Layer, ManufacturingCompileStatus, MaterialRegion2D, PolygonWithHoles,
    PrintCompileOutput,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StlExportStatus {
    Pass,
    #[default]
    Fail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StlDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StlMetrics {
    pub layer_count: usize,
    pub component_count: usize,
    pub polygon_count: usize,
    pub preserved_hole_count: usize,
    pub rejected_hole_count: usize,
    pub triangle_count: usize,
    pub byte_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StlPackage {
    pub status: StlExportStatus,
    pub scope: String,
    pub ascii: String,
    pub diagnostics: Vec<StlDiagnostic>,
    pub metrics: StlMetrics,
    pub artifact_hash: String,
    pub source_manufacturing_artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryStlPackage {
    pub status: StlExportStatus,
    pub scope: String,
    pub bytes: Vec<u8>,
    pub diagnostics: Vec<StlDiagnostic>,
    pub metrics: StlMetrics,
    pub artifact_hash: String,
    pub source_manufacturing_artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StlValidationReport {
    pub status: StlExportStatus,
    pub diagnostics: Vec<StlDiagnostic>,
    pub metrics: StlMetrics,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StlTopologyReport {
    pub status: StlExportStatus,
    pub checked_prism_count: usize,
    pub triangle_count: usize,
    pub raw_triangle_count: usize,
    pub culled_internal_triangle_pair_count: usize,
    pub culled_duplicate_triangle_count: usize,
    pub boundary_edge_count: usize,
    pub non_manifold_edge_count: usize,
    pub degenerate_triangle_count: usize,
    pub boundary_edge_samples: Vec<StlTopologyEdgeSample>,
    pub non_manifold_edge_samples: Vec<StlTopologyEdgeSample>,
    pub diagnostics: Vec<StlDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StlTopologyEdgeSample {
    pub incident_triangle_count: usize,
    pub from: [String; 3],
    pub to: [String; 3],
}

#[derive(Clone, Copy, Debug)]
struct Point3 {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy, Debug)]
struct Triangle {
    normal: Point3,
    a: Point3,
    b: Point3,
    c: Point3,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct VertexKey(String, String, String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeKey(VertexKey, VertexKey);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FaceKey([VertexKey; 3]);

#[derive(Clone, Debug)]
struct TriangleBuildOutput {
    triangles: Vec<Triangle>,
    raw_triangle_count: usize,
    culled_internal_triangle_pair_count: usize,
    culled_duplicate_triangle_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GridRect {
    min_x: i64,
    min_y: i64,
    max_x: i64,
    max_y: i64,
}

pub fn export_ascii_stl_from_layers(print: &PrintCompileOutput) -> StlPackage {
    let mut diagnostics = preflight_diagnostics(print);
    let source_hole_count = source_hole_count(print);

    let mut metrics = metrics_for(print, 0, source_hole_count, 0, 0);
    let ascii = if diagnostics.is_empty() {
        match build_triangles(print) {
            Ok(triangles) => {
                let ascii = ascii_stl(print, &triangles);
                metrics.triangle_count = triangles.len();
                metrics.byte_count = ascii.len();
                ascii
            }
            Err(mut export_diagnostics) => {
                diagnostics.append(&mut export_diagnostics);
                String::new()
            }
        }
    } else {
        String::new()
    };
    let artifact_hash = sha256_bytes(ascii.as_bytes());
    StlPackage {
        status: if diagnostics.is_empty() {
            StlExportStatus::Pass
        } else {
            StlExportStatus::Fail
        },
        scope: "authoritative-manufacturing-layer-ascii-stl-hole-preserving".to_owned(),
        ascii,
        diagnostics,
        metrics,
        artifact_hash,
        source_manufacturing_artifact_hash: print.artifact_hash.clone(),
        visual_mesh_used_for_manufacturing: print.visual_mesh_used_for_manufacturing,
    }
}

pub fn validate_export_triangle_stream_topology(print: &PrintCompileOutput) -> StlTopologyReport {
    let mut report = StlTopologyReport {
        diagnostics: preflight_diagnostics(print),
        checked_prism_count: count_export_prisms(print),
        ..StlTopologyReport::default()
    };
    if !report.diagnostics.is_empty() {
        report.status = StlExportStatus::Fail;
        return report;
    }

    match build_export_triangles(print) {
        Ok(output) => {
            report.raw_triangle_count = output.raw_triangle_count;
            report.culled_internal_triangle_pair_count = output.culled_internal_triangle_pair_count;
            report.culled_duplicate_triangle_count = output.culled_duplicate_triangle_count;
            accumulate_topology(&output.triangles, &mut report);
        }
        Err(mut diagnostics) => report.diagnostics.append(&mut diagnostics),
    }

    push_topology_diagnostics(&mut report, "STL exported aggregate topology");
    report.status = if report.diagnostics.is_empty() {
        StlExportStatus::Pass
    } else {
        StlExportStatus::Fail
    };
    report
}

pub fn export_binary_stl_from_layers(print: &PrintCompileOutput) -> BinaryStlPackage {
    let mut diagnostics = preflight_diagnostics(print);
    let source_hole_count = source_hole_count(print);

    let mut metrics = metrics_for(print, 0, source_hole_count, 0, 0);
    let bytes = if diagnostics.is_empty() {
        match build_triangles(print) {
            Ok(triangles) => {
                if triangles.len() > u32::MAX as usize {
                    diagnostics.push(StlDiagnostic {
                        code: "binary-stl-triangle-count-overflow".to_owned(),
                        message: format!(
                            "binary STL supports at most {} triangles, got {}",
                            u32::MAX,
                            triangles.len()
                        ),
                    });
                    Vec::new()
                } else {
                    let bytes = binary_stl(print, &triangles);
                    metrics.triangle_count = triangles.len();
                    metrics.byte_count = bytes.len();
                    bytes
                }
            }
            Err(mut export_diagnostics) => {
                diagnostics.append(&mut export_diagnostics);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let artifact_hash = sha256_bytes(&bytes);
    BinaryStlPackage {
        status: if diagnostics.is_empty() {
            StlExportStatus::Pass
        } else {
            StlExportStatus::Fail
        },
        scope: "authoritative-manufacturing-layer-binary-stl-hole-preserving".to_owned(),
        bytes,
        diagnostics,
        metrics,
        artifact_hash,
        source_manufacturing_artifact_hash: print.artifact_hash.clone(),
        visual_mesh_used_for_manufacturing: print.visual_mesh_used_for_manufacturing,
    }
}

fn preflight_diagnostics(print: &PrintCompileOutput) -> Vec<StlDiagnostic> {
    let mut diagnostics = Vec::new();
    if print.status != ManufacturingCompileStatus::Pass {
        diagnostics.push(StlDiagnostic {
            code: "manufacturing-output-not-pass".to_owned(),
            message: "STL export requires a passing manufacturing layer output".to_owned(),
        });
    }
    if print.visual_mesh_used_for_manufacturing {
        diagnostics.push(StlDiagnostic {
            code: "visual-mesh-source-rejected".to_owned(),
            message: "STL export must not use visual mesh output as manufacturing source"
                .to_owned(),
        });
    }
    if print.layers.is_empty() {
        diagnostics.push(StlDiagnostic {
            code: "missing-layers".to_owned(),
            message: "STL export requires at least one manufacturing layer".to_owned(),
        });
    }
    diagnostics
}

fn source_hole_count(print: &PrintCompileOutput) -> usize {
    print
        .layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .flat_map(|region| &region.polygons)
        .map(|polygon| polygon.holes.len())
        .sum()
}

pub fn validate_ascii_stl_package(package: &StlPackage) -> StlValidationReport {
    let mut diagnostics = Vec::new();
    if package.status != StlExportStatus::Pass {
        diagnostics.push(StlDiagnostic {
            code: "package-export-not-pass".to_owned(),
            message: "STL validation requires a passing exported package".to_owned(),
        });
    }
    if !package.ascii.starts_with("solid boon_manufacturing_layers") {
        diagnostics.push(StlDiagnostic {
            code: "missing-solid-header".to_owned(),
            message: "ASCII STL output is missing the Boon solid header".to_owned(),
        });
    }
    if !package
        .ascii
        .trim_end()
        .ends_with("endsolid boon_manufacturing_layers")
    {
        diagnostics.push(StlDiagnostic {
            code: "missing-solid-footer".to_owned(),
            message: "ASCII STL output is missing the Boon solid footer".to_owned(),
        });
    }
    let facet_count = package.ascii.matches("facet normal").count();
    let vertex_count = package.ascii.matches("vertex ").count();
    if facet_count == 0 {
        diagnostics.push(StlDiagnostic {
            code: "missing-facets".to_owned(),
            message: "ASCII STL output contains no facets".to_owned(),
        });
    }
    if vertex_count != facet_count * 3 {
        diagnostics.push(StlDiagnostic {
            code: "vertex-count".to_owned(),
            message: format!(
                "ASCII STL vertex count {vertex_count} does not match facet count {facet_count}"
            ),
        });
    }
    if package.metrics.triangle_count != facet_count {
        diagnostics.push(StlDiagnostic {
            code: "triangle-count".to_owned(),
            message: "ASCII STL facet count does not match package metrics".to_owned(),
        });
    }
    if package.metrics.byte_count != package.ascii.len() {
        diagnostics.push(StlDiagnostic {
            code: "byte-count".to_owned(),
            message: "ASCII STL byte count does not match package metrics".to_owned(),
        });
    }
    if !package.artifact_hash.starts_with("sha256:") {
        diagnostics.push(StlDiagnostic {
            code: "artifact-hash".to_owned(),
            message: "ASCII STL artifact hash is missing sha256 prefix".to_owned(),
        });
    }
    if package.visual_mesh_used_for_manufacturing {
        diagnostics.push(StlDiagnostic {
            code: "visual-mesh-source".to_owned(),
            message: "STL package claims visual mesh use for manufacturing".to_owned(),
        });
    }

    StlValidationReport {
        status: if diagnostics.is_empty() {
            StlExportStatus::Pass
        } else {
            StlExportStatus::Fail
        },
        diagnostics,
        metrics: StlMetrics {
            triangle_count: facet_count,
            byte_count: package.ascii.len(),
            ..package.metrics.clone()
        },
    }
}

pub fn validate_binary_stl_package(package: &BinaryStlPackage) -> StlValidationReport {
    let mut diagnostics = Vec::new();
    if package.status != StlExportStatus::Pass {
        diagnostics.push(StlDiagnostic {
            code: "package-export-not-pass".to_owned(),
            message: "binary STL validation requires a passing exported package".to_owned(),
        });
    }
    if package.bytes.len() < 84 {
        diagnostics.push(StlDiagnostic {
            code: "binary-stl-too-short".to_owned(),
            message: "binary STL output is shorter than the 84-byte header/count prefix".to_owned(),
        });
    }
    let triangle_count = package
        .bytes
        .get(80..84)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0) as usize;
    let payload_len = package.bytes.len().saturating_sub(84);
    if payload_len % 50 != 0 {
        diagnostics.push(StlDiagnostic {
            code: "binary-stl-record-size".to_owned(),
            message: "binary STL payload length is not a multiple of 50 bytes".to_owned(),
        });
    }
    let record_count = payload_len / 50;
    if triangle_count != record_count {
        diagnostics.push(StlDiagnostic {
            code: "binary-stl-triangle-count".to_owned(),
            message: format!(
                "binary STL header triangle count {triangle_count} does not match record count {record_count}"
            ),
        });
    }
    if package.metrics.triangle_count != triangle_count {
        diagnostics.push(StlDiagnostic {
            code: "triangle-count".to_owned(),
            message: "binary STL triangle count does not match package metrics".to_owned(),
        });
    }
    if package.metrics.byte_count != package.bytes.len() {
        diagnostics.push(StlDiagnostic {
            code: "byte-count".to_owned(),
            message: "binary STL byte count does not match package metrics".to_owned(),
        });
    }
    if !package.artifact_hash.starts_with("sha256:") {
        diagnostics.push(StlDiagnostic {
            code: "artifact-hash".to_owned(),
            message: "binary STL artifact hash is missing sha256 prefix".to_owned(),
        });
    }
    if package.visual_mesh_used_for_manufacturing {
        diagnostics.push(StlDiagnostic {
            code: "visual-mesh-source".to_owned(),
            message: "binary STL package claims visual mesh use for manufacturing".to_owned(),
        });
    }

    StlValidationReport {
        status: if diagnostics.is_empty() {
            StlExportStatus::Pass
        } else {
            StlExportStatus::Fail
        },
        diagnostics,
        metrics: StlMetrics {
            triangle_count,
            byte_count: package.bytes.len(),
            ..package.metrics.clone()
        },
    }
}

pub fn validate_layer_prism_topology(print: &PrintCompileOutput) -> StlTopologyReport {
    let mut report = StlTopologyReport {
        diagnostics: preflight_diagnostics(print),
        ..StlTopologyReport::default()
    };
    if !report.diagnostics.is_empty() {
        report.status = StlExportStatus::Fail;
        return report;
    }

    for layer in &print.layers {
        let z0 = layer.z - print.request.layer_height * 0.5;
        let z1 = layer.z + print.request.layer_height * 0.5;
        for region in &layer.regions {
            for polygon in &region.polygons {
                if polygon.outer.len() < 3 {
                    continue;
                }
                report.checked_prism_count += 1;
                let mut triangles = Vec::new();
                match push_polygon_prism(
                    &mut triangles,
                    polygon,
                    &[],
                    &[],
                    &[],
                    print.request.integer_grid,
                    z0,
                    z1,
                ) {
                    Ok(()) => accumulate_topology(&triangles, &mut report),
                    Err(diagnostic) => report.diagnostics.push(diagnostic),
                }
            }
        }
    }

    if report.checked_prism_count == 0 {
        report.diagnostics.push(StlDiagnostic {
            code: "topology-no-prisms".to_owned(),
            message: "STL topology validation found no layer prisms to check".to_owned(),
        });
    }
    push_topology_diagnostics(&mut report, "STL per-prism topology");
    report.status = if report.diagnostics.is_empty() {
        StlExportStatus::Pass
    } else {
        StlExportStatus::Fail
    };
    report
}

fn push_topology_diagnostics(report: &mut StlTopologyReport, label: &str) {
    if report.boundary_edge_count > 0 {
        report.diagnostics.push(StlDiagnostic {
            code: "topology-boundary-edges".to_owned(),
            message: format!("{label} has {} boundary edges", report.boundary_edge_count),
        });
    }
    if report.non_manifold_edge_count > 0 {
        report.diagnostics.push(StlDiagnostic {
            code: "topology-non-manifold-edges".to_owned(),
            message: format!(
                "{label} has {} non-manifold edges",
                report.non_manifold_edge_count
            ),
        });
    }
    if report.degenerate_triangle_count > 0 {
        report.diagnostics.push(StlDiagnostic {
            code: "topology-degenerate-triangles".to_owned(),
            message: format!(
                "{label} has {} degenerate triangles",
                report.degenerate_triangle_count
            ),
        });
    }
}

fn accumulate_topology(triangles: &[Triangle], report: &mut StlTopologyReport) {
    let mut edges = BTreeMap::<EdgeKey, usize>::new();
    for triangle in triangles {
        report.triangle_count += 1;
        if triangle.normal.x == 0.0 && triangle.normal.y == 0.0 && triangle.normal.z == 0.0 {
            report.degenerate_triangle_count += 1;
        }
        for edge in [
            edge_key(triangle.a, triangle.b),
            edge_key(triangle.b, triangle.c),
            edge_key(triangle.c, triangle.a),
        ] {
            *edges.entry(edge).or_insert(0) += 1;
        }
    }
    for (edge, count) in edges {
        match count {
            2 => {}
            1 => {
                report.boundary_edge_count += 1;
                push_topology_edge_sample(&mut report.boundary_edge_samples, edge, count);
            }
            _ => {
                report.non_manifold_edge_count += 1;
                push_topology_edge_sample(&mut report.non_manifold_edge_samples, edge, count);
            }
        }
    }
}

fn push_topology_edge_sample(
    samples: &mut Vec<StlTopologyEdgeSample>,
    edge: EdgeKey,
    incident_triangle_count: usize,
) {
    const MAX_TOPOLOGY_EDGE_SAMPLES: usize = 8;
    if samples.len() >= MAX_TOPOLOGY_EDGE_SAMPLES {
        return;
    }
    samples.push(StlTopologyEdgeSample {
        incident_triangle_count,
        from: vertex_key_components(edge.0),
        to: vertex_key_components(edge.1),
    });
}

fn edge_key(a: Point3, b: Point3) -> EdgeKey {
    let a = vertex_key(a);
    let b = vertex_key(b);
    if a <= b { EdgeKey(a, b) } else { EdgeKey(b, a) }
}

fn face_key(triangle: Triangle) -> FaceKey {
    let mut vertices = [
        vertex_key(triangle.a),
        vertex_key(triangle.b),
        vertex_key(triangle.c),
    ];
    vertices.sort();
    FaceKey(vertices)
}

fn triangle_normals_are_opposite(a: Triangle, b: Triangle) -> bool {
    let dot = a.normal.x * b.normal.x + a.normal.y * b.normal.y + a.normal.z * b.normal.z;
    dot < -0.999
}

fn triangle_normals_are_same(a: Triangle, b: Triangle) -> bool {
    let dot = a.normal.x * b.normal.x + a.normal.y * b.normal.y + a.normal.z * b.normal.z;
    dot > 0.999
}

fn vertex_key(point: Point3) -> VertexKey {
    VertexKey(fmt_f64(point.x), fmt_f64(point.y), fmt_f64(point.z))
}

fn vertex_key_components(key: VertexKey) -> [String; 3] {
    [key.0, key.1, key.2]
}

fn metrics_for(
    print: &PrintCompileOutput,
    rejected_hole_count: usize,
    preserved_hole_count: usize,
    triangle_count: usize,
    byte_count: usize,
) -> StlMetrics {
    let components = print
        .layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .map(|region| (region.part, region.instance))
        .collect::<BTreeSet<_>>();
    StlMetrics {
        layer_count: print.metrics.layer_count,
        component_count: components.len(),
        polygon_count: print.metrics.polygon_count,
        preserved_hole_count,
        rejected_hole_count,
        triangle_count,
        byte_count,
    }
}

fn build_export_triangles(
    print: &PrintCompileOutput,
) -> Result<TriangleBuildOutput, Vec<StlDiagnostic>> {
    let triangles = build_raw_triangles(print)?;
    let raw_triangle_count = triangles.len();
    let (triangles, culled_internal_triangle_pair_count, culled_duplicate_triangle_count) =
        cull_internal_duplicate_triangles(triangles);
    Ok(TriangleBuildOutput {
        triangles,
        raw_triangle_count,
        culled_internal_triangle_pair_count,
        culled_duplicate_triangle_count,
    })
}

fn build_triangles(print: &PrintCompileOutput) -> Result<Vec<Triangle>, Vec<StlDiagnostic>> {
    build_export_triangles(print).map(|output| output.triangles)
}

fn build_raw_triangles(print: &PrintCompileOutput) -> Result<Vec<Triangle>, Vec<StlDiagnostic>> {
    let mut triangles = Vec::new();
    let mut diagnostics = Vec::new();
    for (layer_index, layer) in print.layers.iter().enumerate() {
        let z0 = layer.z - print.request.layer_height * 0.5;
        let z1 = layer.z + print.request.layer_height * 0.5;
        let previous_layer = adjacent_layer(print, layer_index, z0, false);
        let next_layer = adjacent_layer(print, layer_index, z1, true);
        for region in &layer.regions {
            let wall_breakers = region_polygons_for_wall_breaks(print, region);
            for polygon in &region.polygons {
                if let Err(diagnostic) = push_polygon_prism(
                    &mut triangles,
                    polygon,
                    wall_breakers.as_slice(),
                    adjacent_region_polygons(previous_layer, region).as_slice(),
                    adjacent_region_polygons(next_layer, region).as_slice(),
                    print.request.integer_grid,
                    z0,
                    z1,
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(triangles)
    } else {
        Err(diagnostics)
    }
}

fn count_export_prisms(print: &PrintCompileOutput) -> usize {
    print
        .layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .flat_map(|region| &region.polygons)
        .filter(|polygon| polygon.outer.len() >= 3)
        .count()
}

fn adjacent_layer(
    print: &PrintCompileOutput,
    layer_index: usize,
    surface_z: f64,
    above: bool,
) -> Option<&Layer> {
    let adjacent = if above {
        print.layers.get(layer_index + 1)
    } else {
        layer_index
            .checked_sub(1)
            .and_then(|index| print.layers.get(index))
    }?;
    let adjacent_surface_z = if above {
        adjacent.z - print.request.layer_height * 0.5
    } else {
        adjacent.z + print.request.layer_height * 0.5
    };
    let tolerance = (print.request.layer_height.abs() * 1.0e-9).max(1.0e-9);
    ((adjacent_surface_z - surface_z).abs() <= tolerance).then_some(adjacent)
}

fn adjacent_region_polygons<'a>(
    adjacent_layer: Option<&'a Layer>,
    region: &MaterialRegion2D,
) -> Vec<&'a PolygonWithHoles> {
    adjacent_layer
        .into_iter()
        .flat_map(|layer| &layer.regions)
        .filter(|adjacent| {
            adjacent.part == region.part
                && adjacent.instance == region.instance
                && adjacent.material == region.material
        })
        .flat_map(|adjacent| &adjacent.polygons)
        .collect()
}

fn region_polygons_for_wall_breaks<'a>(
    print: &'a PrintCompileOutput,
    region: &MaterialRegion2D,
) -> Vec<&'a PolygonWithHoles> {
    print
        .layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .filter(|candidate| {
            candidate.part == region.part
                && candidate.instance == region.instance
                && candidate.material == region.material
        })
        .flat_map(|candidate| &candidate.polygons)
        .collect()
}

fn cull_internal_duplicate_triangles(triangles: Vec<Triangle>) -> (Vec<Triangle>, usize, usize) {
    let mut face_indices = BTreeMap::<FaceKey, Vec<usize>>::new();
    for (index, triangle) in triangles.iter().enumerate() {
        face_indices
            .entry(face_key(*triangle))
            .or_default()
            .push(index);
    }

    let mut culled = vec![false; triangles.len()];
    let mut internal_pair_count = 0;
    let mut duplicate_count = 0;
    for indices in face_indices.values() {
        for (position, a) in indices.iter().enumerate() {
            if culled[*a] {
                continue;
            }
            for b in indices.iter().skip(position + 1) {
                if culled[*b] {
                    continue;
                }
                if triangle_normals_are_opposite(triangles[*a], triangles[*b]) {
                    culled[*a] = true;
                    culled[*b] = true;
                    internal_pair_count += 1;
                    break;
                }
            }
        }
        let mut retained_for_face = Vec::<usize>::new();
        for index in indices {
            if culled[*index] {
                continue;
            }
            if retained_for_face
                .iter()
                .any(|retained| triangle_normals_are_same(triangles[*retained], triangles[*index]))
            {
                culled[*index] = true;
                duplicate_count += 1;
            } else {
                retained_for_face.push(*index);
            }
        }
    }

    let retained = triangles
        .into_iter()
        .enumerate()
        .filter_map(|(index, triangle)| (!culled[index]).then_some(triangle))
        .collect();
    (retained, internal_pair_count, duplicate_count)
}

fn ascii_stl(print: &PrintCompileOutput, triangles: &[Triangle]) -> String {
    let mut output = String::from("solid boon_manufacturing_layers\n");
    output.push_str(&format!(
        "  // source manufacturing artifact: {}\n",
        print.artifact_hash
    ));
    for triangle in triangles {
        write_facet(&mut output, *triangle);
    }
    output.push_str("endsolid boon_manufacturing_layers\n");
    output
}

fn binary_stl(print: &PrintCompileOutput, triangles: &[Triangle]) -> Vec<u8> {
    let mut output = vec![0_u8; 80];
    let header = format!(
        "Boon manufacturing layers {}",
        print.artifact_hash.trim_start_matches("sha256:")
    );
    for (slot, byte) in output.iter_mut().zip(header.as_bytes()) {
        *slot = *byte;
    }
    output.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for triangle in triangles {
        for value in [
            triangle.normal.x,
            triangle.normal.y,
            triangle.normal.z,
            triangle.a.x,
            triangle.a.y,
            triangle.a.z,
            triangle.b.x,
            triangle.b.y,
            triangle.b.z,
            triangle.c.x,
            triangle.c.y,
            triangle.c.z,
        ] {
            output.extend_from_slice(&(value as f32).to_le_bytes());
        }
        output.extend_from_slice(&0_u16.to_le_bytes());
    }
    output
}

fn push_polygon_prism(
    triangles: &mut Vec<Triangle>,
    polygon: &PolygonWithHoles,
    wall_breakers: &[&PolygonWithHoles],
    bottom_blockers: &[&PolygonWithHoles],
    top_blockers: &[&PolygonWithHoles],
    grid: f64,
    z0: f64,
    z1: f64,
) -> Result<(), StlDiagnostic> {
    let wall_polygon = polygon_with_cap_breakpoints(polygon, wall_breakers);
    push_polygon_walls(triangles, &wall_polygon, grid, z0, z1)?;
    for visible_top in visible_cap_polygons(polygon, top_blockers) {
        let visible_top = polygon_with_boundary_points(&visible_top, &wall_polygon.outer);
        push_polygon_cap(triangles, &visible_top, grid, z1, true)?;
    }
    for visible_bottom in visible_cap_polygons(polygon, bottom_blockers) {
        let visible_bottom = polygon_with_boundary_points(&visible_bottom, &wall_polygon.outer);
        push_polygon_cap(triangles, &visible_bottom, grid, z0, false)?;
    }
    Ok(())
}

fn polygon_with_cap_breakpoints(
    polygon: &PolygonWithHoles,
    blockers: &[&PolygonWithHoles],
) -> PolygonWithHoles {
    let Some(subject) = polygon_rect(&polygon.outer) else {
        return polygon.clone();
    };
    let mut boundary_points = subject.to_polygon();
    for blocker in blockers {
        let Some(blocker) = polygon_rect(&blocker.outer) else {
            continue;
        };
        let Some(overlap) = subject.intersection(blocker) else {
            continue;
        };
        for point in overlap.to_polygon() {
            if subject.point_on_boundary(point) && !boundary_points.contains(&point) {
                boundary_points.push(point);
            }
        }
    }
    polygon_with_boundary_points_for_rect(polygon, subject, boundary_points)
}

fn polygon_with_boundary_points(
    polygon: &PolygonWithHoles,
    points: &[GridPoint2D],
) -> PolygonWithHoles {
    let Some(subject) = polygon_rect(&polygon.outer) else {
        return polygon.clone();
    };
    let mut boundary_points = polygon.outer.clone();
    for point in points {
        if subject.point_on_boundary(*point) && !boundary_points.contains(point) {
            boundary_points.push(*point);
        }
    }
    polygon_with_boundary_points_for_rect(polygon, subject, boundary_points)
}

fn polygon_with_boundary_points_for_rect(
    polygon: &PolygonWithHoles,
    rect: GridRect,
    boundary_points: Vec<GridPoint2D>,
) -> PolygonWithHoles {
    PolygonWithHoles {
        outer: sort_rect_boundary_points(boundary_points, rect),
        holes: polygon.holes.clone(),
        source_features: polygon.source_features.clone(),
    }
}

fn visible_cap_polygons(
    polygon: &PolygonWithHoles,
    blockers: &[&PolygonWithHoles],
) -> Vec<PolygonWithHoles> {
    let mut visible = vec![polygon.clone()];
    for blocker in blockers {
        visible = visible
            .into_iter()
            .flat_map(|candidate| subtract_rectangular_cap_overlap(&candidate, blocker))
            .collect();
    }
    visible
}

fn subtract_rectangular_cap_overlap(
    polygon: &PolygonWithHoles,
    blocker: &PolygonWithHoles,
) -> Vec<PolygonWithHoles> {
    let Some(subject) = polygon_rect(&polygon.outer) else {
        return vec![polygon.clone()];
    };
    let blocker_polygon = blocker;
    let Some(blocker) = polygon_rect(&blocker_polygon.outer) else {
        return vec![polygon.clone()];
    };
    let Some(overlap) = subject.intersection(blocker) else {
        return vec![polygon.clone()];
    };
    if overlap == subject {
        return blocker_polygon
            .holes
            .iter()
            .filter_map(|hole| rectangular_hole_cap_piece_inside_subject(hole, subject, polygon))
            .collect();
    }

    let mut pieces = Vec::new();
    if subject.min_x < overlap.min_x {
        pieces.push(GridRect {
            min_x: subject.min_x,
            min_y: subject.min_y,
            max_x: overlap.min_x,
            max_y: subject.max_y,
        });
    }
    if overlap.max_x < subject.max_x {
        pieces.push(GridRect {
            min_x: overlap.max_x,
            min_y: subject.min_y,
            max_x: subject.max_x,
            max_y: subject.max_y,
        });
    }
    if subject.min_y < overlap.min_y {
        pieces.push(GridRect {
            min_x: overlap.min_x,
            min_y: subject.min_y,
            max_x: overlap.max_x,
            max_y: overlap.min_y,
        });
    }
    if overlap.max_y < subject.max_y {
        pieces.push(GridRect {
            min_x: overlap.min_x,
            min_y: overlap.max_y,
            max_x: overlap.max_x,
            max_y: subject.max_y,
        });
    }

    let mut result = Vec::new();
    for piece in pieces {
        let mut holes = Vec::new();
        for hole in &polygon.holes {
            if ring_inside_rect(hole, piece) {
                holes.push(hole.clone());
            } else if ring_intersects_rect(hole, piece) {
                return vec![polygon.clone()];
            }
        }
        result.push(PolygonWithHoles {
            outer: piece.to_polygon(),
            holes,
            source_features: polygon.source_features.clone(),
        });
    }
    result
}

fn rectangular_hole_cap_piece_inside_subject(
    hole: &[GridPoint2D],
    subject: GridRect,
    source: &PolygonWithHoles,
) -> Option<PolygonWithHoles> {
    let hole = polygon_rect(hole)?;
    let piece = subject.intersection(hole)?;
    Some(PolygonWithHoles {
        outer: piece.to_polygon(),
        holes: Vec::new(),
        source_features: source.source_features.clone(),
    })
}

fn polygon_rect(points: &[GridPoint2D]) -> Option<GridRect> {
    if points.len() != 4 {
        return None;
    }
    let min_x = points.iter().map(|point| point.x).min()?;
    let max_x = points.iter().map(|point| point.x).max()?;
    let min_y = points.iter().map(|point| point.y).min()?;
    let max_y = points.iter().map(|point| point.y).max()?;
    if min_x == max_x || min_y == max_y {
        return None;
    }
    let corners = [
        GridPoint2D { x: min_x, y: min_y },
        GridPoint2D { x: max_x, y: min_y },
        GridPoint2D { x: max_x, y: max_y },
        GridPoint2D { x: min_x, y: max_y },
    ];
    corners
        .iter()
        .all(|corner| points.contains(corner))
        .then_some(GridRect {
            min_x,
            min_y,
            max_x,
            max_y,
        })
}

fn ring_inside_rect(points: &[GridPoint2D], rect: GridRect) -> bool {
    points.iter().all(|point| rect.contains(*point))
}

fn ring_intersects_rect(points: &[GridPoint2D], rect: GridRect) -> bool {
    points.iter().any(|point| rect.contains(*point))
}

fn sort_rect_boundary_points(mut points: Vec<GridPoint2D>, rect: GridRect) -> Vec<GridPoint2D> {
    points.sort_by_key(|point| rect_boundary_sort_key(*point, rect));
    points.dedup();
    points
}

fn rect_boundary_sort_key(point: GridPoint2D, rect: GridRect) -> (u8, i64) {
    if point.y == rect.min_y {
        (0, point.x)
    } else if point.x == rect.max_x {
        (1, point.y)
    } else if point.y == rect.max_y {
        (2, -point.x)
    } else {
        (3, -point.y)
    }
}

impl GridRect {
    fn intersection(self, other: GridRect) -> Option<GridRect> {
        let overlap = GridRect {
            min_x: self.min_x.max(other.min_x),
            min_y: self.min_y.max(other.min_y),
            max_x: self.max_x.min(other.max_x),
            max_y: self.max_y.min(other.max_y),
        };
        (overlap.min_x < overlap.max_x && overlap.min_y < overlap.max_y).then_some(overlap)
    }

    fn contains(self, point: GridPoint2D) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
    }

    fn point_on_boundary(self, point: GridPoint2D) -> bool {
        self.contains(point)
            && (point.x == self.min_x
                || point.x == self.max_x
                || point.y == self.min_y
                || point.y == self.max_y)
    }

    fn to_polygon(self) -> Vec<GridPoint2D> {
        vec![
            GridPoint2D {
                x: self.min_x,
                y: self.min_y,
            },
            GridPoint2D {
                x: self.max_x,
                y: self.min_y,
            },
            GridPoint2D {
                x: self.max_x,
                y: self.max_y,
            },
            GridPoint2D {
                x: self.min_x,
                y: self.max_y,
            },
        ]
    }
}

fn push_polygon_cap(
    triangles: &mut Vec<Triangle>,
    polygon: &PolygonWithHoles,
    grid: f64,
    z: f64,
    top: bool,
) -> Result<(), StlDiagnostic> {
    if polygon.outer.len() < 3 {
        return Ok(());
    }
    let (vertices, hole_indices, _rings) = polygon_to_earcut_vertices(polygon, grid);
    let indices = earcutr::earcut(&vertices, &hole_indices, 2).map_err(|error| StlDiagnostic {
        code: "hole-triangulation-failed".to_owned(),
        message: format!("STL export failed to triangulate layer polygon with holes: {error:?}"),
    })?;
    if indices.is_empty() {
        return Err(StlDiagnostic {
            code: "hole-triangulation-empty".to_owned(),
            message: "STL export produced no cap triangles for a layer polygon".to_owned(),
        });
    }
    let points = vertices
        .chunks_exact(2)
        .map(|coords| Point3 {
            x: coords[0],
            y: coords[1],
            z: 0.0,
        })
        .collect::<Vec<_>>();

    for triangle in indices.chunks_exact(3) {
        let a = points[triangle[0]];
        let b = points[triangle[1]];
        let c = points[triangle[2]];
        if top {
            push_oriented_cap(triangles, a, b, c, z, true);
        } else {
            push_oriented_cap(triangles, a, c, b, z, false);
        }
    }
    Ok(())
}

fn push_polygon_walls(
    triangles: &mut Vec<Triangle>,
    polygon: &PolygonWithHoles,
    grid: f64,
    z0: f64,
    z1: f64,
) -> Result<(), StlDiagnostic> {
    if polygon.outer.len() < 3 {
        return Ok(());
    }
    let (vertices, _hole_indices, rings) = polygon_to_earcut_vertices(polygon, grid);
    let points = vertices
        .chunks_exact(2)
        .map(|coords| Point3 {
            x: coords[0],
            y: coords[1],
            z: 0.0,
        })
        .collect::<Vec<_>>();
    if let Some(outer) = rings.first() {
        push_ring_walls(triangles, &points, outer, z0, z1, false);
    }
    for hole in rings.iter().skip(1) {
        push_ring_walls(triangles, &points, hole, z0, z1, true);
    }
    Ok(())
}

fn polygon_to_earcut_vertices(
    polygon: &PolygonWithHoles,
    grid: f64,
) -> (Vec<f64>, Vec<usize>, Vec<Vec<usize>>) {
    let mut vertices = Vec::new();
    let mut hole_indices = Vec::new();
    let mut rings = Vec::new();
    push_ring(&polygon.outer, grid, &mut vertices, &mut rings);
    for hole in &polygon.holes {
        hole_indices.push(vertices.len() / 2);
        push_ring(hole, grid, &mut vertices, &mut rings);
    }
    (vertices, hole_indices, rings)
}

fn push_ring(
    points: &[GridPoint2D],
    grid: f64,
    vertices: &mut Vec<f64>,
    rings: &mut Vec<Vec<usize>>,
) {
    let mut ring = Vec::new();
    for point in points {
        ring.push(vertices.len() / 2);
        vertices.push(point.x as f64 * grid);
        vertices.push(point.y as f64 * grid);
    }
    rings.push(ring);
}

fn push_oriented_cap(
    triangles: &mut Vec<Triangle>,
    a: Point3,
    b: Point3,
    c: Point3,
    z: f64,
    top: bool,
) {
    let a = Point3 { z, ..a };
    let b = Point3 { z, ..b };
    let c = Point3 { z, ..c };
    let normal = normal_for(a, b, c);
    if (top && normal.z < 0.0) || (!top && normal.z > 0.0) {
        push_triangle(triangles, a, c, b);
    } else {
        push_triangle(triangles, a, b, c);
    }
}

fn push_ring_walls(
    triangles: &mut Vec<Triangle>,
    points: &[Point3],
    ring: &[usize],
    z0: f64,
    z1: f64,
    hole: bool,
) {
    if ring.len() < 2 {
        return;
    }
    for index in 0..ring.len() {
        let next = (index + 1) % ring.len();
        let a = points[ring[index]];
        let b = points[ring[next]];
        let a0 = Point3 { z: z0, ..a };
        let b0 = Point3 { z: z0, ..b };
        let a1 = Point3 { z: z1, ..a };
        let b1 = Point3 { z: z1, ..b };
        if hole {
            push_triangle(triangles, b0, a0, a1);
            push_triangle(triangles, b0, a1, b1);
        } else {
            push_triangle(triangles, a0, b0, b1);
            push_triangle(triangles, a0, b1, a1);
        }
    }
}

fn push_triangle(triangles: &mut Vec<Triangle>, a: Point3, b: Point3, c: Point3) {
    triangles.push(Triangle {
        normal: normal_for(a, b, c),
        a,
        b,
        c,
    });
}

fn write_facet(output: &mut String, triangle: Triangle) {
    let a = triangle.a;
    let b = triangle.b;
    let c = triangle.c;
    let normal = triangle.normal;
    output.push_str(&format!(
        "  facet normal {} {} {}\n    outer loop\n      vertex {} {} {}\n      vertex {} {} {}\n      vertex {} {} {}\n    endloop\n  endfacet\n",
        fmt_f64(normal.x),
        fmt_f64(normal.y),
        fmt_f64(normal.z),
        fmt_f64(a.x),
        fmt_f64(a.y),
        fmt_f64(a.z),
        fmt_f64(b.x),
        fmt_f64(b.y),
        fmt_f64(b.z),
        fmt_f64(c.x),
        fmt_f64(c.y),
        fmt_f64(c.z),
    ));
}

fn normal_for(a: Point3, b: Point3, c: Point3) -> Point3 {
    let ux = b.x - a.x;
    let uy = b.y - a.y;
    let uz = b.z - a.z;
    let vx = c.x - a.x;
    let vy = c.y - a.y;
    let vz = c.z - a.z;
    let nx = uy * vz - uz * vy;
    let ny = uz * vx - ux * vz;
    let nz = ux * vy - uy * vx;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if len <= f64::EPSILON {
        return Point3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
    }
    Point3 {
        x: nx / len,
        y: ny / len,
        z: nz / len,
    }
}

fn fmt_f64(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_fixture_exports_to_stl_without_visual_meshes(
        bundle: boon_solid_model::SolidModelBundle,
        expected_hole_mode: ExpectedHoleMode,
    ) {
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_ascii_stl_from_layers(&print);
        let repeat = export_ascii_stl_from_layers(&print);
        let validation = validate_ascii_stl_package(&package);
        let binary = export_binary_stl_from_layers(&print);
        let binary_repeat = export_binary_stl_from_layers(&print);
        let binary_validation = validate_binary_stl_package(&binary);
        let topology = validate_layer_prism_topology(&print);
        let export_topology = validate_export_triangle_stream_topology(&print);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert_eq!(print.metrics.unsupported_operation_count, 0);
        match expected_hole_mode {
            ExpectedHoleMode::NoHoles => assert_eq!(print.metrics.hole_count, 0),
            ExpectedHoleMode::HasHoles => assert!(print.metrics.hole_count > 0),
        }
        assert_eq!(package.status, StlExportStatus::Pass);
        assert_eq!(validation.status, StlExportStatus::Pass);
        assert_eq!(binary.status, StlExportStatus::Pass);
        assert_eq!(binary_validation.status, StlExportStatus::Pass);
        assert_eq!(topology.status, StlExportStatus::Pass, "{topology:?}");
        assert_eq!(
            export_topology.status,
            StlExportStatus::Pass,
            "{export_topology:?}"
        );
        assert_eq!(package.ascii, repeat.ascii);
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(binary.bytes, binary_repeat.bytes);
        assert_eq!(binary.artifact_hash, binary_repeat.artifact_hash);
        assert_eq!(package.metrics.layer_count, print.metrics.layer_count);
        assert_eq!(package.metrics.polygon_count, print.metrics.polygon_count);
        assert_eq!(
            package.metrics.preserved_hole_count,
            print.metrics.hole_count
        );
        assert_eq!(package.metrics.rejected_hole_count, 0);
        assert_eq!(
            binary.metrics.preserved_hole_count,
            print.metrics.hole_count
        );
        assert_eq!(binary.metrics.rejected_hole_count, 0);
        assert_eq!(
            binary.metrics.triangle_count,
            package.metrics.triangle_count
        );
        assert_eq!(
            binary.metrics.byte_count,
            84 + binary.metrics.triangle_count * 50
        );
        assert_eq!(topology.checked_prism_count, print.metrics.polygon_count);
        assert!(topology.triangle_count >= package.metrics.triangle_count);
        assert_eq!(
            export_topology.checked_prism_count,
            print.metrics.polygon_count
        );
        assert_eq!(
            export_topology.triangle_count,
            package.metrics.triangle_count
        );
        assert!(export_topology.raw_triangle_count <= topology.triangle_count);
        assert!(export_topology.raw_triangle_count >= export_topology.triangle_count);
        assert_eq!(topology.boundary_edge_count, 0);
        assert_eq!(topology.non_manifold_edge_count, 0);
        assert_eq!(topology.degenerate_triangle_count, 0);
        assert_eq!(export_topology.boundary_edge_count, 0);
        assert_eq!(export_topology.non_manifold_edge_count, 0);
        assert_eq!(export_topology.degenerate_triangle_count, 0);
        assert_eq!(
            package.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert_eq!(
            binary.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert!(!print.visual_mesh_used_for_manufacturing);
        assert!(!package.visual_mesh_used_for_manufacturing);
        assert!(!binary.visual_mesh_used_for_manufacturing);
    }

    enum ExpectedHoleMode {
        NoHoles,
        HasHoles,
    }

    #[test]
    fn parametric_car_layers_export_to_deterministic_ascii_stl() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_ascii_stl_from_layers(&print);
        let repeat = export_ascii_stl_from_layers(&print);
        let validation = validate_ascii_stl_package(&package);
        let binary = export_binary_stl_from_layers(&print);
        let binary_repeat = export_binary_stl_from_layers(&print);
        let binary_validation = validate_binary_stl_package(&binary);
        let topology = validate_layer_prism_topology(&print);
        let export_topology = validate_export_triangle_stream_topology(&print);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert_eq!(print.metrics.hole_count, 0);
        assert_eq!(package.status, StlExportStatus::Pass);
        assert_eq!(validation.status, StlExportStatus::Pass);
        assert_eq!(binary.status, StlExportStatus::Pass);
        assert_eq!(binary_validation.status, StlExportStatus::Pass);
        assert_eq!(topology.status, StlExportStatus::Pass);
        assert_eq!(
            export_topology.status,
            StlExportStatus::Pass,
            "{export_topology:?}"
        );
        assert_eq!(topology.checked_prism_count, print.metrics.polygon_count);
        assert!(topology.triangle_count >= package.metrics.triangle_count);
        assert_eq!(topology.boundary_edge_count, 0);
        assert_eq!(topology.non_manifold_edge_count, 0);
        assert_eq!(topology.degenerate_triangle_count, 0);
        assert_eq!(
            export_topology.checked_prism_count,
            print.metrics.polygon_count
        );
        assert_eq!(
            export_topology.triangle_count,
            package.metrics.triangle_count
        );
        assert!(export_topology.raw_triangle_count <= topology.triangle_count);
        assert!(export_topology.raw_triangle_count >= export_topology.triangle_count);
        assert_eq!(export_topology.boundary_edge_count, 0);
        assert_eq!(export_topology.non_manifold_edge_count, 0);
        assert_eq!(export_topology.degenerate_triangle_count, 0);
        assert_eq!(package.ascii, repeat.ascii);
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(binary.bytes, binary_repeat.bytes);
        assert_eq!(binary.artifact_hash, binary_repeat.artifact_hash);
        assert_eq!(package.metrics.layer_count, print.metrics.layer_count);
        assert_eq!(package.metrics.polygon_count, print.metrics.polygon_count);
        assert_eq!(package.metrics.preserved_hole_count, 0);
        assert_eq!(package.metrics.rejected_hole_count, 0);
        assert_eq!(
            binary.metrics.triangle_count,
            package.metrics.triangle_count
        );
        assert_eq!(binary.metrics.preserved_hole_count, 0);
        assert_eq!(binary.metrics.rejected_hole_count, 0);
        assert_eq!(
            binary.metrics.byte_count,
            84 + binary.metrics.triangle_count * 50
        );
        assert!(package.metrics.triangle_count > print.metrics.polygon_count);
        assert!(package.metrics.byte_count > 0);
        assert!(binary.metrics.byte_count > 84);
        assert!(package.artifact_hash.starts_with("sha256:"));
        assert!(binary.artifact_hash.starts_with("sha256:"));
        assert_eq!(
            package.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert_eq!(
            binary.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert!(!package.visual_mesh_used_for_manufacturing);
        assert!(!binary.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn box_intersection_layers_export_to_stl_without_visual_meshes() {
        let bundle = boon_solid_model::SolidModelBundle::box_intersection_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_ascii_stl_from_layers(&print);
        let repeat = export_ascii_stl_from_layers(&print);
        let validation = validate_ascii_stl_package(&package);
        let binary = export_binary_stl_from_layers(&print);
        let binary_repeat = export_binary_stl_from_layers(&print);
        let binary_validation = validate_binary_stl_package(&binary);
        let topology = validate_layer_prism_topology(&print);
        let export_topology = validate_export_triangle_stream_topology(&print);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert_eq!(print.metrics.unsupported_operation_count, 0);
        assert_eq!(print.metrics.hole_count, 0);
        assert_eq!(package.status, StlExportStatus::Pass);
        assert_eq!(validation.status, StlExportStatus::Pass);
        assert_eq!(binary.status, StlExportStatus::Pass);
        assert_eq!(binary_validation.status, StlExportStatus::Pass);
        assert_eq!(topology.status, StlExportStatus::Pass);
        assert_eq!(export_topology.status, StlExportStatus::Pass);
        assert_eq!(package.ascii, repeat.ascii);
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(binary.bytes, binary_repeat.bytes);
        assert_eq!(binary.artifact_hash, binary_repeat.artifact_hash);
        assert_eq!(package.metrics.layer_count, print.metrics.layer_count);
        assert_eq!(package.metrics.polygon_count, print.metrics.polygon_count);
        assert_eq!(package.metrics.preserved_hole_count, 0);
        assert_eq!(binary.metrics.preserved_hole_count, 0);
        assert_eq!(
            binary.metrics.triangle_count,
            package.metrics.triangle_count
        );
        assert_eq!(
            export_topology.triangle_count,
            package.metrics.triangle_count
        );
        assert_eq!(
            package.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert_eq!(
            binary.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert!(!print.visual_mesh_used_for_manufacturing);
        assert!(!package.visual_mesh_used_for_manufacturing);
        assert!(!binary.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn shell_box_layers_export_to_stl_without_visual_meshes() {
        assert_fixture_exports_to_stl_without_visual_meshes(
            boon_solid_model::SolidModelBundle::shell_box_fixture(),
            ExpectedHoleMode::HasHoles,
        );
    }

    #[test]
    fn extruded_rectangle_layers_export_to_stl_without_visual_meshes() {
        assert_fixture_exports_to_stl_without_visual_meshes(
            boon_solid_model::SolidModelBundle::extruded_rectangle_fixture(),
            ExpectedHoleMode::NoHoles,
        );
    }

    #[test]
    fn revolved_ring_layers_export_to_stl_without_visual_meshes() {
        assert_fixture_exports_to_stl_without_visual_meshes(
            boon_solid_model::SolidModelBundle::revolved_ring_fixture(),
            ExpectedHoleMode::HasHoles,
        );
    }

    #[test]
    fn box_slot_difference_layers_export_to_hole_preserving_stl_without_visual_meshes() {
        let bundle = boon_solid_model::SolidModelBundle::box_slot_difference_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_ascii_stl_from_layers(&print);
        let repeat = export_ascii_stl_from_layers(&print);
        let validation = validate_ascii_stl_package(&package);
        let binary = export_binary_stl_from_layers(&print);
        let binary_repeat = export_binary_stl_from_layers(&print);
        let binary_validation = validate_binary_stl_package(&binary);
        let topology = validate_layer_prism_topology(&print);
        let export_topology = validate_export_triangle_stream_topology(&print);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert_eq!(print.metrics.unsupported_operation_count, 0);
        assert_eq!(print.metrics.polygon_count, print.metrics.layer_count);
        assert_eq!(print.metrics.hole_count, print.metrics.layer_count);
        assert_eq!(package.status, StlExportStatus::Pass);
        assert_eq!(validation.status, StlExportStatus::Pass);
        assert_eq!(binary.status, StlExportStatus::Pass);
        assert_eq!(binary_validation.status, StlExportStatus::Pass);
        assert_eq!(topology.status, StlExportStatus::Pass);
        assert_eq!(export_topology.status, StlExportStatus::Pass);
        assert_eq!(package.ascii, repeat.ascii);
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(binary.bytes, binary_repeat.bytes);
        assert_eq!(binary.artifact_hash, binary_repeat.artifact_hash);
        assert_eq!(package.metrics.layer_count, print.metrics.layer_count);
        assert_eq!(package.metrics.polygon_count, print.metrics.polygon_count);
        assert_eq!(
            package.metrics.preserved_hole_count,
            print.metrics.hole_count
        );
        assert_eq!(package.metrics.rejected_hole_count, 0);
        assert_eq!(
            binary.metrics.preserved_hole_count,
            print.metrics.hole_count
        );
        assert_eq!(binary.metrics.rejected_hole_count, 0);
        assert_eq!(
            binary.metrics.triangle_count,
            package.metrics.triangle_count
        );
        assert_eq!(
            export_topology.triangle_count,
            package.metrics.triangle_count
        );
        assert_eq!(topology.checked_prism_count, print.metrics.polygon_count);
        assert_eq!(
            export_topology.checked_prism_count,
            print.metrics.polygon_count
        );
        assert_eq!(topology.boundary_edge_count, 0);
        assert_eq!(topology.non_manifold_edge_count, 0);
        assert_eq!(topology.degenerate_triangle_count, 0);
        assert_eq!(export_topology.boundary_edge_count, 0);
        assert_eq!(export_topology.non_manifold_edge_count, 0);
        assert_eq!(export_topology.degenerate_triangle_count, 0);
        assert_eq!(
            package.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert_eq!(
            binary.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert!(!print.visual_mesh_used_for_manufacturing);
        assert!(!package.visual_mesh_used_for_manufacturing);
        assert!(!binary.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn hole_bearing_layers_export_with_preserved_hole_walls() {
        let bundle = boon_solid_model::SolidModelBundle::printable_bracket_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_ascii_stl_from_layers(&print);
        let repeat = export_ascii_stl_from_layers(&print);
        let validation = validate_ascii_stl_package(&package);
        let binary = export_binary_stl_from_layers(&print);
        let binary_repeat = export_binary_stl_from_layers(&print);
        let binary_validation = validate_binary_stl_package(&binary);
        let topology = validate_layer_prism_topology(&print);
        let export_topology = validate_export_triangle_stream_topology(&print);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert!(print.metrics.hole_count > 0);
        assert_eq!(package.status, StlExportStatus::Pass);
        assert_eq!(validation.status, StlExportStatus::Pass);
        assert_eq!(binary.status, StlExportStatus::Pass);
        assert_eq!(binary_validation.status, StlExportStatus::Pass);
        assert_eq!(topology.status, StlExportStatus::Pass);
        assert_eq!(
            export_topology.status,
            StlExportStatus::Pass,
            "{export_topology:?}"
        );
        assert_eq!(topology.checked_prism_count, print.metrics.polygon_count);
        assert!(topology.triangle_count >= package.metrics.triangle_count);
        assert_eq!(topology.boundary_edge_count, 0);
        assert_eq!(topology.non_manifold_edge_count, 0);
        assert_eq!(topology.degenerate_triangle_count, 0);
        assert_eq!(
            export_topology.checked_prism_count,
            print.metrics.polygon_count
        );
        assert_eq!(
            export_topology.triangle_count,
            package.metrics.triangle_count
        );
        assert!(export_topology.raw_triangle_count <= topology.triangle_count);
        assert!(export_topology.raw_triangle_count >= export_topology.triangle_count);
        assert_eq!(export_topology.boundary_edge_count, 0);
        assert_eq!(export_topology.non_manifold_edge_count, 0);
        assert_eq!(export_topology.degenerate_triangle_count, 0);
        assert_eq!(package.ascii, repeat.ascii);
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(binary.bytes, binary_repeat.bytes);
        assert_eq!(binary.artifact_hash, binary_repeat.artifact_hash);
        assert_eq!(
            package.metrics.preserved_hole_count,
            print.metrics.hole_count
        );
        assert_eq!(
            binary.metrics.preserved_hole_count,
            print.metrics.hole_count
        );
        assert_eq!(package.metrics.rejected_hole_count, 0);
        assert_eq!(binary.metrics.rejected_hole_count, 0);
        assert_eq!(
            binary.metrics.triangle_count,
            package.metrics.triangle_count
        );
        assert_eq!(
            binary.metrics.byte_count,
            84 + binary.metrics.triangle_count * 50
        );
        assert!(package.metrics.triangle_count > print.metrics.polygon_count);
        assert!(package.metrics.byte_count > 0);
        assert!(binary.metrics.byte_count > 84);
        assert_eq!(
            package.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert_eq!(
            binary.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert!(!package.visual_mesh_used_for_manufacturing);
        assert!(!binary.visual_mesh_used_for_manufacturing);
    }
}
