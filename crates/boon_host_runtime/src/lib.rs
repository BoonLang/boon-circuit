//! Typed runtime adapter for bounded clock, random, secret, HMAC, and timer services.

#![forbid(unsafe_code)]

use boon_host_services::{
    CancellationHandle, HMAC_SHA256_TAG_BYTES, HmacSha256Tag, HostServiceError, HostServices,
    SecretMaterial, SecretRef, TimerReceiveError,
};
use boon_plan::{EffectId, EffectInvocationId, FiniteReal};
use boon_runtime::{
    ProgramSession, RuntimeTurn, TransientEffectCallId, TransientEffectInvocation, Value,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const MAX_SECRET_NAME_BYTES: usize = 128;
const MAX_DIAGNOSTIC_BYTES: usize = 1024;

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
    NotOwned,
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
    task: JoinHandle<()>,
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
