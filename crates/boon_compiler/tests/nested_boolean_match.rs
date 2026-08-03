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

#[test]
fn retained_value_calls_prune_static_document_branches_without_scope_explosion() {
    let compiled = compile_test_source(
        "retained-document-calls.bn",
        r#"
FUNCTION tone(choice) {
    choice |> WHEN {
        Alpha => TEXT { #111111 }
        Beta => TEXT { #222222 }
        Gamma => TEXT { #333333 }
        Delta => TEXT { #444444 }
        Epsilon => TEXT { #555555 }
        Zeta => TEXT { #666666 }
        Eta => TEXT { #777777 }
        Theta => TEXT { #888888 }
    }
}

FUNCTION style(choice) {
    [color: tone(choice: choice)]
}

document: Document/new(
    root: Element/stripe(
        element: []
        direction: Column
        style: []
        items: LIST {
            Element/label(element: [], style: style(choice: Alpha), label: TEXT { alpha })
            Element/label(element: [], style: style(choice: Beta), label: TEXT { beta })
            Element/label(element: [], style: style(choice: Gamma), label: TEXT { gamma })
            Element/label(element: [], style: style(choice: Delta), label: TEXT { delta })
        }
    )
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();

    let ordinary = compiled
        .ir
        .executable
        .ordinary_functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<Vec<_>>();
    assert!(ordinary.iter().any(|name| name.ends_with("tone")));
    assert!(ordinary.iter().any(|name| name.ends_with("style")));
    let document = compiled.plan.document.as_ref().expect("document plan");
    assert!(
        document.expressions.len() < 120,
        "four static calls produced {} document expressions: {ordinary:?}",
        document.expressions.len()
    );
}

#[test]
fn retained_render_calls_materialize_distinct_invocation_contexts() {
    let compiled = compile_test_source(
        "retained-render-calls.bn",
        r#"
FUNCTION labeled(value) {
    Element/label(element: [], style: [], label: value)
}

document: Document/new(
    root: Element/stripe(
        element: []
        direction: Column
        style: []
        items: LIST {
            labeled(value: TEXT { alpha })
            labeled(value: TEXT { beta })
            labeled(value: TEXT { gamma })
            labeled(value: TEXT { delta })
        }
    )
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();

    let _function = compiled
        .ir
        .executable
        .ordinary_functions
        .iter()
        .find(|function| function.name.ends_with("labeled"))
        .expect("retained render function");
    let document = compiled.plan.document.as_ref().expect("document plan");
    let labels = document
        .templates
        .iter()
        .filter(|template| template.constructor == boon_plan::DocumentConstructor::ElementLabel)
        .collect::<Vec<_>>();
    assert_eq!(labels.len(), 4, "one label template per call occurrence");
    assert_eq!(
        labels
            .iter()
            .map(|template| template.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "shared constructor expressions require distinct stable invocation identities"
    );
    assert!(
        labels.iter().all(|template| matches!(
            document.expressions[template.expression.0].op,
            boon_plan::DocumentExprOp::Constructor { .. }
        )),
        "each invocation must materialize a render constructor"
    );
}
