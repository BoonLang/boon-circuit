//! Typed runtime adapter for bounded clock, random, secret, HMAC, and timer services.

#![forbid(unsafe_code)]

mod content_store;

pub use boon_runtime::ContentRef;
pub use content_store::{
    ContentLease, ContentStore, ContentStoreError, ContentStoreErrorKind, ContentStoreLimits,
    ContentWriter,
};

use atomic_write_file::AtomicWriteFile;
use boon_host_services::{
    CancellationHandle, HMAC_SHA256_TAG_BYTES, HmacSha256Tag, HostServiceError, HostServices,
    SecretMaterial, SecretRef, TimerReceiveError,
};
use boon_plan::{EffectDeliveryCardinality, EffectId, EffectInvocationId, FiniteReal};
use boon_runtime::{
    ByteStreamValidator, EffectCommitPermit, EffectStopDisposition, EffectStopReason,
    EffectTerminalReservation, HostCapabilityErrorKind, HostCapabilityRegistry, HostValueBinding,
    ProgramSession, RuntimeTurn, TransientEffectCallId, TransientEffectCreditGrant,
    TransientEffectInvocation, Value,
};
use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
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

    pub fn try_next_completion(
        &mut self,
    ) -> Result<Option<HostServiceEffectCompletion>, AdapterError> {
        loop {
            let completion = match self.completions_rx.try_recv() {
                Ok(completion) => completion,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => return Ok(None),
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    return Err(AdapterError::new(
                        AdapterErrorKind::Closed,
                        "host-service completion lane closed",
                    ));
                }
            };
            let Some(active) = self.active_deadlines.remove(&completion.call_id) else {
                continue;
            };
            debug_assert_eq!(active.invocation_id, completion.invocation_id);
            return Ok(Some(completion));
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
                BTreeMap::from([(
                    "bytes".to_owned(),
                    Value::Bytes(bytes.as_bytes().to_vec().into()),
                )]),
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
                BTreeMap::from([(
                    "tag".to_owned(),
                    Value::Bytes(tag.into_bytes().to_vec().into()),
                )]),
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
const FILE_CAPABILITY_TOKEN_ATTEMPTS: usize = 16;
const FILE_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const FILE_EFFECT_DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const FILE_CONTENT_TYPE: &str = "application/octet-stream";

/// An opaque process-local reference to a host-owned file path.
///
/// Boon observes only the structural `FileSelected` or `FileTarget` tag. The
/// binding is carried by the executor outside ordinary serializable data and
/// is accepted only by the registry that issued it.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileCapability {
    binding: HostValueBinding,
}

impl FileCapability {
    pub fn file_selected_value(&self) -> Value {
        Value::host_bound(
            tagged("FileSelected", BTreeMap::new()),
            self.binding.clone(),
        )
    }

    pub fn file_target_value(&self) -> Value {
        Value::host_bound(tagged("FileTarget", BTreeMap::new()), self.binding.clone())
    }
}

impl fmt::Debug for FileCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileCapability")
            .field("binding", &self.binding)
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

struct ResolvedFileCapability {
    path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileCapabilityAccess {
    Source,
    Target,
}

/// Bounded registry that keeps selected paths entirely on the host side.
pub struct FileCapabilityRegistry {
    capabilities: HostCapabilityRegistry<PathBuf, FileCapabilityAccess>,
}

impl FileCapabilityRegistry {
    pub fn new(max_capabilities: usize) -> Result<Self, AdapterError> {
        if max_capabilities == 0 {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidConfiguration,
                "file capability capacity must be positive",
            ));
        }
        let mut issuer_identity = [0_u8; FILE_CAPABILITY_TOKEN_BYTES];
        getrandom::fill(&mut issuer_identity).map_err(|error| {
            AdapterError::new(
                AdapterErrorKind::Capability,
                format_args!("cannot initialize file capability issuer: {error}"),
            )
        })?;
        let capabilities = HostCapabilityRegistry::new(issuer_identity, max_capabilities)
            .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidConfiguration, error))?;
        Ok(Self { capabilities })
    }

    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }

    pub const fn capacity(&self) -> usize {
        self.capabilities.capacity()
    }

    pub fn register_file(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> Result<FileCapability, AdapterError> {
        self.register(path.into(), FileCapabilityAccess::Source)
    }

    pub fn register_target(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> Result<FileCapability, AdapterError> {
        self.register(path.into(), FileCapabilityAccess::Target)
    }

    fn register(
        &mut self,
        path: PathBuf,
        access: FileCapabilityAccess,
    ) -> Result<FileCapability, AdapterError> {
        if self.capabilities.len() >= self.capabilities.capacity() {
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
            match self.capabilities.register(token, path.clone(), access) {
                Ok(binding) => return Ok(FileCapability { binding }),
                Err(error) if error.kind() == HostCapabilityErrorKind::DuplicateHandle => continue,
                Err(error) => {
                    return Err(AdapterError::new(AdapterErrorKind::Capability, error));
                }
            }
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
        let binding = self
            .capabilities
            .replace(&capability.binding, path)
            .map_err(|error| AdapterError::new(AdapterErrorKind::Capability, error))?;
        Ok(FileCapability { binding })
    }

    pub fn revoke(&mut self, capability: &FileCapability) -> bool {
        self.capabilities.revoke(&capability.binding)
    }

    pub fn contains(&self, capability: &FileCapability) -> bool {
        self.capabilities.contains(&capability.binding)
    }

    fn resolve_source(
        &self,
        capability: &FileCapability,
    ) -> Result<ResolvedFileCapability, FileCapabilityLookup> {
        self.resolve(capability, FileCapabilityAccess::Source)
    }

    fn resolve_target(
        &self,
        capability: &FileCapability,
    ) -> Result<ResolvedFileCapability, FileCapabilityLookup> {
        self.resolve(capability, FileCapabilityAccess::Target)
    }

    fn resolve(
        &self,
        capability: &FileCapability,
        access: FileCapabilityAccess,
    ) -> Result<ResolvedFileCapability, FileCapabilityLookup> {
        let resolved = self
            .capabilities
            .resolve(&capability.binding, access)
            .map_err(|error| match error.kind() {
                HostCapabilityErrorKind::Stale => FileCapabilityLookup::Stale,
                HostCapabilityErrorKind::WrongAccess => FileCapabilityLookup::WrongAccess,
                HostCapabilityErrorKind::Foreign
                | HostCapabilityErrorKind::Unknown
                | HostCapabilityErrorKind::InvalidConfiguration
                | HostCapabilityErrorKind::Capacity
                | HostCapabilityErrorKind::DuplicateHandle
                | HostCapabilityErrorKind::GenerationExhausted => FileCapabilityLookup::Unknown,
            })?;
        Ok(ResolvedFileCapability {
            path: resolved.resource.clone(),
        })
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

fn target_resource_identity(path: &Path) -> Result<PathBuf, FileStreamFailure> {
    if path.exists() {
        return std::fs::canonicalize(path).map_err(|error| {
            FileStreamFailure::new(
                "invalid_target",
                format!("cannot resolve file target identity: {:?}", error.kind()),
            )
        });
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent =
        std::fs::canonicalize(parent.unwrap_or_else(|| Path::new("."))).map_err(|error| {
            FileStreamFailure::new(
                "invalid_target",
                format!("cannot resolve file target directory: {:?}", error.kind()),
            )
        })?;
    let name = path.file_name().ok_or_else(|| {
        FileStreamFailure::new("invalid_target", "file target has no final path component")
    })?;
    Ok(parent.join(name))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileCapabilityLookup {
    Unknown,
    Stale,
    WrongAccess,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileEffectLimits {
    pub max_active: usize,
    pub event_queue_capacity: usize,
    pub credit_queue_capacity: usize,
    pub operation_timeout: Duration,
}

impl FileEffectLimits {
    pub const fn new(
        max_active: usize,
        event_queue_capacity: usize,
        credit_queue_capacity: usize,
    ) -> Self {
        Self {
            max_active,
            event_queue_capacity,
            credit_queue_capacity,
            operation_timeout: FILE_EFFECT_DEFAULT_TIMEOUT,
        }
    }

    pub const fn with_operation_timeout(mut self, operation_timeout: Duration) -> Self {
        self.operation_timeout = operation_timeout;
        self
    }
}

impl Default for FileEffectLimits {
    fn default() -> Self {
        Self {
            max_active: 8,
            event_queue_capacity: 32,
            credit_queue_capacity: boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT as usize,
            operation_timeout: FILE_EFFECT_DEFAULT_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileEffectEvent {
    pub call_id: TransientEffectCallId,
    pub invocation_id: EffectInvocationId,
    pub result_sequence: u64,
    pub outcome: Value,
    terminal: bool,
    stream: bool,
}

impl FileEffectEvent {
    pub const fn is_terminal(&self) -> bool {
        self.terminal
    }

    pub const fn is_stream(&self) -> bool {
        self.stream
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
pub struct FileEffectSubmission {
    pub call_id: TransientEffectCallId,
    pub queued_terminal: bool,
}

struct ActiveFileOperation {
    invocation_id: EffectInvocationId,
    max_in_flight: u32,
    permit: EffectCommitPermit,
    deadline: Instant,
    sequence_lock: Arc<Mutex<()>>,
    next_emitted_sequence: Arc<AtomicU64>,
    next_accepted_sequence: u64,
    finished: Arc<AtomicBool>,
    outstanding_credits: Arc<AtomicU32>,
    credit_tx: std_mpsc::SyncSender<()>,
    task: Option<thread::JoinHandle<()>>,
    target_resource: Option<PathBuf>,
    byte_stream_validator: Option<ByteStreamValidator>,
}

struct FileWorkerControl {
    call_id: TransientEffectCallId,
    invocation_id: EffectInvocationId,
    permit: EffectCommitPermit,
    deadline: Instant,
    sequence_lock: Arc<Mutex<()>>,
    next_emitted_sequence: Arc<AtomicU64>,
    outstanding_credits: Arc<AtomicU32>,
    credit_rx: std_mpsc::Receiver<()>,
    events_tx: mpsc::Sender<FileEffectEvent>,
    stream: bool,
}

#[derive(Clone, Copy, Debug)]
struct FileWorkerExit {
    call_id: TransientEffectCallId,
    invocation_id: EffectInvocationId,
    panicked: bool,
}

#[derive(Clone, Copy)]
struct FileEffectIds {
    read_bytes: EffectId,
    write_bytes: EffectId,
    read_stream: EffectId,
    content_import: EffectId,
    content_save: EffectId,
}

impl FileEffectIds {
    fn new() -> Result<Self, AdapterError> {
        Ok(Self {
            read_bytes: effect_id(boon_effect_schema::FILE_READ_BYTES_OPERATION)?,
            write_bytes: effect_id(boon_effect_schema::FILE_WRITE_BYTES_OPERATION)?,
            read_stream: effect_id(boon_effect_schema::FILE_READ_STREAM_OPERATION)?,
            content_import: effect_id(boon_effect_schema::CONTENT_IMPORT_OPERATION)?,
            content_save: effect_id(boon_effect_schema::CONTENT_SAVE_OPERATION)?,
        })
    }

    fn operation(self, effect_id: EffectId) -> Option<FileOperation> {
        if effect_id == self.read_bytes {
            Some(FileOperation::ReadBytes)
        } else if effect_id == self.write_bytes {
            Some(FileOperation::WriteBytes)
        } else if effect_id == self.read_stream {
            Some(FileOperation::ReadStream)
        } else if effect_id == self.content_import {
            Some(FileOperation::ContentImport)
        } else if effect_id == self.content_save {
            Some(FileOperation::ContentSave)
        } else {
            None
        }
    }

    const fn all(self) -> [EffectId; 5] {
        [
            self.read_bytes,
            self.write_bytes,
            self.read_stream,
            self.content_import,
            self.content_save,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileOperation {
    ReadBytes,
    WriteBytes,
    ReadStream,
    ContentImport,
    ContentSave,
}

struct RegisteredPackageAsset {
    content: ContentLease,
    display_name: String,
}

/// Bounded host adapter for the typed File and Content effects.
pub struct FileEffectAdapter {
    capabilities: FileCapabilityRegistry,
    package_assets: BTreeMap<String, RegisteredPackageAsset>,
    content_store: ContentStore,
    effects: FileEffectIds,
    limits: FileEffectLimits,
    active: BTreeMap<TransientEffectCallId, ActiveFileOperation>,
    retired: BTreeMap<TransientEffectCallId, ActiveFileOperation>,
    busy_targets: BTreeMap<PathBuf, TransientEffectCallId>,
    queued_terminals: BTreeMap<TransientEffectCallId, EffectInvocationId>,
    pending_host_events: VecDeque<FileEffectEvent>,
    events_tx: mpsc::Sender<FileEffectEvent>,
    events_rx: mpsc::Receiver<FileEffectEvent>,
    worker_exits_tx: mpsc::UnboundedSender<FileWorkerExit>,
    worker_exits_rx: mpsc::UnboundedReceiver<FileWorkerExit>,
}

impl FileEffectAdapter {
    pub fn new(
        capabilities: FileCapabilityRegistry,
        content_store: ContentStore,
        max_active: usize,
    ) -> Result<Self, AdapterError> {
        let events_per_active = (boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT as usize)
            .checked_add(1)
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::InvalidConfiguration,
                    "file stream event capacity overflow",
                )
            })?;
        let event_queue_capacity = max_active.checked_mul(events_per_active).ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::InvalidConfiguration,
                "file stream event capacity overflow",
            )
        })?;
        Self::with_limits(
            capabilities,
            content_store,
            FileEffectLimits::new(
                max_active,
                event_queue_capacity,
                boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT as usize,
            ),
        )
    }

    pub fn with_limits(
        capabilities: FileCapabilityRegistry,
        content_store: ContentStore,
        limits: FileEffectLimits,
    ) -> Result<Self, AdapterError> {
        if limits.max_active == 0
            || limits.event_queue_capacity == 0
            || limits.credit_queue_capacity == 0
            || limits.operation_timeout.is_zero()
        {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidConfiguration,
                "file stream active, event, credit, and timeout limits must be positive",
            ));
        }
        let effects = FileEffectIds::new()?;
        let (events_tx, events_rx) = mpsc::channel(limits.event_queue_capacity);
        let (worker_exits_tx, worker_exits_rx) = mpsc::unbounded_channel();
        Ok(Self {
            capabilities,
            package_assets: BTreeMap::new(),
            content_store,
            effects,
            limits,
            active: BTreeMap::new(),
            retired: BTreeMap::new(),
            busy_targets: BTreeMap::new(),
            queued_terminals: BTreeMap::new(),
            pending_host_events: VecDeque::new(),
            events_tx,
            events_rx,
            worker_exits_tx,
            worker_exits_rx,
        })
    }

    pub const fn effect_ids(&self) -> [EffectId; 5] {
        self.effects.all()
    }

    pub fn owns_effect(&self, effect_id: EffectId) -> bool {
        self.effects.operation(effect_id).is_some()
    }

    pub const fn limits(&self) -> FileEffectLimits {
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
        media: impl Into<Arc<str>>,
        bytes: &[u8],
    ) -> Result<ContentRef, AdapterError> {
        let url = url.into();
        validate_package_asset_url(&url)?;
        let content = self
            .content_store
            .insert_bytes(bytes, media)
            .map_err(|error| AdapterError::new(AdapterErrorKind::Capacity, error))?;
        let lease = self
            .content_store
            .resolve(&content)
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

    pub fn retired_worker_count(&self) -> usize {
        self.retired.len()
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
    ) -> Result<FileEffectSubmission, AdapterError> {
        self.reap_retired_workers();
        let operation = self
            .effects
            .operation(invocation.effect_id)
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::NotOwned,
                    format_args!("file adapter does not own effect {}", invocation.effect_id),
                )
            })?;
        if self.active.contains_key(&invocation.call_id)
            || self.retired.contains_key(&invocation.call_id)
            || self.queued_terminals.contains_key(&invocation.call_id)
        {
            return Err(AdapterError::new(
                AdapterErrorKind::NotOwned,
                "file effect call ID is already owned by the adapter",
            ));
        }

        match operation {
            FileOperation::ReadBytes => self.submit_read_bytes(invocation),
            FileOperation::WriteBytes => self.submit_write_bytes(invocation),
            FileOperation::ReadStream => self.submit_read_stream(invocation),
            FileOperation::ContentImport => self.submit_content_import(invocation),
            FileOperation::ContentSave => self.submit_content_save(invocation),
        }
    }

    fn submit_read_bytes(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<FileEffectSubmission, AdapterError> {
        validate_single_file_delivery(&invocation.delivery, "File/read_bytes")?;
        let decoded = match decode_file_read_bytes_intent(&invocation.intent) {
            Ok(decoded) => decoded,
            Err(failure) => return self.queue_terminal_failure(invocation, failure),
        };
        let source = match self.resolve_source(decoded.source) {
            Ok(source) => source,
            Err(failure) => return self.queue_terminal_failure(invocation, failure),
        };
        self.start_operation(
            invocation,
            0,
            0,
            None,
            None,
            "file-read-bytes",
            move |control| {
                run_file_read_bytes_worker(FileReadBytesWorker {
                    control,
                    source,
                    max_bytes: decoded.max_bytes,
                })
            },
        )
    }

    fn submit_write_bytes(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<FileEffectSubmission, AdapterError> {
        validate_single_file_delivery(&invocation.delivery, "File/write_bytes")?;
        let decoded = match decode_file_write_bytes_intent(&invocation.intent) {
            Ok(decoded) => decoded,
            Err(failure) => return self.queue_terminal_failure(invocation, failure),
        };
        let target = match self.capabilities.resolve_target(&decoded.target) {
            Ok(target) => target,
            Err(error) => {
                return self.queue_terminal_failure(invocation, file_target_lookup_failure(error));
            }
        };
        let target_path = target.path;
        let target_resource = match target_resource_identity(&target_path) {
            Ok(resource) => resource,
            Err(failure) => return self.queue_terminal_failure(invocation, failure),
        };
        self.start_operation(
            invocation,
            0,
            0,
            Some(target_resource),
            None,
            "file-write-bytes",
            move |control| {
                run_file_write_bytes_worker(FileWriteBytesWorker {
                    control,
                    target_path,
                    bytes: decoded.bytes,
                })
            },
        )
    }

    fn submit_content_import(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<FileEffectSubmission, AdapterError> {
        let (initial_credits, max_in_flight) = validate_content_stream_delivery(
            &invocation.delivery,
            &["Busy", "Cancelled", "Failed", "Imported"],
            "Content/import",
        )?;
        let source = match decode_content_import_intent(&invocation.intent)
            .and_then(|source| self.resolve_source(source))
        {
            Ok(source) => source,
            Err(failure) => return self.queue_terminal_failure(invocation, failure),
        };
        let content_store = self.content_store.clone();
        self.start_operation(
            invocation,
            initial_credits,
            max_in_flight,
            None,
            None,
            "content-import",
            move |control| {
                run_content_import_worker(ContentImportWorker {
                    control,
                    source,
                    content_store,
                })
            },
        )
    }

    fn submit_content_save(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<FileEffectSubmission, AdapterError> {
        let (initial_credits, max_in_flight) = validate_content_stream_delivery(
            &invocation.delivery,
            &["Busy", "Cancelled", "Failed", "Saved"],
            "Content/save",
        )?;
        let decoded = match decode_content_save_intent(&invocation.intent) {
            Ok(decoded) => decoded,
            Err(failure) => return self.queue_terminal_failure(invocation, failure),
        };
        let target = match self.capabilities.resolve_target(&decoded.target) {
            Ok(target) => target,
            Err(error) => {
                return self.queue_terminal_failure(invocation, file_target_lookup_failure(error));
            }
        };
        let content = match self.content_store.resolve(&decoded.content) {
            Ok(content) => content,
            Err(error) => {
                return self.queue_terminal_failure(invocation, content_store_failure(error));
            }
        };
        let target_path = target.path;
        let target_resource = match target_resource_identity(&target_path) {
            Ok(resource) => resource,
            Err(failure) => return self.queue_terminal_failure(invocation, failure),
        };
        self.start_operation(
            invocation,
            initial_credits,
            max_in_flight,
            Some(target_resource),
            None,
            "content-save",
            move |control| {
                run_content_save_worker(ContentSaveWorker {
                    control,
                    content,
                    target_path,
                })
            },
        )
    }

    fn submit_read_stream(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<FileEffectSubmission, AdapterError> {
        let (initial_credits, max_in_flight) = validate_file_stream_delivery(&invocation.delivery)?;

        let decoded = match decode_file_read_stream_intent(&invocation.intent) {
            Ok(decoded) => decoded,
            Err(failure) => {
                return self.queue_terminal_failure(invocation, failure);
            }
        };
        let source = match self.resolve_source(decoded.source) {
            Ok(source) => source,
            Err(failure) => return self.queue_terminal_failure(invocation, failure),
        };
        let byte_stream_validator = ByteStreamValidator::new(decoded.chunk_bytes)
            .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidConfiguration, error))?;
        let worker = FileReadWorkerInput {
            path: source.path,
            display_name: source.display_name,
            _source_lease: source.lease,
            media: source.media,
            chunk_bytes: decoded.chunk_bytes,
            retain_content: decoded.retain_content,
            content_store: self.content_store.clone(),
        };
        self.start_operation(
            invocation,
            initial_credits,
            max_in_flight,
            None,
            Some(byte_stream_validator),
            "file-read-stream",
            move |control| {
                run_file_read_worker(FileReadWorker {
                    control,
                    input: worker,
                })
            },
        )
    }

    fn resolve_source(
        &self,
        source: DecodedFileSource,
    ) -> Result<ResolvedFileSource, FileStreamFailure> {
        match source {
            DecodedFileSource::Capability(capability) => {
                let source = self
                    .capabilities
                    .resolve_source(&capability)
                    .map_err(file_source_lookup_failure)?;
                Ok(ResolvedFileSource {
                    path: source.path,
                    display_name: None,
                    lease: None,
                    media: Arc::<str>::from(FILE_CONTENT_TYPE),
                })
            }
            DecodedFileSource::PackageAsset(url) => {
                let asset = self.package_assets.get(&url).ok_or_else(|| {
                    FileStreamFailure::new(
                        "unknown_package_asset",
                        "package asset is absent from the active application",
                    )
                })?;
                let lease = self
                    .content_store
                    .resolve(asset.content.content())
                    .map_err(content_store_failure)?;
                Ok(ResolvedFileSource {
                    path: lease.path().to_path_buf(),
                    display_name: Some(asset.display_name.clone()),
                    media: Arc::<str>::from(asset.content.content().media()),
                    lease: Some(lease),
                })
            }
        }
    }

    fn start_operation<F>(
        &mut self,
        invocation: TransientEffectInvocation,
        initial_credits: u32,
        max_in_flight: u32,
        target_resource: Option<PathBuf>,
        byte_stream_validator: Option<ByteStreamValidator>,
        worker_name: &'static str,
        worker: F,
    ) -> Result<FileEffectSubmission, AdapterError>
    where
        F: FnOnce(FileWorkerControl) + Send + 'static,
    {
        if self.active.len().saturating_add(self.retired.len()) >= self.limits.max_active {
            return self.queue_terminal_failure(
                invocation,
                FileStreamFailure::new(
                    "host_busy",
                    "file host is at its configured active-operation limit",
                ),
            );
        }
        self.reap_retired_workers();
        if let Some(resource) = target_resource.as_ref()
            && self.busy_targets.contains_key(resource)
        {
            return self.queue_terminal_outcome(invocation, tagged("Busy", BTreeMap::new()));
        }

        let call_id = invocation.call_id;
        let invocation_id = invocation.invocation_id;
        let deadline = Instant::now()
            .checked_add(self.limits.operation_timeout)
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::InvalidConfiguration,
                    "file operation timeout exceeds the monotonic clock range",
                )
            })?;
        let permit = EffectCommitPermit::new();
        let sequence_lock = Arc::new(Mutex::new(()));
        let next_emitted_sequence = Arc::new(AtomicU64::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let outstanding_credits = Arc::new(AtomicU32::new(initial_credits));
        let (credit_tx, credit_rx) = std_mpsc::sync_channel(self.limits.credit_queue_capacity);
        let control = FileWorkerControl {
            call_id,
            invocation_id,
            permit: permit.clone(),
            deadline,
            sequence_lock: Arc::clone(&sequence_lock),
            next_emitted_sequence: Arc::clone(&next_emitted_sequence),
            outstanding_credits: Arc::clone(&outstanding_credits),
            credit_rx,
            events_tx: self.events_tx.clone(),
            stream: max_in_flight > 0,
        };
        let worker_exits_tx = self.worker_exits_tx.clone();
        let worker_finished = Arc::clone(&finished);
        let task = match thread::Builder::new()
            .name(format!("boon-{worker_name}"))
            .spawn(move || {
                let panicked = catch_unwind(AssertUnwindSafe(|| worker(control))).is_err();
                worker_finished.store(true, Ordering::Release);
                let _ = worker_exits_tx.send(FileWorkerExit {
                    call_id,
                    invocation_id,
                    panicked,
                });
            }) {
            Ok(task) => task,
            Err(error) => {
                return self.queue_terminal_failure(
                    invocation,
                    FileStreamFailure::new(
                        "worker_unavailable",
                        format!("cannot start bounded file worker: {error}"),
                    ),
                );
            }
        };
        if let Some(resource) = target_resource.as_ref() {
            self.busy_targets.insert(resource.clone(), call_id);
        }
        self.active.insert(
            call_id,
            ActiveFileOperation {
                invocation_id,
                max_in_flight,
                permit,
                deadline,
                sequence_lock,
                next_emitted_sequence,
                next_accepted_sequence: 0,
                finished,
                outstanding_credits,
                credit_tx,
                task: Some(task),
                target_resource,
                byte_stream_validator,
            },
        );
        Ok(FileEffectSubmission {
            call_id,
            queued_terminal: false,
        })
    }

    pub async fn next_event(&mut self) -> Result<FileEffectEvent, AdapterError> {
        loop {
            self.reap_retired_workers();
            self.drain_worker_exits()?;
            if let Some(event) = self.try_take_ready_event()? {
                return Ok(event);
            }
            self.expire_deadlines()?;
            if let Some(event) = self.try_take_ready_event()? {
                return Ok(event);
            }
            let event = if let Some(deadline) = self.next_deadline() {
                tokio::select! {
                    biased;
                    event = self.events_rx.recv() => event,
                    exit = self.worker_exits_rx.recv() => {
                        if let Some(exit) = exit {
                            self.accept_worker_exit(exit)?;
                        }
                        continue;
                    }
                    () = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                        continue;
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    event = self.events_rx.recv() => event,
                    exit = self.worker_exits_rx.recv() => {
                        if let Some(exit) = exit {
                            self.accept_worker_exit(exit)?;
                        }
                        continue;
                    }
                }
            }
            .ok_or_else(|| {
                AdapterError::new(AdapterErrorKind::Closed, "file stream event lane closed")
            })?;
            if let Some(event) = self.accept_event(event)? {
                return Ok(event);
            }
        }
    }

    pub fn try_next_event(&mut self) -> Result<Option<FileEffectEvent>, AdapterError> {
        self.reap_retired_workers();
        self.drain_worker_exits()?;
        if let Some(event) = self.try_take_ready_event()? {
            return Ok(Some(event));
        }
        self.expire_deadlines()?;
        self.try_take_ready_event()
    }

    fn try_take_ready_event(&mut self) -> Result<Option<FileEffectEvent>, AdapterError> {
        loop {
            match self.events_rx.try_recv() {
                Ok(event) => {
                    if let Some(event) = self.accept_event(event)? {
                        return Ok(Some(event));
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if self.pending_host_events.is_empty() {
                        return Err(AdapterError::new(
                            AdapterErrorKind::Closed,
                            "file stream event lane closed",
                        ));
                    }
                    break;
                }
            }
        }
        while let Some(event) = self.pending_host_events.pop_front() {
            if let Some(event) = self.accept_event(event)? {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.active
            .values()
            .filter_map(|active| {
                active
                    .permit
                    .stop_reason()
                    .is_none()
                    .then_some(active.deadline)
            })
            .min()
    }

    fn expire_deadlines(&mut self) -> Result<(), AdapterError> {
        let now = Instant::now();
        let expired = self
            .active
            .iter()
            .filter_map(|(call_id, active)| {
                (active.deadline <= now && active.permit.stop_reason().is_none())
                    .then_some(*call_id)
            })
            .collect::<Vec<_>>();
        for call_id in expired {
            let Some(active) = self.active.get_mut(&call_id) else {
                continue;
            };
            let _sequence_guard = active
                .sequence_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if active.permit.request_timeout()
                == EffectStopDisposition::Accepted(EffectStopReason::TimedOut)
            {
                let _ = active.credit_tx.try_send(());
            }
        }
        Ok(())
    }

    /// Requests an in-contract `Cancelled` terminal result.
    pub fn request_cancel(&mut self, call_id: TransientEffectCallId) -> bool {
        let Some(active) = self.active.get_mut(&call_id) else {
            return false;
        };
        let _sequence_guard = active
            .sequence_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.permit.request_cancel()
            == EffectStopDisposition::Accepted(EffectStopReason::Cancelled)
        {
            let _ = active.credit_tx.try_send(());
            return true;
        }
        false
    }

    /// Discards a runtime-cancelled call without trying to deliver another result.
    pub fn cancel(&mut self, call_id: TransientEffectCallId) -> bool {
        let mut found = false;
        if let Some(active) = self.active.remove(&call_id) {
            found = true;
            active.permit.discard();
            let _ = active.credit_tx.try_send(());
            self.retired.insert(call_id, active);
        }
        if self.queued_terminals.remove(&call_id).is_some() {
            found = true;
        }
        found
    }

    fn reap_retired_workers(&mut self) {
        let finished = self
            .retired
            .iter()
            .filter_map(|(call_id, active)| {
                active.finished.load(Ordering::Acquire).then_some(*call_id)
            })
            .collect::<Vec<_>>();
        for call_id in finished {
            let active = self
                .retired
                .remove(&call_id)
                .expect("finished retired worker was just observed");
            self.release_busy_target(call_id, &active);
            release_file_worker(active);
        }
    }

    fn drain_worker_exits(&mut self) -> Result<(), AdapterError> {
        loop {
            match self.worker_exits_rx.try_recv() {
                Ok(exit) => self.accept_worker_exit(exit)?,
                Err(mpsc::error::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::error::TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }

    fn accept_worker_exit(&mut self, exit: FileWorkerExit) -> Result<(), AdapterError> {
        if self
            .retired
            .get(&exit.call_id)
            .is_some_and(|active| active.invocation_id == exit.invocation_id)
        {
            self.reap_retired_workers();
            return Ok(());
        }
        if !exit.panicked {
            return Ok(());
        }
        let Some(active) = self.active.get_mut(&exit.call_id) else {
            return Ok(());
        };
        if active.invocation_id != exit.invocation_id {
            return Ok(());
        }
        let _sequence_guard = active
            .sequence_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let outcome = match active.permit.reserve_terminal() {
            EffectTerminalReservation::Deliver => file_failure_outcome(FileStreamFailure::new(
                "worker_panicked",
                "file worker terminated unexpectedly",
            )),
            EffectTerminalReservation::Cancelled => file_cancelled_outcome(),
            EffectTerminalReservation::TimedOut => file_timed_out_outcome(),
            EffectTerminalReservation::Discarded | EffectTerminalReservation::AlreadyReserved => {
                return Ok(());
            }
        };
        let result_sequence = active.next_emitted_sequence.load(Ordering::Acquire);
        let next_sequence = result_sequence.checked_add(1).ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Capacity,
                "file effect result sequence overflow",
            )
        })?;
        active
            .next_emitted_sequence
            .store(next_sequence, Ordering::Release);
        self.pending_host_events.push_back(FileEffectEvent {
            call_id: exit.call_id,
            invocation_id: exit.invocation_id,
            result_sequence,
            outcome,
            terminal: true,
            stream: active.max_in_flight > 0,
        });
        Ok(())
    }

    fn release_busy_target(
        &mut self,
        call_id: TransientEffectCallId,
        active: &ActiveFileOperation,
    ) {
        if let Some(resource) = active.target_resource.as_ref()
            && self.busy_targets.get(resource) == Some(&call_id)
        {
            self.busy_targets.remove(resource);
        }
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
        failure: FileStreamFailure,
    ) -> Result<FileEffectSubmission, AdapterError> {
        self.queue_terminal_outcome(invocation, file_failure_outcome(failure))
    }

    fn queue_terminal_outcome(
        &mut self,
        invocation: TransientEffectInvocation,
        outcome: Value,
    ) -> Result<FileEffectSubmission, AdapterError> {
        let event = FileEffectEvent {
            call_id: invocation.call_id,
            invocation_id: invocation.invocation_id,
            result_sequence: 0,
            outcome,
            terminal: true,
            stream: matches!(
                invocation.delivery,
                EffectDeliveryCardinality::Stream { .. }
            ),
        };
        match self.events_tx.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(AdapterError::new(
                    AdapterErrorKind::Capacity,
                    "file effect event queue is full",
                ));
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(AdapterError::new(
                    AdapterErrorKind::Closed,
                    "file effect event lane closed",
                ));
            }
        }
        self.queued_terminals
            .insert(invocation.call_id, invocation.invocation_id);
        Ok(FileEffectSubmission {
            call_id: invocation.call_id,
            queued_terminal: true,
        })
    }

    fn accept_event(
        &mut self,
        event: FileEffectEvent,
    ) -> Result<Option<FileEffectEvent>, AdapterError> {
        if let Some(invocation_id) = self.queued_terminals.remove(&event.call_id) {
            if invocation_id != event.invocation_id || !event.terminal || event.result_sequence != 0
            {
                return Ok(None);
            }
            return Ok(Some(event));
        }
        let Some(active) = self.active.get_mut(&event.call_id) else {
            return Ok(None);
        };
        if active.invocation_id != event.invocation_id {
            self.cancel(event.call_id);
            return Ok(None);
        }
        if event.result_sequence != active.next_accepted_sequence {
            let expected = active.next_accepted_sequence;
            self.cancel(event.call_id);
            return Err(AdapterError::new(
                AdapterErrorKind::Worker,
                format_args!(
                    "file effect expected result sequence {expected}, received {}",
                    event.result_sequence
                ),
            ));
        }
        active.next_accepted_sequence =
            active
                .next_accepted_sequence
                .checked_add(1)
                .ok_or_else(|| {
                    AdapterError::new(
                        AdapterErrorKind::Capacity,
                        "file effect accepted result sequence overflow",
                    )
                })?;
        if let Some(validator) = active.byte_stream_validator.as_mut()
            && let Err(error) =
                validator.accept(event.result_sequence, &event.outcome, event.terminal)
        {
            self.cancel(event.call_id);
            return Err(AdapterError::new(AdapterErrorKind::Worker, error));
        }
        if event.terminal {
            let active = self
                .active
                .remove(&event.call_id)
                .expect("active file call was just observed");
            if active.finished.load(Ordering::Acquire) {
                self.release_busy_target(event.call_id, &active);
                release_file_worker(active);
            } else {
                self.retired.insert(event.call_id, active);
            }
        }
        Ok(Some(event))
    }
}

impl Drop for FileEffectAdapter {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

pub fn apply_file_effect_event(
    program: &mut ProgramSession,
    adapter: &mut FileEffectAdapter,
    event: FileEffectEvent,
) -> Result<RuntimeTurn, AdapterError> {
    let call_id = event.call_id;
    let delivered = if event.is_stream() {
        program.deliver_transient_effect_result(event.call_id, event.result_sequence, event.outcome)
    } else {
        program.complete_transient_effect(event.call_id, event.outcome)
    };
    let turn = match delivered {
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
    adapter: &mut FileEffectAdapter,
    event: FileEffectEvent,
) -> Result<RuntimeTurn, AdapterError> {
    apply_file_effect_event(program, adapter, event)
}

fn validate_file_stream_delivery(
    delivery: &EffectDeliveryCardinality,
) -> Result<(u32, u32), AdapterError> {
    let EffectDeliveryCardinality::Stream {
        initial_credits,
        max_in_flight,
        credit_result_tags,
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
        || !credit_result_tags.iter().map(String::as_str).eq(["Chunk"])
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

fn validate_single_file_delivery(
    delivery: &EffectDeliveryCardinality,
    operation: &str,
) -> Result<(), AdapterError> {
    if !matches!(delivery, EffectDeliveryCardinality::Single) {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidDelivery,
            format_args!("{operation} requires declared Single delivery"),
        ));
    }
    Ok(())
}

fn validate_content_stream_delivery(
    delivery: &EffectDeliveryCardinality,
    expected_terminal_tags: &[&str],
    operation: &str,
) -> Result<(u32, u32), AdapterError> {
    let EffectDeliveryCardinality::Stream {
        initial_credits,
        max_in_flight,
        credit_result_tags,
        terminal_result_tags,
    } = delivery
    else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidDelivery,
            format_args!("{operation} requires declared Stream delivery"),
        ));
    };
    if *initial_credits != boon_effect_schema::FILE_STREAM_INITIAL_CREDITS
        || *max_in_flight != boon_effect_schema::FILE_STREAM_MAX_IN_FLIGHT
        || !credit_result_tags
            .iter()
            .map(String::as_str)
            .eq(["Progress"])
        || !terminal_result_tags
            .iter()
            .map(String::as_str)
            .eq(expected_terminal_tags.iter().copied())
    {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidDelivery,
            format_args!("{operation} delivery differs from the bounded typed contract"),
        ));
    }
    Ok((*initial_credits, *max_in_flight))
}

enum DecodedFileSource {
    Capability(FileCapability),
    PackageAsset(String),
}

struct ResolvedFileSource {
    path: PathBuf,
    display_name: Option<String>,
    lease: Option<ContentLease>,
    media: Arc<str>,
}

fn file_source_lookup_failure(error: FileCapabilityLookup) -> FileStreamFailure {
    match error {
        FileCapabilityLookup::Unknown => FileStreamFailure::new(
            "unknown_capability",
            "file capability is unknown or revoked",
        ),
        FileCapabilityLookup::Stale => {
            FileStreamFailure::new("stale_capability", "file capability generation is stale")
        }
        FileCapabilityLookup::WrongAccess => FileStreamFailure::new(
            "wrong_capability_access",
            "file target capability cannot be used as a read source",
        ),
    }
}

fn file_target_lookup_failure(error: FileCapabilityLookup) -> FileStreamFailure {
    match error {
        FileCapabilityLookup::Unknown => FileStreamFailure::new(
            "unknown_capability",
            "file target capability is unknown or revoked",
        ),
        FileCapabilityLookup::Stale => FileStreamFailure::new(
            "stale_capability",
            "file target capability generation is stale",
        ),
        FileCapabilityLookup::WrongAccess => FileStreamFailure::new(
            "wrong_capability_access",
            "file source capability cannot be used as a write target",
        ),
    }
}

struct DecodedFileReadIntent {
    source: DecodedFileSource,
    chunk_bytes: usize,
    retain_content: bool,
}

struct DecodedFileReadBytesIntent {
    source: DecodedFileSource,
    max_bytes: usize,
}

struct DecodedFileWriteBytesIntent {
    target: FileCapability,
    bytes: Bytes,
}

struct DecodedContentSaveIntent {
    content: ContentRef,
    target: FileCapability,
}

fn decode_file_read_bytes_intent(
    value: &Value,
) -> Result<DecodedFileReadBytesIntent, FileStreamFailure> {
    let fields = file_record(value, &["file", "max_bytes"], "file read-bytes intent")?;
    let source = decode_file_source(
        fields
            .get("file")
            .ok_or_else(|| FileStreamFailure::invalid("read-bytes intent is missing `file`"))?,
    )?;
    let max_bytes = file_positive_u64(fields, "max_bytes")?;
    if !(boon_effect_schema::FILE_BYTES_MIN_LIMIT..=boon_effect_schema::FILE_BYTES_MAX_LIMIT)
        .contains(&max_bytes)
    {
        return Err(FileStreamFailure::invalid(
            "read-bytes max_bytes is outside the typed bounded range",
        ));
    }
    let max_bytes = usize::try_from(max_bytes).map_err(|_| {
        FileStreamFailure::invalid("read-bytes max_bytes exceeds the host platform range")
    })?;
    Ok(DecodedFileReadBytesIntent { source, max_bytes })
}

fn decode_file_write_bytes_intent(
    value: &Value,
) -> Result<DecodedFileWriteBytesIntent, FileStreamFailure> {
    let fields = file_record(value, &["bytes", "file"], "file write-bytes intent")?;
    let target = decode_file_target(
        fields
            .get("file")
            .ok_or_else(|| FileStreamFailure::invalid("write-bytes intent is missing `file`"))?,
    )?;
    let bytes = match fields.get("bytes") {
        Some(Value::Bytes(bytes)) => bytes.clone(),
        _ => {
            return Err(FileStreamFailure::invalid(
                "write-bytes `bytes` must be Bytes",
            ));
        }
    };
    if bytes.len() as u64 > boon_effect_schema::FILE_BYTES_MAX_LIMIT {
        return Err(FileStreamFailure::new(
            "file_too_large",
            "write-bytes payload exceeds the bounded small-file limit",
        ));
    }
    Ok(DecodedFileWriteBytesIntent { target, bytes })
}

fn decode_content_import_intent(value: &Value) -> Result<DecodedFileSource, FileStreamFailure> {
    let fields = file_record(value, &["file"], "content import intent")?;
    decode_file_source(
        fields
            .get("file")
            .ok_or_else(|| FileStreamFailure::invalid("content import intent is missing `file`"))?,
    )
}

fn decode_content_save_intent(
    value: &Value,
) -> Result<DecodedContentSaveIntent, FileStreamFailure> {
    let fields = file_record(value, &["content", "file"], "content save intent")?;
    let content = fields
        .get("content")
        .ok_or_else(|| FileStreamFailure::invalid("content save intent is missing `content`"))
        .and_then(|value| ContentRef::from_value(value).map_err(content_store_failure))?;
    let target = decode_file_target(
        fields
            .get("file")
            .ok_or_else(|| FileStreamFailure::invalid("content save intent is missing `file`"))?,
    )?;
    Ok(DecodedContentSaveIntent { content, target })
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
    let source = decode_file_source(file)?;
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

fn decode_file_source(file: &Value) -> Result<DecodedFileSource, FileStreamFailure> {
    let visible = file.visible();
    let file_fields = match visible {
        Value::Record(fields) => fields,
        _ => {
            return Err(FileStreamFailure::invalid(
                "file input must be a tagged object",
            ));
        }
    };
    match file_fields.get("$tag") {
        Some(Value::Text(tag)) if tag == "FileSelected" => {
            file_record(visible, &["$tag"], "selected file")?;
            let binding = file
                .host_binding()
                .cloned()
                .ok_or_else(|| FileStreamFailure::invalid("selected file has no host binding"))?;
            Ok(DecodedFileSource::Capability(FileCapability { binding }))
        }
        Some(Value::Text(tag)) if tag == "PackageAsset" => {
            if file.host_binding().is_some() {
                return Err(FileStreamFailure::invalid(
                    "package assets must not carry a host binding",
                ));
            }
            let fields = file_record(visible, &["$tag", "url"], "package asset")?;
            let url = file_text(fields, "url")?.to_owned();
            if url.is_empty() || url.len() > MAX_PACKAGE_ASSET_URL_BYTES {
                return Err(FileStreamFailure::invalid(
                    "package asset URL is empty or exceeds the bounded contract",
                ));
            }
            Ok(DecodedFileSource::PackageAsset(url))
        }
        _ => Err(FileStreamFailure::invalid(
            "file input must be FileSelected or PackageAsset",
        )),
    }
}

fn decode_file_target(value: &Value) -> Result<FileCapability, FileStreamFailure> {
    let fields = file_record(value.visible(), &["$tag"], "file target")?;
    match fields.get("$tag") {
        Some(Value::Text(tag)) if tag == "FileTarget" => {}
        _ => return Err(FileStreamFailure::invalid("file target must be FileTarget")),
    }
    let binding = value
        .host_binding()
        .cloned()
        .ok_or_else(|| FileStreamFailure::invalid("file target has no host binding"))?;
    Ok(FileCapability { binding })
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

fn content_store_failure(error: impl Into<ContentStoreError>) -> FileStreamFailure {
    let error = error.into();
    let code = match error.kind() {
        ContentStoreErrorKind::Capacity => "content_store_full",
        ContentStoreErrorKind::Missing => "content_missing",
        ContentStoreErrorKind::InvalidConfiguration | ContentStoreErrorKind::InvalidReference => {
            "content_invalid"
        }
        ContentStoreErrorKind::Corrupt => "content_corrupt",
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

fn file_timed_out_outcome() -> Value {
    file_failure_outcome(FileStreamFailure::new(
        "timeout",
        "file operation exceeded its host deadline",
    ))
}

struct FileReadWorker {
    control: FileWorkerControl,
    input: FileReadWorkerInput,
}

struct FileReadWorkerInput {
    path: PathBuf,
    display_name: Option<String>,
    _source_lease: Option<ContentLease>,
    media: Arc<str>,
    chunk_bytes: usize,
    retain_content: bool,
    content_store: ContentStore,
}

struct FileReadBytesWorker {
    control: FileWorkerControl,
    source: ResolvedFileSource,
    max_bytes: usize,
}

struct FileWriteBytesWorker {
    control: FileWorkerControl,
    target_path: PathBuf,
    bytes: Bytes,
}

struct ContentImportWorker {
    control: FileWorkerControl,
    source: ResolvedFileSource,
    content_store: ContentStore,
}

struct ContentSaveWorker {
    control: FileWorkerControl,
    content: ContentLease,
    target_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerFlow {
    Continue,
    NeedCancelled,
    NeedTimedOut,
    Terminal,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CreditReservation {
    Reserved,
    NeedCancelled,
    NeedTimedOut,
    Stopped,
}

fn finish_worker_flow(control: &FileWorkerControl, result_sequence: &mut u64, flow: WorkerFlow) {
    match flow {
        WorkerFlow::NeedCancelled => {
            emit_cancelled(control, result_sequence);
        }
        WorkerFlow::NeedTimedOut => {
            emit_timed_out(control, result_sequence);
        }
        WorkerFlow::Continue | WorkerFlow::Terminal | WorkerFlow::Stopped => {}
    }
}

fn run_file_read_bytes_worker(worker: FileReadBytesWorker) {
    let mut result_sequence = 0_u64;
    let flow = read_file_bytes(&worker, &mut result_sequence);
    finish_worker_flow(&worker.control, &mut result_sequence, flow);
}

fn read_file_bytes(worker: &FileReadBytesWorker, result_sequence: &mut u64) -> WorkerFlow {
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    let mut file = match File::open(&worker.source.path) {
        Ok(file) => file,
        Err(error) => {
            return send_io_failure(
                &worker.control,
                result_sequence,
                "open_failed",
                "cannot open selected file",
                error,
            );
        }
    };
    let size = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return send_io_failure(
                &worker.control,
                result_sequence,
                "metadata_failed",
                "cannot inspect selected file",
                error,
            );
        }
    };
    if size > worker.max_bytes as u64 {
        return send_outcome(
            &worker.control,
            result_sequence,
            file_failure_outcome(FileStreamFailure::new(
                "file_too_large",
                "selected file exceeds the requested bounded byte limit",
            )),
            true,
        );
    }
    let read_limit = worker.max_bytes.saturating_add(1);
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(read_limit).min(read_limit));
    if let Err(error) = Read::by_ref(&mut file)
        .take(u64::try_from(read_limit).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
    {
        return send_io_failure(
            &worker.control,
            result_sequence,
            "read_failed",
            "cannot read selected file",
            error,
        );
    }
    if bytes.len() > worker.max_bytes {
        return send_outcome(
            &worker.control,
            result_sequence,
            file_failure_outcome(FileStreamFailure::new(
                "file_too_large",
                "selected file grew beyond the requested bounded byte limit",
            )),
            true,
        );
    }
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    let byte_count = match file_number(bytes.len() as u64, "read byte count") {
        Ok(value) => value,
        Err(failure) => {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(failure),
                true,
            );
        }
    };
    send_outcome(
        &worker.control,
        result_sequence,
        tagged(
            "BytesRead",
            BTreeMap::from([
                ("bytes".to_owned(), Value::Bytes(bytes.into())),
                ("byte_count".to_owned(), byte_count),
                (
                    "media".to_owned(),
                    Value::Text(worker.source.media.to_string()),
                ),
                (
                    "display_name".to_owned(),
                    Value::Text(file_source_display_name(&worker.source)),
                ),
            ]),
        ),
        true,
    )
}

fn run_file_write_bytes_worker(worker: FileWriteBytesWorker) {
    let mut result_sequence = 0_u64;
    let flow = write_file_bytes(&worker, &mut result_sequence);
    finish_worker_flow(&worker.control, &mut result_sequence, flow);
}

fn write_file_bytes(worker: &FileWriteBytesWorker, result_sequence: &mut u64) -> WorkerFlow {
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    let mut file = match AtomicWriteFile::options().open(&worker.target_path) {
        Ok(file) => file,
        Err(error) => {
            return send_io_failure(
                &worker.control,
                result_sequence,
                "open_failed",
                "cannot open file target",
                error,
            );
        }
    };
    if let Err(error) = file.write_all(&worker.bytes) {
        return send_io_failure(
            &worker.control,
            result_sequence,
            "write_failed",
            "cannot write file target",
            error,
        );
    }
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    let commit_guard = match worker.control.permit.begin_commit() {
        Ok(guard) => guard,
        Err(_) => return stopped_worker_flow(&worker.control),
    };
    let commit = file.commit();
    commit_guard.finish();
    if let Err(error) = commit {
        return send_io_failure(
            &worker.control,
            result_sequence,
            "commit_failed",
            "cannot atomically commit file target",
            error,
        );
    }
    let byte_count = match file_number(worker.bytes.len() as u64, "written byte count") {
        Ok(value) => value,
        Err(failure) => {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(failure),
                true,
            );
        }
    };
    send_outcome(
        &worker.control,
        result_sequence,
        tagged(
            "BytesWritten",
            BTreeMap::from([("byte_count".to_owned(), byte_count)]),
        ),
        true,
    )
}

fn run_content_import_worker(worker: ContentImportWorker) {
    let mut result_sequence = 0_u64;
    let flow = import_content(&worker, &mut result_sequence);
    finish_worker_flow(&worker.control, &mut result_sequence, flow);
}

fn import_content(worker: &ContentImportWorker, result_sequence: &mut u64) -> WorkerFlow {
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    let mut file = match File::open(&worker.source.path) {
        Ok(file) => file,
        Err(error) => {
            return send_io_failure(
                &worker.control,
                result_sequence,
                "open_failed",
                "cannot open selected file",
                error,
            );
        }
    };
    let total_bytes = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return send_io_failure(
                &worker.control,
                result_sequence,
                "metadata_failed",
                "cannot inspect selected file",
                error,
            );
        }
    };
    let total_value = match file_number(total_bytes, "import byte count") {
        Ok(value) => value,
        Err(failure) => {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(failure),
                true,
            );
        }
    };
    let mut content_writer = match worker.content_store.begin_write(total_bytes) {
        Ok(writer) => writer,
        Err(error) => {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(content_store_failure(error)),
                true,
            );
        }
    };
    match send_outcome(
        &worker.control,
        result_sequence,
        tagged(
            "Started",
            BTreeMap::from([
                ("byte_count".to_owned(), total_value.clone()),
                (
                    "media".to_owned(),
                    Value::Text(worker.source.media.to_string()),
                ),
                (
                    "display_name".to_owned(),
                    Value::Text(file_source_display_name(&worker.source)),
                ),
            ]),
        ),
        false,
    ) {
        WorkerFlow::Continue => {}
        other => return other,
    }

    let mut digest = Sha256::new();
    let mut completed_bytes = 0_u64;
    let mut buffer = vec![0_u8; boon_effect_schema::FILE_STREAM_DEFAULT_CHUNK_BYTES as usize];
    loop {
        match worker_state(&worker.control) {
            WorkerFlow::Continue => {}
            other => return other,
        }
        let read = match file.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                return send_io_failure(
                    &worker.control,
                    result_sequence,
                    "read_failed",
                    "cannot read selected file",
                    error,
                );
            }
        };
        if read == 0 {
            break;
        }
        if let Err(error) = content_writer.write_chunk(&buffer[..read]) {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(content_store_failure(error)),
                true,
            );
        }
        digest.update(&buffer[..read]);
        completed_bytes = match completed_bytes.checked_add(read as u64) {
            Some(value) => value,
            None => {
                return send_outcome(
                    &worker.control,
                    result_sequence,
                    file_failure_outcome(FileStreamFailure::new(
                        "file_too_large",
                        "import byte count exceeds the host range",
                    )),
                    true,
                );
            }
        };
        match reserve_credit(&worker.control) {
            CreditReservation::Reserved => {}
            CreditReservation::NeedCancelled => return WorkerFlow::NeedCancelled,
            CreditReservation::NeedTimedOut => return WorkerFlow::NeedTimedOut,
            CreditReservation::Stopped => return WorkerFlow::Stopped,
        }
        let completed_value = match file_number(completed_bytes, "completed import bytes") {
            Ok(value) => value,
            Err(failure) => {
                return send_outcome(
                    &worker.control,
                    result_sequence,
                    file_failure_outcome(failure),
                    true,
                );
            }
        };
        match send_outcome(
            &worker.control,
            result_sequence,
            tagged(
                "Progress",
                BTreeMap::from([
                    ("completed_bytes".to_owned(), completed_value),
                    ("total_bytes".to_owned(), total_value.clone()),
                ]),
            ),
            false,
        ) {
            WorkerFlow::Continue => {}
            other => return other,
        }
    }
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    let content = match ContentRef::new(
        <[u8; 32]>::from(digest.finalize()),
        completed_bytes,
        Arc::clone(&worker.source.media),
    ) {
        Ok(content) => content,
        Err(error) => {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(content_store_failure(error)),
                true,
            );
        }
    };
    let commit_guard = match worker.control.permit.begin_commit() {
        Ok(guard) => guard,
        Err(_) => return stopped_worker_flow(&worker.control),
    };
    let content = content_writer.finish(content);
    commit_guard.finish();
    let content = match content {
        Ok(content) => content,
        Err(error) => {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(content_store_failure(error)),
                true,
            );
        }
    };
    let content = match content.value() {
        Ok(content) => content,
        Err(error) => {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(content_store_failure(error)),
                true,
            );
        }
    };
    send_outcome(
        &worker.control,
        result_sequence,
        tagged(
            "Imported",
            BTreeMap::from([("content".to_owned(), content)]),
        ),
        true,
    )
}

fn run_content_save_worker(worker: ContentSaveWorker) {
    let mut result_sequence = 0_u64;
    let flow = save_content(&worker, &mut result_sequence);
    finish_worker_flow(&worker.control, &mut result_sequence, flow);
}

fn save_content(worker: &ContentSaveWorker, result_sequence: &mut u64) -> WorkerFlow {
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    let total_bytes = worker.content.content().size();
    let total_value = match file_number(total_bytes, "saved content byte count") {
        Ok(value) => value,
        Err(failure) => {
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(failure),
                true,
            );
        }
    };
    let mut source = match File::open(worker.content.path()) {
        Ok(file) => file,
        Err(error) => {
            return send_io_failure(
                &worker.control,
                result_sequence,
                "content_missing",
                "cannot open retained content",
                error,
            );
        }
    };
    let mut target = match AtomicWriteFile::options().open(&worker.target_path) {
        Ok(file) => file,
        Err(error) => {
            return send_io_failure(
                &worker.control,
                result_sequence,
                "open_failed",
                "cannot open file target",
                error,
            );
        }
    };
    match send_outcome(
        &worker.control,
        result_sequence,
        tagged(
            "Started",
            BTreeMap::from([("byte_count".to_owned(), total_value.clone())]),
        ),
        false,
    ) {
        WorkerFlow::Continue => {}
        other => return other,
    }

    let mut digest = Sha256::new();
    let mut completed_bytes = 0_u64;
    let mut buffer = vec![0_u8; boon_effect_schema::FILE_STREAM_DEFAULT_CHUNK_BYTES as usize];
    loop {
        match worker_state(&worker.control) {
            WorkerFlow::Continue => {}
            other => return other,
        }
        let read = match source.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                return send_io_failure(
                    &worker.control,
                    result_sequence,
                    "content_corrupt",
                    "cannot read retained content",
                    error,
                );
            }
        };
        if read == 0 {
            break;
        }
        if let Err(error) = target.write_all(&buffer[..read]) {
            return send_io_failure(
                &worker.control,
                result_sequence,
                "write_failed",
                "cannot write file target",
                error,
            );
        }
        digest.update(&buffer[..read]);
        completed_bytes = match completed_bytes.checked_add(read as u64) {
            Some(value) => value,
            None => return WorkerFlow::Stopped,
        };
        match reserve_credit(&worker.control) {
            CreditReservation::Reserved => {}
            CreditReservation::NeedCancelled => return WorkerFlow::NeedCancelled,
            CreditReservation::NeedTimedOut => return WorkerFlow::NeedTimedOut,
            CreditReservation::Stopped => return WorkerFlow::Stopped,
        }
        let completed_value = match file_number(completed_bytes, "completed saved bytes") {
            Ok(value) => value,
            Err(failure) => {
                return send_outcome(
                    &worker.control,
                    result_sequence,
                    file_failure_outcome(failure),
                    true,
                );
            }
        };
        match send_outcome(
            &worker.control,
            result_sequence,
            tagged(
                "Progress",
                BTreeMap::from([
                    ("completed_bytes".to_owned(), completed_value),
                    ("total_bytes".to_owned(), total_value.clone()),
                ]),
            ),
            false,
        ) {
            WorkerFlow::Continue => {}
            other => return other,
        }
    }
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    if completed_bytes != total_bytes
        || <[u8; 32]>::from(digest.finalize()) != worker.content.content().digest()
    {
        return send_outcome(
            &worker.control,
            result_sequence,
            file_failure_outcome(FileStreamFailure::new(
                "content_corrupt",
                "retained content differs from its durable descriptor",
            )),
            true,
        );
    }
    let commit_guard = match worker.control.permit.begin_commit() {
        Ok(guard) => guard,
        Err(_) => return stopped_worker_flow(&worker.control),
    };
    let commit = target.commit();
    commit_guard.finish();
    if let Err(error) = commit {
        return send_io_failure(
            &worker.control,
            result_sequence,
            "commit_failed",
            "cannot atomically commit file target",
            error,
        );
    }
    send_outcome(
        &worker.control,
        result_sequence,
        tagged(
            "Saved",
            BTreeMap::from([("byte_count".to_owned(), total_value)]),
        ),
        true,
    )
}

fn worker_state(control: &FileWorkerControl) -> WorkerFlow {
    if Instant::now() >= control.deadline {
        control.permit.request_timeout();
    }
    match control.permit.stop_reason() {
        Some(EffectStopReason::Discarded) => WorkerFlow::Stopped,
        Some(EffectStopReason::TimedOut) => WorkerFlow::NeedTimedOut,
        Some(EffectStopReason::Cancelled) => WorkerFlow::NeedCancelled,
        None => WorkerFlow::Continue,
    }
}

fn stopped_worker_flow(control: &FileWorkerControl) -> WorkerFlow {
    match worker_state(control) {
        WorkerFlow::Continue | WorkerFlow::Terminal => WorkerFlow::Stopped,
        flow => flow,
    }
}

fn file_source_display_name(source: &ResolvedFileSource) -> String {
    source.display_name.clone().unwrap_or_else(|| {
        source
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("selected-file")
            .chars()
            .take(256)
            .collect()
    })
}

fn send_io_failure(
    control: &FileWorkerControl,
    result_sequence: &mut u64,
    code: &'static str,
    action: &'static str,
    error: std::io::Error,
) -> WorkerFlow {
    send_outcome(
        control,
        result_sequence,
        file_failure_outcome(FileStreamFailure::new(
            code,
            format!("{action}: {:?}", error.kind()),
        )),
        true,
    )
}

fn run_file_read_worker(worker: FileReadWorker) {
    let mut result_sequence = 0_u64;
    let flow = stream_file(&worker, &mut result_sequence);
    finish_worker_flow(&worker.control, &mut result_sequence, flow);
}

fn stream_file(worker: &FileReadWorker, result_sequence: &mut u64) -> WorkerFlow {
    match worker_state(&worker.control) {
        WorkerFlow::Continue => {}
        other => return other,
    }
    let mut file = match File::open(&worker.input.path) {
        Ok(file) => file,
        Err(error) => {
            return send_outcome(
                &worker.control,
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
            return send_outcome(
                &worker.control,
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
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(failure),
                true,
            );
        }
    };
    let display_name = worker.input.display_name.clone().unwrap_or_else(|| {
        worker
            .input
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("selected-file")
            .chars()
            .take(256)
            .collect::<String>()
    });
    let mut content_writer = if worker.input.retain_content {
        match worker.input.content_store.begin_write(size_bytes) {
            Ok(writer) => Some(writer),
            Err(error) => {
                return send_outcome(
                    &worker.control,
                    result_sequence,
                    file_failure_outcome(content_store_failure(error)),
                    true,
                );
            }
        }
    } else {
        None
    };
    match send_outcome(
        &worker.control,
        result_sequence,
        tagged(
            "Opened",
            BTreeMap::from([
                ("size".to_owned(), size),
                (
                    "content_type".to_owned(),
                    Value::Text(worker.input.media.to_string()),
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
    let mut buffer = vec![0_u8; worker.input.chunk_bytes];
    loop {
        match worker_state(&worker.control) {
            WorkerFlow::Continue => {}
            other => return other,
        }
        let read = match file.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                return send_outcome(
                    &worker.control,
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
            let content = match ContentRef::new(digest, byte_count, Arc::clone(&worker.input.media))
            {
                Ok(content) => content,
                Err(error) => {
                    return send_outcome(
                        &worker.control,
                        result_sequence,
                        file_failure_outcome(content_store_failure(error)),
                        true,
                    );
                }
            };
            let retained_content = if let Some(writer) = content_writer.take() {
                let commit_guard = match worker.control.permit.begin_commit() {
                    Ok(guard) => guard,
                    Err(_) => return stopped_worker_flow(&worker.control),
                };
                let retained_content = writer.finish(content);
                commit_guard.finish();
                match retained_content {
                    Ok(content) => Some(content),
                    Err(error) => {
                        return send_outcome(
                            &worker.control,
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
                    return send_outcome(
                        &worker.control,
                        result_sequence,
                        file_failure_outcome(failure),
                        true,
                    );
                }
            };
            let retained_value = match retained_content.as_ref() {
                Some(content) => match content.value() {
                    Ok(value) => {
                        tagged("Retained", BTreeMap::from([("content".to_owned(), value)]))
                    }
                    Err(error) => {
                        let _ = worker.input.content_store.remove(content);
                        return send_outcome(
                            &worker.control,
                            result_sequence,
                            file_failure_outcome(content_store_failure(error)),
                            true,
                        );
                    }
                },
                None => tagged("NotRetained", BTreeMap::new()),
            };
            let flow = send_outcome(
                &worker.control,
                result_sequence,
                tagged(
                    "Finished",
                    BTreeMap::from([
                        ("byte_count".to_owned(), byte_count_value),
                        ("digest".to_owned(), Value::Bytes(digest.to_vec().into())),
                        ("retained".to_owned(), retained_value),
                    ]),
                ),
                true,
            );
            if flow == WorkerFlow::Stopped
                && let Some(content) = retained_content
            {
                let _ = worker.input.content_store.remove(&content);
            }
            return flow;
        }
        let offset = byte_count;
        let read_u64 = u64::try_from(read).expect("bounded read length fits u64");
        byte_count = match byte_count.checked_add(read_u64) {
            Some(byte_count) => byte_count,
            None => {
                return send_outcome(
                    &worker.control,
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
            return send_outcome(
                &worker.control,
                result_sequence,
                file_failure_outcome(content_store_failure(error)),
                true,
            );
        }
        let sequence = match file_number(chunk_sequence, "chunk sequence") {
            Ok(value) => value,
            Err(failure) => {
                return send_outcome(
                    &worker.control,
                    result_sequence,
                    file_failure_outcome(failure),
                    true,
                );
            }
        };
        let offset = match file_number(offset, "chunk offset") {
            Ok(value) => value,
            Err(failure) => {
                return send_outcome(
                    &worker.control,
                    result_sequence,
                    file_failure_outcome(failure),
                    true,
                );
            }
        };
        match reserve_credit(&worker.control) {
            CreditReservation::Reserved => {}
            CreditReservation::NeedCancelled => return WorkerFlow::NeedCancelled,
            CreditReservation::NeedTimedOut => return WorkerFlow::NeedTimedOut,
            CreditReservation::Stopped => return WorkerFlow::Stopped,
        }
        match send_outcome(
            &worker.control,
            result_sequence,
            tagged(
                "Chunk",
                BTreeMap::from([
                    ("sequence".to_owned(), sequence),
                    ("offset".to_owned(), offset),
                    (
                        "bytes".to_owned(),
                        Value::Bytes(buffer[..read].to_vec().into()),
                    ),
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

fn emit_cancelled(control: &FileWorkerControl, result_sequence: &mut u64) -> WorkerFlow {
    send_outcome(control, result_sequence, file_cancelled_outcome(), true)
}

fn emit_timed_out(control: &FileWorkerControl, result_sequence: &mut u64) -> WorkerFlow {
    send_outcome(control, result_sequence, file_timed_out_outcome(), true)
}

fn reserve_credit(control: &FileWorkerControl) -> CreditReservation {
    loop {
        match worker_state(control) {
            WorkerFlow::Continue => {}
            WorkerFlow::NeedCancelled => return CreditReservation::NeedCancelled,
            WorkerFlow::NeedTimedOut => return CreditReservation::NeedTimedOut,
            WorkerFlow::Terminal | WorkerFlow::Stopped => return CreditReservation::Stopped,
        }
        let mut current = control.outstanding_credits.load(Ordering::Acquire);
        while current > 0 {
            match control.outstanding_credits.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return CreditReservation::Reserved,
                Err(actual) => current = actual,
            }
        }
        match control.credit_rx.recv_timeout(FILE_WORKER_POLL_INTERVAL) {
            Ok(()) | Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                return CreditReservation::Stopped;
            }
        }
    }
}

fn send_outcome(
    control: &FileWorkerControl,
    result_sequence: &mut u64,
    mut outcome: Value,
    mut terminal: bool,
) -> WorkerFlow {
    let mut terminal_reserved = false;
    loop {
        let sequence_guard = control
            .sequence_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !terminal {
            match worker_state(control) {
                WorkerFlow::Continue => {}
                WorkerFlow::NeedCancelled => {
                    outcome = file_cancelled_outcome();
                    terminal = true;
                }
                WorkerFlow::NeedTimedOut => {
                    outcome = file_timed_out_outcome();
                    terminal = true;
                }
                WorkerFlow::Terminal | WorkerFlow::Stopped => return WorkerFlow::Stopped,
            }
        }
        if terminal && !terminal_reserved {
            outcome = match control.permit.reserve_terminal() {
                EffectTerminalReservation::Deliver => outcome,
                EffectTerminalReservation::Cancelled => file_cancelled_outcome(),
                EffectTerminalReservation::TimedOut => file_timed_out_outcome(),
                EffectTerminalReservation::Discarded
                | EffectTerminalReservation::AlreadyReserved => return WorkerFlow::Stopped,
            };
            terminal_reserved = true;
        }
        let emitted_sequence = control.next_emitted_sequence.load(Ordering::Acquire);
        let event = FileEffectEvent {
            call_id: control.call_id,
            invocation_id: control.invocation_id,
            result_sequence: emitted_sequence,
            outcome: outcome.clone(),
            terminal,
            stream: control.stream,
        };
        match control.events_tx.try_send(event) {
            Ok(()) => {
                let Some(next_sequence) = emitted_sequence.checked_add(1) else {
                    return WorkerFlow::Stopped;
                };
                control
                    .next_emitted_sequence
                    .store(next_sequence, Ordering::Release);
                *result_sequence = next_sequence;
                drop(sequence_guard);
                return if terminal {
                    WorkerFlow::Terminal
                } else {
                    WorkerFlow::Continue
                };
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                drop(sequence_guard);
                thread::sleep(FILE_WORKER_POLL_INTERVAL);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                drop(sequence_guard);
                return WorkerFlow::Stopped;
            }
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

fn release_file_worker(mut active: ActiveFileOperation) {
    drop(active.credit_tx);
    if let Some(task) = active.task.take()
        && task.is_finished()
    {
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
