use boon_compiler::compile_source_text_to_machine_plan;
use boon_plan::{PlanExpressionKind, PlanOpKind, TargetProfile};

#[test]
fn nested_boolean_match_updates_are_cpu_executable() {
    let compiled = compile_source_text_to_machine_plan(
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
                    Enter => enabled |> WHEN {
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
                    Enter => enabled |> WHEN {
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
            .any(|op| matches!(
                op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::MatchValueConst,
                    ..
                }
            ))
    );
    assert_eq!(
        boon_plan::cpu_plan_executor_unsupported_ops(&compiled.plan).len(),
        0
    );
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
}
