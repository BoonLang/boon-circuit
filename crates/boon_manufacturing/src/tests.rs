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
    let connector_output_repeat = compile_connector_print_output(&split_artifact, &split.request);
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

    let preparation = prepare_print_job(&bundle, PrintCompileRequest::default_for_bundle(&bundle));

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
