use super::*;

// Typecheck tests are grouped by language surface while staying in this module for private helper access.
include!("tests/flow_and_state.rs");
include!("tests/functions_and_arguments.rs");

fn passkey_effect_fixture() -> ParsedProgram {
    boon_parser::parse_source(
        "typed-passkey-effects.bn",
        r#"
store: [
    register: SOURCE
    registration_succeeded: SOURCE
    registration_cancelled: SOURCE
    registration_failed: SOURCE
    duplicate_credential: SOURCE
    simulate_cancel: SOURCE
    simulate_failure: SOURCE
    simulate_duplicate: SOURCE
    workspace_id: TEXT { workspace-1 }
    workspace_grant_id: TEXT { grant-1 }
    account_id: TEXT {}
    credential_count: 0
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
    protect_workspace: [
        on: store.register
        perform: DevelopmentPasskey/register(
            workspace_id: store.workspace_id
            workspace_grant_id: store.workspace_grant_id
            account_id: store.account_id
            credential_count: store.credential_count
            simulation: store.simulation
        )
        results: [
            RegistrationSucceeded: store.registration_succeeded
            RegistrationCancelled: store.registration_cancelled
            RegistrationFailed: store.registration_failed
            DuplicateCredential: store.duplicate_credential
        ]
    ]
]
"#,
    )
    .unwrap()
}

#[test]
fn passkey_effect_declaration_has_closed_named_intent_and_typed_result_sources() {
    let report = check(&passkey_effect_fixture());
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
    let declaration = &report.host_effect_table.declarations[0];
    assert_eq!(declaration.name, "protect_workspace");
    assert_eq!(declaration.operation, "DevelopmentPasskey/register");
    let Type::VariantSet(simulation_variants) = &declaration
        .intent_fields
        .iter()
        .find(|field| field.name == "simulation")
        .unwrap()
        .value_type
    else {
        panic!("development simulation must be a closed variant");
    };
    assert_eq!(
        simulation_variants
            .iter()
            .map(|variant| match variant {
                Variant::Tag(tag) | Variant::Tagged { tag, .. } => tag.as_str(),
            })
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Success", "Cancel", "Failure", "Duplicate"])
    );
    assert_eq!(
        declaration
            .intent_fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        [
            "workspace_id",
            "workspace_grant_id",
            "account_id",
            "credential_count",
            "simulation"
        ]
    );
    assert_eq!(
        declaration
            .result_routes
            .iter()
            .map(|route| route.variant.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "RegistrationSucceeded",
            "RegistrationCancelled",
            "RegistrationFailed",
            "DuplicateCredential",
        ])
    );
    let failure = report
        .source_payload_shape_table
        .iter()
        .find(|entry| entry.source_path == "store.registration_failed")
        .unwrap();
    assert_eq!(
        failure
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.ty.clone()))
            .collect::<BTreeMap<_, _>>(),
        BTreeMap::from([
            ("code", Type::Text),
            ("message", Type::Text),
            ("retryable", true_false_type()),
        ])
    );
}

#[test]
fn variant_assignment_accepts_narrow_values_and_rejects_wider_values() {
    let success = Type::VariantSet(vec![Variant::Tag("Success".to_owned())]);
    let development_outcome = Type::VariantSet(vec![
        Variant::Tag("Cancel".to_owned()),
        Variant::Tag("Duplicate".to_owned()),
        Variant::Tag("Failure".to_owned()),
        Variant::Tag("Success".to_owned()),
    ]);

    assert!(type_is_assignable_to(&success, &development_outcome));
    assert!(!type_is_assignable_to(&development_outcome, &success));
}

#[test]
fn typed_host_effect_allows_multiple_declarations_for_one_operation() {
    let parsed = boon_parser::parse_source(
        "repeated-typed-host-effect.bn",
        r#"
store: [
    register_success: SOURCE
    register_cancel: SOURCE
    registration_succeeded: SOURCE
    registration_cancelled: SOURCE
    registration_failed: SOURCE
    duplicate_credential: SOURCE
    workspace_id: TEXT { workspace-1 }
    workspace_grant_id: TEXT { grant-1 }
    account_id: TEXT {}
    credential_count: 0
]

effects: [
    success: [
        on: store.register_success
        perform: DevelopmentPasskey/register(
            workspace_id: store.workspace_id
            workspace_grant_id: store.workspace_grant_id
            account_id: store.account_id
            credential_count: store.credential_count
            simulation: Success
        )
        results: [
            RegistrationSucceeded: store.registration_succeeded
            RegistrationCancelled: store.registration_cancelled
            RegistrationFailed: store.registration_failed
            DuplicateCredential: store.duplicate_credential
        ]
    ]
    cancel: [
        on: store.register_cancel
        perform: DevelopmentPasskey/register(
            workspace_id: store.workspace_id
            workspace_grant_id: store.workspace_grant_id
            account_id: store.account_id
            credential_count: store.credential_count
            simulation: Cancel
        )
        results: [
            RegistrationSucceeded: store.registration_succeeded
            RegistrationCancelled: store.registration_cancelled
            RegistrationFailed: store.registration_failed
            DuplicateCredential: store.duplicate_credential
        ]
    ]
]
"#,
    )
    .unwrap();
    let report = check(&parsed);

    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
    assert_eq!(report.host_effect_table.declarations.len(), 2);
    assert_ne!(
        report.host_effect_table.declarations[0].perform_expr_id,
        report.host_effect_table.declarations[1].perform_expr_id
    );
}

#[test]
fn passkey_effect_declaration_rejects_missing_result_route() {
    let mut parsed = passkey_effect_fixture();
    let route = parsed
        .ast
        .statements
        .iter_mut()
        .find(|statement| statement_field_name(statement) == Some("effects"))
        .unwrap()
        .children[0]
        .children
        .iter_mut()
        .find(|statement| statement_field_name(statement) == Some("results"))
        .unwrap();
    route
        .children
        .retain(|statement| statement_field_name(statement) != Some("RegistrationCancelled"));
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("missing result route `RegistrationCancelled`")
    }));
}

#[test]
fn piped_host_effect_call_outside_effect_declaration_is_rejected() {
    let parsed = boon_parser::parse_source(
        "piped-host-effect-outside-declaration.bn",
        r#"
store: [
    trigger: SOURCE
    workspace_id: TEXT { workspace-1 }
    account_id: TEXT {}
    credential_count: 0
    simulation: Success
    invalid:
        store.workspace_id |> DevelopmentPasskey/register(
            workspace_grant_id: TEXT { grant-1 }
            account_id: store.account_id
            credential_count: store.credential_count
            simulation: store.simulation
        )
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("typed host effect `DevelopmentPasskey/register` may only appear")
    }));
}

#[test]
fn list_chunk_type_uses_declared_output_field_names() {
    let parsed = boon_parser::parse_source(
        "chunk-fields.bn",
        "events: SOURCE\nvalue: 0 |> HOLD value { LATEST { events |> THEN { value } } }\nvalues: LIST {}\nrows: List/chunk(values, size: 2, items: group, label: index)",
    )
    .unwrap();
    let expression = parsed
        .expressions
        .iter()
        .find(|expression| {
            matches!(&expression.kind, AstExprKind::Call { function, .. } if function == "List/chunk")
                || matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "List/chunk")
        })
        .unwrap();
    let Type::List(item) = simple_expr_type(expression, &parsed.expressions) else {
        panic!("List/chunk should infer a list");
    };
    let Type::Object(shape) = item.as_ref() else {
        panic!("List/chunk should infer object rows");
    };

    assert_eq!(
        shape
            .ordered_fields()
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["index", "group"]
    );
    assert!(!shape.fields.contains_key("row_number"));
    assert!(!shape.fields.contains_key("cells"));
}
