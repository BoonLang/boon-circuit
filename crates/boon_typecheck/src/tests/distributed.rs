fn distributed_continuous(ty: Type) -> FlowType {
    FlowType {
        mode: FlowMode::Continuous,
        ty,
    }
}

fn distributed_function(args: &[(&str, Type)], result: Type) -> ExternalFunctionType {
    ExternalFunctionType {
        args: args
            .iter()
            .map(|(name, ty)| ExternalFunctionArgument {
                name: (*name).to_owned(),
                ty: ty.clone(),
            })
            .collect(),
        result: distributed_continuous(result),
        pure: true,
    }
}

#[test]
fn session_info_intrinsics_enforce_role_visibility_and_closed_types() {
    let status = boon_parser::parse_source(
        "session-status.bn",
        "status: SessionInfo/status()\n",
    )
    .unwrap();
    for role in [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ] {
        let report = check_with_external_types(&status, &ExternalTypeEnvironment::empty(role));
        assert!(!report.has_errors(), "{role:?}: {:#?}", report.diagnostics);
    }

    let principal = boon_parser::parse_source(
        "session-principal.bn",
        "principal: SessionInfo/principal()\n",
    )
    .unwrap();
    let session = check_with_external_types(
        &principal,
        &ExternalTypeEnvironment::empty(ProgramRole::Session),
    );
    assert!(!session.has_errors(), "{:#?}", session.diagnostics);
    for role in [ProgramRole::Client] {
        let report = check_with_external_types(&principal, &ExternalTypeEnvironment::empty(role));
        assert!(report.has_errors(), "{role:?} unexpectedly accepted principal");
    }
    let server = check_with_external_types(
        &principal,
        &ExternalTypeEnvironment::empty(ProgramRole::Server),
    );
    assert!(!server.has_errors(), "{:#?}", server.diagnostics);
}

#[test]
fn distributed_external_values_and_calls_have_exact_static_types() {
    let parsed = boon_parser::parse_source(
        "distributed-session.bn",
        "count: Server/store.count\nclient_value: Client/store.x\nsum: Server/add(value: 2)\nformatted: Server/Module/format(value: sum)\n",
    )
    .unwrap();
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Session);
    environment.values.insert(
        "Server/store.count".to_owned(),
        distributed_continuous(Type::Number),
    );
    environment.values.insert(
        "Client/store.x".to_owned(),
        distributed_continuous(Type::Text),
    );
    environment.functions.insert(
        "Server/add".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Number),
    );
    environment.functions.insert(
        "Server/Module/format".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Text),
    );

    let (report, _) = check_profiled_with_external_types(&parsed, &environment);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    for (function, expected) in [
        ("Server/add", Type::Number),
        ("Server/Module/format", Type::Text),
    ] {
        let expression = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Call { function: called, .. }
                    if called == function)
            })
            .unwrap();
        assert_eq!(
            report
                .expr_type_table
                .entries
                .iter()
                .find(|entry| entry.expr_id == expression.id)
                .map(|entry| &entry.flow_type),
            Some(&distributed_continuous(expected))
        );
    }
}

#[test]
fn runtime_checked_program_types_external_calls_inside_user_functions() {
    let parsed = boon_parser::parse_source(
        "distributed-function-body.bn",
        r#"
store: [
    items: LIST { [value: 1] }
    rows:
        items
        |> List/map(item, new: decorate(item: item))
]

FUNCTION decorate(item) {
    [value: Session/add(value: item.value)]
}
"#,
    )
    .unwrap();
    let external_call = parsed
        .expressions
        .iter()
        .find(|expression| {
            matches!(&expression.kind, AstExprKind::Call { function, .. }
                if function == "Session/add")
        })
        .expect("qualified call in function body");
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Client);
    environment.functions.insert(
        "Session/add".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Number),
    );
    let (output, _) = check_runtime_program_profiled_with_external_types(&parsed, &environment);
    assert!(!output.report.has_errors(), "{:#?}", output.report.diagnostics);
    assert_eq!(
        output
            .report
            .expr_type_table
            .entries
            .iter()
            .find(|entry| entry.expr_id == external_call.id)
            .map(|entry| &entry.flow_type),
        Some(&distributed_continuous(Type::Number))
    );
    let checked = output.program.expect("checked runtime program");
    assert_eq!(
        checked
            .expressions
            .iter()
            .find(|expression| expression.id == CheckedExprId(external_call.id as u32))
            .map(|expression| &expression.flow_type),
        Some(&distributed_continuous(Type::Number))
    );
}

#[test]
fn distributed_role_direction_and_same_role_qualification_fail_closed() {
    for (current_role, producer, source, expected) in [
        (
            ProgramRole::Client,
            ProgramRole::Server,
            "value: Server/store.count\n",
            "Client cannot depend on Server",
        ),
        (
            ProgramRole::Server,
            ProgramRole::Client,
            "value: Client/store.count\n",
            "Server cannot depend on Client",
        ),
        (
            ProgramRole::Client,
            ProgramRole::Client,
            "value: Client/store.count\n",
            "same-role qualification",
        ),
    ] {
        let parsed = boon_parser::parse_source("invalid-direction.bn", source).unwrap();
        let qualified = format!("{}/store.count", role_namespace(producer));
        let mut environment = ExternalTypeEnvironment::empty(current_role);
        environment
            .values
            .insert(qualified, distributed_continuous(Type::Number));
        let report = check_with_external_types(&parsed, &environment);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "{:#?}",
            report.diagnostics
        );
    }

    let parsed = boon_parser::parse_source(
        "invalid-call-direction.bn",
        "value: Client/add(value: 1)\n",
    )
    .unwrap();
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Server);
    environment.functions.insert(
        "Client/add".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Number),
    );
    let report = check_with_external_types(&parsed, &environment);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("Server cannot depend on Client")));
}

#[test]
fn distributed_values_reject_role_outputs_and_non_store_roots() {
    for source in [
        "value: Client/outputs.count\n",
        "value: Client/model.count\n",
        "value: Client/store\n",
    ] {
        let parsed = boon_parser::parse_source("invalid-external-root.bn", source).unwrap();
        let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Session);
        let path = source
            .trim()
            .strip_prefix("value: ")
            .expect("fixture value path");
        environment
            .values
            .insert(path.to_owned(), distributed_continuous(Type::Number));
        let report = check_with_external_types(&parsed, &environment);
        assert!(
            report.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains("must use `Client/store.<value>`")),
            "{source}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn distributed_adjacent_roles_can_read_in_both_directions() {
    for (consumer, producer, path) in [
        (ProgramRole::Client, ProgramRole::Session, "Session/store.value"),
        (ProgramRole::Session, ProgramRole::Client, "Client/store.value"),
        (ProgramRole::Session, ProgramRole::Server, "Server/store.value"),
        (ProgramRole::Server, ProgramRole::Session, "Session/store.value"),
    ] {
        let parsed = boon_parser::parse_source(
            "adjacent-role.bn",
            &format!("value: {path}\n"),
        )
        .unwrap();
        let mut environment = ExternalTypeEnvironment::empty(consumer);
        environment
            .values
            .insert(path.to_owned(), distributed_continuous(Type::Number));
        let report = check_with_external_types(&parsed, &environment);
        assert!(!report.has_errors(), "{consumer:?} <- {producer:?}: {:#?}", report.diagnostics);
    }
}

#[test]
fn distributed_unknown_symbols_and_wrong_arguments_are_errors() {
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Session);
    environment.functions.insert(
        "Server/add".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Number),
    );

    for (source, expected) in [
        (
            "value: Server/store.missing\n",
            "unknown qualified external value `Server/store.missing`",
        ),
        (
            "value: Server/missing(value: 1)\n",
            "unknown qualified external function `Server/missing`",
        ),
        (
            "value: Server/add()\n",
            "external function `Server/add` is missing argument `value`",
        ),
        (
            "value: Server/add(other: 1)\n",
            "external function `Server/add` has no argument `other`",
        ),
        (
            "value: Server/add(value: TEXT { no })\n",
            "external function `Server/add` argument `value` has incompatible type",
        ),
    ] {
        let parsed = boon_parser::parse_source("invalid-external.bn", source).unwrap();
        let report = check_with_external_types(&parsed, &environment);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "source: {source}\ndiagnostics: {:#?}",
            report.diagnostics
        );
    }

    let positional = boon_parser::parse_source(
        "invalid-external-positional.bn",
        "value: Server/add(1)\n",
    )
    .unwrap_err();
    assert!(
        positional
            .message
            .contains("ordinary arguments use `name: expression`"),
        "unexpected parser diagnostic: {positional:#?}"
    );

    let duplicate = boon_parser::parse_source(
        "invalid-external-duplicate.bn",
        "value: Server/add(value: 1, value: 2)\n",
    )
    .unwrap_err();
    assert!(
        duplicate.message.contains("duplicate call entry `value`"),
        "unexpected parser diagnostic: {duplicate:#?}"
    );
}

#[test]
fn distributed_external_interfaces_accept_closed_event_and_list_values() {
    let parsed = boon_parser::parse_source("invalid-interface.bn", "value: 1\n").unwrap();
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Session);
    environment.values.insert(
        "Server/store.source".to_owned(),
        FlowType {
            mode: FlowMode::PresentOrAbsent,
            ty: Type::Number,
        },
    );
    environment.values.insert(
        "Server/store.list".to_owned(),
        distributed_continuous(Type::List(Box::new(Type::Number))),
    );
    environment.values.insert(
        "Server/store.absent".to_owned(),
        FlowType {
            mode: FlowMode::Absent,
            ty: Type::Number,
        },
    );
    environment.values.insert(
        "Server/store.open".to_owned(),
        distributed_continuous(open_object_type()),
    );
    environment.values.insert(
        "Server/store.unknown".to_owned(),
        distributed_continuous(Type::Unknown),
    );
    environment.functions.insert(
        "Server/impure".to_owned(),
        ExternalFunctionType {
            args: Vec::new(),
            result: distributed_continuous(Type::Number),
            pure: false,
        },
    );
    environment.functions.insert(
        "Server/noncontinuous".to_owned(),
        ExternalFunctionType {
            args: Vec::new(),
            result: FlowType {
                mode: FlowMode::TickPresent,
                ty: Type::Number,
            },
            pure: true,
        },
    );
    environment.functions.insert(
        "Server/list_arg".to_owned(),
        distributed_function(
            &[("items", Type::List(Box::new(Type::Number)))],
            Type::Number,
        ),
    );

    let report = check_with_external_types(&parsed, &environment);
    for expected in [
        "external value `Server/store.absent` cannot be always absent",
        "external value `Server/store.open` must have a closed value type",
        "external value `Server/store.unknown` must have a closed value type",
        "external function `Server/impure` must be pure",
        "external function `Server/noncontinuous` must have a continuous result",
    ] {
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "missing `{expected}` in {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn distributed_calls_reject_noncontinuous_source_arguments() {
    let parsed = boon_parser::parse_source(
        "source-argument.bn",
        "trigger: SOURCE\nvalue: Server/add(value: trigger)\n",
    )
    .unwrap();
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Session);
    environment.functions.insert(
        "Server/add".to_owned(),
        distributed_function(&[("value", exact_empty_object_type())], Type::Number),
    );
    let report = check_with_external_types(&parsed, &environment);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("external function `Server/add` argument `value` must be continuous")
    }));
}

#[test]
fn named_value_type_table_contains_only_canonical_declaration_paths() {
    let parsed = boon_parser::parse_source(
        "named-values.bn",
        r#"
store: [
    count: 40
    pulse: SOURCE
    items: LIST {}
]
outputs: [
    count: store.count
]
FUNCTION add(value) {
    value + store.count
}
"#,
    )
    .unwrap();
    let (report, _) = check_runtime_profiled_with_external_types(
        &parsed,
        &ExternalTypeEnvironment::empty(ProgramRole::Server),
    );
    let entries = report
        .named_value_type_table
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), &entry.flow_type))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(
        entries.get("store.count").copied(),
        Some(&distributed_continuous(Type::Number))
    );
    assert_eq!(
        entries.get("store.pulse").map(|flow| flow.mode),
        Some(FlowMode::PresentOrAbsent)
    );
    assert_eq!(
        entries.get("store").map(|flow| flow.mode),
        Some(FlowMode::PresentOrAbsent)
    );
    assert!(matches!(
        entries.get("store.items").map(|flow| &flow.ty),
        Some(Type::List(_))
    ));
    assert_eq!(
        entries.get("outputs.count").copied(),
        Some(&distributed_continuous(Type::Number))
    );
    assert!(!entries.contains_key("count"));
    assert!(!entries.keys().any(|path| path.contains("local")));
    assert!(report
        .named_value_type_table
        .entries
        .windows(2)
        .all(|entries| entries[0].path < entries[1].path));
    let function = report
        .function_type_table
        .entries
        .iter()
        .find(|function| function.name == "add")
        .expect("runtime-profiled function interface");
    assert_eq!(function.args, ["value"]);
    assert_eq!(function.arg_types, [Type::Number]);
    assert_eq!(function.result, distributed_continuous(Type::Number));
}
