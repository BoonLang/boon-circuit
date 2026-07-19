#[test]
fn typed_http_host_port_injects_closed_structural_request_payload() {
    let parsed = boon_parser::parse_source(
        "server-outputs.bn",
        include_str!("../../../../examples/server_outputs.bn"),
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );

    let http = report.host_port_table.http.as_ref().unwrap();
    assert_eq!(http.request_source, "store.request_received");
    assert_eq!(http.disconnect_source, None);
    assert_eq!(http.response_output, "api_response");
    let request = report
        .source_payload_shape_table
        .iter()
        .find(|entry| entry.source_path == http.request_source)
        .unwrap();
    let fields = request
        .fields
        .iter()
        .map(|field| (field.name.as_str(), &field.ty))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(fields.get("method"), Some(&&Type::Text));
    assert!(matches!(fields.get("body"), Some(Type::Bytes(_))));
    assert!(
        matches!(fields.get("path_segments"), Some(Type::List(item)) if item.as_ref() == &Type::Text)
    );
    assert!(
        matches!(fields.get("query"), Some(Type::List(item)) if matches!(item.as_ref(), Type::Object(shape) if !shape.open && shape.fields.contains_key("name") && shape.fields.contains_key("value")))
    );
}

#[test]
fn latest_preserves_merged_source_event_flow_for_one_host_effect() {
    let parsed = boon_parser::parse_source(
        "merged-file-effect.bn",
        r#"
store: [
    open_primary: SOURCE
    open_secondary: SOURCE
    request:
        LATEST {
            open_primary |> THEN { Primary }
            open_secondary |> THEN { Secondary }
        }
    selected:
        PackageAsset[url: TEXT { asset://files/primary.bin }] |> HOLD selected {
            request |> WHEN {
                Primary => PackageAsset[url: TEXT { asset://files/primary.bin }]
                Secondary => PackageAsset[url: TEXT { asset://files/secondary.bin }]
            }
        }
    result:
        NotStarted |> HOLD result {
            request |> THEN {
                File/read_stream(
                    file: selected
                    chunk_bytes: 4096
                    retain_content: True
                )
            }
        }
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "merged source events lost their effect-trigger flow: {:?}",
        report.diagnostics
    );
}

#[test]
fn typed_host_effect_allows_omitting_a_schema_defaulted_argument() {
    let parsed = boon_parser::parse_source(
        "defaulted-file-effect.bn",
        r#"
store: [
    read: SOURCE
    selected: PackageAsset[url: TEXT { asset://files/primary.bin }]
    result:
        NotStarted |> HOLD result {
            read |> THEN {
                File/read_stream(
                    file: selected
                    retain_content: False
                )
            }
        }
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "schema default was not accepted: {:?}",
        report.diagnostics
    );
}

#[test]
fn typed_host_effect_arguments_use_when_variant_narrowing() {
    let parsed = boon_parser::parse_source(
        "narrowed-effect-chain.bn",
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
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "host-effect validation ran before WHEN narrowing: {:?}",
        report.diagnostics
    );
}

#[test]
fn typed_host_effect_checks_inline_and_multiline_arguments_against_one_schema() {
    for (label, call) in [
        (
            "inline",
            "File/read_stream(file: selected, chunk_bytes: False, retain_content: False)",
        ),
        (
            "multiline",
            r#"File/read_stream(
                    file: selected
                    chunk_bytes: False
                    retain_content: False
                )"#,
        ),
    ] {
        let source = format!(
            r#"
store: [
    read: SOURCE
    selected: PackageAsset[url: TEXT {{ asset://files/primary.bin }}]
    result:
        NotStarted |> HOLD result {{
            read |> THEN {{ {call} }}
        }}
]
"#,
        );
        let parsed = boon_parser::parse_source(&format!("wrong-{label}.bn"), &source).unwrap();
        let report = check(&parsed);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.message.contains("argument `chunk_bytes`")
                    && diagnostic.message.contains("expected: NUMBER")
            }),
            "{label} call escaped schema typing: {:?}",
            report.diagnostics
        );
    }

    for missing in ["file", "retain_content"] {
        let arguments = match missing {
            "file" => "retain_content: False",
            "retain_content" => "file: selected",
            _ => unreachable!(),
        };
        let source = format!(
            r#"
store: [
    read: SOURCE
    selected: PackageAsset[url: TEXT {{ asset://files/primary.bin }}]
    result:
        NotStarted |> HOLD result {{
            read |> THEN {{ File/read_stream({arguments}) }}
        }}
]
"#,
        );
        let parsed = boon_parser::parse_source(&format!("missing-{missing}.bn"), &source).unwrap();
        let report = check(&parsed);
        assert!(
            report.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains(&format!("missing required argument `{missing}`"))),
            "missing {missing} was accepted: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn http_host_port_accepts_only_the_closed_bytes_response_contract() {
    let valid_responses = [
        r#"[
            status: 200
            body: BYTES {}
        ]"#,
        r#"[
            status: 200
            headers: LIST {
                [name: TEXT { content-type }, value: TEXT { application/octet-stream }]
            }
            body: BYTES[1] { 16uff }
        ]"#,
        r#"[
            status: 200
            headers: LIST {
                [name: TEXT { x-binary }, value: BYTES[1] { 16u01 }]
            }
            body: TEXT { ok } |> Text/to_bytes(encoding: Utf8)
        ]"#,
    ];
    for (index, response) in valid_responses.into_iter().enumerate() {
        let source = format!(
            r#"
store: [
    request: SOURCE
]
outputs: [
    response: {response}
]
host_ports: [
    http: [
        request: store.request
        response: response
    ]
]
"#
        );
        let parsed = boon_parser::parse_source(&format!("valid-http-{index}.bn"), &source).unwrap();
        let report = check(&parsed);
        assert!(
            !report.has_errors(),
            "valid response {index} produced diagnostics: {:?}",
            report.diagnostics
        );
    }

    let invalid_responses = [
        (
            "text-body",
            r#"[
                status: 200
                body: TEXT { no }
            ]"#,
        ),
        (
            "number-body",
            r#"[
                status: 200
                body: 1
            ]"#,
        ),
        (
            "record-body",
            r#"[
                status: 200
                body: [ok: True]
            ]"#,
        ),
        (
            "malformed-headers",
            r#"[
                status: 200
                headers: LIST {
                    [name: TEXT { x-test }, value: TEXT { yes }, extra: TEXT { no }]
                }
                body: BYTES {}
            ]"#,
        ),
        (
            "extra-response-field",
            r#"[
                status: 200
                body: BYTES {}
                request_count: 1
            ]"#,
        ),
    ];
    for (name, response) in invalid_responses {
        let source = format!(
            r#"
store: [
    request: SOURCE
]
outputs: [
    response: {response}
]
host_ports: [
    http: [
        request: store.request
        response: response
    ]
]
"#
        );
        let parsed =
            boon_parser::parse_source(&format!("invalid-http-{name}.bn"), &source).unwrap();
        let report = check(&parsed);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .message
                    .contains("must be exactly `{ status: Number, body: Bytes }`")
            }),
            "invalid response {name} was not rejected by the HTTP boundary: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn nested_multiline_body_pipeline_keeps_the_enclosing_output_record_type() {
    let parsed = boon_parser::parse_source(
        "nested-http-body-pipeline.bn",
        r#"
store: [
    request: SOURCE
]
outputs: [
    response: [
        status: 200
        headers: LIST {
            [name: TEXT { content-type }, value: TEXT { application/octet-stream }]
        }
        body: response_body(
            text: TEXT { ok }
        )
            |> Text/trim()
            |> Text/to_bytes(encoding: Utf8)
    ]
]
host_ports: [
    http: [
        request: store.request
        response: response
    ]
]

FUNCTION response_body(text) {
    text
}
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "nested body pipeline changed the output root type: {:?}",
        report.diagnostics
    );
    assert!(matches!(
        report
            .output_root_types
            .iter()
            .find(|output| output.name == "response")
            .map(|output| &output.ty),
        Some(Type::Object(shape)) if shape.fields.contains_key("body")
    ));
}

#[test]
fn websocket_host_port_requires_direct_sources_and_the_generic_action_envelope() {
    let parsed = boon_parser::parse_source(
        "websocket-server.bn",
        include_str!("../../../../examples/server_websocket_echo.bn"),
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
    let websocket = report.host_port_table.websocket.as_ref().unwrap();
    assert_eq!(websocket.message_source, "store.ws_message");
    let message = report
        .source_payload_shape_table
        .iter()
        .find(|entry| entry.source_path == websocket.message_source)
        .unwrap();
    assert!(message.fields.iter().any(|field| {
        field.name == "bytes" && matches!(field.ty, Type::Bytes(BytesType::Dynamic))
    }));
}

#[test]
fn websocket_host_port_rejects_an_arbitrary_closed_list() {
    let parsed = boon_parser::parse_source(
        "invalid-websocket-actions.bn",
        r#"
store: [
    ws_open: SOURCE
    ws_message: SOURCE
    ws_close: SOURCE
    ws_error: SOURCE
]
outputs: [
    websocket_actions: List/range(from: 0, to: 0)
]
host_ports: [
    websocket: [
        open: store.ws_open
        message: store.ws_message
        close: store.ws_close
        error: store.ws_error
        actions: websocket_actions
    ]
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("closed generic WebSocket action envelopes")
    }));
}

#[test]
fn host_port_rejects_missing_or_non_output_response_reference() {
    let parsed = boon_parser::parse_source(
        "invalid-http-port.bn",
        r#"
store: [
    request: SOURCE
    not_an_output: TEXT { no }
]
outputs: [
    response: [status: 200, body: BYTES {}]
]
host_ports: [
    http: [
        request: store.request
        response: store.not_an_output
    ]
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("must reference one named root from top-level `outputs`")
    }));
}
