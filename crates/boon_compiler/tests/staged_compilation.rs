use boon_compiler::{
    CheckedCompileRequest, CompileRequest, CompilerCheckRequest, check_source,
    compile_machine_plan, finish_checked_machine_plan,
};
use boon_plan::{ApplicationIdentity, ProgramRole, TargetProfile};

const SOURCE: &str = r#"
store: [
    count: 0 |> HOLD count {
        increment |> THEN { count + 1 }
    }
]

increment: SOURCE
value: store.count
"#;

#[test]
fn staged_check_and_finish_match_monolithic_compilation() {
    let monolithic = compile_machine_plan(CompileRequest::source_text(
        "staged-parity.bn",
        SOURCE,
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
        ApplicationIdentity::compiler_default(),
    ))
    .unwrap();

    let checked = check_source(CompilerCheckRequest::source_text(
        "staged-parity.bn",
        SOURCE,
        ProgramRole::Server,
    ))
    .unwrap();
    assert!(!checked.output.report.has_errors());
    let staged_parse_work = checked.profile.parse_work;
    let staged_typecheck_work = checked.profile.typecheck_work;
    let staged = finish_checked_machine_plan(
        checked,
        CheckedCompileRequest::new(
            TargetProfile::SoftwareDefault,
            ProgramRole::Server,
            ApplicationIdentity::compiler_default(),
        ),
    )
    .unwrap();

    assert_eq!(staged.ir, monolithic.ir);
    assert_eq!(staged.plan, monolithic.plan);
    assert_eq!(staged.profile.parse_work, staged_parse_work);
    assert_eq!(staged.profile.typecheck_work, staged_typecheck_work);
    assert_eq!(staged.profile.parse_work, monolithic.profile.parse_work);
    assert_eq!(
        staged.profile.typecheck_work,
        monolithic.profile.typecheck_work
    );
    assert!(staged.profile.typecheck_ms >= 0.0);
    assert!(staged.profile.semantic_ms >= 0.0);
    assert!(staged.profile.contract_verify_ms >= 0.0);
    assert!(staged.profile.ir_lower_ms >= 0.0);
}

#[test]
fn staged_finish_rejects_checked_errors() {
    let checked = check_source(CompilerCheckRequest::source_text(
        "staged-error.bn",
        "value: missing_name",
        ProgramRole::Client,
    ))
    .unwrap();
    assert!(checked.output.report.has_errors());
    let error = finish_checked_machine_plan(
        checked,
        CheckedCompileRequest::new(
            TargetProfile::SoftwareDefault,
            ProgramRole::Client,
            ApplicationIdentity::compiler_default(),
        ),
    )
    .unwrap_err();
    assert!(error.to_string().contains("typecheck failed"));
}
