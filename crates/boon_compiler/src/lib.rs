use boon_ir::TypedProgram;

pub use boon_plan::{MachinePlan, PlanError, TargetProfile};

pub fn compile_typed_program(
    program: &TypedProgram,
    target_profile: TargetProfile,
) -> Result<MachinePlan, PlanError> {
    boon_plan::compile_typed_program(program, target_profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_facade_matches_current_plan_backend_for_counter() {
        let source = include_str!("../../../examples/counter.bn");
        let parsed =
            boon_parser::parse_source("examples/counter.bn".to_owned(), source.to_owned()).unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();

        let facade_plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let backend_plan =
            boon_plan::compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        assert_eq!(
            boon_plan::plan_sha256(&facade_plan).unwrap(),
            boon_plan::plan_sha256(&backend_plan).unwrap()
        );
    }
}
