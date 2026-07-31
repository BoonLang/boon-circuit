use boon_compiler::{
    CompileRequest, CompiledMachinePlanFromSource, CompilerResult, compile_machine_plan,
};
use boon_plan::{
    ApplicationIdentity, PlanOpKind, PlanRowExpressionNode, ProgramRole, TargetProfile,
};

fn compile_test_source(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_machine_plan(CompileRequest::source_text(
        source_label,
        source_text,
        target_profile,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))
}

#[test]
fn nested_boolean_match_updates_are_cpu_executable() {
    let compiled = compile_test_source(
        "nested-boolean-match.bn",
        r#"
store: [
    key: SOURCE
    enabled:
        True |> HOLD enabled {
            key.enabled |> THEN { key.enabled }
        }
    screen:
        Idle |> HOLD screen {
            key.key |> THEN {
                key.key |> WHEN {
                    TEXT { Enter } => enabled |> WHEN {
                        True => Accepted
                        False => Denied
                    }
                    __ => screen
                }
            }
        }
    zoom:
        1 |> HOLD zoom {
            key.key |> THEN {
                key.key |> WHEN {
                    TEXT { Enter } => enabled |> WHEN {
                        True => 11.5
                        False => SKIP
                    }
                    __ => SKIP
                }
            }
        }
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { Match fixture })
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();

    assert!(
        compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .any(|op| {
                let PlanOpKind::StateUpdate {
                    value: Some(value), ..
                } = &op.kind
                else {
                    return false;
                };
                let Ok(PlanRowExpressionNode::Select { arms, .. }) =
                    compiled.plan.row_expressions.node(*value)
                else {
                    return false;
                };
                arms.iter().any(|arm| {
                    matches!(
                        compiled.plan.row_expressions.node(arm.value),
                        Ok(PlanRowExpressionNode::Select { .. })
                    )
                })
            })
    );
    assert_eq!(
        boon_plan::cpu_plan_executor_unsupported_ops(&compiled.plan).len(),
        0
    );
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
}
