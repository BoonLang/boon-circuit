use boon_persistence::{DurableOutboxItem, OutboxItemId, StoredValue};
use boon_plan::{EffectId, EffectInvocationId};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEffectRequest {
    pub item_id: OutboxItemId,
    pub invocation_id: EffectInvocationId,
    pub effect_id: EffectId,
    pub idempotency_key: StoredValue,
    pub intent: StoredValue,
    pub attempt: u32,
}

impl From<&DurableOutboxItem> for HostEffectRequest {
    fn from(item: &DurableOutboxItem) -> Self {
        Self {
            item_id: item.item_id,
            invocation_id: item.invocation_id,
            effect_id: item.effect_id,
            idempotency_key: item.idempotency_key.clone(),
            intent: item.intent.clone(),
            attempt: item.state.attempt(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostEffectReconciliation {
    Applied(StoredValue),
    NotApplied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEffectError {
    pub detail: String,
    pub remote_state_uncertain: bool,
}

impl HostEffectError {
    pub fn rejected(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            remote_state_uncertain: false,
        }
    }

    pub fn uncertain(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            remote_state_uncertain: true,
        }
    }
}

impl fmt::Display for HostEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for HostEffectError {}

pub trait HostEffectDriver {
    fn dispatch(&mut self, request: &HostEffectRequest) -> Result<StoredValue, HostEffectError>;

    fn reconcile(
        &mut self,
        request: &HostEffectRequest,
    ) -> Result<HostEffectReconciliation, HostEffectError>;
}

#[derive(Default)]
pub struct HostEffectRouter {
    drivers: BTreeMap<EffectId, Box<dyn HostEffectDriver + Send>>,
}

impl HostEffectRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        host_operation: &str,
        driver: impl HostEffectDriver + Send + 'static,
    ) -> Result<(), HostEffectError> {
        let effect_id = EffectId::from_host_operation(host_operation)
            .map_err(|error| HostEffectError::rejected(error.to_string()))?;
        if self.drivers.contains_key(&effect_id) {
            return Err(HostEffectError::rejected(format!(
                "host effect driver already registered for `{host_operation}`"
            )));
        }
        self.drivers.insert(effect_id, Box::new(driver));
        Ok(())
    }

    fn driver(
        &mut self,
        request: &HostEffectRequest,
    ) -> Result<&mut (dyn HostEffectDriver + Send + 'static), HostEffectError> {
        self.drivers
            .get_mut(&request.effect_id)
            .map(Box::as_mut)
            .ok_or_else(|| {
                HostEffectError::rejected(format!(
                    "no host effect driver owns effect {}",
                    request.effect_id
                ))
            })
    }
}

impl HostEffectDriver for HostEffectRouter {
    fn dispatch(&mut self, request: &HostEffectRequest) -> Result<StoredValue, HostEffectError> {
        self.driver(request)?.dispatch(request)
    }

    fn reconcile(
        &mut self,
        request: &HostEffectRequest,
    ) -> Result<HostEffectReconciliation, HostEffectError> {
        self.driver(request)?.reconcile(request)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEffectWorkerOperation {
    Dispatch,
    Reconcile,
}

#[derive(Debug)]
pub enum HostEffectWorkerOutcome {
    Dispatched(Result<StoredValue, HostEffectError>),
    Reconciled(Result<HostEffectReconciliation, HostEffectError>),
}

#[derive(Debug)]
pub struct HostEffectWorkerResult {
    pub request: HostEffectRequest,
    pub outcome: HostEffectWorkerOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostEffectWorkerError {
    Spawn(String),
    Busy {
        item_id: OutboxItemId,
        operation: HostEffectWorkerOperation,
    },
    Closed,
    Join(String),
}

impl fmt::Display for HostEffectWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(detail) => {
                write!(formatter, "failed to start host effect worker: {detail}")
            }
            Self::Busy { item_id, operation } => {
                write!(
                    formatter,
                    "host effect worker is busy with {operation:?} for {item_id}"
                )
            }
            Self::Closed => formatter.write_str("host effect worker is closed"),
            Self::Join(detail) => write!(formatter, "host effect worker failed to join: {detail}"),
        }
    }
}

impl std::error::Error for HostEffectWorkerError {}

#[allow(clippy::large_enum_variant)]
enum HostEffectWorkerCommand {
    Run {
        operation: HostEffectWorkerOperation,
        request: HostEffectRequest,
    },
    Shutdown,
}

/// Bounded host-I/O lane. It never owns or mutates a Boon Session; it only
/// executes one already-durable effect request and returns a correlated result
/// to the runtime owner.
pub struct HostEffectWorker {
    commands: SyncSender<HostEffectWorkerCommand>,
    results: Receiver<HostEffectWorkerResult>,
    in_flight: Option<(OutboxItemId, HostEffectWorkerOperation)>,
    worker: Option<JoinHandle<()>>,
}

impl HostEffectWorker {
    pub fn start(
        mut driver: impl HostEffectDriver + Send + 'static,
    ) -> Result<Self, HostEffectWorkerError> {
        let (commands, command_receiver) = mpsc::sync_channel(1);
        let (result_sender, results) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("boon-host-effects".to_owned())
            .spawn(move || {
                while let Ok(command) = command_receiver.recv() {
                    let HostEffectWorkerCommand::Run { operation, request } = command else {
                        break;
                    };
                    let outcome = match operation {
                        HostEffectWorkerOperation::Dispatch => {
                            HostEffectWorkerOutcome::Dispatched(driver.dispatch(&request))
                        }
                        HostEffectWorkerOperation::Reconcile => {
                            HostEffectWorkerOutcome::Reconciled(driver.reconcile(&request))
                        }
                    };
                    if result_sender
                        .send(HostEffectWorkerResult { request, outcome })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|error| HostEffectWorkerError::Spawn(error.to_string()))?;
        Ok(Self {
            commands,
            results,
            in_flight: None,
            worker: Some(worker),
        })
    }

    pub fn is_busy(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn in_flight(&self) -> Option<(OutboxItemId, HostEffectWorkerOperation)> {
        self.in_flight
    }

    pub fn try_submit(
        &mut self,
        operation: HostEffectWorkerOperation,
        request: HostEffectRequest,
    ) -> Result<(), HostEffectWorkerError> {
        if let Some((item_id, operation)) = self.in_flight {
            return Err(HostEffectWorkerError::Busy { item_id, operation });
        }
        let item_id = request.item_id;
        match self
            .commands
            .try_send(HostEffectWorkerCommand::Run { operation, request })
        {
            Ok(()) => {
                self.in_flight = Some((item_id, operation));
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(HostEffectWorkerError::Busy { item_id, operation }),
            Err(TrySendError::Disconnected(_)) => Err(HostEffectWorkerError::Closed),
        }
    }

    pub fn try_result(&mut self) -> Result<Option<HostEffectWorkerResult>, HostEffectWorkerError> {
        match self.results.try_recv() {
            Ok(result) => {
                self.in_flight = None;
                Ok(Some(result))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(HostEffectWorkerError::Closed),
        }
    }

    pub fn shutdown(&mut self) -> Result<(), HostEffectWorkerError> {
        if let Some(worker) = self.worker.take() {
            let send_result = self
                .commands
                .send(HostEffectWorkerCommand::Shutdown)
                .map_err(|_| HostEffectWorkerError::Closed);
            let join_result = worker.join().map_err(|payload| {
                HostEffectWorkerError::Join(
                    payload
                        .downcast_ref::<&str>()
                        .copied()
                        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                        .unwrap_or("unknown panic")
                        .to_owned(),
                )
            });
            self.in_flight = None;
            join_result?;
            send_result?;
        }
        self.in_flight = None;
        Ok(())
    }
}

impl Drop for HostEffectWorker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

pub struct FileEffectDriver {
    root: PathBuf,
}

impl FileEffectDriver {
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn request(
        &self,
        request: &HostEffectRequest,
    ) -> Result<(PathBuf, Vec<u8>, String), HostEffectError> {
        let expected = EffectId::from_host_operation("File/write_bytes")
            .map_err(|error| HostEffectError::rejected(error.to_string()))?;
        if request.effect_id != expected {
            return Err(HostEffectError::rejected(format!(
                "file effect driver does not own effect {}",
                request.effect_id
            )));
        }
        let StoredValue::Record(fields) = &request.intent else {
            return Err(HostEffectError::rejected(
                "File/write_bytes intent is not a record",
            ));
        };
        let path = text_field(fields, "path")?;
        let bytes = bytes_field(fields, "bytes")?.to_vec();
        let relative = validated_relative_path(path)?;
        Ok((self.root.join(relative), bytes, path.to_owned()))
    }
}

impl HostEffectDriver for FileEffectDriver {
    fn dispatch(&mut self, request: &HostEffectRequest) -> Result<StoredValue, HostEffectError> {
        let (path, bytes, result) = self.request(request)?;
        let parent = path.parent().ok_or_else(|| {
            HostEffectError::rejected("File/write_bytes path has no parent directory")
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            HostEffectError::rejected(format!("create effect output directory: {error}"))
        })?;
        let temporary = parent.join(format!(".boon-effect-{}.tmp", request.item_id));
        fs::write(&temporary, &bytes).map_err(|error| {
            HostEffectError::rejected(format!("write effect temporary file: {error}"))
        })?;
        fs::rename(&temporary, &path).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            HostEffectError::uncertain(format!("atomically replace effect output: {error}"))
        })?;
        Ok(StoredValue::Text(result))
    }

    fn reconcile(
        &mut self,
        request: &HostEffectRequest,
    ) -> Result<HostEffectReconciliation, HostEffectError> {
        let (path, expected, result) = self.request(request)?;
        match fs::read(path) {
            Ok(actual) if actual == expected => {
                Ok(HostEffectReconciliation::Applied(StoredValue::Text(result)))
            }
            Ok(_) => Ok(HostEffectReconciliation::NotApplied),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(HostEffectReconciliation::NotApplied)
            }
            Err(error) => Err(HostEffectError::uncertain(format!(
                "read effect output during reconciliation: {error}"
            ))),
        }
    }
}

fn text_field<'a>(
    fields: &'a BTreeMap<String, StoredValue>,
    name: &str,
) -> Result<&'a str, HostEffectError> {
    match fields.get(name) {
        Some(StoredValue::Text(value)) => Ok(value),
        _ => Err(HostEffectError::rejected(format!(
            "effect intent field `{name}` is not Text"
        ))),
    }
}

fn bytes_field<'a>(
    fields: &'a BTreeMap<String, StoredValue>,
    name: &str,
) -> Result<&'a [u8], HostEffectError> {
    match fields.get(name) {
        Some(StoredValue::Bytes(value)) => Ok(value),
        _ => Err(HostEffectError::rejected(format!(
            "effect intent field `{name}` is not Bytes"
        ))),
    }
}

fn validated_relative_path(path: &str) -> Result<&Path, HostEffectError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(HostEffectError::rejected(
            "effect output path must be a non-empty relative path without traversal",
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_persistence::DurableOutboxItem;

    fn item(root_path: &str, bytes: &[u8]) -> DurableOutboxItem {
        let effect = EffectId::from_host_operation("File/write_bytes").unwrap();
        let invocation =
            EffectInvocationId::from_semantic_route(effect, "save.press", "store.result").unwrap();
        let intent = StoredValue::Record(BTreeMap::from([
            ("bytes".to_owned(), StoredValue::Bytes(bytes.to_vec())),
            ("path".to_owned(), StoredValue::Text(root_path.to_owned())),
        ]));
        DurableOutboxItem::pending(
            invocation,
            effect,
            boon_persistence::canonical_intent_key(&intent),
            intent,
            None,
            1,
        )
    }

    #[test]
    fn file_effect_dispatch_and_reconciliation_are_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "boon-effect-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ));
        let _ = fs::remove_dir_all(&root);
        let mut driver = FileEffectDriver::new(&root).unwrap();
        let item = item("nested/output.bin", b"boon");
        let request = HostEffectRequest::from(&item);
        assert_eq!(
            driver.reconcile(&request).unwrap(),
            HostEffectReconciliation::NotApplied
        );
        assert_eq!(
            driver.dispatch(&request).unwrap(),
            StoredValue::Text("nested/output.bin".to_owned())
        );
        assert_eq!(
            driver.reconcile(&request).unwrap(),
            HostEffectReconciliation::Applied(StoredValue::Text("nested/output.bin".to_owned()))
        );
        assert_eq!(fs::read(root.join("nested/output.bin")).unwrap(), b"boon");
        fs::remove_dir_all(root).unwrap();
    }
}
