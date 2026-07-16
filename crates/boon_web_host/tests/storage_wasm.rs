#![cfg(target_arch = "wasm32")]

use boon_web_host::wasm::BrowserIndexedDbPreferenceStorage;
use boon_web_host::{
    BrowserPreferenceKey, BrowserPreferenceNamespace, BrowserPreferenceNamespaceId,
    BrowserPreferenceNamespaceLimits, BrowserPreferencePutOutcome, BrowserPreferenceStorageConfig,
    BrowserPreferenceStorageError, BrowserPreferenceValue, BrowserPreferenceValueKind,
};
use idb::{Database, Factory, ObjectStore};
use std::sync::atomic::{AtomicU32, Ordering};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

static NEXT_DATABASE_ID: AtomicU32 = AtomicU32::new(0);

fn database_name(label: &str) -> String {
    format!(
        "boon-web-host-{label}-{}-{}",
        js_sys::Date::now() as u64,
        NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn namespace(
    id: &str,
    kind: BrowserPreferenceValueKind,
    max_value_bytes: usize,
    max_entries: u32,
) -> BrowserPreferenceNamespace {
    BrowserPreferenceNamespace::new(
        id,
        kind,
        BrowserPreferenceNamespaceLimits::new(32, max_value_bytes, max_entries).unwrap(),
    )
    .unwrap()
}

async fn delete_database(name: &str) {
    Factory::new().unwrap().delete(name).unwrap().await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn indexed_db_round_trips_replaces_deletes_and_isolates_namespaces() {
    let database_name = database_name("round-trip");
    delete_database(&database_name).await;
    let text_namespace = BrowserPreferenceNamespaceId::new("text").unwrap();
    let bytes_namespace = BrowserPreferenceNamespaceId::new("bytes").unwrap();
    let locale_key = BrowserPreferenceKey::new("locale").unwrap();
    let camera_key = BrowserPreferenceKey::new("camera").unwrap();
    let config = BrowserPreferenceStorageConfig::new(
        database_name.clone(),
        [
            namespace("text", BrowserPreferenceValueKind::Text, 32, 4),
            namespace("bytes", BrowserPreferenceValueKind::Bytes, 32, 4),
        ],
    )
    .unwrap();

    let storage = BrowserIndexedDbPreferenceStorage::open(config.clone())
        .await
        .unwrap();
    assert_eq!(storage.config(), &config);
    assert_eq!(
        storage.get(&text_namespace, &locale_key).await.unwrap(),
        None
    );
    assert_eq!(
        storage
            .put(
                &text_namespace,
                &locale_key,
                &BrowserPreferenceValue::Text("nb".to_owned()),
            )
            .await
            .unwrap(),
        BrowserPreferencePutOutcome::Inserted
    );
    assert_eq!(
        storage
            .put(
                &text_namespace,
                &locale_key,
                &BrowserPreferenceValue::Text("en".to_owned()),
            )
            .await
            .unwrap(),
        BrowserPreferencePutOutcome::Replaced
    );
    assert_eq!(
        storage
            .put(
                &bytes_namespace,
                &camera_key,
                &BrowserPreferenceValue::Bytes(vec![1, 2, 3]),
            )
            .await
            .unwrap(),
        BrowserPreferencePutOutcome::Inserted
    );
    storage.close();

    let storage = BrowserIndexedDbPreferenceStorage::open(config)
        .await
        .unwrap();
    assert_eq!(
        storage.get(&text_namespace, &locale_key).await.unwrap(),
        Some(BrowserPreferenceValue::Text("en".to_owned()))
    );
    assert_eq!(
        storage.get(&bytes_namespace, &camera_key).await.unwrap(),
        Some(BrowserPreferenceValue::Bytes(vec![1, 2, 3]))
    );
    assert!(storage.delete(&text_namespace, &locale_key).await.unwrap());
    assert!(!storage.delete(&text_namespace, &locale_key).await.unwrap());
    assert_eq!(storage.clear(&bytes_namespace).await.unwrap(), 1);
    assert_eq!(storage.clear(&bytes_namespace).await.unwrap(), 0);
    storage.close();
    delete_database(&database_name).await;
}

#[wasm_bindgen_test(async)]
async fn indexed_db_rejections_are_bounded_atomic_and_do_not_mutate_existing_data() {
    let database_name = database_name("limits");
    delete_database(&database_name).await;
    let namespace_id = BrowserPreferenceNamespaceId::new("preferences").unwrap();
    let first_key = BrowserPreferenceKey::new("first").unwrap();
    let second_key = BrowserPreferenceKey::new("second").unwrap();
    let config = BrowserPreferenceStorageConfig::new(
        database_name.clone(),
        [namespace(
            "preferences",
            BrowserPreferenceValueKind::Text,
            3,
            1,
        )],
    )
    .unwrap();
    let storage = BrowserIndexedDbPreferenceStorage::open(config)
        .await
        .unwrap();

    assert_eq!(
        storage
            .put(
                &namespace_id,
                &first_key,
                &BrowserPreferenceValue::Text("one".to_owned()),
            )
            .await
            .unwrap(),
        BrowserPreferencePutOutcome::Inserted
    );
    assert!(matches!(
        storage
            .put(
                &namespace_id,
                &second_key,
                &BrowserPreferenceValue::Text("two".to_owned()),
            )
            .await,
        Err(BrowserPreferenceStorageError::LimitExceeded { limit: 1, .. })
    ));
    assert!(matches!(
        storage
            .put(
                &namespace_id,
                &first_key,
                &BrowserPreferenceValue::Text("four".to_owned()),
            )
            .await,
        Err(BrowserPreferenceStorageError::LimitExceeded { limit: 3, .. })
    ));
    assert!(matches!(
        storage
            .put(
                &namespace_id,
                &first_key,
                &BrowserPreferenceValue::Bytes(vec![1]),
            )
            .await,
        Err(BrowserPreferenceStorageError::ValueKindMismatch { .. })
    ));
    assert_eq!(
        storage.get(&namespace_id, &first_key).await.unwrap(),
        Some(BrowserPreferenceValue::Text("one".to_owned()))
    );
    assert_eq!(storage.get(&namespace_id, &second_key).await.unwrap(), None);
    storage.close();
    delete_database(&database_name).await;
}

#[wasm_bindgen_test(async)]
async fn indexed_db_open_rejects_unexpected_and_newer_schemas_without_rewriting_them() {
    let database_name = database_name("schema");
    delete_database(&database_name).await;
    let unexpected = Database::builder(&database_name)
        .version(1)
        .add_object_store(ObjectStore::builder("unexpected"))
        .build()
        .await
        .unwrap();
    unexpected.close();
    let config = BrowserPreferenceStorageConfig::new(
        database_name.clone(),
        [namespace(
            "preferences",
            BrowserPreferenceValueKind::Text,
            8,
            1,
        )],
    )
    .unwrap();
    assert!(matches!(
        BrowserIndexedDbPreferenceStorage::open(config.clone()).await,
        Err(BrowserPreferenceStorageError::SchemaMismatch {
            actual_version: Some(1),
            ..
        })
    ));
    delete_database(&database_name).await;

    let newer = Database::builder(&database_name)
        .version(2)
        .add_object_store(ObjectStore::builder("preferences"))
        .build()
        .await
        .unwrap();
    newer.close();
    assert!(matches!(
        BrowserIndexedDbPreferenceStorage::open(config).await,
        Err(BrowserPreferenceStorageError::SchemaMismatch {
            actual_version: None,
            ..
        })
    ));
    delete_database(&database_name).await;
}
