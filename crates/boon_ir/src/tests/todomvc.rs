// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

#[test]
fn static_schedule_verifier_checks_order_and_symbol_tables() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    verify_static_schedule(&ir).unwrap();

    let mut bad_node_order = ir.clone();
    bad_node_order.nodes[0].id = NodeId(99);
    assert!(
        verify_static_schedule(&bad_node_order)
            .unwrap_err()
            .contains("expected 0")
    );

    let mut bad_expr_id = ir.clone();
    bad_expr_id.nodes[0].expr_id = Some(ExprId(ir.expression_count));
    assert!(
        verify_static_schedule(&bad_expr_id)
            .unwrap_err()
            .contains("missing ExprId")
    );

    let mut bad_branch_source = ir.clone();
    bad_branch_source.update_branches[0].source = "store.sources.missing.press".to_owned();
    assert!(
        verify_static_schedule(&bad_branch_source)
            .unwrap_err()
            .contains("not a declared source port")
    );

    let mut bad_list_target = ir.clone();
    bad_list_target.list_operations[0].list = "missing_list".to_owned();
    assert!(
        verify_static_schedule(&bad_list_target)
            .unwrap_err()
            .contains("unknown list")
    );

    let mut bad_scope_ref = ir.clone();
    bad_scope_ref.sources[0].scope_id = Some(ScopeId(ir.row_scopes.len()));
    assert!(
        verify_static_schedule(&bad_scope_ref)
            .unwrap_err()
            .contains("missing ScopeId")
    );
}


#[test]
fn while_is_scheduled_as_combinational_selection() {
    let source = include_str!("../../../../examples/todomvc.bn").replace(
        "\n    selected_filter:",
        "\n    visible_when_selected:\n        selected_filter |> WHILE { True }\n\n    selected_filter:",
    );
    let parsed = boon_parser::parse_source("row-scope-fixture.bn", source).unwrap();
    let ir = lower(&parsed).unwrap();
    assert!(
        ir.nodes
            .iter()
            .any(|node| matches!(node.kind, IrNodeKind::While))
    );
}


#[test]
fn combinational_cycles_must_be_broken_by_hold() {
    let source = include_str!("../../../../examples/todomvc.bn").replace(
        "\n    selected_filter:",
        "\n    cycle_left:\n        cycle_right |> WHILE { cycle_right }\n\n    cycle_right:\n        cycle_left |> WHILE { cycle_left }\n\n    selected_filter:",
    );
    let parsed = boon_parser::parse_source("row-scope-fixture.bn", source).unwrap();
    let error = lower(&parsed).unwrap_err();
    assert!(error.contains("combinational dependency cycle"));
    assert!(error.contains("broken by HOLD"));
}


#[test]
fn cause_tables_are_derived_from_source_names() {
    let source = include_str!("../../../../examples/todomvc.bn")
        .replace("filter_active", "filter_live")
        .replace("filter_completed", "filter_done");
    let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
    let ir = lower(&parsed).unwrap();
    let filter_causes = ir
        .possible_causes
        .iter()
        .find(|entry| entry.target == "store.selected_filter")
        .unwrap();
    assert!(
        filter_causes
            .sources
            .contains(&"store.sources.filter_live.press".to_owned())
    );
    assert!(
        filter_causes
            .sources
            .contains(&"store.sources.filter_done.press".to_owned())
    );
    assert!(
        !filter_causes
            .sources
            .contains(&"store.sources.filter_active.press".to_owned())
    );
    assert!(ir.update_branches.iter().any(|branch| {
        branch.target == "store.selected_filter"
            && branch.source == "store.sources.filter_live.press"
            && branch.expression
                == UpdateExpression::Const {
                    value: "Active".to_owned(),
                }
    }));
}
