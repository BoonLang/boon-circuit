use std::collections::BTreeSet;

use boon_solid_model::{
    Aabb64, AssemblyId, FeatureId, ManufacturingRole, Mat4d, PartId, PartInstanceId,
    PhysicalMaterialId, SolidGraph, SolidModelBundle, SolidNodeId, SolidOp, SolidValidationStatus,
    Units, Vec3d,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrinterProfileId(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintCompileRequest {
    pub assembly: AssemblyId,
    pub units: Units,
    pub scope: PrintCompileScope,
    pub layer_height: f64,
    pub xy_error: f64,
    pub z_error: f64,
    pub minimum_feature: f64,
    pub integer_grid: f64,
    pub build_volume: Aabb64,
    pub profile: PrinterProfileId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrintCompileScope {
    WholeAssembly,
    SelectedInstances { instances: BTreeSet<PartInstanceId> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManufacturingCompileStatus {
    Pass,
    Fail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManufacturingDiagnostic {
    pub code: String,
    pub node: Option<SolidNodeId>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintCompileOutput {
    pub status: ManufacturingCompileStatus,
    pub request: PrintCompileRequest,
    pub layers: Vec<Layer>,
    pub diagnostics: Vec<ManufacturingDiagnostic>,
    pub metrics: ManufacturingMetrics,
    pub tolerance: ManufacturingToleranceReport,
    pub artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintPreparationPlan {
    pub status: PrintPreparationStatus,
    pub request: PrintCompileRequest,
    pub printable_part_count: usize,
    pub printable_instance_count: usize,
    pub visual_only_instance_count: usize,
    pub printable_bounds: Option<Aabb64>,
    pub build_volume: Aabb64,
    pub fits_build_volume: bool,
    pub minimum_clearance: f64,
    pub minimum_clearance_observed: Option<f64>,
    pub clearance_violation_count: usize,
    pub minimum_wall_thickness: f64,
    pub minimum_wall_thickness_observed: Option<f64>,
    pub wall_thickness_violation_count: usize,
    pub split_required: bool,
    pub split_axis: Option<SplitAxis>,
    pub suggested_segment_count: usize,
    pub connector_strategy: ConnectorStrategy,
    pub connector_pair_count: usize,
    pub connector_fit_status: ConnectorFitStatus,
    pub connector_fit_violation_count: usize,
    pub split_segments: Vec<SplitSegmentPlan>,
    pub connector_plans: Vec<ConnectorPlan>,
    pub diagnostics: Vec<ManufacturingDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintPreparationArtifact {
    pub status: PrintPreparationArtifactStatus,
    pub preparation_status: PrintPreparationStatus,
    pub split_required: bool,
    pub split_axis: Option<SplitAxis>,
    pub split_segments: Vec<SplitSegmentPlan>,
    pub connector_plans: Vec<ConnectorPlan>,
    pub connector_fit_status: ConnectorFitStatus,
    pub connector_fit_violation_count: usize,
    pub minimum_wall_thickness: f64,
    pub minimum_wall_thickness_observed: Option<f64>,
    pub wall_thickness_violation_count: usize,
    pub diagnostics: Vec<ManufacturingDiagnostic>,
    pub artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitPrintOutput {
    pub status: SplitPrintOutputStatus,
    pub source_manufacturing_artifact_hash: String,
    pub preparation_artifact_hash: String,
    pub segments: Vec<SplitPrintSegmentOutput>,
    pub diagnostics: Vec<ManufacturingDiagnostic>,
    pub artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConnectorPrintOutput {
    pub status: ConnectorPrintOutputStatus,
    pub source_preparation_artifact_hash: String,
    pub connector_count: usize,
    pub layers: Vec<Layer>,
    pub metrics: ManufacturingMetrics,
    pub tolerance: ManufacturingToleranceReport,
    pub diagnostics: Vec<ManufacturingDiagnostic>,
    pub artifact_hash: String,
    pub visual_mesh_used_for_manufacturing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConnectorCutoutValidationReport {
    pub status: ConnectorCutoutValidationStatus,
    pub connector_count: usize,
    pub segment_count: usize,
    pub expected_cutout_hole_count: usize,
    pub observed_cutout_hole_count: usize,
    pub declared_cutout_hole_count: usize,
    pub diagnostics: Vec<ManufacturingDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitPrintSegmentOutput {
    pub segment: SplitSegmentPlan,
    pub layers: Vec<Layer>,
    pub metrics: ManufacturingMetrics,
    pub tolerance: ManufacturingToleranceReport,
    pub connector_cutout_hole_count: usize,
    pub artifact_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManufacturingToleranceReport {
    pub requested_xy_error: f64,
    pub requested_z_error: f64,
    pub requested_minimum_feature: f64,
    pub requested_integer_grid: f64,
    pub achieved_xy_error: f64,
    pub achieved_z_error: f64,
    pub max_layer_achieved_error: f64,
    pub within_requested_xy_error: bool,
    pub within_requested_z_error: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrintPreparationStatus {
    Ready,
    SplitRequired,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrintPreparationArtifactStatus {
    Pass,
    Fail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitPrintOutputStatus {
    Pass,
    Fail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorPrintOutputStatus {
    Pass,
    Fail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorCutoutValidationStatus {
    Pass,
    Fail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    X,
    Y,
    Z,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorStrategy {
    None,
    PlannedDowelPins,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorFitStatus {
    NotRequired,
    Pass,
    Fail,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitSegmentPlan {
    pub index: usize,
    pub axis: SplitAxis,
    pub bounds: Aabb64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConnectorPlan {
    pub pair_index: usize,
    pub strategy: ConnectorStrategy,
    pub axis: SplitAxis,
    pub center: Vec3d,
    pub diameter: f64,
    pub length: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManufacturingMetrics {
    pub layer_count: usize,
    pub material_region_count: usize,
    pub polygon_count: usize,
    pub hole_count: usize,
    pub printable_part_count: usize,
    pub printable_instance_count: usize,
    pub unsupported_operation_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub index: u32,
    pub z: f64,
    pub regions: Vec<MaterialRegion2D>,
    pub achieved_error: f64,
    pub diagnostics: Vec<ManufacturingDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialRegion2D {
    pub part: PartId,
    pub instance: PartInstanceId,
    pub material: PhysicalMaterialId,
    pub polygons: Vec<PolygonWithHoles>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolygonWithHoles {
    pub outer: Vec<GridPoint2D>,
    pub holes: Vec<Vec<GridPoint2D>>,
    pub source_features: Vec<FeatureId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GridPoint2D {
    pub x: i64,
    pub y: i64,
}

#[derive(Clone, Debug)]
struct RawPolygon {
    outer: Vec<RawPoint2D>,
    holes: Vec<Vec<RawPoint2D>>,
    source_features: Vec<FeatureId>,
}

#[derive(Clone, Copy, Debug)]
struct RawPoint2D {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug)]
struct PrintableInstanceBounds {
    part: PartId,
    instance: PartInstanceId,
    root: SolidNodeId,
    bounds: Aabb64,
    wall_thickness: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GridBounds2D {
    min_x: i64,
    min_y: i64,
    max_x: i64,
    max_y: i64,
}

impl PrintCompileRequest {
    pub fn default_for_bundle(bundle: &SolidModelBundle) -> Self {
        Self {
            assembly: bundle.assembly.id,
            units: Units::Millimeter,
            scope: PrintCompileScope::WholeAssembly,
            layer_height: 0.4,
            xy_error: 0.05,
            z_error: 0.20,
            minimum_feature: 0.40,
            integer_grid: 0.001,
            build_volume: Aabb64 {
                min: boon_solid_model::Vec3d::new(-200.0, -200.0, -10.0),
                max: boon_solid_model::Vec3d::new(260.0, 200.0, 220.0),
            },
            profile: PrinterProfileId("generic-fdm-0.4mm".to_owned()),
        }
    }

    pub fn for_selected_instances(
        bundle: &SolidModelBundle,
        instances: impl IntoIterator<Item = PartInstanceId>,
    ) -> Self {
        let mut request = Self::default_for_bundle(bundle);
        request.scope = PrintCompileScope::SelectedInstances {
            instances: instances.into_iter().collect(),
        };
        request
    }
}

pub fn compile_print_job(
    bundle: &SolidModelBundle,
    request: PrintCompileRequest,
) -> PrintCompileOutput {
    let mut diagnostics = validate_request(&request);
    let validation = bundle.validate();
    for diagnostic in validation.diagnostics {
        diagnostics.push(ManufacturingDiagnostic {
            code: diagnostic.code,
            node: diagnostic.node,
            message: diagnostic.message,
        });
    }
    validate_printable_roles(bundle, &mut diagnostics);
    validate_print_scope(bundle, &request, &mut diagnostics);

    let mut layers = Vec::new();
    let mut metrics = ManufacturingMetrics {
        printable_part_count: scoped_printable_part_count(bundle, &request),
        printable_instance_count: scoped_printable_instance_count(bundle, &request),
        ..ManufacturingMetrics::default()
    };
    if validation.status == SolidValidationStatus::Pass && diagnostics.is_empty() {
        layers = compile_layers(bundle, &request, &mut diagnostics);
    }

    metrics.layer_count = layers.len();
    metrics.material_region_count = layers.iter().map(|layer| layer.regions.len()).sum();
    metrics.polygon_count = layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .map(|region| region.polygons.len())
        .sum();
    metrics.hole_count = layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .flat_map(|region| &region.polygons)
        .map(|polygon| polygon.holes.len())
        .sum();
    metrics.unsupported_operation_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "unsupported-solid-op")
        .count();

    let status = if diagnostics.is_empty()
        && !layers.is_empty()
        && metrics.unsupported_operation_count == 0
        && validation.status == SolidValidationStatus::Pass
    {
        ManufacturingCompileStatus::Pass
    } else {
        ManufacturingCompileStatus::Fail
    };
    let tolerance = tolerance_report_for_request(&request, &layers);
    let artifact_hash = manufacturing_artifact_hash(&request, &layers, &tolerance);

    PrintCompileOutput {
        status,
        request,
        layers,
        diagnostics,
        metrics,
        tolerance,
        artifact_hash,
        visual_mesh_used_for_manufacturing: false,
    }
}

pub fn prepare_print_job(
    bundle: &SolidModelBundle,
    request: PrintCompileRequest,
) -> PrintPreparationPlan {
    let mut diagnostics = validate_request(&request);
    let validation = bundle.validate();
    for diagnostic in validation.diagnostics {
        diagnostics.push(ManufacturingDiagnostic {
            code: diagnostic.code,
            node: diagnostic.node,
            message: diagnostic.message,
        });
    }
    validate_printable_roles(bundle, &mut diagnostics);
    validate_print_scope(bundle, &request, &mut diagnostics);

    let mut printable_instance_count = 0;
    let mut visual_only_instance_count = 0;
    let mut printable_bounds = None::<Aabb64>;
    let mut printable_instance_bounds = Vec::new();
    for instance in bundle
        .assembly
        .instances
        .iter()
        .filter(|instance| request_includes_instance(&request, instance.id))
    {
        let Some(part) = bundle.assembly.parts.get(&instance.part) else {
            continue;
        };
        match part.manufacturing_role {
            ManufacturingRole::PrintableSolid => {
                printable_instance_count += 1;
                let Some(graph) = bundle.solids.get(&part.geometry) else {
                    continue;
                };
                let Some(root) = graph.nodes.get(&part.root) else {
                    diagnostics.push(ManufacturingDiagnostic {
                        code: "missing-part-root".to_owned(),
                        node: Some(part.root),
                        message: format!(
                            "part {:?} root is missing during print preparation",
                            part.id
                        ),
                    });
                    continue;
                };
                let bounds = root
                    .bounds
                    .translate(translation_from_matrix(instance.transform));
                let wall_thickness = estimated_wall_thickness(graph, part.root);
                printable_bounds = Some(match printable_bounds {
                    Some(current) => current.union(bounds),
                    None => bounds,
                });
                printable_instance_bounds.push(PrintableInstanceBounds {
                    part: part.id,
                    instance: instance.id,
                    root: part.root,
                    bounds,
                    wall_thickness,
                });
            }
            ManufacturingRole::VisualOnly => {
                visual_only_instance_count += 1;
            }
            ManufacturingRole::VoidModifier
            | ManufacturingRole::SupportModifier
            | ManufacturingRole::InfillModifier
            | ManufacturingRole::Reference => {}
        }
    }
    let printable_part_count = scoped_printable_part_count(bundle, &request);

    let fits_build_volume = printable_bounds.is_some_and(|bounds| {
        bounds.min.x >= request.build_volume.min.x
            && bounds.max.x <= request.build_volume.max.x
            && bounds.min.y >= request.build_volume.min.y
            && bounds.max.y <= request.build_volume.max.y
            && bounds.min.z >= request.build_volume.min.z
            && bounds.max.z <= request.build_volume.max.z
    });
    let split_axis =
        printable_bounds.and_then(|bounds| split_axis_for_bounds(bounds, request.build_volume));
    let split_required = !fits_build_volume && split_axis.is_some();
    let suggested_segment_count = if split_required { 2 } else { 1 };
    let connector_strategy = if split_required {
        ConnectorStrategy::PlannedDowelPins
    } else {
        ConnectorStrategy::None
    };
    let (split_segments, connector_plans) = if split_required {
        let bounds = printable_bounds.expect("split_required requires printable_bounds");
        build_split_preparation(
            bounds,
            split_axis.expect("split_required requires split_axis"),
            suggested_segment_count,
            connector_strategy.clone(),
            request.minimum_feature,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let connector_pair_count = connector_plans.len();
    if split_required {
        diagnostics.push(ManufacturingDiagnostic {
            code: "split-required".to_owned(),
            node: None,
            message: format!(
                "printable assembly exceeds build volume; planned split axis: {:?}",
                split_axis
            ),
        });
    }
    let (connector_fit_status, connector_fit_violation_count) = connector_fit_diagnostics(
        printable_bounds,
        split_axis,
        &split_segments,
        &connector_plans,
        request.minimum_feature,
        request.integer_grid,
        &mut diagnostics,
    );
    let minimum_clearance = request.minimum_feature;
    let (minimum_clearance_observed, clearance_violation_count) = clearance_diagnostics(
        &printable_instance_bounds,
        minimum_clearance,
        &mut diagnostics,
    );
    let minimum_wall_thickness = request.minimum_feature;
    let (minimum_wall_thickness_observed, wall_thickness_violation_count) =
        wall_thickness_diagnostics(
            &printable_instance_bounds,
            minimum_wall_thickness,
            &mut diagnostics,
        );
    let status = if validation.status != SolidValidationStatus::Pass
        || diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code != "split-required")
        || printable_part_count == 0
    {
        PrintPreparationStatus::Blocked
    } else if split_required {
        PrintPreparationStatus::SplitRequired
    } else {
        PrintPreparationStatus::Ready
    };

    PrintPreparationPlan {
        status,
        request: request.clone(),
        printable_part_count,
        printable_instance_count,
        visual_only_instance_count,
        printable_bounds,
        build_volume: request.build_volume,
        fits_build_volume,
        minimum_clearance,
        minimum_clearance_observed,
        clearance_violation_count,
        minimum_wall_thickness,
        minimum_wall_thickness_observed,
        wall_thickness_violation_count,
        split_required,
        split_axis,
        suggested_segment_count,
        connector_strategy,
        connector_pair_count,
        connector_fit_status,
        connector_fit_violation_count,
        split_segments,
        connector_plans,
        diagnostics,
    }
}

pub fn preparation_artifact(plan: &PrintPreparationPlan) -> PrintPreparationArtifact {
    let status = if plan.status == PrintPreparationStatus::Blocked {
        PrintPreparationArtifactStatus::Fail
    } else {
        PrintPreparationArtifactStatus::Pass
    };
    let payload = PrintPreparationArtifactPayload {
        preparation_status: plan.status.clone(),
        split_required: plan.split_required,
        split_axis: plan.split_axis,
        split_segments: plan.split_segments.clone(),
        connector_plans: plan.connector_plans.clone(),
        connector_fit_status: plan.connector_fit_status.clone(),
        connector_fit_violation_count: plan.connector_fit_violation_count,
        minimum_wall_thickness: plan.minimum_wall_thickness,
        minimum_wall_thickness_observed: plan.minimum_wall_thickness_observed,
        wall_thickness_violation_count: plan.wall_thickness_violation_count,
        diagnostics: plan.diagnostics.clone(),
        visual_mesh_used_for_manufacturing: false,
    };
    let artifact_hash = preparation_artifact_hash(&payload);
    PrintPreparationArtifact {
        status,
        preparation_status: payload.preparation_status,
        split_required: payload.split_required,
        split_axis: payload.split_axis,
        split_segments: payload.split_segments,
        connector_plans: payload.connector_plans,
        connector_fit_status: payload.connector_fit_status,
        connector_fit_violation_count: payload.connector_fit_violation_count,
        minimum_wall_thickness: payload.minimum_wall_thickness,
        minimum_wall_thickness_observed: payload.minimum_wall_thickness_observed,
        wall_thickness_violation_count: payload.wall_thickness_violation_count,
        diagnostics: payload.diagnostics,
        artifact_hash,
        visual_mesh_used_for_manufacturing: payload.visual_mesh_used_for_manufacturing,
    }
}

pub fn compile_split_print_output(
    print: &PrintCompileOutput,
    preparation: &PrintPreparationArtifact,
) -> SplitPrintOutput {
    let mut diagnostics = Vec::new();
    if print.status != ManufacturingCompileStatus::Pass {
        diagnostics.push(ManufacturingDiagnostic {
            code: "split-source-not-pass".to_owned(),
            node: None,
            message: "split manufacturing output requires a passing print output".to_owned(),
        });
    }
    if preparation.status != PrintPreparationArtifactStatus::Pass {
        diagnostics.push(ManufacturingDiagnostic {
            code: "split-preparation-not-pass".to_owned(),
            node: None,
            message: "split manufacturing output requires a passing preparation artifact"
                .to_owned(),
        });
    }
    if !preparation.split_required || preparation.split_segments.is_empty() {
        diagnostics.push(ManufacturingDiagnostic {
            code: "split-preparation-missing-segments".to_owned(),
            node: None,
            message: "split manufacturing output requires split segment plans".to_owned(),
        });
    }
    let segments = if diagnostics.is_empty() {
        preparation
            .split_segments
            .iter()
            .cloned()
            .map(|segment| split_segment_output(print, segment, &preparation.connector_plans))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let status = if diagnostics.is_empty() && !segments.is_empty() {
        SplitPrintOutputStatus::Pass
    } else {
        SplitPrintOutputStatus::Fail
    };
    let artifact_hash =
        split_print_artifact_hash(&print.artifact_hash, &preparation.artifact_hash, &segments);
    SplitPrintOutput {
        status,
        source_manufacturing_artifact_hash: print.artifact_hash.clone(),
        preparation_artifact_hash: preparation.artifact_hash.clone(),
        segments,
        diagnostics,
        artifact_hash,
        visual_mesh_used_for_manufacturing: print.visual_mesh_used_for_manufacturing
            || preparation.visual_mesh_used_for_manufacturing,
    }
}

pub fn compile_connector_print_output(
    preparation: &PrintPreparationArtifact,
    request: &PrintCompileRequest,
) -> ConnectorPrintOutput {
    let mut diagnostics = Vec::new();
    if preparation.status != PrintPreparationArtifactStatus::Pass {
        diagnostics.push(ManufacturingDiagnostic {
            code: "connector-preparation-not-pass".to_owned(),
            node: None,
            message: "connector print output requires a passing preparation artifact".to_owned(),
        });
    }
    if preparation.connector_plans.is_empty() {
        diagnostics.push(ManufacturingDiagnostic {
            code: "connector-plans-missing".to_owned(),
            node: None,
            message: "connector print output requires at least one connector plan".to_owned(),
        });
    }
    for connector in &preparation.connector_plans {
        if connector.strategy != ConnectorStrategy::PlannedDowelPins {
            diagnostics.push(ManufacturingDiagnostic {
                code: "connector-strategy-unsupported".to_owned(),
                node: None,
                message: format!(
                    "connector strategy {:?} is not supported by connector print output",
                    connector.strategy
                ),
            });
        }
        if connector.diameter <= 0.0 || connector.length <= 0.0 {
            diagnostics.push(ManufacturingDiagnostic {
                code: "connector-dimensions-invalid".to_owned(),
                node: None,
                message: "connector diameter and length must be positive".to_owned(),
            });
        }
        if !aabb_within(connector_bounds(connector), request.build_volume) {
            diagnostics.push(ManufacturingDiagnostic {
                code: "connector-build-volume".to_owned(),
                node: None,
                message: format!(
                    "connector {} bounds exceed the requested build volume",
                    connector.pair_index
                ),
            });
        }
    }

    let layers = if diagnostics.is_empty() {
        connector_layers(&preparation.connector_plans, request)
    } else {
        Vec::new()
    };
    let metrics = metrics_for_layers(&layers, preparation.connector_plans.len());
    let tolerance = tolerance_report_for_request(request, &layers);
    let status = if diagnostics.is_empty() && !layers.is_empty() {
        ConnectorPrintOutputStatus::Pass
    } else {
        ConnectorPrintOutputStatus::Fail
    };
    let artifact_hash = connector_print_artifact_hash(
        &preparation.artifact_hash,
        &preparation.connector_plans,
        &layers,
        &metrics,
        &tolerance,
    );
    ConnectorPrintOutput {
        status,
        source_preparation_artifact_hash: preparation.artifact_hash.clone(),
        connector_count: preparation.connector_plans.len(),
        layers,
        metrics,
        tolerance,
        diagnostics,
        artifact_hash,
        visual_mesh_used_for_manufacturing: preparation.visual_mesh_used_for_manufacturing,
    }
}

pub fn validate_split_connector_cutouts(
    preparation: &PrintPreparationArtifact,
    split: &SplitPrintOutput,
    connector: &ConnectorPrintOutput,
) -> ConnectorCutoutValidationReport {
    let mut diagnostics = Vec::new();
    if preparation.status != PrintPreparationArtifactStatus::Pass {
        diagnostics.push(ManufacturingDiagnostic {
            code: "cutout-preparation-not-pass".to_owned(),
            node: None,
            message: "connector cutout validation requires a passing preparation artifact"
                .to_owned(),
        });
    }
    if !preparation.split_required || preparation.connector_plans.is_empty() {
        diagnostics.push(ManufacturingDiagnostic {
            code: "cutout-preparation-missing-connectors".to_owned(),
            node: None,
            message: "connector cutout validation requires split preparation connector plans"
                .to_owned(),
        });
    }
    if split.status != SplitPrintOutputStatus::Pass {
        diagnostics.push(ManufacturingDiagnostic {
            code: "cutout-split-not-pass".to_owned(),
            node: None,
            message: "connector cutout validation requires a passing split print output".to_owned(),
        });
    }
    if connector.status != ConnectorPrintOutputStatus::Pass {
        diagnostics.push(ManufacturingDiagnostic {
            code: "cutout-connector-not-pass".to_owned(),
            node: None,
            message: "connector cutout validation requires a passing connector print output"
                .to_owned(),
        });
    }
    if split.preparation_artifact_hash != preparation.artifact_hash {
        diagnostics.push(ManufacturingDiagnostic {
            code: "cutout-split-preparation-hash-mismatch".to_owned(),
            node: None,
            message: "split output preparation hash does not match the preparation artifact"
                .to_owned(),
        });
    }
    if connector.source_preparation_artifact_hash != preparation.artifact_hash {
        diagnostics.push(ManufacturingDiagnostic {
            code: "cutout-connector-preparation-hash-mismatch".to_owned(),
            node: None,
            message: "connector output preparation hash does not match the preparation artifact"
                .to_owned(),
        });
    }
    if connector.connector_count != preparation.connector_plans.len() {
        diagnostics.push(ManufacturingDiagnostic {
            code: "cutout-connector-count-mismatch".to_owned(),
            node: None,
            message: format!(
                "connector output has {} connector(s), preparation has {} plan(s)",
                connector.connector_count,
                preparation.connector_plans.len()
            ),
        });
    }

    let mut expected_cutout_hole_count = 0;
    let mut observed_cutout_hole_count = 0;
    let declared_cutout_hole_count = split
        .segments
        .iter()
        .map(|segment| segment.connector_cutout_hole_count)
        .sum();
    for segment in &split.segments {
        let grid = segment.tolerance.requested_integer_grid;
        let grid_bounds = grid_bounds_for_segment(segment.segment.bounds, grid);
        for connector_plan in preparation.connector_plans.iter().filter(|connector_plan| {
            connector_plan.strategy == ConnectorStrategy::PlannedDowelPins
                && connector_plan.axis == segment.segment.axis
                && connector_intersects_segment(connector_plan, &segment.segment)
        }) {
            for layer in &segment.layers {
                let Some(raw_cutout) = connector_polygon_at_z(connector_plan, layer.z) else {
                    continue;
                };
                let cutout = regularize_polygon(raw_cutout, grid);
                let Some(cutout) = clip_polygon_with_holes_to_grid_bounds(&cutout, grid_bounds)
                else {
                    continue;
                };
                expected_cutout_hole_count += 1;
                if layer_contains_hole(layer, &cutout.outer) {
                    observed_cutout_hole_count += 1;
                } else {
                    diagnostics.push(ManufacturingDiagnostic {
                        code: "connector-cutout-hole-missing".to_owned(),
                        node: None,
                        message: format!(
                            "segment {} layer {} is missing cutout hole for connector {}",
                            segment.segment.index, layer.index, connector_plan.pair_index
                        ),
                    });
                }
            }
        }
    }
    if expected_cutout_hole_count == 0 {
        diagnostics.push(ManufacturingDiagnostic {
            code: "connector-cutout-holes-missing".to_owned(),
            node: None,
            message: "connector cutout validation found no expected cutout holes".to_owned(),
        });
    }
    if declared_cutout_hole_count != observed_cutout_hole_count {
        diagnostics.push(ManufacturingDiagnostic {
            code: "connector-cutout-count-mismatch".to_owned(),
            node: None,
            message: format!(
                "split output declares {declared_cutout_hole_count} cutout hole(s), observed {observed_cutout_hole_count}"
            ),
        });
    }

    ConnectorCutoutValidationReport {
        status: if diagnostics.is_empty() {
            ConnectorCutoutValidationStatus::Pass
        } else {
            ConnectorCutoutValidationStatus::Fail
        },
        connector_count: preparation.connector_plans.len(),
        segment_count: split.segments.len(),
        expected_cutout_hole_count,
        observed_cutout_hole_count,
        declared_cutout_hole_count,
        diagnostics,
    }
}

fn connector_layers(connectors: &[ConnectorPlan], request: &PrintCompileRequest) -> Vec<Layer> {
    let Some((min_z, max_z)) = connector_z_bounds(connectors) else {
        return Vec::new();
    };
    let mut layers = Vec::new();
    let mut index = 0_u32;
    let mut z = min_z + request.layer_height * 0.5;
    while z <= max_z + f64::EPSILON {
        let regions = connector_layer_regions(connectors, z, request);
        if !regions.is_empty() {
            layers.push(Layer {
                index,
                z,
                regions,
                achieved_error: request.integer_grid * 0.5,
                diagnostics: Vec::new(),
            });
        }
        index = index.saturating_add(1);
        z += request.layer_height;
    }
    layers
}

fn connector_layer_regions(
    connectors: &[ConnectorPlan],
    z: f64,
    request: &PrintCompileRequest,
) -> Vec<MaterialRegion2D> {
    connectors
        .iter()
        .filter_map(|connector| {
            let polygon = connector_polygon_at_z(connector, z)?;
            let id = 90_000 + connector.pair_index as u64;
            Some(MaterialRegion2D {
                part: PartId(id),
                instance: PartInstanceId(id),
                material: PhysicalMaterialId(90_000),
                polygons: vec![regularize_polygon(polygon, request.integer_grid)],
            })
        })
        .collect()
}

fn connector_polygon_at_z(connector: &ConnectorPlan, z: f64) -> Option<RawPolygon> {
    let radius = connector.diameter * 0.5;
    let feature = FeatureId(90_000 + connector.pair_index as u64);
    match connector.axis {
        SplitAxis::X => {
            let dz = z - connector.center.z;
            if dz.abs() > radius {
                return None;
            }
            let half_width = (radius * radius - dz * dz).max(0.0).sqrt();
            let bounds = Aabb64 {
                min: Vec3d::new(
                    connector.center.x - connector.length * 0.5,
                    connector.center.y - half_width,
                    z,
                ),
                max: Vec3d::new(
                    connector.center.x + connector.length * 0.5,
                    connector.center.y + half_width,
                    z,
                ),
            };
            Some(RawPolygon::rectangle(bounds, feature))
        }
        SplitAxis::Y => {
            let dz = z - connector.center.z;
            if dz.abs() > radius {
                return None;
            }
            let half_width = (radius * radius - dz * dz).max(0.0).sqrt();
            let bounds = Aabb64 {
                min: Vec3d::new(
                    connector.center.x - half_width,
                    connector.center.y - connector.length * 0.5,
                    z,
                ),
                max: Vec3d::new(
                    connector.center.x + half_width,
                    connector.center.y + connector.length * 0.5,
                    z,
                ),
            };
            Some(RawPolygon::rectangle(bounds, feature))
        }
        SplitAxis::Z => {
            let dz = z - connector.center.z;
            if dz.abs() > connector.length * 0.5 {
                return None;
            }
            Some(RawPolygon::circle(
                RawPoint2D {
                    x: connector.center.x,
                    y: connector.center.y,
                },
                radius,
                feature,
            ))
        }
    }
}

fn connector_z_bounds(connectors: &[ConnectorPlan]) -> Option<(f64, f64)> {
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for connector in connectors {
        let bounds = connector_bounds(connector);
        min_z = min_z.min(bounds.min.z);
        max_z = max_z.max(bounds.max.z);
    }
    min_z.is_finite().then_some((min_z, max_z))
}

fn connector_bounds(connector: &ConnectorPlan) -> Aabb64 {
    let radius = connector.diameter * 0.5;
    match connector.axis {
        SplitAxis::X => Aabb64 {
            min: Vec3d::new(
                connector.center.x - connector.length * 0.5,
                connector.center.y - radius,
                connector.center.z - radius,
            ),
            max: Vec3d::new(
                connector.center.x + connector.length * 0.5,
                connector.center.y + radius,
                connector.center.z + radius,
            ),
        },
        SplitAxis::Y => Aabb64 {
            min: Vec3d::new(
                connector.center.x - radius,
                connector.center.y - connector.length * 0.5,
                connector.center.z - radius,
            ),
            max: Vec3d::new(
                connector.center.x + radius,
                connector.center.y + connector.length * 0.5,
                connector.center.z + radius,
            ),
        },
        SplitAxis::Z => Aabb64 {
            min: Vec3d::new(
                connector.center.x - radius,
                connector.center.y - radius,
                connector.center.z - connector.length * 0.5,
            ),
            max: Vec3d::new(
                connector.center.x + radius,
                connector.center.y + radius,
                connector.center.z + connector.length * 0.5,
            ),
        },
    }
}

fn compile_layers(
    bundle: &SolidModelBundle,
    request: &PrintCompileRequest,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> Vec<Layer> {
    let Some((min_z, max_z)) = printable_z_bounds(bundle, request, diagnostics) else {
        return Vec::new();
    };
    if min_z < request.build_volume.min.z || max_z > request.build_volume.max.z {
        diagnostics.push(ManufacturingDiagnostic {
            code: "build-volume-z".to_owned(),
            node: None,
            message: format!(
                "print z bounds [{min_z}, {max_z}] exceed build volume [{}, {}]",
                request.build_volume.min.z, request.build_volume.max.z
            ),
        });
        return Vec::new();
    }

    let mut layers = Vec::new();
    let mut index = 0_u32;
    let mut z = min_z + request.layer_height * 0.5;
    while z <= max_z + f64::EPSILON {
        let mut regions = Vec::new();
        let mut layer_diagnostics = Vec::new();
        for instance in bundle
            .assembly
            .instances
            .iter()
            .filter(|instance| request_includes_instance(request, instance.id))
        {
            let Some(part) = bundle.assembly.parts.get(&instance.part) else {
                continue;
            };
            if part.manufacturing_role != ManufacturingRole::PrintableSolid {
                continue;
            }
            let Some(material) = part.physical_material else {
                continue;
            };
            let Some(graph) = bundle.solids.get(&part.geometry) else {
                continue;
            };
            let translation = translation_from_matrix(instance.transform);
            let local_z = z - translation.z;
            let mut section_diagnostics = Vec::new();
            let raw_polygons =
                section_polygons(graph, part.root, local_z, &mut section_diagnostics)
                    .into_iter()
                    .map(|polygon| polygon.translate(translation.x, translation.y))
                    .collect::<Vec<_>>();
            layer_diagnostics.extend(section_diagnostics);
            if raw_polygons.is_empty() {
                continue;
            }
            if !bounds_inside_build_volume(&raw_polygons, &request.build_volume) {
                diagnostics.push(ManufacturingDiagnostic {
                    code: "build-volume-xy".to_owned(),
                    node: Some(part.root),
                    message: format!(
                        "part {:?} instance {:?} exceeds build volume in XY",
                        part.id, instance.id
                    ),
                });
                continue;
            }
            regions.push(MaterialRegion2D {
                part: part.id,
                instance: instance.id,
                material,
                polygons: raw_polygons
                    .into_iter()
                    .map(|polygon| regularize_polygon(polygon, request.integer_grid))
                    .collect(),
            });
        }
        layer_diagnostics.extend(material_region_conflicts(index, &regions));
        if !regions.is_empty() || !layer_diagnostics.is_empty() {
            diagnostics.extend(layer_diagnostics.iter().cloned());
            layers.push(Layer {
                index,
                z,
                regions,
                achieved_error: request.integer_grid * 0.5,
                diagnostics: layer_diagnostics,
            });
        }
        index = index.saturating_add(1);
        z += request.layer_height;
    }
    layers
}

fn section_polygons(
    graph: &SolidGraph,
    node_id: SolidNodeId,
    z: f64,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> Vec<RawPolygon> {
    let Some(node) = graph.nodes.get(&node_id) else {
        diagnostics.push(ManufacturingDiagnostic {
            code: "missing-solid-node".to_owned(),
            node: Some(node_id),
            message: format!("solid node {:?} is missing during slicing", node_id),
        });
        return Vec::new();
    };
    if z < node.bounds.min.z || z > node.bounds.max.z {
        return Vec::new();
    }
    match &node.op {
        SolidOp::Box { .. } | SolidOp::RoundedBox { .. } => {
            vec![RawPolygon::rectangle(node.bounds, node.feature_id)]
        }
        SolidOp::Cylinder { radius, .. } => {
            vec![RawPolygon::circle(
                center_from_bounds(node.bounds),
                *radius,
                node.feature_id,
            )]
        }
        SolidOp::Union { children } => {
            let polygons = children
                .iter()
                .flat_map(|child| section_polygons(graph, *child, z, diagnostics))
                .collect();
            collapse_contained_union_polygons(polygons)
        }
        SolidOp::Intersection { children } => {
            section_rectangular_intersection_polygons(graph, node.id, children, z, diagnostics)
        }
        SolidOp::Difference { base, tools } => {
            let mut base_polygons = section_polygons(graph, *base, z, diagnostics);
            let hole_polygons = tools
                .iter()
                .flat_map(|tool| section_polygons(graph, *tool, z, diagnostics))
                .collect::<Vec<_>>();
            for hole in hole_polygons {
                for base_polygon in &mut base_polygons {
                    if base_polygon.contains_point(hole.centroid()) {
                        base_polygon
                            .source_features
                            .extend(hole.source_features.iter());
                        base_polygon.holes.push(hole.outer.clone());
                    }
                }
            }
            base_polygons
        }
        SolidOp::Transform { child, transform } => {
            let translation = translation_from_matrix(*transform);
            section_polygons(graph, *child, z - translation.z, diagnostics)
                .into_iter()
                .map(|polygon| polygon.translate(translation.x, translation.y))
                .collect()
        }
        SolidOp::Sphere { radius } => {
            let center_z = (node.bounds.min.z + node.bounds.max.z) * 0.5;
            let dz = z - center_z;
            let section_radius = (radius * radius - dz * dz).max(0.0).sqrt();
            if section_radius <= f64::EPSILON {
                Vec::new()
            } else {
                vec![RawPolygon::circle(
                    center_from_bounds(node.bounds),
                    section_radius,
                    node.feature_id,
                )]
            }
        }
        SolidOp::Cone {
            radius0,
            radius1,
            height,
        } => {
            let z_span = (node.bounds.max.z - node.bounds.min.z).abs().max(*height);
            if z_span <= f64::EPSILON {
                return Vec::new();
            }
            let t = ((z - node.bounds.min.z) / z_span).clamp(0.0, 1.0);
            let section_radius = radius0 + (radius1 - radius0) * t;
            if section_radius <= f64::EPSILON {
                Vec::new()
            } else {
                vec![RawPolygon::circle(
                    center_from_bounds(node.bounds),
                    section_radius,
                    node.feature_id,
                )]
            }
        }
        SolidOp::Torus {
            major_radius,
            minor_radius,
        } => {
            let center_z = (node.bounds.min.z + node.bounds.max.z) * 0.5;
            let dz = z - center_z;
            let tube_section_radius = (minor_radius * minor_radius - dz * dz).max(0.0).sqrt();
            if tube_section_radius <= f64::EPSILON {
                Vec::new()
            } else {
                let outer_radius = major_radius + tube_section_radius;
                let inner_radius = (major_radius - tube_section_radius).max(0.0);
                vec![RawPolygon::annulus(
                    center_from_bounds(node.bounds),
                    outer_radius,
                    inner_radius,
                    node.feature_id,
                )]
            }
        }
        SolidOp::Shell { child, thickness } => {
            section_shell_polygons(graph, node.id, *child, *thickness, z, diagnostics)
        }
        SolidOp::Extrude { profile, .. } => {
            section_extrude_polygons(graph, node.id, *profile, diagnostics)
        }
        SolidOp::Revolve { profile, axis } => {
            section_revolve_polygons(graph, node.id, *profile, *axis, z, diagnostics)
        }
        SolidOp::Loft { profiles } => {
            section_loft_polygons(graph, node.id, profiles, z, diagnostics)
        }
        SolidOp::Sweep { .. }
        | SolidOp::Offset { .. }
        | SolidOp::SmoothUnion { .. }
        | SolidOp::Functional { .. }
        | SolidOp::ImportedSolid { .. } => {
            diagnostics.push(ManufacturingDiagnostic {
                code: "unsupported-solid-op".to_owned(),
                node: Some(node.id),
                message: format!("manufacturing compiler does not yet support {:?}", node.op),
            });
            Vec::new()
        }
    }
}

fn section_rectangular_intersection_polygons(
    graph: &SolidGraph,
    node_id: SolidNodeId,
    children: &[SolidNodeId],
    z: f64,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> Vec<RawPolygon> {
    if children.is_empty() {
        diagnostics.push(ManufacturingDiagnostic {
            code: "unsupported-solid-op".to_owned(),
            node: Some(node_id),
            message: format!(
                "manufacturing compiler supports Intersection only for non-empty box-like child sections, got no children on node {:?}",
                node_id
            ),
        });
        return Vec::new();
    }

    let mut intersection: Option<Aabb64> = None;
    let mut source_features = vec![FeatureId(node_id.0)];
    for child in children {
        let child_polygons = section_polygons(graph, *child, z, diagnostics);
        if child_polygons.is_empty() {
            return Vec::new();
        }
        if child_polygons.len() != 1 {
            diagnostics.push(ManufacturingDiagnostic {
                code: "unsupported-solid-op".to_owned(),
                node: Some(node_id),
                message: format!(
                    "manufacturing compiler supports Intersection only for one rectangular section per child, child {:?} produced {} sections",
                    child,
                    child_polygons.len()
                ),
            });
            return Vec::new();
        }
        let child_polygon = &child_polygons[0];
        let Some(bounds) = child_polygon.rectangular_bounds_at_z(z) else {
            diagnostics.push(ManufacturingDiagnostic {
                code: "unsupported-solid-op".to_owned(),
                node: Some(node_id),
                message: format!(
                    "manufacturing compiler supports Intersection only for hole-free rectangular child sections, child {:?} produced a non-rectangular section",
                    child
                ),
            });
            return Vec::new();
        };
        for feature in &child_polygon.source_features {
            if !source_features.contains(feature) {
                source_features.push(*feature);
            }
        }
        intersection = Some(match intersection {
            Some(current) => Aabb64 {
                min: Vec3d::new(
                    current.min.x.max(bounds.min.x),
                    current.min.y.max(bounds.min.y),
                    z,
                ),
                max: Vec3d::new(
                    current.max.x.min(bounds.max.x),
                    current.max.y.min(bounds.max.y),
                    z,
                ),
            },
            None => bounds,
        });
    }

    let Some(bounds) = intersection else {
        return Vec::new();
    };
    if bounds.min.x >= bounds.max.x || bounds.min.y >= bounds.max.y {
        return Vec::new();
    }
    let mut polygon = RawPolygon::rectangle(bounds, FeatureId(node_id.0));
    polygon.source_features = source_features;
    vec![polygon]
}

fn section_extrude_polygons(
    graph: &SolidGraph,
    node_id: SolidNodeId,
    profile: boon_solid_model::ProfileId,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> Vec<RawPolygon> {
    let Some(profile) = graph.profiles.get(&profile) else {
        diagnostics.push(ManufacturingDiagnostic {
            code: "missing-profile".to_owned(),
            node: Some(node_id),
            message: format!(
                "extrude node {:?} references missing profile {:?} during slicing",
                node_id, profile
            ),
        });
        return Vec::new();
    };
    if !profile.closed || profile.segment_count != 4 {
        diagnostics.push(ManufacturingDiagnostic {
            code: "unsupported-profile".to_owned(),
            node: Some(node_id),
            message: format!(
                "manufacturing compiler supports Extrude only for closed rectangular profiles, got closed={} segments={}",
                profile.closed, profile.segment_count
            ),
        });
        return Vec::new();
    }
    if profile.bounds.min.x >= profile.bounds.max.x || profile.bounds.min.y >= profile.bounds.max.y
    {
        diagnostics.push(ManufacturingDiagnostic {
            code: "invalid-profile-bounds".to_owned(),
            node: Some(node_id),
            message: format!(
                "extrude node {:?} has invalid rectangular profile bounds {:?}",
                node_id, profile.bounds
            ),
        });
        return Vec::new();
    }
    vec![RawPolygon::rectangle(profile.bounds, FeatureId(node_id.0))]
}

fn section_revolve_polygons(
    graph: &SolidGraph,
    node_id: SolidNodeId,
    profile: boon_solid_model::ProfileId,
    axis: boon_solid_model::Axis3d,
    z: f64,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> Vec<RawPolygon> {
    let Some(profile) = graph.profiles.get(&profile) else {
        diagnostics.push(ManufacturingDiagnostic {
            code: "missing-profile".to_owned(),
            node: Some(node_id),
            message: format!(
                "revolve node {:?} references missing profile {:?} during slicing",
                node_id, profile
            ),
        });
        return Vec::new();
    };
    if !axis_is_default_z(axis) {
        diagnostics.push(ManufacturingDiagnostic {
            code: "unsupported-axis".to_owned(),
            node: Some(node_id),
            message: format!(
                "manufacturing compiler supports Revolve only around the default Z axis, got {:?}",
                axis
            ),
        });
        return Vec::new();
    }
    let inner_radius = profile.bounds.min.x.abs().min(profile.bounds.max.x.abs());
    let outer_radius = profile.bounds.min.x.abs().max(profile.bounds.max.x.abs());
    if outer_radius <= f64::EPSILON
        || profile.bounds.min.z >= profile.bounds.max.z
        || inner_radius >= outer_radius
    {
        diagnostics.push(ManufacturingDiagnostic {
            code: "invalid-profile-bounds".to_owned(),
            node: Some(node_id),
            message: format!(
                "revolve node {:?} has invalid rectangular radial profile bounds {:?}",
                node_id, profile.bounds
            ),
        });
        return Vec::new();
    }
    if z < profile.bounds.min.z || z > profile.bounds.max.z {
        return Vec::new();
    }
    vec![RawPolygon::annulus(
        RawPoint2D {
            x: axis.origin.x,
            y: axis.origin.y,
        },
        outer_radius,
        inner_radius,
        FeatureId(node_id.0),
    )]
}

fn section_loft_polygons(
    graph: &SolidGraph,
    node_id: SolidNodeId,
    profiles: &[boon_solid_model::ProfileId],
    z: f64,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> Vec<RawPolygon> {
    if profiles.len() != 2 {
        diagnostics.push(ManufacturingDiagnostic {
            code: "unsupported-solid-op".to_owned(),
            node: Some(node_id),
            message: format!(
                "manufacturing compiler supports Loft only for two rectangular profiles, got {} profiles",
                profiles.len()
            ),
        });
        return Vec::new();
    }
    let Some(bottom) = graph.profiles.get(&profiles[0]) else {
        diagnostics.push(ManufacturingDiagnostic {
            code: "missing-profile".to_owned(),
            node: Some(node_id),
            message: format!(
                "loft node {:?} references missing bottom profile {:?} during slicing",
                node_id, profiles[0]
            ),
        });
        return Vec::new();
    };
    let Some(top) = graph.profiles.get(&profiles[1]) else {
        diagnostics.push(ManufacturingDiagnostic {
            code: "missing-profile".to_owned(),
            node: Some(node_id),
            message: format!(
                "loft node {:?} references missing top profile {:?} during slicing",
                node_id, profiles[1]
            ),
        });
        return Vec::new();
    };
    for profile in [bottom, top] {
        if !profile.closed || profile.segment_count != 4 {
            diagnostics.push(ManufacturingDiagnostic {
                code: "unsupported-profile".to_owned(),
                node: Some(node_id),
                message: format!(
                    "manufacturing compiler supports Loft only for closed rectangular profiles, got closed={} segments={}",
                    profile.closed, profile.segment_count
                ),
            });
            return Vec::new();
        }
        if profile.bounds.min.x >= profile.bounds.max.x
            || profile.bounds.min.y >= profile.bounds.max.y
        {
            diagnostics.push(ManufacturingDiagnostic {
                code: "invalid-profile-bounds".to_owned(),
                node: Some(node_id),
                message: format!(
                    "loft node {:?} has invalid rectangular profile bounds {:?}",
                    node_id, profile.bounds
                ),
            });
            return Vec::new();
        }
    }

    let bottom_z = bottom.bounds.min.z.min(bottom.bounds.max.z);
    let top_z = top.bounds.min.z.max(top.bounds.max.z);
    if bottom_z >= top_z {
        diagnostics.push(ManufacturingDiagnostic {
            code: "invalid-profile-bounds".to_owned(),
            node: Some(node_id),
            message: format!(
                "loft node {:?} needs profiles at increasing z positions, got {:?} then {:?}",
                node_id, bottom.bounds, top.bounds
            ),
        });
        return Vec::new();
    }
    if z < bottom_z || z > top_z {
        return Vec::new();
    }

    let t = ((z - bottom_z) / (top_z - bottom_z)).clamp(0.0, 1.0);
    let bounds = Aabb64 {
        min: Vec3d::new(
            lerp(bottom.bounds.min.x, top.bounds.min.x, t),
            lerp(bottom.bounds.min.y, top.bounds.min.y, t),
            z,
        ),
        max: Vec3d::new(
            lerp(bottom.bounds.max.x, top.bounds.max.x, t),
            lerp(bottom.bounds.max.y, top.bounds.max.y, t),
            z,
        ),
    };
    vec![RawPolygon::rectangle(bounds, FeatureId(node_id.0))]
}

fn collapse_contained_union_polygons(mut polygons: Vec<RawPolygon>) -> Vec<RawPolygon> {
    let mut remove = vec![false; polygons.len()];
    let mut contained_features = vec![Vec::<FeatureId>::new(); polygons.len()];

    for index in 0..polygons.len() {
        if !polygons[index].holes.is_empty() {
            continue;
        }
        for container_index in 0..polygons.len() {
            if index == container_index || !polygons[container_index].holes.is_empty() {
                continue;
            }
            if polygons[index].contains_polygon(&polygons[container_index])
                && container_index > index
            {
                continue;
            }
            if polygons[container_index].contains_polygon(&polygons[index]) {
                remove[index] = true;
                contained_features[container_index]
                    .extend(polygons[index].source_features.iter().copied());
                break;
            }
        }
    }

    for (polygon, features) in polygons.iter_mut().zip(contained_features) {
        for feature in features {
            if !polygon.source_features.contains(&feature) {
                polygon.source_features.push(feature);
            }
        }
    }

    polygons
        .into_iter()
        .enumerate()
        .filter_map(|(index, polygon)| (!remove[index]).then_some(polygon))
        .collect()
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

fn axis_is_default_z(axis: boon_solid_model::Axis3d) -> bool {
    axis.origin.x.abs() <= f64::EPSILON
        && axis.origin.y.abs() <= f64::EPSILON
        && axis.direction.x.abs() <= f64::EPSILON
        && axis.direction.y.abs() <= f64::EPSILON
        && (axis.direction.z.abs() - 1.0).abs() <= f64::EPSILON
}

fn section_shell_polygons(
    graph: &SolidGraph,
    shell_node_id: SolidNodeId,
    child: SolidNodeId,
    thickness: f64,
    z: f64,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> Vec<RawPolygon> {
    let Some(child_node) = graph.nodes.get(&child) else {
        diagnostics.push(ManufacturingDiagnostic {
            code: "missing-solid-node".to_owned(),
            node: Some(child),
            message: format!(
                "shell node {:?} references missing child {:?} during slicing",
                shell_node_id, child
            ),
        });
        return Vec::new();
    };
    if !matches!(
        child_node.op,
        SolidOp::Box { .. } | SolidOp::RoundedBox { .. }
    ) {
        diagnostics.push(ManufacturingDiagnostic {
            code: "unsupported-solid-op".to_owned(),
            node: Some(shell_node_id),
            message: format!(
                "manufacturing compiler supports Shell only for box-like children, got {:?}",
                child_node.op
            ),
        });
        return Vec::new();
    }
    if !thickness.is_finite() || thickness <= f64::EPSILON {
        diagnostics.push(ManufacturingDiagnostic {
            code: "invalid-shell-thickness".to_owned(),
            node: Some(shell_node_id),
            message: format!(
                "shell node {:?} has invalid thickness {thickness}",
                shell_node_id
            ),
        });
        return Vec::new();
    }

    let bounds = child_node.bounds;
    if z < bounds.min.z || z > bounds.max.z {
        return Vec::new();
    }
    let inner_bounds = Aabb64 {
        min: Vec3d::new(
            bounds.min.x + thickness,
            bounds.min.y + thickness,
            bounds.min.z + thickness,
        ),
        max: Vec3d::new(
            bounds.max.x - thickness,
            bounds.max.y - thickness,
            bounds.max.z - thickness,
        ),
    };
    if inner_bounds.min.x >= inner_bounds.max.x
        || inner_bounds.min.y >= inner_bounds.max.y
        || inner_bounds.min.z >= inner_bounds.max.z
    {
        diagnostics.push(ManufacturingDiagnostic {
            code: "invalid-shell-thickness".to_owned(),
            node: Some(shell_node_id),
            message: format!(
                "shell node {:?} thickness {thickness} leaves no printable inner cavity inside {:?}",
                shell_node_id, bounds
            ),
        });
        return Vec::new();
    }

    let in_open_cavity = z > inner_bounds.min.z && z < inner_bounds.max.z;
    if in_open_cavity {
        vec![RawPolygon::rectangle_with_rectangular_hole(
            bounds,
            inner_bounds,
            child_node.feature_id,
            shell_node_id,
        )]
    } else {
        vec![RawPolygon::rectangle(bounds, child_node.feature_id)]
    }
}

fn printable_z_bounds(
    bundle: &SolidModelBundle,
    request: &PrintCompileRequest,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> Option<(f64, f64)> {
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for instance in bundle
        .assembly
        .instances
        .iter()
        .filter(|instance| request_includes_instance(request, instance.id))
    {
        let Some(part) = bundle.assembly.parts.get(&instance.part) else {
            continue;
        };
        if part.manufacturing_role != ManufacturingRole::PrintableSolid {
            continue;
        }
        let Some(graph) = bundle.solids.get(&part.geometry) else {
            continue;
        };
        let Some(root) = graph.nodes.get(&part.root) else {
            diagnostics.push(ManufacturingDiagnostic {
                code: "missing-part-root".to_owned(),
                node: Some(part.root),
                message: format!("part {:?} root is missing during slicing", part.id),
            });
            continue;
        };
        let translation = translation_from_matrix(instance.transform);
        min_z = min_z.min(root.bounds.min.z + translation.z);
        max_z = max_z.max(root.bounds.max.z + translation.z);
    }
    min_z.is_finite().then_some((min_z, max_z))
}

fn validate_request(request: &PrintCompileRequest) -> Vec<ManufacturingDiagnostic> {
    let mut diagnostics = Vec::new();
    for (name, value, positive) in [
        ("layer_height", request.layer_height, true),
        ("xy_error", request.xy_error, false),
        ("z_error", request.z_error, false),
        ("minimum_feature", request.minimum_feature, false),
        ("integer_grid", request.integer_grid, true),
    ] {
        if !value.is_finite() || (positive && value <= 0.0) || (!positive && value < 0.0) {
            diagnostics.push(ManufacturingDiagnostic {
                code: "invalid-print-request".to_owned(),
                node: None,
                message: format!("print request `{name}` has invalid value {value}"),
            });
        }
    }
    if !aabb_is_finite(request.build_volume) {
        diagnostics.push(ManufacturingDiagnostic {
            code: "invalid-build-volume".to_owned(),
            node: None,
            message: "print request build volume must be finite".to_owned(),
        });
    }
    diagnostics
}

fn validate_printable_roles(
    bundle: &SolidModelBundle,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) {
    let printable_count = bundle
        .assembly
        .parts
        .values()
        .filter(|part| part.manufacturing_role == ManufacturingRole::PrintableSolid)
        .count();
    if printable_count == 0 {
        diagnostics.push(ManufacturingDiagnostic {
            code: "no-printable-parts".to_owned(),
            node: None,
            message: "manufacturing compile requires at least one PrintableSolid part".to_owned(),
        });
    }
}

fn validate_print_scope(
    bundle: &SolidModelBundle,
    request: &PrintCompileRequest,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) {
    match &request.scope {
        PrintCompileScope::WholeAssembly => {}
        PrintCompileScope::SelectedInstances { instances } => {
            if instances.is_empty() {
                diagnostics.push(ManufacturingDiagnostic {
                    code: "empty-print-scope".to_owned(),
                    node: None,
                    message: "selected-instance print scope must contain at least one instance"
                        .to_owned(),
                });
            }
            for selected in instances {
                let Some(instance) = bundle
                    .assembly
                    .instances
                    .iter()
                    .find(|instance| instance.id == *selected)
                else {
                    diagnostics.push(ManufacturingDiagnostic {
                        code: "selected-instance-missing".to_owned(),
                        node: None,
                        message: format!(
                            "selected-instance print scope references missing instance {:?}",
                            selected
                        ),
                    });
                    continue;
                };
                let Some(part) = bundle.assembly.parts.get(&instance.part) else {
                    diagnostics.push(ManufacturingDiagnostic {
                        code: "selected-instance-part-missing".to_owned(),
                        node: None,
                        message: format!(
                            "selected-instance print scope references instance {:?} with missing part {:?}",
                            instance.id, instance.part
                        ),
                    });
                    continue;
                };
                if part.manufacturing_role != ManufacturingRole::PrintableSolid {
                    diagnostics.push(ManufacturingDiagnostic {
                        code: "selected-instance-not-printable".to_owned(),
                        node: Some(part.root),
                        message: format!(
                            "selected-instance print scope references non-printable instance {:?} with role {:?}",
                            instance.id, part.manufacturing_role
                        ),
                    });
                }
            }
        }
    }
}

fn request_includes_instance(request: &PrintCompileRequest, instance: PartInstanceId) -> bool {
    match &request.scope {
        PrintCompileScope::WholeAssembly => true,
        PrintCompileScope::SelectedInstances { instances } => instances.contains(&instance),
    }
}

fn scoped_printable_part_count(bundle: &SolidModelBundle, request: &PrintCompileRequest) -> usize {
    bundle
        .assembly
        .instances
        .iter()
        .filter(|instance| request_includes_instance(request, instance.id))
        .filter_map(|instance| bundle.assembly.parts.get(&instance.part))
        .filter(|part| part.manufacturing_role == ManufacturingRole::PrintableSolid)
        .map(|part| part.id)
        .collect::<BTreeSet<_>>()
        .len()
}

fn scoped_printable_instance_count(
    bundle: &SolidModelBundle,
    request: &PrintCompileRequest,
) -> usize {
    bundle
        .assembly
        .instances
        .iter()
        .filter(|instance| request_includes_instance(request, instance.id))
        .filter_map(|instance| {
            bundle
                .assembly
                .parts
                .get(&instance.part)
                .map(|part| (instance, part))
        })
        .filter(|(_, part)| part.manufacturing_role == ManufacturingRole::PrintableSolid)
        .count()
}

fn split_axis_for_bounds(bounds: Aabb64, build_volume: Aabb64) -> Option<SplitAxis> {
    let overflow_x = (bounds.max.x - bounds.min.x) - (build_volume.max.x - build_volume.min.x);
    let overflow_y = (bounds.max.y - bounds.min.y) - (build_volume.max.y - build_volume.min.y);
    let overflow_z = (bounds.max.z - bounds.min.z) - (build_volume.max.z - build_volume.min.z);
    [
        (SplitAxis::X, overflow_x),
        (SplitAxis::Y, overflow_y),
        (SplitAxis::Z, overflow_z),
    ]
    .into_iter()
    .filter(|(_, overflow)| *overflow > 0.0)
    .max_by(|(_, left), (_, right)| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
    .map(|(axis, _)| axis)
}

fn build_split_preparation(
    bounds: Aabb64,
    axis: SplitAxis,
    segment_count: usize,
    connector_strategy: ConnectorStrategy,
    minimum_feature: f64,
) -> (Vec<SplitSegmentPlan>, Vec<ConnectorPlan>) {
    let segment_count = segment_count.max(1);
    let split_segments = split_segments_for_bounds(bounds, axis, segment_count);
    let connector_plans =
        if connector_strategy == ConnectorStrategy::PlannedDowelPins && split_segments.len() > 1 {
            connector_plans_for_split(bounds, axis, &split_segments, minimum_feature)
        } else {
            Vec::new()
        };
    (split_segments, connector_plans)
}

fn split_segments_for_bounds(
    bounds: Aabb64,
    axis: SplitAxis,
    segment_count: usize,
) -> Vec<SplitSegmentPlan> {
    let start = axis_min(bounds, axis);
    let end = axis_max(bounds, axis);
    let step = (end - start) / segment_count as f64;
    (0..segment_count)
        .map(|index| {
            let mut segment_bounds = bounds;
            let min = start + step * index as f64;
            let max = if index + 1 == segment_count {
                end
            } else {
                start + step * (index + 1) as f64
            };
            set_axis_min(&mut segment_bounds, axis, min);
            set_axis_max(&mut segment_bounds, axis, max);
            SplitSegmentPlan {
                index,
                axis,
                bounds: segment_bounds,
            }
        })
        .collect()
}

fn connector_plans_for_split(
    bounds: Aabb64,
    axis: SplitAxis,
    segments: &[SplitSegmentPlan],
    minimum_feature: f64,
) -> Vec<ConnectorPlan> {
    let Some(first_boundary) = segments
        .windows(2)
        .next()
        .map(|pair| (axis_max(pair[0].bounds, axis) + axis_min(pair[1].bounds, axis)) * 0.5)
    else {
        return Vec::new();
    };
    let (primary, secondary) = connector_layout_axes(axis, bounds);
    let primary_min = axis_min(bounds, primary);
    let primary_max = axis_max(bounds, primary);
    let secondary_mid = (axis_min(bounds, secondary) + axis_max(bounds, secondary)) * 0.5;
    let diameter = (minimum_feature * 10.0).max(4.0);
    let length = diameter * 3.0;

    (0..2)
        .map(|pair_index| {
            let fraction = (pair_index + 1) as f64 / 3.0;
            let mut center = Vec3d::new(0.0, 0.0, 0.0);
            set_vec_axis(&mut center, axis, first_boundary);
            set_vec_axis(
                &mut center,
                primary,
                primary_min + (primary_max - primary_min) * fraction,
            );
            set_vec_axis(&mut center, secondary, secondary_mid);
            ConnectorPlan {
                pair_index,
                strategy: ConnectorStrategy::PlannedDowelPins,
                axis,
                center,
                diameter,
                length,
            }
        })
        .collect()
}

fn connector_layout_axes(axis: SplitAxis, bounds: Aabb64) -> (SplitAxis, SplitAxis) {
    let candidates = match axis {
        SplitAxis::X => [SplitAxis::Y, SplitAxis::Z],
        SplitAxis::Y => [SplitAxis::X, SplitAxis::Z],
        SplitAxis::Z => [SplitAxis::X, SplitAxis::Y],
    };
    let first_extent = axis_extent(bounds, candidates[0]);
    let second_extent = axis_extent(bounds, candidates[1]);
    if first_extent >= second_extent {
        (candidates[0], candidates[1])
    } else {
        (candidates[1], candidates[0])
    }
}

fn axis_extent(bounds: Aabb64, axis: SplitAxis) -> f64 {
    axis_max(bounds, axis) - axis_min(bounds, axis)
}

fn axis_min(bounds: Aabb64, axis: SplitAxis) -> f64 {
    match axis {
        SplitAxis::X => bounds.min.x,
        SplitAxis::Y => bounds.min.y,
        SplitAxis::Z => bounds.min.z,
    }
}

fn axis_max(bounds: Aabb64, axis: SplitAxis) -> f64 {
    match axis {
        SplitAxis::X => bounds.max.x,
        SplitAxis::Y => bounds.max.y,
        SplitAxis::Z => bounds.max.z,
    }
}

fn set_axis_min(bounds: &mut Aabb64, axis: SplitAxis, value: f64) {
    match axis {
        SplitAxis::X => bounds.min.x = value,
        SplitAxis::Y => bounds.min.y = value,
        SplitAxis::Z => bounds.min.z = value,
    }
}

fn set_axis_max(bounds: &mut Aabb64, axis: SplitAxis, value: f64) {
    match axis {
        SplitAxis::X => bounds.max.x = value,
        SplitAxis::Y => bounds.max.y = value,
        SplitAxis::Z => bounds.max.z = value,
    }
}

fn set_vec_axis(vector: &mut Vec3d, axis: SplitAxis, value: f64) {
    match axis {
        SplitAxis::X => vector.x = value,
        SplitAxis::Y => vector.y = value,
        SplitAxis::Z => vector.z = value,
    }
}

fn vec_axis(vector: Vec3d, axis: SplitAxis) -> f64 {
    match axis {
        SplitAxis::X => vector.x,
        SplitAxis::Y => vector.y,
        SplitAxis::Z => vector.z,
    }
}

fn connector_fit_diagnostics(
    printable_bounds: Option<Aabb64>,
    split_axis: Option<SplitAxis>,
    split_segments: &[SplitSegmentPlan],
    connectors: &[ConnectorPlan],
    minimum_feature: f64,
    grid: f64,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> (ConnectorFitStatus, usize) {
    if split_segments.is_empty() && connectors.is_empty() {
        return (ConnectorFitStatus::NotRequired, 0);
    }
    let Some(axis) = split_axis else {
        diagnostics.push(ManufacturingDiagnostic {
            code: "connector-fit".to_owned(),
            node: None,
            message: "connector fit validation requires a split axis".to_owned(),
        });
        return (ConnectorFitStatus::Fail, 1);
    };
    let Some(bounds) = printable_bounds else {
        diagnostics.push(ManufacturingDiagnostic {
            code: "connector-fit".to_owned(),
            node: None,
            message: "connector fit validation requires printable bounds".to_owned(),
        });
        return (ConnectorFitStatus::Fail, 1);
    };
    if connectors.is_empty() {
        diagnostics.push(ManufacturingDiagnostic {
            code: "connector-fit".to_owned(),
            node: None,
            message: "split preparation requires connector plans for fit validation".to_owned(),
        });
        return (ConnectorFitStatus::Fail, 1);
    }

    let seam_pairs = split_segments
        .windows(2)
        .map(|pair| {
            let boundary = (axis_max(pair[0].bounds, axis) + axis_min(pair[1].bounds, axis)) * 0.5;
            (boundary, pair[0].bounds, pair[1].bounds)
        })
        .collect::<Vec<_>>();
    let tolerance = grid.max(1.0e-6);
    let mut violation_count = 0;
    for connector in connectors {
        let mut reasons = Vec::new();
        if connector.strategy != ConnectorStrategy::PlannedDowelPins {
            reasons.push("unsupported connector strategy".to_owned());
        }
        if connector.axis != axis {
            reasons.push(format!(
                "connector axis {:?} does not match split axis {:?}",
                connector.axis, axis
            ));
        }
        if connector.diameter < minimum_feature {
            reasons.push(format!(
                "connector diameter {:.3} is below minimum feature {:.3}",
                connector.diameter, minimum_feature
            ));
        }
        if connector.length * 0.5 < connector.diameter {
            reasons.push(format!(
                "connector length {:.3} gives less than one diameter of engagement per side",
                connector.length
            ));
        }
        if !aabb_within(connector_bounds(connector), bounds) {
            reasons.push("connector bounds are outside printable assembly bounds".to_owned());
        }
        let center_on_axis = vec_axis(connector.center, axis);
        let seam_match = seam_pairs
            .iter()
            .find(|(boundary, _, _)| (center_on_axis - *boundary).abs() <= tolerance);
        match seam_match {
            Some((boundary, left, right)) => {
                let half_length = connector.length * 0.5;
                let left_engagement = *boundary - axis_min(*left, axis);
                let right_engagement = axis_max(*right, axis) - *boundary;
                if half_length > left_engagement + tolerance {
                    reasons.push(format!(
                        "connector left engagement {:.3} exceeds segment depth {:.3}",
                        half_length, left_engagement
                    ));
                }
                if half_length > right_engagement + tolerance {
                    reasons.push(format!(
                        "connector right engagement {:.3} exceeds segment depth {:.3}",
                        half_length, right_engagement
                    ));
                }
            }
            None => reasons.push("connector center is not aligned to a split seam".to_owned()),
        }

        if !reasons.is_empty() {
            violation_count += 1;
            diagnostics.push(ManufacturingDiagnostic {
                code: "connector-fit".to_owned(),
                node: None,
                message: format!(
                    "connector {} failed fit validation: {}",
                    connector.pair_index,
                    reasons.join("; ")
                ),
            });
        }
    }

    if violation_count == 0 {
        (ConnectorFitStatus::Pass, 0)
    } else {
        (ConnectorFitStatus::Fail, violation_count)
    }
}

fn clearance_diagnostics(
    instances: &[PrintableInstanceBounds],
    minimum_clearance: f64,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> (Option<f64>, usize) {
    if !minimum_clearance.is_finite() || instances.len() < 2 {
        return (None, 0);
    }

    let mut minimum_observed = f64::INFINITY;
    let mut violation_count = 0;
    for (left_index, left) in instances.iter().enumerate() {
        for right in instances.iter().skip(left_index + 1) {
            let clearance = aabb_clearance(left.bounds, right.bounds);
            minimum_observed = minimum_observed.min(clearance);
            if clearance < minimum_clearance {
                violation_count += 1;
                diagnostics.push(ManufacturingDiagnostic {
                    code: "clearance-violation".to_owned(),
                    node: Some(left.root),
                    message: format!(
                        "printable part {:?} instance {:?} clearance to part {:?} instance {:?} is {clearance:.3}, below minimum {minimum_clearance:.3}",
                        left.part, left.instance, right.part, right.instance
                    ),
                });
            }
        }
    }

    (
        minimum_observed.is_finite().then_some(minimum_observed),
        violation_count,
    )
}

fn wall_thickness_diagnostics(
    instances: &[PrintableInstanceBounds],
    minimum_wall_thickness: f64,
    diagnostics: &mut Vec<ManufacturingDiagnostic>,
) -> (Option<f64>, usize) {
    if !minimum_wall_thickness.is_finite() || minimum_wall_thickness < 0.0 {
        return (None, 0);
    }

    let mut minimum_observed = f64::INFINITY;
    let mut violation_count = 0;
    for instance in instances {
        let Some(thickness) = instance.wall_thickness else {
            continue;
        };
        minimum_observed = minimum_observed.min(thickness);
        if thickness < minimum_wall_thickness {
            violation_count += 1;
            diagnostics.push(ManufacturingDiagnostic {
                code: "wall-thickness-violation".to_owned(),
                node: Some(instance.root),
                message: format!(
                    "printable part {:?} instance {:?} wall thickness is {thickness:.3}, below minimum {minimum_wall_thickness:.3}",
                    instance.part, instance.instance
                ),
            });
        }
    }

    (
        minimum_observed.is_finite().then_some(minimum_observed),
        violation_count,
    )
}

fn estimated_wall_thickness(graph: &SolidGraph, root: SolidNodeId) -> Option<f64> {
    estimated_wall_thickness_for_node(graph, root, &mut BTreeSet::new())
}

fn estimated_wall_thickness_for_node(
    graph: &SolidGraph,
    node_id: SolidNodeId,
    seen: &mut BTreeSet<SolidNodeId>,
) -> Option<f64> {
    if !seen.insert(node_id) {
        return None;
    }
    let result = graph.nodes.get(&node_id).and_then(|node| match &node.op {
        SolidOp::Box { size } | SolidOp::RoundedBox { size, .. } => {
            min_positive([size.x.abs(), size.y.abs(), size.z.abs()])
        }
        SolidOp::Sphere { radius } => positive_value(radius.abs() * 2.0),
        SolidOp::Cylinder { radius, height } => min_positive([radius.abs() * 2.0, height.abs()]),
        SolidOp::Cone {
            radius0,
            radius1,
            height,
        } => {
            let widest_diameter = radius0.abs().max(radius1.abs()) * 2.0;
            min_positive([widest_diameter, height.abs()])
        }
        SolidOp::Torus { minor_radius, .. } => positive_value(minor_radius.abs() * 2.0),
        SolidOp::Shell { child, thickness } => {
            estimated_wall_thickness_for_node(graph, *child, seen).map_or_else(
                || positive_value(thickness.abs()),
                |child_thickness| positive_value(thickness.abs().min(child_thickness)),
            )
        }
        SolidOp::Transform { child, .. } | SolidOp::Offset { child, .. } => {
            estimated_wall_thickness_for_node(graph, *child, seen)
        }
        SolidOp::Union { children } | SolidOp::Intersection { children } => children
            .iter()
            .filter_map(|child| estimated_wall_thickness_for_node(graph, *child, seen))
            .min_by(f64::total_cmp),
        SolidOp::Difference { base, .. } => estimated_wall_thickness_for_node(graph, *base, seen),
        SolidOp::SmoothUnion { a, b, .. } => [
            estimated_wall_thickness_for_node(graph, *a, seen),
            estimated_wall_thickness_for_node(graph, *b, seen),
        ]
        .into_iter()
        .flatten()
        .min_by(f64::total_cmp),
        SolidOp::Extrude { .. }
        | SolidOp::Revolve { .. }
        | SolidOp::Sweep { .. }
        | SolidOp::Loft { .. }
        | SolidOp::Functional { .. }
        | SolidOp::ImportedSolid { .. } => aabb_min_extent(node.bounds),
    });
    seen.remove(&node_id);
    result
}

fn min_positive(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    values
        .into_iter()
        .filter_map(positive_value)
        .min_by(f64::total_cmp)
}

fn positive_value(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn aabb_min_extent(bounds: Aabb64) -> Option<f64> {
    min_positive([
        bounds.max.x - bounds.min.x,
        bounds.max.y - bounds.min.y,
        bounds.max.z - bounds.min.z,
    ])
}

fn aabb_clearance(left: Aabb64, right: Aabb64) -> f64 {
    let dx = axis_clearance(left.min.x, left.max.x, right.min.x, right.max.x);
    let dy = axis_clearance(left.min.y, left.max.y, right.min.y, right.max.y);
    let dz = axis_clearance(left.min.z, left.max.z, right.min.z, right.max.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn axis_clearance(left_min: f64, left_max: f64, right_min: f64, right_max: f64) -> f64 {
    if left_max < right_min {
        right_min - left_max
    } else if right_max < left_min {
        left_min - right_max
    } else {
        0.0
    }
}

fn split_segment_output(
    print: &PrintCompileOutput,
    segment: SplitSegmentPlan,
    connectors: &[ConnectorPlan],
) -> SplitPrintSegmentOutput {
    let grid_bounds = grid_bounds_for_segment(segment.bounds, print.request.integer_grid);
    let mut layers = print
        .layers
        .iter()
        .filter_map(|layer| split_layer_for_segment(layer, segment.bounds, grid_bounds))
        .collect::<Vec<_>>();
    let connector_cutout_hole_count = add_connector_cutouts_to_segment_layers(
        &mut layers,
        &segment,
        connectors,
        grid_bounds,
        print.request.integer_grid,
    );
    let metrics = metrics_for_layers(&layers, print.metrics.printable_part_count);
    let tolerance = tolerance_report_for_request(&print.request, &layers);
    let artifact_hash = split_print_segment_artifact_hash(
        &segment,
        &layers,
        &metrics,
        &tolerance,
        connector_cutout_hole_count,
    );
    SplitPrintSegmentOutput {
        segment,
        layers,
        metrics,
        tolerance,
        connector_cutout_hole_count,
        artifact_hash,
    }
}

fn add_connector_cutouts_to_segment_layers(
    layers: &mut [Layer],
    segment: &SplitSegmentPlan,
    connectors: &[ConnectorPlan],
    grid_bounds: GridBounds2D,
    grid: f64,
) -> usize {
    let mut cutout_count = 0;
    for layer in layers {
        for connector in connectors.iter().filter(|connector| {
            connector.strategy == ConnectorStrategy::PlannedDowelPins
                && connector.axis == segment.axis
                && connector_intersects_segment(connector, segment)
        }) {
            let Some(raw_cutout) = connector_polygon_at_z(connector, layer.z) else {
                continue;
            };
            let cutout = regularize_polygon(raw_cutout, grid);
            let Some(cutout) = clip_polygon_with_holes_to_grid_bounds(&cutout, grid_bounds) else {
                continue;
            };
            let Some(cutout_centroid) = polygon_centroid(&cutout.outer) else {
                continue;
            };
            let mut inserted = false;
            for region in &mut layer.regions {
                for polygon in &mut region.polygons {
                    if polygon_contains_point(&polygon.outer, cutout_centroid) {
                        polygon.holes.push(cutout.outer.clone());
                        polygon
                            .source_features
                            .extend(cutout.source_features.clone());
                        inserted = true;
                    }
                }
            }
            if inserted {
                cutout_count += 1;
            }
        }
    }
    cutout_count
}

fn connector_intersects_segment(connector: &ConnectorPlan, segment: &SplitSegmentPlan) -> bool {
    let connector_bounds = connector_bounds(connector);
    let min = axis_min(connector_bounds, segment.axis);
    let max = axis_max(connector_bounds, segment.axis);
    min < axis_max(segment.bounds, segment.axis) && max > axis_min(segment.bounds, segment.axis)
}

fn layer_contains_hole(layer: &Layer, expected_hole: &[GridPoint2D]) -> bool {
    layer.regions.iter().any(|region| {
        region.polygons.iter().any(|polygon| {
            polygon
                .holes
                .iter()
                .any(|hole| polygon_points_equal(hole, expected_hole))
        })
    })
}

fn polygon_points_equal(left: &[GridPoint2D], right: &[GridPoint2D]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(left, right)| left == right)
}

fn split_layer_for_segment(
    layer: &Layer,
    segment_bounds: Aabb64,
    grid_bounds: GridBounds2D,
) -> Option<Layer> {
    if layer.z < segment_bounds.min.z || layer.z > segment_bounds.max.z {
        return None;
    }
    let regions = layer
        .regions
        .iter()
        .filter_map(|region| split_region_for_grid_bounds(region, grid_bounds))
        .collect::<Vec<_>>();
    (!regions.is_empty()).then(|| Layer {
        index: layer.index,
        z: layer.z,
        regions,
        achieved_error: layer.achieved_error,
        diagnostics: layer.diagnostics.clone(),
    })
}

fn split_region_for_grid_bounds(
    region: &MaterialRegion2D,
    grid_bounds: GridBounds2D,
) -> Option<MaterialRegion2D> {
    let polygons = region
        .polygons
        .iter()
        .filter_map(|polygon| clip_polygon_with_holes_to_grid_bounds(polygon, grid_bounds))
        .collect::<Vec<_>>();
    (!polygons.is_empty()).then(|| MaterialRegion2D {
        part: region.part,
        instance: region.instance,
        material: region.material,
        polygons,
    })
}

fn clip_polygon_with_holes_to_grid_bounds(
    polygon: &PolygonWithHoles,
    grid_bounds: GridBounds2D,
) -> Option<PolygonWithHoles> {
    let outer = clip_points_to_grid_bounds(&polygon.outer, grid_bounds);
    let holes = polygon
        .holes
        .iter()
        .filter_map(|hole| clip_hole_to_grid_bounds(hole, grid_bounds))
        .collect::<Vec<_>>();
    (outer.len() >= 3).then(|| PolygonWithHoles {
        outer,
        holes,
        source_features: polygon.source_features.clone(),
    })
}

fn clip_hole_to_grid_bounds(
    hole: &[GridPoint2D],
    bounds: GridBounds2D,
) -> Option<Vec<GridPoint2D>> {
    match hole_clip_relation(hole, bounds) {
        HoleClipRelation::Inside => {
            let hole = dedupe_polygon_points(hole.to_vec());
            (hole.len() >= 3).then_some(hole)
        }
        HoleClipRelation::Boundary => {
            let clipped = clip_points_to_grid_bounds(hole, bounds);
            (clipped.len() >= 3).then_some(clipped)
        }
        HoleClipRelation::Outside => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoleClipRelation {
    Inside,
    Outside,
    Boundary,
}

fn hole_clip_relation(hole: &[GridPoint2D], bounds: GridBounds2D) -> HoleClipRelation {
    if hole.is_empty() {
        return HoleClipRelation::Outside;
    }
    if hole
        .iter()
        .all(|point| point_inside_grid_bounds(*point, bounds))
    {
        return HoleClipRelation::Inside;
    }
    if clip_points_to_grid_bounds(hole, bounds).is_empty() {
        return HoleClipRelation::Outside;
    }
    HoleClipRelation::Boundary
}

fn point_inside_grid_bounds(point: GridPoint2D, bounds: GridBounds2D) -> bool {
    point.x >= bounds.min_x
        && point.x <= bounds.max_x
        && point.y >= bounds.min_y
        && point.y <= bounds.max_y
}

fn polygon_centroid(points: &[GridPoint2D]) -> Option<GridPoint2D> {
    if points.is_empty() {
        return None;
    }
    let (x, y) = points.iter().fold((0_i128, 0_i128), |(x, y), point| {
        (x + i128::from(point.x), y + i128::from(point.y))
    });
    let count = i128::try_from(points.len()).ok()?;
    Some(GridPoint2D {
        x: (x / count) as i64,
        y: (y / count) as i64,
    })
}

fn polygon_contains_point(points: &[GridPoint2D], point: GridPoint2D) -> bool {
    let Some(bounds) = grid_polygon_bounds(points) else {
        return false;
    };
    point_inside_grid_bounds(point, bounds)
}

fn clip_points_to_grid_bounds(points: &[GridPoint2D], bounds: GridBounds2D) -> Vec<GridPoint2D> {
    let mut clipped = points
        .iter()
        .map(|point| ClipPoint {
            x: point.x as f64,
            y: point.y as f64,
        })
        .collect::<Vec<_>>();
    clipped = clip_edge(
        clipped,
        |point| point.x >= bounds.min_x as f64,
        |a, b| intersect_vertical(a, b, bounds.min_x as f64),
    );
    clipped = clip_edge(
        clipped,
        |point| point.x <= bounds.max_x as f64,
        |a, b| intersect_vertical(a, b, bounds.max_x as f64),
    );
    clipped = clip_edge(
        clipped,
        |point| point.y >= bounds.min_y as f64,
        |a, b| intersect_horizontal(a, b, bounds.min_y as f64),
    );
    clipped = clip_edge(
        clipped,
        |point| point.y <= bounds.max_y as f64,
        |a, b| intersect_horizontal(a, b, bounds.max_y as f64),
    );
    dedupe_polygon_points(
        clipped
            .into_iter()
            .map(|point| GridPoint2D {
                x: point.x.round() as i64,
                y: point.y.round() as i64,
            })
            .collect(),
    )
}

#[derive(Clone, Copy, Debug)]
struct ClipPoint {
    x: f64,
    y: f64,
}

fn clip_edge(
    points: Vec<ClipPoint>,
    inside: impl Fn(ClipPoint) -> bool,
    intersection: impl Fn(ClipPoint, ClipPoint) -> ClipPoint,
) -> Vec<ClipPoint> {
    let Some(mut previous) = points.last().copied() else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut previous_inside = inside(previous);
    for current in points {
        let current_inside = inside(current);
        if current_inside {
            if !previous_inside {
                output.push(intersection(previous, current));
            }
            output.push(current);
        } else if previous_inside {
            output.push(intersection(previous, current));
        }
        previous = current;
        previous_inside = current_inside;
    }
    output
}

fn intersect_vertical(from: ClipPoint, to: ClipPoint, x: f64) -> ClipPoint {
    let dx = to.x - from.x;
    if dx.abs() <= f64::EPSILON {
        return ClipPoint { x, y: from.y };
    }
    let t = (x - from.x) / dx;
    ClipPoint {
        x,
        y: from.y + (to.y - from.y) * t,
    }
}

fn intersect_horizontal(from: ClipPoint, to: ClipPoint, y: f64) -> ClipPoint {
    let dy = to.y - from.y;
    if dy.abs() <= f64::EPSILON {
        return ClipPoint { x: from.x, y };
    }
    let t = (y - from.y) / dy;
    ClipPoint {
        x: from.x + (to.x - from.x) * t,
        y,
    }
}

fn dedupe_polygon_points(mut points: Vec<GridPoint2D>) -> Vec<GridPoint2D> {
    points.dedup();
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    points
}

fn grid_bounds_for_segment(bounds: Aabb64, grid: f64) -> GridBounds2D {
    GridBounds2D {
        min_x: (bounds.min.x / grid).round() as i64,
        min_y: (bounds.min.y / grid).round() as i64,
        max_x: (bounds.max.x / grid).round() as i64,
        max_y: (bounds.max.y / grid).round() as i64,
    }
}

fn metrics_for_layers(layers: &[Layer], printable_part_count: usize) -> ManufacturingMetrics {
    let printable_instance_count = layers
        .iter()
        .flat_map(|layer| &layer.regions)
        .map(|region| region.instance)
        .collect::<BTreeSet<_>>()
        .len();
    ManufacturingMetrics {
        layer_count: layers.len(),
        material_region_count: layers.iter().map(|layer| layer.regions.len()).sum(),
        polygon_count: layers
            .iter()
            .flat_map(|layer| &layer.regions)
            .map(|region| region.polygons.len())
            .sum(),
        hole_count: layers
            .iter()
            .flat_map(|layer| &layer.regions)
            .flat_map(|region| &region.polygons)
            .map(|polygon| polygon.holes.len())
            .sum(),
        printable_part_count,
        printable_instance_count,
        unsupported_operation_count: layers
            .iter()
            .flat_map(|layer| &layer.diagnostics)
            .filter(|diagnostic| diagnostic.code == "unsupported-solid-op")
            .count(),
    }
}

fn material_region_conflicts(
    layer_index: u32,
    regions: &[MaterialRegion2D],
) -> Vec<ManufacturingDiagnostic> {
    let mut diagnostics = Vec::new();
    for (left_index, left) in regions.iter().enumerate() {
        for right in regions.iter().skip(left_index + 1) {
            if left.material == right.material {
                continue;
            }
            let overlaps = left.polygons.iter().any(|left_polygon| {
                let Some(left_bounds) = polygon_bounds(left_polygon) else {
                    return false;
                };
                right.polygons.iter().any(|right_polygon| {
                    polygon_bounds(right_polygon)
                        .is_some_and(|right_bounds| bounds_overlap(left_bounds, right_bounds))
                })
            });
            if overlaps {
                diagnostics.push(ManufacturingDiagnostic {
                    code: "material-region-conflict".to_owned(),
                    node: None,
                    message: format!(
                        "layer {layer_index} has overlapping printable regions with different materials: part {:?} instance {:?} material {:?} overlaps part {:?} instance {:?} material {:?}",
                        left.part,
                        left.instance,
                        left.material,
                        right.part,
                        right.instance,
                        right.material
                    ),
                });
            }
        }
    }
    diagnostics
}

fn polygon_bounds(polygon: &PolygonWithHoles) -> Option<GridBounds2D> {
    grid_polygon_bounds(&polygon.outer)
}

fn grid_polygon_bounds(points: &[GridPoint2D]) -> Option<GridBounds2D> {
    let first = points.first()?;
    let mut bounds = GridBounds2D {
        min_x: first.x,
        min_y: first.y,
        max_x: first.x,
        max_y: first.y,
    };
    for point in points {
        bounds.min_x = bounds.min_x.min(point.x);
        bounds.min_y = bounds.min_y.min(point.y);
        bounds.max_x = bounds.max_x.max(point.x);
        bounds.max_y = bounds.max_y.max(point.y);
    }
    Some(bounds)
}

fn bounds_overlap(left: GridBounds2D, right: GridBounds2D) -> bool {
    left.min_x < right.max_x
        && left.max_x > right.min_x
        && left.min_y < right.max_y
        && left.max_y > right.min_y
}

fn tolerance_report_for_request(
    request: &PrintCompileRequest,
    layers: &[Layer],
) -> ManufacturingToleranceReport {
    let max_layer_achieved_error = layers
        .iter()
        .map(|layer| layer.achieved_error)
        .fold(0.0, f64::max);
    let achieved_xy_error = max_layer_achieved_error;
    let achieved_z_error = if layers.is_empty() {
        0.0
    } else {
        request.layer_height * 0.5
    };
    ManufacturingToleranceReport {
        requested_xy_error: request.xy_error,
        requested_z_error: request.z_error,
        requested_minimum_feature: request.minimum_feature,
        requested_integer_grid: request.integer_grid,
        achieved_xy_error,
        achieved_z_error,
        max_layer_achieved_error,
        within_requested_xy_error: achieved_xy_error <= request.xy_error,
        within_requested_z_error: achieved_z_error <= request.z_error,
    }
}

fn manufacturing_artifact_hash(
    request: &PrintCompileRequest,
    layers: &[Layer],
    tolerance: &ManufacturingToleranceReport,
) -> String {
    let bytes = serde_json::to_vec(&(request, layers, tolerance)).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

#[derive(Serialize)]
struct PrintPreparationArtifactPayload {
    preparation_status: PrintPreparationStatus,
    split_required: bool,
    split_axis: Option<SplitAxis>,
    split_segments: Vec<SplitSegmentPlan>,
    connector_plans: Vec<ConnectorPlan>,
    connector_fit_status: ConnectorFitStatus,
    connector_fit_violation_count: usize,
    minimum_wall_thickness: f64,
    minimum_wall_thickness_observed: Option<f64>,
    wall_thickness_violation_count: usize,
    diagnostics: Vec<ManufacturingDiagnostic>,
    visual_mesh_used_for_manufacturing: bool,
}

fn preparation_artifact_hash(payload: &PrintPreparationArtifactPayload) -> String {
    let bytes = serde_json::to_vec(payload).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn split_print_segment_artifact_hash(
    segment: &SplitSegmentPlan,
    layers: &[Layer],
    metrics: &ManufacturingMetrics,
    tolerance: &ManufacturingToleranceReport,
    connector_cutout_hole_count: usize,
) -> String {
    let bytes = serde_json::to_vec(&(
        segment,
        layers,
        metrics,
        tolerance,
        connector_cutout_hole_count,
    ))
    .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn split_print_artifact_hash(
    source_hash: &str,
    preparation_hash: &str,
    segments: &[SplitPrintSegmentOutput],
) -> String {
    let bytes = serde_json::to_vec(&(source_hash, preparation_hash, segments)).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn connector_print_artifact_hash(
    preparation_hash: &str,
    connectors: &[ConnectorPlan],
    layers: &[Layer],
    metrics: &ManufacturingMetrics,
    tolerance: &ManufacturingToleranceReport,
) -> String {
    let bytes = serde_json::to_vec(&(preparation_hash, connectors, layers, metrics, tolerance))
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn regularize_polygon(polygon: RawPolygon, grid: f64) -> PolygonWithHoles {
    PolygonWithHoles {
        outer: polygon
            .outer
            .into_iter()
            .map(|point| regularize_point(point, grid))
            .collect(),
        holes: polygon
            .holes
            .into_iter()
            .map(|hole| {
                hole.into_iter()
                    .map(|point| regularize_point(point, grid))
                    .collect()
            })
            .collect(),
        source_features: polygon.source_features,
    }
}

fn regularize_point(point: RawPoint2D, grid: f64) -> GridPoint2D {
    GridPoint2D {
        x: (point.x / grid).round() as i64,
        y: (point.y / grid).round() as i64,
    }
}

fn bounds_inside_build_volume(polygons: &[RawPolygon], build_volume: &Aabb64) -> bool {
    polygons.iter().all(|polygon| {
        polygon
            .outer
            .iter()
            .chain(polygon.holes.iter().flatten())
            .all(|point| {
                point.x >= build_volume.min.x
                    && point.x <= build_volume.max.x
                    && point.y >= build_volume.min.y
                    && point.y <= build_volume.max.y
            })
    })
}

fn aabb_within(bounds: Aabb64, outer: Aabb64) -> bool {
    bounds.min.x >= outer.min.x
        && bounds.max.x <= outer.max.x
        && bounds.min.y >= outer.min.y
        && bounds.max.y <= outer.max.y
        && bounds.min.z >= outer.min.z
        && bounds.max.z <= outer.max.z
}

fn aabb_is_finite(bounds: Aabb64) -> bool {
    [
        bounds.min.x,
        bounds.min.y,
        bounds.min.z,
        bounds.max.x,
        bounds.max.y,
        bounds.max.z,
    ]
    .into_iter()
    .all(f64::is_finite)
}

fn center_from_bounds(bounds: Aabb64) -> RawPoint2D {
    RawPoint2D {
        x: (bounds.min.x + bounds.max.x) * 0.5,
        y: (bounds.min.y + bounds.max.y) * 0.5,
    }
}

fn translation_from_matrix(transform: Mat4d) -> boon_solid_model::Vec3d {
    boon_solid_model::Vec3d::new(
        transform.columns[3][0],
        transform.columns[3][1],
        transform.columns[3][2],
    )
}

impl RawPolygon {
    fn rectangle(bounds: Aabb64, feature: FeatureId) -> Self {
        Self {
            outer: vec![
                RawPoint2D {
                    x: bounds.min.x,
                    y: bounds.min.y,
                },
                RawPoint2D {
                    x: bounds.max.x,
                    y: bounds.min.y,
                },
                RawPoint2D {
                    x: bounds.max.x,
                    y: bounds.max.y,
                },
                RawPoint2D {
                    x: bounds.min.x,
                    y: bounds.max.y,
                },
            ],
            holes: Vec::new(),
            source_features: vec![feature],
        }
    }

    fn rectangle_with_rectangular_hole(
        outer_bounds: Aabb64,
        hole_bounds: Aabb64,
        child_feature: FeatureId,
        shell_node: SolidNodeId,
    ) -> Self {
        let mut polygon = Self::rectangle(outer_bounds, child_feature);
        polygon.holes.push(vec![
            RawPoint2D {
                x: hole_bounds.min.x,
                y: hole_bounds.min.y,
            },
            RawPoint2D {
                x: hole_bounds.min.x,
                y: hole_bounds.max.y,
            },
            RawPoint2D {
                x: hole_bounds.max.x,
                y: hole_bounds.max.y,
            },
            RawPoint2D {
                x: hole_bounds.max.x,
                y: hole_bounds.min.y,
            },
        ]);
        polygon.source_features.push(FeatureId(shell_node.0));
        polygon
    }

    fn circle(center: RawPoint2D, radius: f64, feature: FeatureId) -> Self {
        let segments = 32;
        let outer = (0..segments)
            .map(|index| {
                let angle = std::f64::consts::TAU * f64::from(index) / f64::from(segments);
                RawPoint2D {
                    x: center.x + angle.cos() * radius,
                    y: center.y + angle.sin() * radius,
                }
            })
            .collect();
        Self {
            outer,
            holes: Vec::new(),
            source_features: vec![feature],
        }
    }

    fn annulus(
        center: RawPoint2D,
        outer_radius: f64,
        inner_radius: f64,
        feature: FeatureId,
    ) -> Self {
        let mut polygon = Self::circle(center, outer_radius, feature);
        if inner_radius > f64::EPSILON {
            let segments = 32;
            polygon.holes.push(
                (0..segments)
                    .rev()
                    .map(|index| {
                        let angle = std::f64::consts::TAU * f64::from(index) / f64::from(segments);
                        RawPoint2D {
                            x: center.x + angle.cos() * inner_radius,
                            y: center.y + angle.sin() * inner_radius,
                        }
                    })
                    .collect(),
            );
        }
        polygon
    }

    fn translate(mut self, x: f64, y: f64) -> Self {
        for point in &mut self.outer {
            point.x += x;
            point.y += y;
        }
        for hole in &mut self.holes {
            for point in hole {
                point.x += x;
                point.y += y;
            }
        }
        self
    }

    fn centroid(&self) -> RawPoint2D {
        let (x, y, count) = self
            .outer
            .iter()
            .fold((0.0, 0.0, 0_usize), |(x, y, count), point| {
                (x + point.x, y + point.y, count + 1)
            });
        if count == 0 {
            return RawPoint2D { x: 0.0, y: 0.0 };
        }
        RawPoint2D {
            x: x / count as f64,
            y: y / count as f64,
        }
    }

    fn contains_point(&self, point: RawPoint2D) -> bool {
        let min_x = self
            .outer
            .iter()
            .map(|point| point.x)
            .fold(f64::INFINITY, f64::min);
        let max_x = self
            .outer
            .iter()
            .map(|point| point.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = self
            .outer
            .iter()
            .map(|point| point.y)
            .fold(f64::INFINITY, f64::min);
        let max_y = self
            .outer
            .iter()
            .map(|point| point.y)
            .fold(f64::NEG_INFINITY, f64::max);
        point.x >= min_x && point.x <= max_x && point.y >= min_y && point.y <= max_y
    }

    fn contains_polygon(&self, polygon: &RawPolygon) -> bool {
        !polygon.outer.is_empty()
            && polygon
                .outer
                .iter()
                .copied()
                .all(|point| self.contains_point(point))
    }

    fn rectangular_bounds_at_z(&self, z: f64) -> Option<Aabb64> {
        if !self.holes.is_empty() || self.outer.len() != 4 {
            return None;
        }
        let min_x = self
            .outer
            .iter()
            .map(|point| point.x)
            .fold(f64::INFINITY, f64::min);
        let max_x = self
            .outer
            .iter()
            .map(|point| point.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = self
            .outer
            .iter()
            .map(|point| point.y)
            .fold(f64::INFINITY, f64::min);
        let max_y = self
            .outer
            .iter()
            .map(|point| point.y)
            .fold(f64::NEG_INFINITY, f64::max);
        if !min_x.is_finite()
            || !max_x.is_finite()
            || !min_y.is_finite()
            || !max_y.is_finite()
            || min_x >= max_x
            || min_y >= max_y
        {
            return None;
        }
        let corners = [
            (min_x, min_y),
            (max_x, min_y),
            (max_x, max_y),
            (min_x, max_y),
        ];
        let is_axis_aligned_rectangle = corners.iter().all(|corner| {
            self.outer.iter().any(|point| {
                (point.x - corner.0).abs() <= f64::EPSILON
                    && (point.y - corner.1).abs() <= f64::EPSILON
            })
        });
        is_axis_aligned_rectangle.then_some(Aabb64 {
            min: Vec3d::new(min_x, min_y, z),
            max: Vec3d::new(max_x, max_y, z),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_bracket_compiles_to_deterministic_layers_with_holes() {
        let bundle = SolidModelBundle::printable_bracket_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let first = compile_print_job(&bundle, request.clone());
        let second = compile_print_job(&bundle, request);

        assert_eq!(first.status, ManufacturingCompileStatus::Pass);
        assert!(!first.visual_mesh_used_for_manufacturing);
        assert_eq!(first.metrics.printable_part_count, 1);
        assert!(first.metrics.layer_count > 0);
        assert!(first.metrics.material_region_count > 0);
        assert!(first.metrics.hole_count > 0);
        assert_eq!(first.metrics.unsupported_operation_count, 0);
        assert_eq!(first.tolerance.requested_xy_error, 0.05);
        assert_eq!(first.tolerance.requested_z_error, 0.20);
        assert!(first.tolerance.achieved_xy_error <= first.tolerance.requested_xy_error);
        assert!(first.tolerance.achieved_z_error <= first.tolerance.requested_z_error);
        assert!(first.tolerance.within_requested_xy_error);
        assert!(first.tolerance.within_requested_z_error);
        assert_eq!(first.artifact_hash, second.artifact_hash);
        assert!(first.artifact_hash.starts_with("sha256:"));
    }

    #[test]
    fn shell_box_compiles_to_hollow_layers_without_visual_meshes() {
        let bundle = SolidModelBundle::shell_box_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Pass);
        assert!(!output.visual_mesh_used_for_manufacturing);
        assert_eq!(output.metrics.printable_part_count, 1);
        assert_eq!(output.metrics.printable_instance_count, 1);
        assert!(output.metrics.layer_count > 0);
        assert!(output.metrics.polygon_count > 0);
        assert!(output.metrics.hole_count > 0);
        assert_eq!(output.metrics.unsupported_operation_count, 0);
        assert!(output.diagnostics.is_empty());
        assert!(output.layers.iter().any(|layer| {
            layer.regions.iter().any(|region| {
                region
                    .polygons
                    .iter()
                    .any(|polygon| !polygon.holes.is_empty())
            })
        }));
    }

    #[test]
    fn extruded_rectangle_compiles_to_profile_layers_without_visual_meshes() {
        let bundle = SolidModelBundle::extruded_rectangle_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Pass);
        assert!(!output.visual_mesh_used_for_manufacturing);
        assert_eq!(output.metrics.printable_part_count, 1);
        assert_eq!(output.metrics.printable_instance_count, 1);
        assert!(output.metrics.layer_count > 0);
        assert_eq!(output.metrics.polygon_count, output.metrics.layer_count);
        assert_eq!(output.metrics.hole_count, 0);
        assert_eq!(output.metrics.unsupported_operation_count, 0);
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn revolved_ring_compiles_to_annulus_layers_without_visual_meshes() {
        let bundle = SolidModelBundle::revolved_ring_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Pass);
        assert!(!output.visual_mesh_used_for_manufacturing);
        assert_eq!(output.metrics.printable_part_count, 1);
        assert_eq!(output.metrics.printable_instance_count, 1);
        assert!(output.metrics.layer_count > 0);
        assert_eq!(output.metrics.polygon_count, output.metrics.layer_count);
        assert_eq!(output.metrics.hole_count, output.metrics.layer_count);
        assert_eq!(output.metrics.unsupported_operation_count, 0);
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn lofted_rectangle_compiles_to_interpolated_profile_layers_without_visual_meshes() {
        let bundle = SolidModelBundle::lofted_rectangle_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Pass);
        assert!(!output.visual_mesh_used_for_manufacturing);
        assert_eq!(output.metrics.printable_part_count, 1);
        assert_eq!(output.metrics.printable_instance_count, 1);
        assert!(output.metrics.layer_count > 0);
        assert_eq!(output.metrics.polygon_count, output.metrics.layer_count);
        assert_eq!(output.metrics.hole_count, 0);
        assert_eq!(output.metrics.unsupported_operation_count, 0);
        assert!(output.diagnostics.is_empty());

        let first_polygon = &output.layers[0].regions[0].polygons[0];
        let last_polygon = &output
            .layers
            .last()
            .expect("loft should produce layers")
            .regions[0]
            .polygons[0];
        let first_width = first_polygon.outer[1].x - first_polygon.outer[0].x;
        let last_width = last_polygon.outer[1].x - last_polygon.outer[0].x;
        assert!(
            first_width > last_width,
            "loft profile should narrow from bottom to top"
        );
    }

    #[test]
    fn selected_instance_scope_compiles_only_selected_printable_instance() {
        let bundle = SolidModelBundle::parametric_car_fixture();
        let whole = compile_print_job(&bundle, PrintCompileRequest::default_for_bundle(&bundle));
        let selected = compile_print_job(
            &bundle,
            PrintCompileRequest::for_selected_instances(&bundle, [PartInstanceId(3)]),
        );

        assert_eq!(whole.status, ManufacturingCompileStatus::Pass);
        assert_eq!(selected.status, ManufacturingCompileStatus::Pass);
        assert_eq!(whole.metrics.printable_part_count, 2);
        assert_eq!(whole.metrics.printable_instance_count, 5);
        assert_eq!(selected.metrics.printable_part_count, 1);
        assert_eq!(selected.metrics.printable_instance_count, 1);
        assert_ne!(whole.artifact_hash, selected.artifact_hash);
        assert!(!selected.visual_mesh_used_for_manufacturing);
        assert!(selected.layers.iter().all(|layer| {
            layer
                .regions
                .iter()
                .all(|region| region.part == PartId(2) && region.instance == PartInstanceId(3))
        }));
        assert!(selected.metrics.layer_count > 0);
        assert!(selected.metrics.material_region_count > 0);
    }

    #[test]
    fn selected_visual_only_scope_fails_without_printable_layers() {
        let bundle = SolidModelBundle::parametric_car_fixture();
        let selected_visual = compile_print_job(
            &bundle,
            PrintCompileRequest::for_selected_instances(&bundle, [PartInstanceId(2)]),
        );

        assert_eq!(selected_visual.status, ManufacturingCompileStatus::Fail);
        assert!(selected_visual.layers.is_empty());
        assert_eq!(selected_visual.metrics.printable_part_count, 0);
        assert_eq!(selected_visual.metrics.printable_instance_count, 0);
        assert!(selected_visual.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "selected-instance-not-printable"
                && diagnostic.message.contains("PartInstanceId(2)")
        }));
        assert!(!selected_visual.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn minimum_feature_negative_fixture_fails_before_slicing() {
        let bundle = SolidModelBundle::minimum_feature_negative_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Fail);
        assert!(output.layers.is_empty());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "minimum-feature")
        );
        assert!(!output.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn visual_only_fixture_fails_without_materializing_printable_layers() {
        let bundle = SolidModelBundle::visual_only_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Fail);
        assert!(output.layers.is_empty());
        assert_eq!(output.metrics.printable_part_count, 0);
        assert!(!output.visual_mesh_used_for_manufacturing);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "no-printable-parts")
        );
    }

    #[test]
    fn too_small_build_volume_fails_with_diagnostic() {
        let bundle = SolidModelBundle::printable_bracket_fixture();
        let mut request = PrintCompileRequest::default_for_bundle(&bundle);
        request.build_volume = Aabb64 {
            min: boon_solid_model::Vec3d::new(-5.0, -5.0, -5.0),
            max: boon_solid_model::Vec3d::new(5.0, 5.0, 5.0),
        };

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Fail);
        assert!(output.layers.is_empty());
        assert!(!output.visual_mesh_used_for_manufacturing);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "build-volume-z"
                    || diagnostic.code == "build-volume-xy")
        );
    }

    #[test]
    fn unsupported_solid_operation_fails_with_diagnostic() {
        let bundle = SolidModelBundle::unsupported_loft_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Fail);
        assert_eq!(
            output.metrics.unsupported_operation_count,
            output.diagnostics.len()
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unsupported-solid-op")
        );
        assert!(!output.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn curved_primitives_compile_to_analytic_layers_without_visual_meshes() {
        let bundle = SolidModelBundle::curved_primitives_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Pass);
        assert_eq!(output.metrics.printable_part_count, 3);
        assert_eq!(output.metrics.printable_instance_count, 3);
        assert!(output.metrics.layer_count > 0);
        assert!(output.metrics.material_region_count > 0);
        assert!(output.metrics.polygon_count > 0);
        assert!(output.metrics.hole_count > 0);
        assert_eq!(output.metrics.unsupported_operation_count, 0);
        assert!(output.tolerance.within_requested_xy_error);
        assert!(output.tolerance.within_requested_z_error);
        assert!(output.diagnostics.is_empty());
        assert!(!output.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn box_like_intersection_compiles_to_authoritative_layers() {
        let bundle = SolidModelBundle::box_intersection_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Pass);
        assert_eq!(output.metrics.printable_part_count, 1);
        assert_eq!(output.metrics.printable_instance_count, 1);
        assert!(output.metrics.layer_count > 0);
        assert!(output.metrics.polygon_count > 0);
        assert_eq!(output.metrics.unsupported_operation_count, 0);
        assert!(output.diagnostics.is_empty());
        assert!(!output.visual_mesh_used_for_manufacturing);
        assert!(
            output
                .layers
                .iter()
                .flat_map(|layer| &layer.regions)
                .flat_map(|region| &region.polygons)
                .all(|polygon| polygon.source_features.contains(&FeatureId(3))
                    && polygon.source_features.contains(&FeatureId(1))
                    && polygon.source_features.contains(&FeatureId(2)))
        );
    }

    #[test]
    fn curved_intersection_stays_unsupported_until_general_csg_exists() {
        let bundle = SolidModelBundle::curved_intersection_negative_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Fail);
        assert!(output.metrics.unsupported_operation_count > 0);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unsupported-solid-op"
                    && diagnostic.node == Some(SolidNodeId(3)))
        );
        assert!(!output.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn box_slot_difference_compiles_to_authoritative_layers_with_rectangular_holes() {
        let bundle = SolidModelBundle::box_slot_difference_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Pass);
        assert_eq!(output.metrics.printable_part_count, 1);
        assert_eq!(output.metrics.printable_instance_count, 1);
        assert!(output.metrics.layer_count > 0);
        assert_eq!(output.metrics.polygon_count, output.metrics.layer_count);
        assert_eq!(output.metrics.hole_count, output.metrics.layer_count);
        assert_eq!(output.metrics.unsupported_operation_count, 0);
        assert!(output.diagnostics.is_empty());
        assert!(!output.visual_mesh_used_for_manufacturing);
        assert!(
            output
                .layers
                .iter()
                .flat_map(|layer| &layer.regions)
                .flat_map(|region| &region.polygons)
                .all(|polygon| polygon.source_features.contains(&FeatureId(1))
                    && polygon.source_features.contains(&FeatureId(2)))
        );
    }

    #[test]
    fn overlapping_printable_material_regions_fail_with_diagnostic() {
        let bundle = SolidModelBundle::material_region_conflict_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Fail);
        assert!(output.metrics.material_region_count > 0);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "material-region-conflict")
        );
        assert!(!output.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn parametric_car_fixture_compiles_printable_body_and_shared_wheels_only() {
        let bundle = SolidModelBundle::parametric_car_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let output = compile_print_job(&bundle, request);

        assert_eq!(output.status, ManufacturingCompileStatus::Pass);
        assert_eq!(output.metrics.printable_part_count, 2);
        assert!(output.metrics.material_region_count > 0);
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "material-region-conflict")
        );
        assert!(!output.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn parametric_car_print_preparation_detects_split_and_connector_plan() {
        let bundle = SolidModelBundle::parametric_car_fixture();
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let ready = prepare_print_job(&bundle, request.clone());
        assert_eq!(ready.status, PrintPreparationStatus::Ready);
        assert!(ready.fits_build_volume);
        assert!(!ready.split_required);
        assert_eq!(ready.split_axis, None);
        assert_eq!(ready.connector_strategy, ConnectorStrategy::None);
        assert_eq!(ready.connector_pair_count, 0);
        assert_eq!(ready.connector_fit_status, ConnectorFitStatus::NotRequired);
        assert_eq!(ready.connector_fit_violation_count, 0);
        assert_eq!(ready.printable_part_count, 2);
        assert_eq!(ready.printable_instance_count, 5);
        assert_eq!(ready.visual_only_instance_count, 1);
        assert_eq!(ready.clearance_violation_count, 0);
        assert_eq!(ready.wall_thickness_violation_count, 0);
        assert!(
            ready
                .minimum_wall_thickness_observed
                .is_some_and(|thickness| thickness >= ready.minimum_wall_thickness)
        );
        assert!(ready.split_segments.is_empty());
        assert!(ready.connector_plans.is_empty());
        assert!(
            ready
                .minimum_clearance_observed
                .is_some_and(|clearance| clearance >= ready.minimum_clearance)
        );

        let mut narrow_request = request;
        narrow_request.build_volume = Aabb64 {
            min: boon_solid_model::Vec3d::new(-40.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(40.0, 80.0, 40.0),
        };
        let split = prepare_print_job(&bundle, narrow_request);

        assert_eq!(split.status, PrintPreparationStatus::SplitRequired);
        assert!(!split.fits_build_volume);
        assert!(split.split_required);
        assert_eq!(split.split_axis, Some(SplitAxis::X));
        assert_eq!(split.suggested_segment_count, 2);
        assert_eq!(
            split.connector_strategy,
            ConnectorStrategy::PlannedDowelPins
        );
        assert_eq!(split.connector_pair_count, 2);
        assert_eq!(split.connector_fit_status, ConnectorFitStatus::Pass);
        assert_eq!(split.connector_fit_violation_count, 0);
        assert_eq!(split.split_segments.len(), 2);
        assert_eq!(split.connector_plans.len(), 2);
        assert_eq!(split.split_segments[0].axis, SplitAxis::X);
        assert_eq!(split.split_segments[1].axis, SplitAxis::X);
        assert_eq!(split.split_segments[0].bounds.min.x, -59.0);
        assert_eq!(split.split_segments[0].bounds.max.x, 0.0);
        assert_eq!(split.split_segments[1].bounds.min.x, 0.0);
        assert_eq!(split.split_segments[1].bounds.max.x, 59.0);
        assert_eq!(split.connector_plans[0].axis, SplitAxis::X);
        assert_eq!(
            split.connector_plans[0].strategy,
            ConnectorStrategy::PlannedDowelPins
        );
        assert_eq!(split.connector_plans[0].center.x, 0.0);
        assert_eq!(split.connector_plans[0].center.y, -12.0);
        assert_eq!(split.connector_plans[0].center.z, -0.5);
        assert_eq!(split.connector_plans[1].center.x, 0.0);
        assert_eq!(split.connector_plans[1].center.y, 12.0);
        assert_eq!(split.connector_plans[1].center.z, -0.5);
        assert_eq!(split.connector_plans[0].diameter, 4.0);
        assert_eq!(split.connector_plans[0].length, 12.0);
        assert_eq!(split.printable_part_count, 2);
        assert_eq!(split.printable_instance_count, 5);
        assert_eq!(split.visual_only_instance_count, 1);
        assert_eq!(split.clearance_violation_count, 0);
        assert_eq!(split.wall_thickness_violation_count, 0);
        assert!(
            split
                .minimum_wall_thickness_observed
                .is_some_and(|thickness| thickness >= split.minimum_wall_thickness)
        );
        assert!(
            split
                .minimum_clearance_observed
                .is_some_and(|clearance| clearance >= split.minimum_clearance)
        );
        assert!(
            split
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "split-required")
        );

        let split_artifact = preparation_artifact(&split);
        let split_artifact_repeat = preparation_artifact(&split);
        assert_eq!(split_artifact.status, PrintPreparationArtifactStatus::Pass);
        assert_eq!(
            split_artifact.preparation_status,
            PrintPreparationStatus::SplitRequired
        );
        assert!(split_artifact.split_required);
        assert_eq!(split_artifact.split_axis, Some(SplitAxis::X));
        assert_eq!(split_artifact.split_segments.len(), 2);
        assert_eq!(split_artifact.connector_plans.len(), 2);
        assert_eq!(
            split_artifact.connector_fit_status,
            ConnectorFitStatus::Pass
        );
        assert_eq!(split_artifact.connector_fit_violation_count, 0);
        assert_eq!(split_artifact.wall_thickness_violation_count, 0);
        assert!(
            split_artifact
                .minimum_wall_thickness_observed
                .is_some_and(|thickness| thickness >= split_artifact.minimum_wall_thickness)
        );
        assert_eq!(
            split_artifact.artifact_hash,
            split_artifact_repeat.artifact_hash
        );
        assert!(split_artifact.artifact_hash.starts_with("sha256:"));
        assert!(!split_artifact.visual_mesh_used_for_manufacturing);

        let print = compile_print_job(&bundle, PrintCompileRequest::default_for_bundle(&bundle));
        let split_output = compile_split_print_output(&print, &split_artifact);
        let split_output_repeat = compile_split_print_output(&print, &split_artifact);
        assert_eq!(split_output.status, SplitPrintOutputStatus::Pass);
        assert_eq!(
            split_output.source_manufacturing_artifact_hash,
            print.artifact_hash
        );
        assert_eq!(
            split_output.preparation_artifact_hash,
            split_artifact.artifact_hash
        );
        assert_eq!(split_output.segments.len(), 2);
        assert!(split_output.segments.iter().all(|segment| {
            !segment.layers.is_empty()
                && segment.metrics.polygon_count > 0
                && segment.connector_cutout_hole_count > 0
                && segment.metrics.hole_count >= segment.connector_cutout_hole_count
                && segment.artifact_hash.starts_with("sha256:")
        }));
        assert!(
            split_output
                .segments
                .iter()
                .map(|segment| segment.connector_cutout_hole_count)
                .sum::<usize>()
                >= split_artifact.connector_plans.len() * split_output.segments.len()
        );
        assert_eq!(
            split_output.artifact_hash,
            split_output_repeat.artifact_hash
        );
        assert!(split_output.artifact_hash.starts_with("sha256:"));
        assert!(!split_output.visual_mesh_used_for_manufacturing);

        let connector_output = compile_connector_print_output(&split_artifact, &split.request);
        let connector_output_repeat =
            compile_connector_print_output(&split_artifact, &split.request);
        assert_eq!(connector_output.status, ConnectorPrintOutputStatus::Pass);
        assert_eq!(
            connector_output.source_preparation_artifact_hash,
            split_artifact.artifact_hash
        );
        assert_eq!(connector_output.connector_count, 2);
        assert!(connector_output.metrics.layer_count > 0);
        assert!(connector_output.metrics.polygon_count > 0);
        assert_eq!(connector_output.metrics.printable_part_count, 2);
        assert_eq!(
            connector_output.artifact_hash,
            connector_output_repeat.artifact_hash
        );
        assert!(connector_output.artifact_hash.starts_with("sha256:"));
        assert!(!connector_output.visual_mesh_used_for_manufacturing);
        let cutout_validation =
            validate_split_connector_cutouts(&split_artifact, &split_output, &connector_output);
        assert_eq!(
            cutout_validation.status,
            ConnectorCutoutValidationStatus::Pass
        );
        assert_eq!(
            cutout_validation.connector_count,
            split_artifact.connector_plans.len()
        );
        assert_eq!(cutout_validation.segment_count, split_output.segments.len());
        assert_eq!(cutout_validation.expected_cutout_hole_count, 40);
        assert_eq!(
            cutout_validation.expected_cutout_hole_count,
            cutout_validation.observed_cutout_hole_count
        );
        assert_eq!(
            cutout_validation.observed_cutout_hole_count,
            cutout_validation.declared_cutout_hole_count
        );

        let mut tampered_split_output = split_output.clone();
        let removed_hole = tampered_split_output
            .segments
            .iter_mut()
            .flat_map(|segment| &mut segment.layers)
            .flat_map(|layer| &mut layer.regions)
            .flat_map(|region| &mut region.polygons)
            .find_map(|polygon| polygon.holes.pop())
            .is_some();
        assert!(removed_hole);
        let tampered_validation = validate_split_connector_cutouts(
            &split_artifact,
            &tampered_split_output,
            &connector_output,
        );
        assert_eq!(
            tampered_validation.status,
            ConnectorCutoutValidationStatus::Fail
        );
        assert!(
            tampered_validation
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "connector-cutout-hole-missing")
        );

        let ready_artifact = preparation_artifact(&ready);
        let no_connector_output = compile_connector_print_output(&ready_artifact, &ready.request);
        assert_eq!(no_connector_output.status, ConnectorPrintOutputStatus::Fail);
        assert!(
            no_connector_output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "connector-plans-missing")
        );

        let mut bad_connector = split.connector_plans[0].clone();
        bad_connector.center.x += 2.0;
        let mut diagnostics = Vec::new();
        let (fit_status, fit_violation_count) = connector_fit_diagnostics(
            split.printable_bounds,
            split.split_axis,
            &split.split_segments,
            &[bad_connector],
            split.minimum_clearance,
            split.request.integer_grid,
            &mut diagnostics,
        );
        assert_eq!(fit_status, ConnectorFitStatus::Fail);
        assert_eq!(fit_violation_count, 1);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "connector-fit")
        );
    }

    #[test]
    fn parametric_car_print_preparation_reports_clearance_violation() {
        let mut bundle = SolidModelBundle::parametric_car_fixture();
        for instance in &mut bundle.assembly.instances {
            if instance.label == "Front-left wheel" {
                instance.transform = boon_solid_model::Mat4d::translation(
                    boon_solid_model::Vec3d::new(-36.0, -23.0, -6.0),
                );
            }
        }
        let request = PrintCompileRequest::default_for_bundle(&bundle);

        let preparation = prepare_print_job(&bundle, request);

        assert_eq!(preparation.status, PrintPreparationStatus::Blocked);
        assert!(preparation.fits_build_volume);
        assert!(!preparation.split_required);
        assert_eq!(preparation.clearance_violation_count, 1);
        assert_eq!(preparation.minimum_clearance_observed, Some(0.0));
        assert_eq!(preparation.printable_instance_count, 5);
        assert_eq!(preparation.visual_only_instance_count, 1);
        assert!(
            preparation
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "clearance-violation")
        );

        let artifact = preparation_artifact(&preparation);
        assert_eq!(artifact.status, PrintPreparationArtifactStatus::Fail);
        assert_eq!(artifact.preparation_status, PrintPreparationStatus::Blocked);
        assert!(!artifact.visual_mesh_used_for_manufacturing);
        assert!(artifact.artifact_hash.starts_with("sha256:"));
        assert!(
            artifact
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "clearance-violation")
        );
    }

    #[test]
    fn print_preparation_reports_wall_thickness_violation() {
        let bundle = SolidModelBundle::thin_shell_wall_thickness_negative_fixture();

        let preparation =
            prepare_print_job(&bundle, PrintCompileRequest::default_for_bundle(&bundle));

        assert_eq!(preparation.status, PrintPreparationStatus::Blocked);
        assert_eq!(preparation.wall_thickness_violation_count, 1);
        assert_eq!(preparation.minimum_wall_thickness_observed, Some(0.2));
        assert!(
            preparation
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "wall-thickness-violation")
        );

        let artifact = preparation_artifact(&preparation);
        assert_eq!(artifact.status, PrintPreparationArtifactStatus::Fail);
        assert_eq!(artifact.wall_thickness_violation_count, 1);
        assert_eq!(artifact.minimum_wall_thickness_observed, Some(0.2));
        assert!(
            artifact
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "wall-thickness-violation")
        );
    }

    #[test]
    fn split_print_output_preserves_contained_holes() {
        let bundle = SolidModelBundle::printable_bracket_fixture();
        let print = compile_print_job(&bundle, PrintCompileRequest::default_for_bundle(&bundle));

        let mut narrow_request = PrintCompileRequest::default_for_bundle(&bundle);
        narrow_request.build_volume = Aabb64 {
            min: boon_solid_model::Vec3d::new(-20.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(20.0, 80.0, 80.0),
        };
        let preparation = prepare_print_job(&bundle, narrow_request);
        let artifact = preparation_artifact(&preparation);
        let split_output = compile_split_print_output(&print, &artifact);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert!(print.metrics.hole_count > 0);
        assert_eq!(preparation.status, PrintPreparationStatus::SplitRequired);
        assert_eq!(artifact.status, PrintPreparationArtifactStatus::Pass);
        assert_eq!(split_output.status, SplitPrintOutputStatus::Pass);
        assert_eq!(split_output.segments.len(), 2);
        assert!(split_output.diagnostics.is_empty());
        assert!(
            split_output
                .segments
                .iter()
                .all(|segment| segment.metrics.hole_count > 0)
        );
        assert!(split_output.artifact_hash.starts_with("sha256:"));
        assert!(!split_output.visual_mesh_used_for_manufacturing);
    }

    #[test]
    fn split_print_output_preserves_boundary_crossing_holes() {
        let bundle = SolidModelBundle::printable_bracket_fixture();
        let print = compile_print_job(&bundle, PrintCompileRequest::default_for_bundle(&bundle));

        let mut narrow_request = PrintCompileRequest::default_for_bundle(&bundle);
        narrow_request.build_volume = Aabb64 {
            min: boon_solid_model::Vec3d::new(-20.0, -80.0, -20.0),
            max: boon_solid_model::Vec3d::new(20.0, 80.0, 80.0),
        };
        let preparation = prepare_print_job(&bundle, narrow_request);
        let mut artifact = preparation_artifact(&preparation);
        artifact.split_segments[0].bounds.max.x = -23.0;
        artifact.split_segments[1].bounds.min.x = -23.0;

        let split_output = compile_split_print_output(&print, &artifact);

        assert_eq!(print.status, ManufacturingCompileStatus::Pass);
        assert!(print.metrics.hole_count > 0);
        assert_eq!(preparation.status, PrintPreparationStatus::SplitRequired);
        assert_eq!(artifact.status, PrintPreparationArtifactStatus::Pass);
        assert_eq!(split_output.status, SplitPrintOutputStatus::Pass);
        assert_eq!(split_output.segments.len(), 2);
        assert!(split_output.diagnostics.is_empty());
        assert!(
            split_output
                .segments
                .iter()
                .all(|segment| segment.metrics.hole_count > 0)
        );
        assert!(split_output.artifact_hash.starts_with("sha256:"));
        assert!(!split_output.visual_mesh_used_for_manufacturing);
    }
}
