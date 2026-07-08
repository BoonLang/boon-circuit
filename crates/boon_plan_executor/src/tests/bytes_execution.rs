// Included by `../tests.rs`; kept in the parent test module for private executor-helper access.

#[test]
fn source_guard_matching_is_executor_owned() {
    let guard = Some(PlanSourceGuard::SourcePayloadOneOf {
        source_id: SourceId(4),
        field: SourcePayloadField::Key,
        values: vec!["Enter".to_owned(), "NumpadEnter".to_owned()],
    });
    let matching_event = RootJsonSourceEvent {
        key: Some("Enter".to_owned()),
        ..RootJsonSourceEvent::default()
    };
    assert!(
        source_guard_matches(&guard, SourceId(4), &matching_event)
            .expect("matching guard should evaluate")
    );

    let non_matching_event = RootJsonSourceEvent {
        key: Some("Escape".to_owned()),
        ..RootJsonSourceEvent::default()
    };
    assert!(
        !source_guard_matches(&guard, SourceId(4), &non_matching_event)
            .expect("non-matching guard should evaluate")
    );

    let wrong_source = source_guard_matches(&guard, SourceId(9), &matching_event)
        .expect_err("guard source mismatch should be rejected");
    assert!(
        wrong_source
            .to_string()
            .contains("source guard targets source 4"),
        "unexpected error: {wrong_source}"
    );

    let bytes_guard = Some(PlanSourceGuard::SourcePayloadOneOf {
        source_id: SourceId(4),
        field: SourcePayloadField::Bytes,
        values: vec!["00".to_owned(), "de ad be ef".to_owned()],
    });
    let bytes_event = RootJsonSourceEvent {
        payload_bytes: BTreeMap::from([("bytes".to_owned(), vec![0xde, 0xad, 0xbe, 0xef])]),
        ..RootJsonSourceEvent::default()
    };
    assert!(
        source_guard_matches(&bytes_guard, SourceId(4), &bytes_event)
            .expect("matching BYTES guard should evaluate")
    );
    let non_matching_bytes_event = RootJsonSourceEvent {
        payload_bytes: BTreeMap::from([("bytes".to_owned(), vec![0xca, 0xfe])]),
        ..RootJsonSourceEvent::default()
    };
    assert!(
        !source_guard_matches(&bytes_guard, SourceId(4), &non_matching_bytes_event)
            .expect("non-matching BYTES guard should evaluate")
    );
    let invalid_bytes_guard = Some(PlanSourceGuard::SourcePayloadOneOf {
        source_id: SourceId(4),
        field: SourcePayloadField::Bytes,
        values: vec!["not hex".to_owned()],
    });
    let invalid_bytes_error = source_guard_matches(&invalid_bytes_guard, SourceId(4), &bytes_event)
        .expect_err("invalid BYTES guard hex should be rejected");
    assert!(
        invalid_bytes_error.to_string().contains("invalid hex"),
        "unexpected error: {invalid_bytes_error}"
    );
}


#[test]
fn source_payload_bytes_key_policy_is_executor_owned() {
    assert_eq!(source_payload_bytes_toml_key("bytes"), "bytes_hex");
    assert_eq!(source_payload_bytes_toml_key("image"), "image_bytes_hex");
    validate_source_payload_bytes_field_name("bytes")
        .expect("reserved bytes field should be accepted");
    let error = validate_source_payload_bytes_field_name("image")
        .expect_err("named BYTES payload fields should be rejected in v1");
    assert!(
        error.to_string().contains("named BYTES source payload key"),
        "unexpected error: {error}"
    );
}
