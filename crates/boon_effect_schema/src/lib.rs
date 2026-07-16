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
    List { item: Box<ValueType> },
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
pub struct EffectSchema {
    pub intent: ValueType,
    pub result: ValueType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEffectSpec {
    pub operation: &'static str,
    pub replay: ReplaySpec,
    pub barrier: BarrierSpec,
    pub result_policy: ResultPolicySpec,
    pub schema: Option<EffectSchema>,
}

pub const OUTBOUND_HTTP_REQUEST_OPERATION: &str = "Http/request";
pub const WALL_CLOCK_READ_OPERATION: &str = "Clock/wall";
pub const SECURE_RANDOM_BYTES_OPERATION: &str = "Random/bytes";
pub const SECRET_VERIFY_OPERATION: &str = "Secret/verify";
pub const HMAC_SHA256_SIGN_OPERATION: &str = "Crypto/hmac_sha256_sign";
pub const HMAC_SHA256_VERIFY_OPERATION: &str = "Crypto/hmac_sha256_verify";
pub const TIMER_DEADLINE_OPERATION: &str = "Timer/deadline";

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
            schema: None,
        });
    }
    match operation {
        "File/write_bytes" => Some(HostEffectSpec {
            operation: "File/write_bytes",
            replay: ReplaySpec::IdempotentBytesKey,
            barrier: BarrierSpec::BeforeAndAfter,
            result_policy: ResultPolicySpec::Acknowledgement,
            schema: Some(EffectSchema {
                intent: record([
                    field("bytes", ValueType::Bytes { fixed_len: None }),
                    field("path", ValueType::Text),
                ]),
                result: ValueType::Text,
            }),
        }),
        "DevelopmentPasskey/register" => Some(development_passkey_registration()),
        "DevelopmentPasskey/authenticate" => Some(development_passkey_authentication()),
        OUTBOUND_HTTP_REQUEST_OPERATION => Some(outbound_http_request()),
        WALL_CLOCK_READ_OPERATION => Some(wall_clock_read()),
        SECURE_RANDOM_BYTES_OPERATION => Some(secure_random_bytes()),
        SECRET_VERIFY_OPERATION => Some(secret_verify()),
        HMAC_SHA256_SIGN_OPERATION => Some(hmac_sha256_sign()),
        HMAC_SHA256_VERIFY_OPERATION => Some(hmac_sha256_verify()),
        TIMER_DEADLINE_OPERATION => Some(timer_deadline()),
        _ => None,
    }
}

fn wall_clock_read() -> HostEffectSpec {
    transient_host_service(
        WALL_CLOCK_READ_OPERATION,
        ReplaySpec::ReadOnly,
        record([]),
        ValueType::Variant {
            variants: vec![
                variant(
                    "WallClockRead",
                    [
                        field("unix_seconds", ValueType::Number),
                        field("nanoseconds", ValueType::Number),
                    ],
                ),
                host_service_failure(),
            ],
        },
    )
}

fn secure_random_bytes() -> HostEffectSpec {
    transient_host_service(
        SECURE_RANDOM_BYTES_OPERATION,
        ReplaySpec::ReadOnly,
        record([field("byte_count", ValueType::Number)]),
        ValueType::Variant {
            variants: vec![
                variant(
                    "RandomBytesReady",
                    [field("bytes", ValueType::Bytes { fixed_len: None })],
                ),
                host_service_failure(),
            ],
        },
    )
}

fn secret_verify() -> HostEffectSpec {
    transient_host_service(
        SECRET_VERIFY_OPERATION,
        ReplaySpec::ReadOnly,
        record([
            field("secret", ValueType::Text),
            field("candidate", ValueType::Bytes { fixed_len: None }),
        ]),
        ValueType::Variant {
            variants: vec![
                variant("SecretVerified", [field("matches", ValueType::Bool)]),
                host_service_failure(),
            ],
        },
    )
}

fn hmac_sha256_sign() -> HostEffectSpec {
    transient_host_service(
        HMAC_SHA256_SIGN_OPERATION,
        ReplaySpec::ReadOnly,
        record([
            field("secret", ValueType::Text),
            field("message", ValueType::Bytes { fixed_len: None }),
        ]),
        ValueType::Variant {
            variants: vec![
                variant(
                    "HmacSigned",
                    [field(
                        "tag",
                        ValueType::Bytes {
                            fixed_len: Some(32),
                        },
                    )],
                ),
                host_service_failure(),
            ],
        },
    )
}

fn hmac_sha256_verify() -> HostEffectSpec {
    transient_host_service(
        HMAC_SHA256_VERIFY_OPERATION,
        ReplaySpec::ReadOnly,
        record([
            field("secret", ValueType::Text),
            field("message", ValueType::Bytes { fixed_len: None }),
            field(
                "tag",
                ValueType::Bytes {
                    fixed_len: Some(32),
                },
            ),
        ]),
        ValueType::Variant {
            variants: vec![
                variant("HmacVerified", [field("matches", ValueType::Bool)]),
                host_service_failure(),
            ],
        },
    )
}

fn timer_deadline() -> HostEffectSpec {
    transient_host_service(
        TIMER_DEADLINE_OPERATION,
        ReplaySpec::ReadOnly,
        record([field("delay_ms", ValueType::Number)]),
        ValueType::Variant {
            variants: vec![
                variant("TimerFired", [field("delay_ms", ValueType::Number)]),
                host_service_failure(),
            ],
        },
    )
}

fn transient_host_service(
    operation: &'static str,
    replay: ReplaySpec,
    intent: ValueType,
    result: ValueType,
) -> HostEffectSpec {
    HostEffectSpec {
        operation,
        replay,
        barrier: BarrierSpec::None,
        result_policy: ResultPolicySpec::CorrelatedSource,
        schema: Some(EffectSchema { intent, result }),
    }
}

fn host_service_failure() -> Variant {
    variant(
        "HostServiceFailed",
        [
            field("code", ValueType::Text),
            field("diagnostic", ValueType::Text),
        ],
    )
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
        schema: Some(EffectSchema {
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
        schema: Some(EffectSchema {
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

fn outbound_http_request() -> HostEffectSpec {
    let text_pair = || {
        record([
            field("name", ValueType::Text),
            field("value", ValueType::Text),
        ])
    };
    let header = || {
        record([
            field("name", ValueType::Text),
            field("value", ValueType::Bytes { fixed_len: None }),
        ])
    };
    let headers = || ValueType::List {
        item: Box::new(header()),
    };
    HostEffectSpec {
        operation: OUTBOUND_HTTP_REQUEST_OPERATION,
        replay: ReplaySpec::ReadOnly,
        barrier: BarrierSpec::None,
        result_policy: ResultPolicySpec::CorrelatedSource,
        schema: Some(EffectSchema {
            intent: record([
                field("endpoint", ValueType::Text),
                field(
                    "method",
                    tags(["Get", "Head", "Post", "Put", "Patch", "Delete", "Options"]),
                ),
                field(
                    "path_segments",
                    ValueType::List {
                        item: Box::new(ValueType::Text),
                    },
                ),
                field(
                    "query",
                    ValueType::List {
                        item: Box::new(text_pair()),
                    },
                ),
                field("headers", headers()),
                field("body", ValueType::Bytes { fixed_len: None }),
                field("connect_timeout_ms", ValueType::Number),
                field("overall_timeout_ms", ValueType::Number),
                field("cancellation", tags(["Independent", "CancelPrevious"])),
            ]),
            result: ValueType::Variant {
                variants: vec![
                    variant(
                        "HttpSucceeded",
                        [
                            field("endpoint", ValueType::Text),
                            field("status", ValueType::Number),
                            field("headers", headers()),
                            field("body", ValueType::Bytes { fixed_len: None }),
                            field("redirects_followed", ValueType::Number),
                        ],
                    ),
                    variant(
                        "HttpFailed",
                        [
                            field("endpoint", ValueType::Text),
                            field("code", ValueType::Text),
                            field("diagnostic", ValueType::Text),
                            field("retryable", ValueType::Bool),
                            field("timed_out", ValueType::Bool),
                            field("cancelled", ValueType::Bool),
                        ],
                    ),
                ],
            },
        }),
    }
}

fn development_simulation() -> ValueType {
    tags(["Success", "Cancel", "Failure", "Duplicate"])
}

fn tags<const N: usize>(values: [&'static str; N]) -> ValueType {
    ValueType::Variant {
        variants: values.into_iter().map(|tag| variant(tag, [])).collect(),
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
            let schema = spec.schema.unwrap();
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
            .schema
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

    #[test]
    fn outbound_http_is_typed_read_only_and_recursive() {
        let spec = host_effect_spec(OUTBOUND_HTTP_REQUEST_OPERATION).unwrap();
        assert_eq!(spec.replay, ReplaySpec::ReadOnly);
        assert_eq!(spec.barrier, BarrierSpec::None);
        assert_eq!(spec.result_policy, ResultPolicySpec::CorrelatedSource);
        let schema = spec.schema.unwrap();
        let ValueType::Record {
            fields,
            open: false,
        } = schema.intent
        else {
            panic!("HTTP intent must be a closed record");
        };
        assert!(fields.iter().any(|field| {
            field.name == "path_segments" && matches!(field.value_type, ValueType::List { .. })
        }));
        assert!(fields.iter().any(|field| {
            field.name == "headers"
                && matches!(
                    &field.value_type,
                    ValueType::List { item }
                        if matches!(item.as_ref(), ValueType::Record { open: false, .. })
                )
        }));
        assert!(matches!(schema.result, ValueType::Variant { .. }));
    }

    #[test]
    fn host_service_effects_are_closed_correlated_contracts() {
        for operation in [
            WALL_CLOCK_READ_OPERATION,
            SECURE_RANDOM_BYTES_OPERATION,
            SECRET_VERIFY_OPERATION,
            HMAC_SHA256_SIGN_OPERATION,
            HMAC_SHA256_VERIFY_OPERATION,
            TIMER_DEADLINE_OPERATION,
        ] {
            let spec = host_effect_spec(operation).unwrap();
            assert_eq!(spec.operation, operation);
            assert_eq!(spec.result_policy, ResultPolicySpec::CorrelatedSource);
            assert_eq!(spec.barrier, BarrierSpec::None);
            let schema = spec.schema.unwrap();
            assert!(matches!(
                schema.intent,
                ValueType::Record { open: false, .. }
            ));
            let ValueType::Variant { variants } = schema.result else {
                panic!("host-service result must be a closed variant");
            };
            assert!(
                variants
                    .iter()
                    .any(|variant| variant.tag == "HostServiceFailed")
            );
        }
    }
}
