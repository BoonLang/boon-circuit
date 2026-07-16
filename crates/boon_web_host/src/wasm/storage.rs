//! IndexedDB-backed durable preference storage. Product values remain opaque;
//! this adapter only enforces the declared namespace and resource contract.

use crate::{
    BROWSER_PREFERENCE_STORAGE_OBJECT_STORE, BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION,
    BrowserPreferenceKey, BrowserPreferenceNamespace, BrowserPreferenceNamespaceId,
    BrowserPreferencePutOutcome, BrowserPreferenceStorageConfig, BrowserPreferenceStorageError,
    BrowserPreferenceStorageResult, BrowserPreferenceValue, decode_preference_value,
    encode_preference_value, indexed_db_namespace_bounds, indexed_db_storage_key,
};
use idb::{
    Database, KeyRange, ObjectStore, Query, TransactionFuture, TransactionMode, TransactionResult,
};
use js_sys::Uint8Array;
use std::future::IntoFuture;
use wasm_bindgen::{JsCast, JsValue};

/// A browser-local, IndexedDB-backed store for explicit durable preferences.
///
/// The database has one versioned physical object store. Namespace isolation is
/// represented in collision-free keys so adding an application-declared
/// namespace does not silently change the IndexedDB schema.
pub struct BrowserIndexedDbPreferenceStorage {
    database: Database,
    config: BrowserPreferenceStorageConfig,
}

impl BrowserIndexedDbPreferenceStorage {
    pub async fn open(
        config: BrowserPreferenceStorageConfig,
    ) -> BrowserPreferenceStorageResult<Self> {
        config.validate()?;
        let database = Database::builder(&config.database_name)
            .version(BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION)
            .add_object_store(ObjectStore::builder(
                BROWSER_PREFERENCE_STORAGE_OBJECT_STORE,
            ))
            .build()
            .await
            .map_err(open_error)?;

        let actual_version = database
            .version()
            .map_err(|error| idb_error("inspect schema version", error))?;
        if actual_version != BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION {
            database.close();
            return Err(BrowserPreferenceStorageError::SchemaMismatch {
                expected_version: BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION,
                actual_version: Some(actual_version),
                reason: "opened IndexedDB database has a different version".to_owned(),
            });
        }

        let stores = database.store_names();
        if stores.len() != 1
            || stores.first().map(String::as_str) != Some(BROWSER_PREFERENCE_STORAGE_OBJECT_STORE)
        {
            database.close();
            return Err(BrowserPreferenceStorageError::SchemaMismatch {
                expected_version: BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION,
                actual_version: Some(actual_version),
                reason: "opened IndexedDB database has an unexpected object-store set".to_owned(),
            });
        }

        Ok(Self { database, config })
    }

    pub fn config(&self) -> &BrowserPreferenceStorageConfig {
        &self.config
    }

    pub async fn get(
        &self,
        namespace: &BrowserPreferenceNamespaceId,
        key: &BrowserPreferenceKey,
    ) -> BrowserPreferenceStorageResult<Option<BrowserPreferenceValue>> {
        let declaration = self.config.namespace(namespace)?;
        declaration.validate_key(key)?;
        let storage_key = JsValue::from_str(&indexed_db_storage_key(namespace, key));
        let (store, completion) = self.transaction("get", TransactionMode::ReadOnly)?;
        let operation = async {
            let value = store
                .get(storage_key)
                .map_err(|error| idb_error("get", error))?
                .await
                .map_err(|error| idb_error("get", error))?;
            value
                .map(|value| stored_bytes(declaration, value))
                .transpose()
        }
        .await;
        let encoded = finish_transaction("get", operation, completion).await?;
        encoded
            .map(|encoded| decode_preference_value(declaration, &encoded))
            .transpose()
    }

    pub async fn put(
        &self,
        namespace: &BrowserPreferenceNamespaceId,
        key: &BrowserPreferenceKey,
        value: &BrowserPreferenceValue,
    ) -> BrowserPreferenceStorageResult<BrowserPreferencePutOutcome> {
        self.config.validate_entry(namespace, key, value)?;
        let declaration = self.config.namespace(namespace)?;
        let encoded = encode_preference_value(declaration, value)?;
        let stored_value: JsValue = Uint8Array::from(encoded.as_slice()).into();
        let storage_key = JsValue::from_str(&indexed_db_storage_key(namespace, key));
        let namespace_query = namespace_query(namespace)?;
        let (store, completion) = self.transaction("put", TransactionMode::ReadWrite)?;
        let operation = async {
            let existing = store
                .get_key(storage_key.clone())
                .map_err(|error| idb_error("check preference key", error))?
                .await
                .map_err(|error| idb_error("check preference key", error))?
                .is_some();

            if !existing {
                let count = store
                    .count(Some(namespace_query))
                    .map_err(|error| idb_error("count namespace entries", error))?
                    .await
                    .map_err(|error| idb_error("count namespace entries", error))?;
                if count >= declaration.limits().max_entries() {
                    return Err(BrowserPreferenceStorageError::LimitExceeded {
                        resource: format!("entry count in namespace {}", declaration.id()),
                        limit: declaration.limits().max_entries() as usize,
                    });
                }
            }

            store
                .put(&stored_value, Some(&storage_key))
                .map_err(|error| idb_error("put", error))?
                .await
                .map_err(|error| idb_error("put", error))?;
            Ok(if existing {
                BrowserPreferencePutOutcome::Replaced
            } else {
                BrowserPreferencePutOutcome::Inserted
            })
        }
        .await;
        finish_transaction("put", operation, completion).await
    }

    pub async fn delete(
        &self,
        namespace: &BrowserPreferenceNamespaceId,
        key: &BrowserPreferenceKey,
    ) -> BrowserPreferenceStorageResult<bool> {
        self.config.validate_key(namespace, key)?;
        let storage_key = JsValue::from_str(&indexed_db_storage_key(namespace, key));
        let (store, completion) = self.transaction("delete", TransactionMode::ReadWrite)?;
        let operation = async {
            let existed = store
                .get_key(storage_key.clone())
                .map_err(|error| idb_error("check preference key", error))?
                .await
                .map_err(|error| idb_error("check preference key", error))?
                .is_some();
            if existed {
                store
                    .delete(storage_key)
                    .map_err(|error| idb_error("delete", error))?
                    .await
                    .map_err(|error| idb_error("delete", error))?;
            }
            Ok(existed)
        }
        .await;
        finish_transaction("delete", operation, completion).await
    }

    /// Deletes only the addressed logical namespace and returns the number of
    /// records that were present. Other namespaces in the database are intact.
    pub async fn clear(
        &self,
        namespace: &BrowserPreferenceNamespaceId,
    ) -> BrowserPreferenceStorageResult<u32> {
        self.config.namespace(namespace)?;
        let query = namespace_query(namespace)?;
        let (store, completion) = self.transaction("clear", TransactionMode::ReadWrite)?;
        let operation = async {
            let count = store
                .count(Some(query.clone()))
                .map_err(|error| idb_error("count namespace entries", error))?
                .await
                .map_err(|error| idb_error("count namespace entries", error))?;
            if count != 0 {
                store
                    .delete(query)
                    .map_err(|error| idb_error("clear", error))?
                    .await
                    .map_err(|error| idb_error("clear", error))?;
            }
            Ok(count)
        }
        .await;
        finish_transaction("clear", operation, completion).await
    }

    pub fn close(self) {
        self.database.close();
    }

    fn transaction(
        &self,
        operation: &str,
        mode: TransactionMode,
    ) -> BrowserPreferenceStorageResult<(ObjectStore, TransactionFuture)> {
        let transaction = self
            .database
            .transaction(&[BROWSER_PREFERENCE_STORAGE_OBJECT_STORE], mode)
            .map_err(|error| idb_error(operation, error))?;
        let store = transaction
            .object_store(BROWSER_PREFERENCE_STORAGE_OBJECT_STORE)
            .map_err(|error| idb_error(operation, error))?;
        let completion = transaction.into_future();
        Ok((store, completion))
    }
}

impl Drop for BrowserIndexedDbPreferenceStorage {
    fn drop(&mut self) {
        self.database.close();
    }
}

async fn finish_transaction<T>(
    operation: &str,
    request_result: BrowserPreferenceStorageResult<T>,
    completion: TransactionFuture,
) -> BrowserPreferenceStorageResult<T> {
    match (request_result, completion.await) {
        (Err(request_error), Err(transaction_error)) => {
            let transaction_error = idb_error(operation, transaction_error);
            if transaction_error.is_quota_exceeded() {
                Err(transaction_error)
            } else {
                Err(request_error)
            }
        }
        (Err(request_error), _) => Err(request_error),
        (Ok(_), Err(transaction_error)) => Err(idb_error(operation, transaction_error)),
        (Ok(value), Ok(TransactionResult::Committed)) => Ok(value),
        (Ok(_), Ok(TransactionResult::Aborted)) => {
            Err(BrowserPreferenceStorageError::from_platform(
                operation,
                Some("AbortError"),
                "IndexedDB transaction aborted",
            ))
        }
    }
}

fn namespace_query(
    namespace: &BrowserPreferenceNamespaceId,
) -> BrowserPreferenceStorageResult<Query> {
    let (lower, upper) = indexed_db_namespace_bounds(namespace);
    KeyRange::bound(
        &JsValue::from_str(&lower),
        &JsValue::from_str(&upper),
        Some(false),
        Some(true),
    )
    .map(Query::from)
    .map_err(|error| idb_error("create namespace key range", error))
}

fn stored_bytes(
    namespace: &BrowserPreferenceNamespace,
    value: JsValue,
) -> BrowserPreferenceStorageResult<Vec<u8>> {
    if !value.is_instance_of::<Uint8Array>() {
        return Err(BrowserPreferenceStorageError::CorruptValue {
            namespace: namespace.id().to_string(),
            reason: "stored IndexedDB value is not a Uint8Array".to_owned(),
        });
    }
    let bytes = Uint8Array::new(&value);
    let max_encoded_bytes = namespace.limits().max_value_bytes() + 2;
    if bytes.length() as usize > max_encoded_bytes {
        return Err(BrowserPreferenceStorageError::CorruptValue {
            namespace: namespace.id().to_string(),
            reason: format!(
                "stored IndexedDB value exceeds the encoded byte limit {max_encoded_bytes}"
            ),
        });
    }
    Ok(bytes.to_vec())
}

fn open_error(error: idb::Error) -> BrowserPreferenceStorageError {
    let detail = format!("{error}; {error:?}");
    let is_version_error = match &error {
        idb::Error::DomException(exception) => exception.name() == "VersionError",
        _ => detail.to_ascii_lowercase().contains("versionerror"),
    };
    if is_version_error {
        BrowserPreferenceStorageError::SchemaMismatch {
            expected_version: BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION,
            actual_version: None,
            reason: "browser rejected opening an older schema version".to_owned(),
        }
    } else {
        idb_error_with_detail("open", error, detail)
    }
}

fn idb_error(operation: &str, error: idb::Error) -> BrowserPreferenceStorageError {
    let detail = format!("{error}; {error:?}");
    idb_error_with_detail(operation, error, detail)
}

fn idb_error_with_detail(
    operation: &str,
    error: idb::Error,
    detail: String,
) -> BrowserPreferenceStorageError {
    match error {
        idb::Error::DomException(exception) => BrowserPreferenceStorageError::from_platform(
            operation,
            Some(&exception.name()),
            &exception.message(),
        ),
        _ => BrowserPreferenceStorageError::from_platform(operation, None, &detail),
    }
}
