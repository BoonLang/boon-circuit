use boon_host_runtime::{ContentRef, ContentStore, ContentStoreErrorKind, ContentStoreLimits};
use sha2::{Digest, Sha256};
use std::fs;

fn store(max_entries: usize, max_bytes: u64) -> (tempfile::TempDir, ContentStore) {
    let root = tempfile::tempdir().unwrap();
    let store = ContentStore::new(
        root.path().join("content"),
        ContentStoreLimits::new(max_entries, max_bytes),
    )
    .unwrap();
    (root, store)
}

#[test]
fn insertion_is_content_addressed_and_deduplicated() {
    let (_root, store) = store(2, 1024);
    let first = store
        .insert_bytes(b"waveform", "application/vnd.test.waveform")
        .unwrap();
    let second = store
        .insert_bytes(b"waveform", "application/vnd.test.waveform")
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(first.size(), 8);
    assert_eq!(first.media(), "application/vnd.test.waveform");
    assert_eq!(store.entry_count(), 1);
    assert_eq!(store.stored_bytes(), 8);
    assert_eq!(
        fs::read(store.resolve(&first).unwrap().path()).unwrap(),
        b"waveform"
    );
}

#[test]
fn abandoned_writer_releases_reservations_and_partial_file() {
    let (root, store) = store(1, 32);
    let content_root = root.path().join("content");
    {
        let mut writer = store.begin_write(8).unwrap();
        writer.write_chunk(b"partial").unwrap();
        assert_eq!(store.pending_writer_count(), 1);
    }

    assert_eq!(store.pending_writer_count(), 0);
    assert_eq!(store.entry_count(), 0);
    assert!(fs::read_dir(content_root).unwrap().next().is_none());
}

#[test]
fn leases_pin_entries_and_unpinned_lru_content_is_evicted() {
    let (_root, store) = store(1, 32);
    let first = store
        .insert_bytes(b"first", "application/octet-stream")
        .unwrap();
    let lease = store.resolve(&first).unwrap();
    let error = store
        .insert_bytes(b"second", "application/octet-stream")
        .unwrap_err();
    assert_eq!(error.kind(), ContentStoreErrorKind::Capacity);
    assert!(store.contains(&first));

    drop(lease);
    let second = store
        .insert_bytes(b"second", "application/octet-stream")
        .unwrap();
    assert!(!store.contains(&first));
    assert!(store.contains(&second));
    assert_eq!(store.entry_count(), 1);
}

#[test]
fn durable_roots_prevent_eviction_and_update_atomically() {
    let (_root, store) = store(1, 32);
    let first = store
        .insert_bytes(b"first", "application/octet-stream")
        .unwrap();
    store.replace_durable_roots([first.clone()]).unwrap();
    assert_eq!(store.durable_root_count(), 1);
    assert!(!store.remove(&first).unwrap());

    let error = store
        .insert_bytes(b"second", "application/octet-stream")
        .unwrap_err();
    assert_eq!(error.kind(), ContentStoreErrorKind::Capacity);
    assert!(store.contains(&first));

    let missing = ContentRef::new([9; 32], 3, "application/octet-stream").unwrap();
    assert_eq!(
        store.replace_durable_roots([missing]).unwrap_err().kind(),
        ContentStoreErrorKind::Missing
    );
    assert_eq!(store.durable_root_count(), 1);

    store.replace_durable_roots([]).unwrap();
    let second = store
        .insert_bytes(b"second", "application/octet-stream")
        .unwrap();
    assert!(!store.contains(&first));
    assert!(store.contains(&second));
}

#[test]
fn writer_rejects_a_descriptor_that_disagrees_with_written_bytes() {
    let (_root, store) = store(1, 32);
    let mut writer = store.begin_write(4).unwrap();
    writer.write_chunk(b"data").unwrap();
    let wrong = ContentRef::new(
        <[u8; 32]>::from(Sha256::digest(b"other")),
        5,
        "application/octet-stream",
    )
    .unwrap();
    let error = writer.finish(wrong).unwrap_err();
    assert_eq!(error.kind(), ContentStoreErrorKind::InvalidReference);
    assert_eq!(store.pending_writer_count(), 0);
    assert_eq!(store.entry_count(), 0);
}

#[test]
fn completed_content_survives_store_restart_and_round_trips_its_descriptor() {
    let root = tempfile::tempdir().unwrap();
    let content_root = root.path().join("content");
    let content = {
        let store = ContentStore::new(&content_root, ContentStoreLimits::new(2, 1024)).unwrap();
        let content = store
            .insert_bytes(b"durable", "application/vnd.test.durable")
            .unwrap();
        assert_eq!(
            ContentRef::from_value(&content.value().unwrap()).unwrap(),
            content
        );
        content
    };

    let reopened = ContentStore::new(&content_root, ContentStoreLimits::new(2, 1024)).unwrap();
    assert!(reopened.contains(&content));
    assert_eq!(
        fs::read(reopened.resolve(&content).unwrap().path()).unwrap(),
        b"durable"
    );
}

#[test]
fn recovered_durable_roots_are_bound_before_the_store_accepts_new_writes() {
    let root = tempfile::tempdir().unwrap();
    let content_root = root.path().join("content");
    let content = {
        let store = ContentStore::new(&content_root, ContentStoreLimits::new(1, 32)).unwrap();
        store
            .insert_bytes(b"durable", "application/octet-stream")
            .unwrap()
    };

    let reopened = ContentStore::new_with_durable_roots(
        &content_root,
        ContentStoreLimits::new(1, 32),
        [content.clone()],
    )
    .unwrap();
    assert_eq!(reopened.durable_root_count(), 1);
    assert_eq!(
        reopened
            .insert_bytes(b"replacement", "application/octet-stream")
            .unwrap_err()
            .kind(),
        ContentStoreErrorKind::Capacity
    );
    assert!(reopened.contains(&content));
}

#[test]
fn recovered_content_is_digest_checked_before_first_lease() {
    let root = tempfile::tempdir().unwrap();
    let content_root = root.path().join("content");
    let (content, path) = {
        let store = ContentStore::new(&content_root, ContentStoreLimits::new(2, 1024)).unwrap();
        let content = store
            .insert_bytes(b"original", "application/octet-stream")
            .unwrap();
        let path = store.resolve(&content).unwrap().path().to_path_buf();
        (content, path)
    };
    fs::write(path, b"tampered").unwrap();

    let reopened = ContentStore::new(&content_root, ContentStoreLimits::new(2, 1024)).unwrap();
    let error = reopened.resolve(&content).err().unwrap();
    assert_eq!(error.kind(), ContentStoreErrorKind::Corrupt);
    assert!(!reopened.contains(&content));
}

#[test]
fn verified_reimport_atomically_repairs_a_corrupt_recovered_entry() {
    let root = tempfile::tempdir().unwrap();
    let content_root = root.path().join("content");
    let (content, path) = {
        let store = ContentStore::new(&content_root, ContentStoreLimits::new(2, 1024)).unwrap();
        let content = store
            .insert_bytes(b"original", "application/octet-stream")
            .unwrap();
        let path = store.resolve(&content).unwrap().path().to_path_buf();
        (content, path)
    };
    fs::write(&path, b"tampered").unwrap();

    let reopened = ContentStore::new(&content_root, ContentStoreLimits::new(2, 1024)).unwrap();
    assert_eq!(
        reopened.resolve(&content).err().unwrap().kind(),
        ContentStoreErrorKind::Corrupt
    );
    let repaired = reopened
        .insert_bytes(b"original", "application/octet-stream")
        .unwrap();

    assert_eq!(repaired, content);
    assert_eq!(
        fs::read(reopened.resolve(&content).unwrap().path()).unwrap(),
        b"original"
    );
    assert_eq!(reopened.entry_count(), 1);
}

#[test]
fn restart_removes_abandoned_partial_materializations() {
    let root = tempfile::tempdir().unwrap();
    let content_root = root.path().join("content");
    fs::create_dir_all(&content_root).unwrap();
    fs::write(content_root.join(".partial-abandoned"), b"partial").unwrap();

    let store = ContentStore::new(&content_root, ContentStoreLimits::new(2, 1024)).unwrap();
    assert_eq!(store.entry_count(), 0);
    assert!(fs::read_dir(content_root).unwrap().next().is_none());
}
