use boon_web_host::*;

fn limits(
    max_key_bytes: usize,
    max_value_bytes: usize,
    max_entries: u32,
) -> BrowserPreferenceNamespaceLimits {
    BrowserPreferenceNamespaceLimits::new(max_key_bytes, max_value_bytes, max_entries).unwrap()
}

fn namespace(
    id: &str,
    kind: BrowserPreferenceValueKind,
    max_value_bytes: usize,
) -> BrowserPreferenceNamespace {
    BrowserPreferenceNamespace::new(id, kind, limits(32, max_value_bytes, 8)).unwrap()
}

#[test]
fn config_is_explicitly_versioned_unique_and_canonical() {
    let text = namespace("textual", BrowserPreferenceValueKind::Text, 32);
    let binary = namespace("binary", BrowserPreferenceValueKind::Bytes, 32);
    let config = BrowserPreferenceStorageConfig::new(
        "org.example.preferences",
        [text.clone(), binary.clone()],
    )
    .unwrap();

    assert_eq!(
        config.schema_version,
        BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION
    );
    assert_eq!(
        config
            .namespaces
            .iter()
            .map(|namespace| namespace.id().as_str())
            .collect::<Vec<_>>(),
        ["binary", "textual"]
    );
    assert_eq!(
        config
            .namespace(&BrowserPreferenceNamespaceId::new("textual").unwrap())
            .unwrap()
            .value_kind(),
        BrowserPreferenceValueKind::Text
    );

    let mut wrong_version = config.clone();
    wrong_version.schema_version += 1;
    assert!(matches!(
        wrong_version.validate(),
        Err(BrowserPreferenceStorageError::SchemaMismatch {
            expected_version: BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION,
            actual_version: Some(2),
            ..
        })
    ));

    let mut duplicate = config;
    duplicate.namespaces.push(binary);
    assert!(matches!(
        duplicate.validate(),
        Err(BrowserPreferenceStorageError::InvalidInput { field, .. })
            if field == "namespaces"
    ));
}

#[test]
fn identifiers_keys_values_and_namespace_counts_are_bounded() {
    assert!(
        BrowserPreferenceStorageConfig::new(
            "bad/name",
            [namespace("valid", BrowserPreferenceValueKind::Bytes, 1)]
        )
        .is_err()
    );
    assert!(
        BrowserPreferenceStorageConfig::new(
            "example.empty",
            std::iter::empty::<BrowserPreferenceNamespace>()
        )
        .is_err()
    );
    let too_many_namespaces = (0..=MAX_BROWSER_PREFERENCE_NAMESPACES)
        .map(|index| {
            namespace(
                &format!("namespace-{index}"),
                BrowserPreferenceValueKind::Bytes,
                1,
            )
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        BrowserPreferenceStorageConfig::new("example.many", too_many_namespaces),
        Err(BrowserPreferenceStorageError::LimitExceeded {
            resource,
            limit: MAX_BROWSER_PREFERENCE_NAMESPACES
        }) if resource == "namespace count"
    ));

    assert!(BrowserPreferenceNamespaceId::new("bad/name").is_err());
    assert!(BrowserPreferenceNamespaceId::new("").is_err());
    assert!(BrowserPreferenceNamespaceLimits::new(0, 1, 1).is_err());
    assert!(BrowserPreferenceNamespaceLimits::new(1, 0, 1).is_err());
    assert!(BrowserPreferenceNamespaceLimits::new(1, 1, 0).is_err());
    assert!(
        BrowserPreferenceNamespaceLimits::new(MAX_BROWSER_PREFERENCE_KEY_BYTES + 1, 1, 1).is_err()
    );
    assert!(
        BrowserPreferenceNamespaceLimits::new(1, MAX_BROWSER_PREFERENCE_VALUE_BYTES + 1, 1)
            .is_err()
    );
    assert!(
        BrowserPreferenceNamespaceLimits::new(
            1,
            1,
            MAX_BROWSER_PREFERENCE_ENTRIES_PER_NAMESPACE + 1
        )
        .is_err()
    );

    assert!(BrowserPreferenceKey::new("").is_err());
    assert!(BrowserPreferenceKey::new("line\nbreak").is_err());
    assert!(BrowserPreferenceKey::new("x".repeat(MAX_BROWSER_PREFERENCE_KEY_BYTES + 1)).is_err());

    let declaration =
        BrowserPreferenceNamespace::new("small", BrowserPreferenceValueKind::Text, limits(8, 4, 2))
            .unwrap();
    let config = BrowserPreferenceStorageConfig::new("example.small", [declaration]).unwrap();
    let small = BrowserPreferenceNamespaceId::new("small").unwrap();
    let unknown = BrowserPreferenceNamespaceId::new("unknown").unwrap();

    assert!(
        config
            .validate_key(&small, &BrowserPreferenceKey::new("123456789").unwrap())
            .is_err()
    );
    assert!(matches!(
        config.validate_entry(
            &small,
            &BrowserPreferenceKey::new("key").unwrap(),
            &BrowserPreferenceValue::Bytes(vec![1])
        ),
        Err(BrowserPreferenceStorageError::ValueKindMismatch { .. })
    ));
    assert!(matches!(
        config.validate_entry(
            &small,
            &BrowserPreferenceKey::new("key").unwrap(),
            &BrowserPreferenceValue::Text("12345".to_owned())
        ),
        Err(BrowserPreferenceStorageError::LimitExceeded { limit: 4, .. })
    ));
    assert!(matches!(
        config.namespace(&unknown),
        Err(BrowserPreferenceStorageError::NamespaceNotDeclared { namespace })
            if namespace == "unknown"
    ));
}

#[test]
fn platform_failures_distinguish_quota_and_bound_diagnostics() {
    let quota = BrowserPreferenceStorageError::from_platform(
        "put",
        Some("QuotaExceededError"),
        "origin quota is full",
    );
    assert!(quota.is_quota_exceeded());
    assert!(matches!(
        quota,
        BrowserPreferenceStorageError::QuotaExceeded {
            operation,
            message
        } if operation == "put" && message == "QuotaExceededError: origin quota is full"
    ));

    let fallback_quota =
        BrowserPreferenceStorageError::from_platform("put", None, "browser quota exceeded");
    assert!(fallback_quota.is_quota_exceeded());

    let platform = BrowserPreferenceStorageError::from_platform(
        "open",
        Some("SecurityError"),
        &"\u{e9}".repeat(MAX_BROWSER_PREFERENCE_PLATFORM_ERROR_BYTES),
    );
    let BrowserPreferenceStorageError::Platform { message, .. } = platform else {
        panic!("expected a non-quota platform error")
    };
    assert!(message.len() <= MAX_BROWSER_PREFERENCE_PLATFORM_ERROR_BYTES);
    assert!(message.ends_with("..."));
    assert!(message.is_char_boundary(message.len()));
}
