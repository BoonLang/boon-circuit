#[test]
fn host_ports_lower_as_explicit_metadata_not_runtime_fields() {
    let parsed = boon_parser::parse_source(
        "server-outputs.bn",
        include_str!("../../../../examples/server_outputs.bn"),
    )
    .unwrap();
    let typed = lower(&parsed).unwrap();
    let [HostPortDeclaration::HttpServer {
        request_source,
        disconnect_source,
        response_output,
        ..
    }] = typed.host_ports.as_slice()
    else {
        panic!("expected one HTTP host port");
    };
    assert_eq!(request_source, "store.request_received");
    assert_eq!(disconnect_source, &None);
    assert_eq!(response_output, "api_response");
    assert!(typed
        .derived_values
        .iter()
        .all(|value| !value.path.starts_with("host_ports.")));
    assert!(typed
        .semantic_index
        .fields
        .iter()
        .all(|field| !field.path.starts_with("host_ports.")));
}
