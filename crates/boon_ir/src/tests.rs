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
            "cancellation",
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
        Some(UpdateGuard::TriggerValueOneOf {
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
