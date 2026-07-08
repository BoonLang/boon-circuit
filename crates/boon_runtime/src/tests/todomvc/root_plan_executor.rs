// Included by `../todomvc.rs`.

// test: root_scalar_plan_executor_replays_todomvc_multi_event_subset
#[test]
fn root_scalar_plan_executor_replays_todomvc_multi_event_subset() {
    let steps = vec![
        "add-test-todo-type".to_owned(),
        "filter-active".to_owned(),
        "filter-completed".to_owned(),
        "filter-all".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/todomvc.bn"),
        Path::new("../../examples/todomvc.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("TodoMVC root scalar subset should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_eq!(
        output.report["plan_executor"]["executor"],
        "cpu-plan-root-list-scenario-v1"
    );
    assert_eq!(
        output.report["plan_executor"]["report_assembly_core"]["executor"],
        "cpu-plan-root-scenario-report-assembly-v1"
    );
    assert_eq!(output.state_summary["store.new_todo_text"], "Test todo");
    assert_eq!(output.state_summary["store.selected_filter"], "All");
    assert_eq!(output.state_summary["store.new_todo_focused"], true);
    assert!(output.report.get("comparison_status").is_none());
    assert_eq!(
        output.report["command_report_assembly_core"]["executor"],
        "cpu-plan-root-scenario-command-report-assembly-v1"
    );
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        4
    );
    assert_eq!(
        output.report["plan_executor"]["per_step"][0]["updates"][0]["expression_kind"],
        "source_payload"
    );
    assert_eq!(
        output.report["plan_executor"]["per_step"][0]["report_assembly_core"]["executor"],
        "cpu-plan-root-scenario-step-report-assembly-v1"
    );
    assert_eq!(
        output.report["plan_executor"]["per_step"][1]["updates"][0]["expression_kind"],
        "const"
    );
    assert_eq!(output.report["plan_executor"]["runtime_ast_eval_count"], 0);
    assert_eq!(
        output.report["plan_executor"]["executable_string_path_count"],
        0
    );
}

