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
