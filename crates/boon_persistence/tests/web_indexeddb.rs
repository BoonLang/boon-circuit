#![cfg(target_arch = "wasm32")]

use boon_persistence::{
    ActivationBatch, BrowserFailureKind, BrowserPersistenceEnqueueError, CheckpointBatch,
    DurableChange, DurableOutboxChange, DurableOutboxItem, DurableOwner, DurableRowId,
    ExportApplicationRequest, InMemoryDriver, InspectRequest, PersistenceCommand,
    PersistenceDriver, PersistenceResult, ResetApplicationBatch, RestoreImage, RestoreRequest,
    RexieDriver, ShutdownRequest, StoreError, StoredList, StoredRow, StoredScalar, StoredValue,
    browser_failure_kind,
};
use boon_plan::{
    ApplicationIdentity, EffectId, EffectInvocationId, MemoryId, MemoryKind, MemoryLeafId,
    MemoryOwnerPath, MigrationEdgeId,
};
use js_sys::Uint8Array;
use rexie::{ObjectStore, Rexie, TransactionMode};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::Poll;
use wasm_bindgen_futures::spawn_local;
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

fn database_name(label: &str) -> String {
    format!(
        "boon-persistence-{label}-{}-{}",
        js_sys::Date::now() as u64,
        NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn scalar_checkpoint(
    application: &ApplicationIdentity,
    schema_hash: [u8; 32],
    base_epoch: u64,
    next_epoch: u64,
    turn_sequence: u64,
    text: String,
) -> CheckpointBatch {
    CheckpointBatch {
        application: application.clone(),
        schema_hash,
        base_epoch,
        next_epoch,
        first_turn_sequence: turn_sequence,
        last_turn_sequence: turn_sequence,
        changes: vec![DurableChange::SetScalar {
            memory_id: memory("admission", MemoryKind::Scalar),
            value: StoredScalar {
                touched: true,
                value: StoredValue::Text(text),
            },
        }],
        outbox_changes: Vec::new(),
        content_artifact_changes: Vec::new(),
        checksum: [0; 32],
    }
    .seal()
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
        source_order_token: 1_u128 << 64,
        owner: DurableOwner {
            ancestors: vec![DurableRowId {
                list_memory_id: list_memory,
                row_key: 0,
                row_generation: 1,
            }],
        },
        materialization_origin: None,
        fields: BTreeMap::from([(
            payload_field,
            StoredValue::Bytes(shared_payload.clone().into()),
        )]),
        touched_fields: BTreeSet::from([payload_field]),
    };
    let effect = EffectId::from_host_operation("Browser/send").unwrap();
    let invocation = EffectInvocationId::from_result_owner(effect, "browser/target").unwrap();
    let outbox_key = StoredValue::Text("browser-key".to_owned());
    let outbox_intent = number(7);
    let first_effect_row = DurableRowId {
        list_memory_id: list_memory,
        row_key: 0,
        row_generation: 1,
    };
    let outbox_item = DurableOutboxItem::pending(
        invocation,
        effect,
        outbox_key.clone(),
        outbox_intent.clone(),
        DurableOwner {
            ancestors: vec![first_effect_row],
        },
        Some(first_effect_row),
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
                    value: StoredValue::Bytes(shared_payload.clone().into()),
                },
            },
            DurableChange::SetList {
                memory_id: list_memory,
                value: StoredList {
                    touched: true,
                    revision: 0,
                    next_key: 1,
                    next_order_token: 2_u128 << 64,
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
        source_order_token: 2_u128 << 64,
        owner: DurableOwner {
            ancestors: vec![DurableRowId {
                list_memory_id: list_memory,
                row_key: 1,
                row_generation: 1,
            }],
        },
        materialization_origin: None,
        fields: BTreeMap::from([(inserted_field, StoredValue::Bool(true))]),
        touched_fields: BTreeSet::from([inserted_field]),
    };
    let second_effect_row = DurableRowId {
        list_memory_id: list_memory,
        row_key: 1,
        row_generation: 1,
    };
    let second_outbox_item = DurableOutboxItem::pending(
        invocation,
        effect,
        outbox_key,
        outbox_intent,
        DurableOwner {
            ancestors: vec![second_effect_row],
        },
        Some(second_effect_row),
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
                list_revision: 1,
                row_key: 0,
                row_generation: 1,
                owner: DurableOwner {
                    ancestors: vec![first_effect_row],
                },
                materialization_origin: None,
                field_id: payload_field,
                value: number(9),
            },
            DurableChange::InsertRow {
                memory_id: list_memory,
                list_revision: 1,
                index: 1,
                row: inserted_row,
                next_key: 2,
                next_order_token: 3_u128 << 64,
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
        source_order_token: 1_u128 << 64,
        owner: DurableOwner {
            ancestors: vec![first_effect_row],
        },
        materialization_origin: None,
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
                list_revision: 2,
                row_key: 1,
                row_generation: 1,
                next_key: 2,
                next_order_token: 3_u128 << 64,
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
                    revision: 2,
                    next_key: 2,
                    next_order_token: 3_u128 << 64,
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
                list_revision: 3,
                index: 0,
                row: StoredRow {
                    key: 0,
                    generation: 1,
                    source_order_token: 1_u128 << 64,
                    owner: DurableOwner {
                        ancestors: vec![DurableRowId {
                            list_memory_id: missing_list,
                            row_key: 0,
                            row_generation: 1,
                        }],
                    },
                    materialization_origin: None,
                    fields: BTreeMap::new(),
                    touched_fields: BTreeSet::new(),
                },
                next_key: 1,
                next_order_token: 2_u128 << 64,
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
    assert_parity(
        &mut browser,
        &mut memory_driver,
        PersistenceCommand::ExportApplication(ExportApplicationRequest {
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
            application: app.clone(),
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

#[wasm_bindgen_test(async)]
async fn cancelled_mutating_control_remains_tracked_and_can_be_resumed() {
    let database_name = database_name("cancelled-control");
    let app = application();
    let schema_hash = [0x61; 32];
    let mut browser = RexieDriver::open(database_name.clone()).await.unwrap();
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Initialize(RestoreImage::empty(
                app.clone(),
                1,
                schema_hash,
            )))
            .await,
        PersistenceResult::Initialized(Ok(_))
    ));

    let reset = ResetApplicationBatch {
        application: app,
        expected_base_epoch: 0,
        next_epoch: 1,
        source_schema_hash: schema_hash,
        default_image: RestoreImage::empty(application(), 1, schema_hash),
        checksum: [0; 32],
    }
    .seal();
    let mut execution = Box::pin(browser.execute(PersistenceCommand::ResetApplication(reset)));
    assert!(matches!(futures::poll!(execution.as_mut()), Poll::Pending));
    drop(execution);

    assert_eq!(browser.outstanding_operation_count(), 1);
    assert!(browser.outstanding_payload_bytes() > 0);
    assert_eq!(browser.outstanding_change_count(), 0);
    let operation = browser.outstanding_operations()[0];
    assert!(matches!(
        browser.complete(&operation).await.unwrap(),
        PersistenceResult::ApplicationReset(Ok(_))
    ));
    assert_eq!(browser.outstanding_operation_count(), 0);
    assert!(matches!(
        browser.try_complete(&operation),
        Err(BrowserPersistenceEnqueueError::UnknownOperation)
    ));
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    RexieDriver::delete_database(&database_name).await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn operation_handles_are_driver_bound() {
    let first_name = database_name("driver-bound-first");
    let second_name = database_name("driver-bound-second");
    let mut first = RexieDriver::open(first_name.clone()).await.unwrap();
    let mut second = RexieDriver::open(second_name.clone()).await.unwrap();
    let operation = first
        .try_enqueue(PersistenceCommand::Load(RestoreRequest {
            application: application(),
            expected_schema_hash: None,
        }))
        .unwrap();

    assert!(matches!(
        second.try_complete(&operation),
        Err(BrowserPersistenceEnqueueError::CrossDriver)
    ));
    assert_eq!(first.outstanding_operation_count(), 1);
    assert_eq!(second.outstanding_operation_count(), 0);
    assert!(matches!(
        first.complete(&operation).await.unwrap(),
        PersistenceResult::Loaded(Ok(None))
    ));
    assert!(matches!(
        first
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    assert!(matches!(
        second
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    RexieDriver::delete_database(&first_name).await.unwrap();
    RexieDriver::delete_database(&second_name).await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn first_queued_commit_failure_preserves_later_outstanding_accounting() {
    let database_name = database_name("ordered-failure-accounting");
    let app = application();
    let schema_hash = [0x62; 32];
    let mut browser = RexieDriver::open(database_name.clone()).await.unwrap();
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Initialize(RestoreImage::empty(
                app.clone(),
                1,
                schema_hash,
            )))
            .await,
        PersistenceResult::Initialized(Ok(_))
    ));

    let first = browser
        .try_enqueue(PersistenceCommand::Commit(scalar_checkpoint(
            &app,
            schema_hash,
            99,
            100,
            1,
            "must-fail".to_owned(),
        )))
        .unwrap();
    let second = browser
        .try_enqueue(PersistenceCommand::Commit(scalar_checkpoint(
            &app,
            schema_hash,
            0,
            1,
            1,
            "must-commit".to_owned(),
        )))
        .unwrap();
    assert_eq!(browser.outstanding_operation_count(), 2);

    assert!(matches!(
        browser.complete(&first).await.unwrap(),
        PersistenceResult::Committed(Err(StoreError::StaleEpoch))
    ));
    assert_eq!(browser.outstanding_operation_count(), 1);
    assert!(browser.outstanding_payload_bytes() > 0);
    assert_eq!(browser.outstanding_change_count(), 1);
    assert!(matches!(
        browser.complete(&second).await.unwrap(),
        PersistenceResult::Committed(Ok(_))
    ));
    assert_eq!(browser.outstanding_operation_count(), 0);
    assert_eq!(browser.outstanding_payload_bytes(), 0);
    assert_eq!(browser.outstanding_change_count(), 0);
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    RexieDriver::delete_database(&database_name).await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn admission_is_bounded_by_retained_payload_and_change_count() {
    let database_name = database_name("payload-change-bounds");
    let app = application();
    let schema_hash = [0x63; 32];
    let mut browser = RexieDriver::open(database_name.clone()).await.unwrap();
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Initialize(RestoreImage::empty(
                app.clone(),
                1,
                schema_hash,
            )))
            .await,
        PersistenceResult::Initialized(Ok(_))
    ));

    browser.set_admission_limits(900, 10).unwrap();
    assert!(matches!(
        browser.try_enqueue(PersistenceCommand::Commit(scalar_checkpoint(
            &app,
            schema_hash,
            0,
            1,
            1,
            "x".repeat(1_000),
        ))),
        Err(BrowserPersistenceEnqueueError::PayloadTooLarge { .. })
    ));
    let payload_operation = browser
        .try_enqueue(PersistenceCommand::Commit(scalar_checkpoint(
            &app,
            schema_hash,
            0,
            1,
            1,
            "x".repeat(400),
        )))
        .unwrap();
    assert!(matches!(
        browser.try_enqueue(PersistenceCommand::Commit(scalar_checkpoint(
            &app,
            schema_hash,
            1,
            2,
            2,
            "y".repeat(400),
        ))),
        Err(BrowserPersistenceEnqueueError::PayloadBackpressure { .. })
    ));
    assert!(matches!(
        browser.complete(&payload_operation).await.unwrap(),
        PersistenceResult::Committed(Ok(_))
    ));

    browser.set_admission_limits(64 * 1024, 1).unwrap();
    let mut oversized_changes = scalar_checkpoint(&app, schema_hash, 1, 2, 2, "first".to_owned());
    oversized_changes.changes.push(DurableChange::SetScalar {
        memory_id: memory("second-admission", MemoryKind::Scalar),
        value: StoredScalar {
            touched: true,
            value: StoredValue::Text("second".to_owned()),
        },
    });
    oversized_changes = oversized_changes.seal();
    assert!(matches!(
        browser.try_enqueue(PersistenceCommand::Commit(oversized_changes)),
        Err(BrowserPersistenceEnqueueError::ChangeCountTooLarge { .. })
    ));
    let change_operation = browser
        .try_enqueue(PersistenceCommand::Commit(scalar_checkpoint(
            &app,
            schema_hash,
            1,
            2,
            2,
            "one".to_owned(),
        )))
        .unwrap();
    assert!(matches!(
        browser.try_enqueue(PersistenceCommand::Commit(scalar_checkpoint(
            &app,
            schema_hash,
            2,
            3,
            3,
            "two".to_owned(),
        ))),
        Err(BrowserPersistenceEnqueueError::ChangeBackpressure { .. })
    ));
    assert!(matches!(
        browser.complete(&change_operation).await.unwrap(),
        PersistenceResult::Committed(Ok(_))
    ));
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    RexieDriver::delete_database(&database_name).await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn large_indexeddb_restore_yields_in_bounded_slices_before_runtime_build() {
    const ROW_COUNT: u64 = 4_096;
    let database_name = database_name("large-cooperative-restore");
    let app = ApplicationIdentity::new("dev.boon.web-large-restore", "browser-test", "indexeddb");
    let schema_hash = [0x64; 32];
    let list_memory = memory("large-restore-rows", MemoryKind::List);
    let rows = (0..ROW_COUNT)
        .map(|key| StoredRow {
            key,
            generation: 1,
            source_order_token: (u128::from(key) + 1) << 64,
            owner: DurableOwner {
                ancestors: vec![DurableRowId {
                    list_memory_id: list_memory,
                    row_key: key,
                    row_generation: 1,
                }],
            },
            materialization_origin: None,
            fields: BTreeMap::new(),
            touched_fields: BTreeSet::new(),
        })
        .collect();
    let mut image = RestoreImage::empty(app.clone(), 1, schema_hash);
    image.lists.insert(
        list_memory,
        StoredList {
            touched: true,
            revision: 1,
            next_key: ROW_COUNT,
            next_order_token: (u128::from(ROW_COUNT) + 1) << 64,
            rows,
        },
    );

    let mut browser = RexieDriver::open(database_name.clone()).await.unwrap();
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Initialize(image.clone()))
            .await,
        PersistenceResult::Initialized(Ok(_))
    ));
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));

    let mut browser = RexieDriver::open(database_name.clone()).await.unwrap();
    let heartbeat_count = Rc::new(Cell::new(0u64));
    let heartbeat_running = Rc::new(Cell::new(true));
    let heartbeat_count_task = Rc::clone(&heartbeat_count);
    let heartbeat_running_task = Rc::clone(&heartbeat_running);
    spawn_local(async move {
        while heartbeat_running_task.get() {
            gloo_timers::future::TimeoutFuture::new(0).await;
            heartbeat_count_task.set(heartbeat_count_task.get().saturating_add(1));
        }
    });
    let mut loading = Box::pin(browser.execute(PersistenceCommand::Load(RestoreRequest {
        application: app,
        expected_schema_hash: Some(schema_hash),
    })));
    assert!(matches!(futures::poll!(loading.as_mut()), Poll::Pending));
    drop(loading);
    assert_eq!(browser.outstanding_operation_count(), 1);
    let operation = browser.outstanding_operations()[0];
    let restored = browser.complete(&operation).await.unwrap();
    assert_eq!(browser.outstanding_operation_count(), 0);
    heartbeat_running.set(false);
    gloo_timers::future::TimeoutFuture::new(0).await;
    assert!(heartbeat_count.get() > 1);
    assert_eq!(restored, PersistenceResult::Loaded(Ok(Some(image))));
    let status = browser.storage_status();
    assert!(status.restore_record_count >= ROW_COUNT);
    assert!(status.restore_slice_count > 1);
    assert!(status.restore_yield_count > 1);
    assert!(status.restore_max_slice_records <= 128);
    assert!(status.restore_max_slice_bytes > 0);
    assert!(status.restore_max_slice_us > 0);
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));
    RexieDriver::delete_database(&database_name).await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn malformed_raw_metadata_fails_closed_before_restore_publication() {
    const DATABASE_VERSION: u32 = 4;
    const STORES: [&str; 10] = [
        "meta",
        "slots",
        "lists",
        "rows",
        "checkpoints",
        "migrations",
        "outbox",
        "blobs",
        "artifacts",
        "artifact_owners",
    ];

    let database_name = format!(
        "boon-persistence-corrupt-meta-{}-{}",
        js_sys::Date::now() as u64,
        NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed)
    );
    let app = ApplicationIdentity::new("dev.boon.web-corrupt", "metadata", "browser");
    let mut browser = RexieDriver::open(database_name.clone()).await.unwrap();
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Initialize(RestoreImage::empty(
                app.clone(),
                1,
                [0x6c; 32],
            )))
            .await,
        PersistenceResult::Initialized(Ok(_))
    ));
    assert!(matches!(
        browser
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await,
        PersistenceResult::ShutdownComplete(Ok(_))
    ));

    let mut builder = Rexie::builder(&database_name).version(DATABASE_VERSION);
    for store in STORES {
        builder = builder.add_object_store(ObjectStore::new(store));
    }
    let database = builder.build().await.unwrap();
    let transaction = database
        .transaction(&["meta"], TransactionMode::ReadWrite)
        .unwrap();
    let meta = transaction.store("meta").unwrap();
    let entries = meta.scan(None, Some(1), None, None).await.unwrap();
    let (key, _) = entries
        .into_iter()
        .next()
        .expect("initialized application metadata");
    let malformed = Uint8Array::from(&[0xff_u8, 0x00, 0x7f][..]).into();
    meta.put(&malformed, Some(&key)).await.unwrap();
    transaction.commit().await.unwrap();
    database.close();

    let mut browser = RexieDriver::open(database_name.clone()).await.unwrap();
    let result = browser
        .execute(PersistenceCommand::Load(RestoreRequest {
            application: app,
            expected_schema_hash: None,
        }))
        .await;
    assert!(matches!(
        result,
        PersistenceResult::Loaded(Err(StoreError::Backend(ref detail)))
            if detail.contains("durable CBOR") || detail.contains("corrupt durable state")
    ));

    drop(browser);
    gloo_timers::future::TimeoutFuture::new(0).await;
    RexieDriver::delete_database(&database_name).await.unwrap();
}
