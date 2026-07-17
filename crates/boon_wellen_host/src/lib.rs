//! Bounded, content-addressed host adapter for official `wellen`.

#![forbid(unsafe_code)]

use boon_host_runtime::{
    ContentLease, ContentRef, ContentStore, ContentStoreError, ContentStoreErrorKind,
};
use boon_plan::{EffectDeliveryCardinality, EffectId, EffectInvocationId, FiniteReal};
use boon_runtime::{
    ProgramSession, RuntimeTurn, TransientEffectCallId, TransientEffectInvocation, Value,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use wellen::{FileFormat, Hierarchy, SignalEncoding, SignalValue, TimescaleUnit};

const MAX_DIAGNOSTIC_BYTES: usize = 1024;
const MAX_WAVEFORM_TEXT_BYTES: usize = 1024;
const MAX_SIGNAL_VALUE_BYTES: usize = 64 * 1024;
const MAX_SIGNAL_PAGE_PAYLOAD_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaveformEffectLimits {
    pub max_cached_waveforms: usize,
}

impl WaveformEffectLimits {
    pub const fn new(max_cached_waveforms: usize) -> Self {
        Self {
            max_cached_waveforms,
        }
    }
}

impl Default for WaveformEffectLimits {
    fn default() -> Self {
        Self {
            max_cached_waveforms: 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaveformAdapterErrorKind {
    InvalidConfiguration,
    InvalidDelivery,
    NotOwned,
    Capacity,
    Worker,
    Closed,
    Runtime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaveformAdapterError {
    kind: WaveformAdapterErrorKind,
    diagnostic: String,
}

impl WaveformAdapterError {
    fn new(kind: WaveformAdapterErrorKind, diagnostic: impl fmt::Display) -> Self {
        Self {
            kind,
            diagnostic: bounded_diagnostic(diagnostic.to_string()),
        }
    }

    pub const fn kind(&self) -> WaveformAdapterErrorKind {
        self.kind
    }

    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl fmt::Display for WaveformAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl std::error::Error for WaveformAdapterError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaveformEffectCompletion {
    pub call_id: TransientEffectCallId,
    pub invocation_id: EffectInvocationId,
    pub outcome: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaveformEffectSubmission {
    pub call_id: TransientEffectCallId,
    pub completion: WaveformEffectCompletion,
}

enum WaveformWorkerCommand {
    Submit(TransientEffectInvocation),
    Shutdown,
}

struct WaveformWorkerResult {
    call_id: TransientEffectCallId,
    result: Result<WaveformEffectCompletion, WaveformAdapterError>,
}

/// Bounded worker lane for parser and page I/O.
///
/// The worker never owns a Boon runtime. The runtime owner submits immutable
/// invocations and applies only still-current correlated completions.
pub struct WaveformEffectWorker {
    effects: WaveformEffectIds,
    commands: SyncSender<WaveformWorkerCommand>,
    results: Receiver<WaveformWorkerResult>,
    pending: BTreeSet<TransientEffectCallId>,
    worker: Option<JoinHandle<()>>,
}

impl WaveformEffectWorker {
    pub fn start(
        content_store: ContentStore,
        limits: WaveformEffectLimits,
        max_pending: usize,
    ) -> Result<Self, WaveformAdapterError> {
        if max_pending == 0 {
            return Err(WaveformAdapterError::new(
                WaveformAdapterErrorKind::InvalidConfiguration,
                "waveform worker pending capacity must be positive",
            ));
        }
        let adapter = WaveformEffectAdapter::with_limits(content_store, limits)?;
        let effects = adapter.effects;
        let (commands, command_rx) = mpsc::sync_channel(max_pending);
        let (result_tx, results) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("boon-wellen-host".to_owned())
            .spawn(move || {
                let mut adapter = adapter;
                while let Ok(command) = command_rx.recv() {
                    let WaveformWorkerCommand::Submit(invocation) = command else {
                        break;
                    };
                    let call_id = invocation.call_id;
                    let result = adapter
                        .submit(invocation)
                        .map(|submission| submission.completion);
                    if result_tx
                        .send(WaveformWorkerResult { call_id, result })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|error| WaveformAdapterError::new(WaveformAdapterErrorKind::Worker, error))?;
        Ok(Self {
            effects,
            commands,
            results,
            pending: BTreeSet::new(),
            worker: Some(worker),
        })
    }

    pub fn effect_ids(&self) -> [EffectId; 4] {
        self.effects.all()
    }

    pub fn owns(&self, effect_id: EffectId) -> bool {
        self.effects.operation(effect_id).is_some()
    }

    pub fn submit(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<(), WaveformAdapterError> {
        if !self.owns(invocation.effect_id) {
            return Err(WaveformAdapterError::new(
                WaveformAdapterErrorKind::NotOwned,
                format_args!(
                    "waveform worker does not own effect {}",
                    invocation.effect_id
                ),
            ));
        }
        if invocation.delivery != EffectDeliveryCardinality::Single {
            return Err(WaveformAdapterError::new(
                WaveformAdapterErrorKind::InvalidDelivery,
                "Wellen bridge operations require single-result delivery",
            ));
        }
        let call_id = invocation.call_id;
        match self
            .commands
            .try_send(WaveformWorkerCommand::Submit(invocation))
        {
            Ok(()) => {
                self.pending.insert(call_id);
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(WaveformAdapterError::new(
                WaveformAdapterErrorKind::Capacity,
                "waveform worker queue is full",
            )),
            Err(TrySendError::Disconnected(_)) => Err(WaveformAdapterError::new(
                WaveformAdapterErrorKind::Closed,
                "waveform worker is closed",
            )),
        }
    }

    pub fn try_completion(
        &mut self,
    ) -> Result<Option<WaveformEffectCompletion>, WaveformAdapterError> {
        loop {
            let result = match self.results.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => {
                    return Err(WaveformAdapterError::new(
                        WaveformAdapterErrorKind::Closed,
                        "waveform worker completion lane is closed",
                    ));
                }
            };
            if !self.pending.remove(&result.call_id) {
                continue;
            }
            return result.result.map(Some);
        }
    }

    pub fn cancel(&mut self, call_id: TransientEffectCallId) -> bool {
        self.pending.remove(&call_id)
    }

    pub fn cancel_all(&mut self) -> Vec<TransientEffectCallId> {
        let calls = self.pending.iter().copied().collect::<Vec<_>>();
        self.pending.clear();
        calls
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn is_busy(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn shutdown(&mut self) -> Result<(), WaveformAdapterError> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let send = self
            .commands
            .send(WaveformWorkerCommand::Shutdown)
            .map_err(|_| {
                WaveformAdapterError::new(
                    WaveformAdapterErrorKind::Closed,
                    "waveform worker is closed",
                )
            });
        let join = worker.join().map_err(|payload| {
            WaveformAdapterError::new(
                WaveformAdapterErrorKind::Worker,
                payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("waveform worker panicked"),
            )
        });
        self.pending.clear();
        join?;
        send
    }
}

impl Drop for WaveformEffectWorker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Clone, Copy)]
struct WaveformEffectIds {
    open: EffectId,
    hierarchy_page: EffectId,
    signal_page: EffectId,
    cursor_values: EffectId,
}

impl WaveformEffectIds {
    fn new() -> Result<Self, WaveformAdapterError> {
        Ok(Self {
            open: effect_id(boon_effect_schema::WELLEN_OPEN_OPERATION)?,
            hierarchy_page: effect_id(boon_effect_schema::WELLEN_HIERARCHY_PAGE_OPERATION)?,
            signal_page: effect_id(boon_effect_schema::WELLEN_SIGNAL_PAGE_OPERATION)?,
            cursor_values: effect_id(boon_effect_schema::WELLEN_CURSOR_VALUES_OPERATION)?,
        })
    }

    fn operation(self, id: EffectId) -> Option<WaveformOperation> {
        if id == self.open {
            Some(WaveformOperation::Open)
        } else if id == self.hierarchy_page {
            Some(WaveformOperation::HierarchyPage)
        } else if id == self.signal_page {
            Some(WaveformOperation::SignalPage)
        } else if id == self.cursor_values {
            Some(WaveformOperation::CursorValues)
        } else {
            None
        }
    }

    fn all(self) -> [EffectId; 4] {
        [
            self.open,
            self.hierarchy_page,
            self.signal_page,
            self.cursor_values,
        ]
    }
}

#[derive(Clone, Copy)]
enum WaveformOperation {
    Open,
    HierarchyPage,
    SignalPage,
    CursorValues,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Artifact {
    content: ContentRef,
    format: String,
    schema_version: String,
    parser_version: String,
}

struct OpenWaveform {
    _content_lease: ContentLease,
    waveform: wellen::simple::Waveform,
    last_used: u64,
}

/// Thin adapter around official `wellen` with a bounded parser cache.
///
/// Artifacts are structural `ContentRef` descriptors plus format/schema
/// versions. Cache eviction never invalidates an artifact: the adapter reparses
/// immutable content from `ContentStore`. Opening/parsing performs I/O and must
/// be called from a host worker lane, never a renderer callback.
pub struct WaveformEffectAdapter {
    content_store: ContentStore,
    effects: WaveformEffectIds,
    limits: WaveformEffectLimits,
    waveforms: BTreeMap<ContentRef, OpenWaveform>,
    use_sequence: u64,
}

impl WaveformEffectAdapter {
    pub fn new(
        content_store: ContentStore,
        max_cached_waveforms: usize,
    ) -> Result<Self, WaveformAdapterError> {
        Self::with_limits(
            content_store,
            WaveformEffectLimits::new(max_cached_waveforms),
        )
    }

    pub fn with_limits(
        content_store: ContentStore,
        limits: WaveformEffectLimits,
    ) -> Result<Self, WaveformAdapterError> {
        if limits.max_cached_waveforms == 0 {
            return Err(WaveformAdapterError::new(
                WaveformAdapterErrorKind::InvalidConfiguration,
                "waveform parser cache capacity must be positive",
            ));
        }
        Ok(Self {
            content_store,
            effects: WaveformEffectIds::new()?,
            limits,
            waveforms: BTreeMap::new(),
            use_sequence: 0,
        })
    }

    pub fn effect_ids(&self) -> [EffectId; 4] {
        self.effects.all()
    }

    pub fn owns(&self, effect_id: EffectId) -> bool {
        self.effects.operation(effect_id).is_some()
    }

    pub const fn limits(&self) -> WaveformEffectLimits {
        self.limits
    }

    pub fn cached_waveform_count(&self) -> usize {
        self.waveforms.len()
    }

    pub fn content_store(&self) -> &ContentStore {
        &self.content_store
    }

    pub fn submit(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<WaveformEffectSubmission, WaveformAdapterError> {
        if invocation.delivery != EffectDeliveryCardinality::Single {
            return Err(WaveformAdapterError::new(
                WaveformAdapterErrorKind::InvalidDelivery,
                "Wellen bridge operations require single-result delivery",
            ));
        }
        let operation = self
            .effects
            .operation(invocation.effect_id)
            .ok_or_else(|| {
                WaveformAdapterError::new(
                    WaveformAdapterErrorKind::NotOwned,
                    format_args!(
                        "waveform adapter does not own effect {}",
                        invocation.effect_id
                    ),
                )
            })?;
        let outcome = match operation {
            WaveformOperation::Open => self.open(&invocation.intent),
            WaveformOperation::HierarchyPage => self.hierarchy_page(&invocation.intent),
            WaveformOperation::SignalPage => self.signal_page(&invocation.intent),
            WaveformOperation::CursorValues => self.cursor_values(&invocation.intent),
        };
        let completion = WaveformEffectCompletion {
            call_id: invocation.call_id,
            invocation_id: invocation.invocation_id,
            outcome,
        };
        Ok(WaveformEffectSubmission {
            call_id: invocation.call_id,
            completion,
        })
    }

    fn open(&mut self, intent: &Value) -> Value {
        match self.try_open(intent) {
            Ok(value) => value,
            Err(failure) => failure.into_value(),
        }
    }

    fn try_open(&mut self, intent: &Value) -> Result<Value, WaveformFailure> {
        let fields = exact_record(intent, &["content"], "Wellen/open intent")?;
        let content =
            ContentRef::from_value(required(fields, "content")?).map_err(content_failure)?;
        self.ensure_content_loaded(content)?;
        let waveform = self
            .waveforms
            .get(&content)
            .expect("content was loaded before open response");
        opened_value(content, &waveform.waveform)
    }

    fn hierarchy_page(&mut self, intent: &Value) -> Value {
        match self.try_hierarchy_page(intent) {
            Ok(value) => value,
            Err(failure) => failure.into_value(),
        }
    }

    fn try_hierarchy_page(&mut self, intent: &Value) -> Result<Value, WaveformFailure> {
        let fields = exact_record(
            intent,
            &["artifact", "limit", "offset", "request_fingerprint"],
            "Wellen/hierarchy_page intent",
        )?;
        let artifact = decode_artifact(required(fields, "artifact")?)?;
        let fingerprint = bounded_waveform_text(
            text_field(fields, "request_fingerprint")?,
            "request fingerprint",
        )?;
        let offset = unsigned_usize(fields, "offset", 0, usize::MAX)?;
        let limit = unsigned_usize(
            fields,
            "limit",
            1,
            boon_effect_schema::WELLEN_MAX_HIERARCHY_ROWS as usize,
        )?;
        self.ensure_artifact_loaded(&artifact)?;
        let use_sequence = self.next_use_sequence();
        let open = self
            .waveforms
            .get_mut(&artifact.content)
            .expect("artifact was loaded");
        open.last_used = use_sequence;
        let hierarchy = open.waveform.hierarchy();
        let scope_count = hierarchy.iter_scopes().count();
        let signal_count = hierarchy.iter_vars().count();
        let total_rows = scope_count
            .checked_add(signal_count)
            .ok_or_else(|| WaveformFailure::new("too_many_rows", "hierarchy row count overflow"))?;
        let end = offset.saturating_add(limit).min(total_rows);
        let mut rows = Vec::with_capacity(end.saturating_sub(offset));
        let mut signal_ids = Vec::new();
        if offset < scope_count {
            let scope_end = end.min(scope_count);
            for scope in hierarchy
                .iter_scopes()
                .skip(offset)
                .take(scope_end - offset)
            {
                rows.push(hierarchy_scope_row(hierarchy, scope)?);
            }
        }
        if end > scope_count {
            let first_signal = offset.saturating_sub(scope_count);
            let signal_end = end - scope_count;
            for signal in hierarchy
                .iter_vars()
                .skip(first_signal)
                .take(signal_end - first_signal)
            {
                signal_ids.push(Value::Text(bounded_waveform_text(
                    signal.full_name(hierarchy),
                    "signal id",
                )?));
                rows.push(hierarchy_signal_row(hierarchy, signal)?);
            }
        }
        let (start_time, end_time) = time_bounds(open.waveform.time_table());
        Ok(tagged(
            "HierarchyPage",
            BTreeMap::from([
                ("artifact".to_owned(), artifact_value(&artifact)?),
                ("request_fingerprint".to_owned(), Value::Text(fingerprint)),
                ("start_time".to_owned(), number_from_u64(start_time)?),
                ("end_time".to_owned(), number_from_u64(end_time)?),
                ("offset".to_owned(), number_from_usize(offset)?),
                ("has_more".to_owned(), Value::Bool(end < total_rows)),
                ("next_offset".to_owned(), number_from_usize(end)?),
                ("total_rows".to_owned(), number_from_usize(total_rows)?),
                ("signal_ids".to_owned(), Value::List(signal_ids)),
                ("rows".to_owned(), Value::List(rows)),
            ]),
        ))
    }

    fn signal_page(&mut self, intent: &Value) -> Value {
        match self.try_signal_page(intent) {
            Ok(value) => value,
            Err(failure) => failure.into_value(),
        }
    }

    fn try_signal_page(&mut self, intent: &Value) -> Result<Value, WaveformFailure> {
        let fields = exact_record(
            intent,
            &[
                "artifact",
                "end_time",
                "max_transitions",
                "offset",
                "request_fingerprint",
                "signal_ids",
                "start_time",
            ],
            "Wellen/signal_page intent",
        )?;
        let artifact = decode_artifact(required(fields, "artifact")?)?;
        let fingerprint = bounded_waveform_text(
            text_field(fields, "request_fingerprint")?,
            "request fingerprint",
        )?;
        let signal_ids = bounded_signal_ids(required(fields, "signal_ids")?)?;
        let start_time = unsigned_u64(
            fields,
            "start_time",
            0,
            boon_effect_schema::WELLEN_MAX_SAFE_TIME,
        )?;
        let end_time = unsigned_u64(
            fields,
            "end_time",
            0,
            boon_effect_schema::WELLEN_MAX_SAFE_TIME,
        )?;
        if start_time > end_time {
            return Err(WaveformFailure::invalid(
                "signal page start_time must not exceed end_time",
            ));
        }
        let offset = unsigned_usize(fields, "offset", 0, usize::MAX)?;
        let max_transitions = unsigned_usize(
            fields,
            "max_transitions",
            1,
            boon_effect_schema::WELLEN_MAX_SIGNAL_TRANSITIONS as usize,
        )?;
        self.ensure_artifact_loaded(&artifact)?;
        let use_sequence = self.next_use_sequence();
        let open = self
            .waveforms
            .get_mut(&artifact.content)
            .expect("artifact was loaded");
        open.last_used = use_sequence;
        let resolved = resolve_signals(&open.waveform, &signal_ids)?;
        let result_signal_ids = signal_ids
            .iter()
            .cloned()
            .map(Value::Text)
            .collect::<Vec<_>>();
        let signal_refs = resolved
            .iter()
            .map(|(_, signal_ref)| *signal_ref)
            .collect::<Vec<_>>();
        open.waveform.load_signals(&signal_refs);
        let time_table = open.waveform.time_table();
        let mut skipped = 0_usize;
        let mut emitted = 0_usize;
        let mut payload_bytes = 0_usize;
        let mut has_more = false;
        let mut signal_rows = Vec::with_capacity(resolved.len());
        for (signal_id, signal_ref) in resolved {
            let signal = open.waveform.get_signal(signal_ref).ok_or_else(|| {
                WaveformFailure::new(
                    "signal_unavailable",
                    "wellen did not load a requested signal",
                )
            })?;
            let mut transitions = Vec::new();
            let mut page_full = false;
            for (time_index, value) in signal.iter_changes() {
                let time = *time_table.get(time_index as usize).ok_or_else(|| {
                    WaveformFailure::new(
                        "invalid_waveform",
                        "signal references a missing waveform time entry",
                    )
                })?;
                if time < start_time || time > end_time {
                    continue;
                }
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                let (value, value_bytes) = waveform_value(value)?;
                let row_bytes = value_bytes.saturating_add(32);
                if emitted == max_transitions
                    || payload_bytes.saturating_add(row_bytes) > MAX_SIGNAL_PAGE_PAYLOAD_BYTES
                {
                    has_more = true;
                    page_full = true;
                    break;
                }
                payload_bytes += row_bytes;
                emitted += 1;
                transitions.push(Value::Record(BTreeMap::from([
                    ("time".to_owned(), number_from_u64(time)?),
                    ("value".to_owned(), value),
                ])));
            }
            signal_rows.push(Value::Record(BTreeMap::from([
                ("signal_id".to_owned(), Value::Text(signal_id)),
                ("transitions".to_owned(), Value::List(transitions)),
            ])));
            if page_full {
                break;
            }
        }
        let next_offset = offset.saturating_add(emitted);
        Ok(tagged(
            "SignalPage",
            BTreeMap::from([
                ("artifact".to_owned(), artifact_value(&artifact)?),
                ("request_fingerprint".to_owned(), Value::Text(fingerprint)),
                ("signal_ids".to_owned(), Value::List(result_signal_ids)),
                ("start_time".to_owned(), number_from_u64(start_time)?),
                ("end_time".to_owned(), number_from_u64(end_time)?),
                ("offset".to_owned(), number_from_usize(offset)?),
                ("has_more".to_owned(), Value::Bool(has_more)),
                ("next_offset".to_owned(), number_from_usize(next_offset)?),
                ("signals".to_owned(), Value::List(signal_rows)),
            ]),
        ))
    }

    fn cursor_values(&mut self, intent: &Value) -> Value {
        match self.try_cursor_values(intent) {
            Ok(value) => value,
            Err(failure) => failure.into_value(),
        }
    }

    fn try_cursor_values(&mut self, intent: &Value) -> Result<Value, WaveformFailure> {
        let fields = exact_record(
            intent,
            &[
                "artifact",
                "cursor_time",
                "request_fingerprint",
                "signal_ids",
            ],
            "Wellen/cursor_values intent",
        )?;
        let artifact = decode_artifact(required(fields, "artifact")?)?;
        let fingerprint = bounded_waveform_text(
            text_field(fields, "request_fingerprint")?,
            "request fingerprint",
        )?;
        let cursor_time = unsigned_u64(
            fields,
            "cursor_time",
            0,
            boon_effect_schema::WELLEN_MAX_SAFE_TIME,
        )?;
        let signal_ids = bounded_signal_ids(required(fields, "signal_ids")?)?;
        self.ensure_artifact_loaded(&artifact)?;
        let use_sequence = self.next_use_sequence();
        let open = self
            .waveforms
            .get_mut(&artifact.content)
            .expect("artifact was loaded");
        open.last_used = use_sequence;
        let resolved = resolve_signals(&open.waveform, &signal_ids)?;
        let signal_refs = resolved
            .iter()
            .map(|(_, signal_ref)| *signal_ref)
            .collect::<Vec<_>>();
        open.waveform.load_signals(&signal_refs);
        let time_index = open
            .waveform
            .time_table()
            .partition_point(|time| *time <= cursor_time)
            .checked_sub(1)
            .and_then(|index| u32::try_from(index).ok());
        let mut rows = Vec::with_capacity(resolved.len());
        for (signal_id, signal_ref) in resolved {
            let value = match (time_index, open.waveform.get_signal(signal_ref)) {
                (Some(time_index), Some(signal)) => match signal.get_offset(time_index) {
                    Some(offset) => {
                        waveform_value(
                            signal.get_value_at(&offset, offset.elements.saturating_sub(1)),
                        )?
                        .0
                    }
                    None => tagged("UnavailableValue", BTreeMap::new()),
                },
                _ => tagged("UnavailableValue", BTreeMap::new()),
            };
            rows.push(Value::Record(BTreeMap::from([
                ("signal_id".to_owned(), Value::Text(signal_id)),
                ("value".to_owned(), value),
            ])));
        }
        Ok(tagged(
            "CursorValues",
            BTreeMap::from([
                ("artifact".to_owned(), artifact_value(&artifact)?),
                ("request_fingerprint".to_owned(), Value::Text(fingerprint)),
                ("cursor_time".to_owned(), number_from_u64(cursor_time)?),
                ("rows".to_owned(), Value::List(rows)),
            ]),
        ))
    }

    fn ensure_artifact_loaded(&mut self, artifact: &Artifact) -> Result<(), WaveformFailure> {
        if artifact.schema_version != boon_effect_schema::WELLEN_BRIDGE_SCHEMA_VERSION {
            return Err(WaveformFailure::new(
                "schema_mismatch",
                "waveform artifact schema version is unsupported",
            ));
        }
        if artifact.parser_version != wellen::VERSION {
            return Err(WaveformFailure::new(
                "parser_mismatch",
                "waveform artifact parser version is unsupported",
            ));
        }
        self.ensure_content_loaded(artifact.content)?;
        let open = self
            .waveforms
            .get(&artifact.content)
            .expect("content was loaded");
        if file_format(open.waveform.hierarchy().file_format()) != artifact.format {
            return Err(WaveformFailure::new(
                "format_mismatch",
                "waveform artifact format differs from retained content",
            ));
        }
        Ok(())
    }

    fn ensure_content_loaded(&mut self, content: ContentRef) -> Result<(), WaveformFailure> {
        let use_sequence = self.next_use_sequence();
        if let Some(open) = self.waveforms.get_mut(&content) {
            open.last_used = use_sequence;
            return Ok(());
        }
        let lease = self
            .content_store
            .resolve(content)
            .map_err(content_failure)?;
        let waveform = catch_unwind(AssertUnwindSafe(|| wellen::simple::read(lease.path())))
            .map_err(|_| {
                WaveformFailure::new(
                    "parser_panicked",
                    "wellen rejected retained content without exposing host state",
                )
            })?
            .map_err(wellen_failure)?;
        ensure_supported_time_range(waveform.time_table())?;
        if self.waveforms.len() == self.limits.max_cached_waveforms {
            self.evict_least_recently_used();
        }
        self.waveforms.insert(
            content,
            OpenWaveform {
                _content_lease: lease,
                waveform,
                last_used: use_sequence,
            },
        );
        Ok(())
    }

    fn next_use_sequence(&mut self) -> u64 {
        self.use_sequence = self.use_sequence.wrapping_add(1).max(1);
        self.use_sequence
    }

    fn evict_least_recently_used(&mut self) {
        let victim = self
            .waveforms
            .iter()
            .min_by_key(|(_, waveform)| waveform.last_used)
            .map(|(content, _)| *content);
        if let Some(victim) = victim {
            self.waveforms.remove(&victim);
        }
    }
}

pub fn apply_waveform_completion(
    program: &mut ProgramSession,
    completion: WaveformEffectCompletion,
) -> Result<RuntimeTurn, WaveformAdapterError> {
    program
        .complete_transient_effect(completion.call_id, completion.outcome)
        .map_err(|error| WaveformAdapterError::new(WaveformAdapterErrorKind::Runtime, error))
}

fn effect_id(operation: &str) -> Result<EffectId, WaveformAdapterError> {
    EffectId::from_host_operation(operation).map_err(|error| {
        WaveformAdapterError::new(WaveformAdapterErrorKind::InvalidConfiguration, error)
    })
}

fn opened_value(
    content: ContentRef,
    waveform: &wellen::simple::Waveform,
) -> Result<Value, WaveformFailure> {
    let hierarchy = waveform.hierarchy();
    let format = file_format(hierarchy.file_format()).to_owned();
    let artifact = Artifact {
        content,
        format: format.clone(),
        schema_version: boon_effect_schema::WELLEN_BRIDGE_SCHEMA_VERSION.to_owned(),
        parser_version: wellen::VERSION.to_owned(),
    };
    let (start_time, end_time) = time_bounds(waveform.time_table());
    let (timescale_factor, timescale_unit) = match hierarchy.timescale() {
        Some(timescale) => (u64::from(timescale.factor), timescale_unit(timescale.unit)),
        None => (0, "Unknown"),
    };
    Ok(tagged(
        "WaveformOpened",
        BTreeMap::from([
            ("artifact".to_owned(), artifact_value(&artifact)?),
            ("format".to_owned(), Value::Text(format)),
            (
                "byte_length".to_owned(),
                number_from_u64(content.byte_count())?,
            ),
            ("start_time".to_owned(), number_from_u64(start_time)?),
            ("end_time".to_owned(), number_from_u64(end_time)?),
            (
                "timescale_factor".to_owned(),
                number_from_u64(timescale_factor)?,
            ),
            (
                "timescale_unit".to_owned(),
                Value::Text(timescale_unit.to_owned()),
            ),
            (
                "scope_count".to_owned(),
                number_from_usize(hierarchy.iter_scopes().count())?,
            ),
            (
                "signal_count".to_owned(),
                number_from_usize(hierarchy.iter_vars().count())?,
            ),
            (
                "hierarchy_bytes".to_owned(),
                number_from_usize(hierarchy.size_in_memory())?,
            ),
            (
                "provider".to_owned(),
                Value::Text(format!("wellen/{}", wellen::VERSION)),
            ),
        ]),
    ))
}

fn hierarchy_scope_row(
    hierarchy: &Hierarchy,
    scope: &wellen::Scope,
) -> Result<Value, WaveformFailure> {
    let full_name = bounded_waveform_text(scope.full_name(hierarchy), "scope id")?;
    let name = bounded_waveform_text(scope.name(hierarchy), "scope name")?;
    let parent = full_name
        .rsplit_once('.')
        .map(|(parent, _)| format!("scope:{parent}"))
        .unwrap_or_default();
    Ok(Value::Record(BTreeMap::from([
        ("kind".to_owned(), Value::Text("Scope".to_owned())),
        ("id".to_owned(), Value::Text(format!("scope:{full_name}"))),
        ("parent_id".to_owned(), Value::Text(parent)),
        ("name".to_owned(), Value::Text(name)),
        ("signal_id".to_owned(), Value::Text(String::new())),
        ("width".to_owned(), number(0)),
        ("encoding".to_owned(), Value::Text("Scope".to_owned())),
    ])))
}

fn hierarchy_signal_row(
    hierarchy: &Hierarchy,
    var: &wellen::Var,
) -> Result<Value, WaveformFailure> {
    let full_name = bounded_waveform_text(var.full_name(hierarchy), "signal id")?;
    let name = bounded_waveform_text(var.name(hierarchy), "signal name")?;
    let parent = full_name
        .rsplit_once('.')
        .map(|(parent, _)| format!("scope:{parent}"))
        .unwrap_or_default();
    let (width, encoding) = match var.signal_encoding() {
        SignalEncoding::BitVector(width) => (u64::from(width.get()), "Bits"),
        SignalEncoding::Real => (0, "Real"),
        SignalEncoding::String => (0, "String"),
    };
    Ok(Value::Record(BTreeMap::from([
        ("kind".to_owned(), Value::Text("Signal".to_owned())),
        ("id".to_owned(), Value::Text(format!("signal:{full_name}"))),
        ("parent_id".to_owned(), Value::Text(parent)),
        ("name".to_owned(), Value::Text(name)),
        ("signal_id".to_owned(), Value::Text(full_name)),
        ("width".to_owned(), number_from_u64(width)?),
        ("encoding".to_owned(), Value::Text(encoding.to_owned())),
    ])))
}

fn resolve_signals(
    waveform: &wellen::simple::Waveform,
    signal_ids: &[String],
) -> Result<Vec<(String, wellen::SignalRef)>, WaveformFailure> {
    signal_ids
        .iter()
        .map(|signal_id| {
            waveform
                .hierarchy()
                .iter_vars()
                .find(|var| var.full_name(waveform.hierarchy()) == *signal_id)
                .map(|var| (signal_id.clone(), var.signal_ref()))
                .ok_or_else(|| {
                    WaveformFailure::new(
                        "unknown_signal",
                        "signal id is absent from the waveform artifact",
                    )
                })
        })
        .collect()
}

fn bounded_signal_ids(value: &Value) -> Result<Vec<String>, WaveformFailure> {
    let Value::List(values) = value else {
        return Err(WaveformFailure::invalid("signal_ids must be LIST<TEXT>"));
    };
    if values.is_empty() || values.len() > boon_effect_schema::WELLEN_MAX_CURSOR_SIGNALS {
        return Err(WaveformFailure::invalid(
            "signal_ids must be nonempty and within the bounded signal count",
        ));
    }
    let mut unique = BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let Value::Text(value) = value else {
                return Err(WaveformFailure::invalid(
                    "signal_ids must contain only Text values",
                ));
            };
            let value = bounded_waveform_text(value, "signal id")?;
            if !unique.insert(value.clone()) {
                return Err(WaveformFailure::invalid(
                    "signal_ids must not contain duplicates",
                ));
            }
            Ok(value)
        })
        .collect()
}

fn decode_artifact(value: &Value) -> Result<Artifact, WaveformFailure> {
    let fields = exact_record(
        value,
        &["content", "format", "parser_version", "schema_version"],
        "waveform artifact",
    )?;
    Ok(Artifact {
        content: ContentRef::from_value(required(fields, "content")?).map_err(content_failure)?,
        format: bounded_waveform_text(text_field(fields, "format")?, "artifact format")?,
        schema_version: bounded_waveform_text(
            text_field(fields, "schema_version")?,
            "artifact schema version",
        )?,
        parser_version: bounded_waveform_text(
            text_field(fields, "parser_version")?,
            "artifact parser version",
        )?,
    })
}

fn artifact_value(artifact: &Artifact) -> Result<Value, WaveformFailure> {
    Ok(Value::Record(BTreeMap::from([
        (
            "content".to_owned(),
            artifact.content.value().map_err(content_failure)?,
        ),
        ("format".to_owned(), Value::Text(artifact.format.clone())),
        (
            "schema_version".to_owned(),
            Value::Text(artifact.schema_version.clone()),
        ),
        (
            "parser_version".to_owned(),
            Value::Text(artifact.parser_version.clone()),
        ),
    ])))
}

fn ensure_supported_time_range(time_table: &[u64]) -> Result<(), WaveformFailure> {
    if time_table
        .last()
        .is_some_and(|time| *time > boon_effect_schema::WELLEN_MAX_SAFE_TIME)
    {
        return Err(WaveformFailure::new(
            "time_range_unsupported",
            "waveform tick range exceeds the exact Number contract",
        ));
    }
    Ok(())
}

fn time_bounds(time_table: &[u64]) -> (u64, u64) {
    (
        time_table.first().copied().unwrap_or(0),
        time_table.last().copied().unwrap_or(0),
    )
}

fn file_format(format: FileFormat) -> &'static str {
    match format {
        FileFormat::Vcd => "VCD",
        FileFormat::Fst => "FST",
        FileFormat::Ghw => "GHW",
        FileFormat::Unknown => "Unknown",
    }
}

fn timescale_unit(unit: TimescaleUnit) -> &'static str {
    match unit {
        TimescaleUnit::ZeptoSeconds => "zs",
        TimescaleUnit::AttoSeconds => "as",
        TimescaleUnit::FemtoSeconds => "fs",
        TimescaleUnit::PicoSeconds => "ps",
        TimescaleUnit::NanoSeconds => "ns",
        TimescaleUnit::MicroSeconds => "us",
        TimescaleUnit::MilliSeconds => "ms",
        TimescaleUnit::Seconds => "s",
        TimescaleUnit::Unknown => "Unknown",
    }
}

fn waveform_value(value: SignalValue<'_>) -> Result<(Value, usize), WaveformFailure> {
    match value {
        SignalValue::Binary(_, _) => bits_value("BinaryValue", value),
        SignalValue::FourValue(_, _) => bits_value("FourStateValue", value),
        SignalValue::NineValue(_, _) => bits_value("NineStateValue", value),
        SignalValue::String(value) => {
            let value = bounded_signal_text(value)?;
            let bytes = value.len();
            Ok((
                tagged(
                    "StringValue",
                    BTreeMap::from([("text".to_owned(), Value::Text(value))]),
                ),
                bytes,
            ))
        }
        SignalValue::Real(value) if value.is_finite() => Ok((
            tagged(
                "RealValue",
                BTreeMap::from([(
                    "value".to_owned(),
                    Value::Number(FiniteReal::new(value).map_err(|_| {
                        WaveformFailure::new(
                            "invalid_real",
                            "finite waveform real is not a valid Number",
                        )
                    })?),
                )]),
            ),
            std::mem::size_of::<f64>(),
        )),
        SignalValue::Real(value) => {
            let classification = if value.is_nan() {
                "NaN"
            } else if value.is_sign_positive() {
                "PositiveInfinity"
            } else {
                "NegativeInfinity"
            };
            Ok((
                tagged(
                    "NonFiniteReal",
                    BTreeMap::from([(
                        "classification".to_owned(),
                        Value::Text(classification.to_owned()),
                    )]),
                ),
                classification.len(),
            ))
        }
    }
}

fn bits_value(
    tag: &'static str,
    value: SignalValue<'_>,
) -> Result<(Value, usize), WaveformFailure> {
    let bits = value.to_bit_string().ok_or_else(|| {
        WaveformFailure::new("invalid_signal", "bit signal has no bit representation")
    })?;
    let bits = bounded_signal_text(bits)?;
    let bytes = bits.len();
    Ok((
        tagged(
            tag,
            BTreeMap::from([("bits".to_owned(), Value::Text(bits))]),
        ),
        bytes,
    ))
}

fn bounded_signal_text(value: impl Into<String>) -> Result<String, WaveformFailure> {
    let value = value.into();
    if value.len() > MAX_SIGNAL_VALUE_BYTES {
        return Err(WaveformFailure::new(
            "signal_value_too_large",
            "one signal value exceeds the bounded page payload",
        ));
    }
    Ok(value)
}

fn bounded_waveform_text(
    value: impl Into<String>,
    context: &str,
) -> Result<String, WaveformFailure> {
    let value = value.into();
    if value.len() > MAX_WAVEFORM_TEXT_BYTES {
        return Err(WaveformFailure::new(
            "waveform_text_too_large",
            format!("{context} exceeds the bounded text contract"),
        ));
    }
    Ok(value)
}

fn exact_record<'a>(
    value: &'a Value,
    expected: &[&str],
    context: &str,
) -> Result<&'a BTreeMap<String, Value>, WaveformFailure> {
    let Value::Record(fields) = value else {
        return Err(WaveformFailure::invalid(format!(
            "{context} must be a record"
        )));
    };
    let actual = fields.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(WaveformFailure::invalid(format!(
            "{context} fields differ from the typed contract"
        )));
    }
    Ok(fields)
}

fn required<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<&'a Value, WaveformFailure> {
    fields
        .get(name)
        .ok_or_else(|| WaveformFailure::invalid(format!("missing `{name}` field")))
}

fn text_field<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<&'a str, WaveformFailure> {
    match fields.get(name) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(WaveformFailure::invalid(format!(
            "field `{name}` must be Text"
        ))),
    }
}

fn unsigned_usize(
    fields: &BTreeMap<String, Value>,
    name: &str,
    min: usize,
    max: usize,
) -> Result<usize, WaveformFailure> {
    let max = max.min(boon_effect_schema::WELLEN_MAX_SAFE_TIME as usize);
    let value = unsigned_u64(fields, name, min as u64, max as u64)?;
    usize::try_from(value).map_err(|_| WaveformFailure::invalid("numeric field exceeds usize"))
}

fn unsigned_u64(
    fields: &BTreeMap<String, Value>,
    name: &str,
    min: u64,
    max: u64,
) -> Result<u64, WaveformFailure> {
    let Some(Value::Number(value)) = fields.get(name) else {
        return Err(WaveformFailure::invalid(format!(
            "field `{name}` must be Number"
        )));
    };
    let value = value
        .to_i64_exact()
        .map_err(|_| WaveformFailure::invalid(format!("field `{name}` must be a whole number")))?;
    let value = u64::try_from(value)
        .map_err(|_| WaveformFailure::invalid(format!("field `{name}` must not be negative")))?;
    if !(min..=max).contains(&value) {
        return Err(WaveformFailure::invalid(format!(
            "field `{name}` is outside the bounded contract"
        )));
    }
    Ok(value)
}

fn number(value: i64) -> Value {
    Value::Number(FiniteReal::from_i64_exact(value).expect("small constant is exact"))
}

fn number_from_usize(value: usize) -> Result<Value, WaveformFailure> {
    number_from_u64(
        u64::try_from(value)
            .map_err(|_| WaveformFailure::new("number_out_of_range", "host count exceeds u64"))?,
    )
}

fn number_from_u64(value: u64) -> Result<Value, WaveformFailure> {
    if value > boon_effect_schema::WELLEN_MAX_SAFE_TIME {
        return Err(WaveformFailure::new(
            "number_out_of_range",
            "waveform value exceeds the exact Number contract",
        ));
    }
    Ok(number(i64::try_from(value).expect("safe Number fits i64")))
}

fn tagged(tag: &str, fields: BTreeMap<String, Value>) -> Value {
    let mut tagged = BTreeMap::from([("$tag".to_owned(), Value::Text(tag.to_owned()))]);
    tagged.extend(fields);
    Value::Record(tagged)
}

fn content_failure(error: ContentStoreError) -> WaveformFailure {
    let code = match error.kind() {
        ContentStoreErrorKind::Missing => "content_missing",
        ContentStoreErrorKind::Capacity => "content_store_full",
        ContentStoreErrorKind::InvalidReference => "invalid_content_ref",
        ContentStoreErrorKind::InvalidConfiguration => "content_store_invalid",
        ContentStoreErrorKind::Io => "content_io",
    };
    WaveformFailure::new(code, error.diagnostic())
}

fn wellen_failure(error: wellen::WellenError) -> WaveformFailure {
    let (code, diagnostic) = match error {
        wellen::WellenError::UnknownFileFormat => (
            "unsupported_format",
            "retained content is not a supported VCD, FST, or GHW waveform".to_owned(),
        ),
        wellen::WellenError::Io(error) => (
            "content_io",
            format!("wellen could not read retained content: {:?}", error.kind()),
        ),
        wellen::WellenError::FailedToLoad(format, _) => (
            "invalid_waveform",
            format!("wellen could not parse retained {format:?} waveform"),
        ),
    };
    WaveformFailure::new(code, diagnostic)
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

#[derive(Clone, Debug)]
struct WaveformFailure {
    code: &'static str,
    diagnostic: String,
}

impl WaveformFailure {
    fn new(code: &'static str, diagnostic: impl Into<String>) -> Self {
        Self {
            code,
            diagnostic: bounded_diagnostic(diagnostic.into()),
        }
    }

    fn invalid(diagnostic: impl Into<String>) -> Self {
        Self::new("invalid_intent", diagnostic)
    }

    fn into_value(self) -> Value {
        tagged(
            "WaveformFailed",
            BTreeMap::from([
                ("code".to_owned(), Value::Text(self.code.to_owned())),
                ("diagnostic".to_owned(), Value::Text(self.diagnostic)),
            ]),
        )
    }
}
