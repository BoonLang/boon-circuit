use std::collections::BTreeMap;

use boon_manufacturing::{
    ConnectorPrintOutput, ConnectorPrintOutputStatus, ManufacturingCompileStatus, MaterialRegion2D,
    PolygonWithHoles, PrintCompileOutput, PrintCompileRequest, PrintPreparationArtifact,
    SplitPrintOutput, SplitPrintOutputStatus, SplitPrintSegmentOutput,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreeMfExportStatus {
    Pass,
    Fail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreeMfDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreeMfValidationStatus {
    Pass,
    Fail,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreeMfValidationMetrics {
    pub zip_entry_count: usize,
    pub required_entry_count: usize,
    pub required_entry_present_count: usize,
    pub crc_checked_count: usize,
    pub content_type_status: bool,
    pub relationship_status: bool,
    pub model_units_status: bool,
    pub material_metadata_status: bool,
    pub model_mesh_status: bool,
    pub model_mesh_reference_status: bool,
    pub slice_metadata_status: bool,
    pub preparation_metadata_status: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreeMfValidationReport {
    pub status: ThreeMfValidationStatus,
    pub diagnostics: Vec<ThreeMfDiagnostic>,
    pub metrics: ThreeMfValidationMetrics,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreeMfImporterSmokeMetrics {
    pub zip_entry_count: usize,
    pub model_object_count: usize,
    pub build_item_count: usize,
    pub resolved_build_item_count: usize,
    pub material_count: usize,
    pub slice_count: usize,
    pub region_count: usize,
    pub polygon_count: usize,
    pub hole_count: usize,
    pub mesh_vertex_count: usize,
    pub mesh_triangle_count: usize,
    pub mesh_object_count: usize,
    pub mesh_object_with_vertices_count: usize,
    pub mesh_object_with_triangles_count: usize,
    pub mesh_invalid_triangle_reference_count: usize,
    pub mesh_degenerate_triangle_count: usize,
    pub placeholder_mesh_object_count: usize,
    pub source_hash_matches: bool,
    pub preparation_metadata_present: bool,
    pub preparation_metadata_decoded: bool,
    pub preparation_metadata_visual_mesh_status: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreeMfImporterSmokeReport {
    pub status: ThreeMfValidationStatus,
    pub diagnostics: Vec<ThreeMfDiagnostic>,
    pub metrics: ThreeMfImporterSmokeMetrics,
    pub scope: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreeMfEntry {
    pub path: String,
    pub media_type: String,
    pub utf8: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreeMfMetrics {
    pub entry_count: usize,
    pub model_object_count: usize,
    pub component_count: usize,
    pub material_count: usize,
    pub slice_count: usize,
    pub polygon_count: usize,
    pub hole_count: usize,
    pub mesh_vertex_count: usize,
    pub mesh_triangle_count: usize,
    pub placeholder_mesh_object_count: usize,
    pub units_metadata_present: bool,
    pub material_metadata_present: bool,
    pub slice_entry_present: bool,
    pub preparation_metadata_present: bool,
    pub preparation_metadata_hash: Option<String>,
    pub opc_zip_container_present: bool,
    pub opc_zip_entry_count: usize,
    pub opc_zip_byte_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreeMfPackage {
    pub status: ThreeMfExportStatus,
    pub package_status: String,
    pub entries: Vec<ThreeMfEntry>,
    pub diagnostics: Vec<ThreeMfDiagnostic>,
    pub metrics: ThreeMfMetrics,
    pub artifact_hash: String,
    pub opc_zip_hash: String,
    pub opc_zip_bytes: Vec<u8>,
    pub source_manufacturing_artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitThreeMfPackageSet {
    pub status: ThreeMfExportStatus,
    pub package_status: String,
    pub source_split_print_artifact_hash: String,
    pub segment_packages: Vec<SplitThreeMfSegmentPackage>,
    pub diagnostics: Vec<ThreeMfDiagnostic>,
    pub artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitThreeMfSegmentPackage {
    pub segment_index: usize,
    pub segment_artifact_hash: String,
    pub connector_cutout_hole_count: usize,
    pub segment_hole_count: usize,
    pub package: ThreeMfPackage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreparedSplitConnectorThreeMfPackageSet {
    pub status: ThreeMfExportStatus,
    pub package_status: String,
    pub source_split_print_artifact_hash: String,
    pub source_connector_print_artifact_hash: String,
    pub segment_packages: Vec<SplitThreeMfSegmentPackage>,
    pub connector_package: ThreeMfPackage,
    pub diagnostics: Vec<ThreeMfDiagnostic>,
    pub artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ZipEntryReadback {
    path: String,
    data: Vec<u8>,
    crc32: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ModelMesh {
    vertices: Vec<MeshVertex>,
    triangles: Vec<MeshTriangle>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MeshVertex {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MeshTriangle {
    a: usize,
    b: usize,
    c: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct VertexKey(String, String, String);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ModelMeshReadbackStats {
    object_count: usize,
    object_with_vertices_count: usize,
    object_with_triangles_count: usize,
    vertex_count: usize,
    triangle_count: usize,
    invalid_triangle_reference_count: usize,
    degenerate_triangle_count: usize,
}

pub fn export_3mf_entry_set(print: &PrintCompileOutput) -> ThreeMfPackage {
    export_3mf_entry_set_with_preparation(print, None)
}

pub fn export_connector_3mf_entry_set(
    connector: &ConnectorPrintOutput,
    request: &PrintCompileRequest,
) -> ThreeMfPackage {
    export_connector_3mf_entry_set_with_preparation(connector, request, None)
}

pub fn export_connector_3mf_entry_set_with_preparation(
    connector: &ConnectorPrintOutput,
    request: &PrintCompileRequest,
    preparation: Option<&PrintPreparationArtifact>,
) -> ThreeMfPackage {
    let print = PrintCompileOutput {
        status: match connector.status {
            ConnectorPrintOutputStatus::Pass => ManufacturingCompileStatus::Pass,
            ConnectorPrintOutputStatus::Fail => ManufacturingCompileStatus::Fail,
        },
        request: request.clone(),
        layers: connector.layers.clone(),
        diagnostics: connector.diagnostics.clone(),
        metrics: connector.metrics.clone(),
        tolerance: connector.tolerance.clone(),
        artifact_hash: connector.artifact_hash.clone(),
        visual_mesh_used_for_manufacturing: connector.visual_mesh_used_for_manufacturing,
    };
    export_3mf_entry_set_with_preparation(&print, preparation)
}

pub fn export_3mf_entry_set_with_preparation(
    print: &PrintCompileOutput,
    preparation: Option<&PrintPreparationArtifact>,
) -> ThreeMfPackage {
    let mut diagnostics = Vec::new();
    if print.status != ManufacturingCompileStatus::Pass {
        diagnostics.push(ThreeMfDiagnostic {
            code: "manufacturing-output-not-pass".to_owned(),
            message: "3MF export requires a passing manufacturing layer output".to_owned(),
        });
    }
    if print.visual_mesh_used_for_manufacturing {
        diagnostics.push(ThreeMfDiagnostic {
            code: "visual-mesh-source-rejected".to_owned(),
            message: "3MF export must not use visual mesh output as manufacturing source"
                .to_owned(),
        });
    }
    if print.layers.is_empty() {
        diagnostics.push(ThreeMfDiagnostic {
            code: "missing-slices".to_owned(),
            message: "3MF export requires at least one manufacturing layer".to_owned(),
        });
    }

    let mut entries = Vec::new();
    if diagnostics.is_empty() {
        entries.push(ThreeMfEntry {
            path: "[Content_Types].xml".to_owned(),
            media_type: "application/xml".to_owned(),
            utf8: content_types_xml(),
        });
        entries.push(ThreeMfEntry {
            path: "_rels/.rels".to_owned(),
            media_type: "application/vnd.openxmlformats-package.relationships+xml".to_owned(),
            utf8: relationships_xml(),
        });
        entries.push(ThreeMfEntry {
            path: "3D/3dmodel.model".to_owned(),
            media_type: "application/vnd.ms-package.3dmanufacturing-3dmodel+xml".to_owned(),
            utf8: model_xml(print),
        });
        entries.push(ThreeMfEntry {
            path: "3D/Slices/boon-slices.xml".to_owned(),
            media_type: "application/vnd.boon.3dmanufacturing-slices+xml".to_owned(),
            utf8: slices_xml(print),
        });
        if let Some(preparation) = preparation {
            entries.push(ThreeMfEntry {
                path: "Metadata/boon-print-preparation.json".to_owned(),
                media_type: "application/vnd.boon.print-preparation+json".to_owned(),
                utf8: preparation_metadata_json(preparation),
            });
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let opc_zip_bytes = if diagnostics.is_empty() {
        write_stored_zip(&entries)
    } else {
        Vec::new()
    };
    let metrics = metrics_for(print, &entries, &opc_zip_bytes);
    let status = if diagnostics.is_empty() {
        ThreeMfExportStatus::Pass
    } else {
        ThreeMfExportStatus::Fail
    };
    let artifact_hash = three_mf_artifact_hash(&entries);
    let opc_zip_hash = sha256_bytes(&opc_zip_bytes);
    ThreeMfPackage {
        status,
        package_status: "deterministic-opc-zip-package-foundation".to_owned(),
        entries,
        diagnostics,
        metrics,
        artifact_hash,
        opc_zip_hash,
        opc_zip_bytes,
        source_manufacturing_artifact_hash: print.artifact_hash.clone(),
        visual_mesh_used_for_manufacturing: print.visual_mesh_used_for_manufacturing,
    }
}

pub fn export_split_3mf_entry_sets(
    print: &PrintCompileOutput,
    split: &SplitPrintOutput,
) -> SplitThreeMfPackageSet {
    export_split_3mf_entry_sets_with_preparation(print, split, None)
}

pub fn export_split_3mf_entry_sets_with_preparation(
    print: &PrintCompileOutput,
    split: &SplitPrintOutput,
    preparation: Option<&PrintPreparationArtifact>,
) -> SplitThreeMfPackageSet {
    let mut diagnostics = Vec::new();
    if split.status != SplitPrintOutputStatus::Pass {
        diagnostics.push(ThreeMfDiagnostic {
            code: "split-output-not-pass".to_owned(),
            message: "split 3MF export requires a passing split print output".to_owned(),
        });
    }
    if split.source_manufacturing_artifact_hash != print.artifact_hash {
        diagnostics.push(ThreeMfDiagnostic {
            code: "split-source-hash-mismatch".to_owned(),
            message: "split 3MF export source hash does not match print output".to_owned(),
        });
    }
    if split.visual_mesh_used_for_manufacturing {
        diagnostics.push(ThreeMfDiagnostic {
            code: "visual-mesh-source-rejected".to_owned(),
            message: "split 3MF export must not use visual mesh output as manufacturing source"
                .to_owned(),
        });
    }
    if split.segments.is_empty() {
        diagnostics.push(ThreeMfDiagnostic {
            code: "missing-split-segments".to_owned(),
            message: "split 3MF export requires at least one split segment".to_owned(),
        });
    }

    let segment_packages = if diagnostics.is_empty() {
        split
            .segments
            .iter()
            .map(|segment| export_split_3mf_segment(print, segment, preparation))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    for segment_package in &segment_packages {
        if segment_package.package.status != ThreeMfExportStatus::Pass {
            diagnostics.push(ThreeMfDiagnostic {
                code: "split-segment-package-not-pass".to_owned(),
                message: format!(
                    "split segment {} did not export as a passing 3MF package",
                    segment_package.segment_index
                ),
            });
        }
        if segment_package.package.metrics.hole_count < segment_package.connector_cutout_hole_count
        {
            diagnostics.push(ThreeMfDiagnostic {
                code: "split-segment-cutout-holes-not-preserved".to_owned(),
                message: format!(
                    "split segment {} 3MF package carries {} holes but declares {} connector cutout holes",
                    segment_package.segment_index,
                    segment_package.package.metrics.hole_count,
                    segment_package.connector_cutout_hole_count
                ),
            });
        }
    }
    let status = if diagnostics.is_empty() {
        ThreeMfExportStatus::Pass
    } else {
        ThreeMfExportStatus::Fail
    };
    let artifact_hash = split_three_mf_artifact_hash(split, &segment_packages);
    SplitThreeMfPackageSet {
        status,
        package_status: "deterministic-split-opc-zip-package-set-foundation".to_owned(),
        source_split_print_artifact_hash: split.artifact_hash.clone(),
        segment_packages,
        diagnostics,
        artifact_hash,
        visual_mesh_used_for_manufacturing: split.visual_mesh_used_for_manufacturing,
    }
}

pub fn export_prepared_split_connector_3mf_package_set(
    print: &PrintCompileOutput,
    split: &SplitPrintOutput,
    connector: &ConnectorPrintOutput,
    request: &PrintCompileRequest,
    preparation: &PrintPreparationArtifact,
) -> PreparedSplitConnectorThreeMfPackageSet {
    let split_packages =
        export_split_3mf_entry_sets_with_preparation(print, split, Some(preparation));
    let connector_package =
        export_connector_3mf_entry_set_with_preparation(connector, request, Some(preparation));
    let mut diagnostics = Vec::new();
    diagnostics.extend(split_packages.diagnostics.clone());
    diagnostics.extend(connector_package.diagnostics.clone());

    if split_packages.status != ThreeMfExportStatus::Pass {
        diagnostics.push(ThreeMfDiagnostic {
            code: "prepared-split-package-set-not-pass".to_owned(),
            message:
                "prepared split+connector 3MF package set requires passing split segment packages"
                    .to_owned(),
        });
    }
    if connector_package.status != ThreeMfExportStatus::Pass {
        diagnostics.push(ThreeMfDiagnostic {
            code: "prepared-connector-package-not-pass".to_owned(),
            message:
                "prepared split+connector 3MF package set requires a passing connector package"
                    .to_owned(),
        });
    }
    if connector.source_preparation_artifact_hash != preparation.artifact_hash {
        diagnostics.push(ThreeMfDiagnostic {
            code: "connector-preparation-hash-mismatch".to_owned(),
            message:
                "connector package source preparation hash does not match preparation artifact"
                    .to_owned(),
        });
    }
    if preparation.visual_mesh_used_for_manufacturing {
        diagnostics.push(ThreeMfDiagnostic {
            code: "visual-mesh-source-rejected".to_owned(),
            message: "prepared split+connector package set must not use visual mesh output as manufacturing source"
                .to_owned(),
        });
    }

    let status = if diagnostics.is_empty() {
        ThreeMfExportStatus::Pass
    } else {
        ThreeMfExportStatus::Fail
    };
    let artifact_hash = prepared_split_connector_artifact_hash(
        split,
        connector,
        &split_packages,
        &connector_package,
    );
    let visual_mesh_used_for_manufacturing = split_packages.visual_mesh_used_for_manufacturing
        || connector_package.visual_mesh_used_for_manufacturing
        || preparation.visual_mesh_used_for_manufacturing;

    PreparedSplitConnectorThreeMfPackageSet {
        status,
        package_status: "deterministic-prepared-split-connector-package-set".to_owned(),
        source_split_print_artifact_hash: split.artifact_hash.clone(),
        source_connector_print_artifact_hash: connector.artifact_hash.clone(),
        segment_packages: split_packages.segment_packages,
        connector_package,
        diagnostics,
        artifact_hash,
        visual_mesh_used_for_manufacturing,
    }
}

pub fn validate_3mf_package(package: &ThreeMfPackage) -> ThreeMfValidationReport {
    let mut diagnostics = Vec::new();
    if package.status != ThreeMfExportStatus::Pass {
        diagnostics.push(ThreeMfDiagnostic {
            code: "package-export-not-pass".to_owned(),
            message: "3MF validation requires a passing exported package".to_owned(),
        });
    }
    let readback = match read_stored_zip(&package.opc_zip_bytes) {
        Ok(entries) => entries,
        Err(message) => {
            diagnostics.push(ThreeMfDiagnostic {
                code: "zip-readback".to_owned(),
                message,
            });
            Vec::new()
        }
    };
    let entry_map = readback
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let required = [
        "[Content_Types].xml",
        "_rels/.rels",
        "3D/3dmodel.model",
        "3D/Slices/boon-slices.xml",
    ];
    let mut required_entry_present_count = 0;
    for path in required {
        if entry_map.contains_key(path) {
            required_entry_present_count += 1;
        } else {
            diagnostics.push(ThreeMfDiagnostic {
                code: "missing-required-entry".to_owned(),
                message: format!("3MF package is missing `{path}`"),
            });
        }
    }

    let content_type_status = text_entry(&entry_map, "[Content_Types].xml").is_some_and(|text| {
        text.contains("application/vnd.ms-package.3dmanufacturing-3dmodel+xml")
            && text.contains("application/vnd.boon.3dmanufacturing-slices+xml")
    });
    let relationship_status = text_entry(&entry_map, "_rels/.rels")
        .is_some_and(|text| text.contains("Target=\"/3D/3dmodel.model\""));
    let model_units_status = text_entry(&entry_map, "3D/3dmodel.model")
        .is_some_and(|text| text.contains("unit=\"millimeter\""));
    let material_metadata_status = text_entry(&entry_map, "3D/3dmodel.model")
        .is_some_and(|text| text.contains("<basematerials") && text.contains("physical-material-"));
    let model_mesh_status = text_entry(&entry_map, "3D/3dmodel.model").is_some_and(|text| {
        count_tag(text, "vertex") > 0
            && count_tag(text, "triangle") > 0
            && count_occurrences(text, "<mesh><vertices/><triangles/></mesh>") == 0
    });
    let model_mesh_readback = text_entry(&entry_map, "3D/3dmodel.model")
        .map(model_mesh_readback_stats)
        .unwrap_or_default();
    let model_mesh_reference_status = model_mesh_readback.object_count > 0
        && model_mesh_readback.object_with_vertices_count == model_mesh_readback.object_count
        && model_mesh_readback.object_with_triangles_count == model_mesh_readback.object_count
        && model_mesh_readback.vertex_count > 0
        && model_mesh_readback.triangle_count > 0
        && model_mesh_readback.invalid_triangle_reference_count == 0
        && model_mesh_readback.degenerate_triangle_count == 0;
    let slice_metadata_status =
        text_entry(&entry_map, "3D/Slices/boon-slices.xml").is_some_and(|text| {
            text.contains("<boonSlices")
                && text.contains("<slice")
                && text.contains("<region")
                && text.contains("<outer>")
        });
    let preparation_metadata_status =
        text_entry(&entry_map, "Metadata/boon-print-preparation.json").is_none_or(|text| {
            text.contains("\"artifact_hash\"")
                && text.contains("\"visual_mesh_used_for_manufacturing\":false")
        });
    for (ok, code, message) in [
        (
            content_type_status,
            "content-types",
            "3MF package lacks required content type declarations",
        ),
        (
            relationship_status,
            "relationships",
            "3MF package lacks model relationship target",
        ),
        (
            model_units_status,
            "model-units",
            "3MF model lacks millimetre unit metadata",
        ),
        (
            material_metadata_status,
            "model-materials",
            "3MF model lacks material metadata",
        ),
        (
            model_mesh_status,
            "model-mesh-payload",
            "3MF model lacks concrete mesh vertices/triangles or still contains placeholder meshes",
        ),
        (
            model_mesh_reference_status,
            "model-mesh-references",
            "3MF model mesh has missing per-object vertices/triangles, invalid triangle references, or degenerate triangles",
        ),
        (
            slice_metadata_status,
            "slice-metadata",
            "3MF slice entry lacks slice, region, or polygon metadata",
        ),
        (
            preparation_metadata_status,
            "preparation-metadata",
            "3MF preparation metadata lacks artifact hash or visual-mesh-source marker",
        ),
    ] {
        if !ok {
            diagnostics.push(ThreeMfDiagnostic {
                code: code.to_owned(),
                message: message.to_owned(),
            });
        }
    }

    ThreeMfValidationReport {
        status: if diagnostics.is_empty() {
            ThreeMfValidationStatus::Pass
        } else {
            ThreeMfValidationStatus::Fail
        },
        diagnostics,
        metrics: ThreeMfValidationMetrics {
            zip_entry_count: readback.len(),
            required_entry_count: required.len(),
            required_entry_present_count,
            crc_checked_count: readback.len(),
            content_type_status,
            relationship_status,
            model_units_status,
            material_metadata_status,
            model_mesh_status,
            model_mesh_reference_status,
            slice_metadata_status,
            preparation_metadata_status,
        },
    }
}

pub fn import_3mf_package_smoke(package: &ThreeMfPackage) -> ThreeMfImporterSmokeReport {
    let mut diagnostics = Vec::new();
    if package.status != ThreeMfExportStatus::Pass {
        diagnostics.push(ThreeMfDiagnostic {
            code: "package-export-not-pass".to_owned(),
            message: "3MF importer smoke requires a passing exported package".to_owned(),
        });
    }
    let readback = match read_stored_zip(&package.opc_zip_bytes) {
        Ok(entries) => entries,
        Err(message) => {
            diagnostics.push(ThreeMfDiagnostic {
                code: "zip-readback".to_owned(),
                message,
            });
            Vec::new()
        }
    };
    let entry_map = readback
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let model_text = text_entry(&entry_map, "3D/3dmodel.model").unwrap_or_default();
    let slices_text = text_entry(&entry_map, "3D/Slices/boon-slices.xml").unwrap_or_default();
    let object_ids = tag_attribute_values(model_text, "object", "id");
    let build_item_ids = tag_attribute_values(model_text, "item", "objectid");
    let resolved_build_item_count = build_item_ids
        .iter()
        .filter(|id| object_ids.contains(id))
        .count();
    let material_count = count_tag(model_text, "base");
    let slice_count = count_tag(slices_text, "slice");
    let region_count = count_tag(slices_text, "region");
    let polygon_count = count_tag(slices_text, "polygon");
    let hole_count = count_tag(slices_text, "hole");
    let mesh_vertex_count = count_tag(model_text, "vertex");
    let mesh_triangle_count = count_tag(model_text, "triangle");
    let model_mesh_readback = model_mesh_readback_stats(model_text);
    let source_hash_matches = tag_attribute_value(slices_text, "boonSlices", "sourceHash")
        .is_some_and(|hash| hash == package.source_manufacturing_artifact_hash);
    let preparation_text = text_entry(&entry_map, "Metadata/boon-print-preparation.json");
    let preparation_metadata_present = preparation_text.is_some();
    let preparation_metadata = preparation_text
        .and_then(|text| serde_json::from_str::<PrintPreparationArtifact>(text).ok());
    let preparation_metadata_decoded =
        !preparation_metadata_present || preparation_metadata.is_some();
    let preparation_metadata_visual_mesh_status = preparation_metadata
        .as_ref()
        .is_none_or(|metadata| !metadata.visual_mesh_used_for_manufacturing);
    let placeholder_mesh_object_count =
        count_occurrences(model_text, "<mesh><vertices/><triangles/></mesh>");

    for (ok, code, message) in [
        (
            model_text
                .contains("xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\""),
            "model-core-namespace",
            "3MF model is missing the core 3MF namespace",
        ),
        (
            model_text.contains("unit=\"millimeter\""),
            "model-units",
            "3MF model is missing millimetre units",
        ),
        (
            !object_ids.is_empty(),
            "model-objects",
            "3MF model does not declare any object IDs",
        ),
        (
            !build_item_ids.is_empty(),
            "build-items",
            "3MF model does not declare any build items",
        ),
        (
            resolved_build_item_count == build_item_ids.len(),
            "build-item-references",
            "3MF build item references do not resolve to model object IDs",
        ),
        (
            material_count == package.metrics.material_count,
            "material-count",
            "3MF material count does not match package metrics",
        ),
        (
            object_ids.len() == package.metrics.model_object_count,
            "object-count",
            "3MF object count does not match package metrics",
        ),
        (
            slice_count == package.metrics.slice_count,
            "slice-count",
            "3MF slice count does not match package metrics",
        ),
        (
            polygon_count == package.metrics.polygon_count,
            "polygon-count",
            "3MF polygon count does not match package metrics",
        ),
        (
            hole_count == package.metrics.hole_count,
            "hole-count",
            "3MF hole count does not match package metrics",
        ),
        (
            mesh_vertex_count == package.metrics.mesh_vertex_count && mesh_vertex_count > 0,
            "mesh-vertex-count",
            "3MF model vertex count does not match package metrics or is empty",
        ),
        (
            mesh_triangle_count == package.metrics.mesh_triangle_count && mesh_triangle_count > 0,
            "mesh-triangle-count",
            "3MF model triangle count does not match package metrics or is empty",
        ),
        (
            placeholder_mesh_object_count == 0,
            "placeholder-meshes",
            "3MF model still contains placeholder mesh objects",
        ),
        (
            model_mesh_readback.object_count == package.metrics.model_object_count
                && model_mesh_readback.object_with_vertices_count
                    == model_mesh_readback.object_count
                && model_mesh_readback.object_with_triangles_count
                    == model_mesh_readback.object_count,
            "mesh-object-content",
            "3MF model mesh objects do not all contain vertices and triangles",
        ),
        (
            model_mesh_readback.invalid_triangle_reference_count == 0,
            "mesh-triangle-references",
            "3MF model mesh contains triangle references outside object-local vertex ranges",
        ),
        (
            model_mesh_readback.degenerate_triangle_count == 0,
            "mesh-degenerate-triangles",
            "3MF model mesh contains degenerate triangles",
        ),
        (
            source_hash_matches,
            "source-hash",
            "3MF slice source hash does not match the package source manufacturing artifact hash",
        ),
        (
            preparation_metadata_decoded,
            "preparation-metadata-json",
            "3MF preparation metadata is present but does not decode as PrintPreparationArtifact",
        ),
        (
            preparation_metadata_visual_mesh_status,
            "preparation-metadata-visual-mesh",
            "3MF preparation metadata claims visual mesh use for manufacturing",
        ),
    ] {
        if !ok {
            diagnostics.push(ThreeMfDiagnostic {
                code: code.to_owned(),
                message: message.to_owned(),
            });
        }
    }

    ThreeMfImporterSmokeReport {
        status: if diagnostics.is_empty() {
            ThreeMfValidationStatus::Pass
        } else {
            ThreeMfValidationStatus::Fail
        },
        diagnostics,
        metrics: ThreeMfImporterSmokeMetrics {
            zip_entry_count: readback.len(),
            model_object_count: object_ids.len(),
            build_item_count: build_item_ids.len(),
            resolved_build_item_count,
            material_count,
            slice_count,
            region_count,
            polygon_count,
            hole_count,
            mesh_vertex_count,
            mesh_triangle_count,
            mesh_object_count: model_mesh_readback.object_count,
            mesh_object_with_vertices_count: model_mesh_readback.object_with_vertices_count,
            mesh_object_with_triangles_count: model_mesh_readback.object_with_triangles_count,
            mesh_invalid_triangle_reference_count: model_mesh_readback
                .invalid_triangle_reference_count,
            mesh_degenerate_triangle_count: model_mesh_readback.degenerate_triangle_count,
            placeholder_mesh_object_count,
            source_hash_matches,
            preparation_metadata_present,
            preparation_metadata_decoded,
            preparation_metadata_visual_mesh_status,
        },
        scope: "deterministic-opc-zip-structure-import-smoke-not-external-conformance".to_owned(),
    }
}

fn export_split_3mf_segment(
    print: &PrintCompileOutput,
    segment: &SplitPrintSegmentOutput,
    preparation: Option<&PrintPreparationArtifact>,
) -> SplitThreeMfSegmentPackage {
    let mut request = print.request.clone();
    request.build_volume = segment.segment.bounds;
    let segment_print = PrintCompileOutput {
        status: ManufacturingCompileStatus::Pass,
        request,
        layers: segment.layers.clone(),
        diagnostics: Vec::new(),
        metrics: segment.metrics.clone(),
        tolerance: segment.tolerance.clone(),
        artifact_hash: segment.artifact_hash.clone(),
        visual_mesh_used_for_manufacturing: false,
    };
    SplitThreeMfSegmentPackage {
        segment_index: segment.segment.index,
        segment_artifact_hash: segment.artifact_hash.clone(),
        connector_cutout_hole_count: segment.connector_cutout_hole_count,
        segment_hole_count: segment.metrics.hole_count,
        package: export_3mf_entry_set_with_preparation(&segment_print, preparation),
    }
}

fn metrics_for(
    print: &PrintCompileOutput,
    entries: &[ThreeMfEntry],
    opc_zip_bytes: &[u8],
) -> ThreeMfMetrics {
    let materials = print
        .layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .map(|region| region.material)
        .collect::<std::collections::BTreeSet<_>>();
    let components = print
        .layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .map(|region| (region.part, region.instance))
        .collect::<std::collections::BTreeSet<_>>();
    let model_text = entries
        .iter()
        .find(|entry| entry.path == "3D/3dmodel.model")
        .map(|entry| entry.utf8.as_str())
        .unwrap_or_default();
    ThreeMfMetrics {
        entry_count: entries.len(),
        model_object_count: components.len(),
        component_count: components.len(),
        material_count: materials.len(),
        slice_count: print.metrics.layer_count,
        polygon_count: print.metrics.polygon_count,
        hole_count: print.metrics.hole_count,
        mesh_vertex_count: count_tag(model_text, "vertex"),
        mesh_triangle_count: count_tag(model_text, "triangle"),
        placeholder_mesh_object_count: count_occurrences(
            model_text,
            "<mesh><vertices/><triangles/></mesh>",
        ),
        units_metadata_present: entries.iter().any(|entry| {
            entry.path == "3D/3dmodel.model" && entry.utf8.contains("unit=\"millimeter\"")
        }),
        material_metadata_present: entries
            .iter()
            .any(|entry| entry.path == "3D/3dmodel.model" && entry.utf8.contains("<basematerials")),
        slice_entry_present: entries
            .iter()
            .any(|entry| entry.path == "3D/Slices/boon-slices.xml"),
        preparation_metadata_present: entries
            .iter()
            .any(|entry| entry.path == "Metadata/boon-print-preparation.json"),
        preparation_metadata_hash: entries
            .iter()
            .find(|entry| entry.path == "Metadata/boon-print-preparation.json")
            .map(|entry| sha256_bytes(entry.utf8.as_bytes())),
        opc_zip_container_present: is_zip_container(opc_zip_bytes),
        opc_zip_entry_count: zip_central_directory_entry_count(opc_zip_bytes).unwrap_or_default(),
        opc_zip_byte_count: opc_zip_bytes.len(),
    }
}

fn content_types_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="json" ContentType="application/json"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
  <Override PartName="/3D/Slices/boon-slices.xml" ContentType="application/vnd.boon.3dmanufacturing-slices+xml"/>
  <Override PartName="/Metadata/boon-print-preparation.json" ContentType="application/vnd.boon.print-preparation+json"/>
</Types>
"#
    .to_owned()
}

fn relationships_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#
    .to_owned()
}

fn model_xml(print: &PrintCompileOutput) -> String {
    let materials = material_ids(print);
    let components = component_ids(print);
    let meshes = model_meshes_by_component(print, &components);
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:boon="https://boon.dev/ns/3mf/2026">
  <metadata name="Boon:packageStatus">deterministic-opc-zip-package-foundation</metadata>
  <metadata name="Boon:source">SolidGraph manufacturing layers</metadata>
  <resources>
"#,
    );
    xml.push_str("    <basematerials id=\"1\">\n");
    for material in &materials {
        xml.push_str(&format!(
            "      <base name=\"physical-material-{}\" displaycolor=\"#7FB8E8FF\"/>\n",
            material.0
        ));
    }
    xml.push_str("    </basematerials>\n");
    for (object_id, (_part, instance)) in components.iter().enumerate() {
        let mesh = meshes.get(object_id).cloned().unwrap_or_default();
        xml.push_str(&format!(
            "    <object id=\"{}\" type=\"model\" pid=\"1\" pindex=\"0\">\n",
            object_id + 1
        ));
        xml.push_str(&format!(
            "      <metadata name=\"Boon:partInstance\">{}</metadata>\n",
            instance.0
        ));
        write_model_mesh_xml(&mut xml, &mesh);
        xml.push_str("    </object>\n");
    }
    xml.push_str("  </resources>\n  <build>\n");
    for object_id in 1..=components.len() {
        xml.push_str(&format!("    <item objectid=\"{}\"/>\n", object_id));
    }
    xml.push_str("  </build>\n</model>\n");
    xml
}

fn model_meshes_by_component(
    print: &PrintCompileOutput,
    components: &[(boon_solid_model::PartId, boon_solid_model::PartInstanceId)],
) -> Vec<ModelMesh> {
    components
        .iter()
        .map(|component| model_mesh_for_component(print, *component))
        .collect()
}

fn model_mesh_for_component(
    print: &PrintCompileOutput,
    component: (boon_solid_model::PartId, boon_solid_model::PartInstanceId),
) -> ModelMesh {
    let mut mesh = ModelMesh::default();
    let mut vertex_map = BTreeMap::<VertexKey, usize>::new();
    for layer in &print.layers {
        let z0 = layer.z - print.request.layer_height * 0.5;
        let z1 = layer.z + print.request.layer_height * 0.5;
        for region in &layer.regions {
            if (region.part, region.instance) != component {
                continue;
            }
            for polygon in &region.polygons {
                push_polygon_prism_mesh(
                    &mut mesh,
                    &mut vertex_map,
                    polygon,
                    print.request.integer_grid,
                    z0,
                    z1,
                );
            }
        }
    }
    mesh
}

fn push_polygon_prism_mesh(
    mesh: &mut ModelMesh,
    vertex_map: &mut BTreeMap<VertexKey, usize>,
    polygon: &PolygonWithHoles,
    grid: f64,
    z0: f64,
    z1: f64,
) {
    if polygon.outer.len() < 3 {
        return;
    }
    let (vertices, hole_indices, rings) = polygon_to_earcut_vertices(polygon, grid);
    let Ok(indices) = earcutr::earcut(&vertices, &hole_indices, 2) else {
        return;
    };
    if indices.is_empty() {
        return;
    }
    let points = vertices
        .chunks_exact(2)
        .map(|coords| MeshVertex {
            x: coords[0],
            y: coords[1],
            z: 0.0,
        })
        .collect::<Vec<_>>();

    for triangle in indices.chunks_exact(3) {
        let a = points[triangle[0]];
        let b = points[triangle[1]];
        let c = points[triangle[2]];
        push_cap_triangle(mesh, vertex_map, a, b, c, z1, true);
        push_cap_triangle(mesh, vertex_map, a, c, b, z0, false);
    }
    if let Some(outer) = rings.first() {
        push_ring_wall_mesh(mesh, vertex_map, &points, outer, z0, z1, false);
    }
    for hole in rings.iter().skip(1) {
        push_ring_wall_mesh(mesh, vertex_map, &points, hole, z0, z1, true);
    }
}

fn polygon_to_earcut_vertices(
    polygon: &PolygonWithHoles,
    grid: f64,
) -> (Vec<f64>, Vec<usize>, Vec<Vec<usize>>) {
    let mut vertices = Vec::new();
    let mut hole_indices = Vec::new();
    let mut rings = Vec::new();
    push_ring_vertices(&polygon.outer, grid, &mut vertices, &mut rings);
    for hole in &polygon.holes {
        hole_indices.push(vertices.len() / 2);
        push_ring_vertices(hole, grid, &mut vertices, &mut rings);
    }
    (vertices, hole_indices, rings)
}

fn push_ring_vertices(
    points: &[boon_manufacturing::GridPoint2D],
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

fn push_cap_triangle(
    mesh: &mut ModelMesh,
    vertex_map: &mut BTreeMap<VertexKey, usize>,
    a: MeshVertex,
    b: MeshVertex,
    c: MeshVertex,
    z: f64,
    top: bool,
) {
    let a = MeshVertex { z, ..a };
    let b = MeshVertex { z, ..b };
    let c = MeshVertex { z, ..c };
    if top {
        push_mesh_triangle(mesh, vertex_map, a, b, c);
    } else {
        push_mesh_triangle(mesh, vertex_map, a, c, b);
    }
}

fn push_ring_wall_mesh(
    mesh: &mut ModelMesh,
    vertex_map: &mut BTreeMap<VertexKey, usize>,
    points: &[MeshVertex],
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
        let a0 = MeshVertex { z: z0, ..a };
        let b0 = MeshVertex { z: z0, ..b };
        let a1 = MeshVertex { z: z1, ..a };
        let b1 = MeshVertex { z: z1, ..b };
        if hole {
            push_mesh_triangle(mesh, vertex_map, b0, a0, a1);
            push_mesh_triangle(mesh, vertex_map, b0, a1, b1);
        } else {
            push_mesh_triangle(mesh, vertex_map, a0, b0, b1);
            push_mesh_triangle(mesh, vertex_map, a0, b1, a1);
        }
    }
}

fn push_mesh_triangle(
    mesh: &mut ModelMesh,
    vertex_map: &mut BTreeMap<VertexKey, usize>,
    a: MeshVertex,
    b: MeshVertex,
    c: MeshVertex,
) {
    let a = mesh_vertex_index(mesh, vertex_map, a);
    let b = mesh_vertex_index(mesh, vertex_map, b);
    let c = mesh_vertex_index(mesh, vertex_map, c);
    if a != b && b != c && a != c {
        mesh.triangles.push(MeshTriangle { a, b, c });
    }
}

fn mesh_vertex_index(
    mesh: &mut ModelMesh,
    vertex_map: &mut BTreeMap<VertexKey, usize>,
    vertex: MeshVertex,
) -> usize {
    let key = vertex_key(vertex);
    if let Some(index) = vertex_map.get(&key) {
        return *index;
    }
    let index = mesh.vertices.len();
    mesh.vertices.push(vertex);
    vertex_map.insert(key, index);
    index
}

fn vertex_key(vertex: MeshVertex) -> VertexKey {
    VertexKey(fmt_f64(vertex.x), fmt_f64(vertex.y), fmt_f64(vertex.z))
}

fn write_model_mesh_xml(xml: &mut String, mesh: &ModelMesh) {
    xml.push_str("      <mesh>\n");
    xml.push_str("        <vertices>\n");
    for vertex in &mesh.vertices {
        xml.push_str(&format!(
            "          <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>\n",
            fmt_f64(vertex.x),
            fmt_f64(vertex.y),
            fmt_f64(vertex.z)
        ));
    }
    xml.push_str("        </vertices>\n");
    xml.push_str("        <triangles>\n");
    for triangle in &mesh.triangles {
        xml.push_str(&format!(
            "          <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>\n",
            triangle.a, triangle.b, triangle.c
        ));
    }
    xml.push_str("        </triangles>\n");
    xml.push_str("      </mesh>\n");
}

fn slices_xml(print: &PrintCompileOutput) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<boonSlices units=\"millimeter\" layerHeight=\"{}\" integerGrid=\"{}\" sourceHash=\"{}\">\n",
        fmt_f64(print.request.layer_height),
        fmt_f64(print.request.integer_grid),
        escape_xml(&print.artifact_hash)
    );
    for layer in &print.layers {
        xml.push_str(&format!(
            "  <slice index=\"{}\" z=\"{}\" achievedError=\"{}\">\n",
            layer.index,
            fmt_f64(layer.z),
            fmt_f64(layer.achieved_error)
        ));
        for region in &layer.regions {
            write_region_xml(&mut xml, region);
        }
        xml.push_str("  </slice>\n");
    }
    xml.push_str("</boonSlices>\n");
    xml
}

fn preparation_metadata_json(preparation: &PrintPreparationArtifact) -> String {
    serde_json::to_string(preparation).unwrap_or_else(|_| "{}".to_owned())
}

fn write_region_xml(xml: &mut String, region: &MaterialRegion2D) {
    xml.push_str(&format!(
        "    <region part=\"{}\" instance=\"{}\" material=\"{}\">\n",
        region.part.0, region.instance.0, region.material.0
    ));
    for polygon in &region.polygons {
        write_polygon_xml(xml, polygon);
    }
    xml.push_str("    </region>\n");
}

fn write_polygon_xml(xml: &mut String, polygon: &PolygonWithHoles) {
    let features = polygon
        .source_features
        .iter()
        .map(|feature| feature.0.to_string())
        .collect::<Vec<_>>()
        .join(",");
    xml.push_str(&format!(
        "      <polygon sourceFeatures=\"{}\">\n",
        escape_xml(&features)
    ));
    xml.push_str(&format!(
        "        <outer>{}</outer>\n",
        point_list(&polygon.outer)
    ));
    for hole in &polygon.holes {
        xml.push_str(&format!("        <hole>{}</hole>\n", point_list(hole)));
    }
    xml.push_str("      </polygon>\n");
}

fn point_list(points: &[boon_manufacturing::GridPoint2D]) -> String {
    points
        .iter()
        .map(|point| format!("{},{}", point.x, point.y))
        .collect::<Vec<_>>()
        .join(" ")
}

fn material_ids(print: &PrintCompileOutput) -> Vec<boon_solid_model::PhysicalMaterialId> {
    print
        .layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .map(|region| region.material)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn component_ids(
    print: &PrintCompileOutput,
) -> Vec<(boon_solid_model::PartId, boon_solid_model::PartInstanceId)> {
    let mut components = BTreeMap::new();
    for region in print.layers.iter().flat_map(|layer| &layer.regions) {
        components.insert((region.part, region.instance), ());
    }
    components.into_keys().collect()
}

fn three_mf_artifact_hash(entries: &[ThreeMfEntry]) -> String {
    let bytes = serde_json::to_vec(entries).unwrap_or_default();
    sha256_bytes(&bytes)
}

fn split_three_mf_artifact_hash(
    split: &SplitPrintOutput,
    packages: &[SplitThreeMfSegmentPackage],
) -> String {
    let bytes = serde_json::to_vec(&(split.artifact_hash.as_str(), packages)).unwrap_or_default();
    sha256_bytes(&bytes)
}

fn prepared_split_connector_artifact_hash(
    split: &SplitPrintOutput,
    connector: &ConnectorPrintOutput,
    split_packages: &SplitThreeMfPackageSet,
    connector_package: &ThreeMfPackage,
) -> String {
    let bytes = serde_json::to_vec(&(
        split.artifact_hash.as_str(),
        connector.artifact_hash.as_str(),
        split_packages,
        connector_package,
    ))
    .unwrap_or_default();
    sha256_bytes(&bytes)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn write_stored_zip(entries: &[ThreeMfEntry]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = u32::try_from(output.len()).expect("3MF ZIP offset exceeds u32");
        let name = entry.path.as_bytes();
        let data = entry.utf8.as_bytes();
        let crc = crc32(data);
        let size = u32::try_from(data.len()).expect("3MF ZIP entry exceeds u32");
        let name_len = u16::try_from(name.len()).expect("3MF ZIP path exceeds u16");

        write_u32(&mut output, 0x0403_4b50);
        write_u16(&mut output, 20);
        write_u16(&mut output, 0);
        write_u16(&mut output, 0);
        write_u16(&mut output, 0);
        write_u16(&mut output, 33);
        write_u32(&mut output, crc);
        write_u32(&mut output, size);
        write_u32(&mut output, size);
        write_u16(&mut output, name_len);
        write_u16(&mut output, 0);
        output.extend_from_slice(name);
        output.extend_from_slice(data);

        write_u32(&mut central, 0x0201_4b50);
        write_u16(&mut central, 20);
        write_u16(&mut central, 20);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 33);
        write_u32(&mut central, crc);
        write_u32(&mut central, size);
        write_u32(&mut central, size);
        write_u16(&mut central, name_len);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u32(&mut central, 0);
        write_u32(&mut central, offset);
        central.extend_from_slice(name);
    }

    let central_offset = u32::try_from(output.len()).expect("3MF central offset exceeds u32");
    let central_size = u32::try_from(central.len()).expect("3MF central size exceeds u32");
    let entry_count = u16::try_from(entries.len()).expect("3MF entry count exceeds u16");
    output.extend_from_slice(&central);
    write_u32(&mut output, 0x0605_4b50);
    write_u16(&mut output, 0);
    write_u16(&mut output, 0);
    write_u16(&mut output, entry_count);
    write_u16(&mut output, entry_count);
    write_u32(&mut output, central_size);
    write_u32(&mut output, central_offset);
    write_u16(&mut output, 0);
    output
}

fn is_zip_container(bytes: &[u8]) -> bool {
    bytes.starts_with(&0x0403_4b50_u32.to_le_bytes())
        && bytes
            .windows(4)
            .any(|window| window == 0x0605_4b50_u32.to_le_bytes())
}

fn zip_central_directory_entry_count(bytes: &[u8]) -> Option<usize> {
    let eocd_position = bytes
        .windows(4)
        .rposition(|window| window == 0x0605_4b50_u32.to_le_bytes())?;
    let count_start = eocd_position.checked_add(10)?;
    Some(u16::from_le_bytes([*bytes.get(count_start)?, *bytes.get(count_start + 1)?]) as usize)
}

fn read_stored_zip(bytes: &[u8]) -> Result<Vec<ZipEntryReadback>, String> {
    if !is_zip_container(bytes) {
        return Err("package bytes are not a ZIP container".to_owned());
    }
    let eocd_position = bytes
        .windows(4)
        .rposition(|window| window == 0x0605_4b50_u32.to_le_bytes())
        .ok_or_else(|| "ZIP end-of-central-directory record is missing".to_owned())?;
    let entry_count = read_u16_at(bytes, eocd_position + 10)? as usize;
    let central_size = read_u32_at(bytes, eocd_position + 12)? as usize;
    let central_offset = read_u32_at(bytes, eocd_position + 16)? as usize;
    if central_offset + central_size > bytes.len() || central_offset + central_size > eocd_position
    {
        return Err("ZIP central directory bounds are invalid".to_owned());
    }
    let mut entries = Vec::new();
    let mut cursor = central_offset;
    for _ in 0..entry_count {
        if read_u32_at(bytes, cursor)? != 0x0201_4b50 {
            return Err("ZIP central directory entry signature is invalid".to_owned());
        }
        let method = read_u16_at(bytes, cursor + 10)?;
        if method != 0 {
            return Err("ZIP entry uses compression; only stored entries are expected".to_owned());
        }
        let crc = read_u32_at(bytes, cursor + 16)?;
        let compressed_size = read_u32_at(bytes, cursor + 20)? as usize;
        let uncompressed_size = read_u32_at(bytes, cursor + 24)? as usize;
        let name_len = read_u16_at(bytes, cursor + 28)? as usize;
        let extra_len = read_u16_at(bytes, cursor + 30)? as usize;
        let comment_len = read_u16_at(bytes, cursor + 32)? as usize;
        let local_offset = read_u32_at(bytes, cursor + 42)? as usize;
        let name_start = cursor + 46;
        let name_end = name_start + name_len;
        let path = std::str::from_utf8(
            bytes
                .get(name_start..name_end)
                .ok_or_else(|| "ZIP central directory name exceeds package bounds".to_owned())?,
        )
        .map_err(|_| "ZIP central directory name is not UTF-8".to_owned())?
        .to_owned();
        let data = read_local_stored_entry(bytes, local_offset, &path)?;
        if data.len() != compressed_size || data.len() != uncompressed_size {
            return Err(format!(
                "ZIP entry `{path}` size does not match central directory"
            ));
        }
        if crc32(&data) != crc {
            return Err(format!("ZIP entry `{path}` CRC does not match"));
        }
        entries.push(ZipEntryReadback {
            path,
            data,
            crc32: crc,
        });
        cursor = name_end + extra_len + comment_len;
    }
    if cursor != central_offset + central_size {
        return Err("ZIP central directory size does not match parsed entries".to_owned());
    }
    Ok(entries)
}

fn read_local_stored_entry(
    bytes: &[u8],
    offset: usize,
    expected_path: &str,
) -> Result<Vec<u8>, String> {
    if read_u32_at(bytes, offset)? != 0x0403_4b50 {
        return Err(format!(
            "ZIP local entry `{expected_path}` has invalid signature"
        ));
    }
    let method = read_u16_at(bytes, offset + 8)?;
    if method != 0 {
        return Err(format!("ZIP local entry `{expected_path}` is compressed"));
    }
    let compressed_size = read_u32_at(bytes, offset + 18)? as usize;
    let uncompressed_size = read_u32_at(bytes, offset + 22)? as usize;
    let name_len = read_u16_at(bytes, offset + 26)? as usize;
    let extra_len = read_u16_at(bytes, offset + 28)? as usize;
    let name_start = offset + 30;
    let name_end = name_start + name_len;
    let path = std::str::from_utf8(
        bytes
            .get(name_start..name_end)
            .ok_or_else(|| "ZIP local entry name exceeds package bounds".to_owned())?,
    )
    .map_err(|_| "ZIP local entry name is not UTF-8".to_owned())?;
    if path != expected_path {
        return Err(format!(
            "ZIP local entry path `{path}` does not match central directory path `{expected_path}`"
        ));
    }
    let data_start = name_end + extra_len;
    let data_end = data_start + compressed_size;
    if compressed_size != uncompressed_size {
        return Err(format!("ZIP local entry `{path}` stored sizes differ"));
    }
    Ok(bytes
        .get(data_start..data_end)
        .ok_or_else(|| format!("ZIP local entry `{path}` data exceeds package bounds"))?
        .to_vec())
}

fn text_entry<'a>(entries: &'a BTreeMap<&str, &ZipEntryReadback>, path: &str) -> Option<&'a str> {
    entries
        .get(path)
        .and_then(|entry| std::str::from_utf8(&entry.data).ok())
}

fn count_tag(text: &str, tag: &str) -> usize {
    count_occurrences(text, &format!("<{tag} "))
        + count_occurrences(text, &format!("<{tag}>"))
        + count_occurrences(text, &format!("<{tag}/"))
}

fn count_occurrences(text: &str, needle: &str) -> usize {
    text.match_indices(needle).count()
}

fn model_mesh_readback_stats(model_text: &str) -> ModelMeshReadbackStats {
    let mut stats = ModelMeshReadbackStats::default();
    let mut rest = model_text;
    while let Some(start) = rest.find("<object ") {
        rest = &rest[start + "<object ".len()..];
        let Some(object_tag_end) = rest.find('>') else {
            break;
        };
        let object_tail = &rest[object_tag_end + 1..];
        let Some(object_end) = object_tail.find("</object>") else {
            break;
        };
        let object_body = &object_tail[..object_end];
        stats.object_count += 1;
        let vertex_count = count_tag(object_body, "vertex");
        let triangle_stats = triangle_readback_stats(object_body, vertex_count);
        stats.vertex_count += vertex_count;
        stats.triangle_count += triangle_stats.triangle_count;
        stats.invalid_triangle_reference_count += triangle_stats.invalid_reference_count;
        stats.degenerate_triangle_count += triangle_stats.degenerate_triangle_count;
        if vertex_count > 0 {
            stats.object_with_vertices_count += 1;
        }
        if triangle_stats.triangle_count > 0 {
            stats.object_with_triangles_count += 1;
        }
        rest = &object_tail[object_end + "</object>".len()..];
    }
    stats
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TriangleReadbackStats {
    triangle_count: usize,
    invalid_reference_count: usize,
    degenerate_triangle_count: usize,
}

fn triangle_readback_stats(object_body: &str, vertex_count: usize) -> TriangleReadbackStats {
    let mut stats = TriangleReadbackStats::default();
    let mut rest = object_body;
    while let Some(start) = rest.find("<triangle ") {
        rest = &rest[start + "<triangle ".len()..];
        let Some(end) = rest.find('>') else {
            break;
        };
        let tag_body = &rest[..end];
        stats.triangle_count += 1;
        let refs = ["v1", "v2", "v3"].map(|attribute| {
            attribute_value(tag_body, attribute).and_then(|value| value.parse::<usize>().ok())
        });
        match refs {
            [Some(a), Some(b), Some(c)] => {
                if a >= vertex_count || b >= vertex_count || c >= vertex_count {
                    stats.invalid_reference_count += 1;
                }
                if a == b || b == c || a == c {
                    stats.degenerate_triangle_count += 1;
                }
            }
            _ => stats.invalid_reference_count += 1,
        }
        rest = &rest[end + 1..];
    }
    stats
}

fn tag_attribute_values(text: &str, tag: &str, attribute: &str) -> Vec<String> {
    let mut values = Vec::new();
    let tag_prefix = format!("<{tag}");
    let mut rest = text;
    while let Some(start) = rest.find(&tag_prefix) {
        rest = &rest[start + tag_prefix.len()..];
        let Some(end) = rest.find('>') else {
            break;
        };
        let tag_body = &rest[..end];
        if let Some(value) = attribute_value(tag_body, attribute) {
            values.push(value.to_owned());
        }
        rest = &rest[end + 1..];
    }
    values
}

fn tag_attribute_value(text: &str, tag: &str, attribute: &str) -> Option<String> {
    tag_attribute_values(text, tag, attribute)
        .into_iter()
        .next()
}

fn attribute_value<'a>(tag_body: &'a str, attribute: &str) -> Option<&'a str> {
    let pattern = format!("{attribute}=\"");
    let value_start = tag_body.find(&pattern)? + pattern.len();
    let value_tail = &tag_body[value_start..];
    let value_end = value_tail.find('"')?;
    Some(&value_tail[..value_end])
}

fn read_u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes([
        *bytes
            .get(offset)
            .ok_or_else(|| "unexpected end of ZIP bytes".to_owned())?,
        *bytes
            .get(offset + 1)
            .ok_or_else(|| "unexpected end of ZIP bytes".to_owned())?,
    ]))
}

fn read_u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes([
        *bytes
            .get(offset)
            .ok_or_else(|| "unexpected end of ZIP bytes".to_owned())?,
        *bytes
            .get(offset + 1)
            .ok_or_else(|| "unexpected end of ZIP bytes".to_owned())?,
        *bytes
            .get(offset + 2)
            .ok_or_else(|| "unexpected end of ZIP bytes".to_owned())?,
        *bytes
            .get(offset + 3)
            .ok_or_else(|| "unexpected end of ZIP bytes".to_owned())?,
    ]))
}

fn write_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn fmt_f64(value: f64) -> String {
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.push('0');
    }
    text
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_export_contains_units_materials_components_and_slices() {
        let bundle = boon_solid_model::SolidModelBundle::printable_bracket_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_3mf_entry_set(&print);
        let repeat = export_3mf_entry_set(&print);

        assert_eq!(package.status, ThreeMfExportStatus::Pass);
        assert_eq!(
            package.package_status,
            "deterministic-opc-zip-package-foundation"
        );
        assert_eq!(package.metrics.entry_count, 4);
        assert!(package.metrics.units_metadata_present);
        assert!(package.metrics.material_metadata_present);
        assert!(package.metrics.slice_entry_present);
        assert!(!package.metrics.preparation_metadata_present);
        assert_eq!(package.metrics.preparation_metadata_hash, None);
        assert!(package.metrics.opc_zip_container_present);
        assert_eq!(
            package.metrics.opc_zip_entry_count,
            package.metrics.entry_count
        );
        assert!(package.metrics.opc_zip_byte_count > 0);
        assert_eq!(package.opc_zip_bytes, repeat.opc_zip_bytes);
        assert!(package.opc_zip_hash.starts_with("sha256:"));
        assert_eq!(package.metrics.component_count, 2);
        assert_eq!(package.metrics.slice_count, print.metrics.layer_count);
        assert_eq!(package.metrics.hole_count, print.metrics.hole_count);
        assert!(package.metrics.mesh_vertex_count > 0);
        assert!(package.metrics.mesh_triangle_count > 0);
        assert_eq!(package.metrics.placeholder_mesh_object_count, 0);
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert!(!package.visual_mesh_used_for_manufacturing);

        let validation = validate_3mf_package(&package);
        let importer = import_3mf_package_smoke(&package);
        assert_eq!(validation.status, ThreeMfValidationStatus::Pass);
        assert_eq!(
            validation.metrics.required_entry_present_count,
            validation.metrics.required_entry_count
        );
        assert_eq!(
            validation.metrics.crc_checked_count,
            package.metrics.entry_count
        );
        assert!(validation.metrics.content_type_status);
        assert!(validation.metrics.relationship_status);
        assert!(validation.metrics.model_units_status);
        assert!(validation.metrics.material_metadata_status);
        assert!(validation.metrics.model_mesh_status);
        assert!(validation.metrics.model_mesh_reference_status);
        assert!(validation.metrics.slice_metadata_status);
        assert!(validation.metrics.preparation_metadata_status);
        assert_eq!(importer.status, ThreeMfValidationStatus::Pass);
        assert_eq!(
            importer.metrics.model_object_count,
            package.metrics.model_object_count
        );
        assert_eq!(
            importer.metrics.build_item_count,
            package.metrics.model_object_count
        );
        assert_eq!(
            importer.metrics.resolved_build_item_count,
            importer.metrics.build_item_count
        );
        assert_eq!(
            importer.metrics.material_count,
            package.metrics.material_count
        );
        assert_eq!(importer.metrics.slice_count, package.metrics.slice_count);
        assert_eq!(
            importer.metrics.polygon_count,
            package.metrics.polygon_count
        );
        assert_eq!(importer.metrics.hole_count, package.metrics.hole_count);
        assert_eq!(
            importer.metrics.mesh_vertex_count,
            package.metrics.mesh_vertex_count
        );
        assert_eq!(
            importer.metrics.mesh_triangle_count,
            package.metrics.mesh_triangle_count
        );
        assert_eq!(
            importer.metrics.mesh_object_count,
            package.metrics.model_object_count
        );
        assert_eq!(
            importer.metrics.mesh_object_with_vertices_count,
            importer.metrics.mesh_object_count
        );
        assert_eq!(
            importer.metrics.mesh_object_with_triangles_count,
            importer.metrics.mesh_object_count
        );
        assert_eq!(importer.metrics.mesh_invalid_triangle_reference_count, 0);
        assert_eq!(importer.metrics.mesh_degenerate_triangle_count, 0);
        assert_eq!(importer.metrics.placeholder_mesh_object_count, 0);
        assert!(importer.metrics.source_hash_matches);
        assert!(!importer.metrics.preparation_metadata_present);
    }

    #[test]
    fn failing_manufacturing_output_is_not_exported() {
        let bundle = boon_solid_model::SolidModelBundle::minimum_feature_negative_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_3mf_entry_set(&print);

        assert_eq!(package.status, ThreeMfExportStatus::Fail);
        assert!(package.entries.is_empty());
        assert!(
            package
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "manufacturing-output-not-pass")
        );

        let validation = validate_3mf_package(&package);
        assert_eq!(validation.status, ThreeMfValidationStatus::Fail);
    }

    #[test]
    fn no_hole_slice_package_still_validates_slice_metadata() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_3mf_entry_set(&print);
        let validation = validate_3mf_package(&package);

        assert_eq!(package.status, ThreeMfExportStatus::Pass);
        assert_eq!(package.metrics.hole_count, 0);
        assert!(package.metrics.mesh_vertex_count > 0);
        assert!(package.metrics.mesh_triangle_count > 0);
        assert_eq!(package.metrics.placeholder_mesh_object_count, 0);
        assert_eq!(validation.status, ThreeMfValidationStatus::Pass);
        assert!(validation.metrics.model_mesh_status);
        assert!(validation.metrics.model_mesh_reference_status);
        assert!(validation.metrics.slice_metadata_status);
    }

    #[test]
    fn box_intersection_layers_export_and_import_without_visual_meshes() {
        let bundle = boon_solid_model::SolidModelBundle::box_intersection_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_3mf_entry_set(&print);
        let repeat = export_3mf_entry_set(&print);
        let validation = validate_3mf_package(&package);
        let importer = import_3mf_package_smoke(&package);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert_eq!(print.metrics.unsupported_operation_count, 0);
        assert_eq!(print.metrics.hole_count, 0);
        assert!(!print.visual_mesh_used_for_manufacturing);
        assert_eq!(package.status, ThreeMfExportStatus::Pass);
        assert_eq!(validation.status, ThreeMfValidationStatus::Pass);
        assert_eq!(importer.status, ThreeMfValidationStatus::Pass);
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(package.opc_zip_hash, repeat.opc_zip_hash);
        assert_eq!(package.opc_zip_bytes, repeat.opc_zip_bytes);
        assert_eq!(package.metrics.slice_count, print.metrics.layer_count);
        assert_eq!(package.metrics.polygon_count, print.metrics.polygon_count);
        assert_eq!(package.metrics.hole_count, print.metrics.hole_count);
        assert!(package.metrics.mesh_vertex_count > 0);
        assert!(package.metrics.mesh_triangle_count > 0);
        assert_eq!(package.metrics.placeholder_mesh_object_count, 0);
        assert_eq!(importer.metrics.placeholder_mesh_object_count, 0);
        assert_eq!(importer.metrics.mesh_invalid_triangle_reference_count, 0);
        assert_eq!(importer.metrics.mesh_degenerate_triangle_count, 0);
        assert!(importer.metrics.source_hash_matches);
        assert!(!package.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn curved_analytic_layer_package_exports_without_visual_meshes() {
        let bundle = boon_solid_model::SolidModelBundle::curved_primitives_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);

        let package = export_3mf_entry_set(&print);
        let validation = validate_3mf_package(&package);
        let importer = import_3mf_package_smoke(&package);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert_eq!(print.metrics.unsupported_operation_count, 0);
        assert!(print.metrics.hole_count > 0);
        assert!(!print.visual_mesh_used_for_manufacturing);
        assert_eq!(package.status, ThreeMfExportStatus::Pass);
        assert_eq!(validation.status, ThreeMfValidationStatus::Pass);
        assert_eq!(importer.status, ThreeMfValidationStatus::Pass);
        assert_eq!(package.metrics.slice_count, print.metrics.layer_count);
        assert_eq!(package.metrics.polygon_count, print.metrics.polygon_count);
        assert_eq!(package.metrics.hole_count, print.metrics.hole_count);
        assert!(package.metrics.mesh_vertex_count > 0);
        assert!(package.metrics.mesh_triangle_count > 0);
        assert_eq!(package.metrics.placeholder_mesh_object_count, 0);
        assert_eq!(importer.metrics.placeholder_mesh_object_count, 0);
        assert_eq!(importer.metrics.mesh_invalid_triangle_reference_count, 0);
        assert_eq!(importer.metrics.mesh_degenerate_triangle_count, 0);
        assert!(importer.metrics.source_hash_matches);
        assert!(!package.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn package_can_embed_deterministic_preparation_metadata() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let print_request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, print_request.clone());
        let mut narrow_request = print_request;
        narrow_request.build_volume = boon_solid_model::Aabb64 {
            min: boon_solid_model::Vec3d::new(-40.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(40.0, 80.0, 40.0),
        };
        let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request);
        let preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);

        let package = export_3mf_entry_set_with_preparation(&print, Some(&preparation_artifact));
        let repeat = export_3mf_entry_set_with_preparation(&print, Some(&preparation_artifact));
        let validation = validate_3mf_package(&package);
        let importer = import_3mf_package_smoke(&package);

        assert_eq!(package.status, ThreeMfExportStatus::Pass);
        assert_eq!(package.metrics.entry_count, 5);
        assert!(package.metrics.preparation_metadata_present);
        assert!(
            package
                .metrics
                .preparation_metadata_hash
                .as_ref()
                .is_some_and(|hash| hash.starts_with("sha256:"))
        );
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(package.opc_zip_hash, repeat.opc_zip_hash);
        assert_eq!(package.opc_zip_bytes, repeat.opc_zip_bytes);
        assert_eq!(
            package.metrics.opc_zip_entry_count,
            package.metrics.entry_count
        );
        assert!(package.entries.iter().any(|entry| {
            entry.path == "Metadata/boon-print-preparation.json"
                && entry.utf8.contains(&preparation_artifact.artifact_hash)
                && entry
                    .utf8
                    .contains("\"visual_mesh_used_for_manufacturing\":false")
        }));
        assert_eq!(validation.status, ThreeMfValidationStatus::Pass);
        assert!(validation.metrics.preparation_metadata_status);
        assert_eq!(importer.status, ThreeMfValidationStatus::Pass);
        assert!(importer.metrics.preparation_metadata_present);
        assert!(importer.metrics.preparation_metadata_decoded);
        assert!(importer.metrics.preparation_metadata_visual_mesh_status);
        assert!(!package.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn importer_smoke_rejects_source_hash_mismatch() {
        let bundle = boon_solid_model::SolidModelBundle::printable_bracket_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);
        let mut package = export_3mf_entry_set(&print);
        package.source_manufacturing_artifact_hash = "sha256:not-the-source".to_owned();

        let importer = import_3mf_package_smoke(&package);

        assert_eq!(importer.status, ThreeMfValidationStatus::Fail);
        assert!(!importer.metrics.source_hash_matches);
        assert!(
            importer
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "source-hash")
        );
    }

    #[test]
    fn importer_smoke_rejects_invalid_model_mesh_triangle_reference() {
        let bundle = boon_solid_model::SolidModelBundle::printable_bracket_fixture();
        let request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, request);
        let mut package = export_3mf_entry_set(&print);
        let model_entry = package
            .entries
            .iter_mut()
            .find(|entry| entry.path == "3D/3dmodel.model")
            .expect("3MF package should contain a model entry");
        model_entry.utf8 = model_entry.utf8.replacen(" v1=\"0\"", " v1=\"999999\"", 1);
        package.opc_zip_bytes = write_stored_zip(&package.entries);
        package.opc_zip_hash = sha256_bytes(&package.opc_zip_bytes);

        let importer = import_3mf_package_smoke(&package);

        assert_eq!(importer.status, ThreeMfValidationStatus::Fail);
        assert!(importer.metrics.mesh_invalid_triangle_reference_count > 0);
        assert!(
            importer
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "mesh-triangle-references")
        );
    }

    #[test]
    fn split_car_exports_deterministic_segment_packages() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let print_request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, print_request.clone());
        let mut narrow_request = print_request;
        narrow_request.build_volume = boon_solid_model::Aabb64 {
            min: boon_solid_model::Vec3d::new(-40.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(40.0, 80.0, 40.0),
        };
        let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request);
        let preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);
        let split = boon_manufacturing::compile_split_print_output(&print, &preparation_artifact);

        let packages = export_split_3mf_entry_sets(&print, &split);
        let repeat = export_split_3mf_entry_sets(&print, &split);

        assert_eq!(packages.status, ThreeMfExportStatus::Pass);
        assert_eq!(
            packages.package_status,
            "deterministic-split-opc-zip-package-set-foundation"
        );
        assert_eq!(
            packages.source_split_print_artifact_hash,
            split.artifact_hash
        );
        assert_eq!(packages.segment_packages.len(), 2);
        assert_eq!(packages.artifact_hash, repeat.artifact_hash);
        assert!(packages.artifact_hash.starts_with("sha256:"));
        assert!(!packages.visual_mesh_used_for_manufacturing);
        for segment_package in &packages.segment_packages {
            assert_eq!(segment_package.package.status, ThreeMfExportStatus::Pass);
            assert_eq!(segment_package.package.metrics.entry_count, 4);
            assert!(segment_package.package.metrics.slice_count > 0);
            assert!(segment_package.package.metrics.polygon_count > 0);
            assert!(segment_package.package.metrics.hole_count > 0);
            assert!(segment_package.connector_cutout_hole_count > 0);
            assert!(
                segment_package.segment_hole_count >= segment_package.connector_cutout_hole_count
            );
            assert!(
                segment_package.package.metrics.hole_count
                    >= segment_package.connector_cutout_hole_count
            );
            assert_eq!(
                segment_package.package.source_manufacturing_artifact_hash,
                segment_package.segment_artifact_hash
            );
            assert!(segment_package.package.opc_zip_hash.starts_with("sha256:"));
            assert_eq!(
                validate_3mf_package(&segment_package.package).status,
                ThreeMfValidationStatus::Pass
            );
        }
    }

    #[test]
    fn split_car_segment_packages_can_embed_preparation_metadata() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let print_request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, print_request.clone());
        let mut narrow_request = print_request;
        narrow_request.build_volume = boon_solid_model::Aabb64 {
            min: boon_solid_model::Vec3d::new(-40.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(40.0, 80.0, 40.0),
        };
        let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request);
        let preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);
        let split = boon_manufacturing::compile_split_print_output(&print, &preparation_artifact);

        let packages = export_split_3mf_entry_sets_with_preparation(
            &print,
            &split,
            Some(&preparation_artifact),
        );
        let repeat = export_split_3mf_entry_sets_with_preparation(
            &print,
            &split,
            Some(&preparation_artifact),
        );

        assert_eq!(packages.status, ThreeMfExportStatus::Pass);
        assert_eq!(packages.segment_packages.len(), 2);
        assert_eq!(packages.artifact_hash, repeat.artifact_hash);
        assert_eq!(
            packages.source_split_print_artifact_hash,
            split.artifact_hash
        );
        assert!(!packages.visual_mesh_used_for_manufacturing);
        for segment_package in &packages.segment_packages {
            assert_eq!(segment_package.package.status, ThreeMfExportStatus::Pass);
            assert_eq!(segment_package.package.metrics.entry_count, 5);
            assert!(segment_package.connector_cutout_hole_count > 0);
            assert!(
                segment_package.segment_hole_count >= segment_package.connector_cutout_hole_count
            );
            assert!(
                segment_package.package.metrics.hole_count
                    >= segment_package.connector_cutout_hole_count
            );
            assert!(segment_package.package.metrics.preparation_metadata_present);
            assert_eq!(
                segment_package.package.metrics.preparation_metadata_hash,
                Some(sha256_bytes(
                    preparation_metadata_json(&preparation_artifact).as_bytes()
                ))
            );
            let validation = validate_3mf_package(&segment_package.package);
            let importer = import_3mf_package_smoke(&segment_package.package);
            assert_eq!(validation.status, ThreeMfValidationStatus::Pass);
            assert!(validation.metrics.preparation_metadata_status);
            assert_eq!(importer.status, ThreeMfValidationStatus::Pass);
            assert!(importer.metrics.preparation_metadata_present);
            assert!(importer.metrics.preparation_metadata_decoded);
            assert!(importer.metrics.preparation_metadata_visual_mesh_status);
            assert_eq!(
                segment_package.package.source_manufacturing_artifact_hash,
                segment_package.segment_artifact_hash
            );
        }
    }

    #[test]
    fn car_connector_output_exports_deterministic_pin_package() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let print_request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let mut narrow_request = print_request;
        narrow_request.build_volume = boon_solid_model::Aabb64 {
            min: boon_solid_model::Vec3d::new(-40.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(40.0, 80.0, 40.0),
        };
        let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request.clone());
        let preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);
        let connector = boon_manufacturing::compile_connector_print_output(
            &preparation_artifact,
            &narrow_request,
        );

        let package = export_connector_3mf_entry_set(&connector, &narrow_request);
        let repeat = export_connector_3mf_entry_set(&connector, &narrow_request);
        let validation = validate_3mf_package(&package);

        assert_eq!(
            connector.status,
            boon_manufacturing::ConnectorPrintOutputStatus::Pass
        );
        assert_eq!(connector.connector_count, 2);
        assert_eq!(package.status, ThreeMfExportStatus::Pass);
        assert_eq!(validation.status, ThreeMfValidationStatus::Pass);
        assert_eq!(package.metrics.component_count, 2);
        assert_eq!(package.metrics.material_count, 1);
        assert_eq!(package.metrics.slice_count, connector.metrics.layer_count);
        assert!(package.metrics.mesh_vertex_count > 0);
        assert!(package.metrics.mesh_triangle_count > 0);
        assert_eq!(package.metrics.placeholder_mesh_object_count, 0);
        assert_eq!(
            package.metrics.polygon_count,
            connector.metrics.polygon_count
        );
        assert_eq!(
            package.source_manufacturing_artifact_hash,
            connector.artifact_hash
        );
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(package.opc_zip_hash, repeat.opc_zip_hash);
        assert_eq!(package.opc_zip_bytes, repeat.opc_zip_bytes);
        assert!(package.artifact_hash.starts_with("sha256:"));
        assert!(!package.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn car_connector_package_can_embed_preparation_metadata() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let print_request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let mut narrow_request = print_request;
        narrow_request.build_volume = boon_solid_model::Aabb64 {
            min: boon_solid_model::Vec3d::new(-40.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(40.0, 80.0, 40.0),
        };
        let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request.clone());
        let preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);
        let connector = boon_manufacturing::compile_connector_print_output(
            &preparation_artifact,
            &narrow_request,
        );

        let package = export_connector_3mf_entry_set_with_preparation(
            &connector,
            &narrow_request,
            Some(&preparation_artifact),
        );
        let repeat = export_connector_3mf_entry_set_with_preparation(
            &connector,
            &narrow_request,
            Some(&preparation_artifact),
        );
        let validation = validate_3mf_package(&package);
        let importer = import_3mf_package_smoke(&package);

        assert_eq!(
            connector.status,
            boon_manufacturing::ConnectorPrintOutputStatus::Pass
        );
        assert_eq!(package.status, ThreeMfExportStatus::Pass);
        assert_eq!(package.metrics.entry_count, 5);
        assert!(package.metrics.preparation_metadata_present);
        assert_eq!(
            package.metrics.preparation_metadata_hash,
            Some(sha256_bytes(
                preparation_metadata_json(&preparation_artifact).as_bytes()
            ))
        );
        assert_eq!(package.artifact_hash, repeat.artifact_hash);
        assert_eq!(package.opc_zip_hash, repeat.opc_zip_hash);
        assert_eq!(package.opc_zip_bytes, repeat.opc_zip_bytes);
        assert_eq!(validation.status, ThreeMfValidationStatus::Pass);
        assert!(validation.metrics.preparation_metadata_status);
        assert_eq!(importer.status, ThreeMfValidationStatus::Pass);
        assert!(importer.metrics.preparation_metadata_present);
        assert!(importer.metrics.preparation_metadata_decoded);
        assert!(importer.metrics.preparation_metadata_visual_mesh_status);
        assert!(!package.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn prepared_split_connector_package_set_carries_segments_and_connector() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let print_request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        let print = boon_manufacturing::compile_print_job(&bundle, print_request.clone());
        let mut narrow_request = print_request;
        narrow_request.build_volume = boon_solid_model::Aabb64 {
            min: boon_solid_model::Vec3d::new(-40.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(40.0, 80.0, 40.0),
        };
        let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request.clone());
        let preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);
        let split = boon_manufacturing::compile_split_print_output(&print, &preparation_artifact);
        let connector = boon_manufacturing::compile_connector_print_output(
            &preparation_artifact,
            &narrow_request,
        );

        let package_set = export_prepared_split_connector_3mf_package_set(
            &print,
            &split,
            &connector,
            &narrow_request,
            &preparation_artifact,
        );
        let repeat = export_prepared_split_connector_3mf_package_set(
            &print,
            &split,
            &connector,
            &narrow_request,
            &preparation_artifact,
        );

        assert_eq!(package_set.status, ThreeMfExportStatus::Pass);
        assert_eq!(
            package_set.package_status,
            "deterministic-prepared-split-connector-package-set"
        );
        assert_eq!(
            package_set.source_split_print_artifact_hash,
            split.artifact_hash
        );
        assert_eq!(
            package_set.source_connector_print_artifact_hash,
            connector.artifact_hash
        );
        assert_eq!(package_set.segment_packages.len(), 2);
        assert_eq!(
            package_set.connector_package.status,
            ThreeMfExportStatus::Pass
        );
        assert_eq!(package_set.connector_package.metrics.component_count, 2);
        assert!(
            package_set
                .connector_package
                .metrics
                .preparation_metadata_present
        );
        assert!(
            package_set
                .connector_package
                .metrics
                .preparation_metadata_hash
                .is_some()
        );
        assert_eq!(
            validate_3mf_package(&package_set.connector_package).status,
            ThreeMfValidationStatus::Pass
        );
        assert!(package_set.segment_packages.iter().all(|segment| {
            segment.package.status == ThreeMfExportStatus::Pass
                && segment.connector_cutout_hole_count > 0
                && segment.segment_hole_count >= segment.connector_cutout_hole_count
                && segment.package.metrics.hole_count > 0
                && segment.package.metrics.hole_count >= segment.connector_cutout_hole_count
                && segment.package.metrics.preparation_metadata_present
                && segment.package.metrics.preparation_metadata_hash.is_some()
                && validate_3mf_package(&segment.package).status == ThreeMfValidationStatus::Pass
        }));
        assert_eq!(package_set.artifact_hash, repeat.artifact_hash);
        assert!(package_set.artifact_hash.starts_with("sha256:"));
        assert!(!package_set.visual_mesh_used_for_manufacturing);
        assert!(package_set.diagnostics.is_empty());
    }

    #[test]
    fn split_bracket_exports_hole_preserving_segment_packages() {
        let bundle = boon_solid_model::SolidModelBundle::printable_bracket_fixture();
        let print = boon_manufacturing::compile_print_job(
            &bundle,
            boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle),
        );
        let mut narrow_request =
            boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        narrow_request.build_volume = boon_solid_model::Aabb64 {
            min: boon_solid_model::Vec3d::new(-20.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(20.0, 80.0, 80.0),
        };
        let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request);
        let preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);
        let split = boon_manufacturing::compile_split_print_output(&print, &preparation_artifact);

        let packages = export_split_3mf_entry_sets(&print, &split);

        assert_eq!(
            split.status,
            boon_manufacturing::SplitPrintOutputStatus::Pass
        );
        assert_eq!(packages.status, ThreeMfExportStatus::Pass);
        assert_eq!(packages.segment_packages.len(), 2);
        assert!(packages.segment_packages.iter().all(|segment_package| {
            segment_package.package.metrics.hole_count > 0
                && validate_3mf_package(&segment_package.package).status
                    == ThreeMfValidationStatus::Pass
        }));
        assert!(!packages.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn failing_split_output_is_not_exported_as_segments() {
        let bundle = boon_solid_model::SolidModelBundle::printable_bracket_fixture();
        let print = boon_manufacturing::compile_print_job(
            &bundle,
            boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle),
        );
        let mut narrow_request =
            boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
        narrow_request.build_volume = boon_solid_model::Aabb64 {
            min: boon_solid_model::Vec3d::new(-20.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(20.0, 80.0, 80.0),
        };
        let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request);
        let mut preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);
        preparation_artifact.split_segments[0].bounds.max.x = -23.0;
        preparation_artifact.split_segments[1].bounds.min.x = -23.0;
        let mut split =
            boon_manufacturing::compile_split_print_output(&print, &preparation_artifact);
        split.status = boon_manufacturing::SplitPrintOutputStatus::Fail;

        let packages = export_split_3mf_entry_sets(&print, &split);

        assert_eq!(
            split.status,
            boon_manufacturing::SplitPrintOutputStatus::Fail
        );
        assert_eq!(packages.status, ThreeMfExportStatus::Fail);
        assert!(packages.segment_packages.is_empty());
        assert!(
            packages
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "split-output-not-pass" })
        );
        assert!(packages.artifact_hash.starts_with("sha256:"));
        assert!(!packages.visual_mesh_used_for_manufacturing);
    }
}
