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
        assert!(segment_package.segment_hole_count >= segment_package.connector_cutout_hole_count);
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

    let packages =
        export_split_3mf_entry_sets_with_preparation(&print, &split, Some(&preparation_artifact));
    let repeat =
        export_split_3mf_entry_sets_with_preparation(&print, &split, Some(&preparation_artifact));

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
        assert!(segment_package.segment_hole_count >= segment_package.connector_cutout_hole_count);
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
    let connector =
        boon_manufacturing::compile_connector_print_output(&preparation_artifact, &narrow_request);

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
    let connector =
        boon_manufacturing::compile_connector_print_output(&preparation_artifact, &narrow_request);

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
    let connector =
        boon_manufacturing::compile_connector_print_output(&preparation_artifact, &narrow_request);

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
    let mut narrow_request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
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
    let mut narrow_request = boon_manufacturing::PrintCompileRequest::default_for_bundle(&bundle);
    narrow_request.build_volume = boon_solid_model::Aabb64 {
        min: boon_solid_model::Vec3d::new(-20.0, -80.0, -20.0),
        max: boon_solid_model::Vec3d::new(20.0, 80.0, 80.0),
    };
    let preparation = boon_manufacturing::prepare_print_job(&bundle, narrow_request);
    let mut preparation_artifact = boon_manufacturing::preparation_artifact(&preparation);
    preparation_artifact.split_segments[0].bounds.max.x = -23.0;
    preparation_artifact.split_segments[1].bounds.min.x = -23.0;
    let mut split = boon_manufacturing::compile_split_print_output(&print, &preparation_artifact);
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
