use boon_compiler::compile_source_text_to_machine_plan;
use boon_plan::{
    PlanRowBuiltin, PlanRowExpressionNode, PlanTransientCollectionKind,
    PlanTransientCollectionResult, TargetProfile,
};

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

#[test]
fn linear_block_local_list_map_and_set_lower_to_private_transient_regions() {
    let compiled = compile_source_text_to_machine_plan(
        "map-set-transient.bn",
        r#"
store: [
    selected_user:
        BLOCK {
            users: MAP {
                TEXT { alice } => [score: 1]
            }
            with_bob:
                users
                |> Map/upsert(entry: [key: TEXT { bob }, value: [score: 2]])
            without_alice:
                with_bob
                |> Map/remove(key: TEXT { alice })
            without_alice
            |> Map/get(key: TEXT { bob })
        }
    has_editor:
        BLOCK {
            roles: SET {
                Admin
            }
            with_editor:
                roles
                |> Set/add(item: Editor)
            without_admin:
                with_editor
                |> Set/remove(item: Admin)
            without_admin
            |> Set/contains(item: Editor)
        }
    third_item:
        BLOCK {
            items: LIST {
                1
                2
            }
            extended:
                items
                |> List/append(item: 3)
            extended
            |> List/get(position: 3)
        }
    item_count:
        BLOCK {
            items: LIST {
                4
            }
            extended:
                List/append(
                    list: items
                    item: 5
                )
            List/length(list: extended)
        }
    has_items:
        BLOCK {
            items: LIST {
                6
            }
            items
            |> List/is_not_empty()
        }
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { transient })
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();

    let transient = compiled
        .plan
        .row_expressions
        .iter()
        .filter_map(|(_, node)| match node {
            PlanRowExpressionNode::TransientCollection { region } => Some(region.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(transient.len(), 5);
    assert_eq!(
        transient
            .iter()
            .filter(|region| region.kind == PlanTransientCollectionKind::List)
            .count(),
        3
    );
    assert!(
        transient
            .iter()
            .any(|region| matches!(region.result, PlanTransientCollectionResult::ListGet { .. }))
    );
    assert!(
        transient
            .iter()
            .any(|region| matches!(region.result, PlanTransientCollectionResult::ListLength))
    );
    assert!(
        transient
            .iter()
            .any(|region| matches!(region.result, PlanTransientCollectionResult::ListIsNotEmpty))
    );
    assert!(
        transient
            .iter()
            .all(|region| region.snapshot_copy_budget == 0)
    );
    assert!(
        !compiled
            .plan
            .row_expressions
            .iter()
            .any(|(_, node)| matches!(
                node,
                PlanRowExpressionNode::ListLiteral { .. }
                    | PlanRowExpressionNode::MapLiteral { .. }
                    | PlanRowExpressionNode::SetLiteral { .. }
            ))
    );
    assert!(compiled.plan.persistence.collections.is_empty());
    assert!(compiled.plan.persistence.lists.is_empty());
}

#[test]
fn multiply_observed_local_collection_stays_on_authority_path() {
    let compiled = compile_source_text_to_machine_plan(
        "map-set-observed.bn",
        r#"
store: [
    observations:
        BLOCK {
            users: MAP {
                TEXT { alice } => 1
            }
            first:
                users
                |> Map/get(key: TEXT { alice })
            second:
                users
                |> Map/get(key: TEXT { missing })
            [
                first_result: first
                second_result: second
                list_results:
                    BLOCK {
                        items: LIST {
                            1
                            2
                        }
                        first_item:
                            items
                            |> List/get(position: 1)
                        second_item:
                            items
                            |> List/get(position: 2)
                        [
                            first_item_result: first_item
                            second_item_result: second_item
                        ]
                    }
            ]
        }
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { observed })
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();

    assert!(
        !compiled
            .plan
            .row_expressions
            .iter()
            .any(|(_, node)| matches!(node, PlanRowExpressionNode::TransientCollection { .. }))
    );
    assert!(
        compiled
            .plan
            .row_expressions
            .iter()
            .any(|(_, node)| matches!(
                node,
                PlanRowExpressionNode::MapLiteral { .. }
                    | PlanRowExpressionNode::ListLiteral { .. }
            ))
    );
}

#[test]
fn capacity_exhausting_local_list_retains_its_terminal_error_contract() {
    let compiled = compile_source_text_to_machine_plan(
        "list-capacity-transient-negative.bn",
        r#"
store: [
    selected:
        BLOCK {
            items: LIST[1] {
                1
            }
            extended:
                items
                |> List/append(item: 2)
            extended
            |> List/get(position: 2)
        }
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { capacity })
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();

    let regions = compiled
        .plan
        .row_expressions
        .iter()
        .filter_map(|(_, node)| match node {
            PlanRowExpressionNode::TransientCollection { region } => Some(region.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        panic!("expected one capacity-bound transient LIST region");
    };
    assert_eq!(region.kind, PlanTransientCollectionKind::List);
    assert_eq!(region.declared_capacity, Some(1));
    assert_eq!(region.storage_growth_budget, 1);
}
