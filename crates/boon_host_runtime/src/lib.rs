//! Typed runtime adapter for bounded clock, random, secret, HMAC, and timer services.

#![forbid(unsafe_code)]

mod content_store;

pub use content_store::{
    ContentLease, ContentRef, ContentStore, ContentStoreError, ContentStoreErrorKind,
    ContentStoreLimits, ContentWriter,
};

use boon_host_services::{
    CancellationHandle, HMAC_SHA256_TAG_BYTES, HmacSha256Tag, HostServiceError, HostServices,
    SecretMaterial, SecretRef, TimerReceiveError,
};
use boon_plan::{EffectDeliveryCardinality, EffectId, EffectInvocationId, FiniteReal};
use boon_runtime::{
    ProgramSession, RuntimeTurn, TransientEffectCallId, TransientEffectCreditGrant,
    TransientEffectInvocation, Value,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle as TokioJoinHandle;

const MAX_SECRET_NAME_BYTES: usize = 128;
const MAX_DIAGNOSTIC_BYTES: usize = 1024;
const MAX_PACKAGE_ASSET_URL_BYTES: usize = 1024;

pub struct NamedSecret {
    name: String,
    material: SecretMaterial,
}

impl NamedSecret {
    pub fn new(name: impl Into<String>, material: SecretMaterial) -> Self {
        Self {
            name: name.into(),
            material,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterErrorKind {
    InvalidConfiguration,
    InvalidDelivery,
    NotOwned,
    Capacity,
    Capability,
    Worker,
    Runtime,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterError {
    kind: AdapterErrorKind,
    diagnostic: String,
}

impl AdapterError {
    fn new(kind: AdapterErrorKind, diagnostic: impl fmt::Display) -> Self {
        Self {
            kind,
            diagnostic: bounded_diagnostic(diagnostic.to_string()),
        }
    }

    pub const fn kind(&self) -> AdapterErrorKind {
        self.kind
    }

    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl std::error::Error for AdapterError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostServiceEffectCompletion {
    pub call_id: TransientEffectCallId,
    pub invocation_id: EffectInvocationId,
    pub outcome: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostServiceEffectSubmission {
    pub call_id: TransientEffectCallId,
    pub immediate_completion: Option<HostServiceEffectCompletion>,
}

#[derive(Clone, Copy)]
struct EffectIds {
    wall_clock: EffectId,
    secure_random: EffectId,
    secret_verify: EffectId,
    hmac_sign: EffectId,
    hmac_verify: EffectId,
    deadline: EffectId,
}

impl EffectIds {
    fn new() -> Result<Self, AdapterError> {
        Ok(Self {
            wall_clock: effect_id(boon_effect_schema::WALL_CLOCK_READ_OPERATION)?,
            secure_random: effect_id(boon_effect_schema::SECURE_RANDOM_BYTES_OPERATION)?,
            secret_verify: effect_id(boon_effect_schema::SECRET_VERIFY_OPERATION)?,
            hmac_sign: effect_id(boon_effect_schema::HMAC_SHA256_SIGN_OPERATION)?,
            hmac_verify: effect_id(boon_effect_schema::HMAC_SHA256_VERIFY_OPERATION)?,
            deadline: effect_id(boon_effect_schema::TIMER_DEADLINE_OPERATION)?,
        })
    }

    fn operation(self, effect_id: EffectId) -> Option<Operation> {
        if effect_id == self.wall_clock {
            Some(Operation::WallClock)
        } else if effect_id == self.secure_random {
            Some(Operation::SecureRandom)
        } else if effect_id == self.secret_verify {
            Some(Operation::SecretVerify)
        } else if effect_id == self.hmac_sign {
            Some(Operation::HmacSign)
        } else if effect_id == self.hmac_verify {
            Some(Operation::HmacVerify)
        } else if effect_id == self.deadline {
            Some(Operation::Deadline)
        } else {
            None
        }
    }

    fn all(self) -> [EffectId; 6] {
        [
            self.wall_clock,
            self.secure_random,
            self.secret_verify,
            self.hmac_sign,
            self.hmac_verify,
            self.deadline,
        ]
    }
}

#[derive(Clone, Copy)]
enum Operation {
    WallClock,
    SecureRandom,
    SecretVerify,
    HmacSign,
    HmacVerify,
    Deadline,
}

struct ActiveDeadline {
    invocation_id: EffectInvocationId,
    cancellation: CancellationHandle,
    task: TokioJoinHandle<()>,
}

/// Owns the process-local host services and exposes only centrally typed Boon effects.
pub struct HostServiceEffectAdapter {
    services: HostServices,
    effects: EffectIds,
    secrets: BTreeMap<String, SecretRef>,
    max_active_deadlines: usize,
    active_deadlines: BTreeMap<TransientEffectCallId, ActiveDeadline>,
    completions_tx: mpsc::Sender<HostServiceEffectCompletion>,
    completions_rx: mpsc::Receiver<HostServiceEffectCompletion>,
}

impl HostServiceEffectAdapter {
    pub fn new(
        mut services: HostServices,
        named_secrets: impl IntoIterator<Item = NamedSecret>,
        max_active_deadlines: usize,
    ) -> Result<Self, AdapterError> {
        if max_active_deadlines == 0 {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidConfiguration,
                "active deadline capacity must be positive",
            ));
        }
        let mut secrets = BTreeMap::new();
        for named in named_secrets {
            validate_secret_name(&named.name)?;
            if secrets.contains_key(&named.name) {
                return Err(AdapterError::new(
                    AdapterErrorKind::InvalidConfiguration,
                    format_args!("duplicate named secret `{}`", named.name),
                ));
            }
            let secret_ref = services.configure_secret(named.material).map_err(|error| {
                AdapterError::new(
                    AdapterErrorKind::InvalidConfiguration,
                    format_args!("cannot configure named secret `{}`: {error}", named.name),
                )
            })?;
            secrets.insert(named.name, secret_ref);
        }
        let effects = EffectIds::new()?;
        let (completions_tx, completions_rx) = mpsc::channel(max_active_deadlines);
        Ok(Self {
            services,
            effects,
            secrets,
            max_active_deadlines,
            active_deadlines: BTreeMap::new(),
            completions_tx,
            completions_rx,
        })
    }

    pub fn effect_ids(&self) -> [EffectId; 6] {
        self.effects.all()
    }

    pub fn owns(&self, effect_id: EffectId) -> bool {
        self.effects.operation(effect_id).is_some()
    }

    pub fn active_deadline_count(&self) -> usize {
        self.active_deadlines.len()
    }

    pub fn submit(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<HostServiceEffectSubmission, AdapterError> {
        let operation = self
            .effects
            .operation(invocation.effect_id)
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::NotOwned,
                    format_args!(
                        "host-service adapter does not own effect {}",
                        invocation.effect_id
                    ),
                )
            })?;
        if matches!(operation, Operation::Deadline) {
            return self.submit_deadline(invocation);
        }
        let outcome = match operation {
            Operation::WallClock => self.read_wall_clock(&invocation.intent),
            Operation::SecureRandom => self.secure_random(&invocation.intent),
            Operation::SecretVerify => self.verify_secret(&invocation.intent),
            Operation::HmacSign => self.sign_hmac(&invocation.intent),
            Operation::HmacVerify => self.verify_hmac(&invocation.intent),
            Operation::Deadline => unreachable!("deadline handled above"),
        };
        Ok(immediate_submission(invocation, outcome))
    }

    pub async fn next_completion(&mut self) -> Result<HostServiceEffectCompletion, AdapterError> {
        loop {
            let completion = self.completions_rx.recv().await.ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::Closed,
                    "host-service completion lane closed",
                )
            })?;
            let Some(active) = self.active_deadlines.remove(&completion.call_id) else {
                continue;
            };
            debug_assert_eq!(active.invocation_id, completion.invocation_id);
            return Ok(completion);
        }
    }

    pub fn cancel(&mut self, call_id: TransientEffectCallId) -> bool {
        let Some(active) = self.active_deadlines.remove(&call_id) else {
            return false;
        };
        active.cancellation.cancel();
        active.task.abort();
        true
    }

    pub fn cancel_all(&mut self) -> Vec<TransientEffectCallId> {
        let calls = self.active_deadlines.keys().copied().collect::<Vec<_>>();
        for call in &calls {
            self.cancel(*call);
        }
        calls
    }

    fn read_wall_clock(&self, intent: &Value) -> Value {
        if let Err(failure) = exact_record(intent, &[]) {
            return failure;
        }
        let snapshot = match self.services.wall_clock_now() {
            Ok(snapshot) => snapshot,
            Err(error) => return host_failure(error),
        };
        let Ok(unix_seconds) = i64::try_from(snapshot.unix_epoch_seconds()) else {
            return failure(
                "time_out_of_range",
                "wall-clock seconds are outside Number range",
            );
        };
        tagged(
            "WallClockRead",
            BTreeMap::from([
                ("unix_seconds".to_owned(), number(unix_seconds)),
                (
                    "nanoseconds".to_owned(),
                    number(i64::from(snapshot.nanoseconds_within_second())),
                ),
            ]),
        )
    }

    fn secure_random(&self, intent: &Value) -> Value {
        let fields = match exact_record(intent, &["byte_count"]) {
            Ok(fields) => fields,
            Err(failure) => return failure,
        };
        let byte_count = match positive_usize(fields, "byte_count") {
            Ok(value) => value,
            Err(failure) => return failure,
        };
        match self.services.secure_random(byte_count) {
            Ok(bytes) => tagged(
                "RandomBytesReady",
                BTreeMap::from([("bytes".to_owned(), Value::Bytes(bytes.as_bytes().to_vec()))]),
            ),
            Err(error) => host_failure(error),
        }
    }

    fn verify_secret(&self, intent: &Value) -> Value {
        let fields = match exact_record(intent, &["candidate", "secret"]) {
            Ok(fields) => fields,
            Err(failure) => return failure,
        };
        let (secret, candidate) = match self.secret_and_bytes(fields, "candidate") {
            Ok(values) => values,
            Err(failure) => return failure,
        };
        match self.services.verify_configured_secret(secret, candidate) {
            Ok(verification) => tagged(
                "SecretVerified",
                BTreeMap::from([(
                    "matches".to_owned(),
                    Value::Bool(verification.is_verified()),
                )]),
            ),
            Err(error) => host_failure(error),
        }
    }

    fn sign_hmac(&self, intent: &Value) -> Value {
        let fields = match exact_record(intent, &["message", "secret"]) {
            Ok(fields) => fields,
            Err(failure) => return failure,
        };
        let (secret, message) = match self.secret_and_bytes(fields, "message") {
            Ok(values) => values,
            Err(failure) => return failure,
        };
        match self.services.hmac_sha256_sign(secret, message) {
            Ok(tag) => tagged(
                "HmacSigned",
                BTreeMap::from([("tag".to_owned(), Value::Bytes(tag.into_bytes().to_vec()))]),
            ),
            Err(error) => host_failure(error),
        }
    }

    fn verify_hmac(&self, intent: &Value) -> Value {
        let fields = match exact_record(intent, &["message", "secret", "tag"]) {
            Ok(fields) => fields,
            Err(failure) => return failure,
        };
        let (secret, message) = match self.secret_and_bytes(fields, "message") {
            Ok(values) => values,
            Err(failure) => return failure,
        };
        let tag = match bytes_field(fields, "tag")
            .and_then(|bytes| <[u8; HMAC_SHA256_TAG_BYTES]>::try_from(bytes).map_err(|_| ()))
        {
            Ok(tag) => HmacSha256Tag::from_bytes(tag),
            Err(()) => return failure("invalid_intent", "HMAC tag must contain exactly 32 bytes"),
        };
        match self.services.hmac_sha256_verify(secret, message, &tag) {
            Ok(verification) => tagged(
                "HmacVerified",
                BTreeMap::from([(
                    "matches".to_owned(),
                    Value::Bool(verification.is_verified()),
                )]),
            ),
            Err(error) => host_failure(error),
        }
    }

    fn submit_deadline(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<HostServiceEffectSubmission, AdapterError> {
        let fields = match exact_record(&invocation.intent, &["delay_ms"]) {
            Ok(fields) => fields,
            Err(failure) => return Ok(immediate_submission(invocation, failure)),
        };
        let delay_ms = match positive_u64(fields, "delay_ms") {
            Ok(value) => value,
            Err(failure) => return Ok(immediate_submission(invocation, failure)),
        };
        if self.active_deadlines.len() >= self.max_active_deadlines {
            return Ok(immediate_submission(
                invocation,
                failure(
                    "host_busy",
                    "host-service deadline lane is at its configured capacity",
                ),
            ));
        }
        let timer = match self
            .services
            .schedule_deadline_after(Duration::from_millis(delay_ms))
        {
            Ok(timer) => timer,
            Err(error) => return Ok(immediate_submission(invocation, host_failure(error))),
        };
        let call_id = invocation.call_id;
        let invocation_id = invocation.invocation_id;
        let cancellation = timer.cancellation_handle();
        let sender = self.completions_tx.clone();
        let task = tokio::task::spawn_blocking(move || {
            let outcome = match timer.recv() {
                Ok(_) => tagged(
                    "TimerFired",
                    BTreeMap::from([(
                        "delay_ms".to_owned(),
                        number(i64::try_from(delay_ms).unwrap_or(i64::MAX)),
                    )]),
                ),
                Err(error) => timer_failure(error),
            };
            let _ = sender.blocking_send(HostServiceEffectCompletion {
                call_id,
                invocation_id,
                outcome,
            });
        });
        self.active_deadlines.insert(
            call_id,
            ActiveDeadline {
                invocation_id,
                cancellation,
                task,
            },
        );
        Ok(HostServiceEffectSubmission {
            call_id,
            immediate_completion: None,
        })
    }

    fn secret_and_bytes<'a>(
        &self,
        fields: &'a BTreeMap<String, Value>,
        bytes_name: &str,
    ) -> Result<(SecretRef, &'a [u8]), Value> {
        let secret_name = text_field(fields, "secret")
            .map_err(|()| failure("invalid_intent", "secret capability name must be Text"))?;
        let secret = self.secrets.get(secret_name).copied().ok_or_else(|| {
            failure(
                "unknown_secret",
                "named secret capability is not configured for this program",
            )
        })?;
        let bytes = bytes_field(fields, bytes_name).map_err(|()| {
            failure(
                "invalid_intent",
                "host-service byte field differs from the typed contract",
            )
        })?;
        Ok((secret, bytes))
    }
}

impl Drop for HostServiceEffectAdapter {
    fn drop(&mut self) {
        self.cancel_all();
        self.services.shutdown();
    }
}

const FILE_CAPABILITY_TOKEN_BYTES: usize = 32;
const FILE_CAPABILITY_FIRST_GENERATION: u32 = 1;
const FILE_CAPABILITY_TOKEN_ATTEMPTS: usize = 16;
const FILE_WORKER_RUNNING: u8 = 0;
const FILE_WORKER_CANCEL_REQUESTED: u8 = 1;
const FILE_WORKER_DISCARD: u8 = 2;
const FILE_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const FILE_CONTENT_TYPE: &str = "application/octet-stream";

/// An opaque process-local reference to a host-owned file path.
///
/// The token is intentionally not exposed through Rust accessors. The only
/// public serialization route produces the typed value accepted by
/// `File/read_stream`; no path is copied into Boon state.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileCapability {
    token: [u8; FILE_CAPABILITY_TOKEN_BYTES],
    generation: u32,
}

impl FileCapability {
    pub const fn generation(&self) -> u32 {
        self.generation
    }

    pub fn capability_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            ("token".to_owned(), Value::Bytes(self.token.to_vec())),
            ("generation".to_owned(), number(i64::from(self.generation))),
        ]))
    }

    pub fn file_selected_value(&self) -> Value {
        tagged(
            "FileSelected",
            BTreeMap::from([("capability".to_owned(), self.capability_value())]),
        )
    }
}

impl fmt::Debug for FileCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileCapability")
            .field("token", &"<opaque>")
            .field("generation", &self.generation)
            .finish()
    }
}

pub fn package_asset_value(url: impl Into<String>) -> Value {
    tagged(
        "PackageAsset",
        BTreeMap::from([("url".to_owned(), Value::Text(url.into()))]),
    )
}

fn validate_package_asset_url(url: &str) -> Result<(), AdapterError> {
    if url.is_empty() || url.len() > MAX_PACKAGE_ASSET_URL_BYTES {
        return Err(AdapterError::new(
            AdapterErrorKind::Capability,
            "package asset URL is empty or exceeds the bounded contract",
        ));
    }
    if !url.starts_with("asset://") {
        return Err(AdapterError::new(
            AdapterErrorKind::Capability,
            "package asset URL must use the asset scheme",
        ));
    }
    Ok(())
}

fn package_asset_display_name(url: &str) -> String {
    url.rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("package-asset")
        .chars()
        .take(256)
        .collect()
}

struct RegisteredFile {
    generation: u32,
    path: PathBuf,
}

/// Bounded registry that keeps selected paths entirely on the host side.
pub struct FileCapabilityRegistry {
    max_capabilities: usize,
    files: BTreeMap<[u8; FILE_CAPABILITY_TOKEN_BYTES], RegisteredFile>,
}

impl FileCapabilityRegistry {
    pub fn new(max_capabilities: usize) -> Result<Self, AdapterError> {
        if max_capabilities == 0 {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidConfiguration,
                "file capability capacity must be positive",
            ));
        }
        Ok(Self {
            max_capabilities,
            files: BTreeMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub const fn capacity(&self) -> usize {
        self.max_capabilities
    }

    pub fn register_file(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> Result<FileCapability, AdapterError> {
        if self.files.len() >= self.max_capabilities {
            return Err(AdapterError::new(
                AdapterErrorKind::Capacity,
                "file capability registry is at its configured capacity",
            ));
        }
        let path = validated_host_path(path.into())?;
        for _ in 0..FILE_CAPABILITY_TOKEN_ATTEMPTS {
            let mut token = [0_u8; FILE_CAPABILITY_TOKEN_BYTES];
            getrandom::fill(&mut token).map_err(|error| {
                AdapterError::new(
                    AdapterErrorKind::Capability,
                    format_args!("cannot mint file capability token: {error}"),
                )
            })?;
            if self.files.contains_key(&token) {
                continue;
            }
            self.files.insert(
                token,
                RegisteredFile {
                    generation: FILE_CAPABILITY_FIRST_GENERATION,
                    path,
                },
            );
            return Ok(FileCapability {
                token,
                generation: FILE_CAPABILITY_FIRST_GENERATION,
            });
        }
        Err(AdapterError::new(
            AdapterErrorKind::Capability,
            "cannot mint a unique file capability token",
        ))
    }

    pub fn replace_file(
        &mut self,
        capability: &FileCapability,
        path: impl Into<PathBuf>,
    ) -> Result<FileCapability, AdapterError> {
        let path = validated_host_path(path.into())?;
        let entry = self.files.get_mut(&capability.token).ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Capability,
                "file capability is unknown or revoked",
            )
        })?;
        if entry.generation != capability.generation {
            return Err(AdapterError::new(
                AdapterErrorKind::Capability,
                "file capability generation is stale",
            ));
        }
        let generation = entry.generation.checked_add(1).ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Capability,
                "file capability generation is exhausted",
            )
        })?;
        entry.generation = generation;
        entry.path = path;
        Ok(FileCapability {
            token: capability.token,
            generation,
        })
    }

    pub fn revoke(&mut self, capability: &FileCapability) -> bool {
        if self
            .files
            .get(&capability.token)
            .is_none_or(|entry| entry.generation != capability.generation)
        {
            return false;
        }
        self.files.remove(&capability.token);
        true
    }

    pub fn contains(&self, capability: &FileCapability) -> bool {
        self.files
            .get(&capability.token)
            .is_some_and(|entry| entry.generation == capability.generation)
    }

    fn resolve(&self, capability: &FileCapability) -> Result<PathBuf, FileCapabilityLookup> {
        let Some(entry) = self.files.get(&capability.token) else {
            return Err(FileCapabilityLookup::Unknown);
        };
        if entry.generation != capability.generation {
            return Err(FileCapabilityLookup::Stale);
        }
        Ok(entry.path.clone())
    }
}

fn validated_host_path(path: PathBuf) -> Result<PathBuf, AdapterError> {
    if path.as_os_str().is_empty() {
        return Err(AdapterError::new(
            AdapterErrorKind::Capability,
            "file capability path must not be empty",
        ));
    }
    Ok(path)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileCapabilityLookup {
    Unknown,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileReadStreamLimits {
    pub max_active: usize,
    pub event_queue_capacity: usize,
    pub credit_queue_capacity: usize,
}

impl FileReadStreamLimits {
    pub const fn new(
        max_active: usize,
        event_queue_capacity: usize,
        credit_queue_capacity: usize,
    ) -> Self {
        Self {
            max_active,
            event_queue_capacity,
            credit_queue_capacity,
        }
    }
}

impl Default for FileReadStreamLimits {
    fn default() -> Self {
        Self {
            max_active: 8,
            event_queue_capacity: 32,
            credit_queue_capacity: boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT as usize,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileReadStreamEvent {
    pub call_id: TransientEffectCallId,
    pub invocation_id: EffectInvocationId,
    pub result_sequence: u64,
    pub outcome: Value,
    terminal: bool,
}

impl FileReadStreamEvent {
    pub const fn is_terminal(&self) -> bool {
        self.terminal
    }

    pub fn result_tag(&self) -> Option<&str> {
        let Value::Record(fields) = &self.outcome else {
            return None;
        };
        match fields.get("$tag") {
            Some(Value::Text(tag)) => Some(tag.as_str()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileReadStreamSubmission {
    pub call_id: TransientEffectCallId,
    pub replaced_call: Option<TransientEffectCallId>,
    pub queued_terminal: bool,
}

struct ActiveFileRead {
    invocation_id: EffectInvocationId,
    max_in_flight: u32,
    state: Arc<AtomicU8>,
    outstanding_credits: Arc<AtomicU32>,
    credit_tx: std_mpsc::SyncSender<()>,
    task: Option<thread::JoinHandle<()>>,
}

struct RegisteredPackageAsset {
    content: ContentLease,
    display_name: String,
}

/// Bounded host adapter for only the typed `File/read_stream` effect.
pub struct FileReadStreamEffectAdapter {
    capabilities: FileCapabilityRegistry,
    package_assets: BTreeMap<String, RegisteredPackageAsset>,
    content_store: ContentStore,
    effect_id: EffectId,
    limits: FileReadStreamLimits,
    active: BTreeMap<TransientEffectCallId, ActiveFileRead>,
    queued_terminals: BTreeMap<TransientEffectCallId, EffectInvocationId>,
    latest_by_invocation: BTreeMap<EffectInvocationId, TransientEffectCallId>,
    events_tx: mpsc::Sender<FileReadStreamEvent>,
    events_rx: mpsc::Receiver<FileReadStreamEvent>,
}

impl FileReadStreamEffectAdapter {
    pub fn new(
        capabilities: FileCapabilityRegistry,
        content_store: ContentStore,
        max_active: usize,
    ) -> Result<Self, AdapterError> {
        let event_queue_capacity = max_active
            .checked_mul(boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT as usize)
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::InvalidConfiguration,
                    "file stream event capacity overflow",
                )
            })?;
        Self::with_limits(
            capabilities,
            content_store,
            FileReadStreamLimits::new(
                max_active,
                event_queue_capacity,
                boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT as usize,
            ),
        )
    }

    pub fn with_limits(
        capabilities: FileCapabilityRegistry,
        content_store: ContentStore,
        limits: FileReadStreamLimits,
    ) -> Result<Self, AdapterError> {
        if limits.max_active == 0
            || limits.event_queue_capacity == 0
            || limits.credit_queue_capacity == 0
        {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidConfiguration,
                "file stream active, event, and credit capacities must be positive",
            ));
        }
        let effect_id = EffectId::from_host_operation(
            boon_effect_schema::FILE_READ_STREAM_OPERATION,
        )
        .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidConfiguration, error))?;
        let (events_tx, events_rx) = mpsc::channel(limits.event_queue_capacity);
        Ok(Self {
            capabilities,
            package_assets: BTreeMap::new(),
            content_store,
            effect_id,
            limits,
            active: BTreeMap::new(),
            queued_terminals: BTreeMap::new(),
            latest_by_invocation: BTreeMap::new(),
            events_tx,
            events_rx,
        })
    }

    pub const fn effect_id(&self) -> EffectId {
        self.effect_id
    }

    pub const fn limits(&self) -> FileReadStreamLimits {
        self.limits
    }

    pub fn capabilities(&self) -> &FileCapabilityRegistry {
        &self.capabilities
    }

    pub fn capabilities_mut(&mut self) -> &mut FileCapabilityRegistry {
        &mut self.capabilities
    }

    pub fn content_store(&self) -> &ContentStore {
        &self.content_store
    }

    pub fn register_package_asset(
        &mut self,
        url: impl Into<String>,
        bytes: &[u8],
    ) -> Result<ContentRef, AdapterError> {
        let url = url.into();
        validate_package_asset_url(&url)?;
        let content = self
            .content_store
            .insert_bytes(bytes)
            .map_err(|error| AdapterError::new(AdapterErrorKind::Capacity, error))?;
        let lease = self
            .content_store
            .resolve(content)
            .map_err(|error| AdapterError::new(AdapterErrorKind::Capacity, error))?;
        let display_name = package_asset_display_name(&url);
        self.package_assets.insert(
            url,
            RegisteredPackageAsset {
                content: lease,
                display_name,
            },
        );
        Ok(content)
    }

    pub fn clear_package_assets(&mut self) {
        self.package_assets.clear();
    }

    pub fn package_asset_count(&self) -> usize {
        self.package_assets.len()
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn owned_call_count(&self) -> usize {
        self.active.len() + self.queued_terminals.len()
    }

    pub fn outstanding_credits(&self, call_id: TransientEffectCallId) -> Option<u32> {
        self.active
            .get(&call_id)
            .map(|active| active.outstanding_credits.load(Ordering::Acquire))
    }

    pub fn submit(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<FileReadStreamSubmission, AdapterError> {
        if invocation.effect_id != self.effect_id {
            return Err(AdapterError::new(
                AdapterErrorKind::NotOwned,
                format_args!(
                    "file stream adapter does not own effect {}",
                    invocation.effect_id
                ),
            ));
        }
        let (initial_credits, max_in_flight) = validate_file_stream_delivery(&invocation.delivery)?;

        let replaced_call = self
            .latest_by_invocation
            .get(&invocation.invocation_id)
            .copied()
            .filter(|call_id| *call_id != invocation.call_id);
        if let Some(previous) = replaced_call {
            self.cancel(previous);
        }

        let decoded = match decode_file_read_stream_intent(&invocation.intent) {
            Ok(decoded) => decoded,
            Err(failure) => {
                return self.queue_terminal_failure(invocation, replaced_call, failure);
            }
        };
        let (path, display_name, source_lease) = match decoded.source {
            DecodedFileSource::Capability(capability) => {
                let path = match self.capabilities.resolve(&capability) {
                    Ok(path) => path,
                    Err(FileCapabilityLookup::Unknown) => {
                        return self.queue_terminal_failure(
                            invocation,
                            replaced_call,
                            FileStreamFailure::new(
                                "unknown_capability",
                                "file capability is unknown or revoked",
                            ),
                        );
                    }
                    Err(FileCapabilityLookup::Stale) => {
                        return self.queue_terminal_failure(
                            invocation,
                            replaced_call,
                            FileStreamFailure::new(
                                "stale_capability",
                                "file capability generation is stale",
                            ),
                        );
                    }
                };
                (path, None, None)
            }
            DecodedFileSource::PackageAsset(url) => {
                let Some(asset) = self.package_assets.get(&url) else {
                    return self.queue_terminal_failure(
                        invocation,
                        replaced_call,
                        FileStreamFailure::new(
                            "unknown_package_asset",
                            "package asset is absent from the active application",
                        ),
                    );
                };
                let lease = match self.content_store.resolve(asset.content.content()) {
                    Ok(lease) => lease,
                    Err(error) => {
                        return self.queue_terminal_failure(
                            invocation,
                            replaced_call,
                            content_store_failure(error),
                        );
                    }
                };
                (
                    lease.path().to_path_buf(),
                    Some(asset.display_name.clone()),
                    Some(lease),
                )
            }
        };
        if self.active.len() >= self.limits.max_active {
            return self.queue_terminal_failure(
                invocation,
                replaced_call,
                FileStreamFailure::new(
                    "host_busy",
                    "file stream host is at its configured active-read limit",
                ),
            );
        }

        let call_id = invocation.call_id;
        let invocation_id = invocation.invocation_id;
        let state = Arc::new(AtomicU8::new(FILE_WORKER_RUNNING));
        let outstanding_credits = Arc::new(AtomicU32::new(initial_credits));
        let (credit_tx, credit_rx) = std_mpsc::sync_channel(self.limits.credit_queue_capacity);
        let worker = FileReadWorker {
            call_id,
            invocation_id,
            path,
            display_name,
            _source_lease: source_lease,
            chunk_bytes: decoded.chunk_bytes,
            retain_content: decoded.retain_content,
            content_store: self.content_store.clone(),
            state: Arc::clone(&state),
            outstanding_credits: Arc::clone(&outstanding_credits),
            credit_rx,
            events_tx: self.events_tx.clone(),
        };
        let task = match thread::Builder::new()
            .name(format!("boon-file-read-{}", call_id.sequence()))
            .spawn(move || run_file_read_worker(worker))
        {
            Ok(task) => task,
            Err(error) => {
                return self.queue_terminal_failure(
                    invocation,
                    replaced_call,
                    FileStreamFailure::new(
                        "worker_unavailable",
                        format!("cannot start bounded file worker: {error}"),
                    ),
                );
            }
        };
        self.active.insert(
            call_id,
            ActiveFileRead {
                invocation_id,
                max_in_flight,
                state,
                outstanding_credits,
                credit_tx,
                task: Some(task),
            },
        );
        self.latest_by_invocation.insert(invocation_id, call_id);
        Ok(FileReadStreamSubmission {
            call_id,
            replaced_call,
            queued_terminal: false,
        })
    }

    pub async fn next_event(&mut self) -> Result<FileReadStreamEvent, AdapterError> {
        loop {
            let event = self.events_rx.recv().await.ok_or_else(|| {
                AdapterError::new(AdapterErrorKind::Closed, "file stream event lane closed")
            })?;
            if let Some(event) = self.accept_event(event) {
                return Ok(event);
            }
        }
    }

    pub fn try_next_event(&mut self) -> Result<Option<FileReadStreamEvent>, AdapterError> {
        loop {
            match self.events_rx.try_recv() {
                Ok(event) => {
                    if let Some(event) = self.accept_event(event) {
                        return Ok(Some(event));
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => return Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err(AdapterError::new(
                        AdapterErrorKind::Closed,
                        "file stream event lane closed",
                    ));
                }
            }
        }
    }

    /// Requests an in-contract `Cancelled` terminal result.
    pub fn request_cancel(&mut self, call_id: TransientEffectCallId) -> bool {
        let Some(active) = self.active.get(&call_id) else {
            return false;
        };
        let requested = active
            .state
            .compare_exchange(
                FILE_WORKER_RUNNING,
                FILE_WORKER_CANCEL_REQUESTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
            || active.state.load(Ordering::Acquire) == FILE_WORKER_CANCEL_REQUESTED;
        if requested {
            let _ = active.credit_tx.try_send(());
        }
        requested
    }

    /// Discards a runtime-cancelled call without trying to deliver another result.
    pub fn cancel(&mut self, call_id: TransientEffectCallId) -> bool {
        let mut found = false;
        if let Some(active) = self.active.remove(&call_id) {
            found = true;
            stop_file_worker(active);
        }
        if self.queued_terminals.remove(&call_id).is_some() {
            found = true;
        }
        self.remove_latest_call(call_id);
        found
    }

    pub fn cancel_all(&mut self) -> Vec<TransientEffectCallId> {
        let calls = self
            .active
            .keys()
            .chain(self.queued_terminals.keys())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        for call_id in &calls {
            self.cancel(*call_id);
        }
        calls
    }

    pub fn accept_credit_grant(
        &mut self,
        grant: TransientEffectCreditGrant,
    ) -> Result<bool, AdapterError> {
        self.grant_credits(grant.call_id, grant.credits)
    }

    pub fn grant_credits(
        &mut self,
        call_id: TransientEffectCallId,
        credits: u32,
    ) -> Result<bool, AdapterError> {
        if credits == 0 {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidConfiguration,
                "file stream credit grant must be positive",
            ));
        }
        let Some(active) = self.active.get(&call_id) else {
            return Ok(false);
        };
        let mut current = active.outstanding_credits.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(credits) else {
                return Err(AdapterError::new(
                    AdapterErrorKind::Capacity,
                    "file stream credit count overflow",
                ));
            };
            if next > active.max_in_flight {
                return Err(AdapterError::new(
                    AdapterErrorKind::Capacity,
                    "file stream credit grant exceeds declared max_in_flight",
                ));
            }
            match active.outstanding_credits.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        match active.credit_tx.try_send(()) {
            Ok(()) | Err(std_mpsc::TrySendError::Full(())) => Ok(true),
            Err(std_mpsc::TrySendError::Disconnected(())) => Ok(true),
        }
    }

    pub fn route_runtime_turn(&mut self, turn: &RuntimeTurn) -> Result<(), AdapterError> {
        for call_id in &turn.cancelled_transient_effects {
            self.cancel(*call_id);
        }
        for grant in &turn.transient_effect_credit_grants {
            self.accept_credit_grant(*grant)?;
        }
        Ok(())
    }

    fn queue_terminal_failure(
        &mut self,
        invocation: TransientEffectInvocation,
        replaced_call: Option<TransientEffectCallId>,
        failure: FileStreamFailure,
    ) -> Result<FileReadStreamSubmission, AdapterError> {
        let event = FileReadStreamEvent {
            call_id: invocation.call_id,
            invocation_id: invocation.invocation_id,
            result_sequence: 0,
            outcome: file_failure_outcome(failure),
            terminal: true,
        };
        match self.events_tx.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(AdapterError::new(
                    AdapterErrorKind::Capacity,
                    "file stream event queue is full",
                ));
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(AdapterError::new(
                    AdapterErrorKind::Closed,
                    "file stream event lane closed",
                ));
            }
        }
        self.queued_terminals
            .insert(invocation.call_id, invocation.invocation_id);
        self.latest_by_invocation
            .insert(invocation.invocation_id, invocation.call_id);
        Ok(FileReadStreamSubmission {
            call_id: invocation.call_id,
            replaced_call,
            queued_terminal: true,
        })
    }

    fn accept_event(&mut self, event: FileReadStreamEvent) -> Option<FileReadStreamEvent> {
        if let Some(invocation_id) = self.queued_terminals.remove(&event.call_id) {
            if invocation_id != event.invocation_id || !event.terminal {
                self.remove_latest_call(event.call_id);
                return None;
            }
            self.remove_latest_call(event.call_id);
            return Some(event);
        }
        let Some(active) = self.active.get(&event.call_id) else {
            return None;
        };
        if active.invocation_id != event.invocation_id {
            self.cancel(event.call_id);
            return None;
        }
        if event.terminal {
            let active = self
                .active
                .remove(&event.call_id)
                .expect("active file call was just observed");
            finish_file_worker(active);
            self.remove_latest_call(event.call_id);
        }
        Some(event)
    }

    fn remove_latest_call(&mut self, call_id: TransientEffectCallId) {
        let invocation_id = self
            .latest_by_invocation
            .iter()
            .find_map(|(invocation_id, latest)| (*latest == call_id).then_some(*invocation_id));
        if let Some(invocation_id) = invocation_id {
            self.latest_by_invocation.remove(&invocation_id);
        }
    }
}

impl Drop for FileReadStreamEffectAdapter {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

pub fn apply_file_read_stream_event(
    program: &mut ProgramSession,
    adapter: &mut FileReadStreamEffectAdapter,
    event: FileReadStreamEvent,
) -> Result<RuntimeTurn, AdapterError> {
    let call_id = event.call_id;
    let turn = match program.deliver_transient_effect_result(
        event.call_id,
        event.result_sequence,
        event.outcome,
    ) {
        Ok(turn) => turn,
        Err(error) => {
            adapter.cancel(call_id);
            return Err(AdapterError::new(AdapterErrorKind::Runtime, error));
        }
    };
    adapter.route_runtime_turn(&turn)?;
    Ok(turn)
}

pub fn apply_event(
    program: &mut ProgramSession,
    adapter: &mut FileReadStreamEffectAdapter,
    event: FileReadStreamEvent,
) -> Result<RuntimeTurn, AdapterError> {
    apply_file_read_stream_event(program, adapter, event)
}

fn validate_file_stream_delivery(
    delivery: &EffectDeliveryCardinality,
) -> Result<(u32, u32), AdapterError> {
    let EffectDeliveryCardinality::Stream {
        initial_credits,
        max_in_flight,
        terminal_result_tags,
    } = delivery
    else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidDelivery,
            "File/read_stream requires declared Stream delivery",
        ));
    };
    let expected_terminal_tags = ["Cancelled", "Failed", "Finished"];
    if *initial_credits != boon_effect_schema::FILE_STREAM_INITIAL_CREDITS
        || *max_in_flight != boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT
        || !terminal_result_tags
            .iter()
            .map(String::as_str)
            .eq(expected_terminal_tags)
    {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidDelivery,
            "File/read_stream delivery differs from the bounded typed contract",
        ));
    }
    Ok((*initial_credits, *max_in_flight))
}

enum DecodedFileSource {
    Capability(FileCapability),
    PackageAsset(String),
}

struct DecodedFileReadIntent {
    source: DecodedFileSource,
    chunk_bytes: usize,
    retain_content: bool,
}

fn decode_file_read_stream_intent(
    value: &Value,
) -> Result<DecodedFileReadIntent, FileStreamFailure> {
    let fields = file_record(
        value,
        &["chunk_bytes", "file", "retain_content"],
        "file stream intent",
    )?;
    let file = fields
        .get("file")
        .ok_or_else(|| FileStreamFailure::invalid("file stream intent is missing `file`"))?;
    let file_fields = match file {
        Value::Record(fields) => fields,
        _ => {
            return Err(FileStreamFailure::invalid(
                "file input must be a tagged object",
            ));
        }
    };
    let source = match file_fields.get("$tag") {
        Some(Value::Text(tag)) if tag == "FileSelected" => {
            let file = file_record(file, &["$tag", "capability"], "selected file")?;
            let capability = file.get("capability").ok_or_else(|| {
                FileStreamFailure::invalid("selected file is missing its capability")
            })?;
            let capability = file_record(capability, &["generation", "token"], "file capability")?;
            let token = match capability.get("token") {
                Some(Value::Bytes(token)) => <[u8; FILE_CAPABILITY_TOKEN_BYTES]>::try_from(
                    token.as_slice(),
                )
                .map_err(|_| {
                    FileStreamFailure::invalid(
                        "file capability token must contain exactly 32 bytes",
                    )
                })?,
                _ => {
                    return Err(FileStreamFailure::invalid(
                        "file capability token must be Bytes",
                    ));
                }
            };
            let generation = file_positive_u32(capability, "generation")?;
            DecodedFileSource::Capability(FileCapability { token, generation })
        }
        Some(Value::Text(tag)) if tag == "PackageAsset" => {
            let file = file_record(file, &["$tag", "url"], "package asset")?;
            let url = file_text(file, "url")?.to_owned();
            if url.is_empty() || url.len() > MAX_PACKAGE_ASSET_URL_BYTES {
                return Err(FileStreamFailure::invalid(
                    "package asset URL is empty or exceeds the bounded contract",
                ));
            }
            DecodedFileSource::PackageAsset(url)
        }
        _ => {
            return Err(FileStreamFailure::invalid(
                "file input must be FileSelected or PackageAsset",
            ));
        }
    };
    let chunk_bytes = file_positive_u64(fields, "chunk_bytes")?;
    if !(boon_effect_schema::FILE_STREAM_MIN_CHUNK_BYTES
        ..=boon_effect_schema::FILE_STREAM_MAX_CHUNK_BYTES)
        .contains(&chunk_bytes)
    {
        return Err(FileStreamFailure::invalid(
            "file stream chunk_bytes is outside the typed bounded range",
        ));
    }
    let chunk_bytes = usize::try_from(chunk_bytes).map_err(|_| {
        FileStreamFailure::invalid("file stream chunk_bytes exceeds the host platform range")
    })?;
    let retain_content = match fields.get("retain_content") {
        Some(Value::Bool(value)) => *value,
        _ => {
            return Err(FileStreamFailure::invalid(
                "file stream retain_content must be Bool",
            ));
        }
    };
    Ok(DecodedFileReadIntent {
        source,
        chunk_bytes,
        retain_content,
    })
}

fn file_record<'a>(
    value: &'a Value,
    expected: &[&str],
    context: &str,
) -> Result<&'a BTreeMap<String, Value>, FileStreamFailure> {
    let Value::Record(fields) = value else {
        return Err(FileStreamFailure::invalid(format!(
            "{context} must be a record"
        )));
    };
    let actual = fields.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(FileStreamFailure::invalid(format!(
            "{context} fields differ from the typed contract"
        )));
    }
    Ok(fields)
}

fn file_text<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<&'a str, FileStreamFailure> {
    match fields.get(name) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(FileStreamFailure::invalid(format!(
            "file stream field `{name}` must be Text"
        ))),
    }
}

fn file_positive_u32(
    fields: &BTreeMap<String, Value>,
    name: &str,
) -> Result<u32, FileStreamFailure> {
    let value = file_positive_i64(fields, name)?;
    u32::try_from(value).map_err(|_| {
        FileStreamFailure::invalid(format!(
            "file stream field `{name}` exceeds the capability generation range"
        ))
    })
}

fn file_positive_u64(
    fields: &BTreeMap<String, Value>,
    name: &str,
) -> Result<u64, FileStreamFailure> {
    let value = file_positive_i64(fields, name)?;
    u64::try_from(value).map_err(|_| {
        FileStreamFailure::invalid(format!(
            "file stream field `{name}` exceeds the unsigned range"
        ))
    })
}

fn file_positive_i64(
    fields: &BTreeMap<String, Value>,
    name: &str,
) -> Result<i64, FileStreamFailure> {
    let Some(Value::Number(value)) = fields.get(name) else {
        return Err(FileStreamFailure::invalid(format!(
            "file stream field `{name}` must be Number"
        )));
    };
    value
        .to_i64_exact()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            FileStreamFailure::invalid(format!(
                "file stream field `{name}` must be a positive whole number"
            ))
        })
}

#[derive(Clone, Debug)]
struct FileStreamFailure {
    code: &'static str,
    diagnostic: String,
}

impl FileStreamFailure {
    fn new(code: &'static str, diagnostic: impl Into<String>) -> Self {
        Self {
            code,
            diagnostic: bounded_diagnostic(diagnostic.into()),
        }
    }

    fn invalid(diagnostic: impl Into<String>) -> Self {
        Self::new("invalid_intent", diagnostic)
    }
}

fn content_store_failure(error: ContentStoreError) -> FileStreamFailure {
    let code = match error.kind() {
        ContentStoreErrorKind::Capacity => "content_store_full",
        ContentStoreErrorKind::Missing => "content_missing",
        ContentStoreErrorKind::InvalidConfiguration | ContentStoreErrorKind::InvalidReference => {
            "content_invalid"
        }
        ContentStoreErrorKind::Io => "content_io",
    };
    FileStreamFailure::new(code, error.diagnostic())
}

fn file_failure_outcome(failure: FileStreamFailure) -> Value {
    tagged(
        "Failed",
        BTreeMap::from([
            ("code".to_owned(), Value::Text(failure.code.to_owned())),
            ("diagnostic".to_owned(), Value::Text(failure.diagnostic)),
        ]),
    )
}

fn file_cancelled_outcome() -> Value {
    tagged("Cancelled", BTreeMap::new())
}

struct FileReadWorker {
    call_id: TransientEffectCallId,
    invocation_id: EffectInvocationId,
    path: PathBuf,
    display_name: Option<String>,
    _source_lease: Option<ContentLease>,
    chunk_bytes: usize,
    retain_content: bool,
    content_store: ContentStore,
    state: Arc<AtomicU8>,
    outstanding_credits: Arc<AtomicU32>,
    credit_rx: std_mpsc::Receiver<()>,
    events_tx: mpsc::Sender<FileReadStreamEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerFlow {
    Continue,
    NeedCancelled,
    Terminal,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CreditReservation {
    Reserved,
    NeedCancelled,
    Stopped,
}

fn run_file_read_worker(worker: FileReadWorker) {
    let mut result_sequence = 0_u64;
    if stream_file(&worker, &mut result_sequence) == WorkerFlow::NeedCancelled {
        emit_cancelled(&worker, &mut result_sequence);
    }
}

fn stream_file(worker: &FileReadWorker, result_sequence: &mut u64) -> WorkerFlow {
    if worker.state.load(Ordering::Acquire) == FILE_WORKER_CANCEL_REQUESTED {
        return WorkerFlow::NeedCancelled;
    }
    if worker.state.load(Ordering::Acquire) == FILE_WORKER_DISCARD {
        return WorkerFlow::Stopped;
    }
    let mut file = match File::open(&worker.path) {
        Ok(file) => file,
        Err(error) => {
            return emit_outcome(
                worker,
                result_sequence,
                file_failure_outcome(FileStreamFailure::new(
                    "open_failed",
                    format!("cannot open selected file: {error}"),
                )),
                true,
            );
        }
    };
    let size_bytes = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return emit_outcome(
                worker,
                result_sequence,
                file_failure_outcome(FileStreamFailure::new(
                    "metadata_failed",
                    format!("cannot inspect selected file: {error}"),
                )),
                true,
            );
        }
    };
    let size = match file_number(size_bytes, "selected file size") {
        Ok(size) => size,
        Err(failure) => {
            return emit_outcome(worker, result_sequence, file_failure_outcome(failure), true);
        }
    };
    let display_name = worker.display_name.clone().unwrap_or_else(|| {
        worker
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("selected-file")
            .chars()
            .take(256)
            .collect::<String>()
    });
    let mut content_writer = if worker.retain_content {
        match worker.content_store.begin_write(size_bytes) {
            Ok(writer) => Some(writer),
            Err(error) => {
                return emit_outcome(
                    worker,
                    result_sequence,
                    file_failure_outcome(content_store_failure(error)),
                    true,
                );
            }
        }
    } else {
        None
    };
    match emit_outcome(
        worker,
        result_sequence,
        tagged(
            "Opened",
            BTreeMap::from([
                ("size".to_owned(), size),
                (
                    "content_type".to_owned(),
                    Value::Text(FILE_CONTENT_TYPE.to_owned()),
                ),
                ("display_name".to_owned(), Value::Text(display_name)),
            ]),
        ),
        false,
    ) {
        WorkerFlow::Continue => {}
        other => return other,
    }

    let mut digest = Sha256::new();
    let mut byte_count = 0_u64;
    let mut chunk_sequence = 0_u64;
    let mut buffer = vec![0_u8; worker.chunk_bytes];
    loop {
        match reserve_credit(worker, true) {
            CreditReservation::Reserved => {}
            CreditReservation::NeedCancelled => return WorkerFlow::NeedCancelled,
            CreditReservation::Stopped => return WorkerFlow::Stopped,
        }
        if worker.state.load(Ordering::Acquire) == FILE_WORKER_DISCARD {
            return WorkerFlow::Stopped;
        }
        let read = match file.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                return send_reserved_outcome(
                    worker,
                    result_sequence,
                    file_failure_outcome(FileStreamFailure::new(
                        "read_failed",
                        format!("cannot read selected file: {error}"),
                    )),
                    true,
                );
            }
        };
        if read == 0 {
            let digest = <[u8; 32]>::from(digest.finalize());
            let content = ContentRef::new(digest, byte_count);
            let retained_content = if let Some(writer) = content_writer.take() {
                match writer.finish(content) {
                    Ok(content) => Some(content),
                    Err(error) => {
                        return send_reserved_outcome(
                            worker,
                            result_sequence,
                            file_failure_outcome(content_store_failure(error)),
                            true,
                        );
                    }
                }
            } else {
                None
            };
            let byte_count_value = match file_number(byte_count, "stream byte count") {
                Ok(value) => value,
                Err(failure) => {
                    return send_reserved_outcome(
                        worker,
                        result_sequence,
                        file_failure_outcome(failure),
                        true,
                    );
                }
            };
            let content_value = match content.value() {
                Ok(value) => value,
                Err(error) => {
                    if let Some(content) = retained_content {
                        worker.content_store.remove(content);
                    }
                    return send_reserved_outcome(
                        worker,
                        result_sequence,
                        file_failure_outcome(content_store_failure(error)),
                        true,
                    );
                }
            };
            let flow = send_reserved_outcome(
                worker,
                result_sequence,
                tagged(
                    "Finished",
                    BTreeMap::from([
                        ("byte_count".to_owned(), byte_count_value),
                        ("digest".to_owned(), Value::Bytes(digest.to_vec())),
                        ("content".to_owned(), content_value),
                    ]),
                ),
                true,
            );
            if flow == WorkerFlow::Stopped
                && let Some(content) = retained_content
            {
                worker.content_store.remove(content);
            }
            return flow;
        }
        let offset = byte_count;
        let read_u64 = u64::try_from(read).expect("bounded read length fits u64");
        byte_count = match byte_count.checked_add(read_u64) {
            Some(byte_count) => byte_count,
            None => {
                return send_reserved_outcome(
                    worker,
                    result_sequence,
                    file_failure_outcome(FileStreamFailure::new(
                        "file_too_large",
                        "stream byte count exceeds the host range",
                    )),
                    true,
                );
            }
        };
        digest.update(&buffer[..read]);
        if let Some(writer) = content_writer.as_mut()
            && let Err(error) = writer.write_chunk(&buffer[..read])
        {
            return send_reserved_outcome(
                worker,
                result_sequence,
                file_failure_outcome(content_store_failure(error)),
                true,
            );
        }
        let sequence = match file_number(chunk_sequence, "chunk sequence") {
            Ok(value) => value,
            Err(failure) => {
                return send_reserved_outcome(
                    worker,
                    result_sequence,
                    file_failure_outcome(failure),
                    true,
                );
            }
        };
        let offset = match file_number(offset, "chunk offset") {
            Ok(value) => value,
            Err(failure) => {
                return send_reserved_outcome(
                    worker,
                    result_sequence,
                    file_failure_outcome(failure),
                    true,
                );
            }
        };
        match send_reserved_outcome(
            worker,
            result_sequence,
            tagged(
                "Chunk",
                BTreeMap::from([
                    ("sequence".to_owned(), sequence),
                    ("offset".to_owned(), offset),
                    ("bytes".to_owned(), Value::Bytes(buffer[..read].to_vec())),
                ]),
            ),
            false,
        ) {
            WorkerFlow::Continue => {}
            other => return other,
        }
        chunk_sequence = match chunk_sequence.checked_add(1) {
            Some(sequence) => sequence,
            None => return WorkerFlow::Stopped,
        };
    }
}

fn emit_outcome(
    worker: &FileReadWorker,
    result_sequence: &mut u64,
    outcome: Value,
    terminal: bool,
) -> WorkerFlow {
    match reserve_credit(worker, true) {
        CreditReservation::Reserved => {
            send_reserved_outcome(worker, result_sequence, outcome, terminal)
        }
        CreditReservation::NeedCancelled => WorkerFlow::NeedCancelled,
        CreditReservation::Stopped => WorkerFlow::Stopped,
    }
}

fn emit_cancelled(worker: &FileReadWorker, result_sequence: &mut u64) -> WorkerFlow {
    match reserve_credit(worker, false) {
        CreditReservation::Reserved => {
            send_reserved_outcome(worker, result_sequence, file_cancelled_outcome(), true)
        }
        CreditReservation::NeedCancelled => unreachable!("graceful cancellation is allowed"),
        CreditReservation::Stopped => WorkerFlow::Stopped,
    }
}

fn reserve_credit(worker: &FileReadWorker, stop_on_cancel_request: bool) -> CreditReservation {
    loop {
        match worker.state.load(Ordering::Acquire) {
            FILE_WORKER_DISCARD => return CreditReservation::Stopped,
            FILE_WORKER_CANCEL_REQUESTED if stop_on_cancel_request => {
                return CreditReservation::NeedCancelled;
            }
            _ => {}
        }
        let mut current = worker.outstanding_credits.load(Ordering::Acquire);
        while current > 0 {
            match worker.outstanding_credits.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return CreditReservation::Reserved,
                Err(actual) => current = actual,
            }
        }
        match worker.credit_rx.recv_timeout(FILE_WORKER_POLL_INTERVAL) {
            Ok(()) | Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                return CreditReservation::Stopped;
            }
        }
    }
}

fn send_reserved_outcome(
    worker: &FileReadWorker,
    result_sequence: &mut u64,
    mut outcome: Value,
    mut terminal: bool,
) -> WorkerFlow {
    let mut event = FileReadStreamEvent {
        call_id: worker.call_id,
        invocation_id: worker.invocation_id,
        result_sequence: *result_sequence,
        outcome: outcome.clone(),
        terminal,
    };
    loop {
        match worker.state.load(Ordering::Acquire) {
            FILE_WORKER_DISCARD => return WorkerFlow::Stopped,
            FILE_WORKER_CANCEL_REQUESTED if !terminal => {
                outcome = file_cancelled_outcome();
                terminal = true;
                event.outcome = outcome.clone();
                event.terminal = true;
            }
            _ => {}
        }
        match worker.events_tx.try_send(event) {
            Ok(()) => {
                *result_sequence = result_sequence.saturating_add(1);
                return if terminal {
                    WorkerFlow::Terminal
                } else {
                    WorkerFlow::Continue
                };
            }
            Err(mpsc::error::TrySendError::Full(returned)) => {
                event = returned;
                thread::sleep(FILE_WORKER_POLL_INTERVAL);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => return WorkerFlow::Stopped,
        }
    }
}

fn file_number(value: u64, context: &str) -> Result<Value, FileStreamFailure> {
    let value = i64::try_from(value).map_err(|_| {
        FileStreamFailure::new(
            "file_too_large",
            format!("{context} exceeds the Boon Number range"),
        )
    })?;
    let value = FiniteReal::from_i64_exact(value).map_err(|_| {
        FileStreamFailure::new(
            "file_too_large",
            format!("{context} is not exactly representable as a Boon Number"),
        )
    })?;
    Ok(Value::Number(value))
}

fn stop_file_worker(mut active: ActiveFileRead) {
    active.state.store(FILE_WORKER_DISCARD, Ordering::Release);
    let _ = active.credit_tx.try_send(());
    drop(active.credit_tx);
    if let Some(task) = active.task.take() {
        let _ = task.join();
    }
}

fn finish_file_worker(mut active: ActiveFileRead) {
    drop(active.credit_tx);
    if let Some(task) = active.task.take() {
        let _ = task.join();
    }
}

pub fn apply_submission(
    program: &mut ProgramSession,
    submission: HostServiceEffectSubmission,
) -> Result<Option<RuntimeTurn>, AdapterError> {
    submission
        .immediate_completion
        .map(|completion| apply_completion(program, completion))
        .transpose()
}

pub fn apply_completion(
    program: &mut ProgramSession,
    completion: HostServiceEffectCompletion,
) -> Result<RuntimeTurn, AdapterError> {
    program
        .complete_transient_effect(completion.call_id, completion.outcome)
        .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error))
}

fn effect_id(operation: &str) -> Result<EffectId, AdapterError> {
    EffectId::from_host_operation(operation)
        .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidConfiguration, error))
}

fn immediate_submission(
    invocation: TransientEffectInvocation,
    outcome: Value,
) -> HostServiceEffectSubmission {
    HostServiceEffectSubmission {
        call_id: invocation.call_id,
        immediate_completion: Some(HostServiceEffectCompletion {
            call_id: invocation.call_id,
            invocation_id: invocation.invocation_id,
            outcome,
        }),
    }
}

fn exact_record<'a>(
    value: &'a Value,
    expected: &[&str],
) -> Result<&'a BTreeMap<String, Value>, Value> {
    let Value::Record(fields) = value else {
        return Err(failure(
            "invalid_intent",
            "host-service effect intent must be a record",
        ));
    };
    let actual = fields.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(failure(
            "invalid_intent",
            "host-service effect fields differ from the typed contract",
        ));
    }
    Ok(fields)
}

fn text_field<'a>(fields: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, ()> {
    match fields.get(name) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(()),
    }
}

fn bytes_field<'a>(fields: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a [u8], ()> {
    match fields.get(name) {
        Some(Value::Bytes(value)) => Ok(value),
        _ => Err(()),
    }
}

fn positive_usize(fields: &BTreeMap<String, Value>, name: &str) -> Result<usize, Value> {
    let value = positive_i64(fields, name)?;
    usize::try_from(value).map_err(|_| {
        failure(
            "invalid_intent",
            "numeric field exceeds host platform range",
        )
    })
}

fn positive_u64(fields: &BTreeMap<String, Value>, name: &str) -> Result<u64, Value> {
    let value = positive_i64(fields, name)?;
    u64::try_from(value)
        .map_err(|_| failure("invalid_intent", "numeric field exceeds duration range"))
}

fn positive_i64(fields: &BTreeMap<String, Value>, name: &str) -> Result<i64, Value> {
    let Some(Value::Number(value)) = fields.get(name) else {
        return Err(failure(
            "invalid_intent",
            "host-service numeric field differs from the typed contract",
        ));
    };
    value
        .to_i64_exact()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            failure(
                "invalid_intent",
                "host-service numeric field must be a positive whole number",
            )
        })
}

fn tagged(tag: &str, mut fields: BTreeMap<String, Value>) -> Value {
    fields.insert("$tag".to_owned(), Value::Text(tag.to_owned()));
    Value::Record(fields)
}

fn number(value: i64) -> Value {
    Value::Number(FiniteReal::from_i64_exact(value).expect("i64 is exactly representable here"))
}

fn failure(code: &str, diagnostic: impl Into<String>) -> Value {
    tagged(
        "HostServiceFailed",
        BTreeMap::from([
            ("code".to_owned(), Value::Text(code.to_owned())),
            (
                "diagnostic".to_owned(),
                Value::Text(bounded_diagnostic(diagnostic.into())),
            ),
        ]),
    )
}

fn host_failure(error: HostServiceError) -> Value {
    let code = match &error {
        HostServiceError::CapabilityDisabled(_) => "capability_disabled",
        HostServiceError::Shutdown => "host_shutdown",
        HostServiceError::LimitExceeded { .. } => "limit_exceeded",
        HostServiceError::BelowMinimum { .. } => "below_minimum",
        HostServiceError::Time(_) => "time_error",
        HostServiceError::EmptySecret => "empty_secret",
        HostServiceError::SecretNotFound(_) => "secret_unavailable",
        HostServiceError::SecretIdExhausted => "secret_capacity",
        HostServiceError::OsRandomUnavailable(_) => "secure_random_unavailable",
        HostServiceError::DeterministicRandomExhausted => "random_provider_exhausted",
        HostServiceError::SchedulerUnavailable => "scheduler_unavailable",
    };
    failure(code, error.to_string())
}

fn timer_failure(error: TimerReceiveError) -> Value {
    let code = match error {
        TimerReceiveError::Cancelled => "timer_cancelled",
        TimerReceiveError::SchedulerShutdown => "scheduler_shutdown",
        TimerReceiveError::Completed => "timer_completed_without_event",
        TimerReceiveError::Empty | TimerReceiveError::Timeout => "timer_unavailable",
    };
    failure(code, error.to_string())
}

fn validate_secret_name(name: &str) -> Result<(), AdapterError> {
    if name.is_empty()
        || name.len() > MAX_SECRET_NAME_BYTES
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
    {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidConfiguration,
            "named secret must be bounded ASCII alphanumerics with '.', '_', '-', or '/'",
        ));
    }
    Ok(())
}

fn bounded_diagnostic(mut diagnostic: String) -> String {
    if diagnostic.len() <= MAX_DIAGNOSTIC_BYTES {
        return diagnostic;
    }
    let mut end = MAX_DIAGNOSTIC_BYTES;
    while !diagnostic.is_char_boundary(end) {
        end -= 1;
    }
    diagnostic.truncate(end);
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_secret_validation_is_strict_and_bounded() {
        for valid in ["session", "admin/password", "signing-key_v1"] {
            validate_secret_name(valid).unwrap();
        }
        for invalid in ["", "has space", "contains:colon"] {
            assert_eq!(
                validate_secret_name(invalid).unwrap_err().kind(),
                AdapterErrorKind::InvalidConfiguration
            );
        }
    }

    #[test]
    fn strict_intent_decoder_rejects_extra_fields() {
        let value = Value::Record(BTreeMap::from([
            ("byte_count".to_owned(), number(16)),
            ("unexpected".to_owned(), Value::Bool(true)),
        ]));
        let Value::Record(result) = exact_record(&value, &["byte_count"]).unwrap_err() else {
            panic!("failure must be a variant record");
        };
        assert_eq!(result["$tag"], Value::Text("HostServiceFailed".to_owned()));
        assert_eq!(result["code"], Value::Text("invalid_intent".to_owned()));
    }
}
