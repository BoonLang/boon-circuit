use boon_compiler::compile_source_text_to_machine_plan;
use boon_plan::{PlanRowBuiltin, PlanRowExpressionNode, TargetProfile};

#[test]
fn map_and_set_literals_and_operations_cross_the_verified_compiler_spine() {
    let compiled = compile_source_text_to_machine_plan(
        "map-set-spine.bn",
        r#"
store: [
    users: MAP {
        TEXT { alice } => [score: 1]
    }
    updated_users:
        users
        |> Map/upsert(entry: [key: TEXT { bob }, value: [score: 2]])
    selected_user:
        updated_users
        |> Map/get(key: TEXT { bob })
    roles: SET {
        Admin
        Editor
    }
    updated_roles:
        roles
        |> Set/remove(item: Admin)
    has_editor:
        updated_roles
        |> Set/contains(item: Editor)
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { MAP and SET })
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();

    assert!(
        compiled
            .plan
            .row_expressions
            .iter()
            .any(|(_, node)| matches!(node, PlanRowExpressionNode::MapLiteral { .. }))
    );
    assert!(
        compiled
            .plan
            .row_expressions
            .iter()
            .any(|(_, node)| matches!(node, PlanRowExpressionNode::SetLiteral { .. }))
    );

    let builtins = compiled
        .plan
        .row_expressions
        .iter()
        .filter_map(|(_, node)| match node {
            PlanRowExpressionNode::BuiltinCall { function, .. } => Some(*function),
            _ => None,
        })
        .collect::<Vec<_>>();
    for expected in [
        PlanRowBuiltin::MapUpsert,
        PlanRowBuiltin::MapGet,
        PlanRowBuiltin::SetRemove,
        PlanRowBuiltin::SetContains,
    ] {
        assert!(
            builtins.contains(&expected),
            "missing lowered builtin {expected}; found {builtins:?}"
        );
    }
}
