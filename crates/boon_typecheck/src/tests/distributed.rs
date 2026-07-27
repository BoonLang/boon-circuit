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
                flow_type: distributed_continuous(ty.clone()),
            })
            .collect(),
        result: distributed_continuous(result),
        effect: CheckedEffectSummary::default(),
    }
}

#[test]
fn session_info_intrinsics_enforce_role_visibility_and_closed_types() {
    let status =
        boon_parser::parse_source("session-status.bn", "status: SessionInfo/status()\n").unwrap();
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
    let role = ProgramRole::Client;
    let report = check_with_external_types(&principal, &ExternalTypeEnvironment::empty(role));
    assert!(
        report.has_errors(),
        "{role:?} unexpectedly accepted principal"
    );
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
    assert!(
        !output.report.has_errors(),
        "{:#?}",
        output.report.diagnostics
    );
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
fn checked_program_retains_final_lowering_metadata_and_external_environment() {
    let parsed = boon_parser::parse_project(
        "Client/RUN.bn",
        [
            (
                "Client/RUN.bn".to_owned(),
                r#"
store: [
    submitted: SOURCE
    submitted_text:
        TEXT { idle } |> HOLD submitted_text {
            submitted.text
        }
    remote_title: Presentation/identity(value: Session/store.title)
]
outputs: [
    submitted_text: store.submitted_text
    remote_title: store.remote_title
]
"#
                .to_owned(),
            ),
            (
                "Client/Presentation.bn".to_owned(),
                r#"
FUNCTION identity(value) {
    value
}
"#
                .to_owned(),
            ),
        ],
    )
    .unwrap();
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Client);
    environment.values.insert(
        "Session/store.title".to_owned(),
        distributed_continuous(Type::Text),
    );

    let (output, _) = check_runtime_program_profiled_with_external_types(&parsed, &environment);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("runtime program is checked");
    let metadata = &checked.lowering_metadata;

    assert_eq!(checked.external_types, environment);
    assert_eq!(
        metadata.source_units,
        vec![
            CheckedSourceUnitMetadata {
                path: "Client/Presentation.bn".to_owned(),
                module: Some("Presentation".to_owned()),
                start_line: 1,
                line_count: parsed.files[0].source.lines().count().max(1),
            },
            CheckedSourceUnitMetadata {
                path: "Client/RUN.bn".to_owned(),
                module: None,
                start_line: parsed.files[1].start_line,
                line_count: parsed.files[1].source.lines().count().max(1),
            },
        ]
    );
    assert_eq!(
        metadata.original_source_expression_count,
        parsed.expressions.len()
    );
    assert_eq!(
        metadata.source_payload_shape_table,
        output.report.source_payload_shape_table
    );
    assert!(
        metadata
            .source_payload_shape_table
            .iter()
            .any(|entry| entry.diagnostic_path == "store.submitted"
                && entry.fields.iter().any(|field| field.name == "text"))
    );
    assert_eq!(metadata.host_port_table, output.report.host_port_table);
    assert_eq!(metadata.output_root_types, output.report.output_root_types);
    assert_eq!(metadata.expr_type_table, output.report.expr_type_table);
    assert_eq!(
        metadata.function_type_table,
        output.report.function_type_table
    );
    assert_eq!(
        metadata.named_value_type_table,
        output.report.named_value_type_table
    );
    let repeated_submitted_text_sites = metadata
        .named_value_type_table
        .entries
        .iter()
        .filter(|entry| entry.path == "store.submitted_text")
        .collect::<Vec<_>>();
    assert_eq!(
        repeated_submitted_text_sites.len(),
        2,
        "the field and its nested HOLD are distinct exact sites with one diagnostic path"
    );
    assert_ne!(
        repeated_submitted_text_sites[0].origins,
        repeated_submitted_text_sites[1].origins
    );
    assert_eq!(metadata.render_slot_table, output.report.render_slot_table);
    assert_eq!(
        metadata.checked_expression_count,
        output.report.checked_expression_count
    );
    assert_eq!(
        metadata.dynamic_fallback_count,
        output.report.dynamic_fallback_count
    );
    assert_eq!(metadata.diagnostics, output.report.diagnostics);
    assert!(
        metadata
            .output_root_types
            .iter()
            .any(|output| output.name == "remote_title" && output.ty == Type::Text)
    );
    validate_structural_lowering_metadata(
        &checked,
        &metadata.source_payload_shape_table,
        &metadata.function_type_table,
        &metadata.named_value_type_table,
        &metadata.output_root_types,
        &metadata.host_port_table,
    )
    .expect("fresh lowering metadata is structurally exact");

    let mut stale_functions = metadata.function_type_table.clone();
    stale_functions.entries[0].callable = DeclId(u32::MAX);
    assert!(
        validate_structural_lowering_metadata(
            &checked,
            &metadata.source_payload_shape_table,
            &stale_functions,
            &metadata.named_value_type_table,
            &metadata.output_root_types,
            &metadata.host_port_table,
        )
        .unwrap_err()
        .contains("function type identity"),
        "a stale callable identity must fail closed"
    );

    let mut stale_output = metadata.output_root_types.clone();
    stale_output[0].statement = CheckedStatementId(u32::MAX);
    assert!(
        validate_structural_lowering_metadata(
            &checked,
            &metadata.source_payload_shape_table,
            &metadata.function_type_table,
            &metadata.named_value_type_table,
            &stale_output,
            &metadata.host_port_table,
        )
        .unwrap_err()
        .contains("missing checked statement"),
        "a stale output statement identity must fail closed"
    );

    let mut stale_source_payloads = metadata.source_payload_shape_table.clone();
    stale_source_payloads[0].checked_sources[0] = CheckedSourceId(u32::MAX);
    assert!(
        validate_structural_lowering_metadata(
            &checked,
            &stale_source_payloads,
            &metadata.function_type_table,
            &metadata.named_value_type_table,
            &metadata.output_root_types,
            &metadata.host_port_table,
        )
        .unwrap_err()
        .contains("missing checked source"),
        "a stale source payload identity must fail closed"
    );

    let mut missing_named_site = metadata.named_value_type_table.clone();
    missing_named_site.entries.remove(0);
    assert!(
        validate_structural_lowering_metadata(
            &checked,
            &metadata.source_payload_shape_table,
            &metadata.function_type_table,
            &missing_named_site,
            &metadata.output_root_types,
            &metadata.host_port_table,
        )
        .unwrap_err()
        .contains("do not exactly cover checked statement sites"),
        "a missing named-value exact site must fail closed"
    );

    let mut empty_named_origin = metadata.named_value_type_table.clone();
    empty_named_origin.entries[0].origins.clear();
    assert!(
        validate_structural_lowering_metadata(
            &checked,
            &metadata.source_payload_shape_table,
            &metadata.function_type_table,
            &empty_named_origin,
            &metadata.output_root_types,
            &metadata.host_port_table,
        )
        .unwrap_err()
        .contains("has no structural origins"),
        "an empty named-value exact origin must fail closed"
    );

    let mut duplicate_named_site = metadata.named_value_type_table.clone();
    duplicate_named_site
        .entries
        .push(duplicate_named_site.entries[0].clone());
    duplicate_named_site.entries.sort_by(|left, right| {
        left.origins
            .cmp(&right.origins)
            .then_with(|| left.path.cmp(&right.path))
    });
    assert!(
        validate_structural_lowering_metadata(
            &checked,
            &metadata.source_payload_shape_table,
            &metadata.function_type_table,
            &duplicate_named_site,
            &metadata.output_root_types,
            &metadata.host_port_table,
        )
        .unwrap_err()
        .contains("duplicates an exact checked site"),
        "a duplicate named-value exact site must fail closed"
    );

    let mut stale_named_statement = metadata.named_value_type_table.clone();
    stale_named_statement.entries[0].origins[0].statement =
        Some(CheckedStatementId(u32::MAX));
    stale_named_statement.entries.sort_by(|left, right| {
        left.origins
            .cmp(&right.origins)
            .then_with(|| left.path.cmp(&right.path))
    });
    assert!(
        validate_structural_lowering_metadata(
            &checked,
            &metadata.source_payload_shape_table,
            &metadata.function_type_table,
            &stale_named_statement,
            &metadata.output_root_types,
            &metadata.host_port_table,
        )
        .unwrap_err()
        .contains("missing statement origin"),
        "a stale named-value statement identity must fail closed"
    );
}

#[test]
fn checked_builtin_call_result_does_not_oscillate_with_an_event_argument() {
    let parsed = boon_parser::parse_source(
        "distributed-client-render-call.bn",
        r#"
store: [
    read_clock: SOURCE
]

scene: Scene/Element/text(
    element: [events: [press: store.read_clock]]
    style: [width: Fill]
    text: Session/store.server_seconds
)
"#,
    )
    .unwrap();
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Client);
    environment.values.insert(
        "Session/store.server_seconds".to_owned(),
        FlowType {
            mode: FlowMode::PresentOrAbsent,
            ty: Type::Number,
        },
    );

    let (output, _) = check_runtime_program_profiled_with_external_types(&parsed, &environment);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    assert!(output.program.is_some());
}

#[test]
fn distributed_boundary_requirements_seed_generic_user_schemes() {
    let parsed = boon_parser::parse_source(
        "distributed-identity-boundary.bn",
        r#"
FUNCTION identity(value) {
    value
}
"#,
    )
    .unwrap();
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Session);
    environment.local_function_requirements.insert(
        "identity".to_owned(),
        BTreeMap::from([("value".to_owned(), Type::Number)]),
    );

    let output = check_program_with_external_types(&parsed, &environment);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("identity boundary is checked");
    let identity = checked
        .callables
        .iter()
        .find(|callable| callable.name == "identity")
        .expect("identity signature");
    assert_eq!(identity.parameters[0].flow_type.ty, Type::Number);
    assert_eq!(identity.result.ty, Type::Number);
}

#[test]
fn provisional_distributed_check_preserves_external_reads_and_named_calls() {
    let parsed = boon_parser::parse_source(
        "distributed-provisional.bn",
        "count: Session/store.count\nnext: Session/add(value: count)\n",
    )
    .unwrap();
    let (output, _) = check_runtime_program_profiled_with_external_types(
        &parsed,
        &ExternalTypeEnvironment::provisional(ProgramRole::Client),
    );
    assert!(
        output
            .report
            .diagnostics
            .iter()
            .all(|diagnostic| { !diagnostic.message.starts_with("unknown qualified external") }),
        "{:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("provisional checked program");
    assert!(checked.expressions.iter().any(|expression| {
        matches!(
            &expression.kind,
            CheckedExpressionKind::ExternalRead { canonical_path, .. }
                if canonical_path == "Session/store.count"
        )
    }));
    let call = checked
        .calls
        .iter()
        .find(|call| call.function == "Session/add")
        .expect("provisional external call");
    assert!(matches!(
        call.entries.as_slice(),
        [CheckedCallEntry::Input { name, .. }] if name == "value"
    ));
}

#[test]
fn sealed_distributed_check_records_source_bound_external_identities() {
    let producer = boon_parser::parse_source(
        "session-producer.bn",
        "store: [count: 1]\nFUNCTION add(value) { value }\n",
    )
    .unwrap();
    let parsed = boon_parser::parse_source(
        "client-consumer.bn",
        "count: Session/store.count\nnext: Session/add(value: count)\n",
    )
    .unwrap();
    let value_identity = CheckedExternalDeclarationIdentityV1 {
        producer_role: ProgramRole::Session,
        producer_source_bundle_digest_v1: producer.source_bundle_digest_v1,
        producer_declaration: DeclId(41),
        kind: CheckedExternalDeclarationKind::Value,
    };
    let callable_identity = CheckedExternalDeclarationIdentityV1 {
        producer_role: ProgramRole::Session,
        producer_source_bundle_digest_v1: producer.source_bundle_digest_v1,
        producer_declaration: DeclId(42),
        kind: CheckedExternalDeclarationKind::Callable,
    };
    let mut environment = ExternalTypeEnvironment::sealed(ProgramRole::Client);
    environment.values.insert(
        "Session/store.count".to_owned(),
        distributed_continuous(Type::Number),
    );
    environment.functions.insert(
        "Session/add".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Number),
    );
    environment
        .external_identities
        .insert("Session/store.count".to_owned(), value_identity);
    environment
        .external_identities
        .insert("Session/add".to_owned(), callable_identity);

    let (output, _) = check_runtime_program_profiled_with_external_types(&parsed, &environment);
    assert!(
        !output.report.has_errors(),
        "{:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("sealed checked program");
    assert!(checked.expressions.iter().any(|expression| {
        matches!(
            &expression.kind,
            CheckedExpressionKind::ExternalRead {
                canonical_path,
                external_identity: Some(identity),
            } if canonical_path == "Session/store.count" && *identity == value_identity
        )
    }));
    assert_eq!(
        checked
            .callables
            .iter()
            .find(|callable| callable.name == "Session/add")
            .and_then(|callable| callable.external_identity),
        Some(callable_identity)
    );
}

#[test]
fn sealed_distributed_check_rejects_missing_or_mismatched_external_identities() {
    let parsed = boon_parser::parse_source(
        "client-consumer.bn",
        "count: Session/store.count\nnext: Session/add(value: count)\n",
    )
    .unwrap();
    let mut environment = ExternalTypeEnvironment::sealed(ProgramRole::Client);
    environment.values.insert(
        "Session/store.count".to_owned(),
        distributed_continuous(Type::Number),
    );
    environment.functions.insert(
        "Session/add".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Number),
    );
    environment.external_identities.insert(
        "Session/add".to_owned(),
        CheckedExternalDeclarationIdentityV1 {
            producer_role: ProgramRole::Server,
            producer_source_bundle_digest_v1: parsed.source_bundle_digest_v1,
            producer_declaration: DeclId(42),
            kind: CheckedExternalDeclarationKind::Value,
        },
    );

    let report = check_with_external_types(&parsed, &environment);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("missing its source-bound producer identity")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("which does not match its qualified role")
    }));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("expected Callable"))
    );
}

#[test]
fn sealed_distributed_check_rejects_duplicate_producer_declaration_identity() {
    let parsed = boon_parser::parse_source(
        "client-consumer.bn",
        "count: Session/store.count\nnext: Session/add(value: count)\n",
    )
    .unwrap();
    let mut environment = ExternalTypeEnvironment::sealed(ProgramRole::Client);
    environment.values.insert(
        "Session/store.count".to_owned(),
        distributed_continuous(Type::Number),
    );
    environment.functions.insert(
        "Session/add".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Number),
    );
    for (name, kind) in [
        ("Session/store.count", CheckedExternalDeclarationKind::Value),
        ("Session/add", CheckedExternalDeclarationKind::Callable),
    ] {
        environment.external_identities.insert(
            name.to_owned(),
            CheckedExternalDeclarationIdentityV1 {
                producer_role: ProgramRole::Session,
                producer_source_bundle_digest_v1: parsed.source_bundle_digest_v1,
                producer_declaration: DeclId(42),
                kind,
            },
        );
    }

    let report = check_with_external_types(&parsed, &environment);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("select the same producer declaration")
    }));
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

    let parsed =
        boon_parser::parse_source("invalid-call-direction.bn", "value: Client/add(value: 1)\n")
            .unwrap();
    let mut environment = ExternalTypeEnvironment::empty(ProgramRole::Server);
    environment.functions.insert(
        "Client/add".to_owned(),
        distributed_function(&[("value", Type::Number)], Type::Number),
    );
    let report = check_with_external_types(&parsed, &environment);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Server cannot depend on Client")
    }));
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
        (
            ProgramRole::Client,
            ProgramRole::Session,
            "Session/store.value",
        ),
        (
            ProgramRole::Session,
            ProgramRole::Client,
            "Client/store.value",
        ),
        (
            ProgramRole::Session,
            ProgramRole::Server,
            "Server/store.value",
        ),
        (
            ProgramRole::Server,
            ProgramRole::Session,
            "Session/store.value",
        ),
    ] {
        let parsed =
            boon_parser::parse_source("adjacent-role.bn", format!("value: {path}\n")).unwrap();
        let mut environment = ExternalTypeEnvironment::empty(consumer);
        environment
            .values
            .insert(path.to_owned(), distributed_continuous(Type::Number));
        let report = check_with_external_types(&parsed, &environment);
        assert!(
            !report.has_errors(),
            "{consumer:?} <- {producer:?}: {:#?}",
            report.diagnostics
        );
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

    let positional =
        boon_parser::parse_source("invalid-external-positional.bn", "value: Server/add(1)\n")
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
            effect: CheckedEffectSummary {
                invokes_host: true,
                ..CheckedEffectSummary::default()
            },
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
            effect: CheckedEffectSummary::default(),
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
fn distributed_calls_preserve_event_argument_flow() {
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
    let output = check_program_with_external_types(&parsed, &environment);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output
        .program
        .expect("event-valued external call is checked");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "Server/add")
        .expect("distributed call");
    assert_eq!(call.result.mode, FlowMode::PresentOrAbsent);
}

#[test]
fn named_value_type_table_contains_canonical_paths_ordered_by_exact_site() {
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
    assert!(
        report
            .named_value_type_table
            .entries
            .windows(2)
            .all(|entries| {
                entries[0].origins < entries[1].origins
                    || entries[0].origins == entries[1].origins
                        && entries[0].path < entries[1].path
            })
    );
    let function = report
        .function_type_table
        .entries
        .iter()
        .find(|function| function.name == "add")
        .expect("runtime-profiled function interface");
    assert_eq!(
        function
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["value"]
    );
    assert_eq!(
        function
            .parameters
            .iter()
            .map(|parameter| parameter.flow_type.clone())
            .collect::<Vec<_>>(),
        [distributed_continuous(Type::Number)]
    );
    assert_eq!(function.parameters[0].ordinal, 0);
    assert_ne!(function.parameters[0].formal, DeclId(u32::MAX));
    assert_ne!(function.callable, DeclId(u32::MAX));
    assert_eq!(function.result, distributed_continuous(Type::Number));
}
