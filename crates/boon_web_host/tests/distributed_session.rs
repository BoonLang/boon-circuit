use boon_runtime::{DistributedClientUpdate, DistributedRuntimeError};
use boon_web_host::{
    DISTRIBUTED_SESSION_STORAGE_KEY_BYTES, DISTRIBUTED_SESSION_TOKEN_JOURNAL_MAX_BYTES,
    DistributedSessionClientRuntime, DistributedSessionHandshake, DistributedSessionHandshakeError,
    DistributedSessionHandshakePhase, DistributedSessionHandshakeStep, DistributedSessionIdentity,
    DistributedSessionJournalStore, DistributedSessionSocketAdmission,
    DistributedSessionSocketError, DistributedSessionSocketLimits, DistributedSessionSocketOwner,
    DistributedSessionSocketPhase, DistributedSessionStorageError,
};
use boon_wire::{
    ResumeToken, SESSION_CONTROL_MAX_FRAME_BYTES, ServerOffer, ServerReady, ServerReject,
    ServerRevoked, SessionControlFrame, SessionId, decode_session_control_frame,
    encode_session_control_frame,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

const PACKAGE_ID: &str = "dev.boon.distributed-session-test";
const GRAPH_ID: [u8; 32] = [0x11; 32];
const GRAPH_REVISION: u64 = 7;
const SCHEMA_HASH: [u8; 32] = [0x22; 32];
const TEST_SESSION_ID: SessionId = SessionId::from_bytes([0x5a; 32]);

#[derive(Default)]
struct MemoryState {
    values: BTreeMap<String, String>,
    fail_reads: bool,
    fail_writes: bool,
    fail_removes: bool,
}

#[derive(Clone, Default)]
struct MemoryStore(Rc<RefCell<MemoryState>>);

impl MemoryStore {
    fn value(&self, key: &str) -> Option<String> {
        self.0.borrow().values.get(key).cloned()
    }

    fn insert(&self, key: &str, value: &str) {
        self.0
            .borrow_mut()
            .values
            .insert(key.to_owned(), value.to_owned());
    }
}

impl DistributedSessionJournalStore for MemoryStore {
    fn read(&mut self, key: &str) -> Result<Option<String>, DistributedSessionStorageError> {
        let state = self.0.borrow();
        if state.fail_reads {
            return Err(DistributedSessionStorageError::platform(
                "read",
                "injected failure",
            ));
        }
        Ok(state.values.get(key).cloned())
    }

    fn write(&mut self, key: &str, value: &str) -> Result<(), DistributedSessionStorageError> {
        let mut state = self.0.borrow_mut();
        if state.fail_writes {
            return Err(DistributedSessionStorageError::platform(
                "write",
                "injected failure",
            ));
        }
        state.values.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<(), DistributedSessionStorageError> {
        let mut state = self.0.borrow_mut();
        if state.fail_removes {
            return Err(DistributedSessionStorageError::platform(
                "remove",
                "injected failure",
            ));
        }
        state.values.remove(key);
        Ok(())
    }
}

fn test_identity() -> DistributedSessionIdentity {
    DistributedSessionIdentity::new(PACKAGE_ID, GRAPH_ID, GRAPH_REVISION, SCHEMA_HASH).unwrap()
}

fn lookup_text(byte: u8) -> String {
    ResumeToken::from_bytes([byte; 32])
        .to_lookup_key()
        .as_storage_str()
        .to_owned()
}

fn journal_text(current: Option<u8>, pending: Option<u8>) -> String {
    let current = current.map_or_else(|| "-".to_owned(), lookup_text);
    let pending = pending.map_or_else(|| "-".to_owned(), lookup_text);
    format!("2:{current}:{pending}")
}

fn offer(byte: u8, generation: u64) -> Vec<u8> {
    encode_session_control_frame(&SessionControlFrame::ServerOffer(ServerOffer::new(
        ResumeToken::from_bytes([byte; 32]),
        TEST_SESSION_ID,
        generation,
        0,
    )))
    .unwrap()
}

fn ready(generation: u64) -> Vec<u8> {
    encode_session_control_frame(&SessionControlFrame::ServerReady(ServerReady::new(
        TEST_SESSION_ID,
        generation,
        0,
    )))
    .unwrap()
}

fn reject() -> Vec<u8> {
    encode_session_control_frame(&SessionControlFrame::ServerReject(ServerReject::new())).unwrap()
}

fn revoked() -> Vec<u8> {
    encode_session_control_frame(&SessionControlFrame::ServerRevoked(ServerRevoked::new())).unwrap()
}

fn hello_token(bytes: &[u8]) -> Option<[u8; 32]> {
    let SessionControlFrame::ClientHello(hello) = decode_session_control_frame(bytes).unwrap()
    else {
        panic!("expected ClientHello");
    };
    assert_eq!(hello.graph_id(), &GRAPH_ID);
    assert_eq!(hello.graph_revision(), GRAPH_REVISION);
    assert_eq!(hello.schema_hash(), &SCHEMA_HASH);
    hello.resume_token().map(|token| *token.as_bytes())
}

fn start_handshake(
    identity: DistributedSessionIdentity,
    store: MemoryStore,
) -> Result<(DistributedSessionHandshake<MemoryStore>, Vec<u8>), DistributedSessionHandshakeError> {
    DistributedSessionHandshake::start(identity, store, 0)
}

fn make_current(
    store: MemoryStore,
    current: u8,
    offered: u8,
) -> (DistributedSessionHandshake<MemoryStore>, String) {
    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    store.insert(&storage_key, &journal_text(Some(current), None));
    let (mut handshake, hello) = start_handshake(identity, store).unwrap();
    assert_eq!(hello_token(&hello), Some([current; 32]));
    handshake.accept_server_frame(&offer(offered, 9)).unwrap();
    assert!(matches!(
        handshake.accept_server_frame(&ready(9)).unwrap(),
        DistributedSessionHandshakeStep::Current
    ));
    (handshake, storage_key)
}

#[test]
fn fresh_handshake_uses_exact_identity_and_one_bounded_storage_key() {
    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    let (handshake, hello_bytes) = start_handshake(identity, MemoryStore::default()).unwrap();

    assert_eq!(
        handshake.phase(),
        DistributedSessionHandshakePhase::AwaitingOffer
    );
    assert_eq!(handshake.storage_key(), storage_key);
    assert_eq!(storage_key.len(), DISTRIBUTED_SESSION_STORAGE_KEY_BYTES);
    assert!(storage_key.starts_with("boon.session.v2."));
    assert!(hello_bytes.len() <= SESSION_CONTROL_MAX_FRAME_BYTES);
    assert_eq!(hello_token(&hello_bytes), None);

    let SessionControlFrame::ClientHello(hello) =
        decode_session_control_frame(&hello_bytes).unwrap()
    else {
        panic!("expected ClientHello");
    };
    assert_eq!(hello.graph_id(), &GRAPH_ID);
    assert_eq!(hello.graph_revision(), GRAPH_REVISION);
    assert_eq!(hello.schema_hash(), &SCHEMA_HASH);
}

#[test]
fn journal_is_strict_canonical_and_pending_is_attempted_first() {
    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    let store = MemoryStore::default();
    let journal = journal_text(Some(0x33), Some(0x44));
    assert!(journal.len() <= DISTRIBUTED_SESSION_TOKEN_JOURNAL_MAX_BYTES);
    store.insert(&storage_key, &journal);

    let (_handshake, hello) = start_handshake(identity, store.clone()).unwrap();
    assert_eq!(hello_token(&hello), Some([0x44; 32]));
    assert_eq!(store.value(&storage_key).as_deref(), Some(journal.as_str()));

    let identity = test_identity();
    store.insert(&storage_key, &lookup_text(0x55));
    let (_handshake, hello) = start_handshake(identity, store.clone()).unwrap();
    assert_eq!(hello_token(&hello), None);
    assert_eq!(store.value(&storage_key), None);
}

#[test]
fn crash_after_journal_write_before_ready_resumes_with_pending_key() {
    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    let store = MemoryStore::default();
    store.insert(&storage_key, &journal_text(Some(0x44), None));
    let (mut handshake, _) = start_handshake(identity, store.clone()).unwrap();

    let DistributedSessionHandshakeStep::SendClientFrame(commit) =
        handshake.accept_server_frame(&offer(0x55, 1)).unwrap()
    else {
        panic!("offer must produce ClientCommit");
    };
    assert!(matches!(
        decode_session_control_frame(&commit).unwrap(),
        SessionControlFrame::ClientCommit(_)
    ));
    assert_eq!(
        store.value(&storage_key).as_deref(),
        Some(journal_text(Some(0x44), Some(0x55)).as_str())
    );

    drop(handshake);
    let (_restarted, hello) = start_handshake(test_identity(), store).unwrap();
    assert_eq!(hello_token(&hello), Some([0x55; 32]));
}

#[test]
fn rejected_pending_falls_back_to_current_once() {
    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    let store = MemoryStore::default();
    store.insert(&storage_key, &journal_text(Some(0x61), Some(0x62)));
    let (mut handshake, hello) = start_handshake(identity, store.clone()).unwrap();
    assert_eq!(hello_token(&hello), Some([0x62; 32]));

    let DistributedSessionHandshakeStep::SendClientFrame(current_hello) =
        handshake.accept_server_frame(&reject()).unwrap()
    else {
        panic!("pending rejection must retry current");
    };
    assert_eq!(hello_token(&current_hello), Some([0x61; 32]));
    assert_eq!(
        store.value(&storage_key).as_deref(),
        Some(journal_text(Some(0x61), None).as_str())
    );

    let DistributedSessionHandshakeStep::SendClientFrame(fresh_hello) =
        handshake.accept_server_frame(&reject()).unwrap()
    else {
        panic!("current rejection must make one fresh attempt");
    };
    assert_eq!(hello_token(&fresh_hello), None);
    assert_eq!(store.value(&storage_key), None);
    assert!(matches!(
        handshake.accept_server_frame(&reject()).unwrap(),
        DistributedSessionHandshakeStep::Rejected
    ));
}

#[test]
fn expired_current_token_gets_one_fresh_retry_then_rejection_is_terminal() {
    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    let store = MemoryStore::default();
    store.insert(&storage_key, &journal_text(Some(0x71), None));
    let (mut handshake, hello) = start_handshake(identity, store.clone()).unwrap();
    assert_eq!(hello_token(&hello), Some([0x71; 32]));

    let DistributedSessionHandshakeStep::SendClientFrame(fresh_hello) =
        handshake.accept_server_frame(&reject()).unwrap()
    else {
        panic!("expired current token must get one fresh retry");
    };
    assert_eq!(hello_token(&fresh_hello), None);
    assert_eq!(store.value(&storage_key), None);

    assert!(matches!(
        handshake.accept_server_frame(&reject()).unwrap(),
        DistributedSessionHandshakeStep::Rejected
    ));
    assert_eq!(
        handshake.phase(),
        DistributedSessionHandshakePhase::Rejected
    );
    assert_eq!(store.value(&storage_key), None);
}

#[test]
fn journal_write_failure_before_commit_sends_nothing_and_preserves_current() {
    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    let store = MemoryStore::default();
    let current = journal_text(Some(0x81), None);
    store.insert(&storage_key, &current);
    let (mut handshake, _) = start_handshake(identity, store.clone()).unwrap();
    store.0.borrow_mut().fail_writes = true;

    let error = handshake
        .accept_server_frame(&offer(0x82, 1))
        .err()
        .expect("journal write must precede ClientCommit");
    assert!(matches!(
        error,
        DistributedSessionHandshakeError::Storage(_)
    ));
    assert_eq!(handshake.phase(), DistributedSessionHandshakePhase::Failed);
    assert_eq!(store.value(&storage_key).as_deref(), Some(current.as_str()));
}

#[test]
fn server_ready_atomically_collapses_pending_to_current() {
    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    let store = MemoryStore::default();
    store.insert(&storage_key, &journal_text(Some(0x91), None));
    let (mut handshake, _) = start_handshake(identity, store.clone()).unwrap();
    handshake.accept_server_frame(&offer(0x92, 17)).unwrap();
    assert_eq!(
        store.value(&storage_key).as_deref(),
        Some(journal_text(Some(0x91), Some(0x92)).as_str())
    );

    assert!(matches!(
        handshake.accept_server_frame(&ready(17)).unwrap(),
        DistributedSessionHandshakeStep::Current
    ));
    assert_eq!(handshake.generation(), Some(17));
    assert_eq!(
        store.value(&storage_key).as_deref(),
        Some(journal_text(Some(0x92), None).as_str())
    );
}

#[test]
fn revoke_frame_loss_preserves_the_journal() {
    let store = MemoryStore::default();
    let (mut handshake, storage_key) = make_current(store.clone(), 0xa1, 0xa2);
    let journal = journal_text(Some(0xa2), None);

    let revoke = handshake.revoke().unwrap();
    assert!(matches!(
        decode_session_control_frame(&revoke).unwrap(),
        SessionControlFrame::ClientRevoke(_)
    ));
    assert_eq!(
        handshake.phase(),
        DistributedSessionHandshakePhase::AwaitingRevoke
    );
    assert_eq!(handshake.generation(), Some(9));
    assert_eq!(store.value(&storage_key).as_deref(), Some(journal.as_str()));

    drop(handshake);
    let (_restarted, hello) = start_handshake(test_identity(), store).unwrap();
    assert_eq!(hello_token(&hello), Some([0xa2; 32]));
}

#[test]
fn server_revoked_acknowledgement_clears_journal_and_completes_revocation() {
    let store = MemoryStore::default();
    let (mut handshake, storage_key) = make_current(store.clone(), 0xb1, 0xb2);
    handshake.revoke().unwrap();

    assert!(matches!(
        handshake.accept_server_frame(&revoked()).unwrap(),
        DistributedSessionHandshakeStep::Revoked
    ));
    assert_eq!(handshake.phase(), DistributedSessionHandshakePhase::Revoked);
    assert_eq!(handshake.generation(), None);
    assert_eq!(store.value(&storage_key), None);
}

#[test]
fn storage_and_ordering_failures_fail_closed_without_compatibility_fallbacks() {
    let store = MemoryStore::default();
    store.0.borrow_mut().fail_reads = true;
    let error = start_handshake(test_identity(), store)
        .err()
        .expect("sessionStorage read failure must prevent startup");
    assert!(matches!(
        error,
        DistributedSessionHandshakeError::Storage(_)
    ));

    let identity = test_identity();
    let storage_key = identity.storage_key().to_owned();
    let store = MemoryStore::default();
    store.insert(&storage_key, "1:obsolete:journal");
    store.0.borrow_mut().fail_removes = true;
    let error = start_handshake(identity, store)
        .err()
        .expect("invalid journal removal failure must prevent startup");
    assert!(matches!(
        error,
        DistributedSessionHandshakeError::Storage(_)
    ));

    let (mut wrong_order, _) = start_handshake(test_identity(), MemoryStore::default()).unwrap();
    let error = wrong_order
        .accept_server_frame(&ready(1))
        .err()
        .expect("out-of-order ServerReady must fail");
    assert!(matches!(
        error,
        DistributedSessionHandshakeError::UnexpectedControlFrame(
            DistributedSessionHandshakePhase::AwaitingOffer
        )
    ));
    assert_eq!(
        wrong_order.phase(),
        DistributedSessionHandshakePhase::Failed
    );
}

#[test]
fn revoke_is_rejected_until_current() {
    let (mut handshake, _) = start_handshake(test_identity(), MemoryStore::default()).unwrap();
    assert!(matches!(
        handshake.revoke(),
        Err(DistributedSessionHandshakeError::Closed(
            DistributedSessionHandshakePhase::AwaitingOffer
        ))
    ));
}

#[test]
fn identity_validation_rejects_unbounded_or_ambiguous_keys() {
    assert!(DistributedSessionIdentity::new("", GRAPH_ID, 1, SCHEMA_HASH).is_err());
    assert!(DistributedSessionIdentity::new("bad key", GRAPH_ID, 1, SCHEMA_HASH).is_err());
    assert!(DistributedSessionIdentity::new(PACKAGE_ID, GRAPH_ID, 0, SCHEMA_HASH).is_err());

    let first = test_identity();
    let second = DistributedSessionIdentity::new(PACKAGE_ID, GRAPH_ID, 8, SCHEMA_HASH).unwrap();
    assert_ne!(first.storage_key(), second.storage_key());
}

#[derive(Default)]
struct FakeClientRuntime {
    bound_generations: Vec<u64>,
    bound_session_ids: Vec<[u8; 32]>,
    bound_applied_client_through: Vec<u64>,
    current_count: usize,
    stale_count: usize,
    accepted_frames: Vec<Vec<u8>>,
    outbound: VecDeque<Vec<u8>>,
    leased: Option<Vec<u8>>,
    acknowledgements: usize,
    queue_on_current: Option<Vec<u8>>,
}

impl DistributedSessionClientRuntime for FakeClientRuntime {
    fn bind(
        &mut self,
        session_id: SessionId,
        generation: u64,
        applied_client_through: u64,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        self.bound_generations.push(generation);
        self.bound_session_ids.push(*session_id.as_bytes());
        self.bound_applied_client_through
            .push(applied_client_through);
        self.outbound.clear();
        self.leased = None;
        Ok(DistributedClientUpdate::default())
    }

    fn mark_current(&mut self) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        self.current_count += 1;
        if let Some(bytes) = self.queue_on_current.take() {
            self.outbound.push_back(bytes);
        }
        Ok(DistributedClientUpdate::default())
    }

    fn mark_stale(&mut self) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        self.stale_count += 1;
        self.outbound.clear();
        self.leased = None;
        Ok(DistributedClientUpdate::default())
    }

    fn accept_session_frame(
        &mut self,
        bytes: &[u8],
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        self.accepted_frames.push(bytes.to_vec());
        Ok(DistributedClientUpdate::default())
    }

    fn next_session_frame(&mut self) -> Result<Option<Vec<u8>>, DistributedRuntimeError> {
        if let Some(bytes) = &self.leased {
            return Ok(Some(bytes.clone()));
        }
        self.leased = self.outbound.front().cloned();
        Ok(self.leased.clone())
    }

    fn acknowledge_session_frame(&mut self) -> bool {
        if self.leased.take().is_none() || self.outbound.pop_front().is_none() {
            return false;
        }
        self.acknowledgements += 1;
        true
    }

    fn pending_session_frames(&self) -> usize {
        self.outbound.len()
    }

    fn applied_server_through(&self) -> u64 {
        0
    }
}

type TestSocketOwner = DistributedSessionSocketOwner<MemoryStore, FakeClientRuntime>;

fn test_socket_owner(runtime: FakeClientRuntime) -> TestSocketOwner {
    DistributedSessionSocketOwner::new(
        test_identity(),
        MemoryStore::default(),
        runtime,
        DistributedSessionSocketLimits::default(),
    )
    .unwrap()
}

fn open_socket(owner: &mut TestSocketOwner) -> u64 {
    let socket_epoch = owner.begin_connect().unwrap();
    assert_eq!(
        owner.socket_opened(socket_epoch).unwrap(),
        DistributedSessionSocketAdmission::Accepted
    );
    socket_epoch
}

fn leased_frame(owner: &mut TestSocketOwner, socket_epoch: u64) -> (u64, Vec<u8>) {
    let lease = owner
        .lease_outbound(socket_epoch)
        .unwrap()
        .expect("an outbound frame should be leased");
    assert_eq!(lease.socket_epoch(), socket_epoch);
    (lease.lease_id(), lease.bytes().to_vec())
}

fn acknowledge(owner: &mut TestSocketOwner, socket_epoch: u64, lease_id: u64) {
    assert_eq!(
        owner.acknowledge_outbound(socket_epoch, lease_id).unwrap(),
        DistributedSessionSocketAdmission::Accepted
    );
}

fn make_socket_current(owner: &mut TestSocketOwner, offered_token: u8, generation: u64) -> u64 {
    let socket_epoch = open_socket(owner);
    let (hello_id, hello) = leased_frame(owner, socket_epoch);
    assert_eq!(hello_token(&hello), None);
    acknowledge(owner, socket_epoch, hello_id);

    owner
        .push_inbound_binary(socket_epoch, offer(offered_token, generation))
        .unwrap();
    assert!(owner.poll_inbound().unwrap().is_some());
    let (commit_id, commit) = leased_frame(owner, socket_epoch);
    assert!(matches!(
        decode_session_control_frame(&commit).unwrap(),
        SessionControlFrame::ClientCommit(_)
    ));
    acknowledge(owner, socket_epoch, commit_id);

    owner
        .push_inbound_binary(socket_epoch, ready(generation))
        .unwrap();
    let poll = owner.poll_inbound().unwrap().unwrap();
    assert_eq!(poll.runtime_updates.len(), 2);
    assert_eq!(owner.phase(), DistributedSessionSocketPhase::Current);
    assert_eq!(owner.runtime_generation(), Some(generation));
    socket_epoch
}

#[test]
fn socket_owner_runs_binary_handshake_and_delivers_only_after_ready() {
    let mut owner = test_socket_owner(FakeClientRuntime::default());
    let socket_epoch = make_socket_current(&mut owner, 0xc1, 31);
    assert_eq!(owner.runtime().bound_generations, vec![31]);
    assert_eq!(owner.runtime().current_count, 1);

    owner
        .push_inbound_binary(socket_epoch, vec![0xde, 0xad, 0xbe, 0xef])
        .unwrap();
    let poll = owner.poll_inbound().unwrap().unwrap();
    assert_eq!(poll.runtime_updates.len(), 1);
    assert_eq!(
        owner.runtime().accepted_frames,
        vec![vec![0xde, 0xad, 0xbe, 0xef]]
    );
}

#[test]
fn failed_send_releases_nothing_and_retries_the_exact_runtime_lease() {
    let runtime = FakeClientRuntime {
        queue_on_current: Some(vec![0x10, 0x20, 0x30]),
        ..FakeClientRuntime::default()
    };
    let mut owner = test_socket_owner(runtime);
    let socket_epoch = make_socket_current(&mut owner, 0xc2, 32);

    let first = leased_frame(&mut owner, socket_epoch);
    assert_eq!(first.1, vec![0x10, 0x20, 0x30]);
    assert_eq!(owner.runtime().acknowledgements, 0);

    // A failed WebSocket.send performs no acknowledgement.
    let retry = leased_frame(&mut owner, socket_epoch);
    assert_eq!(retry, first);
    assert!(matches!(
        owner.acknowledge_outbound(socket_epoch, first.0 + 1),
        Err(DistributedSessionSocketError::InvalidOutboundLease)
    ));
    assert_eq!(leased_frame(&mut owner, socket_epoch), first);
    assert_eq!(owner.runtime().acknowledgements, 0);

    acknowledge(&mut owner, socket_epoch, first.0);
    assert_eq!(owner.runtime().acknowledgements, 1);
    assert!(owner.lease_outbound(socket_epoch).unwrap().is_none());
}

#[test]
fn reconnect_resumes_journal_and_ignores_replaced_socket_callbacks() {
    let mut owner = test_socket_owner(FakeClientRuntime::default());
    let first_epoch = make_socket_current(&mut owner, 0xc3, 33);
    let disconnected = owner.socket_disconnected(first_epoch).unwrap();
    assert_eq!(
        disconnected.admission,
        DistributedSessionSocketAdmission::Accepted
    );
    assert!(disconnected.runtime_update.is_some());
    assert_eq!(owner.runtime().stale_count, 1);
    assert_eq!(
        owner.phase(),
        DistributedSessionSocketPhase::ReconnectRequired
    );

    let second_epoch = owner.begin_connect().unwrap();
    assert!(second_epoch > first_epoch);
    owner.socket_opened(second_epoch).unwrap();
    let (_, resumed_hello) = leased_frame(&mut owner, second_epoch);
    assert_eq!(hello_token(&resumed_hello), Some([0xc3; 32]));

    assert_eq!(
        owner.push_inbound_binary(first_epoch, vec![0xff]).unwrap(),
        DistributedSessionSocketAdmission::IgnoredStaleSocket
    );
    let stale_close = owner.socket_disconnected(first_epoch).unwrap();
    assert_eq!(
        stale_close.admission,
        DistributedSessionSocketAdmission::IgnoredStaleSocket
    );
    assert_eq!(owner.active_socket_epoch(), Some(second_epoch));
    assert!(owner.runtime().accepted_frames.is_empty());
}

#[test]
fn inbound_and_outbound_queues_apply_deterministic_backpressure() {
    let limits = DistributedSessionSocketLimits {
        max_inbound_messages: 2,
        max_outbound_messages: 1,
        ..DistributedSessionSocketLimits::default()
    };
    let mut owner = DistributedSessionSocketOwner::new(
        test_identity(),
        MemoryStore::default(),
        FakeClientRuntime::default(),
        limits,
    )
    .unwrap();
    let socket_epoch = open_socket(&mut owner);

    owner
        .push_inbound_binary(socket_epoch, offer(0xd1, 1))
        .unwrap();
    owner
        .push_inbound_binary(socket_epoch, offer(0xd2, 1))
        .unwrap();
    assert!(matches!(
        owner.push_inbound_binary(socket_epoch, offer(0xd3, 1)),
        Err(DistributedSessionSocketError::QueueFull { .. })
    ));
    assert_eq!(owner.pending_inbound_frames(), 2);

    // The one-slot outbound queue still owns ClientHello, so Offer waits.
    assert!(owner.poll_inbound().unwrap().is_none());
    let (hello_id, _) = leased_frame(&mut owner, socket_epoch);
    acknowledge(&mut owner, socket_epoch, hello_id);
    assert!(owner.poll_inbound().unwrap().is_some());
    assert_eq!(owner.pending_inbound_frames(), 1);
    assert_eq!(owner.pending_browser_outbound_frames(), 1);
}

#[test]
fn text_frames_fail_protocol_validation_and_abort_marks_current_runtime_stale() {
    let mut owner = test_socket_owner(FakeClientRuntime::default());
    let socket_epoch = make_socket_current(&mut owner, 0xc4, 34);
    assert!(matches!(
        owner.reject_text_frame(socket_epoch),
        Err(DistributedSessionSocketError::TextFrame)
    ));
    let aborted = owner.abort_socket(socket_epoch).unwrap();
    assert!(aborted.runtime_update.is_some());
    assert_eq!(owner.runtime().stale_count, 1);
    assert_eq!(owner.phase(), DistributedSessionSocketPhase::Failed);
}
