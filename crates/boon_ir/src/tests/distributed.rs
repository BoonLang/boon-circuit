// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

fn distributed_test_flow(ty: boon_typecheck::Type) -> boon_typecheck::FlowType {
    boon_typecheck::FlowType {
        mode: boon_typecheck::FlowMode::Continuous,
        ty,
    }
}

fn distributed_test_function(
    arguments: &[(&str, boon_typecheck::Type)],
    result: boon_typecheck::Type,
) -> boon_typecheck::ExternalFunctionType {
    boon_typecheck::ExternalFunctionType {
        args: arguments
            .iter()
            .map(|(name, ty)| boon_typecheck::ExternalFunctionArgument {
                name: (*name).to_owned(),
                flow_type: distributed_test_flow(ty.clone()),
            })
            .collect(),
        result: distributed_test_flow(result),
        effect: boon_typecheck::CheckedEffectSummary::default(),
    }
}

fn distributed_test_environment() -> boon_typecheck::ExternalTypeEnvironment {
    let mut environment =
        boon_typecheck::ExternalTypeEnvironment::empty(boon_typecheck::ProgramRole::Session);
    environment.values.insert(
        "Server/store.count".to_owned(),
        distributed_test_flow(boon_typecheck::Type::Number),
    );
    environment.values.insert(
        "Client/store.x".to_owned(),
        distributed_test_flow(boon_typecheck::Type::Text),
    );
    environment.functions.insert(
        "Server/add".to_owned(),
        distributed_test_function(
            &[("value", boon_typecheck::Type::Number)],
            boon_typecheck::Type::Number,
        ),
    );
    environment.functions.insert(
        "Server/Module/format".to_owned(),
        distributed_test_function(
            &[("value", boon_typecheck::Type::Number)],
            boon_typecheck::Type::Text,
        ),
    );
    environment
}

#[test]
fn qualified_distributed_values_and_calls_have_explicit_typed_metadata() {
    let parsed = boon_parser::parse_source(
        "distributed-ir.bn",
        "count: Server/store.count\nclient_value: Client/store.x\nsum: Server/add(value: 2)\nformatted: Server/Module/format(value: sum)\n",
    )
    .unwrap();
    let environment = distributed_test_environment();
    let typed = lower_with_external_types(&parsed, &environment).unwrap();

    assert_eq!(typed.distributed_references.value_references.len(), 2);
    let server_count = typed
        .distributed_references
        .value_references
        .iter()
        .find(|reference| reference.canonical_path == "Server/store.count")
        .unwrap();
    assert_eq!(
        server_count.producer_role,
        boon_typecheck::ProgramRole::Server
    );
    assert_eq!(server_count.value_type, boon_typecheck::Type::Number);
    assert!(typed.executable.expressions.iter().any(|expression| {
        expression.checked_expr_id.0 as usize == server_count.expr_id.as_usize()
            && matches!(
                &expression.kind,
                ExecutableExpressionKind::ExternalRead { canonical_path }
                    if canonical_path == "Server/store.count"
            )
    }));

    let session_output = typed
        .distributed_references
        .value_references
        .iter()
        .find(|reference| reference.canonical_path == "Client/store.x")
        .unwrap();
    assert_eq!(
        session_output.producer_role,
        boon_typecheck::ProgramRole::Client
    );
    assert_eq!(session_output.value_type, boon_typecheck::Type::Text);

    assert_eq!(typed.distributed_references.calls.len(), 2);
    let add = typed
        .distributed_references
        .calls
        .iter()
        .find(|call| call.canonical_function == "Server/add")
        .unwrap();
    assert_eq!(add.producer_role, boon_typecheck::ProgramRole::Server);
    assert_eq!(add.result, distributed_test_flow(boon_typecheck::Type::Number));
    assert_eq!(add.arguments.len(), 1);
    assert_eq!(add.arguments[0].name, "value");
    assert_eq!(
        add.arguments[0].flow_type,
        distributed_test_flow(boon_typecheck::Type::Number)
    );
    assert!(matches!(
        typed.executable.expressions[add.expression.as_usize()].kind,
        ExecutableExpressionKind::Call {
            callable_kind: ExecutableCallableKind::External,
            ..
        }
    ));
    assert!(matches!(
        typed.executable.expressions[add.arguments[0].value.as_usize()].kind,
        ExecutableExpressionKind::Number(_)
    ));

    let format = typed
        .distributed_references
        .calls
        .iter()
        .find(|call| call.canonical_function == "Server/Module/format")
        .unwrap();
    assert_eq!(format.producer_role, boon_typecheck::ProgramRole::Server);
    assert_eq!(format.result, distributed_test_flow(boon_typecheck::Type::Text));
    assert_eq!(format.arguments.len(), 1);
    assert_eq!(format.arguments[0].name, "value");
    assert_eq!(
        format.arguments[0].flow_type,
        distributed_test_flow(boon_typecheck::Type::Number)
    );

    assert_eq!(
        typed
            .expression_coverage
            .distributed_reference_expression_count,
        4
    );
    assert_eq!(typed.expression_coverage.unknown_total(), 0);
    verify_static_schedule(&typed).unwrap();

    let runtime = lower_runtime_with_external_types(&parsed, &environment).unwrap();
    assert_eq!(runtime.distributed_references, typed.distributed_references);
}

#[test]
fn hold_backed_distributed_call_has_current_flow_without_invocation_arms() {
    let parsed = boon_parser::parse_source(
        "hold-backed-distributed-call.bn",
        r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    doubled: Server/double(value: count)
]
"#,
    )
    .unwrap();
    let mut environment =
        boon_typecheck::ExternalTypeEnvironment::empty(boon_typecheck::ProgramRole::Session);
    environment.values.insert(
        "Client/store.increment".to_owned(),
        boon_typecheck::FlowType {
            mode: boon_typecheck::FlowMode::PresentOrAbsent,
            ty: boon_typecheck::Type::Object(boon_typecheck::ObjectShape {
                fields: BTreeMap::new(),
                field_order: Vec::new(),
                open: false,
            }),
        },
    );
    environment.functions.insert(
        "Server/double".to_owned(),
        distributed_test_function(
            &[("value", boon_typecheck::Type::Number)],
            boon_typecheck::Type::Number,
        ),
    );

    let ir = lower_with_external_types(&parsed, &environment).unwrap();
    let [call] = ir.distributed_references.calls.as_slice() else {
        panic!("expected one distributed call")
    };
    assert_eq!(call.result.mode, boon_typecheck::FlowMode::Continuous);
    assert_eq!(
        call.arguments[0].flow_type.mode,
        boon_typecheck::FlowMode::Continuous
    );
    assert!(
        call.invocation_arms.is_empty(),
        "a current call depending on HOLD currentness is not event-owned"
    );
}

#[test]
fn empty_environment_lowering_and_external_typecheck_errors_fail_closed() {
    let parsed = boon_parser::parse_source(
        "missing-distributed-ir.bn",
        "value: Session/store.missing\ncall: Session/missing(value: 1)\n",
    )
    .unwrap();
    let error = lower(&parsed).unwrap_err();
    assert!(
        error.contains("unknown qualified external value `Session/store.missing`")
            && error.contains("unknown qualified external function `Session/missing`"),
        "unexpected empty-environment error: {error}"
    );

    let parsed = boon_parser::parse_source(
        "invalid-distributed-direction.bn",
        "value: Server/store.count\n",
    )
    .unwrap();
    let mut environment =
        boon_typecheck::ExternalTypeEnvironment::empty(boon_typecheck::ProgramRole::Client);
    environment.values.insert(
        "Server/store.count".to_owned(),
        distributed_test_flow(boon_typecheck::Type::Number),
    );
    let error = lower_with_external_types(&parsed, &environment).unwrap_err();
    assert!(
        error.contains("Client cannot depend on Server through `Server/store.count`"),
        "unexpected direction error: {error}"
    );

    let parsed =
        boon_parser::parse_source("same-role-distributed-ir.bn", "value: Server/store.count\n")
            .unwrap();
    let mut environment =
        boon_typecheck::ExternalTypeEnvironment::empty(boon_typecheck::ProgramRole::Server);
    environment.values.insert(
        "Server/store.count".to_owned(),
        distributed_test_flow(boon_typecheck::Type::Number),
    );
    let error = lower_with_external_types(&parsed, &environment).unwrap_err();
    assert!(
        error.contains("same-role qualification `Server/store.count` is not allowed in Server"),
        "unexpected same-role error: {error}"
    );
}

#[test]
fn distributed_metadata_accepts_closed_lists_event_flows_and_effects_but_excludes_open_types() {
    let parsed = boon_parser::parse_source("invalid-distributed-types.bn", "local: 1\n").unwrap();
    let mut environment =
        boon_typecheck::ExternalTypeEnvironment::empty(boon_typecheck::ProgramRole::Session);
    environment.values.insert(
        "Server/store.source".to_owned(),
        boon_typecheck::FlowType {
            mode: boon_typecheck::FlowMode::PresentOrAbsent,
            ty: boon_typecheck::Type::Number,
        },
    );
    environment.values.insert(
        "Server/store.list".to_owned(),
        distributed_test_flow(boon_typecheck::Type::List(Box::new(
            boon_typecheck::Type::Number,
        ))),
    );
    environment.values.insert(
        "Server/store.open".to_owned(),
        distributed_test_flow(boon_typecheck::Type::Object(boon_typecheck::ObjectShape {
            fields: BTreeMap::new(),
            field_order: Vec::new(),
            open: true,
        })),
    );
    environment.functions.insert(
        "Server/effect".to_owned(),
        boon_typecheck::ExternalFunctionType {
            args: Vec::new(),
            result: distributed_test_flow(boon_typecheck::Type::Number),
            effect: boon_typecheck::CheckedEffectSummary {
                invokes_host: true,
                ..boon_typecheck::CheckedEffectSummary::default()
            },
        },
    );

    let error = lower_with_external_types(&parsed, &environment).unwrap_err();
    assert!(
        error.contains("external value `Server/store.open` must have a closed value type"),
        "unexpected open-type error: {error}"
    );
    assert!(
        !error.contains("Server/store.source"),
        "unexpected event-flow error: {error}"
    );
    assert!(
        !error.contains("Server/effect"),
        "unexpected effect-signature error: {error}"
    );
}
