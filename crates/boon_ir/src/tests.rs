use super::*;

// IR tests are grouped by lowering domain while staying in this module for private helper access.
include!("tests/bytes.rs");
include!("tests/distributed.rs");
include!("tests/sources_and_events.rs");

#[test]
fn outbound_http_call_lowers_as_a_direct_typed_state_update() {
    let parsed = boon_parser::parse_source(
        "outbound-http-effect.bn",
        include_str!("../../../examples/outbound_http_effect.bn"),
    )
    .unwrap();
    let typed = lower(&parsed).unwrap();
    let branches = typed
        .update_branches
        .iter()
        .filter(|branch| {
            matches!(&branch.expression,
            UpdateExpression::HostEffect { operation, .. } if operation == "Http/request")
        })
        .collect::<Vec<_>>();
    let [branch] = branches.as_slice() else {
        panic!("expected one direct typed outbound update");
    };
    assert_eq!(branch.source, "store.request");
    assert_eq!(branch.target, "store.response");
    let UpdateExpression::HostEffect { arguments, .. } = &branch.expression else {
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
    let request = typed
        .sources
        .iter()
        .find(|source| source.path == "store.request")
        .unwrap();
    assert!(request.payload_schema.typed_fields.iter().any(|field| {
        field.field == SourcePayloadField::Named("path_segments".to_owned())
            && matches!(field.data_type, SemanticDataType::List { .. })
    }));
}

#[test]
fn effect_result_state_lowers_as_the_next_effect_trigger() {
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
    let typed = lower(&parsed).unwrap();
    let branch = typed
        .update_branches
        .iter()
        .find(|branch| {
            matches!(&branch.expression,
                UpdateExpression::HostEffect { operation, .. }
                    if operation == "Random/bytes")
        })
        .expect("state-triggered Random/bytes branch");
    assert_eq!(branch.source, "store.clock_result");
    assert_eq!(branch.target, "store.random_result");
    assert_eq!(
        branch.guard,
        Some(UpdateGuard::ValueOneOf {
            input: "store.clock_result".to_owned(),
            values: vec!["WallClockRead".to_owned()],
        })
    );
    assert!(
        !typed
            .possible_causes
            .iter()
            .find(|cause| cause.target == "store.random_result")
            .unwrap()
            .sources
            .iter()
            .any(|source| source == "store.start"),
        "the second effect must not inherit the original SOURCE trigger"
    );
}

#[test]
fn effect_result_changes_flow_through_pure_values_into_hold_branches() {
    let parsed = boon_parser::parse_source(
        "effect-result-derived-hold-branch.bn",
        r#"
store: [
    start: SOURCE
    clock_result:
        ClockNotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    current_label:
        clock_result |> WHEN {
            WallClockRead => TEXT { current }
            __ => TEXT { none }
        }
    selected_label:
        TEXT { none } |> HOLD selected_label {
            current_label |> WHEN {
                TEXT { current } => current_label
                __ => SKIP
            }
        }
]
"#,
    )
    .unwrap();
    let typed = lower(&parsed).unwrap();
    let branch = typed
        .update_branches
        .iter()
        .find(|branch| {
            branch.target == "store.selected_label" && branch.source == "store.clock_result"
        })
        .expect("effect result must trigger the HOLD branch through its pure projection");
    assert!(
        !matches!(branch.expression, UpdateExpression::Unknown { .. }),
        "derived state-triggered branch did not lower: {branch:?}"
    );
    assert!(
        typed
            .possible_causes
            .iter()
            .find(|cause| cause.target == "store.selected_label")
            .unwrap()
            .sources
            .iter()
            .any(|source| source == "store.clock_result")
    );
}

#[test]
fn nested_effect_result_guards_preserve_statement_ancestry() {
    let parsed = boon_parser::parse_source(
        "nested-effect-result-guards.bn",
        r#"
store: [
    read: SOURCE
    selected: PackageAsset[url: TEXT { asset://files/primary.vcd }]
    file_result:
        NotStarted |> HOLD file_result {
            read |> THEN {
                File/read_stream(
                    file: selected
                    retain_content: True
                )
            }
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
    let typed = lower(&parsed).unwrap();
    let branch = typed
        .update_branches
        .iter()
        .find(|branch| {
            matches!(&branch.expression,
                UpdateExpression::HostEffect { operation, .. }
                    if operation == "Wellen/open")
        })
        .expect("state-triggered Wellen/open branch");
    assert_eq!(
        branch.guard,
        Some(UpdateGuard::All {
            guards: vec![
                UpdateGuard::ValueOneOf {
                    input: "store.file_result".to_owned(),
                    values: vec!["Finished".to_owned()],
                },
                UpdateGuard::ValueOneOf {
                    input: "store.file_result.retained".to_owned(),
                    values: vec!["Retained".to_owned()],
                },
            ],
        })
    );
}

#[test]
fn nested_effect_guards_preserve_field_equality() {
    let parsed = boon_parser::parse_source(
        "nested-effect-field-equality.bn",
        r#"
store: [
    start: SOURCE
    replace_request: SOURCE
    request_fingerprint:
        TEXT { current } |> HOLD request_fingerprint {
            replace_request.text
        }
    response_fingerprint: TEXT { current }
    clock_result:
        ClockNotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    random_result:
        RandomNotRequested |> HOLD random_result {
            clock_result |> WHEN {
                WallClockRead => request_fingerprint == response_fingerprint |> WHEN {
                    True => Random/bytes(byte_count: 16)
                    False => SKIP
                }
                __ => SKIP
            }
        }
]
"#,
    )
    .unwrap();
    let typed = lower(&parsed).unwrap();
    let branch = typed
        .update_branches
        .iter()
        .find(|branch| {
            matches!(&branch.expression,
                UpdateExpression::HostEffect { operation, .. }
                    if operation == "Random/bytes")
        })
        .expect("field-equality guarded Random/bytes branch");
    assert_eq!(
        branch.guard,
        Some(UpdateGuard::All {
            guards: vec![
                UpdateGuard::ValueOneOf {
                    input: "store.clock_result".to_owned(),
                    values: vec!["WallClockRead".to_owned()],
                },
                UpdateGuard::ValuesEqual {
                    left: "store.request_fingerprint".to_owned(),
                    right: "store.response_fingerprint".to_owned(),
                },
            ],
        })
    );
}

#[test]
fn nested_effect_guards_preserve_scalar_list_nonempty_predicates() {
    let parsed = boon_parser::parse_source(
        "nested-effect-list-nonempty.bn",
        r#"
store: [
    start: SOURCE
    signal_ids: LIST { TEXT { top.clk } }
    random_result:
        RandomNotRequested |> HOLD random_result {
            start |> THEN {
                signal_ids |> List/is_not_empty() |> WHEN {
                    True => Random/bytes(byte_count: 1)
                    False => SKIP
                }
            }
        }
]
"#,
    )
    .unwrap();
    let typed = lower(&parsed).unwrap();
    let branch = typed
        .update_branches
        .iter()
        .find(|branch| {
            matches!(&branch.expression,
                UpdateExpression::HostEffect { operation, .. }
                    if operation == "Random/bytes")
        })
        .expect("list-guarded Random/bytes branch");

    assert_eq!(
        branch.guard,
        Some(UpdateGuard::ListIsNotEmpty {
            input: "store.signal_ids".to_owned(),
            expected: true,
        })
    );
}

#[test]
fn typed_passkey_call_lowers_to_its_result_hold() {
    let parsed = boon_parser::parse_source(
        "typed-passkey-effects.bn",
        r#"
store: [
    authenticate: SOURCE
    simulate_cancel: SOURCE
    simulate_failure: SOURCE
    simulate_duplicate: SOURCE
    account_id: TEXT { account-1 }
    credential_count: 1
    simulation:
        Success |> HOLD simulation {
            LATEST {
                store.simulate_cancel |> THEN { Cancel }
                store.simulate_failure |> THEN { Failure }
                store.simulate_duplicate |> THEN { Duplicate }
            }
        }
    authentication_result:
        AuthenticationNotRequested |> HOLD authentication_result {
            store.authenticate |> THEN {
                DevelopmentPasskey/authenticate(
                    account_id: store.account_id
                    credential_count: store.credential_count
                    simulation: store.simulation
                )
            }
        }
]
"#,
    )
    .unwrap();
    let typed = lower(&parsed).unwrap();
    let branch = typed
        .update_branches
        .iter()
        .find(|branch| {
            matches!(&branch.expression,
            UpdateExpression::HostEffect { operation, .. }
                if operation == "DevelopmentPasskey/authenticate")
        })
        .unwrap();
    assert_eq!(branch.source, "store.authenticate");
    assert_eq!(branch.target, "store.authentication_result");
    let UpdateExpression::HostEffect { arguments, .. } = &branch.expression else {
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
