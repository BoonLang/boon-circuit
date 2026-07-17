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
    Discarded,
}

pub const MAX_STREAM_INITIAL_CREDITS: u32 = 256;
pub const MAX_STREAM_IN_FLIGHT: u32 = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryCardinalitySpec {
    Single,
    Stream {
        initial_credits: u32,
        max_in_flight: u32,
        terminal_result_tags: Vec<&'static str>,
    },
}

impl Default for DeliveryCardinalitySpec {
    fn default() -> Self {
        Self::Single
    }
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
    pub intent_constraints: Vec<IntentConstraintSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntentConstraintSpec {
    UnsignedIntegerRange {
        field_path: Vec<&'static str>,
        min_inclusive: u64,
        max_inclusive: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEffectSpec {
    pub operation: &'static str,
    pub replay: ReplaySpec,
    pub barrier: BarrierSpec,
    pub result_policy: ResultPolicySpec,
    pub delivery: DeliveryCardinalitySpec,
    pub schema: Option<EffectSchema>,
}

pub const OUTBOUND_HTTP_REQUEST_OPERATION: &str = "Http/request";
pub const FILE_READ_STREAM_OPERATION: &str = "File/read_stream";
pub const FILE_STREAM_MIN_CHUNK_BYTES: u64 = 1;
pub const FILE_STREAM_MAX_CHUNK_BYTES: u64 = 1024 * 1024;
pub const FILE_STREAM_INITIAL_CREDITS: u32 = 2;
pub const FILE_STREAM_MAX_IN_FLIGHT: u32 = 4;
pub const WALL_CLOCK_READ_OPERATION: &str = "Clock/wall";
pub const SECURE_RANDOM_BYTES_OPERATION: &str = "Random/bytes";
pub const SECRET_VERIFY_OPERATION: &str = "Secret/verify";
pub const HMAC_SHA256_SIGN_OPERATION: &str = "Crypto/hmac_sha256_sign";
pub const HMAC_SHA256_VERIFY_OPERATION: &str = "Crypto/hmac_sha256_verify";
pub const TIMER_DEADLINE_OPERATION: &str = "Timer/deadline";
pub const WELLEN_OPEN_OPERATION: &str = "Wellen/open";
pub const WELLEN_HIERARCHY_PAGE_OPERATION: &str = "Wellen/hierarchy_page";
pub const WELLEN_SIGNAL_PAGE_OPERATION: &str = "Wellen/signal_page";
pub const WELLEN_CURSOR_VALUES_OPERATION: &str = "Wellen/cursor_values";
pub const WELLEN_BRIDGE_SCHEMA_VERSION: &str = "wellen.v1";
pub const WELLEN_MAX_HIERARCHY_ROWS: u64 = 256;
pub const WELLEN_MAX_SIGNAL_TRANSITIONS: u64 = 256;
pub const WELLEN_MAX_CURSOR_SIGNALS: usize = 32;
pub const WELLEN_MAX_SAFE_TIME: u64 = (1_u64 << 53) - 1;

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
            delivery: DeliveryCardinalitySpec::Single,
            schema: None,
        });
    }
    match operation {
        "File/write_bytes" => Some(HostEffectSpec {
            operation: "File/write_bytes",
            replay: ReplaySpec::IdempotentBytesKey,
            barrier: BarrierSpec::BeforeAndAfter,
            result_policy: ResultPolicySpec::Acknowledgement,
            delivery: DeliveryCardinalitySpec::Single,
            schema: Some(EffectSchema {
                intent: record([
                    field("bytes", ValueType::Bytes { fixed_len: None }),
                    field("path", ValueType::Text),
                ]),
                result: ValueType::Text,
                intent_constraints: Vec::new(),
            }),
        }),
        FILE_READ_STREAM_OPERATION => Some(file_read_stream()),
        "DevelopmentPasskey/register" => Some(development_passkey_registration()),
        "DevelopmentPasskey/authenticate" => Some(development_passkey_authentication()),
        OUTBOUND_HTTP_REQUEST_OPERATION => Some(outbound_http_request()),
        WALL_CLOCK_READ_OPERATION => Some(wall_clock_read()),
        SECURE_RANDOM_BYTES_OPERATION => Some(secure_random_bytes()),
        SECRET_VERIFY_OPERATION => Some(secret_verify()),
        HMAC_SHA256_SIGN_OPERATION => Some(hmac_sha256_sign()),
        HMAC_SHA256_VERIFY_OPERATION => Some(hmac_sha256_verify()),
        TIMER_DEADLINE_OPERATION => Some(timer_deadline()),
        WELLEN_OPEN_OPERATION => Some(wellen_open()),
        WELLEN_HIERARCHY_PAGE_OPERATION => Some(wellen_hierarchy_page()),
        WELLEN_SIGNAL_PAGE_OPERATION => Some(wellen_signal_page()),
        WELLEN_CURSOR_VALUES_OPERATION => Some(wellen_cursor_values()),
        _ => None,
    }
}

fn file_selection_type() -> ValueType {
    ValueType::Variant {
        variants: vec![
            variant(
                "FileSelected",
                [field(
                    "capability",
                    record([
                        field(
                            "token",
                            ValueType::Bytes {
                                fixed_len: Some(32),
                            },
                        ),
                        field("generation", ValueType::Number),
                    ]),
                )],
            ),
            variant("PackageAsset", [field("url", ValueType::Text)]),
        ],
    }
}

fn waveform_artifact_type() -> ValueType {
    record([
        field("content", content_ref_type()),
        field("format", ValueType::Text),
        field("schema_version", ValueType::Text),
        field("parser_version", ValueType::Text),
    ])
}

fn content_ref_type() -> ValueType {
    record([
        field(
            "digest",
            ValueType::Bytes {
                fixed_len: Some(32),
            },
        ),
        field("byte_count", ValueType::Number),
    ])
}

fn waveform_failure_variant() -> Variant {
    variant(
        "WaveformFailed",
        [
            field("code", ValueType::Text),
            field("diagnostic", ValueType::Text),
        ],
    )
}

fn waveform_value_type() -> ValueType {
    ValueType::Variant {
        variants: vec![
            variant("BinaryValue", [field("bits", ValueType::Text)]),
            variant("FourStateValue", [field("bits", ValueType::Text)]),
            variant("NineStateValue", [field("bits", ValueType::Text)]),
            variant("StringValue", [field("text", ValueType::Text)]),
            variant("RealValue", [field("value", ValueType::Number)]),
            variant("NonFiniteReal", [field("classification", ValueType::Text)]),
            variant("UnavailableValue", []),
        ],
    }
}

fn wellen_open() -> HostEffectSpec {
    transient_host_service(
        WELLEN_OPEN_OPERATION,
        ReplaySpec::ReadOnly,
        record([field("content", content_ref_type())]),
        ValueType::Variant {
            variants: vec![
                variant(
                    "WaveformOpened",
                    [
                        field("artifact", waveform_artifact_type()),
                        field("format", ValueType::Text),
                        field("byte_length", ValueType::Number),
                        field("start_time", ValueType::Number),
                        field("end_time", ValueType::Number),
                        field("timescale_factor", ValueType::Number),
                        field("timescale_unit", ValueType::Text),
                        field("scope_count", ValueType::Number),
                        field("signal_count", ValueType::Number),
                        field("hierarchy_bytes", ValueType::Number),
                        field("provider", ValueType::Text),
                    ],
                ),
                waveform_failure_variant(),
            ],
        },
    )
}

fn wellen_hierarchy_page() -> HostEffectSpec {
    HostEffectSpec {
        operation: WELLEN_HIERARCHY_PAGE_OPERATION,
        replay: ReplaySpec::ReadOnly,
        barrier: BarrierSpec::None,
        result_policy: ResultPolicySpec::ReturnValue,
        delivery: DeliveryCardinalitySpec::Single,
        schema: Some(EffectSchema {
            intent: record([
                field("artifact", waveform_artifact_type()),
                field("request_fingerprint", ValueType::Text),
                field("offset", ValueType::Number),
                field("limit", ValueType::Number),
            ]),
            result: ValueType::Variant {
                variants: vec![
                    variant(
                        "HierarchyPage",
                        [
                            field("artifact", waveform_artifact_type()),
                            field("request_fingerprint", ValueType::Text),
                            field("start_time", ValueType::Number),
                            field("end_time", ValueType::Number),
                            field("offset", ValueType::Number),
                            field("has_more", ValueType::Bool),
                            field("next_offset", ValueType::Number),
                            field("total_rows", ValueType::Number),
                            field(
                                "signal_ids",
                                ValueType::List {
                                    item: Box::new(ValueType::Text),
                                },
                            ),
                            field(
                                "rows",
                                ValueType::List {
                                    item: Box::new(record([
                                        field("kind", ValueType::Text),
                                        field("id", ValueType::Text),
                                        field("parent_id", ValueType::Text),
                                        field("name", ValueType::Text),
                                        field("signal_id", ValueType::Text),
                                        field("width", ValueType::Number),
                                        field("encoding", ValueType::Text),
                                    ])),
                                },
                            ),
                        ],
                    ),
                    waveform_failure_variant(),
                ],
            },
            intent_constraints: vec![
                IntentConstraintSpec::UnsignedIntegerRange {
                    field_path: vec!["limit"],
                    min_inclusive: 1,
                    max_inclusive: WELLEN_MAX_HIERARCHY_ROWS,
                },
                IntentConstraintSpec::UnsignedIntegerRange {
                    field_path: vec!["offset"],
                    min_inclusive: 0,
                    max_inclusive: WELLEN_MAX_SAFE_TIME,
                },
            ],
        }),
    }
}

fn wellen_signal_page() -> HostEffectSpec {
    HostEffectSpec {
        operation: WELLEN_SIGNAL_PAGE_OPERATION,
        replay: ReplaySpec::ReadOnly,
        barrier: BarrierSpec::None,
        result_policy: ResultPolicySpec::ReturnValue,
        delivery: DeliveryCardinalitySpec::Single,
        schema: Some(EffectSchema {
            intent: record([
                field("artifact", waveform_artifact_type()),
                field("request_fingerprint", ValueType::Text),
                field(
                    "signal_ids",
                    ValueType::List {
                        item: Box::new(ValueType::Text),
                    },
                ),
                field("start_time", ValueType::Number),
                field("end_time", ValueType::Number),
                field("offset", ValueType::Number),
                field("max_transitions", ValueType::Number),
            ]),
            result: ValueType::Variant {
                variants: vec![
                    variant(
                        "SignalPage",
                        [
                            field("artifact", waveform_artifact_type()),
                            field("request_fingerprint", ValueType::Text),
                            field(
                                "signal_ids",
                                ValueType::List {
                                    item: Box::new(ValueType::Text),
                                },
                            ),
                            field("start_time", ValueType::Number),
                            field("end_time", ValueType::Number),
                            field("offset", ValueType::Number),
                            field("has_more", ValueType::Bool),
                            field("next_offset", ValueType::Number),
                            field(
                                "signals",
                                ValueType::List {
                                    item: Box::new(record([
                                        field("signal_id", ValueType::Text),
                                        field(
                                            "transitions",
                                            ValueType::List {
                                                item: Box::new(record([
                                                    field("time", ValueType::Number),
                                                    field("value", waveform_value_type()),
                                                ])),
                                            },
                                        ),
                                    ])),
                                },
                            ),
                        ],
                    ),
                    waveform_failure_variant(),
                ],
            },
            intent_constraints: vec![
                IntentConstraintSpec::UnsignedIntegerRange {
                    field_path: vec!["end_time"],
                    min_inclusive: 0,
                    max_inclusive: WELLEN_MAX_SAFE_TIME,
                },
                IntentConstraintSpec::UnsignedIntegerRange {
                    field_path: vec!["max_transitions"],
                    min_inclusive: 1,
                    max_inclusive: WELLEN_MAX_SIGNAL_TRANSITIONS,
                },
                IntentConstraintSpec::UnsignedIntegerRange {
                    field_path: vec!["offset"],
                    min_inclusive: 0,
                    max_inclusive: WELLEN_MAX_SAFE_TIME,
                },
                IntentConstraintSpec::UnsignedIntegerRange {
                    field_path: vec!["start_time"],
                    min_inclusive: 0,
                    max_inclusive: WELLEN_MAX_SAFE_TIME,
                },
            ],
        }),
    }
}

fn wellen_cursor_values() -> HostEffectSpec {
    HostEffectSpec {
        operation: WELLEN_CURSOR_VALUES_OPERATION,
        replay: ReplaySpec::ReadOnly,
        barrier: BarrierSpec::None,
        result_policy: ResultPolicySpec::ReturnValue,
        delivery: DeliveryCardinalitySpec::Single,
        schema: Some(EffectSchema {
            intent: record([
                field("artifact", waveform_artifact_type()),
                field("request_fingerprint", ValueType::Text),
                field("cursor_time", ValueType::Number),
                field(
                    "signal_ids",
                    ValueType::List {
                        item: Box::new(ValueType::Text),
                    },
                ),
            ]),
            result: ValueType::Variant {
                variants: vec![
                    variant(
                        "CursorValues",
                        [
                            field("artifact", waveform_artifact_type()),
                            field("request_fingerprint", ValueType::Text),
                            field("cursor_time", ValueType::Number),
                            field(
                                "rows",
                                ValueType::List {
                                    item: Box::new(record([
                                        field("signal_id", ValueType::Text),
                                        field("value", waveform_value_type()),
                                    ])),
                                },
                            ),
                        ],
                    ),
                    waveform_failure_variant(),
                ],
            },
            intent_constraints: vec![IntentConstraintSpec::UnsignedIntegerRange {
                field_path: vec!["cursor_time"],
                min_inclusive: 0,
                max_inclusive: WELLEN_MAX_SAFE_TIME,
            }],
        }),
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
        result_policy: ResultPolicySpec::ReturnValue,
        delivery: DeliveryCardinalitySpec::Single,
        schema: Some(EffectSchema {
            intent,
            result,
            intent_constraints: Vec::new(),
        }),
    }
}

fn file_read_stream() -> HostEffectSpec {
    HostEffectSpec {
        operation: FILE_READ_STREAM_OPERATION,
        replay: ReplaySpec::ReadOnly,
        barrier: BarrierSpec::None,
        result_policy: ResultPolicySpec::ReturnValue,
        delivery: DeliveryCardinalitySpec::Stream {
            initial_credits: FILE_STREAM_INITIAL_CREDITS,
            max_in_flight: FILE_STREAM_MAX_IN_FLIGHT,
            terminal_result_tags: vec!["Cancelled", "Failed", "Finished"],
        },
        schema: Some(EffectSchema {
            intent: record([
                field("file", file_selection_type()),
                field("chunk_bytes", ValueType::Number),
                field("retain_content", ValueType::Bool),
            ]),
            result: ValueType::Variant {
                variants: vec![
                    variant(
                        "Opened",
                        [
                            field("size", ValueType::Number),
                            field("content_type", ValueType::Text),
                            field("display_name", ValueType::Text),
                        ],
                    ),
                    variant(
                        "Chunk",
                        [
                            field("sequence", ValueType::Number),
                            field("offset", ValueType::Number),
                            field("bytes", ValueType::Bytes { fixed_len: None }),
                        ],
                    ),
                    variant(
                        "Finished",
                        [
                            field("byte_count", ValueType::Number),
                            field(
                                "digest",
                                ValueType::Bytes {
                                    fixed_len: Some(32),
                                },
                            ),
                            field("content", content_ref_type()),
                        ],
                    ),
                    variant(
                        "Failed",
                        [
                            field("code", ValueType::Text),
                            field("diagnostic", ValueType::Text),
                        ],
                    ),
                    variant("Cancelled", []),
                ],
            },
            intent_constraints: vec![IntentConstraintSpec::UnsignedIntegerRange {
                field_path: vec!["chunk_bytes"],
                min_inclusive: FILE_STREAM_MIN_CHUNK_BYTES,
                max_inclusive: FILE_STREAM_MAX_CHUNK_BYTES,
            }],
        }),
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
        result_policy: ResultPolicySpec::ReturnValue,
        delivery: DeliveryCardinalitySpec::Single,
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
            intent_constraints: Vec::new(),
        }),
    }
}

fn development_passkey_authentication() -> HostEffectSpec {
    HostEffectSpec {
        operation: "DevelopmentPasskey/authenticate",
        replay: ReplaySpec::IdempotentBytesKey,
        barrier: BarrierSpec::BeforeAndAfter,
        result_policy: ResultPolicySpec::ReturnValue,
        delivery: DeliveryCardinalitySpec::Single,
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
            intent_constraints: Vec::new(),
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
        result_policy: ResultPolicySpec::ReturnValue,
        delivery: DeliveryCardinalitySpec::Single,
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
            intent_constraints: Vec::new(),
        }),
    }
}

impl HostEffectSpec {
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(schema) = &self.schema {
            validate_intent_constraints(schema)?;
        }
        let DeliveryCardinalitySpec::Stream {
            initial_credits,
            max_in_flight,
            terminal_result_tags,
        } = &self.delivery
        else {
            return Ok(());
        };
        if self.replay != ReplaySpec::ReadOnly
            || self.barrier != BarrierSpec::None
            || self.result_policy != ResultPolicySpec::ReturnValue
        {
            return Err("stream effects must be read-only, barrier-free return-value effects");
        }
        if *initial_credits == 0
            || *initial_credits > MAX_STREAM_INITIAL_CREDITS
            || *max_in_flight == 0
            || *max_in_flight > MAX_STREAM_IN_FLIGHT
            || initial_credits > max_in_flight
        {
            return Err("stream effect credit limits must be nonzero, bounded, and ordered");
        }
        if terminal_result_tags.is_empty()
            || terminal_result_tags
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("stream terminal result tags must be nonempty, unique, and ordered");
        }
        let Some(EffectSchema {
            result: ValueType::Variant { variants },
            ..
        }) = &self.schema
        else {
            return Err("stream effects require a closed variant result schema");
        };
        if terminal_result_tags
            .iter()
            .any(|terminal| !variants.iter().any(|variant| variant.tag == *terminal))
        {
            return Err("stream terminal result tags must exist in the result schema");
        }
        if terminal_result_tags.len() == variants.len() {
            return Err("stream effects require at least one nonterminal result variant");
        }
        Ok(())
    }
}

fn validate_intent_constraints(schema: &EffectSchema) -> Result<(), &'static str> {
    let mut previous_path: Option<&[&str]> = None;
    for constraint in &schema.intent_constraints {
        let IntentConstraintSpec::UnsignedIntegerRange {
            field_path,
            min_inclusive,
            max_inclusive,
        } = constraint;
        if field_path.is_empty() || min_inclusive > max_inclusive {
            return Err("effect intent constraints must have a valid field path and range");
        }
        if previous_path.is_some_and(|previous| previous >= field_path.as_slice()) {
            return Err("effect intent constraints must be uniquely ordered by field path");
        }
        previous_path = Some(field_path);
        if !matches!(
            value_type_at_path(&schema.intent, field_path),
            Some(ValueType::Number)
        ) {
            return Err("unsigned integer constraints must target numeric intent fields");
        }
    }
    Ok(())
}

fn value_type_at_path<'a>(root: &'a ValueType, field_path: &[&str]) -> Option<&'a ValueType> {
    field_path.iter().try_fold(root, |value_type, part| {
        let ValueType::Record {
            fields,
            open: false,
        } = value_type
        else {
            return None;
        };
        fields
            .iter()
            .find(|field| field.name == *part)
            .map(|field| &field.value_type)
    })
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
            assert_eq!(spec.result_policy, ResultPolicySpec::ReturnValue);
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
        assert_eq!(spec.result_policy, ResultPolicySpec::ReturnValue);
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
    fn host_service_effects_are_closed_return_value_contracts() {
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
            assert_eq!(spec.result_policy, ResultPolicySpec::ReturnValue);
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

    #[test]
    fn wellen_bridge_is_typed_read_only_single_delivery_and_bounded() {
        for operation in [
            WELLEN_OPEN_OPERATION,
            WELLEN_HIERARCHY_PAGE_OPERATION,
            WELLEN_SIGNAL_PAGE_OPERATION,
            WELLEN_CURSOR_VALUES_OPERATION,
        ] {
            let spec = host_effect_spec(operation).unwrap();
            assert_eq!(spec.operation, operation);
            assert_eq!(spec.replay, ReplaySpec::ReadOnly);
            assert_eq!(spec.barrier, BarrierSpec::None);
            assert_eq!(spec.result_policy, ResultPolicySpec::ReturnValue);
            assert_eq!(spec.delivery, DeliveryCardinalitySpec::Single);
            assert_eq!(spec.validate(), Ok(()));
            let schema = spec.schema.unwrap();
            assert!(matches!(
                schema.intent,
                ValueType::Record { open: false, .. }
            ));
            let ValueType::Variant { variants } = schema.result else {
                panic!("Wellen result must be a closed variant");
            };
            assert_eq!(variants.last().unwrap().tag, "WaveformFailed");
        }

        let hierarchy = host_effect_spec(WELLEN_HIERARCHY_PAGE_OPERATION)
            .unwrap()
            .schema
            .unwrap();
        assert!(hierarchy.intent_constraints.iter().any(|constraint| {
            matches!(
                constraint,
                IntentConstraintSpec::UnsignedIntegerRange {
                    field_path,
                    max_inclusive: WELLEN_MAX_HIERARCHY_ROWS,
                    ..
                } if field_path == &["limit"]
            )
        }));
        let signal = host_effect_spec(WELLEN_SIGNAL_PAGE_OPERATION)
            .unwrap()
            .schema
            .unwrap();
        assert!(signal.intent_constraints.iter().any(|constraint| {
            matches!(
                constraint,
                IntentConstraintSpec::UnsignedIntegerRange {
                    field_path,
                    max_inclusive: WELLEN_MAX_SIGNAL_TRANSITIONS,
                    ..
                } if field_path == &["max_transitions"]
            )
        }));
    }

    #[test]
    fn existing_effects_remain_single_delivery() {
        for operation in [
            "Directory/entries",
            "File/read_bytes",
            "File/read_text",
            "File/write_bytes",
            "File/write_text",
            "Log/error",
            "Log/info",
            "DevelopmentPasskey/register",
            "DevelopmentPasskey/authenticate",
            OUTBOUND_HTTP_REQUEST_OPERATION,
            WALL_CLOCK_READ_OPERATION,
            SECURE_RANDOM_BYTES_OPERATION,
            SECRET_VERIFY_OPERATION,
            HMAC_SHA256_SIGN_OPERATION,
            HMAC_SHA256_VERIFY_OPERATION,
            TIMER_DEADLINE_OPERATION,
            WELLEN_OPEN_OPERATION,
            WELLEN_HIERARCHY_PAGE_OPERATION,
            WELLEN_SIGNAL_PAGE_OPERATION,
            WELLEN_CURSOR_VALUES_OPERATION,
        ] {
            let spec = host_effect_spec(operation).unwrap();
            assert_eq!(spec.delivery, DeliveryCardinalitySpec::Single);
            assert_eq!(spec.validate(), Ok(()));
        }
    }

    #[test]
    fn file_read_stream_is_structural_bounded_and_closed() {
        let spec = host_effect_spec(FILE_READ_STREAM_OPERATION).unwrap();
        assert_eq!(spec.replay, ReplaySpec::ReadOnly);
        assert_eq!(spec.barrier, BarrierSpec::None);
        assert_eq!(spec.result_policy, ResultPolicySpec::ReturnValue);
        assert_eq!(spec.validate(), Ok(()));
        assert_eq!(
            spec.delivery,
            DeliveryCardinalitySpec::Stream {
                initial_credits: FILE_STREAM_INITIAL_CREDITS,
                max_in_flight: FILE_STREAM_MAX_IN_FLIGHT,
                terminal_result_tags: vec!["Cancelled", "Failed", "Finished"],
            }
        );

        let schema = spec.schema.unwrap();
        assert_eq!(
            schema.intent_constraints,
            vec![IntentConstraintSpec::UnsignedIntegerRange {
                field_path: vec!["chunk_bytes"],
                min_inclusive: FILE_STREAM_MIN_CHUNK_BYTES,
                max_inclusive: FILE_STREAM_MAX_CHUNK_BYTES,
            }]
        );
        let ValueType::Record {
            fields,
            open: false,
        } = &schema.intent
        else {
            panic!("stream intent must be a closed record");
        };
        let file = fields.iter().find(|field| field.name == "file").unwrap();
        let ValueType::Variant { variants } = &file.value_type else {
            panic!("file intent must be a structural selection tag");
        };
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].tag, "FileSelected");
        assert_eq!(variants[1].tag, "PackageAsset");
        assert_eq!(variants[1].fields[0].name, "url");
        let capability = variants[0]
            .fields
            .iter()
            .find(|field| field.name == "capability")
            .unwrap();
        assert!(matches!(
            &capability.value_type,
            ValueType::Record { fields, open: false }
                if fields.iter().any(|field| {
                    field.name == "token"
                        && field.value_type == ValueType::Bytes { fixed_len: Some(32) }
                })
        ));
        let ValueType::Variant { variants } = schema.result else {
            panic!("stream result must be a closed variant");
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.tag)
                .collect::<Vec<_>>(),
            ["Opened", "Chunk", "Finished", "Failed", "Cancelled"]
        );
        let finished = variants
            .iter()
            .find(|variant| variant.tag == "Finished")
            .unwrap();
        assert!(finished.fields.iter().any(|field| {
            field.name == "digest"
                && field.value_type
                    == ValueType::Bytes {
                        fixed_len: Some(32),
                    }
        }));
    }

    #[test]
    fn stream_validation_rejects_unsafe_delivery_contracts() {
        let valid = host_effect_spec(FILE_READ_STREAM_OPERATION).unwrap();

        let mut zero_credit = valid.clone();
        zero_credit.delivery = DeliveryCardinalitySpec::Stream {
            initial_credits: 0,
            max_in_flight: 1,
            terminal_result_tags: vec!["Cancelled", "Failed", "Finished"],
        };
        assert!(zero_credit.validate().is_err());

        let mut unbounded = valid.clone();
        unbounded.delivery = DeliveryCardinalitySpec::Stream {
            initial_credits: 1,
            max_in_flight: MAX_STREAM_IN_FLIGHT + 1,
            terminal_result_tags: vec!["Cancelled", "Failed", "Finished"],
        };
        assert!(unbounded.validate().is_err());

        let mut consequential = valid.clone();
        consequential.replay = ReplaySpec::NonReplayable;
        assert!(consequential.validate().is_err());

        let mut barrier = valid.clone();
        barrier.barrier = BarrierSpec::Before;
        assert!(barrier.validate().is_err());

        let mut acknowledgement = valid.clone();
        acknowledgement.result_policy = ResultPolicySpec::Acknowledgement;
        assert!(acknowledgement.validate().is_err());

        let mut unknown_terminal = valid;
        unknown_terminal.delivery = DeliveryCardinalitySpec::Stream {
            initial_credits: 1,
            max_in_flight: 1,
            terminal_result_tags: vec!["Missing"],
        };
        assert!(unknown_terminal.validate().is_err());
    }
}
