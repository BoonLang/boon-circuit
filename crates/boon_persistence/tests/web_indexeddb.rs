#![cfg(target_arch = "wasm32")]

use boon_persistence::{
    ActivationBatch, BrowserFailureKind, CheckpointBatch, DurableChange, DurableEffectRow,
    DurableOutboxChange, DurableOutboxItem, InMemoryDriver, InspectRequest, PersistenceCommand,
    PersistenceDriver, PersistenceResult, ResetApplicationBatch, RestoreImage, RestoreRequest,
    RexieDriver, ShutdownRequest, StoreError, StoredList, StoredRow, StoredScalar, StoredValue,
    browser_failure_kind,
};
use boon_plan::{
    ApplicationIdentity, EffectId, EffectInvocationId, MemoryId, MemoryKind, MemoryLeafId,
    MemoryOwnerPath, MigrationEdgeId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::Poll;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

static NEXT_DATABASE_ID: AtomicU32 = AtomicU32::new(0);

fn number(value: i64) -> StoredValue {
    StoredValue::integer(value).unwrap()
}

fn application() -> ApplicationIdentity {
    ApplicationIdentity::new("dev.boon.web-test", "sparse", "browser")
}

fn memory(name: &str, kind: MemoryKind) -> MemoryId {
    MemoryId::from_identity(
        &MemoryOwnerPath {
            canonical_module: "web_indexeddb".to_owned(),
            named_owner_path: "state".to_owned(),
        },
        name,
        kind,
    )
    .unwrap()
}

async fn assert_parity(
    browser: &mut RexieDriver,
    memory: &mut InMemoryDriver,
    command: PersistenceCommand,
) -> PersistenceResult {
    let expected = memory.execute(command.clone());
    let actual = browser.execute(command).await;
    assert_eq!(actual, expected);
    actual
}

#[wasm_bindgen_test(async)]
async fn sparse_transactions_match_in_memory_and_abort_atomically() {
    let database_name = format!(
        "boon-persistence-sparse-{}-{}",
        js_sys::Date::now() as u64,
        NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed)
    );
    let mut browser = RexieDriver::open(database_name.clone()).await.unwrap();
    let mut memory_driver = InMemoryDriver::default();
    let app = application();
    let schema_v1 = [0x11; 32];
    let scalar_memory = memory("payload", MemoryKind::Scalar);
    let list_memory = memory("rows", MemoryKind::List);
    let missing_list = memory("missing", MemoryKind::List);
    let payload_field = MemoryLeafId::from_memory_path(list_memory, "payload").unwrap();
    let inserted_field = MemoryLeafId::from_memory_path(list_memory, "inserted").unwrap();

    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Load(RestoreRequest {
            application: app.clone(),
            expected_schema_hash: Some(schema_v1),
        }),
    )
    .await;
    assert!(browser.storage_status().missing_or_evicted);
    assert_eq!(
        browser.storage_status().last_operation_failure,
        Some(BrowserFailureKind::MissingOrEvicted)
    );

    let initial = RestoreImage::empty(app.clone(), 1, schema_v1);
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Initialize(initial),
    )
    .await;
    assert!(!browser.storage_status().missing_or_evicted);
    assert_eq!(browser.storage_status().last_operation_failure, None);

    let shared_payload = vec![0x6a; boon_persistence::INLINE_BYTES_THRESHOLD + 1];
    let first_row = StoredRow {
        key: 0,
        generation: 1,
        fields: BTreeMap::from([(payload_field, StoredValue::Bytes(shared_payload.clone()))]),
        touched_fields: BTreeSet::from([payload_field]),
    };
    let effect = EffectId::from_host_operation("Browser/send").unwrap();
    let invocation =
        EffectInvocationId::from_semantic_route(effect, "browser/source", "browser/target")
            .unwrap();
    let outbox_key = StoredValue::Text("browser-key".to_owned());
    let outbox_intent = number(7);
    let outbox_item = DurableOutboxItem::pending(
        invocation,
        effect,
        outbox_key.clone(),
        outbox_intent.clone(),
        Some(DurableEffectRow {
            list_memory_id: list_memory,
            row_key: 0,
            row_generation: 1,
        }),
        1,
    );
    let first = CheckpointBatch {
        application: app.clone(),
        schema_hash: schema_v1,
        base_epoch: 0,
        next_epoch: 1,
        first_turn_sequence: 1,
        last_turn_sequence: 1,
        changes: vec![
            DurableChange::SetScalar {
                memory_id: scalar_memory,
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Bytes(shared_payload.clone()),
                },
            },
            DurableChange::SetList {
                memory_id: list_memory,
                value: StoredList {
                    touched: true,
                    next_key: 1,
                    rows: vec![first_row],
                },
            },
        ],
        outbox_changes: vec![DurableOutboxChange::Enqueue {
            item: outbox_item.clone(),
        }],
        content_artifact_changes: Vec::new(),
        checksum: [0; 32],
    }
    .seal();
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Commit(first),
    )
    .await;
    browser.refresh_storage_status().await;
    assert!(!browser.storage_status().missing_or_evicted);

    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    browser = RexieDriver::open(database_name.clone()).await.unwrap();
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Load(RestoreRequest {
            application: app.clone(),
            expected_schema_hash: Some(schema_v1),
        }),
    )
    .await;

    let inserted_row = StoredRow {
        key: 1,
        generation: 1,
        fields: BTreeMap::from([(inserted_field, StoredValue::Bool(true))]),
        touched_fields: BTreeSet::from([inserted_field]),
    };
    let second_outbox_item = DurableOutboxItem::pending(
        invocation,
        effect,
        outbox_key,
        outbox_intent,
        Some(DurableEffectRow {
            list_memory_id: list_memory,
            row_key: 1,
            row_generation: 1,
        }),
        2,
    );
    assert_ne!(outbox_item.item_id, second_outbox_item.item_id);
    assert_eq!(
        outbox_item.idempotency_key,
        second_outbox_item.idempotency_key
    );
    let second = CheckpointBatch {
        application: app.clone(),
        schema_hash: schema_v1,
        base_epoch: 1,
        next_epoch: 2,
        first_turn_sequence: 2,
        last_turn_sequence: 2,
        changes: vec![
            DurableChange::SetScalar {
                memory_id: scalar_memory,
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Text("inline".to_owned()),
                },
            },
            DurableChange::SetRowField {
                memory_id: list_memory,
                row_key: 0,
                row_generation: 1,
                field_id: payload_field,
                value: number(9),
            },
            DurableChange::InsertRow {
                memory_id: list_memory,
                index: 1,
                row: inserted_row,
                next_key: 2,
            },
        ],
        outbox_changes: vec![
            DurableOutboxChange::BeginDispatch {
                item_id: outbox_item.item_id,
                expected_revision: 0,
                next_revision: 1,
                attempt: 1,
                turn_sequence: 2,
            },
            DurableOutboxChange::Enqueue {
                item: second_outbox_item,
            },
        ],
        content_artifact_changes: Vec::new(),
        checksum: [0; 32],
    }
    .seal();
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Commit(second),
    )
    .await;

    let retained_row = StoredRow {
        key: 0,
        generation: 1,
        fields: BTreeMap::from([(payload_field, number(9))]),
        touched_fields: BTreeSet::from([payload_field]),
    };
    let third = CheckpointBatch {
        application: app.clone(),
        schema_hash: schema_v1,
        base_epoch: 2,
        next_epoch: 3,
        first_turn_sequence: 3,
        last_turn_sequence: 3,
        changes: vec![
            DurableChange::RemoveRow {
                memory_id: list_memory,
                row_key: 1,
                row_generation: 1,
                next_key: 2,
            },
            DurableChange::DeleteScalar {
                memory_id: scalar_memory,
            },
            DurableChange::DeleteList {
                memory_id: list_memory,
            },
            DurableChange::SetList {
                memory_id: list_memory,
                value: StoredList {
                    touched: true,
                    next_key: 2,
                    rows: vec![retained_row],
                },
            },
        ],
        outbox_changes: vec![DurableOutboxChange::RequireReconciliation {
            item_id: outbox_item.item_id,
            expected_revision: 1,
            next_revision: 2,
            attempt: 1,
            turn_sequence: 3,
        }],
        content_artifact_changes: Vec::new(),
        checksum: [0; 32],
    }
    .seal();
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Commit(third),
    )
    .await;

    let before_abort = memory_driver.image(&app).unwrap().clone();
    let failing = CheckpointBatch {
        application: app.clone(),
        schema_hash: schema_v1,
        base_epoch: 3,
        next_epoch: 4,
        first_turn_sequence: 4,
        last_turn_sequence: 4,
        changes: vec![
            DurableChange::SetScalar {
                memory_id: scalar_memory,
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Text("must roll back".to_owned()),
                },
            },
            DurableChange::InsertRow {
                memory_id: missing_list,
                index: 0,
                row: StoredRow {
                    key: 0,
                    generation: 1,
                    fields: BTreeMap::new(),
                    touched_fields: BTreeSet::new(),
                },
                next_key: 1,
            },
        ],
        outbox_changes: Vec::new(),
        content_artifact_changes: Vec::new(),
        checksum: [0; 32],
    }
    .seal();
    let failed = assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Commit(failing),
    )
    .await;
    assert!(matches!(
        failed,
        PersistenceResult::Committed(Err(boon_persistence::StoreError::InvalidAuthority(_)))
    ));
    assert_eq!(memory_driver.image(&app), Some(&before_abort));

    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Load(RestoreRequest {
            application: app.clone(),
            expected_schema_hash: Some(schema_v1),
        }),
    )
    .await;
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Inspect(InspectRequest {
            application: app.clone(),
        }),
    )
    .await;

    let current = memory_driver.image(&app).unwrap().clone();
    let mut candidate = current.clone();
    candidate.schema_version = 3;
    candidate.schema_hash = [0x33; 32];
    candidate.lists.remove(&list_memory);
    candidate.scalars.insert(
        scalar_memory,
        StoredScalar {
            touched: true,
            value: StoredValue::Text("activated".to_owned()),
        },
    );
    candidate
        .completed_migration_edges
        .insert(MigrationEdgeId([0x44; 32]));
    candidate
        .completed_migration_edges
        .insert(MigrationEdgeId([0x45; 32]));
    let activation = ActivationBatch::between(&current, &candidate).unwrap();
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Activate(activation),
    )
    .await;

    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    browser = RexieDriver::open(database_name.clone()).await.unwrap();
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Load(RestoreRequest {
            application: app.clone(),
            expected_schema_hash: Some([0x33; 32]),
        }),
    )
    .await;

    let completed = CheckpointBatch {
        application: app.clone(),
        schema_hash: [0x33; 32],
        base_epoch: 4,
        next_epoch: 5,
        first_turn_sequence: 4,
        last_turn_sequence: 4,
        changes: Vec::new(),
        outbox_changes: vec![DurableOutboxChange::Complete {
            item_id: outbox_item.item_id,
            expected_revision: 2,
            next_revision: 3,
            attempt: 1,
            outcome: StoredValue::Text("done".to_owned()),
            turn_sequence: 4,
        }],
        content_artifact_changes: Vec::new(),
        checksum: [0; 32],
    }
    .seal();
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Commit(completed),
    )
    .await;

    let default_image = RestoreImage::empty(app.clone(), 4, [0x44; 32]);
    let reset = ResetApplicationBatch {
        application: app.clone(),
        expected_base_epoch: 5,
        next_epoch: 6,
        source_schema_hash: [0x33; 32],
        default_image,
        checksum: [0; 32],
    }
    .seal();
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::ResetApplication(reset),
    )
    .await;
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Load(RestoreRequest {
            application: app,
            expected_schema_hash: Some([0x44; 32]),
        }),
    )
    .await;
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::Shutdown(ShutdownRequest),
    )
    .await;
    rexie::Rexie::delete(&database_name).await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn coordinator_defers_indexeddb_and_failure_mapping_is_explicit() {
    let database_name = format!(
        "boon-persistence-coordinator-{}-{}",
        js_sys::Date::now() as u64,
        NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed)
    );
    let mut opening = Box::pin(RexieDriver::open(database_name.clone()));
    assert!(matches!(futures::poll!(opening.as_mut()), Poll::Pending));
    let mut browser = opening.await.unwrap();
    assert_eq!(browser.command_queue_capacity(), 64);

    let image = RestoreImage::empty(application(), 1, [0x51; 32]);
    let mut execution = Box::pin(browser.execute(PersistenceCommand::Initialize(image)));
    assert!(matches!(futures::poll!(execution.as_mut()), Poll::Pending));
    assert!(matches!(
        execution.await,
        PersistenceResult::Initialized(Ok(_))
    ));

    for (code, expected) in [
        ("quota_exceeded", BrowserFailureKind::QuotaExceeded),
        (
            "private_mode_or_unavailable",
            BrowserFailureKind::PrivateModeOrUnavailable,
        ),
        ("upgrade_blocked", BrowserFailureKind::UpgradeBlocked),
        ("timeout", BrowserFailureKind::Timeout),
        (
            "transaction_aborted",
            BrowserFailureKind::TransactionAborted,
        ),
    ] {
        let error = StoreError::Backend(format!("indexeddb/{code}: deterministic test"));
        assert_eq!(browser_failure_kind(&error), Some(expected));
    }

    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    rexie::Rexie::delete(&database_name).await.unwrap();
}
