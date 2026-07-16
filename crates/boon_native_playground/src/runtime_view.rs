use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentNodeKind, DocumentState, LayoutDemand, StylePatch,
    StyleValue, TextValue,
};
use boon_editor::{Buffer, Command, Position};
use boon_host::{
    DocumentNodeId as HostDocumentNodeId, HostEvent, PointerButton, PointerPhase, SourceBindingId,
};
use boon_persistence::{
    ContentArtifact, ContentArtifactLoadEnqueueError, ContentArtifactLoadTicket,
    ContentArtifactRetention, ContentArtifactStoreEnqueueError, ContentArtifactStoreTicket,
    DurableContentArtifactChange, InMemoryDriver, MigrationPreview, OutboxInspectorState,
    PersistenceInspectorSnapshot, PersistenceWorkerConfig, PersistenceWorkerStatus, RedbDriver,
};
use boon_plan::{ApplicationIdentity, ApplicationPlan, MachinePlan, MemoryKind};
use boon_runtime::{
    DocumentPatch, DocumentPatchStatus, DurabilityTicket, FileEffectDriver, HostEffectRouter,
    HostEffectWorker, LiveRuntime, PersistentDispatchError, PersistentRuntime,
    PersistentRuntimeStartup, PersistentRuntimeStartupDisposition, ProgramArtifact,
    ProgramArtifactOwnership, ProgramCompletion, ProgramDiagnostic, ProgramDocumentHost,
    ProgramHostCompletion, ProgramHostDiagnostic, ProgramHostRequest, ProgramRequestId,
    ProgramSessionId, RowId, RuntimePhaseTimings, RuntimeTurn, SessionOptions, SourcePayload,
    Value,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::protocol::{
    AuthoritySelection, AuthoritySelectionKind, AuthoritySummary, DurableSummary,
    MAX_PERSISTENCE_OUTBOX_SAMPLES, MAX_PERSISTENCE_STATUS_BYTES, OutboxSample, OutboxSampleState,
    OutboxSummary, PendingSummary, PersistenceCapabilities, PersistenceCapability,
    PersistenceOperationStatus, PersistenceSnapshot, PersistenceTimingSummary,
    StateArtifactPreviewSummary, StoredSummary,
};
use crate::view::HitTarget;
type ViewResult<T> = Result<T, String>;

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const PERSISTENCE_ACK_POLL_INTERVAL: Duration = Duration::from_millis(25);
const STATE_DIRECTORY: &str = "playground/state";
pub(crate) const STATE_ROOT_ENV: &str = "BOON_PLAYGROUND_STATE_ROOT";
const EFFECT_DIRECTORY: &str = "playground/effects";
const EFFECT_POLL_INTERVAL: Duration = Duration::from_millis(1);
const HOST_LIFECYCLE_STARTED_SOURCE: &str = "host.lifecycle.started";
const MAX_PROGRAM_ARTIFACT_STORE_SESSIONS: usize = 8;
const MAX_PROGRAM_ARTIFACT_STORE_BYTES: usize = 32 * 1024 * 1024;
const MAX_PROGRAM_ARTIFACT_LOAD_SESSIONS: usize = 8;
const REPLACEABLE_ARTIFACT_QUIET_PERIOD: Duration = Duration::from_millis(34);
const PROGRAM_ARTIFACT_STARTUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostIdentityMode {
    Interactive,
    Deterministic,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AuthorityPlanCounts {
    scalar: u32,
    indexed_field: u32,
    list: u32,
    effect_contract: u32,
}

struct ScheduledSource {
    path: String,
    interval: Duration,
    next: Instant,
}

struct TextInputState {
    buffer: Buffer,
    caret_visible: bool,
    next_blink_at: Option<Instant>,
    viewport_width: Option<f32>,
    viewport_height: Option<f32>,
}

#[derive(Clone, Debug)]
struct ActivationFocusRequest {
    input_id: boon_document::TextInputId,
    position: Position,
}

impl TextInputState {
    fn new(text: &str) -> Self {
        Self {
            buffer: Buffer::new(text),
            caret_visible: true,
            next_blink_at: None,
            viewport_width: None,
            viewport_height: None,
        }
    }

    fn reset(&mut self, text: &str, position: Position) {
        self.buffer = Buffer::new(text);
        self.buffer.set_caret(position, false);
        self.reset_blink();
    }

    fn reset_blink(&mut self) {
        self.caret_visible = true;
        self.next_blink_at = Some(Instant::now() + CARET_BLINK_INTERVAL);
    }
}

#[derive(Default)]
struct InputModifiers {
    shift: bool,
    control: bool,
    alt: bool,
    meta: bool,
}

pub struct RuntimeView {
    runtime: PersistentRuntime,
    program_host: ProgramDocumentHost,
    pending_program_requests: Vec<ProgramHostRequest>,
    program_artifact_store_lane: ProgramArtifactStoreLane,
    program_artifact_load_lane: ProgramArtifactLoadLane,
    program_artifact_cache: BTreeMap<boon_persistence::ContentArtifactId, ContentArtifact>,
    application: ApplicationIdentity,
    persistence_schema_version: u64,
    persistence_schema_hash: [u8; 32],
    startup: RuntimeStartupEvidence,
    authority_plan_counts: AuthorityPlanCounts,
    authority_selections: std::collections::BTreeMap<String, AuthoritySelection>,
    persistence_status: PersistenceWorkerStatus,
    persistence_inspector: Option<PersistenceInspectorSnapshot>,
    persistence_inspector_error: Option<String>,
    next_persistence_poll: Option<Instant>,
    runtime_turn_sequence: u64,
    hovered: Option<String>,
    pressed: Option<String>,
    focused: Option<String>,
    text_inputs: std::collections::BTreeMap<String, TextInputState>,
    text_drag: Option<String>,
    modifiers: InputModifiers,
    clipboard_fallback: Option<String>,
    clipboard_system_synchronized: bool,
    scroll_offsets: std::collections::BTreeMap<String, boon_document_model::ScrollState>,
    materialization_overscan: std::collections::BTreeMap<u64, std::ops::Range<u64>>,
    pending_patches: Vec<DocumentPatch>,
    sequence: u64,
    event_dispatches: Option<Vec<RuntimeSourceDispatch>>,
    pending_external_url: Option<String>,
    last_primary_click: Option<(String, Instant)>,
    last_runtime_phase: RuntimePhaseTimings,
    scheduled_sources: Vec<ScheduledSource>,
    effect_worker: HostEffectWorker,
    next_effect_poll: Option<Instant>,
    host_identity_mode: HostIdentityMode,
    host_identity_generation: u64,
    scenario_trigger_source: Option<String>,
    scenario_trigger_turn: Option<RuntimeTurn>,
    pending_durable_lanes: BTreeMap<u64, PendingDurableLane>,
    async_lane_observations: Vec<RuntimeAsyncLaneObservation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeAsyncLaneKind {
    PersistenceTurn,
    ProgramArtifactStore,
    ProgramArtifactLoad,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeAsyncLaneOutcome {
    Applied,
    StaleRejected,
    Failed,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeAsyncLaneObservation {
    pub lane: RuntimeAsyncLaneKind,
    pub request_id: String,
    pub revision: u64,
    pub queue_depth: u32,
    pub queue_wait_us: u64,
    pub worker_us: u64,
    pub apply_us: u64,
    pub end_to_end_us: u64,
    pub outcome: RuntimeAsyncLaneOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeSourceDispatch {
    pub source_path: String,
    pub source_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeEventOutcome {
    pub changed: bool,
    pub dispatches: Vec<RuntimeSourceDispatch>,
}

impl RuntimeEventOutcome {
    pub fn dispatched(&self, source_path: &str) -> bool {
        self.dispatches
            .iter()
            .any(|dispatch| dispatch.source_path == source_path)
    }
}

#[derive(Clone, Debug)]
struct PendingDurableLane {
    queued_at: Instant,
    enqueue_us: u64,
    queue_depth: u32,
}

#[derive(Clone, Debug)]
struct PendingProgramArtifactStore {
    candidate_sequence: u64,
    mount_epoch: u64,
    session: ProgramSessionId,
    request_id: ProgramRequestId,
    artifact: ProgramArtifact,
    ownership: ProgramArtifactOwnership,
    activated_before_store: bool,
    queued_at: Instant,
    store_after: Instant,
    queue_depth: u32,
    queue_wait_us: u64,
    worker_us: u64,
}

#[derive(Debug)]
struct ProgramArtifactStoreFlight {
    ticket: ContentArtifactStoreTicket,
    mount_epoch: u64,
    artifact_id: boon_persistence::ContentArtifactId,
    waiters: BTreeMap<ProgramSessionId, PendingProgramArtifactStore>,
    started_at: Instant,
}

#[derive(Debug)]
struct PendingProgramArtifactActivation {
    ticket: DurabilityTicket,
    pending: PendingProgramArtifactStore,
}

#[derive(Debug, Default)]
struct ProgramArtifactStoreLane {
    mount_epoch: u64,
    next_candidate_sequence: u64,
    in_flight: Option<ProgramArtifactStoreFlight>,
    pending_by_session: BTreeMap<ProgramSessionId, PendingProgramArtifactStore>,
    ready_for_authority: BTreeMap<u64, PendingProgramArtifactStore>,
    awaiting_durability: BTreeMap<u64, PendingProgramArtifactActivation>,
    queued_bytes: usize,
}

impl ProgramArtifactStoreLane {
    fn reset(&mut self) {
        self.mount_epoch = self.mount_epoch.saturating_add(1);
        self.next_candidate_sequence = 0;
        self.in_flight = None;
        self.pending_by_session.clear();
        self.ready_for_authority.clear();
        self.awaiting_durability.clear();
        self.queued_bytes = 0;
    }

    fn has_pending(&self) -> bool {
        self.in_flight.is_some()
            || !self.pending_by_session.is_empty()
            || !self.ready_for_authority.is_empty()
            || !self.awaiting_durability.is_empty()
    }

    fn session_count(&self) -> usize {
        let mut sessions = self
            .pending_by_session
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(in_flight) = &self.in_flight {
            sessions.extend(in_flight.waiters.keys().cloned());
        }
        sessions.extend(
            self.ready_for_authority
                .values()
                .map(|pending| pending.session.clone()),
        );
        sessions.extend(
            self.awaiting_durability
                .values()
                .map(|activation| activation.pending.session.clone()),
        );
        sessions.len()
    }

    fn next_sequence(&mut self) -> u64 {
        self.next_candidate_sequence = self.next_candidate_sequence.saturating_add(1);
        self.next_candidate_sequence
    }

    fn remove_waiter_bytes(&mut self, waiter: &PendingProgramArtifactStore) {
        self.queued_bytes = self
            .queued_bytes
            .saturating_sub(waiter.artifact.content_bytes_len());
    }
}

#[derive(Debug)]
struct PendingProgramArtifactLoad {
    candidate_sequence: u64,
    mount_epoch: u64,
    request: ProgramHostRequest,
    queued_at: Instant,
    queue_depth: u32,
}

#[derive(Debug)]
struct ProgramArtifactLoadFlight {
    ticket: ContentArtifactLoadTicket,
    mount_epoch: u64,
    artifact_id: boon_persistence::ContentArtifactId,
    waiters: BTreeMap<ProgramSessionId, PendingProgramArtifactLoad>,
    started_at: Instant,
}

#[derive(Debug, Default)]
struct ProgramArtifactLoadLane {
    mount_epoch: u64,
    next_candidate_sequence: u64,
    in_flight: Option<ProgramArtifactLoadFlight>,
    pending_by_session: BTreeMap<ProgramSessionId, PendingProgramArtifactLoad>,
}

impl ProgramArtifactLoadLane {
    fn reset(&mut self) {
        self.mount_epoch = self.mount_epoch.saturating_add(1);
        self.next_candidate_sequence = 0;
        self.in_flight = None;
        self.pending_by_session.clear();
    }

    fn has_pending(&self) -> bool {
        self.in_flight.is_some() || !self.pending_by_session.is_empty()
    }

    fn session_count(&self) -> usize {
        let mut sessions = self
            .pending_by_session
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(in_flight) = &self.in_flight {
            sessions.extend(in_flight.waiters.keys().cloned());
        }
        sessions.len()
    }

    fn next_sequence(&mut self) -> u64 {
        self.next_candidate_sequence = self.next_candidate_sequence.saturating_add(1);
        self.next_candidate_sequence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeStartupDisposition {
    Fresh,
    Restored,
    Migrated(MigrationPreview),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeStartupEvidence {
    pub disposition: RuntimeStartupDisposition,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub durable_epoch: u64,
    pub durable_turn_sequence: u64,
}

impl From<&PersistentRuntimeStartup> for RuntimeStartupEvidence {
    fn from(startup: &PersistentRuntimeStartup) -> Self {
        Self {
            disposition: match &startup.disposition {
                PersistentRuntimeStartupDisposition::Fresh => RuntimeStartupDisposition::Fresh,
                PersistentRuntimeStartupDisposition::Restored => {
                    RuntimeStartupDisposition::Restored
                }
                PersistentRuntimeStartupDisposition::Migrated(preview) => {
                    RuntimeStartupDisposition::Migrated(preview.clone())
                }
            },
            schema_version: startup.restore_image.schema_version,
            schema_hash: startup.restore_image.schema_hash,
            durable_epoch: startup.restore_image.epoch,
            durable_turn_sequence: startup.restore_image.through_turn_sequence,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ProgramCompletionObservation {
    Host(ProgramHostCompletion),
    ArtifactStorePending {
        session: ProgramSessionId,
        request_id: ProgramRequestId,
        artifact_id: boon_persistence::ContentArtifactId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObservedProgramCompletion {
    pub changed: bool,
    pub completion: ProgramCompletionObservation,
}

pub struct RuntimePlanChange {
    pub target_schema_version: u64,
    pub durable_epoch: u64,
    pub through_turn_sequence: u64,
    pub migration: Option<MigrationPreview>,
}

impl RuntimeView {
    pub fn open(plan: Arc<MachinePlan>, deterministic: bool) -> ViewResult<Self> {
        let identity_mode = match deterministic {
            true => HostIdentityMode::Deterministic,
            false => HostIdentityMode::Interactive,
        };
        Self::open_with_state_root_and_identity_mode(plan, configured_state_root(), identity_mode)
    }

    pub fn open_for_scenario(plan: Arc<MachinePlan>) -> ViewResult<Self> {
        let (mut runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig::default(),
        )
        .map_err(|error| error.to_string())?;
        let host_identity_generation = 1;
        let host_started = dispatch_host_lifecycle_started(
            &mut runtime,
            HostIdentityMode::Deterministic,
            host_identity_generation,
            1,
        )?;
        let mount = runtime.runtime().mount();
        let startup = RuntimeStartupEvidence::from(&startup);
        Self::mount_persistent(
            runtime,
            mount,
            host_started,
            startup,
            HostIdentityMode::Deterministic,
            host_identity_generation,
        )
    }

    pub(crate) fn open_with_state_root_deterministic(
        plan: Arc<MachinePlan>,
        state_root: impl AsRef<Path>,
    ) -> ViewResult<Self> {
        Self::open_with_state_root_and_identity_mode(
            plan,
            state_root,
            HostIdentityMode::Deterministic,
        )
    }

    fn open_with_state_root_and_identity_mode(
        plan: Arc<MachinePlan>,
        state_root: impl AsRef<Path>,
        host_identity_mode: HostIdentityMode,
    ) -> ViewResult<Self> {
        validate_preview_plan(&plan)?;
        let database_path =
            state_database_path_in(state_root.as_ref(), &plan.application.identity)?;
        let parent = database_path
            .parent()
            .ok_or_else(|| "playground state database has no parent directory".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "create playground state directory `{}`: {error}",
                parent.display()
            )
        })?;
        let driver = RedbDriver::open(&database_path).map_err(|error| {
            format!(
                "open playground state database `{}`: {error}",
                database_path.display()
            )
        })?;
        let (mut runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            driver,
            PersistenceWorkerConfig::default(),
        )
        .map_err(|error| error.to_string())?;
        let host_identity_generation = 1;
        let host_started = dispatch_host_lifecycle_started(
            &mut runtime,
            host_identity_mode,
            host_identity_generation,
            1,
        )?;
        let mount = runtime.runtime().mount();
        let startup = RuntimeStartupEvidence::from(&startup);
        Self::mount_persistent(
            runtime,
            mount,
            host_started,
            startup,
            host_identity_mode,
            host_identity_generation,
        )
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory(runtime: LiveRuntime) -> ViewResult<Self> {
        Self::open_for_scenario(runtime.shared_machine_plan())
    }

    fn mount_persistent(
        runtime: PersistentRuntime,
        turn: RuntimeTurn,
        host_started: Option<RuntimeTurn>,
        startup: RuntimeStartupEvidence,
        host_identity_mode: HostIdentityMode,
        host_identity_generation: u64,
    ) -> ViewResult<Self> {
        let source_sequence = source_sequence_after_turn(
            source_sequence_after_turn(0, turn.source_sequence),
            host_started.as_ref().and_then(|turn| turn.source_sequence),
        );
        let runtime_turn_sequence = host_started
            .as_ref()
            .map_or(turn.sequence, |turn| turn.sequence);
        if turn.document_patch_status != DocumentPatchStatus::Complete {
            return Err("MachinePlan did not produce complete typed document bindings".to_owned());
        }
        let mounted = state_from_mount(turn.document_patches)?;
        let frame = runtime
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| error.to_string())?
            .clone();
        debug_assert_eq!(mounted.frame(), &frame);
        let application = runtime
            .runtime()
            .machine_plan()
            .application
            .identity
            .clone();
        let (program_host, pending_program_requests) =
            ProgramDocumentHost::mount(application.clone(), &frame);
        let frame = program_host.frame();
        let text_inputs = frame
            .nodes
            .values()
            .filter(|node| {
                node.kind == DocumentNodeKind::TextInput && !node.is_sensitive_text_input()
            })
            .map(|node| {
                (
                    node.id.0.clone(),
                    TextInputState::new(
                        node.text
                            .as_ref()
                            .map(|text| text.text.as_str())
                            .unwrap_or_default(),
                    ),
                )
            })
            .collect();
        let scheduled_sources = scheduled_sources(runtime.runtime())?;
        let plan = runtime.runtime().machine_plan();
        let persistence_schema_version = plan.persistence.schema_version;
        let persistence_schema_hash = plan.persistence.schema_hash;
        let authority_plan_counts = authority_plan_counts(plan);
        let authority_selections = authority_selections(plan);
        let (persistence_inspector, persistence_inspector_error) = match runtime.inspect() {
            Ok(inspector) => (inspector, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let persistence_status = runtime.status();
        let effect_worker = native_effect_worker()?;
        let next_effect_poll = runtime.has_effect_work().then_some(Instant::now());
        let mut view = Self {
            runtime,
            program_host,
            pending_program_requests,
            program_artifact_store_lane: ProgramArtifactStoreLane::default(),
            program_artifact_load_lane: ProgramArtifactLoadLane::default(),
            program_artifact_cache: BTreeMap::new(),
            application,
            persistence_schema_version,
            persistence_schema_hash,
            startup,
            authority_plan_counts,
            authority_selections,
            persistence_status,
            persistence_inspector,
            persistence_inspector_error,
            next_persistence_poll: host_started
                .as_ref()
                .map(|_| Instant::now() + PERSISTENCE_ACK_POLL_INTERVAL),
            runtime_turn_sequence,
            hovered: None,
            pressed: None,
            focused: None,
            text_inputs,
            text_drag: None,
            modifiers: InputModifiers::default(),
            clipboard_fallback: None,
            clipboard_system_synchronized: false,
            scroll_offsets: std::collections::BTreeMap::new(),
            materialization_overscan: std::collections::BTreeMap::new(),
            pending_patches: Vec::new(),
            sequence: source_sequence,
            event_dispatches: None,
            pending_external_url: None,
            last_primary_click: None,
            last_runtime_phase: host_started
                .map_or_else(RuntimePhaseTimings::default, |turn| turn.phase_timings),
            scheduled_sources,
            effect_worker,
            next_effect_poll,
            host_identity_mode,
            host_identity_generation,
            scenario_trigger_source: None,
            scenario_trigger_turn: None,
            pending_durable_lanes: BTreeMap::new(),
            async_lane_observations: Vec::new(),
        };
        view.resolve_program_artifact_requests_blocking()?;
        view.pending_patches.clear();
        Ok(view)
    }

    pub fn application_identity(&self) -> &ApplicationIdentity {
        &self.application
    }

    pub(crate) fn shared_machine_plan(&self) -> Arc<MachinePlan> {
        self.runtime.runtime().shared_machine_plan()
    }

    pub fn plan_schema_matches(&self, plan: &MachinePlan) -> bool {
        self.persistence_schema_version == plan.persistence.schema_version
            && self.persistence_schema_hash == plan.persistence.schema_hash
    }

    pub fn preview_machine_plan(
        &self,
        plan: Arc<MachinePlan>,
    ) -> ViewResult<boon_runtime::PersistentPlanPreview> {
        validate_preview_plan(&plan)?;
        if &plan.application.identity != self.application_identity() {
            return Err("preview plan belongs to a different application identity".to_owned());
        }
        self.runtime
            .preview_machine_plan(plan, SessionOptions::default())
            .map_err(|error| error.to_string())
    }

    pub fn activate_machine_plan(
        &mut self,
        plan: Arc<MachinePlan>,
    ) -> ViewResult<RuntimePlanChange> {
        validate_preview_plan(&plan)?;
        if &plan.application.identity != self.application_identity() {
            return Err("replacement plan belongs to a different application identity".to_owned());
        }
        let target_schema_version = plan.persistence.schema_version;
        let activation = self
            .runtime
            .activate_machine_plan(plan, SessionOptions::default())
            .map_err(|error| error.to_string())?;
        let acknowledgement = activation.acknowledgement;
        let migration = activation.migration;
        self.install_replacement_runtime(activation.mount, false)?;
        let durable_epoch = acknowledgement
            .as_ref()
            .map_or(self.persistence_status.durable_epoch, |ack| ack.epoch);
        let through_turn_sequence = acknowledgement.as_ref().map_or(
            self.persistence_status.durable_through_turn_sequence,
            |ack| ack.through_turn_sequence,
        );
        Ok(RuntimePlanChange {
            target_schema_version,
            durable_epoch,
            through_turn_sequence,
            migration,
        })
    }

    pub fn restart(&mut self) -> ViewResult<RuntimePlanChange> {
        let plan = self.runtime.runtime().shared_machine_plan();
        self.activate_machine_plan(plan)
    }

    pub fn start_over(&mut self) -> ViewResult<RuntimePlanChange> {
        let plan = self.runtime.runtime().shared_machine_plan();
        let target_schema_version = plan.persistence.schema_version;
        let reset = self
            .runtime
            .start_over_machine_plan(plan, SessionOptions::default())
            .map_err(|error| error.to_string())?;
        let durable_epoch = reset.acknowledgement.epoch;
        let through_turn_sequence = reset.acknowledgement.through_turn_sequence;
        self.install_replacement_runtime(reset.mount, true)?;
        Ok(RuntimePlanChange {
            target_schema_version,
            durable_epoch,
            through_turn_sequence,
            migration: None,
        })
    }

    fn install_runtime_mount(&mut self, mount: RuntimeTurn) -> ViewResult<()> {
        self.program_artifact_store_lane.reset();
        self.program_artifact_load_lane.reset();
        self.program_artifact_cache.clear();
        let _ = self.runtime.take_content_artifact_store_completions();
        let _ = self.runtime.take_content_artifact_load_completions();
        let runtime_turn_sequence = mount.sequence;
        if mount.document_patch_status != DocumentPatchStatus::Complete {
            return Err("MachinePlan did not produce complete typed document bindings".to_owned());
        }
        let mounted = state_from_mount(mount.document_patches)?;
        let frame = self
            .runtime
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| error.to_string())?
            .clone();
        debug_assert_eq!(mounted.frame(), &frame);

        let (program_host, pending_program_requests) =
            ProgramDocumentHost::mount(self.application.clone(), &frame);
        self.program_host = program_host;
        self.pending_program_requests = pending_program_requests;
        self.resolve_program_artifact_requests_blocking()?;
        self.pending_patches.clear();
        let frame = self.program_host.frame().clone();
        self.retain_view_state(&frame, true);
        self.materialization_overscan.clear();
        self.pending_patches.clear();
        self.last_runtime_phase = RuntimePhaseTimings::default();
        self.scheduled_sources = scheduled_sources(self.runtime.runtime())?;
        self.runtime_turn_sequence = runtime_turn_sequence;
        self.refresh_plan_metadata();
        self.refresh_persistence_after_control();
        self.schedule_effect_poll()?;
        Ok(())
    }

    fn install_replacement_runtime(
        &mut self,
        mount: RuntimeTurn,
        renew_host_identity: bool,
    ) -> ViewResult<()> {
        self.install_runtime_mount(mount)?;
        if renew_host_identity {
            self.host_identity_generation = self.host_identity_generation.saturating_add(1);
        }
        self.dispatch_host_lifecycle_started()
    }

    pub fn persistence_status(&self) -> &PersistenceWorkerStatus {
        &self.persistence_status
    }

    pub fn persistence_schema_version(&self) -> u64 {
        self.persistence_schema_version
    }

    pub(crate) fn startup_evidence(&self) -> &RuntimeStartupEvidence {
        &self.startup
    }

    pub(crate) fn parent_runtime_generation(&self) -> u64 {
        self.runtime.generation()
    }

    pub fn authority_selection_for_path(&self, path: &str) -> Option<AuthoritySelection> {
        self.authority_selections.get(path).cloned()
    }

    pub fn runtime_turn_sequence(&self) -> u64 {
        self.runtime_turn_sequence
    }

    pub fn assert_scenario_step(&mut self, step: &boon_runtime::ScenarioStep) -> ViewResult<()> {
        self.scenario_trigger_source = None;
        let turn = self.scenario_trigger_turn.take();
        self.runtime
            .assert_scenario_step(step, turn.as_ref())
            .map_err(|error| error.to_string())
    }

    pub fn begin_scenario_step(&mut self, source_path: &str) {
        self.scenario_trigger_source = Some(source_path.to_owned());
        self.scenario_trigger_turn = None;
    }

    pub fn persistence_poll_deadline(&self) -> Option<Instant> {
        self.next_persistence_poll
    }

    pub fn effect_poll_deadline(&self) -> Option<Instant> {
        self.next_effect_poll
    }

    pub fn poll_host_effects(&mut self, now: Instant) -> ViewResult<bool> {
        if self.next_effect_poll.is_none_or(|deadline| deadline > now) {
            return Ok(false);
        }
        let turn = self
            .runtime
            .poll_effect_worker(&mut self.effect_worker)
            .map_err(|error| error.to_string())?;
        let changed = turn.map_or(Ok(false), |turn| self.finish_parent_runtime_turn(turn))?;
        self.schedule_effect_poll()?;
        Ok(changed)
    }

    pub fn poll_persistence_acknowledgement(&mut self, now: Instant) -> bool {
        if self
            .next_persistence_poll
            .is_none_or(|deadline| deadline > now)
        {
            return false;
        }
        let previous_status = self.persistence_status.clone();
        let apply_started = Instant::now();
        self.persistence_status = self.query_persistence_status();
        let apply_us = duration_us(apply_started.elapsed());
        self.record_durable_lane_completions(now, apply_us);
        let mut changed = self.persistence_status != previous_status;
        let idle = self.persistence_status.pending.is_none()
            && self.persistence_status.queue_depth == 0
            && self.persistence_status.reserved_slots == 0
            && self.persistence_status.pending_content_artifact_stores == 0
            && self.persistence_status.pending_content_artifact_loads == 0
            && !self.program_artifact_store_lane.has_pending()
            && !self.program_artifact_load_lane.has_pending();
        let inspector_is_stale = self.persistence_inspector.as_ref().is_none_or(|inspector| {
            inspector.epoch < self.persistence_status.durable_epoch
                || inspector.through_turn_sequence
                    < self.persistence_status.durable_through_turn_sequence
        });
        if idle && inspector_is_stale {
            changed |= self.refresh_persistence_inspector();
        }
        self.next_persistence_poll = (!idle).then_some(now + PERSISTENCE_ACK_POLL_INTERVAL);
        changed
    }

    pub fn cached_persistence_snapshot(
        &self,
        snapshot_sequence: u64,
        revision: u64,
        last_operation: Option<PersistenceOperationStatus>,
        import_preview: Option<StateArtifactPreviewSummary>,
    ) -> PersistenceSnapshot {
        let pending = self.persistence_status.pending.as_ref();
        let first_turn_sequence = pending.map(|pending| pending.first_turn_sequence);
        let last_turn_sequence = pending.map(|pending| pending.last_turn_sequence);
        let turn_count = pending.map_or(0, |pending| {
            pending
                .last_turn_sequence
                .saturating_sub(pending.first_turn_sequence)
                .saturating_add(1)
        });
        let inspector = self.persistence_inspector.as_ref();
        let stored = inspector.map(|inspector| StoredSummary {
            epoch: inspector.epoch,
            through_turn_sequence: inspector.through_turn_sequence,
            scalar_count: saturating_u32(inspector.scalar_count),
            list_count: saturating_u32(inspector.list_count),
            row_count: inspector.row_count.try_into().unwrap_or(u64::MAX),
            content_artifact_count: saturating_u32(inspector.content_artifact_count),
            content_artifact_bytes: inspector.content_artifact_bytes,
            encoded_value_bytes: inspector.encoded_value_bytes,
            completed_migration_count: saturating_u32(inspector.completed_migration_count),
        });
        let outbox = inspector.map_or_else(
            || OutboxSummary {
                pending_count: 0,
                dispatching_count: 0,
                reconciliation_count: 0,
                completed_count: 0,
                samples: Vec::new(),
            },
            |inspector| OutboxSummary {
                pending_count: saturating_u32(inspector.outbox_pending_count),
                dispatching_count: saturating_u32(inspector.outbox_dispatching_count),
                reconciliation_count: saturating_u32(inspector.outbox_reconciliation_count),
                completed_count: saturating_u32(inspector.outbox_completed_count),
                samples: inspector
                    .outbox_samples
                    .iter()
                    .take(MAX_PERSISTENCE_OUTBOX_SAMPLES)
                    .map(|sample| OutboxSample {
                        item_id: *sample.item_id.as_bytes(),
                        invocation_id: *sample.invocation_id.as_bytes(),
                        effect_id: *sample.effect_id.as_bytes(),
                        state: match sample.state {
                            OutboxInspectorState::Pending => OutboxSampleState::Pending,
                            OutboxInspectorState::Dispatching => OutboxSampleState::Dispatching,
                            OutboxInspectorState::ReconciliationRequired => {
                                OutboxSampleState::ReconciliationRequired
                            }
                            OutboxInspectorState::Completed => OutboxSampleState::Completed,
                        },
                        attempt: sample.attempt,
                        created_turn_sequence: sample.created_turn_sequence,
                        updated_turn_sequence: sample.updated_turn_sequence,
                    })
                    .collect(),
            },
        );
        let operation_error = last_operation
            .as_ref()
            .filter(|operation| !operation.ok)
            .map(|operation| operation.message.as_str());
        let worker_error = self
            .persistence_status
            .last_error
            .as_ref()
            .map(ToString::to_string);
        let last_actionable_error = operation_error
            .or(self.persistence_inspector_error.as_deref())
            .map(str::to_owned)
            .or(worker_error)
            .map(|message| bounded_persistence_text(&message));

        PersistenceSnapshot {
            snapshot_sequence,
            revision,
            application: self.application.clone(),
            schema_version: self.persistence_schema_version,
            schema_hash: self.persistence_schema_hash,
            authority: AuthoritySummary {
                runtime_turn_sequence: self.runtime_turn_sequence,
                source_event_sequence: self.sequence,
                scalar_count: self.authority_plan_counts.scalar,
                indexed_field_count: self.authority_plan_counts.indexed_field,
                list_count: self.authority_plan_counts.list,
                effect_contract_count: self.authority_plan_counts.effect_contract,
            },
            stored,
            pending: PendingSummary {
                first_turn_sequence,
                last_turn_sequence,
                oldest_age_millis: pending.map_or(0, |pending| {
                    pending.age.as_millis().try_into().unwrap_or(u64::MAX)
                }),
                turn_count,
                queue_depth: saturating_u32(self.persistence_status.queue_depth),
                reserved_slots: saturating_u32(self.persistence_status.reserved_slots),
                accepting_turns: self.persistence_status.accepting_turns,
            },
            durable: DurableSummary {
                epoch: self.persistence_status.durable_epoch,
                through_turn_sequence: self.persistence_status.durable_through_turn_sequence,
            },
            timings: PersistenceTimingSummary {
                authority_enqueue_us: self
                    .last_runtime_phase
                    .persistence_enqueue_us
                    .max(self.persistence_status.timings.authority_enqueue_us),
                encode_us: self.persistence_status.timings.encode_us,
                checkpoint_us: self.persistence_status.timings.checkpoint_us,
                barrier_us: self.persistence_status.timings.barrier_us,
                restore_us: self.persistence_status.timings.restore_us,
                migration_us: self.persistence_status.timings.migration_us,
                rebuild_derived_us: self.runtime.last_rebuild_derived_us(),
            },
            outbox,
            worker_alive: self.persistence_status.worker_alive,
            capabilities: PersistenceCapabilities {
                clear_selected: available_capability(),
                export_state: available_capability(),
                import_preview: available_capability(),
                activate_import: available_capability(),
            },
            import_preview,
            last_actionable_error,
            last_operation: last_operation.map(|mut operation| {
                operation.message = bounded_persistence_text(&operation.message);
                operation
            }),
        }
    }

    pub fn flush_persistence(&mut self) -> ViewResult<(u64, u64)> {
        let acknowledgement = self.runtime.flush().map_err(|error| error.to_string())?;
        self.refresh_persistence_after_control();
        Ok((
            acknowledgement.epoch,
            self.persistence_status.durable_through_turn_sequence,
        ))
    }

    pub fn compact_persistence(&mut self) -> ViewResult<u64> {
        let acknowledgement = self.runtime.compact().map_err(|error| error.to_string())?;
        self.refresh_persistence_after_control();
        Ok(acknowledgement.epoch)
    }

    pub fn export_state_artifact(&mut self) -> ViewResult<Vec<u8>> {
        let artifact = self
            .runtime
            .export_state_artifact()
            .map_err(|error| error.to_string())?;
        self.refresh_persistence_after_control();
        Ok(artifact)
    }

    pub fn preview_state_artifact(
        &mut self,
        artifact: &[u8],
    ) -> ViewResult<boon_runtime::PersistentStateArtifactPreview> {
        let preview = self
            .runtime
            .preview_state_artifact(artifact, SessionOptions::default())
            .map_err(|error| error.to_string())?;
        self.refresh_persistence_after_control();
        Ok(preview)
    }

    pub fn activate_state_artifact(
        &mut self,
        artifact: &[u8],
    ) -> ViewResult<(boon_runtime::PersistentStateArtifactPreview, u64)> {
        let activation = self
            .runtime
            .activate_state_artifact(artifact, SessionOptions::default())
            .map_err(|error| error.to_string())?;
        let epoch = activation.acknowledgement.epoch;
        let preview = activation.preview;
        self.install_replacement_runtime(activation.mount, false)?;
        Ok((preview, epoch))
    }

    pub fn clear_authority_path(&mut self, semantic_path: &str) -> ViewResult<(u64, u64)> {
        let activation = self
            .runtime
            .clear_authority_path(semantic_path, SessionOptions::default())
            .map_err(|error| error.to_string())?;
        let acknowledgement = activation
            .acknowledgement
            .ok_or_else(|| "clear authority did not produce a durable activation".to_owned())?;
        let epoch = acknowledgement.epoch;
        let turn = acknowledgement.through_turn_sequence;
        self.install_replacement_runtime(activation.mount, false)?;
        Ok((epoch, turn))
    }

    fn refresh_plan_metadata(&mut self) {
        let plan = self.runtime.runtime().machine_plan();
        self.application = plan.application.identity.clone();
        self.persistence_schema_version = plan.persistence.schema_version;
        self.persistence_schema_hash = plan.persistence.schema_hash;
        self.authority_plan_counts = authority_plan_counts(plan);
        self.authority_selections = authority_selections(plan);
    }

    fn refresh_persistence_after_control(&mut self) {
        let apply_started = Instant::now();
        self.persistence_status = self.query_persistence_status();
        self.refresh_persistence_inspector();
        self.persistence_status = self.query_persistence_status();
        self.record_durable_lane_completions(Instant::now(), duration_us(apply_started.elapsed()));
        self.next_persistence_poll = None;
    }

    fn refresh_persistence_inspector(&mut self) -> bool {
        let previous = self.persistence_inspector.clone();
        let previous_error = self.persistence_inspector_error.clone();
        match self.runtime.inspect() {
            Ok(inspector) => {
                self.persistence_inspector = inspector;
                self.persistence_inspector_error = None;
            }
            Err(error) => {
                self.persistence_inspector_error = Some(error.to_string());
            }
        }
        self.persistence_inspector != previous || self.persistence_inspector_error != previous_error
    }

    fn query_persistence_status(&mut self) -> PersistenceWorkerStatus {
        self.runtime.status()
    }

    fn retain_view_state(&mut self, frame: &DocumentFrame, reset_input: bool) {
        let contains_node = |id: &str| frame.nodes.contains_key(&DocumentNodeId(id.to_owned()));
        self.hovered = self.hovered.take().filter(|id| contains_node(id));
        self.pressed = self.pressed.take().filter(|id| contains_node(id));
        self.focused = self.focused.take().filter(|id| contains_node(id));
        self.text_drag = self.text_drag.take().filter(|id| contains_node(id));
        self.scroll_offsets.retain(|id, _| contains_node(id));
        self.last_primary_click = self
            .last_primary_click
            .take()
            .filter(|(id, _)| contains_node(id));

        let mut previous = std::mem::take(&mut self.text_inputs);
        self.text_inputs = frame
            .nodes
            .values()
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .map(|node| {
                let text = node
                    .text
                    .as_ref()
                    .map(|text| text.text.as_str())
                    .unwrap_or_default();
                let state = previous.remove(&node.id.0).map_or_else(
                    || TextInputState::new(text),
                    |mut state| {
                        if state.buffer.text() != text {
                            let position = state.buffer.caret();
                            state.reset(text, position);
                        }
                        state
                    },
                );
                (node.id.0.clone(), state)
            })
            .collect();
        if reset_input {
            self.modifiers = InputModifiers::default();
            self.pending_external_url = None;
        }
    }

    pub fn frame(&self) -> DocumentFrame {
        let mut frame = self.retained_frame().clone();
        for (id, scroll) in &self.scroll_offsets {
            if let Some(node) = frame.nodes.get_mut(&DocumentNodeId(id.clone())) {
                node.scroll = Some(*scroll);
            }
        }
        frame
    }

    fn retained_frame(&self) -> &DocumentFrame {
        self.program_host.frame()
    }

    fn resolve_program_artifact_requests_blocking(&mut self) -> ViewResult<bool> {
        let started = Instant::now();
        let mut changed = self.resolve_program_artifact_requests()?;
        loop {
            changed |= self.poll_program_artifact_loads()?;
            let queued_artifact_request = self
                .pending_program_requests
                .iter()
                .any(ProgramHostRequest::is_artifact_load);
            if !queued_artifact_request && !self.program_artifact_load_lane.has_pending() {
                return Ok(changed);
            }
            if started.elapsed() >= PROGRAM_ARTIFACT_STARTUP_TIMEOUT {
                let persistence = self.runtime.status();
                return Err(format!(
                    "program artifact startup currentness barrier exceeded {} ms; queued_requests={}, load_sessions={}, persistence_loads={}, worker_alive={}, last_error={:?}",
                    PROGRAM_ARTIFACT_STARTUP_TIMEOUT.as_millis(),
                    usize::from(queued_artifact_request),
                    self.program_artifact_load_lane.session_count(),
                    persistence.pending_content_artifact_loads,
                    persistence.worker_alive,
                    persistence.last_error,
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn resolve_program_artifact_requests(&mut self) -> ViewResult<bool> {
        let mut compile_requests = Vec::new();
        let mut changed = false;
        loop {
            let requests = std::mem::take(&mut self.pending_program_requests);
            if requests.is_empty() {
                break;
            }
            for request in requests {
                let Some(artifact_id) = request.artifact_id else {
                    compile_requests.push(request);
                    continue;
                };
                if let Some(content) = self.program_artifact_cache.get(&artifact_id).cloned() {
                    let result = ProgramArtifact::from_content_artifact(
                        request.compile.revision,
                        request.compile.capability_profile,
                        content,
                    );
                    changed |= self
                        .complete_program_observed(&request.session, &request.request_id, result)?
                        .changed;
                } else {
                    changed |= self.enqueue_program_artifact_load(request)?;
                }
            }
        }
        self.pending_program_requests = compile_requests;
        Ok(changed)
    }

    fn enqueue_program_artifact_load(&mut self, request: ProgramHostRequest) -> ViewResult<bool> {
        let artifact_id = request
            .artifact_id
            .expect("artifact load request carries an artifact identity");
        let session_exists = self
            .program_artifact_load_lane
            .pending_by_session
            .contains_key(&request.session)
            || self
                .program_artifact_load_lane
                .in_flight
                .as_ref()
                .is_some_and(|flight| flight.waiters.contains_key(&request.session));
        if !session_exists
            && self.program_artifact_load_lane.session_count() >= MAX_PROGRAM_ARTIFACT_LOAD_SESSIONS
        {
            return Ok(self
                .complete_program_observed(
                    &request.session,
                    &request.request_id,
                    Err(ProgramDiagnostic::artifact(
                        request.compile.revision,
                        format!(
                            "program artifact load has {} pending sessions, limit is {MAX_PROGRAM_ARTIFACT_LOAD_SESSIONS}",
                            self.program_artifact_load_lane.session_count()
                        ),
                    )),
                )?
                .changed);
        }

        let pending = PendingProgramArtifactLoad {
            candidate_sequence: self.program_artifact_load_lane.next_sequence(),
            mount_epoch: self.program_artifact_load_lane.mount_epoch,
            queue_depth: self
                .program_artifact_load_lane
                .session_count()
                .saturating_add(1)
                .try_into()
                .unwrap_or(u32::MAX),
            queued_at: Instant::now(),
            request,
        };
        let joins_in_flight = self
            .program_artifact_load_lane
            .in_flight
            .as_ref()
            .is_some_and(|flight| {
                flight.mount_epoch == pending.mount_epoch && flight.artifact_id == artifact_id
            });
        if joins_in_flight {
            self.program_artifact_load_lane
                .pending_by_session
                .remove(&pending.request.session);
            self.program_artifact_load_lane
                .in_flight
                .as_mut()
                .expect("joining artifact load has an active flight")
                .waiters
                .insert(pending.request.session.clone(), pending);
        } else {
            self.program_artifact_load_lane
                .pending_by_session
                .insert(pending.request.session.clone(), pending);
        }
        let changed = self.drive_program_artifact_load_lane()?;
        self.next_persistence_poll = Some(Instant::now());
        Ok(changed)
    }

    fn drive_program_artifact_load_lane(&mut self) -> ViewResult<bool> {
        if self.program_artifact_load_lane.in_flight.is_some() {
            return Ok(false);
        }
        let Some(session) = self
            .program_artifact_load_lane
            .pending_by_session
            .iter()
            .min_by_key(|(_, candidate)| candidate.candidate_sequence)
            .map(|(session, _)| session.clone())
        else {
            return Ok(false);
        };
        let first = self
            .program_artifact_load_lane
            .pending_by_session
            .remove(&session)
            .expect("selected artifact load candidate exists");
        let artifact_id = first
            .request
            .artifact_id
            .expect("artifact load candidate carries an identity");
        let matching_sessions = self
            .program_artifact_load_lane
            .pending_by_session
            .iter()
            .filter_map(|(session, candidate)| {
                (candidate.request.artifact_id == Some(artifact_id)).then_some(session.clone())
            })
            .collect::<Vec<_>>();
        let mut waiters = BTreeMap::from([(first.request.session.clone(), first)]);
        for session in matching_sessions {
            let candidate = self
                .program_artifact_load_lane
                .pending_by_session
                .remove(&session)
                .expect("matching artifact load candidate exists");
            waiters.insert(session, candidate);
        }
        match self.runtime.try_load_content_artifact(artifact_id) {
            Ok(ticket) => {
                self.program_artifact_load_lane.in_flight = Some(ProgramArtifactLoadFlight {
                    ticket,
                    mount_epoch: self.program_artifact_load_lane.mount_epoch,
                    artifact_id,
                    waiters,
                    started_at: Instant::now(),
                });
                Ok(false)
            }
            Err(ContentArtifactLoadEnqueueError::Backpressure(_)) => {
                self.program_artifact_load_lane.pending_by_session.extend(
                    waiters
                        .into_values()
                        .map(|candidate| (candidate.request.session.clone(), candidate)),
                );
                Ok(false)
            }
            Err(ContentArtifactLoadEnqueueError::Closed(_)) => {
                let mut changed = false;
                for (_, pending) in waiters {
                    changed |= self
                        .complete_program_observed(
                            &pending.request.session,
                            &pending.request.request_id,
                            Err(ProgramDiagnostic::artifact(
                                pending.request.compile.revision,
                                "persistence coordinator closed before loading the program artifact",
                            )),
                        )?
                        .changed;
                }
                Ok(changed)
            }
        }
    }

    fn poll_program_artifact_loads(&mut self) -> ViewResult<bool> {
        let mut changed = false;
        for completion in self.runtime.take_content_artifact_load_completions() {
            if self
                .program_artifact_load_lane
                .in_flight
                .as_ref()
                .is_none_or(|flight| flight.ticket != completion.ticket)
            {
                continue;
            }
            let flight = self
                .program_artifact_load_lane
                .in_flight
                .take()
                .expect("matching artifact load flight exists");
            if flight.mount_epoch != self.program_artifact_load_lane.mount_epoch
                || completion.id != flight.artifact_id
            {
                for pending in flight.waiters.into_values() {
                    self.async_lane_observations
                        .push(RuntimeAsyncLaneObservation {
                            lane: RuntimeAsyncLaneKind::ProgramArtifactLoad,
                            request_id: pending.request.request_id.0,
                            revision: pending.request.compile.revision,
                            queue_depth: pending.queue_depth,
                            queue_wait_us: duration_us(
                                flight
                                    .started_at
                                    .saturating_duration_since(pending.queued_at),
                            ),
                            worker_us: duration_us(flight.started_at.elapsed()),
                            apply_us: 0,
                            end_to_end_us: duration_us(pending.queued_at.elapsed()),
                            outcome: RuntimeAsyncLaneOutcome::StaleRejected,
                        });
                }
                continue;
            }
            if let Ok(Some(content)) = &completion.result {
                self.program_artifact_cache
                    .insert(content.id, content.clone());
            }
            for (_, pending) in flight.waiters {
                let apply_started = Instant::now();
                let result = match &completion.result {
                    Ok(Some(content)) => ProgramArtifact::from_content_artifact(
                        pending.request.compile.revision,
                        pending.request.compile.capability_profile,
                        content.clone(),
                    ),
                    Ok(None) => Err(ProgramDiagnostic::artifact(
                        pending.request.compile.revision,
                        format!(
                            "immutable program artifact {} is missing",
                            flight.artifact_id
                        ),
                    )),
                    Err(error) => Err(ProgramDiagnostic::artifact(
                        pending.request.compile.revision,
                        error.to_string(),
                    )),
                };
                let failed = result.is_err();
                let observed = self.complete_program_observed(
                    &pending.request.session,
                    &pending.request.request_id,
                    result,
                )?;
                changed |= observed.changed;
                self.async_lane_observations
                    .push(RuntimeAsyncLaneObservation {
                        lane: RuntimeAsyncLaneKind::ProgramArtifactLoad,
                        request_id: pending.request.request_id.0,
                        revision: pending.request.compile.revision,
                        queue_depth: pending.queue_depth,
                        queue_wait_us: duration_us(
                            flight
                                .started_at
                                .saturating_duration_since(pending.queued_at),
                        ),
                        worker_us: duration_us(flight.started_at.elapsed()),
                        apply_us: duration_us(apply_started.elapsed()),
                        end_to_end_us: duration_us(pending.queued_at.elapsed()),
                        outcome: runtime_lane_outcome(&observed.completion, failed),
                    });
            }
        }
        changed |= self.drive_program_artifact_load_lane()?;
        if changed {
            changed |= self.resolve_program_artifact_requests()?;
        }
        Ok(changed)
    }

    pub fn take_program_requests(&mut self) -> Vec<ProgramHostRequest> {
        std::mem::take(&mut self.pending_program_requests)
    }

    pub(crate) fn program_artifact_lane_counts(&self) -> (usize, usize) {
        (
            self.program_artifact_store_lane.session_count(),
            self.program_artifact_load_lane.session_count(),
        )
    }

    pub(crate) fn take_async_lane_observations(&mut self) -> Vec<RuntimeAsyncLaneObservation> {
        std::mem::take(&mut self.async_lane_observations)
    }

    pub fn complete_program(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> ViewResult<bool> {
        self.complete_program_observed(session, request_id, result)
            .map(|outcome| outcome.changed)
    }

    pub(crate) fn complete_program_observed(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> ViewResult<ObservedProgramCompletion> {
        let request_is_current = self.program_host.request_is_current(session, request_id);
        let artifact_ownership = self
            .program_host
            .request_artifact_ownership(session, request_id);
        let artifact_load = self
            .program_host
            .request_is_artifact_load(session, request_id);
        if request_is_current && let Some(ownership) = artifact_ownership {
            return match result {
                Ok(artifact) => {
                    let activate_before_store =
                        ownership.retention == ContentArtifactRetention::Replaceable;
                    let pending = self.pending_program_artifact_store(
                        session,
                        request_id,
                        artifact.clone(),
                        ownership,
                        activate_before_store,
                    );
                    if !activate_before_store {
                        return self.enqueue_program_artifact_store(pending);
                    }

                    let activated = self.finish_program_completion_observed(
                        session,
                        request_id,
                        Ok(artifact),
                        false,
                        true,
                    )?;
                    if !matches!(
                        activated.completion,
                        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
                            ProgramCompletion::Activated { .. }
                        ))
                    ) {
                        return Ok(activated);
                    }
                    let queued = self.enqueue_program_artifact_store(pending)?;
                    if matches!(
                        queued.completion,
                        ProgramCompletionObservation::ArtifactStorePending { .. }
                    ) {
                        Ok(ObservedProgramCompletion {
                            changed: activated.changed || queued.changed,
                            completion: activated.completion,
                        })
                    } else {
                        Ok(ObservedProgramCompletion {
                            changed: activated.changed || queued.changed,
                            completion: queued.completion,
                        })
                    }
                }
                Err(diagnostic) => self.finish_program_completion_observed(
                    session,
                    request_id,
                    Err(diagnostic),
                    false,
                    false,
                ),
            };
        }
        self.finish_program_completion_observed(session, request_id, result, artifact_load, false)
    }

    fn pending_program_artifact_store(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        artifact: ProgramArtifact,
        ownership: ProgramArtifactOwnership,
        activated_before_store: bool,
    ) -> PendingProgramArtifactStore {
        let queued_at = Instant::now();
        let store_after = if ownership.retention == ContentArtifactRetention::Replaceable {
            queued_at + REPLACEABLE_ARTIFACT_QUIET_PERIOD
        } else {
            queued_at
        };
        PendingProgramArtifactStore {
            candidate_sequence: self.program_artifact_store_lane.next_sequence(),
            mount_epoch: self.program_artifact_store_lane.mount_epoch,
            session: session.clone(),
            request_id: request_id.clone(),
            artifact,
            ownership,
            activated_before_store,
            queued_at,
            store_after,
            queue_depth: self
                .program_artifact_store_lane
                .session_count()
                .saturating_add(1)
                .try_into()
                .unwrap_or(u32::MAX),
            queue_wait_us: 0,
            worker_us: 0,
        }
    }

    fn enqueue_program_artifact_store(
        &mut self,
        pending: PendingProgramArtifactStore,
    ) -> ViewResult<ObservedProgramCompletion> {
        let session_exists = self
            .program_artifact_store_lane
            .pending_by_session
            .contains_key(&pending.session)
            || self
                .program_artifact_store_lane
                .in_flight
                .as_ref()
                .is_some_and(|flight| flight.waiters.contains_key(&pending.session))
            || self
                .program_artifact_store_lane
                .ready_for_authority
                .values()
                .any(|candidate| candidate.session == pending.session)
            || self
                .program_artifact_store_lane
                .awaiting_durability
                .values()
                .any(|activation| activation.pending.session == pending.session);
        if !session_exists
            && self.program_artifact_store_lane.session_count()
                >= MAX_PROGRAM_ARTIFACT_STORE_SESSIONS
        {
            return self.finish_program_completion_observed(
                &pending.session,
                &pending.request_id,
                Err(ProgramDiagnostic::artifact(
                    pending.artifact.revision(),
                    format!(
                        "program artifact store has {} pending sessions, limit is {MAX_PROGRAM_ARTIFACT_STORE_SESSIONS}",
                        self.program_artifact_store_lane.session_count()
                    ),
                )),
                false,
                false,
            );
        }

        let artifact_id = pending.artifact.id();
        let joins_in_flight = self
            .program_artifact_store_lane
            .in_flight
            .as_ref()
            .is_some_and(|flight| {
                flight.mount_epoch == pending.mount_epoch && flight.artifact_id == artifact_id
            });
        let replaced_pending_bytes = self
            .program_artifact_store_lane
            .pending_by_session
            .get(&pending.session)
            .map_or(0, |replaced| replaced.artifact.content_bytes_len());
        let replaced_flight_bytes = joins_in_flight
            .then(|| {
                self.program_artifact_store_lane
                    .in_flight
                    .as_ref()?
                    .waiters
                    .get(&pending.session)
                    .map(|replaced| replaced.artifact.content_bytes_len())
            })
            .flatten()
            .unwrap_or(0);
        let projected_bytes = self
            .program_artifact_store_lane
            .queued_bytes
            .saturating_sub(replaced_pending_bytes)
            .saturating_sub(replaced_flight_bytes)
            .saturating_add(pending.artifact.content_bytes_len());
        if projected_bytes > MAX_PROGRAM_ARTIFACT_STORE_BYTES {
            return self.finish_program_completion_observed(
                &pending.session,
                &pending.request_id,
                Err(ProgramDiagnostic::artifact(
                    pending.artifact.revision(),
                    format!(
                        "program artifact store would retain {projected_bytes} queued bytes, limit is {MAX_PROGRAM_ARTIFACT_STORE_BYTES}"
                    ),
                )),
                false,
                false,
            );
        }

        let completion = ProgramCompletionObservation::ArtifactStorePending {
            session: pending.session.clone(),
            request_id: pending.request_id.clone(),
            artifact_id,
        };

        if joins_in_flight {
            self.program_artifact_store_lane
                .pending_by_session
                .remove(&pending.session);
            let in_flight = self
                .program_artifact_store_lane
                .in_flight
                .as_mut()
                .expect("joining candidate has an artifact flight");
            in_flight.waiters.insert(pending.session.clone(), pending);
            self.program_artifact_store_lane.queued_bytes = projected_bytes;
        } else {
            self.program_artifact_store_lane
                .pending_by_session
                .insert(pending.session.clone(), pending);
            self.program_artifact_store_lane.queued_bytes = projected_bytes;
        }

        let changed = self.drive_program_artifact_store_lane()?;
        self.next_persistence_poll = Some(Instant::now());
        Ok(ObservedProgramCompletion {
            changed,
            completion,
        })
    }

    fn drive_program_artifact_store_lane(&mut self) -> ViewResult<bool> {
        if self.program_artifact_store_lane.in_flight.is_some()
            || !self
                .program_artifact_store_lane
                .ready_for_authority
                .is_empty()
            || !self
                .program_artifact_store_lane
                .awaiting_durability
                .is_empty()
        {
            return Ok(false);
        }
        let now = Instant::now();
        let Some(session) = self
            .program_artifact_store_lane
            .pending_by_session
            .iter()
            .filter(|(_, candidate)| candidate.store_after <= now)
            .min_by_key(|(_, candidate)| candidate.candidate_sequence)
            .map(|(session, _)| session.clone())
        else {
            return Ok(false);
        };
        let first = self
            .program_artifact_store_lane
            .pending_by_session
            .remove(&session)
            .expect("selected artifact candidate exists");
        let artifact_id = first.artifact.id();
        let matching_sessions = self
            .program_artifact_store_lane
            .pending_by_session
            .iter()
            .filter_map(|(session, candidate)| {
                (candidate.artifact.id() == artifact_id).then_some(session.clone())
            })
            .collect::<Vec<_>>();
        let mut waiters = BTreeMap::from([(first.session.clone(), first)]);
        for session in matching_sessions {
            let candidate = self
                .program_artifact_store_lane
                .pending_by_session
                .remove(&session)
                .expect("matching artifact candidate exists");
            waiters.insert(session, candidate);
        }
        let content = waiters
            .values()
            .next()
            .expect("artifact flight has at least one waiter")
            .artifact
            .to_content_artifact();
        match self.runtime.try_put_content_artifact(content) {
            Ok(ticket) => {
                self.program_artifact_store_lane.in_flight = Some(ProgramArtifactStoreFlight {
                    ticket,
                    mount_epoch: self.program_artifact_store_lane.mount_epoch,
                    artifact_id,
                    waiters,
                    started_at: Instant::now(),
                });
                Ok(false)
            }
            Err(ContentArtifactStoreEnqueueError::Backpressure(_)) => {
                self.program_artifact_store_lane
                    .pending_by_session
                    .extend(waiters);
                Ok(false)
            }
            Err(ContentArtifactStoreEnqueueError::Closed(_)) => {
                let mut changed = false;
                for (_, waiter) in waiters {
                    self.program_artifact_store_lane
                        .remove_waiter_bytes(&waiter);
                    changed |= self
                        .finish_program_completion_observed(
                            &waiter.session,
                            &waiter.request_id,
                            Err(ProgramDiagnostic::artifact(
                                waiter.artifact.revision(),
                                "persistence coordinator closed before storing the program artifact",
                            )),
                            false,
                            false,
                        )?
                        .changed;
                }
                Ok(changed)
            }
        }
    }

    pub fn poll_program_artifact_stores(&mut self) -> ViewResult<bool> {
        let mut changed = self.poll_program_artifact_loads()?;
        changed |= self.poll_program_artifact_durability()?;
        changed |= self.drive_program_artifact_authority_lane()?;
        for completion in self.runtime.take_content_artifact_store_completions() {
            if self
                .program_artifact_store_lane
                .in_flight
                .as_ref()
                .is_none_or(|flight| flight.ticket != completion.ticket)
            {
                continue;
            }
            let flight = self
                .program_artifact_store_lane
                .in_flight
                .take()
                .expect("matching artifact flight exists");
            let acknowledged = completion.result.and_then(|ack| {
                (ack.id == flight.artifact_id).then_some(()).ok_or_else(|| {
                    boon_persistence::StoreError::InvalidContentArtifact(
                        "persistence acknowledged a different program artifact".to_owned(),
                    )
                })
            });
            let store_worker_us = duration_us(flight.started_at.elapsed());
            if acknowledged.is_ok()
                && flight.mount_epoch == self.program_artifact_store_lane.mount_epoch
                && let Some(artifact) = flight.waiters.values().next()
            {
                self.program_artifact_cache
                    .insert(flight.artifact_id, artifact.artifact.to_content_artifact());
            }
            for (_, mut waiter) in flight.waiters {
                waiter.queue_wait_us = duration_us(
                    flight
                        .started_at
                        .saturating_duration_since(waiter.queued_at),
                );
                waiter.worker_us = store_worker_us;
                if flight.mount_epoch != self.program_artifact_store_lane.mount_epoch {
                    self.program_artifact_store_lane
                        .remove_waiter_bytes(&waiter);
                    let revision = waiter.artifact.revision();
                    self.async_lane_observations
                        .push(RuntimeAsyncLaneObservation {
                            lane: RuntimeAsyncLaneKind::ProgramArtifactStore,
                            request_id: waiter.request_id.0,
                            revision,
                            queue_depth: waiter.queue_depth,
                            queue_wait_us: waiter.queue_wait_us,
                            worker_us: waiter.worker_us,
                            apply_us: 0,
                            end_to_end_us: duration_us(waiter.queued_at.elapsed()),
                            outcome: RuntimeAsyncLaneOutcome::StaleRejected,
                        });
                    continue;
                }
                match &acknowledged {
                    Ok(()) => {
                        self.program_artifact_store_lane
                            .ready_for_authority
                            .insert(waiter.candidate_sequence, waiter);
                    }
                    Err(error) => {
                        self.program_artifact_store_lane
                            .remove_waiter_bytes(&waiter);
                        let diagnostic = ProgramDiagnostic::artifact(
                            waiter.artifact.revision(),
                            error.to_string(),
                        );
                        changed |= self
                            .finish_program_artifact_store_observed(waiter, Err(diagnostic), false)?
                            .changed;
                    }
                }
            }
        }
        changed |= self.drive_program_artifact_authority_lane()?;
        changed |= self.poll_program_artifact_durability()?;
        changed |= self.drive_program_artifact_store_lane()?;
        if changed {
            changed |= self.resolve_program_artifact_requests()?;
        }
        if self.program_artifact_store_lane.has_pending()
            || self.program_artifact_load_lane.has_pending()
        {
            self.next_persistence_poll = Some(Instant::now() + PERSISTENCE_ACK_POLL_INTERVAL);
        }
        Ok(changed)
    }

    fn drive_program_artifact_authority_lane(&mut self) -> ViewResult<bool> {
        let mut changed = false;
        loop {
            let Some(candidate_sequence) = self
                .program_artifact_store_lane
                .ready_for_authority
                .first_key_value()
                .map(|(sequence, _)| *sequence)
            else {
                return Ok(changed);
            };
            let pending = self
                .program_artifact_store_lane
                .ready_for_authority
                .remove(&candidate_sequence)
                .expect("selected durable artifact candidate exists");
            if !self
                .program_host
                .request_is_current(&pending.session, &pending.request_id)
            {
                self.program_artifact_store_lane
                    .remove_waiter_bytes(&pending);
                changed |= self
                    .finish_program_artifact_store_observed(pending, Ok(()), false)?
                    .changed;
                continue;
            }

            let paths = self
                .program_host
                .lifecycle_source_paths(&pending.session, "compiled");
            if paths.len() != 1 {
                self.program_artifact_store_lane
                    .remove_waiter_bytes(&pending);
                changed |= self
                    .finish_program_completion_observed(
                        &pending.session,
                        &pending.request_id,
                        Err(ProgramDiagnostic::artifact(
                            pending.artifact.revision(),
                            format!(
                                "retained embedded program requires exactly one compiled lifecycle route, found {}",
                                paths.len()
                            ),
                        )),
                        false,
                        false,
                    )?
                    .changed;
                continue;
            }
            let change = match pending.ownership.retention {
                ContentArtifactRetention::Replaceable => {
                    DurableContentArtifactChange::SetReplaceable {
                        owner_id: pending.ownership.owner,
                        artifact_id: pending.artifact.id(),
                    }
                }
                ContentArtifactRetention::Immutable => {
                    DurableContentArtifactChange::InsertImmutable {
                        owner_id: pending.ownership.owner,
                        artifact_id: pending.artifact.id(),
                    }
                }
            };
            let payload = compiled_program_payload(&pending.artifact, false);
            match self.dispatch_parent_source_with_content_artifact_changes(
                &paths[0],
                payload,
                vec![change],
            ) {
                Ok((turn_changed, ticket)) => {
                    changed |= turn_changed;
                    self.program_artifact_store_lane.awaiting_durability.insert(
                        ticket.turn_sequence,
                        PendingProgramArtifactActivation { ticket, pending },
                    );
                }
                Err(
                    PersistentDispatchError::Backpressure(_)
                    | PersistentDispatchError::PersistenceAdmissionFailed { .. },
                ) => {
                    self.program_artifact_store_lane
                        .ready_for_authority
                        .insert(candidate_sequence, pending);
                    return Ok(changed);
                }
                Err(error) => {
                    self.program_artifact_store_lane
                        .remove_waiter_bytes(&pending);
                    changed |= self
                        .finish_program_completion_observed(
                            &pending.session,
                            &pending.request_id,
                            Err(ProgramDiagnostic::artifact(
                                pending.artifact.revision(),
                                error.to_string(),
                            )),
                            false,
                            false,
                        )?
                        .changed;
                }
            }
        }
    }

    fn poll_program_artifact_durability(&mut self) -> ViewResult<bool> {
        let acknowledged = self
            .program_artifact_store_lane
            .awaiting_durability
            .iter()
            .filter_map(|(sequence, activation)| {
                self.runtime
                    .durability_ticket_is_acknowledged(activation.ticket)
                    .then_some(*sequence)
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for sequence in acknowledged {
            let activation = self
                .program_artifact_store_lane
                .awaiting_durability
                .remove(&sequence)
                .expect("acknowledged program artifact activation exists");
            let pending = activation.pending;
            self.program_artifact_store_lane
                .remove_waiter_bytes(&pending);
            changed |= self
                .finish_program_artifact_store_observed(pending, Ok(()), true)?
                .changed;
        }
        Ok(changed)
    }

    fn dispatch_parent_source_with_content_artifact_changes(
        &mut self,
        path: &str,
        payload: SourcePayload,
        changes: Vec<DurableContentArtifactChange>,
    ) -> Result<(bool, DurabilityTicket), PersistentDispatchError> {
        if self.program_host.owns_source_route(path) {
            return Err(PersistentDispatchError::Runtime(
                "program lifecycle route resolved inside the restricted child runtime".to_owned(),
            ));
        }
        let next_sequence = self.sequence.saturating_add(1);
        let event = self
            .runtime
            .runtime()
            .source_event(next_sequence, path, None, payload)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        let (turn, ticket) = self
            .runtime
            .dispatch_with_content_artifact_changes(event, changes)?;
        self.capture_scenario_turn(path, &turn);
        let source_sequence = turn.source_sequence.ok_or_else(|| {
            PersistentDispatchError::Runtime(format!(
                "durable source dispatch `{path}` produced no source sequence"
            ))
        })?;
        let changed = self
            .finish_parent_runtime_turn(turn)
            .map_err(PersistentDispatchError::Runtime)?;
        self.record_event_dispatch(path, source_sequence);
        self.schedule_effect_poll()
            .map_err(PersistentDispatchError::Runtime)?;
        Ok((changed, ticket))
    }

    fn finish_program_artifact_store_observed(
        &mut self,
        pending: PendingProgramArtifactStore,
        result: Result<(), ProgramDiagnostic>,
        authority_committed: bool,
    ) -> ViewResult<ObservedProgramCompletion> {
        let request_id = pending.request_id.0.clone();
        let revision = pending.artifact.revision();
        let queue_depth = pending.queue_depth;
        let queue_wait_us = pending.queue_wait_us;
        let worker_us = pending.worker_us;
        let queued_at = pending.queued_at;
        let failed = result.is_err();
        let apply_started = Instant::now();
        let observed = if !pending.activated_before_store || result.is_err() {
            self.finish_program_completion_observed(
                &pending.session,
                &pending.request_id,
                result.map(|()| pending.artifact),
                false,
                authority_committed,
            )?
        } else if !authority_committed {
            ObservedProgramCompletion {
                changed: false,
                completion: ProgramCompletionObservation::Host(ProgramHostCompletion::Superseded {
                    session: pending.session,
                    request_id: pending.request_id,
                }),
            }
        } else {
            ObservedProgramCompletion {
                changed: false,
                completion: ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
                    ProgramCompletion::Activated { revision },
                )),
            }
        };
        self.async_lane_observations
            .push(RuntimeAsyncLaneObservation {
                lane: RuntimeAsyncLaneKind::ProgramArtifactStore,
                request_id,
                revision,
                queue_depth,
                queue_wait_us,
                worker_us,
                apply_us: duration_us(apply_started.elapsed()),
                end_to_end_us: duration_us(queued_at.elapsed()),
                outcome: runtime_lane_outcome(&observed.completion, failed),
            });
        Ok(observed)
    }

    fn finish_program_completion_observed(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
        artifact_load: bool,
        lifecycle_already_dispatched: bool,
    ) -> ViewResult<ObservedProgramCompletion> {
        let (completion, update) = self.program_host.complete(session, request_id, result);
        let bootstrap = update.bootstrap;
        let lifecycle = if artifact_load || lifecycle_already_dispatched {
            None
        } else {
            match &completion {
                ProgramHostCompletion::Program(ProgramCompletion::Activated { .. }) => self
                    .program_host
                    .active_artifact(session)
                    .map(|artifact| ("compiled", compiled_program_payload(artifact, bootstrap))),
                ProgramHostCompletion::Program(ProgramCompletion::Rejected { diagnostic }) => {
                    let mut payload = SourcePayload {
                        text: Some(diagnostic.message.clone()),
                        ..SourcePayload::default()
                    };
                    payload.fields.insert(
                        "revision".to_owned(),
                        Value::Text(diagnostic.revision.to_string()),
                    );
                    payload.fields.insert(
                        "source_path".to_owned(),
                        Value::Text(diagnostic.source_path.clone()),
                    );
                    payload
                        .fields
                        .insert("line".to_owned(), Value::Text(diagnostic.line.to_string()));
                    payload.fields.insert(
                        "column".to_owned(),
                        Value::Text(diagnostic.column.to_string()),
                    );
                    payload.fields.insert(
                        "diagnostic".to_owned(),
                        Value::Text(diagnostic.message.clone()),
                    );
                    Some(("rejected", payload))
                }
                ProgramHostCompletion::Program(ProgramCompletion::Stale { .. })
                | ProgramHostCompletion::Superseded { .. }
                | ProgramHostCompletion::Removed { .. } => None,
            }
        };
        let program_state_changed = matches!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { .. })
        );
        let mut changed =
            self.queue_program_update(update.patches, update.requests) || program_state_changed;
        if let Some((intent, payload)) = lifecycle {
            for path in self.program_host.lifecycle_source_paths(session, intent) {
                changed |= self.dispatch_source(&path, None, payload.clone())?;
            }
        }
        Ok(ObservedProgramCompletion {
            changed,
            completion: ProgramCompletionObservation::Host(completion),
        })
    }

    pub fn program_diagnostics(&self) -> Vec<ProgramHostDiagnostic> {
        self.program_host.diagnostics()
    }

    pub fn hovered(&self) -> Option<&str> {
        self.hovered.as_deref()
    }

    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }

    pub fn focused_sensitive_input(&self) -> Option<(HostDocumentNodeId, Option<SourceBindingId>)> {
        let node = self.focused.as_ref().and_then(|focused| {
            self.retained_frame()
                .nodes
                .get(&DocumentNodeId(focused.clone()))
        })?;
        node.is_sensitive_text_input().then(|| {
            (
                HostDocumentNodeId(node.id.0.clone()),
                node.primary_source_binding()
                    .map(|binding| binding.id.clone()),
            )
        })
    }

    pub fn event_sequence(&self) -> u64 {
        self.sequence
    }

    pub fn take_external_url(&mut self) -> Option<String> {
        self.pending_external_url.take()
    }

    pub fn last_runtime_phase(&self) -> RuntimePhaseTimings {
        self.last_runtime_phase
    }

    pub fn scheduled_source_deadline(&self) -> Option<Instant> {
        self.scheduled_sources
            .iter()
            .map(|source| source.next)
            .min()
    }

    pub fn advance_scheduled_sources(&mut self, now: Instant) -> ViewResult<bool> {
        let mut due = Vec::new();
        for source in &mut self.scheduled_sources {
            if source.next > now {
                continue;
            }
            due.push(source.path.clone());
            source.next += source.interval;
            if source.next <= now {
                source.next = now + source.interval;
            }
        }
        let mut changed = false;
        for path in due {
            changed |= self.dispatch_source(&path, None, SourcePayload::default())?;
        }
        Ok(changed)
    }

    pub fn inspect_root_current(&mut self, path: &str) -> ViewResult<String> {
        let value = self
            .runtime
            .inspect_value_current(path, 8)
            .map_err(|error| error.to_string())?;
        Ok(format_inspection_value(&value, 0))
    }

    pub fn scenario_target_row(
        &self,
        source_path: &str,
        target_text: Option<&str>,
        address: Option<&str>,
        occurrence: Option<u64>,
    ) -> ViewResult<Option<(u64, u64)>> {
        let Some(target_text) = target_text.or(address) else {
            return Ok(None);
        };
        let occurrence = usize::try_from(occurrence.unwrap_or(0))
            .map_err(|_| "scenario target occurrence exceeds usize".to_owned())?;
        Ok(self
            .runtime
            .runtime()
            .row_target_for_source_text(source_path, target_text, occurrence)
            .map_err(|error| error.to_string())?
            .map(|row| (row.key, row.generation)))
    }

    pub fn take_patches(&mut self) -> Vec<DocumentPatch> {
        std::mem::take(&mut self.pending_patches)
    }

    pub fn apply_layout_demands(&mut self, demands: &[LayoutDemand]) -> ViewResult<bool> {
        let mut windows =
            std::collections::BTreeMap::<u64, (std::ops::Range<u64>, std::ops::Range<u64>)>::new();
        for demand in demands {
            let Some(materialization) = demand.materialization else {
                continue;
            };
            windows
                .entry(materialization)
                .and_modify(|(visible, overscan)| {
                    visible.start = visible.start.min(demand.visible.start);
                    visible.end = visible.end.max(demand.visible.end);
                    overscan.start = overscan.start.min(demand.overscan.start);
                    overscan.end = overscan.end.max(demand.overscan.end);
                })
                .or_insert_with(|| (demand.visible.clone(), demand.overscan.clone()));
        }
        let mut changed = false;
        for (materialization, (visible, overscan)) in windows {
            if self
                .materialization_overscan
                .get(&materialization)
                .is_some_and(|current| current.start <= visible.start && current.end >= visible.end)
            {
                continue;
            }
            let patches = if self.program_host.owns_materialization(materialization) {
                self.program_host
                    .demand_document_window(materialization, visible, overscan.clone())
                    .map_err(|error| error.to_string())?
            } else {
                self.runtime
                    .demand_document_window_by_id(materialization, visible, overscan.clone())
                    .map_err(|error| error.to_string())?;
                let parent = self
                    .runtime
                    .runtime()
                    .primary_retained_output_frame()
                    .map_err(|error| error.to_string())?
                    .clone();
                self.program_host.reconcile(&parent).patches
            };
            self.materialization_overscan
                .insert(materialization, overscan);
            for patch in patches {
                let patch = self.with_view_state(patch);
                self.sync_text_input_patch(&patch);
                self.pending_patches.push(patch);
                changed = true;
            }
        }
        Ok(changed)
    }

    pub fn handle_event(
        &mut self,
        event: &HostEvent,
        target: Option<HitTarget>,
    ) -> ViewResult<bool> {
        self.handle_event_observed(event, target)
            .map(|outcome| outcome.changed)
    }

    pub(crate) fn handle_event_observed(
        &mut self,
        event: &HostEvent,
        target: Option<HitTarget>,
    ) -> ViewResult<RuntimeEventOutcome> {
        debug_assert!(self.event_dispatches.is_none());
        self.event_dispatches = Some(Vec::new());
        self.last_runtime_phase = RuntimePhaseTimings::default();
        let result = self.handle_event_inner(event, target);
        let dispatches = self
            .event_dispatches
            .take()
            .expect("host event dispatch collection is active");
        result.map(|changed| RuntimeEventOutcome {
            changed,
            dispatches,
        })
    }

    fn handle_event_inner(
        &mut self,
        event: &HostEvent,
        target: Option<HitTarget>,
    ) -> ViewResult<bool> {
        match event {
            HostEvent::Pointer(pointer) => match pointer.phase {
                PointerPhase::Move => {
                    let next = target.as_ref().map(|target| target.node.clone());
                    let hover_changed = next != self.hovered;
                    self.hovered = next;
                    let selection_changed = if let (Some(drag), Some(target)) =
                        (self.text_drag.clone(), target.as_ref())
                        && target.node == drag
                        && let (Some(line), Some(column)) = (target.text_line, target.text_column)
                    {
                        self.set_text_input_caret(&drag, line, column, true)
                    } else {
                        false
                    };
                    let source_changed = if let Some(target) = target.as_ref() {
                        self.dispatch_pointer_intent(
                            target,
                            &["pointer_move", "move"],
                            pointer_source_payload(pointer, target),
                        )?
                    } else {
                        false
                    };
                    Ok(hover_changed || selection_changed || source_changed)
                }
                PointerPhase::Leave => {
                    self.text_drag = None;
                    Ok(self.hovered.take().is_some())
                }
                PointerPhase::Down if pointer.button == Some(PointerButton::Primary) => {
                    let focus_requires_immediate_present = target
                        .as_ref()
                        .is_some_and(|target| self.target_is_text_input(target));
                    self.pressed = target.as_ref().map(|target| target.node.clone());
                    let next_focus = target.as_ref().map(|target| target.node.clone());
                    let changed = next_focus != self.focused;
                    let mut dirty = false;
                    if changed && let Some(previous) = self.focused.clone() {
                        dirty |= self.dispatch_node_intent(
                            &previous,
                            &["blur", "source"],
                            SourcePayload::default(),
                        )?;
                        self.sync_text_input_from_document(&previous, None);
                        self.queue_text_input_overlay(&previous);
                    }
                    self.focused = next_focus;
                    if let Some(target) = target.as_ref().filter(|target| {
                        self.focused.as_deref() == Some(target.node.as_str())
                            && self.target_is_text_input(target)
                    }) {
                        self.set_text_input_viewport(
                            &target.node,
                            target.bounds_width,
                            target.bounds_height,
                        );
                        let position = target
                            .text_line
                            .zip(target.text_column)
                            .map(|(line, column)| Position { line, column });
                        let extended = !changed
                            && self.modifiers.shift
                            && position.is_some_and(|position| {
                                self.set_text_input_caret(
                                    &target.node,
                                    position.line,
                                    position.column,
                                    true,
                                )
                            });
                        if !extended {
                            self.sync_text_input_from_document(&target.node, position);
                            self.queue_text_input_overlay(&target.node);
                        }
                        self.text_drag = Some(target.node.clone());
                    } else {
                        self.text_drag = None;
                    }
                    Ok(dirty || focus_requires_immediate_present || changed)
                }
                PointerPhase::Up if pointer.button == Some(PointerButton::Primary) => {
                    self.text_drag = None;
                    let matches = self.pressed.take().as_deref()
                        == target.as_ref().map(|target| target.node.as_str());
                    if matches && let Some(target) = target {
                        if let Some(url) = self.external_url_for_node(&target.node) {
                            self.pending_external_url = Some(url);
                            return Ok(true);
                        }
                        if target.source_intent.as_deref() == Some("double_click") {
                            let now = Instant::now();
                            let is_double_click =
                                self.last_primary_click.take().is_some_and(|(node, at)| {
                                    node == target.node
                                        && now.saturating_duration_since(at)
                                            <= DOUBLE_CLICK_INTERVAL
                                });
                            if is_double_click {
                                return self.dispatch_target(
                                    &target,
                                    pointer_source_payload(pointer, &target),
                                );
                            }
                            self.last_primary_click = Some((target.node, now));
                            return Ok(false);
                        }
                        if pointer_activation_intent(target.source_intent.as_deref())
                            && !self.bare_source_is_text_input(&target)
                        {
                            let focus_request = self.activation_focus_request(&target.node);
                            let mut changed = self.dispatch_target(
                                &target,
                                pointer_source_payload(pointer, &target),
                            )?;
                            if let Some(request) = focus_request {
                                changed |= self.apply_activation_focus_request(request)?;
                            }
                            return Ok(changed);
                        }
                    }
                    Ok(false)
                }
                _ => Ok(false),
            },
            HostEvent::Wheel(wheel) => {
                let Some(target) = target else {
                    return Ok(false);
                };
                let Some(root) = target.scroll_root.clone() else {
                    return Ok(false);
                };
                let root = DocumentNodeId(root);
                let mut scroll = self
                    .scroll_offsets
                    .get(&root.0)
                    .copied()
                    .or_else(|| {
                        self.retained_frame()
                            .nodes
                            .get(&root)
                            .and_then(|node| node.scroll)
                    })
                    .unwrap_or(boon_document_model::ScrollState { x: 0.0, y: 0.0 });
                let previous = scroll;
                scroll.x = (scroll.x + wheel.delta_x).clamp(0.0, target.scroll_max_x.max(0.0));
                scroll.y = (scroll.y + wheel.delta_y).clamp(0.0, target.scroll_max_y.max(0.0));
                if scroll == previous {
                    return Ok(false);
                }
                let root_id = root.0.clone();
                let patch = DocumentPatch::SetScroll { id: root, scroll };
                self.scroll_offsets.insert(root_id, scroll);
                self.pending_patches.push(patch);
                Ok(true)
            }
            HostEvent::TextInput(text) => {
                let text = if self.focused_is_multiline() {
                    text.text.clone()
                } else {
                    single_line_text(&text.text)
                };
                self.edit_focused_text(Command::InsertPlain(text))
            }
            HostEvent::Ime(ime) => match &ime.kind {
                boon_host::ImeInputKind::Commit { text } => {
                    let text = if self.focused_is_multiline() {
                        text.clone()
                    } else {
                        single_line_text(text)
                    };
                    self.edit_focused_text(Command::InsertPlain(text))
                }
                boon_host::ImeInputKind::DeleteSurrounding {
                    before_bytes,
                    after_bytes,
                } => self.delete_surrounding(*before_bytes, *after_bytes),
                _ => Ok(false),
            },
            HostEvent::Keyboard(key) => self.handle_keyboard(key),
            HostEvent::Focus { focused: false, .. } => {
                let previous = self.focused.take();
                self.text_drag = None;
                self.modifiers = InputModifiers::default();
                let Some(previous) = previous else {
                    return Ok(false);
                };
                self.dispatch_node_intent(
                    &previous,
                    &["blur", "source"],
                    SourcePayload::default(),
                )?;
                self.sync_text_input_from_document(&previous, None);
                self.queue_text_input_overlay(&previous);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn dispatch_node_intent(
        &mut self,
        node_id: &str,
        intents: &[&str],
        mut payload: SourcePayload,
    ) -> ViewResult<bool> {
        let frame = self.retained_frame();
        let Some(node) = frame.nodes.get(&DocumentNodeId(node_id.to_owned())) else {
            return Ok(false);
        };
        let Some(binding) = intents.iter().find_map(|intent| {
            node.source_bindings
                .iter()
                .find(|binding| binding.intent == *intent)
        }) else {
            return Ok(false);
        };
        let target = HitTarget {
            node: node_id.to_owned(),
            source_path: Some(binding.source_path.clone()),
            source_intent: Some(binding.intent.clone()),
            row_key: style_u64(node, &["row_key", "target_key", "__row_key"]),
            row_generation: style_u64(
                node,
                &["row_generation", "target_generation", "__row_generation"],
            ),
            scroll_root: None,
            center_x: 0.0,
            center_y: 0.0,
            bounds_x: 0.0,
            bounds_y: 0.0,
            bounds_width: 0.0,
            bounds_height: 0.0,
            scroll_max_x: 0.0,
            scroll_max_y: 0.0,
            text_line: None,
            text_column: None,
        };
        if payload.text.is_none()
            && (matches!(binding.intent.as_str(), "commit" | "submit" | "blur")
                || (node.kind == DocumentNodeKind::TextInput
                    && (binding.intent == "source" || payload.key.is_some())))
        {
            payload.text = self
                .text_inputs
                .get(node_id)
                .map(|state| state.buffer.text())
                .or_else(|| node.text.as_ref().map(|text| text.text.clone()));
        }
        self.dispatch_target(&target, payload)
    }

    fn dispatch_pointer_intent(
        &mut self,
        target: &HitTarget,
        intents: &[&str],
        payload: SourcePayload,
    ) -> ViewResult<bool> {
        let binding = self
            .retained_frame()
            .nodes
            .get(&DocumentNodeId(target.node.clone()))
            .and_then(|node| {
                intents.iter().find_map(|intent| {
                    node.source_bindings
                        .iter()
                        .find(|binding| binding.intent == *intent)
                })
            })
            .cloned();
        let Some(binding) = binding else {
            return Ok(false);
        };
        let mut routed = target.clone();
        routed.source_path = Some(binding.source_path);
        routed.source_intent = Some(binding.intent);
        self.dispatch_target(&routed, payload)
    }

    fn bare_source_is_text_input(&self, target: &HitTarget) -> bool {
        target.source_intent.as_deref() == Some("source") && self.target_is_text_input(target)
    }

    fn target_is_text_input(&self, target: &HitTarget) -> bool {
        self.retained_frame()
            .nodes
            .get(&DocumentNodeId(target.node.clone()))
            .is_some_and(|node| node.kind == DocumentNodeKind::TextInput)
    }

    fn external_url_for_node(&self, node_id: &str) -> Option<String> {
        let style = &self
            .retained_frame()
            .nodes
            .get(&DocumentNodeId(node_id.to_owned()))?
            .style;
        let value = ["to", "href", "url"]
            .into_iter()
            .find_map(|key| style.get(key))?;
        let StyleValue::Text(url) = value else {
            return None;
        };
        external_url_scheme_is_allowed(url).then(|| url.clone())
    }

    fn dispatch_target(
        &mut self,
        target: &HitTarget,
        mut payload: SourcePayload,
    ) -> ViewResult<bool> {
        let Some(path) = target.source_path.as_deref() else {
            return Ok(false);
        };
        if target.row_key.is_none()
            && let Some(field) = self
                .runtime
                .runtime()
                .source_row_lookup_field(path)
                .map(str::to_owned)
            && let Some(value) = self
                .retained_frame()
                .nodes
                .get(&DocumentNodeId(target.node.clone()))
                .and_then(|node| node.style.get(&field))
                .and_then(style_payload_value)
        {
            match (field.as_str(), value) {
                ("address", Value::Text(value)) => payload.address = Some(value),
                ("key", Value::Text(value)) => payload.key = Some(value),
                ("text", Value::Text(value)) => payload.text = Some(value),
                (field, value) => {
                    payload.fields.insert(field.to_owned(), value);
                }
            }
        }
        let row_scoped = self
            .program_host
            .source_is_row_scoped(path)
            .or_else(|| self.runtime.runtime().source_is_row_scoped(path));
        let row = if row_scoped == Some(true) {
            self.row_target(path, target.row_key, target.row_generation)?
        } else {
            None
        };
        self.dispatch_source(path, row, payload)
    }

    fn dispatch_source(
        &mut self,
        path: &str,
        row: Option<RowId>,
        payload: SourcePayload,
    ) -> ViewResult<bool> {
        let next_sequence = self.sequence.saturating_add(1);
        if self.program_host.owns_source_route(path) {
            let (turn, patches) = self
                .program_host
                .dispatch(next_sequence, path, row, payload)
                .map_err(|error| error.to_string())?;
            let source_sequence = turn.source_sequence.ok_or_else(|| {
                format!("child source dispatch `{path}` produced no source sequence")
            })?;
            self.capture_scenario_turn(path, &turn);
            self.sequence = source_sequence_after_turn(self.sequence, turn.source_sequence);
            self.last_runtime_phase = turn.phase_timings;
            self.record_event_dispatch(path, source_sequence);
            return Ok(self.queue_program_update(patches, Vec::new()));
        }
        let event = self
            .runtime
            .runtime()
            .source_event(next_sequence, path, row, payload)
            .map_err(|error| error.to_string())?;
        let turn = self
            .runtime
            .dispatch(event)
            .map_err(|error| error.to_string())?;
        self.capture_scenario_turn(path, &turn);
        let source_sequence = turn
            .source_sequence
            .ok_or_else(|| format!("source dispatch `{path}` produced no source sequence"))?;
        let changed = self.finish_parent_runtime_turn(turn)?;
        self.record_event_dispatch(path, source_sequence);
        self.schedule_effect_poll()?;
        Ok(changed)
    }

    fn record_event_dispatch(&mut self, source_path: &str, source_sequence: u64) {
        if let Some(dispatches) = self.event_dispatches.as_mut() {
            dispatches.push(RuntimeSourceDispatch {
                source_path: source_path.to_owned(),
                source_sequence,
            });
        }
    }

    pub fn caret_blink_deadline(&self) -> Option<Instant> {
        self.focused
            .as_ref()
            .and_then(|focused| self.text_inputs.get(focused))
            .and_then(|state| state.next_blink_at)
    }

    pub fn advance_caret_blink(&mut self, now: Instant) -> bool {
        let Some(focused) = self.focused.clone() else {
            return false;
        };
        let Some(state) = self.text_inputs.get_mut(&focused) else {
            return false;
        };
        if state.next_blink_at.is_none_or(|deadline| deadline > now) {
            return false;
        }
        state.caret_visible = !state.caret_visible;
        state.next_blink_at = Some(now + CARET_BLINK_INTERVAL);
        self.queue_text_input_style(&focused);
        true
    }

    fn focused_is_multiline(&self) -> bool {
        self.focused
            .as_ref()
            .and_then(|id| self.retained_frame().nodes.get(&DocumentNodeId(id.clone())))
            .and_then(|node| node.style.get("multiline"))
            .is_some_and(|value| match value {
                StyleValue::Bool(value) => *value,
                StyleValue::Text(value) => value.eq_ignore_ascii_case("true"),
                StyleValue::Number(value) => *value != 0.0,
                StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => false,
            })
    }

    fn set_text_input_caret(&mut self, id: &str, line: usize, column: usize, extend: bool) -> bool {
        let Some(state) = self.text_inputs.get_mut(id) else {
            return false;
        };
        let _ = state.buffer.set_caret(Position { line, column }, extend);
        state.reset_blink();
        self.queue_text_input_style(id);
        true
    }

    fn edit_focused_text(&mut self, command: Command) -> ViewResult<bool> {
        let Some(focused) = self.focused.clone() else {
            return Ok(false);
        };
        let Some(state) = self.text_inputs.get_mut(&focused) else {
            return Ok(false);
        };
        if !state.buffer.apply(command) {
            return Ok(false);
        }
        state.reset_blink();
        self.finish_focused_edit(&focused)
    }

    fn finish_focused_edit(&mut self, focused: &str) -> ViewResult<bool> {
        let text = self
            .text_inputs
            .get(focused)
            .map(|state| state.buffer.text())
            .unwrap_or_default();
        self.dispatch_node_intent(
            focused,
            &["change", "text", "input", "source"],
            SourcePayload {
                text: Some(text),
                ..SourcePayload::default()
            },
        )?;
        self.queue_text_input_overlay(focused);
        Ok(true)
    }

    fn delete_surrounding(&mut self, before_bytes: u32, after_bytes: u32) -> ViewResult<bool> {
        let Some(focused) = self.focused.clone() else {
            return Ok(false);
        };
        let Some(state) = self.text_inputs.get_mut(&focused) else {
            return Ok(false);
        };
        if !state
            .buffer
            .delete_surrounding_bytes(before_bytes as usize, after_bytes as usize)
        {
            return Ok(false);
        }
        state.reset_blink();
        self.finish_focused_edit(&focused)
    }

    fn handle_keyboard(&mut self, key: &boon_host::KeyEvent) -> ViewResult<bool> {
        let value = logical_key_text(&key.logical_key);
        if update_modifier(&mut self.modifiers, &value, key.pressed) {
            return Ok(false);
        }
        if !key.pressed {
            return Ok(false);
        }
        let Some(focused) = self.focused.clone() else {
            return Ok(false);
        };
        if !self.text_inputs.contains_key(&focused) {
            return self.dispatch_node_intent(
                &focused,
                &["key_down", "source"],
                SourcePayload {
                    key: Some(value),
                    ..SourcePayload::default()
                },
            );
        }

        let normalized = normalize_key(&value);
        if self.modifiers.control || self.modifiers.meta {
            match normalized.as_str() {
                "a" => {
                    let state = self.text_inputs.get_mut(&focused).expect("focused input");
                    let _ = state.buffer.apply(Command::SelectAll);
                    state.reset_blink();
                    self.queue_text_input_style(&focused);
                    return Ok(true);
                }
                "c" => {
                    self.copy_selection_to_clipboard(&focused, false)?;
                    return Ok(false);
                }
                "x" => return self.copy_selection_to_clipboard(&focused, true),
                "v" => {
                    if let Some(text) = self.clipboard_text() {
                        let text = if self.focused_is_multiline() {
                            text
                        } else {
                            single_line_text(&text)
                        };
                        return self.edit_focused_text(Command::InsertPlain(text));
                    }
                    return Ok(false);
                }
                "z" if self.modifiers.shift => {
                    return self.edit_focused_text(Command::Redo);
                }
                "z" => return self.edit_focused_text(Command::Undo),
                "y" => return self.edit_focused_text(Command::Redo),
                _ => return Ok(false),
            }
        }

        let extend = self.modifiers.shift;
        let command = match normalized.as_str() {
            "left" => Some(Command::MoveLeft { extend }),
            "right" => Some(Command::MoveRight { extend }),
            "up" => Some(Command::MoveUp { extend }),
            "down" => Some(Command::MoveDown { extend }),
            "home" => Some(Command::MoveHome { extend }),
            "end" => Some(Command::MoveEnd { extend }),
            "pageup" => Some(Command::PageUp { extend, lines: 20 }),
            "pagedown" => Some(Command::PageDown { extend, lines: 20 }),
            "backspace" => Some(Command::DeleteBackward),
            "delete" => Some(Command::DeleteForward),
            _ => None,
        };
        if let Some(command) = command {
            if matches!(&command, Command::DeleteBackward | Command::DeleteForward) {
                return self.edit_focused_text(command);
            }
            let state = self.text_inputs.get_mut(&focused).expect("focused input");
            let _ = state.buffer.apply(command);
            state.reset_blink();
            self.queue_text_input_style(&focused);
            return Ok(true);
        }

        if normalized == "enter" {
            if self.focused_is_multiline() {
                return self.edit_focused_text(Command::Newline);
            }
            self.dispatch_node_intent(
                &focused,
                &["commit", "submit", "key_down", "source"],
                SourcePayload {
                    key: Some("Enter".to_owned()),
                    ..SourcePayload::default()
                },
            )?;
            self.sync_text_input_from_document(&focused, None);
            self.queue_text_input_overlay(&focused);
            return Ok(true);
        }
        if normalized == "tab" && self.focused_is_multiline() {
            return self.edit_focused_text(if self.modifiers.shift {
                Command::Unindent
            } else {
                Command::Indent
            });
        }
        if normalized == "escape" {
            self.dispatch_node_intent(
                &focused,
                &["cancel", "escape", "key_down", "source"],
                SourcePayload {
                    key: Some("Escape".to_owned()),
                    ..SourcePayload::default()
                },
            )?;
            self.sync_text_input_from_document(&focused, None);
            self.queue_text_input_overlay(&focused);
            return Ok(true);
        }
        Ok(false)
    }

    fn copy_selection_to_clipboard(&mut self, focused: &str, cut: bool) -> ViewResult<bool> {
        let Some(selected) = self
            .text_inputs
            .get(focused)
            .map(|state| state.buffer.selected_text())
        else {
            return Ok(false);
        };
        if selected.is_empty() {
            return Ok(false);
        }
        self.clipboard_system_synchronized = arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(selected.clone()))
            .is_ok();
        self.clipboard_fallback = Some(selected);
        let Some(state) = self.text_inputs.get_mut(focused) else {
            return Ok(false);
        };
        if !cut || !state.buffer.apply(Command::InsertPlain(String::new())) {
            return Ok(false);
        }
        state.reset_blink();
        self.finish_focused_edit(focused)
    }

    fn clipboard_text(&self) -> Option<String> {
        let platform = (self.clipboard_system_synchronized || self.clipboard_fallback.is_none())
            .then(|| {
                arboard::Clipboard::new()
                    .and_then(|mut clipboard| clipboard.get_text())
                    .ok()
            })
            .flatten();
        platform.or_else(|| self.clipboard_fallback.clone())
    }

    fn activation_focus_request(&self, source: &str) -> Option<ActivationFocusRequest> {
        let node = self
            .retained_frame()
            .nodes
            .get(&DocumentNodeId(source.to_owned()))?;
        let request = node.activation_focus.as_ref()?;
        let line = request
            .line
            .saturating_sub(1)
            .try_into()
            .unwrap_or(usize::MAX);
        let column = request
            .column
            .saturating_sub(1)
            .try_into()
            .unwrap_or(usize::MAX);
        Some(ActivationFocusRequest {
            input_id: request.input_id.clone(),
            position: Position { line, column },
        })
    }

    fn apply_activation_focus_request(
        &mut self,
        request: ActivationFocusRequest,
    ) -> ViewResult<bool> {
        let Some(target) = unique_text_input(self.retained_frame(), &request.input_id) else {
            return Ok(false);
        };
        let focus_changed = self.focused.as_deref() != Some(target.as_str());
        if focus_changed && let Some(previous) = self.focused.clone() {
            self.dispatch_node_intent(&previous, &["blur", "source"], SourcePayload::default())?;
            self.sync_text_input_from_document(&previous, None);
            self.queue_text_input_overlay(&previous);
        }
        self.focused = Some(target.clone());
        self.text_drag = None;
        self.sync_text_input_from_document(&target, Some(request.position));
        self.queue_text_input_overlay(&target);
        Ok(true)
    }

    fn set_text_input_viewport(&mut self, id: &str, width: f32, height: f32) {
        let Some(state) = self.text_inputs.get_mut(id) else {
            return;
        };
        state.viewport_width = (width.is_finite() && width > 0.0).then_some(width);
        state.viewport_height = (height.is_finite() && height > 0.0).then_some(height);
    }

    fn sync_text_input_from_document(&mut self, id: &str, position: Option<Position>) {
        let Some(text) = self
            .retained_frame()
            .nodes
            .get(&DocumentNodeId(id.to_owned()))
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .map(|node| {
                node.text
                    .as_ref()
                    .map(|text| text.text.clone())
                    .unwrap_or_default()
            })
        else {
            return;
        };
        let state = self
            .text_inputs
            .entry(id.to_owned())
            .or_insert_with(|| TextInputState::new(&text));
        state.reset(
            &text,
            position.unwrap_or(Position {
                line: usize::MAX,
                column: usize::MAX,
            }),
        );
    }

    fn queue_text_input_overlay(&mut self, id: &str) {
        let Some(state) = self.text_inputs.get(id) else {
            return;
        };
        self.pending_patches.push(DocumentPatch::SetText {
            id: DocumentNodeId(id.to_owned()),
            text: TextValue {
                text: state.buffer.text(),
            },
        });
        self.queue_text_input_style(id);
    }

    fn queue_text_input_style(&mut self, id: &str) {
        let Some(state) = self.text_inputs.get(id) else {
            return;
        };
        let focused = self.focused.as_deref() == Some(id);
        let selection = state.buffer.selection();
        let (start, end) = if selection.anchor <= selection.head {
            (selection.anchor, selection.head)
        } else {
            (selection.head, selection.anchor)
        };
        let mut patch = StylePatch::new();
        patch.insert(
            "caret_visible".to_owned(),
            Some(StyleValue::Bool(focused && state.caret_visible)),
        );
        patch.insert(
            "caret_column".to_owned(),
            Some(StyleValue::Number(state.buffer.caret().column as f64)),
        );
        patch.insert(
            "caret_line".to_owned(),
            Some(StyleValue::Number(state.buffer.caret().line as f64)),
        );
        patch.insert(
            "selection_start".to_owned(),
            (focused && start != end).then_some(StyleValue::Number(start.column as f64)),
        );
        patch.insert(
            "selection_end".to_owned(),
            (focused && start != end).then_some(StyleValue::Number(end.column as f64)),
        );
        patch.insert(
            "selection_start_line".to_owned(),
            (focused && start != end).then_some(StyleValue::Number(start.line as f64)),
        );
        patch.insert(
            "selection_end_line".to_owned(),
            (focused && start != end).then_some(StyleValue::Number(end.line as f64)),
        );
        self.pending_patches.push(DocumentPatch::SetStyle {
            id: DocumentNodeId(id.to_owned()),
            patch,
        });
        self.queue_text_input_caret_reveal(id);
    }

    fn queue_text_input_caret_reveal(&mut self, id: &str) {
        let Some(node) = self
            .retained_frame()
            .nodes
            .get(&DocumentNodeId(id.to_owned()))
        else {
            return;
        };
        if !style_flag(node, &["scroll", "scroll_x", "scroll_y", "scrollbars"]) {
            return;
        }
        let Some(state) = self.text_inputs.get(id) else {
            return;
        };
        let Some(viewport_width) = state.viewport_width else {
            return;
        };
        let Some(viewport_height) = state.viewport_height else {
            return;
        };
        let font_size = style_number(node, &["size"]).unwrap_or(14.0).max(1.0) as f32;
        let line_height = style_number(node, &["line_height"])
            .map(|value| {
                if value > 0.0 && value < 4.0 {
                    value as f32 * font_size
                } else {
                    value as f32
                }
            })
            .unwrap_or(font_size * 1.25)
            .max(1.0);
        let padding = style_number(node, &["padding"]).unwrap_or(0.0).max(0.0) as f32;
        let inset = style_number(node, &["text_inset"]).unwrap_or(4.0).max(0.0) as f32;
        let visible_width = (viewport_width - padding * 2.0).max(font_size);
        let visible_height = (viewport_height - padding * 2.0).max(line_height);
        let caret = state.buffer.caret();
        let content_height = state.buffer.line_count() as f32 * line_height + inset * 2.0;
        let max_y = (content_height - visible_height).max(0.0);
        let advance = font_size * 0.62;
        let content_width = state.buffer.max_line_columns() as f32 * advance + inset * 2.0;
        let max_x = if style_flag(node, &["text_wrap"]) {
            0.0
        } else {
            (content_width - visible_width).max(0.0)
        };
        let mut scroll = self
            .scroll_offsets
            .get(id)
            .copied()
            .or(node.scroll)
            .unwrap_or(boon_document_model::ScrollState { x: 0.0, y: 0.0 });
        let previous = scroll;
        let caret_top = inset + caret.line as f32 * line_height;
        let caret_bottom = caret_top + line_height;
        if caret_top < scroll.y + line_height {
            scroll.y = (caret_top - line_height).max(0.0);
        } else if caret_bottom > scroll.y + visible_height - line_height {
            scroll.y = caret_bottom - visible_height + line_height;
        }
        let caret_x = inset + caret.column as f32 * advance;
        if caret_x < scroll.x + advance {
            scroll.x = (caret_x - advance).max(0.0);
        } else if caret_x + advance > scroll.x + visible_width - advance {
            scroll.x = caret_x + advance - visible_width + advance;
        }
        scroll.x = scroll.x.clamp(0.0, max_x);
        scroll.y = scroll.y.clamp(0.0, max_y);
        if scroll == previous {
            return;
        }
        self.scroll_offsets.insert(id.to_owned(), scroll);
        self.pending_patches.push(DocumentPatch::SetScroll {
            id: DocumentNodeId(id.to_owned()),
            scroll,
        });
    }

    fn sync_text_input_patch(&mut self, patch: &DocumentPatch) {
        match patch {
            DocumentPatch::UpsertNode(node) if node.kind == DocumentNodeKind::TextInput => {
                if self.focused.as_deref() != Some(node.id.0.as_str()) {
                    self.text_inputs.insert(
                        node.id.0.clone(),
                        TextInputState::new(
                            node.text
                                .as_ref()
                                .map(|text| text.text.as_str())
                                .unwrap_or_default(),
                        ),
                    );
                }
            }
            DocumentPatch::SetText { id, text }
                if self.focused.as_deref() != Some(id.0.as_str()) =>
            {
                self.text_inputs
                    .insert(id.0.clone(), TextInputState::new(&text.text));
            }
            DocumentPatch::RemoveNode { id } => {
                self.text_inputs.remove(&id.0);
            }
            _ => {}
        }
    }

    fn with_view_state(&self, patch: DocumentPatch) -> DocumentPatch {
        match patch {
            DocumentPatch::UpsertNode(mut node) => {
                if let Some(scroll) = self.scroll_offsets.get(&node.id.0).copied() {
                    node.scroll = Some(scroll);
                }
                DocumentPatch::UpsertNode(node)
            }
            patch => patch,
        }
    }

    fn row_target(
        &self,
        source_path: &str,
        key: Option<u64>,
        generation: Option<u64>,
    ) -> ViewResult<Option<RowId>> {
        let Some(key) = key else {
            return Ok(None);
        };
        if self.program_host.owns_source_route(source_path) {
            return self
                .program_host
                .row_target_for_source_path(source_path, key, generation.unwrap_or(1))
                .map(Some)
                .map_err(|error| error.to_string());
        }
        self.runtime
            .runtime()
            .row_target_for_source_path(source_path, key, generation.unwrap_or(1))
            .map(Some)
            .map_err(|error| error.to_string())
    }

    fn queue_program_update(
        &mut self,
        patches: Vec<DocumentPatch>,
        requests: Vec<ProgramHostRequest>,
    ) -> bool {
        let changed = !patches.is_empty();
        let structural = patches.iter().any(|patch| {
            matches!(
                patch,
                DocumentPatch::UpsertNode(_)
                    | DocumentPatch::RemoveNode { .. }
                    | DocumentPatch::InsertChild { .. }
                    | DocumentPatch::RemoveChild { .. }
                    | DocumentPatch::MoveChild { .. }
            )
        });
        self.pending_program_requests.extend(requests);
        for patch in patches {
            let patch = self.with_view_state(patch);
            self.sync_text_input_patch(&patch);
            self.pending_patches.push(patch);
        }
        if structural {
            let frame = self.program_host.frame().clone();
            self.retain_view_state(&frame, false);
        }
        changed
    }

    fn finish_parent_runtime_turn(&mut self, turn: RuntimeTurn) -> ViewResult<bool> {
        let durable_sequence = turn.sequence;
        let durable_queued_at = Instant::now();
        let persistence_enqueue_us = turn.phase_timings.persistence_enqueue_us;
        let parent_patches = turn.document_patches;
        self.runtime_turn_sequence = turn.sequence;
        self.sequence = source_sequence_after_turn(self.sequence, turn.source_sequence);
        self.persistence_status = self.query_persistence_status();
        self.pending_durable_lanes.insert(
            durable_sequence,
            PendingDurableLane {
                queued_at: durable_queued_at,
                enqueue_us: persistence_enqueue_us,
                queue_depth: self
                    .persistence_status
                    .queue_depth
                    .max(1)
                    .try_into()
                    .unwrap_or(u32::MAX),
            },
        );
        self.record_durable_lane_completions(Instant::now(), 0);
        self.next_persistence_poll = Some(Instant::now() + PERSISTENCE_ACK_POLL_INTERVAL);
        self.last_runtime_phase = turn.phase_timings;
        let parent = self
            .runtime
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| error.to_string())?;
        let update = self
            .program_host
            .reconcile_with_parent_patches(parent, parent_patches);
        let changed = self.queue_program_update(update.patches, update.requests);
        Ok(changed)
    }

    fn record_durable_lane_completions(&mut self, now: Instant, apply_us: u64) {
        let through = self.persistence_status.durable_through_turn_sequence;
        let worker_us = self
            .persistence_status
            .timings
            .encode_us
            .saturating_add(self.persistence_status.timings.checkpoint_us);
        let completed = self
            .pending_durable_lanes
            .range(..=through)
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>();
        for sequence in completed {
            let pending = self
                .pending_durable_lanes
                .remove(&sequence)
                .expect("selected durable lane exists");
            let end_to_end_us = pending.enqueue_us.saturating_add(duration_us(
                now.saturating_duration_since(pending.queued_at),
            ));
            let queue_wait_us = end_to_end_us
                .saturating_sub(pending.enqueue_us)
                .saturating_sub(worker_us)
                .saturating_sub(apply_us);
            self.async_lane_observations
                .push(RuntimeAsyncLaneObservation {
                    lane: RuntimeAsyncLaneKind::PersistenceTurn,
                    request_id: format!("turn-{sequence}"),
                    revision: sequence,
                    queue_depth: pending.queue_depth,
                    queue_wait_us,
                    worker_us,
                    apply_us,
                    end_to_end_us,
                    outcome: RuntimeAsyncLaneOutcome::Applied,
                });
        }
    }

    fn schedule_effect_poll(&mut self) -> ViewResult<()> {
        let has_work = self.effect_worker.is_busy() || self.runtime.has_effect_work();
        self.next_effect_poll = has_work.then_some(Instant::now() + EFFECT_POLL_INTERVAL);
        Ok(())
    }

    fn capture_scenario_turn(&mut self, source_path: &str, turn: &RuntimeTurn) {
        if self.scenario_trigger_source.as_deref() != Some(source_path) {
            if self.scenario_trigger_source.is_some() && !turn.document_patches.is_empty() {
                self.scenario_trigger_turn = Some(turn.clone());
            }
            return;
        }
        let mut declared = turn.clone();
        if let Some(earlier) = self.scenario_trigger_turn.take() {
            declared.document_patches.extend(earlier.document_patches);
        }
        self.scenario_trigger_turn = Some(declared);
        self.scenario_trigger_source = None;
    }

    fn dispatch_host_lifecycle_started(&mut self) -> ViewResult<()> {
        if !has_host_lifecycle_started_source(self.runtime.runtime()) {
            return Ok(());
        }
        let payload =
            host_lifecycle_started_payload(self.host_identity_mode, self.host_identity_generation);
        self.dispatch_source(HOST_LIFECYCLE_STARTED_SOURCE, None, payload)?;
        Ok(())
    }
}

fn dispatch_host_lifecycle_started(
    runtime: &mut PersistentRuntime,
    mode: HostIdentityMode,
    generation: u64,
    sequence: u64,
) -> ViewResult<Option<RuntimeTurn>> {
    if !has_host_lifecycle_started_source(runtime.runtime()) {
        return Ok(None);
    }
    let event = runtime
        .runtime()
        .source_event(
            sequence,
            HOST_LIFECYCLE_STARTED_SOURCE,
            None,
            host_lifecycle_started_payload(mode, generation),
        )
        .map_err(|error| error.to_string())?;
    runtime
        .dispatch(event)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn has_host_lifecycle_started_source(runtime: &LiveRuntime) -> bool {
    runtime
        .source_inventory()
        .sources
        .iter()
        .any(|source| source.path == HOST_LIFECYCLE_STARTED_SOURCE)
}

fn host_lifecycle_started_payload(mode: HostIdentityMode, generation: u64) -> SourcePayload {
    let (instance_id, grant_id) = match mode {
        HostIdentityMode::Interactive => (
            uuid::Uuid::new_v4().hyphenated().to_string(),
            uuid::Uuid::new_v4().hyphenated().to_string(),
        ),
        HostIdentityMode::Deterministic => (
            format!(
                "00000000-0000-4000-8000-{:012x}",
                generation.min(0xffff_ffff_ffff)
            ),
            format!(
                "10000000-0000-4000-8000-{:012x}",
                generation.min(0xffff_ffff_ffff)
            ),
        ),
    };
    SourcePayload {
        fields: [
            ("instance_id".to_owned(), Value::Text(instance_id)),
            ("grant_id".to_owned(), Value::Text(grant_id)),
        ]
        .into_iter()
        .collect(),
        ..SourcePayload::default()
    }
}

fn native_effect_worker() -> ViewResult<HostEffectWorker> {
    let mut router = HostEffectRouter::new();
    router
        .register(
            "File/write_bytes",
            FileEffectDriver::new(repository_root().join(EFFECT_DIRECTORY))
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    router
        .register(
            crate::passkey_simulator::REGISTER_OPERATION,
            crate::passkey_simulator::DevelopmentPasskeySimulator::registration(),
        )
        .map_err(|error| error.to_string())?;
    router
        .register(
            crate::passkey_simulator::AUTHENTICATE_OPERATION,
            crate::passkey_simulator::DevelopmentPasskeySimulator::authentication(),
        )
        .map_err(|error| error.to_string())?;
    HostEffectWorker::start(router).map_err(|error| error.to_string())
}

fn validate_preview_plan(plan: &MachinePlan) -> ViewResult<()> {
    if !plan.application.identity.is_valid() {
        return Err("MachinePlan has an invalid application identity".to_owned());
    }
    if plan.document_plan().is_none() {
        return Err("MachinePlan has no typed document plan".to_owned());
    }
    Ok(())
}

fn scheduled_sources(runtime: &LiveRuntime) -> ViewResult<Vec<ScheduledSource>> {
    if let Some(source) = runtime
        .source_inventory()
        .sources
        .iter()
        .find(|source| source.interval_ms == Some(0))
    {
        return Err(format!(
            "scheduled source `{}` has a zero interval",
            source.path
        ));
    }
    let now = Instant::now();
    Ok(runtime
        .source_inventory()
        .sources
        .iter()
        .filter_map(|source| {
            let interval = Duration::from_millis(source.interval_ms?);
            Some(ScheduledSource {
                path: source.path.clone(),
                interval,
                next: now + interval,
            })
        })
        .collect())
}

fn authority_plan_counts(plan: &MachinePlan) -> AuthorityPlanCounts {
    AuthorityPlanCounts {
        scalar: saturating_u32(
            plan.persistence
                .memory
                .iter()
                .filter(|memory| memory.kind == MemoryKind::Scalar)
                .count(),
        ),
        indexed_field: saturating_u32(
            plan.persistence
                .memory
                .iter()
                .filter(|memory| memory.kind == MemoryKind::IndexedField)
                .count(),
        ),
        list: saturating_u32(plan.persistence.lists.len()),
        effect_contract: saturating_u32(plan.persistence.effect_outbox.len()),
    }
}

fn authority_selections(
    plan: &MachinePlan,
) -> std::collections::BTreeMap<String, AuthoritySelection> {
    let mut selections = std::collections::BTreeMap::new();
    for memory in &plan.persistence.memory {
        let kind = match memory.kind {
            MemoryKind::Scalar => AuthoritySelectionKind::Scalar,
            MemoryKind::IndexedField => AuthoritySelectionKind::IndexedField,
            MemoryKind::List => AuthoritySelectionKind::List,
        };
        selections.insert(
            memory.semantic_path.clone(),
            AuthoritySelection {
                semantic_path: memory.semantic_path.clone(),
                memory_id: *memory.memory_id.as_bytes(),
                kind,
                row: None,
                leaf_id: (memory.kind == MemoryKind::IndexedField)
                    .then(|| memory.leaves.first().map(|leaf| *leaf.leaf_id.as_bytes()))
                    .flatten(),
            },
        );
    }
    for list in &plan.persistence.lists {
        selections.insert(
            list.semantic_path.clone(),
            AuthoritySelection {
                semantic_path: list.semantic_path.clone(),
                memory_id: *list.memory_id.as_bytes(),
                kind: AuthoritySelectionKind::List,
                row: None,
                leaf_id: None,
            },
        );
        for field in &list.row_fields {
            selections.insert(
                field.semantic_path.clone(),
                AuthoritySelection {
                    semantic_path: field.semantic_path.clone(),
                    memory_id: *list.memory_id.as_bytes(),
                    kind: AuthoritySelectionKind::IndexedField,
                    row: None,
                    leaf_id: Some(*field.leaf_id.as_bytes()),
                },
            );
        }
    }
    selections
}

fn saturating_u32(value: usize) -> u32 {
    value.try_into().unwrap_or(u32::MAX)
}

fn bounded_persistence_text(value: &str) -> String {
    if value.len() <= MAX_PERSISTENCE_STATUS_BYTES {
        return value.to_owned();
    }
    let mut end = MAX_PERSISTENCE_STATUS_BYTES
        .saturating_sub(3)
        .min(value.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

fn available_capability() -> PersistenceCapability {
    PersistenceCapability {
        available: true,
        reason: String::new(),
    }
}

fn state_database_path_in(
    state_root: &Path,
    application: &ApplicationIdentity,
) -> ViewResult<PathBuf> {
    let application =
        ApplicationPlan::new(application.clone()).map_err(|error| error.to_string())?;
    Ok(state_root
        .join(digest_hex(&application.identity_hash))
        .join("state.redb"))
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("boon_native_playground lives under the workspace crates directory")
        .to_path_buf()
}

fn configured_state_root() -> PathBuf {
    std::env::var_os(STATE_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| repository_root().join(STATE_DIRECTORY))
}

pub(crate) fn digest_hex(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn external_url_scheme_is_allowed(url: &str) -> bool {
    ["https://", "http://", "mailto:"]
        .into_iter()
        .any(|prefix| url.starts_with(prefix))
}

fn single_line_text(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

fn logical_key_text(key: &boon_host::LogicalKey) -> String {
    match key {
        boon_host::LogicalKey::Character(value) | boon_host::LogicalKey::Named(value) => {
            value.clone()
        }
        boon_host::LogicalKey::Dead(Some(value)) => value.to_string(),
        boon_host::LogicalKey::Dead(None) | boon_host::LogicalKey::Unidentified => String::new(),
    }
}

fn normalize_key(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "arrowleft" | "leftarrow" => "left".to_owned(),
        "arrowright" | "rightarrow" => "right".to_owned(),
        "arrowup" | "uparrow" => "up".to_owned(),
        "arrowdown" | "downarrow" => "down".to_owned(),
        "back_space" => "backspace".to_owned(),
        "return" | "kp_enter" => "enter".to_owned(),
        value => value.to_owned(),
    }
}

fn update_modifier(modifiers: &mut InputModifiers, value: &str, pressed: bool) -> bool {
    let normalized = value.to_ascii_lowercase();
    let target = if normalized == "shift" || normalized.starts_with("shift_") {
        Some(&mut modifiers.shift)
    } else if matches!(normalized.as_str(), "control" | "ctrl")
        || normalized.starts_with("control_")
        || normalized.starts_with("ctrl_")
    {
        Some(&mut modifiers.control)
    } else if normalized == "alt" || normalized.starts_with("alt_") {
        Some(&mut modifiers.alt)
    } else if matches!(normalized.as_str(), "meta" | "super")
        || normalized.starts_with("meta_")
        || normalized.starts_with("super_")
    {
        Some(&mut modifiers.meta)
    } else {
        None
    };
    if let Some(target) = target {
        *target = pressed;
        true
    } else {
        false
    }
}

fn format_inspection_value(value: &Value, depth: usize) -> String {
    const MAX_DEPTH: usize = 4;
    const MAX_ITEMS: usize = 24;
    const MAX_TEXT: usize = 256;
    if depth >= MAX_DEPTH {
        return "...".to_owned();
    }
    match value {
        Value::Null => "Null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Text(value) => {
            let mut bounded = value.chars().take(MAX_TEXT).collect::<String>();
            if value.chars().count() > MAX_TEXT {
                bounded.push_str("...");
            }
            format!("\"{bounded}\"")
        }
        Value::Bytes(value) => format!("Bytes[{}]", value.len()),
        Value::List(values) => {
            let mut parts = values
                .iter()
                .take(MAX_ITEMS)
                .map(|value| format_inspection_value(value, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > MAX_ITEMS {
                parts.push(format!("... {} more", values.len() - MAX_ITEMS));
            }
            format!("[{}]", parts.join(", "))
        }
        Value::Record(fields) => {
            let mut parts = fields
                .iter()
                .take(MAX_ITEMS)
                .map(|(name, value)| {
                    format!("{name}: {}", format_inspection_value(value, depth + 1))
                })
                .collect::<Vec<_>>();
            if fields.len() > MAX_ITEMS {
                parts.push(format!("... {} more", fields.len() - MAX_ITEMS));
            }
            format!("[{}]", parts.join(", "))
        }
        Value::MappedRow { id, fields } => {
            let mut parts = fields
                .iter()
                .take(MAX_ITEMS)
                .map(|(name, value)| {
                    format!("{name}: {}", format_inspection_value(value, depth + 1))
                })
                .collect::<Vec<_>>();
            if fields.len() > MAX_ITEMS {
                parts.push(format!("... {} more", fields.len() - MAX_ITEMS));
            }
            format!(
                "MappedRow(list={}, key={}, generation={}, [{}])",
                id.list.0,
                id.key,
                id.generation,
                parts.join(", ")
            )
        }
        Value::Row { id, fields } => format!(
            "Row(list={}, key={}, generation={}, fields={})",
            id.list.0,
            id.key,
            id.generation,
            fields.len()
        ),
        Value::Error { code } => format!("Error[{code}]"),
    }
}

fn pointer_activation_intent(intent: Option<&str>) -> bool {
    intent.is_some_and(|intent| {
        matches!(
            intent,
            "press" | "click" | "source" | "activate" | "toggle" | "submit" | "open" | "select"
        )
    })
}

fn pointer_source_payload(pointer: &boon_host::PointerEvent, target: &HitTarget) -> SourcePayload {
    let mut payload = SourcePayload::default();
    if target.bounds_width.is_finite()
        && target.bounds_height.is_finite()
        && target.bounds_width > 0.0
        && target.bounds_height > 0.0
    {
        let local_x = (pointer.x - target.bounds_x).clamp(0.0, target.bounds_width);
        let local_y = (pointer.y - target.bounds_y).clamp(0.0, target.bounds_height);
        payload.fields.insert(
            "pointer_x".to_owned(),
            Value::Number(local_x.round() as i64),
        );
        payload.fields.insert(
            "pointer_y".to_owned(),
            Value::Number(local_y.round() as i64),
        );
        payload.fields.insert(
            "pointer_width".to_owned(),
            Value::Number(target.bounds_width.round() as i64),
        );
        payload.fields.insert(
            "pointer_height".to_owned(),
            Value::Number(target.bounds_height.round() as i64),
        );
    }
    payload
}

fn compiled_program_payload(artifact: &ProgramArtifact, bootstrap: bool) -> SourcePayload {
    let mut payload = SourcePayload {
        text: Some(artifact.source_digest().to_owned()),
        ..SourcePayload::default()
    };
    payload.fields.insert(
        "revision".to_owned(),
        Value::Text(artifact.revision().to_string()),
    );
    payload.fields.insert(
        "source_digest".to_owned(),
        Value::Text(artifact.source_digest().to_owned()),
    );
    payload.fields.insert(
        "compiler".to_owned(),
        Value::Text(artifact.compiler_id().to_owned()),
    );
    payload.fields.insert(
        "target".to_owned(),
        Value::Text(artifact.target_profile_id().to_owned()),
    );
    payload.fields.insert(
        "capability_profile".to_owned(),
        Value::Text(artifact.capability_profile_id().to_owned()),
    );
    payload
        .fields
        .insert("artifact_id".to_owned(), Value::Text(artifact.id_text()));
    payload.fields.insert(
        "plan_digest".to_owned(),
        Value::Text(artifact.plan_digest().to_owned()),
    );
    payload
        .fields
        .insert("bootstrap".to_owned(), Value::Bool(bootstrap));
    payload
}

fn style_payload_value(value: &StyleValue) -> Option<Value> {
    match value {
        StyleValue::Text(value) => Some(Value::Text(value.clone())),
        StyleValue::Number(value) if value.is_finite() => Some(Value::Number(*value as i64)),
        StyleValue::Number(_) => None,
        StyleValue::Bool(value) => Some(Value::Bool(*value)),
        StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

fn state_from_mount(patches: Vec<DocumentPatch>) -> ViewResult<DocumentState> {
    let root = patches.iter().find_map(|patch| match patch {
        DocumentPatch::UpsertNode(node)
            if node.parent.is_none() && node.kind == DocumentNodeKind::Root =>
        {
            Some(node.id.0.clone())
        }
        _ => None,
    });
    let root = root.ok_or_else(|| "typed mount patches contain no document root".to_owned())?;
    let mut state = DocumentState::new(root);
    for patch in patches {
        state
            .apply_patch(patch)
            .map_err(|error| error.to_string())?;
    }
    Ok(state)
}

fn source_sequence_after_turn(current: u64, source_sequence: Option<u64>) -> u64 {
    source_sequence.unwrap_or(current)
}

fn style_u64(node: &boon_document::DocumentNode, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| match node.style.get(*key) {
        Some(StyleValue::Number(value)) if value.is_finite() && *value >= 0.0 => {
            Some(*value as u64)
        }
        Some(StyleValue::Text(value)) => value.parse().ok(),
        _ => None,
    })
}

fn style_number(node: &boon_document::DocumentNode, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| match node.style.get(*key) {
        Some(StyleValue::Number(value)) if value.is_finite() => Some(*value),
        Some(StyleValue::Text(value)) => value.parse().ok(),
        _ => None,
    })
}

fn style_flag(node: &boon_document::DocumentNode, keys: &[&str]) -> bool {
    keys.iter().any(|key| match node.style.get(*key) {
        Some(StyleValue::Bool(value)) => *value,
        Some(StyleValue::Number(value)) => *value != 0.0,
        Some(StyleValue::Text(value)) => value.eq_ignore_ascii_case("true"),
        Some(StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_)) | None => false,
    })
}

fn unique_text_input(
    frame: &DocumentFrame,
    input_id: &boon_document::TextInputId,
) -> Option<String> {
    let mut candidates = frame
        .nodes
        .values()
        .filter(|node| {
            node.kind == DocumentNodeKind::TextInput
                && node.text_input_id.as_ref() == Some(input_id)
        })
        .map(|node| node.id.0.clone());
    let target = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    Some(target)
}

fn duration_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn runtime_lane_outcome(
    completion: &ProgramCompletionObservation,
    failed: bool,
) -> RuntimeAsyncLaneOutcome {
    if failed {
        return RuntimeAsyncLaneOutcome::Failed;
    }
    match completion {
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Stale { .. },
        ))
        | ProgramCompletionObservation::Host(ProgramHostCompletion::Superseded { .. })
        | ProgramCompletionObservation::Host(ProgramHostCompletion::Removed { .. }) => {
            RuntimeAsyncLaneOutcome::StaleRejected
        }
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Rejected { .. },
        )) => RuntimeAsyncLaneOutcome::Failed,
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Activated { .. },
        ))
        | ProgramCompletionObservation::ArtifactStorePending { .. } => {
            RuntimeAsyncLaneOutcome::Applied
        }
    }
}
