use super::*;

// IR tests are grouped by lowering domain while staying in this module for private helper access.
include!("tests/bytes.rs");
include!("tests/cells.rs");
include!("tests/output_roots.rs");
include!("tests/host_ports.rs");
include!("tests/indexed_queries.rs");
include!("tests/migrations.rs");
include!("tests/sources_and_events.rs");
include!("tests/todomvc.rs");

#[test]
fn outbound_http_effect_lowers_recursive_schema_without_executable_string_routes() {
    let parsed = boon_parser::parse_source(
        "outbound-http-effect.bn",
        include_str!("../../../examples/outbound_http_effect.bn"),
    )
    .unwrap();
    let typed = lower(&parsed).unwrap();
    let [effect] = typed.host_effects.as_slice() else {
        panic!("expected one typed outbound effect");
    };
    assert_eq!(effect.operation, "Http/request");
    assert!(matches!(
        effect
            .intent_fields
            .iter()
            .find(|field| field.name == "headers")
            .map(|field| &field.data_type),
        Some(SemanticDataType::List { item })
            if matches!(item.as_ref(), SemanticDataType::Record { open: false, .. })
    ));
    assert_eq!(
        effect
            .result_routes
            .iter()
            .map(|route| route.variant.as_str())
            .collect::<Vec<_>>(),
        ["HttpFailed", "HttpSucceeded"]
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
fn typed_passkey_effects_lower_as_metadata_with_typed_result_sources() {
    let parsed = boon_parser::parse_source(
        "typed-passkey-effects.bn",
        r#"
store: [
    authenticate: SOURCE
    authentication_succeeded: SOURCE
    authentication_cancelled: SOURCE
    authentication_failed: SOURCE
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
]

effects: [
    sign_in: [
        on: store.authenticate
        perform: DevelopmentPasskey/authenticate(
            account_id: store.account_id
            credential_count: store.credential_count
            simulation: store.simulation
        )
        results: [
            AuthenticationSucceeded: store.authentication_succeeded
            AuthenticationCancelled: store.authentication_cancelled
            AuthenticationFailed: store.authentication_failed
        ]
    ]
]
"#,
    )
    .unwrap();
    let typed = lower(&parsed).unwrap();
    assert_eq!(typed.host_effects.len(), 1);
    let effect = &typed.host_effects[0];
    assert_eq!(effect.name, "sign_in");
    assert_eq!(effect.operation, "DevelopmentPasskey/authenticate");
    let SemanticDataType::Variant { variants } = &effect
        .intent_fields
        .iter()
        .find(|field| field.name == "simulation")
        .unwrap()
        .data_type
    else {
        panic!("simulation intent must remain a closed variant");
    };
    assert_eq!(
        variants
            .iter()
            .map(|variant| variant.tag.as_str())
            .collect::<Vec<_>>(),
        ["Cancel", "Duplicate", "Failure", "Success"]
    );
    assert!(
        variants
            .iter()
            .all(|variant| !variant.open && variant.fields.is_empty())
    );
    assert_eq!(
        effect
            .intent_fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["account_id", "credential_count", "simulation"]
    );
    assert!(
        typed
            .derived_values
            .iter()
            .all(|value| !value.path.starts_with("effects."))
    );
    let failure = typed
        .sources
        .iter()
        .find(|source| source.path == "store.authentication_failed")
        .unwrap();
    assert!(failure.payload_schema.typed_fields.iter().any(|field| {
        field.field == SourcePayloadField::Named("retryable".to_owned())
            && field.data_type == SemanticDataType::Bool
    }));
}
