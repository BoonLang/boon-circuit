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
    assert!(matches!(fields.get("path_segments"), Some(Type::List(item)) if item.as_ref() == &Type::Text));
    assert!(matches!(fields.get("query"), Some(Type::List(item)) if matches!(item.as_ref(), Type::Object(shape) if !shape.open && shape.fields.contains_key("name") && shape.fields.contains_key("value"))));
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
    response: [status: 200, body: TEXT { ok }]
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
