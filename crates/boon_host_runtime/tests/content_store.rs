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
    let first = store.insert_bytes(b"waveform").unwrap();
    let second = store.insert_bytes(b"waveform").unwrap();

    assert_eq!(first, second);
    assert_eq!(store.entry_count(), 1);
    assert_eq!(store.stored_bytes(), 8);
    assert_eq!(
        fs::read(store.resolve(first).unwrap().path()).unwrap(),
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
    let first = store.insert_bytes(b"first").unwrap();
    let lease = store.resolve(first).unwrap();
    let error = store.insert_bytes(b"second").unwrap_err();
    assert_eq!(error.kind(), ContentStoreErrorKind::Capacity);
    assert!(store.contains(first));

    drop(lease);
    let second = store.insert_bytes(b"second").unwrap();
    assert!(!store.contains(first));
    assert!(store.contains(second));
    assert_eq!(store.entry_count(), 1);
}

#[test]
fn writer_rejects_a_descriptor_that_disagrees_with_written_bytes() {
    let (_root, store) = store(1, 32);
    let mut writer = store.begin_write(4).unwrap();
    writer.write_chunk(b"data").unwrap();
    let wrong = ContentRef::new(<[u8; 32]>::from(Sha256::digest(b"other")), 5);
    let error = writer.finish(wrong).unwrap_err();
    assert_eq!(error.kind(), ContentStoreErrorKind::InvalidReference);
    assert_eq!(store.pending_writer_count(), 0);
    assert_eq!(store.entry_count(), 0);
}
