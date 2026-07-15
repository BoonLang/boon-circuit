use boon_persistence::StoredValue;
use boon_plan::EffectId;
use boon_runtime::{
    HostEffectDriver, HostEffectError, HostEffectReconciliation, HostEffectRequest,
};
use std::collections::BTreeMap;

pub const REGISTER_OPERATION: &str = "DevelopmentPasskey/register";
pub const AUTHENTICATE_OPERATION: &str = "DevelopmentPasskey/authenticate";

#[derive(Clone, Copy)]
enum Operation {
    Register,
    Authenticate,
}

/// Deterministic native development adapter for the production Passkey port.
/// It creates public credential descriptors only; no private credential bytes
/// exist in this process or in Boon authority.
pub struct DevelopmentPasskeySimulator {
    operation: Operation,
}

impl DevelopmentPasskeySimulator {
    pub fn registration() -> Self {
        Self {
            operation: Operation::Register,
        }
    }

    pub fn authentication() -> Self {
        Self {
            operation: Operation::Authenticate,
        }
    }

    fn result(&self, request: &HostEffectRequest) -> Result<StoredValue, HostEffectError> {
        let expected = match self.operation {
            Operation::Register => REGISTER_OPERATION,
            Operation::Authenticate => AUTHENTICATE_OPERATION,
        };
        let expected_id = EffectId::from_host_operation(expected)
            .map_err(|error| HostEffectError::rejected(error.to_string()))?;
        if request.effect_id != expected_id {
            return Err(HostEffectError::rejected(format!(
                "development passkey simulator for `{expected}` does not own effect {}",
                request.effect_id
            )));
        }
        let StoredValue::Record(intent) = &request.intent else {
            return Err(HostEffectError::rejected(format!(
                "{expected} intent is not a record"
            )));
        };
        match self.operation {
            Operation::Register => registration_result(intent),
            Operation::Authenticate => authentication_result(intent),
        }
    }
}

impl HostEffectDriver for DevelopmentPasskeySimulator {
    fn dispatch(&mut self, request: &HostEffectRequest) -> Result<StoredValue, HostEffectError> {
        self.result(request)
    }

    fn reconcile(
        &mut self,
        request: &HostEffectRequest,
    ) -> Result<HostEffectReconciliation, HostEffectError> {
        self.result(request).map(HostEffectReconciliation::Applied)
    }
}

fn registration_result(
    intent: &BTreeMap<String, StoredValue>,
) -> Result<StoredValue, HostEffectError> {
    let workspace_id = text_field(intent, "workspace_id")?;
    let workspace_grant_id = text_field(intent, "workspace_grant_id")?;
    let account_id = text_field(intent, "account_id")?;
    let credential_count = number_field(intent, "credential_count")?;
    let simulation = variant_tag_field(intent, "simulation")?;
    if workspace_id.is_empty() {
        return Err(HostEffectError::rejected(
            "passkey registration requires a workspace identity",
        ));
    }
    if account_id.is_empty() && workspace_grant_id.is_empty() {
        return Err(HostEffectError::rejected(
            "first passkey registration requires an anonymous workspace grant",
        ));
    }
    let first_registration = account_id.is_empty();
    let account_id = if first_registration {
        stable_public_id("account", workspace_grant_id, 0)
    } else {
        account_id.to_owned()
    };
    Ok(match simulation {
        "Cancel" => variant("RegistrationCancelled", []),
        "Failure" => failure_variant(
            "RegistrationFailed",
            "development_registration_failed",
            "The development simulator rejected registration.",
        ),
        "Duplicate" => variant(
            "DuplicateCredential",
            [
                ("account_id", StoredValue::Text(account_id.clone())),
                (
                    "credential_id",
                    StoredValue::Text(stable_public_id(
                        "credential",
                        &account_id,
                        credential_count.max(1),
                    )),
                ),
            ],
        ),
        "Success" if credential_count < 2 => {
            let ordinal = credential_count + 1;
            variant(
                "RegistrationSucceeded",
                [
                    ("account_id", StoredValue::Text(account_id.clone())),
                    (
                        "credential_id",
                        StoredValue::Text(stable_public_id("credential", &account_id, ordinal)),
                    ),
                    (
                        "label",
                        StoredValue::Text(format!("Development passkey {ordinal}")),
                    ),
                    (
                        "workspace_grant_bound",
                        StoredValue::Bool(first_registration),
                    ),
                ],
            )
        }
        "Success" => variant(
            "DuplicateCredential",
            [
                ("account_id", StoredValue::Text(account_id.clone())),
                (
                    "credential_id",
                    StoredValue::Text(stable_public_id("credential", &account_id, 2)),
                ),
            ],
        ),
        other => failure_variant(
            "RegistrationFailed",
            "invalid_simulation_outcome",
            &format!("Unknown development simulation outcome `{other}`."),
        ),
    })
}

fn authentication_result(
    intent: &BTreeMap<String, StoredValue>,
) -> Result<StoredValue, HostEffectError> {
    let account_id = text_field(intent, "account_id")?;
    let credential_count = number_field(intent, "credential_count")?;
    let simulation = variant_tag_field(intent, "simulation")?;
    Ok(match simulation {
        "Cancel" => variant("AuthenticationCancelled", []),
        "Failure" => failure_variant(
            "AuthenticationFailed",
            "development_authentication_failed",
            "The development simulator rejected authentication.",
        ),
        "Success" if !account_id.is_empty() && credential_count > 0 => variant(
            "AuthenticationSucceeded",
            [
                ("account_id", StoredValue::Text(account_id.to_owned())),
                (
                    "credential_id",
                    StoredValue::Text(stable_public_id("credential", account_id, 1)),
                ),
            ],
        ),
        "Success" => failure_variant(
            "AuthenticationFailed",
            "no_registered_credential",
            "No registered development credential is available.",
        ),
        other => failure_variant(
            "AuthenticationFailed",
            "invalid_simulation_outcome",
            &format!("Unknown development simulation outcome `{other}`."),
        ),
    })
}

fn stable_public_id(kind: &str, authority: &str, ordinal: i64) -> String {
    let digest = boon_runtime::sha256_bytes(format!("{kind}:{authority}:{ordinal}").as_bytes());
    format!("{kind}-{}", &digest[..20])
}

fn variant<const N: usize>(tag: &str, fields: [(&str, StoredValue); N]) -> StoredValue {
    StoredValue::Variant {
        tag: tag.to_owned(),
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    }
}

fn failure_variant(tag: &str, code: &str, message: &str) -> StoredValue {
    variant(
        tag,
        [
            ("code", StoredValue::Text(code.to_owned())),
            ("message", StoredValue::Text(message.to_owned())),
            ("retryable", StoredValue::Bool(true)),
        ],
    )
}

fn text_field<'a>(
    intent: &'a BTreeMap<String, StoredValue>,
    name: &str,
) -> Result<&'a str, HostEffectError> {
    match intent.get(name) {
        Some(StoredValue::Text(value)) => Ok(value),
        _ => Err(HostEffectError::rejected(format!(
            "passkey intent field `{name}` is not Text"
        ))),
    }
}

fn number_field(
    intent: &BTreeMap<String, StoredValue>,
    name: &str,
) -> Result<i64, HostEffectError> {
    match intent.get(name) {
        Some(StoredValue::Number(value)) => Ok(*value),
        _ => Err(HostEffectError::rejected(format!(
            "passkey intent field `{name}` is not Number"
        ))),
    }
}

fn variant_tag_field<'a>(
    intent: &'a BTreeMap<String, StoredValue>,
    name: &str,
) -> Result<&'a str, HostEffectError> {
    match intent.get(name) {
        Some(StoredValue::Variant { tag, fields }) if fields.is_empty() => Ok(tag),
        _ => Err(HostEffectError::rejected(format!(
            "passkey intent field `{name}` is not a fieldless variant"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_persistence::OutboxItemId;
    use boon_plan::EffectInvocationId;

    fn request(operation: &str, intent: BTreeMap<String, StoredValue>) -> HostEffectRequest {
        let effect_id = EffectId::from_host_operation(operation).unwrap();
        let invocation_id =
            EffectInvocationId::from_semantic_route(effect_id, "test.press", "test.result")
                .unwrap();
        HostEffectRequest {
            item_id: OutboxItemId([7; 32]),
            invocation_id,
            effect_id,
            idempotency_key: StoredValue::Bytes(vec![0; 32]),
            intent: StoredValue::Record(intent),
            attempt: 1,
        }
    }

    #[test]
    fn registration_simulates_success_cancel_failure_duplicate_and_second_credential() {
        let mut simulator = DevelopmentPasskeySimulator::registration();
        for (simulation, count, expected) in [
            ("Success", 0, "RegistrationSucceeded"),
            ("Cancel", 0, "RegistrationCancelled"),
            ("Failure", 0, "RegistrationFailed"),
            ("Duplicate", 1, "DuplicateCredential"),
            ("Success", 1, "RegistrationSucceeded"),
            ("Success", 2, "DuplicateCredential"),
        ] {
            let outcome = simulator
                .dispatch(&request(
                    REGISTER_OPERATION,
                    BTreeMap::from([
                        (
                            "workspace_id".to_owned(),
                            StoredValue::Text("workspace-test".to_owned()),
                        ),
                        (
                            "workspace_grant_id".to_owned(),
                            StoredValue::Text("grant-test".to_owned()),
                        ),
                        ("account_id".to_owned(), StoredValue::Text(String::new())),
                        ("credential_count".to_owned(), StoredValue::Number(count)),
                        ("simulation".to_owned(), variant(simulation, [])),
                    ]),
                ))
                .unwrap();
            assert!(matches!(outcome, StoredValue::Variant { ref tag, .. } if tag == expected));
        }
    }

    #[test]
    fn registration_binds_private_grant_only_for_the_first_account_credential() {
        let mut simulator = DevelopmentPasskeySimulator::registration();
        let first = simulator
            .dispatch(&request(
                REGISTER_OPERATION,
                BTreeMap::from([
                    (
                        "workspace_id".to_owned(),
                        StoredValue::Text("workspace-public".to_owned()),
                    ),
                    (
                        "workspace_grant_id".to_owned(),
                        StoredValue::Text("workspace-private-grant".to_owned()),
                    ),
                    ("account_id".to_owned(), StoredValue::Text(String::new())),
                    ("credential_count".to_owned(), StoredValue::Number(0)),
                    ("simulation".to_owned(), variant("Success", [])),
                ]),
            ))
            .unwrap();
        let StoredValue::Variant {
            tag,
            fields: first_fields,
        } = first
        else {
            panic!("registration result must be a variant");
        };
        assert_eq!(tag, "RegistrationSucceeded");
        assert_eq!(
            first_fields["workspace_grant_bound"],
            StoredValue::Bool(true)
        );
        let StoredValue::Text(account_id) = &first_fields["account_id"] else {
            panic!("first registration must create a public account id");
        };

        let second = simulator
            .dispatch(&request(
                REGISTER_OPERATION,
                BTreeMap::from([
                    (
                        "workspace_id".to_owned(),
                        StoredValue::Text("workspace-public".to_owned()),
                    ),
                    (
                        "workspace_grant_id".to_owned(),
                        StoredValue::Text(String::new()),
                    ),
                    (
                        "account_id".to_owned(),
                        StoredValue::Text(account_id.clone()),
                    ),
                    ("credential_count".to_owned(), StoredValue::Number(1)),
                    ("simulation".to_owned(), variant("Success", [])),
                ]),
            ))
            .unwrap();
        let StoredValue::Variant { tag, fields } = second else {
            panic!("registration result must be a variant");
        };
        assert_eq!(tag, "RegistrationSucceeded");
        assert_eq!(fields["workspace_grant_bound"], StoredValue::Bool(false));
    }

    #[test]
    fn authentication_requires_a_public_credential_descriptor() {
        let mut simulator = DevelopmentPasskeySimulator::authentication();
        let outcome = simulator
            .dispatch(&request(
                AUTHENTICATE_OPERATION,
                BTreeMap::from([
                    ("account_id".to_owned(), StoredValue::Text(String::new())),
                    ("credential_count".to_owned(), StoredValue::Number(0)),
                    ("simulation".to_owned(), variant("Success", [])),
                ]),
            ))
            .unwrap();
        assert!(matches!(
            outcome,
            StoredValue::Variant { ref tag, .. } if tag == "AuthenticationFailed"
        ));
    }

    #[test]
    fn authentication_simulates_success_cancellation_and_failure() {
        let mut simulator = DevelopmentPasskeySimulator::authentication();
        for (simulation, expected) in [
            ("Success", "AuthenticationSucceeded"),
            ("Cancel", "AuthenticationCancelled"),
            ("Failure", "AuthenticationFailed"),
        ] {
            let outcome = simulator
                .dispatch(&request(
                    AUTHENTICATE_OPERATION,
                    BTreeMap::from([
                        (
                            "account_id".to_owned(),
                            StoredValue::Text("account-test".to_owned()),
                        ),
                        ("credential_count".to_owned(), StoredValue::Number(2)),
                        ("simulation".to_owned(), variant(simulation, [])),
                    ]),
                ))
                .unwrap();
            assert!(matches!(outcome, StoredValue::Variant { ref tag, .. } if tag == expected));
        }
    }
}
