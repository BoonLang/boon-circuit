use super::*;

// IR tests are grouped by lowering domain while staying in this module for private helper access.
include!("tests/bytes.rs");
include!("tests/distributed.rs");
include!("tests/sources_and_events.rs");

fn exact_state_arm<'a>(
    program: &'a ErasedProgram,
    target: &str,
    cause: EventCause,
) -> &'a StateUpdateArm {
    let state = program
        .state_cells
        .iter()
        .find(|state| state.path == target)
        .unwrap_or_else(|| panic!("missing state `{target}`"));
    program
        .state_update_arms
        .iter()
        .find(|arm| arm.state == state.id && arm.cause == cause)
        .unwrap_or_else(|| panic!("missing exact state arm for `{target}` from {cause:?}"))
}

fn exact_source_cause(program: &ErasedProgram, path: &str) -> EventCause {
    EventCause::Source(
        program
            .sources
            .iter()
            .find(|source| source.path == path)
            .unwrap_or_else(|| panic!("missing source `{path}`"))
            .id,
    )
}

fn exact_state_cause(program: &ErasedProgram, path: &str) -> EventCause {
    EventCause::State(
        program
            .state_cells
            .iter()
            .find(|state| state.path == path)
            .unwrap_or_else(|| panic!("missing state `{path}`"))
            .id,
    )
}

fn exact_subtree<'a>(
    program: &'a ErasedProgram,
    root: ExecutableExprId,
) -> Vec<&'a ExecutableExpression> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    let mut expressions = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let expression = program
            .executable
            .expressions
            .get(id.as_usize())
            .filter(|expression| expression.id == id)
            .unwrap_or_else(|| panic!("missing executable expression {id}"));
        pending.extend(executable_expression_children(&expression.kind));
        expressions.push(expression);
    }
    expressions
}

fn exact_call<'a>(
    program: &'a ErasedProgram,
    root: ExecutableExprId,
    function: &str,
) -> &'a ExecutableExpression {
    exact_subtree(program, root)
        .into_iter()
        .find(|expression| {
            matches!(
                &expression.kind,
                ExecutableExpressionKind::Call { name, .. } if name == function
            )
        })
        .unwrap_or_else(|| panic!("missing exact call `{function}` below {root}"))
}

#[test]
fn source_payload_erasure_keeps_the_endpoint_field_and_residual_projection() {
    let source = SourcePort {
        id: SourceId(0),
        path: "store.input".to_owned(),
        binding_path: "store.input".to_owned(),
        executable_source_id: None,
        static_owner: None,
        source_expr_id: None,
        source_line: 1,
        scoped: false,
        scope_id: None,
        interval_ms: None,
        payload_schema: SourcePayloadSchema {
            fields: vec![SourcePayloadField::Named("payload".to_owned())],
            typed_fields: vec![SourcePayloadDescriptor {
                field: SourcePayloadField::Named("payload".to_owned()),
                data_type: SemanticDataType::Record {
                    fields: vec![SemanticTypeField {
                        name: "child".to_owned(),
                        data_type: SemanticDataType::Text,
                    }],
                    open: false,
                },
            }],
        },
    };

    assert_eq!(
        erased_source_payload_read(
            &[source.clone()],
            ErasedBindingId(0),
            source.id,
            &["payload".to_owned(), "child".to_owned()],
        )
        .unwrap(),
        ErasedReadTarget::SourcePayload {
            binding: ErasedBindingId(0),
            source: source.id,
            field: SourcePayloadField::Named("payload".to_owned()),
            projection: vec!["child".to_owned()],
        }
    );
    assert!(
        erased_source_payload_read(
            &[source],
            ErasedBindingId(0),
            SourceId(0),
            &["missing".to_owned(), "payload".to_owned()],
        )
        .unwrap_err()
        .contains("payload projection `missing`")
    );
}

#[test]
fn outbound_http_effect_is_owned_by_one_exact_state_arm() {
    let parsed = boon_parser::parse_source(
        "outbound-http-effect.bn",
        include_str!("../../../examples/outbound_http_effect.bn"),
    )
    .unwrap();
    let program = lower(&parsed).unwrap();
    let arm = exact_state_arm(
        &program,
        "store.response",
        exact_source_cause(&program, "store.request"),
    );
    let call = exact_call(&program, arm.output_expression_id, "Http/request");
    let ExecutableExpressionKind::Call { arguments, .. } = &call.kind else {
        unreachable!();
    };
    assert_eq!(
        arguments
            .iter()
            .map(|argument| argument.name.as_str())
            .collect::<Vec<_>>(),
        [
            "endpoint",
            "method",
            "path_segments",
            "query",
            "headers",
            "body",
            "connect_timeout_ms",
            "overall_timeout_ms",
        ]
    );
}

#[test]
fn tagged_pattern_binding_survives_a_user_function_call_as_an_exact_projection() {
    let parsed = boon_parser::parse_source(
        "pattern-binding-user-call.bn",
        r#"
FUNCTION row_value(row) {
    row.value
}

rows: LIST { [id: TEXT { one }, value: 7] }

selected:
    rows |> List/find(item, if: item.id == TEXT { one }) |> WHEN {
        Found[value] => row_value(row: value)
        NotFound => 0
    }
"#,
    )
    .unwrap();
    let program = lower(&parsed).unwrap();
    let (found, binding) = program
        .executable
        .expressions
        .iter()
        .find_map(|expression| {
            let ExecutableExpressionKind::When { arms, .. } = &expression.kind else {
                return None;
            };
            arms.iter().find_map(|arm| {
                if matches!(
                    &arm.pattern,
                    boon_typecheck::CheckedMatchPattern::Tag { name } if name == "Found"
                ) {
                    arm.bindings.first().map(|binding| (arm.output, binding))
                } else {
                    None
                }
            })
        })
        .expect("typed Found arm");
    assert_eq!(binding.name, "value");
    assert_eq!(binding.projection, ["value"]);
    let subtree = exact_subtree(&program, found);
    assert!(subtree.iter().all(|expression| {
        !matches!(
            expression.kind,
            ExecutableExpressionKind::ExternalRead { .. }
        )
    }));
    assert!(
        subtree.iter().any(|expression| {
            matches!(
                &expression.kind,
                ExecutableExpressionKind::Project { fields, .. }
                    if fields.iter().any(|field| field == "value")
            )
        }),
        "Found arm subtree: {subtree:#?}"
    );
}

#[test]
fn effect_result_state_is_the_exact_next_effect_cause() {
    let parsed = boon_parser::parse_source(
        "state-triggered-effect-chain.bn",
        r#"
store: [
    start: SOURCE
    clock_result:
        ClockNotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    random_result:
        RandomNotRequested |> HOLD random_result {
            clock_result |> WHEN {
                WallClockRead => Random/bytes(byte_count: 16)
                __ => SKIP
            }
        }
]
"#,
    )
    .unwrap();
    let program = lower(&parsed).unwrap();
    let arm = exact_state_arm(
        &program,
        "store.random_result",
        exact_state_cause(&program, "store.clock_result"),
    );
    exact_call(&program, arm.output_expression_id, "Random/bytes");
    assert!(
        !program
            .possible_causes
            .iter()
            .find(|cause| cause.target == "store.random_result")
            .unwrap()
            .sources
            .iter()
            .any(|source| source == "store.start")
    );
}

#[test]
fn nested_effect_guards_remain_exact_executable_control_flow() {
    let parsed = boon_parser::parse_source(
        "nested-effect-result-guards.bn",
        r#"
store: [
    read: SOURCE
    selected: PackageAsset[url: TEXT { asset://files/primary.vcd }]
    file_result:
        NotStarted |> HOLD file_result {
            read |> THEN { File/read_stream(file: selected, retain_content: True) }
        }
    waveform_result:
        NotStarted |> HOLD waveform_result {
            file_result |> WHEN {
                Finished => file_result.retained |> WHEN {
                    Retained => Wellen/open(content: file_result.retained.content)
                    __ => SKIP
                }
                __ => SKIP
            }
        }
]
"#,
    )
    .unwrap();
    let program = lower(&parsed).unwrap();
    let arm = exact_state_arm(
        &program,
        "store.waveform_result",
        exact_state_cause(&program, "store.file_result"),
    );
    exact_call(&program, arm.output_expression_id, "Wellen/open");
    let when_count = exact_subtree(&program, arm.output_expression_id)
        .into_iter()
        .filter(|expression| matches!(expression.kind, ExecutableExpressionKind::When { .. }))
        .count();
    assert!(when_count >= 2, "nested guard ancestry was flattened");
}

#[test]
fn passkey_effect_arguments_are_preserved_in_exact_call_order() {
    let parsed = boon_parser::parse_source(
        "typed-passkey-effects.bn",
        r#"
store: [
    authenticate: SOURCE
    account_id: TEXT { account-1 }
    credential_count: 1
    simulation: Success
    authentication_result:
        AuthenticationNotRequested |> HOLD authentication_result {
            authenticate |> THEN {
                DevelopmentPasskey/authenticate(
                    account_id: account_id
                    credential_count: credential_count
                    simulation: simulation
                )
            }
        }
]
"#,
    )
    .unwrap();
    let program = lower(&parsed).unwrap();
    let arm = exact_state_arm(
        &program,
        "store.authentication_result",
        exact_source_cause(&program, "store.authenticate"),
    );
    let call = exact_call(
        &program,
        arm.output_expression_id,
        "DevelopmentPasskey/authenticate",
    );
    let ExecutableExpressionKind::Call { arguments, .. } = &call.kind else {
        unreachable!();
    };
    assert_eq!(
        arguments
            .iter()
            .map(|argument| argument.name.as_str())
            .collect::<Vec<_>>(),
        ["account_id", "credential_count", "simulation"]
    );
}
