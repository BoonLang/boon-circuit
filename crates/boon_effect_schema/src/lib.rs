#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplaySpec {
    ReadOnly,
    IdempotentBytesKey,
    NonReplayable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierSpec {
    None,
    Before,
    BeforeAndAfter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultPolicySpec {
    ReturnValue,
    Acknowledgement,
    CorrelatedSource,
    Discarded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueType {
    Bool,
    Number,
    Text,
    Bytes { fixed_len: Option<u64> },
    Record { fields: Vec<Field>, open: bool },
    Variant { variants: Vec<Variant> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub value_type: ValueType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variant {
    pub tag: &'static str,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableSchema {
    pub intent: ValueType,
    pub result: ValueType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEffectSpec {
    pub operation: &'static str,
    pub replay: ReplaySpec,
    pub barrier: BarrierSpec,
    pub result_policy: ResultPolicySpec,
    pub durable_schema: Option<DurableSchema>,
}

pub fn host_effect_spec(operation: &str) -> Option<HostEffectSpec> {
    let simple = match operation {
        "Directory/entries" => Some((
            ReplaySpec::ReadOnly,
            BarrierSpec::None,
            ResultPolicySpec::ReturnValue,
        )),
        "File/read_bytes" | "File/read_text" => Some((
            ReplaySpec::ReadOnly,
            BarrierSpec::None,
            ResultPolicySpec::ReturnValue,
        )),
        "File/write_text" => Some((
            ReplaySpec::NonReplayable,
            BarrierSpec::BeforeAndAfter,
            ResultPolicySpec::Acknowledgement,
        )),
        "Log/error" | "Log/info" => Some((
            ReplaySpec::NonReplayable,
            BarrierSpec::None,
            ResultPolicySpec::Discarded,
        )),
        _ => None,
    };
    if let Some((replay, barrier, result_policy)) = simple {
        return Some(HostEffectSpec {
            operation: canonical_operation(operation),
            replay,
            barrier,
            result_policy,
            durable_schema: None,
        });
    }
    match operation {
        "File/write_bytes" => Some(HostEffectSpec {
            operation: "File/write_bytes",
            replay: ReplaySpec::IdempotentBytesKey,
            barrier: BarrierSpec::BeforeAndAfter,
            result_policy: ResultPolicySpec::Acknowledgement,
            durable_schema: Some(DurableSchema {
                intent: record([
                    field("bytes", ValueType::Bytes { fixed_len: None }),
                    field("path", ValueType::Text),
                ]),
                result: ValueType::Text,
            }),
        }),
        "DevelopmentPasskey/register" => Some(development_passkey_registration()),
        "DevelopmentPasskey/authenticate" => Some(development_passkey_authentication()),
        _ => None,
    }
}

fn canonical_operation(operation: &str) -> &'static str {
    match operation {
        "Directory/entries" => "Directory/entries",
        "File/read_bytes" => "File/read_bytes",
        "File/read_text" => "File/read_text",
        "File/write_text" => "File/write_text",
        "Log/error" => "Log/error",
        "Log/info" => "Log/info",
        _ => unreachable!("caller filters known operations"),
    }
}

fn development_passkey_registration() -> HostEffectSpec {
    HostEffectSpec {
        operation: "DevelopmentPasskey/register",
        replay: ReplaySpec::IdempotentBytesKey,
        barrier: BarrierSpec::BeforeAndAfter,
        result_policy: ResultPolicySpec::CorrelatedSource,
        durable_schema: Some(DurableSchema {
            intent: record([
                field("workspace_id", ValueType::Text),
                field("workspace_grant_id", ValueType::Text),
                field("account_id", ValueType::Text),
                field("credential_count", ValueType::Number),
                field("simulation", development_simulation()),
            ]),
            result: ValueType::Variant {
                variants: vec![
                    variant(
                        "RegistrationSucceeded",
                        [
                            field("account_id", ValueType::Text),
                            field("credential_id", ValueType::Text),
                            field("label", ValueType::Text),
                            field("workspace_grant_bound", ValueType::Bool),
                        ],
                    ),
                    variant("RegistrationCancelled", []),
                    variant(
                        "RegistrationFailed",
                        [
                            field("code", ValueType::Text),
                            field("message", ValueType::Text),
                            field("retryable", ValueType::Bool),
                        ],
                    ),
                    variant(
                        "DuplicateCredential",
                        [
                            field("account_id", ValueType::Text),
                            field("credential_id", ValueType::Text),
                        ],
                    ),
                ],
            },
        }),
    }
}

fn development_passkey_authentication() -> HostEffectSpec {
    HostEffectSpec {
        operation: "DevelopmentPasskey/authenticate",
        replay: ReplaySpec::IdempotentBytesKey,
        barrier: BarrierSpec::BeforeAndAfter,
        result_policy: ResultPolicySpec::CorrelatedSource,
        durable_schema: Some(DurableSchema {
            intent: record([
                field("account_id", ValueType::Text),
                field("credential_count", ValueType::Number),
                field("simulation", development_simulation()),
            ]),
            result: ValueType::Variant {
                variants: vec![
                    variant(
                        "AuthenticationSucceeded",
                        [
                            field("account_id", ValueType::Text),
                            field("credential_id", ValueType::Text),
                        ],
                    ),
                    variant("AuthenticationCancelled", []),
                    variant(
                        "AuthenticationFailed",
                        [
                            field("code", ValueType::Text),
                            field("message", ValueType::Text),
                            field("retryable", ValueType::Bool),
                        ],
                    ),
                ],
            },
        }),
    }
}

fn development_simulation() -> ValueType {
    ValueType::Variant {
        variants: ["Success", "Cancel", "Failure", "Duplicate"]
            .into_iter()
            .map(|tag| variant(tag, []))
            .collect(),
    }
}

fn record<const N: usize>(fields: [Field; N]) -> ValueType {
    ValueType::Record {
        fields: fields.into(),
        open: false,
    }
}

fn field(name: &'static str, value_type: ValueType) -> Field {
    Field { name, value_type }
}

fn variant<const N: usize>(tag: &'static str, fields: [Field; N]) -> Variant {
    Variant {
        tag,
        fields: fields.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_passkey_is_explicit_and_has_closed_durable_schemas() {
        for operation in [
            "DevelopmentPasskey/register",
            "DevelopmentPasskey/authenticate",
        ] {
            let spec = host_effect_spec(operation).unwrap();
            assert_eq!(spec.operation, operation);
            assert_eq!(spec.replay, ReplaySpec::IdempotentBytesKey);
            assert_eq!(spec.result_policy, ResultPolicySpec::CorrelatedSource);
            let schema = spec.durable_schema.unwrap();
            assert!(matches!(
                schema.intent,
                ValueType::Record { open: false, .. }
            ));
            assert!(matches!(schema.result, ValueType::Variant { .. }));
        }
        assert!(host_effect_spec("Passkey/register").is_none());
    }

    #[test]
    fn development_registration_binds_an_explicit_workspace_grant() {
        let schema = host_effect_spec("DevelopmentPasskey/register")
            .unwrap()
            .durable_schema
            .unwrap();
        let ValueType::Record {
            fields,
            open: false,
        } = schema.intent
        else {
            panic!("registration intent must be a closed record");
        };
        assert_eq!(
            fields.iter().map(|field| field.name).collect::<Vec<_>>(),
            [
                "workspace_id",
                "workspace_grant_id",
                "account_id",
                "credential_count",
                "simulation",
            ]
        );
        let ValueType::Variant { variants } = schema.result else {
            panic!("registration result must be a closed variant");
        };
        let success = variants
            .iter()
            .find(|variant| variant.tag == "RegistrationSucceeded")
            .unwrap();
        assert!(success.fields.iter().any(|field| {
            field.name == "workspace_grant_bound" && field.value_type == ValueType::Bool
        }));
    }
}
