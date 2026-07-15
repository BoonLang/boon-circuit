use super::*;

// IR tests are grouped by lowering domain while staying in this module for private helper access.
include!("tests/bytes.rs");
include!("tests/cells.rs");
include!("tests/output_roots.rs");
include!("tests/migrations.rs");
include!("tests/sources_and_events.rs");
include!("tests/todomvc.rs");

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
            && field.value_type == SourcePayloadValueType::Bool
    }));
}
