use super::window;
use crate::WebHostResult;
use crate::client_effect_host::{
    BrowserClientEffectCommand, BrowserClientEffectHostCore, BrowserClientEffectKind,
};
use boon_app_package::{BrowserPackageAssetDescriptor, CapabilityProfileDescriptor};
use boon_plan::EffectContract;
use boon_runtime::{RuntimeTurn, TransientEffectCallId, TransientEffectInvocation, Value};
use boon_web_effect_host::{
    BrowserFileEffectHost, BrowserFileEffectLimits, BrowserFileEffectNotification,
    BrowserFileEffectOperation,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use wasm_bindgen::{JsCast, closure::Closure};

const MAX_BROWSER_RANDOM_BYTES: usize = 1024 * 1024;

pub(crate) struct BrowserClientEffectCompletion {
    pub call_id: TransientEffectCallId,
    pub kind: BrowserClientEffectKind,
    pub result_sequence: Option<u64>,
    pub terminal: bool,
    pub outcome: Value,
}

struct ActiveBrowserTimer {
    timeout_id: i32,
    _callback: Closure<dyn FnMut()>,
}

/// Browser platform adapter for the generic Client effect contract.
pub(crate) struct BrowserClientEffectHost {
    core: BrowserClientEffectHostCore,
    files: BrowserFileEffectHost,
    ready: Rc<RefCell<VecDeque<BrowserClientEffectCompletion>>>,
    timers: BTreeMap<TransientEffectCallId, ActiveBrowserTimer>,
    wake: Rc<dyn Fn()>,
}

impl BrowserClientEffectHost {
    pub(crate) async fn open(
        package_id: &str,
        profile: &CapabilityProfileDescriptor,
        contracts: &[EffectContract],
        package_assets: &[BrowserPackageAssetDescriptor],
        wake: Rc<dyn Fn()>,
    ) -> WebHostResult<Self> {
        let file_limits = BrowserFileEffectLimits::default();
        let mut files =
            BrowserFileEffectHost::open(package_id, Rc::clone(&wake), file_limits).await?;
        files.register_package_assets(package_assets)?;
        Ok(Self {
            core: BrowserClientEffectHostCore::new(profile, contracts, file_limits.max_active)?,
            files,
            ready: Rc::new(RefCell::new(VecDeque::new())),
            timers: BTreeMap::new(),
            wake,
        })
    }

    pub(crate) fn route_turns(&mut self, turns: &[RuntimeTurn]) -> WebHostResult<()> {
        let commands = self.core.route_turns(turns)?;
        for command in &commands {
            let BrowserClientEffectCommand::Cancel { kind, call_id } = command else {
                continue;
            };
            self.cancel(*kind, *call_id);
        }
        for command in &commands {
            let BrowserClientEffectCommand::Submit { kind, invocation } = command else {
                continue;
            };
            self.submit(*kind, invocation.clone())?;
        }
        for command in &commands {
            let BrowserClientEffectCommand::GrantCredits { kind, grant } = command else {
                continue;
            };
            self.grant_credits(*kind, *grant)?;
        }
        Ok(())
    }

    fn grant_credits(
        &mut self,
        kind: BrowserClientEffectKind,
        grant: boon_runtime::TransientEffectCreditGrant,
    ) -> WebHostResult<()> {
        if file_operation(kind).is_none() {
            return Err(crate::WebHostError::InvalidInput {
                field: "browser stream credit".to_owned(),
                reason: format!(
                    "non-stream browser effect {:?} received credit for call {}",
                    kind, grant.call_id
                ),
            });
        }
        self.files.grant_credits(grant)?;
        Ok(())
    }

    pub(crate) fn try_completion(
        &mut self,
    ) -> WebHostResult<Option<BrowserClientEffectCompletion>> {
        loop {
            let completion = self
                .ready
                .borrow_mut()
                .pop_front()
                .or_else(|| self.files.dequeue_notification().map(file_completion));
            let Some(completion) = completion else {
                return Ok(None);
            };
            if self
                .core
                .accept_result(completion.call_id, completion.kind, completion.terminal)
                .is_err()
            {
                continue;
            }
            if let Some(timer) = self.timers.remove(&completion.call_id) {
                window()?.clear_timeout_with_handle(timer.timeout_id);
            }
            return Ok(Some(completion));
        }
    }

    pub(crate) fn cancel_all(&mut self) {
        for command in self.core.cancel_all() {
            let BrowserClientEffectCommand::Cancel { kind, call_id } = command else {
                unreachable!("cancel_all emits only cancellation commands");
            };
            self.cancel(kind, call_id);
        }
        self.ready.borrow_mut().clear();
    }

    fn submit(
        &mut self,
        kind: BrowserClientEffectKind,
        invocation: TransientEffectInvocation,
    ) -> WebHostResult<()> {
        match kind {
            BrowserClientEffectKind::WallClock => {
                let outcome = wall_clock_outcome(&invocation.intent);
                self.queue(invocation.call_id, kind, outcome);
            }
            BrowserClientEffectKind::SecureRandom => {
                let outcome = random_outcome(&invocation.intent);
                self.queue(invocation.call_id, kind, outcome);
            }
            BrowserClientEffectKind::Deadline => {
                self.submit_deadline(invocation)?;
            }
            BrowserClientEffectKind::FileReadBytes
            | BrowserClientEffectKind::FileWriteBytes
            | BrowserClientEffectKind::FileReadStream
            | BrowserClientEffectKind::ContentImport
            | BrowserClientEffectKind::ContentSave => {
                self.files.submit(
                    file_operation(kind).expect("File/Content kind has a platform operation"),
                    invocation,
                )?;
            }
        }
        Ok(())
    }

    fn submit_deadline(&mut self, invocation: TransientEffectInvocation) -> WebHostResult<()> {
        let delay_ms = match positive_integer_field(&invocation.intent, "delay_ms") {
            Ok(delay_ms) if delay_ms <= i64::from(i32::MAX) => delay_ms,
            Ok(_) => {
                self.queue(
                    invocation.call_id,
                    BrowserClientEffectKind::Deadline,
                    failure(
                        "delay_out_of_range",
                        "browser timer delay exceeds the platform timeout range",
                    ),
                );
                return Ok(());
            }
            Err(outcome) => {
                self.queue(
                    invocation.call_id,
                    BrowserClientEffectKind::Deadline,
                    outcome,
                );
                return Ok(());
            }
        };
        let call_id = invocation.call_id;
        let ready = Rc::clone(&self.ready);
        let wake = Rc::clone(&self.wake);
        let callback = Closure::wrap(Box::new(move || {
            ready.borrow_mut().push_back(BrowserClientEffectCompletion {
                call_id,
                kind: BrowserClientEffectKind::Deadline,
                result_sequence: None,
                terminal: true,
                outcome: tagged(
                    "TimerFired",
                    BTreeMap::from([(
                        "delay_ms".to_owned(),
                        Value::integer(delay_ms)
                            .expect("bounded timer delay is exactly representable"),
                    )]),
                ),
            });
            wake();
        }) as Box<dyn FnMut()>);
        let timeout_id = window()?
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                i32::try_from(delay_ms).expect("browser timer delay was bounded"),
            )
            .map_err(|error| super::js_error("schedule Client effect deadline", error))?;
        self.timers.insert(
            call_id,
            ActiveBrowserTimer {
                timeout_id,
                _callback: callback,
            },
        );
        Ok(())
    }

    fn queue(&self, call_id: TransientEffectCallId, kind: BrowserClientEffectKind, outcome: Value) {
        self.ready
            .borrow_mut()
            .push_back(BrowserClientEffectCompletion {
                call_id,
                kind,
                result_sequence: None,
                terminal: true,
                outcome,
            });
    }

    fn cancel(&mut self, kind: BrowserClientEffectKind, call_id: TransientEffectCallId) {
        self.ready
            .borrow_mut()
            .retain(|completion| completion.call_id != call_id);
        if kind == BrowserClientEffectKind::Deadline
            && let Some(timer) = self.timers.remove(&call_id)
            && let Ok(window) = window()
        {
            window.clear_timeout_with_handle(timer.timeout_id);
        }
        if file_operation(kind).is_some() {
            self.files.cancel(call_id);
        }
    }
}

impl Drop for BrowserClientEffectHost {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

fn file_operation(kind: BrowserClientEffectKind) -> Option<BrowserFileEffectOperation> {
    match kind {
        BrowserClientEffectKind::FileReadBytes => Some(BrowserFileEffectOperation::ReadBytes),
        BrowserClientEffectKind::FileWriteBytes => Some(BrowserFileEffectOperation::WriteBytes),
        BrowserClientEffectKind::FileReadStream => Some(BrowserFileEffectOperation::ReadStream),
        BrowserClientEffectKind::ContentImport => Some(BrowserFileEffectOperation::ContentImport),
        BrowserClientEffectKind::ContentSave => Some(BrowserFileEffectOperation::ContentSave),
        BrowserClientEffectKind::WallClock
        | BrowserClientEffectKind::SecureRandom
        | BrowserClientEffectKind::Deadline => None,
    }
}

fn file_completion(notification: BrowserFileEffectNotification) -> BrowserClientEffectCompletion {
    let kind = match notification.operation {
        BrowserFileEffectOperation::ReadBytes => BrowserClientEffectKind::FileReadBytes,
        BrowserFileEffectOperation::WriteBytes => BrowserClientEffectKind::FileWriteBytes,
        BrowserFileEffectOperation::ReadStream => BrowserClientEffectKind::FileReadStream,
        BrowserFileEffectOperation::ContentImport => BrowserClientEffectKind::ContentImport,
        BrowserFileEffectOperation::ContentSave => BrowserClientEffectKind::ContentSave,
    };
    BrowserClientEffectCompletion {
        call_id: notification.call_id,
        kind,
        result_sequence: notification.result_sequence,
        terminal: notification.terminal,
        outcome: notification.outcome,
    }
}

fn wall_clock_outcome(intent: &Value) -> Value {
    if let Err(outcome) = exact_record(intent, &[]) {
        return outcome;
    }
    let unix_ms = js_sys::Date::now();
    if !unix_ms.is_finite() || unix_ms < 0.0 || unix_ms > 9_007_199_254_740_991.0 {
        return failure(
            "time_out_of_range",
            "browser wall clock is outside Number range",
        );
    }
    let unix_ms = unix_ms.floor() as i64;
    let unix_seconds = unix_ms / 1_000;
    let nanoseconds = (unix_ms % 1_000) * 1_000_000;
    tagged(
        "WallClockRead",
        BTreeMap::from([
            (
                "unix_seconds".to_owned(),
                Value::integer(unix_seconds).expect("current Unix seconds fit Boon Number"),
            ),
            (
                "nanoseconds".to_owned(),
                Value::integer(nanoseconds).expect("millisecond nanoseconds fit Boon Number"),
            ),
        ]),
    )
}

fn random_outcome(intent: &Value) -> Value {
    let byte_count = match positive_integer_field(intent, "byte_count") {
        Ok(value) => match usize::try_from(value) {
            Ok(value) if value <= MAX_BROWSER_RANDOM_BYTES => value,
            _ => {
                return failure(
                    "byte_count_out_of_range",
                    "browser random byte count exceeds the bounded host limit",
                );
            }
        },
        Err(outcome) => return outcome,
    };
    let mut bytes = vec![0; byte_count];
    if getrandom::fill(&mut bytes).is_err() {
        return failure(
            "random_unavailable",
            "browser secure random provider is unavailable",
        );
    }
    tagged(
        "RandomBytesReady",
        BTreeMap::from([("bytes".to_owned(), Value::Bytes(bytes.into()))]),
    )
}

fn positive_integer_field(intent: &Value, field: &str) -> Result<i64, Value> {
    let fields = exact_record(intent, &[field])?;
    let Value::Number(value) = fields
        .get(field)
        .expect("exact record contains requested field")
    else {
        return Err(failure(
            "invalid_intent",
            "browser effect numeric field differs from the typed contract",
        ));
    };
    let value = value.to_i64_exact().map_err(|_| {
        failure(
            "invalid_intent",
            "browser effect numeric field must be an exact integer",
        )
    })?;
    if value <= 0 {
        return Err(failure(
            "invalid_intent",
            "browser effect numeric field must be positive",
        ));
    }
    Ok(value)
}

fn exact_record<'a>(
    value: &'a Value,
    expected: &[&str],
) -> Result<&'a BTreeMap<String, Value>, Value> {
    let Value::Record(fields) = value else {
        return Err(failure(
            "invalid_intent",
            "browser effect intent must be a closed record",
        ));
    };
    if fields.len() != expected.len() || expected.iter().any(|name| !fields.contains_key(*name)) {
        return Err(failure(
            "invalid_intent",
            "browser effect intent fields differ from the typed contract",
        ));
    }
    Ok(fields)
}

fn failure(code: &str, diagnostic: &str) -> Value {
    tagged(
        "HostServiceFailed",
        BTreeMap::from([
            ("code".to_owned(), Value::Text(code.to_owned())),
            ("diagnostic".to_owned(), Value::Text(diagnostic.to_owned())),
        ]),
    )
}

fn tagged(tag: &str, mut fields: BTreeMap<String, Value>) -> Value {
    fields.insert("$tag".to_owned(), Value::Text(tag.to_owned()));
    Value::Record(fields)
}
