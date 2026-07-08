use super::*;

fn parse_cells_project_for_plan_test() -> boon_parser::ParsedProgram {
    boon_parser::parse_project(
        "examples/cells.bn",
        [
            (
                "examples/cells/defaults.bn".to_owned(),
                include_str!("../../../examples/cells/defaults.bn").to_owned(),
            ),
            (
                "examples/cells/formula.bn".to_owned(),
                include_str!("../../../examples/cells/formula.bn").to_owned(),
            ),
            (
                "examples/cells/cell.bn".to_owned(),
                include_str!("../../../examples/cells/cell.bn").to_owned(),
            ),
            (
                "examples/cells/model.bn".to_owned(),
                include_str!("../../../examples/cells/model.bn").to_owned(),
            ),
            (
                "examples/cells/columns.bn".to_owned(),
                include_str!("../../../examples/cells/columns.bn").to_owned(),
            ),
            (
                "examples/cells/store.bn".to_owned(),
                include_str!("../../../examples/cells/store.bn").to_owned(),
            ),
            (
                "examples/cells/view.bn".to_owned(),
                include_str!("../../../examples/cells/view.bn").to_owned(),
            ),
            (
                "examples/cells.bn".to_owned(),
                include_str!("../../../examples/cells.bn").to_owned(),
            ),
        ],
    )
    .expect("checked-in Cells project should parse")
}

// Machine-plan backend test shards are grouped by behavior area while staying in this module for private invariant access.
include!("machine_plan_backend_tests/bytes.rs");
include!("machine_plan_backend_tests/cells.rs");
include!("machine_plan_backend_tests/root_and_derived_ops.rs");
include!("machine_plan_backend_tests/todomvc.rs");
include!("machine_plan_backend_tests/verifier_and_tamper.rs");
