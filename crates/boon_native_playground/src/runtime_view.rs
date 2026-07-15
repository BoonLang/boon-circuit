use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentNodeKind, DocumentState, LayoutDemand, StylePatch,
    StyleValue, TextValue,
};
use boon_editor::{Buffer, Command, Position};
use boon_host::{
    DocumentNodeId as HostDocumentNodeId, HostEvent, PointerButton, PointerPhase, SourceBindingId,
};
use boon_persistence::{
    ContentArtifact, ContentArtifactStoreEnqueueError, ContentArtifactStoreTicket,
    MigrationPreview, OutboxInspectorState, PersistenceInspectorSnapshot, PersistenceWorkerConfig,
    PersistenceWorkerStatus, RedbDriver,
};
use boon_plan::{ApplicationIdentity, ApplicationPlan, MachinePlan, MemoryKind};
use boon_runtime::{
    DocumentPatch, DocumentPatchStatus, FileEffectDriver, HostEffectRouter, HostEffectWorker,
    LiveRuntime, PersistentRuntime, PersistentRuntimeStartup, PersistentRuntimeStartupDisposition,
    ProgramArtifact, ProgramCompletion, ProgramDiagnostic, ProgramDocumentHost,
    ProgramHostCompletion, ProgramHostDiagnostic, ProgramHostRequest, ProgramRequestId,
    ProgramSessionId, RowId, RuntimePhaseTimings, RuntimeTurn, SessionOptions, SourcePayload,
    Value,
};
use std::collections::BTreeMap;
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
#[cfg(test)]
use boon_persistence::InMemoryDriver;

type ViewResult<T> = Result<T, String>;

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const PERSISTENCE_ACK_POLL_INTERVAL: Duration = Duration::from_millis(25);
const STATE_DIRECTORY: &str = "playground/state";
const STATE_ROOT_ENV: &str = "BOON_PLAYGROUND_STATE_ROOT";
const EFFECT_DIRECTORY: &str = "playground/effects";
const EFFECT_POLL_INTERVAL: Duration = Duration::from_millis(1);
const HOST_LIFECYCLE_STARTED_SOURCE: &str = "host.lifecycle.started";

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

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PersistenceQueryCounts {
    pub status: u64,
    pub storage: u64,
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
}

impl TextInputState {
    fn new(text: &str) -> Self {
        Self {
            buffer: Buffer::new(text),
            caret_visible: true,
            next_blink_at: None,
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
    pending_program_artifact_stores:
        BTreeMap<ContentArtifactStoreTicket, PendingProgramArtifactStore>,
    retry_program_artifact_store: Option<PendingProgramArtifactStore>,
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
    #[cfg(test)]
    persistence_query_counts: PersistenceQueryCounts,
    hovered: Option<String>,
    pressed: Option<String>,
    focused: Option<String>,
    text_inputs: std::collections::BTreeMap<String, TextInputState>,
    text_drag: Option<String>,
    modifiers: InputModifiers,
    scroll_offsets: std::collections::BTreeMap<String, boon_document_model::ScrollState>,
    materialization_overscan: std::collections::BTreeMap<u64, std::ops::Range<u64>>,
    pending_patches: Vec<DocumentPatch>,
    sequence: u64,
    last_dispatched_source: Option<String>,
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
}

#[derive(Clone, Debug)]
struct PendingProgramArtifactStore {
    session: ProgramSessionId,
    request_id: ProgramRequestId,
    artifact: ProgramArtifact,
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
    pub fn open(plan: Arc<MachinePlan>) -> ViewResult<Self> {
        Self::open_with_state_root_and_identity_mode(
            plan,
            configured_state_root(),
            HostIdentityMode::Interactive,
        )
    }

    pub fn open_for_scenario(plan: Arc<MachinePlan>) -> ViewResult<Self> {
        Self::open_with_state_root_and_identity_mode(
            plan,
            configured_state_root(),
            HostIdentityMode::Deterministic,
        )
    }

    #[cfg(test)]
    pub fn open_with_state_root(
        plan: Arc<MachinePlan>,
        state_root: impl AsRef<Path>,
    ) -> ViewResult<Self> {
        Self::open_with_state_root_and_identity_mode(
            plan,
            state_root,
            HostIdentityMode::Interactive,
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
        let (mut runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            runtime.shared_machine_plan(),
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
            pending_program_artifact_stores: BTreeMap::new(),
            retry_program_artifact_store: None,
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
            #[cfg(test)]
            persistence_query_counts: PersistenceQueryCounts {
                status: 1,
                storage: 1,
            },
            hovered: None,
            pressed: None,
            focused: None,
            text_inputs,
            text_drag: None,
            modifiers: InputModifiers::default(),
            scroll_offsets: std::collections::BTreeMap::new(),
            materialization_overscan: std::collections::BTreeMap::new(),
            pending_patches: Vec::new(),
            sequence: source_sequence,
            last_dispatched_source: None,
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
        };
        view.resolve_program_artifact_requests()?;
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
        self.pending_program_artifact_stores.clear();
        self.retry_program_artifact_store = None;
        self.program_artifact_cache.clear();
        let _ = self.runtime.take_content_artifact_store_completions();
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
        self.resolve_program_artifact_requests()?;
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
        self.persistence_status = self.query_persistence_status();
        let mut changed = self.persistence_status != previous_status;
        let idle = self.persistence_status.pending.is_none()
            && self.persistence_status.queue_depth == 0
            && self.persistence_status.reserved_slots == 0
            && self.persistence_status.pending_content_artifact_stores == 0
            && self.pending_program_artifact_stores.is_empty()
            && self.retry_program_artifact_store.is_none();
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

    #[cfg(test)]
    pub(crate) fn persistence_query_counts(&self) -> PersistenceQueryCounts {
        self.persistence_query_counts
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
        self.persistence_status = self.query_persistence_status();
        self.refresh_persistence_inspector();
        self.persistence_status = self.query_persistence_status();
        self.next_persistence_poll = None;
    }

    fn refresh_persistence_inspector(&mut self) -> bool {
        #[cfg(test)]
        {
            self.persistence_query_counts.storage =
                self.persistence_query_counts.storage.saturating_add(1);
        }
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
        #[cfg(test)]
        {
            self.persistence_query_counts.status =
                self.persistence_query_counts.status.saturating_add(1);
        }
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
                let content = self
                    .program_artifact_cache
                    .get(&artifact_id)
                    .cloned()
                    .map_or_else(
                        || {
                            self.runtime
                                .load_content_artifact(artifact_id)
                                .map_err(|error| {
                                    ProgramDiagnostic::artifact(
                                        request.compile.revision,
                                        error.to_string(),
                                    )
                                })
                                .and_then(|artifact| {
                                    artifact.ok_or_else(|| {
                                    ProgramDiagnostic::artifact(
                                        request.compile.revision,
                                        format!(
                                            "immutable program artifact {artifact_id} is missing"
                                        ),
                                    )
                                })
                                })
                        },
                        Ok,
                    );
                let result = content.and_then(|artifact| {
                    self.program_artifact_cache
                        .insert(artifact.id, artifact.clone());
                    ProgramArtifact::from_content_artifact(
                        request.compile.revision,
                        request.compile.capability_profile,
                        artifact,
                    )
                });
                changed |= self
                    .complete_program_observed(&request.session, &request.request_id, result)?
                    .changed;
            }
        }
        self.pending_program_requests = compile_requests;
        Ok(changed)
    }

    pub fn take_program_requests(&mut self) -> Vec<ProgramHostRequest> {
        std::mem::take(&mut self.pending_program_requests)
    }

    pub fn has_pending_program_artifact_store(&self) -> bool {
        !self.pending_program_artifact_stores.is_empty()
            || self.retry_program_artifact_store.is_some()
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
        let persist_artifact = self
            .program_host
            .request_persists_artifact(session, request_id);
        let artifact_load = self
            .program_host
            .request_is_artifact_load(session, request_id);
        if persist_artifact {
            return match result {
                Ok(artifact) => {
                    let pending = PendingProgramArtifactStore {
                        session: session.clone(),
                        request_id: request_id.clone(),
                        artifact,
                    };
                    self.enqueue_program_artifact_store(pending)
                }
                Err(diagnostic) => self.finish_program_completion_observed(
                    session,
                    request_id,
                    Err(diagnostic),
                    false,
                ),
            };
        }
        self.finish_program_completion_observed(session, request_id, result, artifact_load)
    }

    fn enqueue_program_artifact_store(
        &mut self,
        pending: PendingProgramArtifactStore,
    ) -> ViewResult<ObservedProgramCompletion> {
        if self.retry_program_artifact_store.is_some()
            || !self.pending_program_artifact_stores.is_empty()
        {
            return self.finish_program_completion_observed(
                &pending.session,
                &pending.request_id,
                Err(ProgramDiagnostic::artifact(
                    pending.artifact.revision(),
                    "another immutable program artifact is still pending persistence",
                )),
                false,
            );
        }
        let content = pending.artifact.to_content_artifact();
        let artifact_id = pending.artifact.id();
        let completion = ProgramCompletionObservation::ArtifactStorePending {
            session: pending.session.clone(),
            request_id: pending.request_id.clone(),
            artifact_id,
        };
        match self.runtime.try_put_content_artifact(content) {
            Ok(ticket) => {
                self.pending_program_artifact_stores.insert(ticket, pending);
            }
            Err(ContentArtifactStoreEnqueueError::Backpressure(_)) => {
                self.retry_program_artifact_store = Some(pending);
            }
            Err(ContentArtifactStoreEnqueueError::Closed(_)) => {
                return self.finish_program_completion_observed(
                    &pending.session,
                    &pending.request_id,
                    Err(ProgramDiagnostic::artifact(
                        pending.artifact.revision(),
                        "persistence coordinator closed before storing the program artifact",
                    )),
                    false,
                );
            }
        }
        self.next_persistence_poll = Some(Instant::now());
        Ok(ObservedProgramCompletion {
            changed: false,
            completion,
        })
    }

    pub fn poll_program_artifact_stores(&mut self) -> ViewResult<bool> {
        let mut changed = false;
        if let Some(pending) = self.retry_program_artifact_store.take() {
            let content = pending.artifact.to_content_artifact();
            match self.runtime.try_put_content_artifact(content) {
                Ok(ticket) => {
                    self.pending_program_artifact_stores.insert(ticket, pending);
                }
                Err(ContentArtifactStoreEnqueueError::Backpressure(_)) => {
                    self.retry_program_artifact_store = Some(pending);
                }
                Err(ContentArtifactStoreEnqueueError::Closed(_)) => {
                    changed |= self
                        .finish_program_completion_observed(
                            &pending.session,
                            &pending.request_id,
                            Err(ProgramDiagnostic::artifact(
                                pending.artifact.revision(),
                                "persistence coordinator closed before storing the program artifact",
                            )),
                            false,
                        )?
                        .changed;
                }
            }
        }
        for completion in self.runtime.take_content_artifact_store_completions() {
            let Some(pending) = self
                .pending_program_artifact_stores
                .remove(&completion.ticket)
            else {
                continue;
            };
            let result = completion
                .result
                .map_err(|error| {
                    ProgramDiagnostic::artifact(pending.artifact.revision(), error.to_string())
                })
                .and_then(|ack| {
                    (ack.id == pending.artifact.id())
                        .then_some(pending.artifact.clone())
                        .ok_or_else(|| {
                            ProgramDiagnostic::artifact(
                                pending.artifact.revision(),
                                "persistence acknowledged a different program artifact",
                            )
                        })
                });
            if let Ok(artifact) = &result {
                self.program_artifact_cache
                    .insert(artifact.id(), artifact.to_content_artifact());
            }
            changed |= self
                .finish_program_completion_observed(
                    &pending.session,
                    &pending.request_id,
                    result,
                    false,
                )?
                .changed;
        }
        if changed {
            changed |= self.resolve_program_artifact_requests()?;
        }
        let pending = self.retry_program_artifact_store.is_some()
            || !self.pending_program_artifact_stores.is_empty();
        if pending {
            self.next_persistence_poll = Some(Instant::now() + PERSISTENCE_ACK_POLL_INTERVAL);
        }
        Ok(changed)
    }

    fn finish_program_completion_observed(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
        artifact_load: bool,
    ) -> ViewResult<ObservedProgramCompletion> {
        let (completion, update) = self.program_host.complete(session, request_id, result);
        let bootstrap = update.bootstrap;
        let lifecycle = if artifact_load {
            None
        } else {
            match &completion {
                ProgramHostCompletion::Program(ProgramCompletion::Activated { revision }) => {
                    self.program_host.active_artifact(session).map(|artifact| {
                        let mut payload = SourcePayload {
                            text: Some(artifact.source_digest().to_owned()),
                            ..SourcePayload::default()
                        };
                        payload
                            .fields
                            .insert("revision".to_owned(), Value::Text(revision.to_string()));
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
                        ("compiled", payload)
                    })
                }
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
        let mut changed = self.queue_program_update(update.patches, update.requests);
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

    pub fn last_dispatched_source(&self) -> Option<&str> {
        self.last_dispatched_source.as_deref()
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
        self.last_runtime_phase = RuntimePhaseTimings::default();
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
                        self.sync_text_input_from_document(
                            &target.node,
                            target
                                .text_line
                                .zip(target.text_column)
                                .map(|(line, column)| Position { line, column }),
                        );
                        self.text_drag = Some(target.node.clone());
                        self.queue_text_input_overlay(&target.node);
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
                            return self.dispatch_target(
                                &target,
                                pointer_source_payload(pointer, &target),
                            );
                        }
                    }
                    Ok(false)
                }
                _ => Ok(false),
            },
            HostEvent::Wheel(wheel) => {
                let Some(root) = target.and_then(|target| target.scroll_root) else {
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
                scroll.x = (scroll.x + wheel.delta_x).max(0.0);
                scroll.y = (scroll.y + wheel.delta_y).max(0.0);
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
            self.capture_scenario_turn(path, &turn);
            self.sequence = source_sequence_after_turn(self.sequence, turn.source_sequence);
            self.last_runtime_phase = turn.phase_timings;
            self.last_dispatched_source = Some(path.to_owned());
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
        let source_sequence = turn.source_sequence;
        let changed = self.finish_parent_runtime_turn(turn)?;
        self.last_dispatched_source = Some(path.to_owned());
        debug_assert!(source_sequence.is_some());
        self.schedule_effect_poll()?;
        Ok(changed)
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
        let changed = state.buffer.set_caret(Position { line, column }, extend);
        state.reset_blink();
        self.queue_text_input_style(id);
        let _ = changed;
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
        let runtime_changed = self.dispatch_node_intent(
            focused,
            &["change", "text", "input", "source"],
            SourcePayload {
                text: Some(text),
                ..SourcePayload::default()
            },
        )?;
        self.queue_text_input_overlay(focused);
        let _ = runtime_changed;
        Ok(true)
    }

    fn delete_surrounding(&mut self, before_bytes: u32, after_bytes: u32) -> ViewResult<bool> {
        let Some(focused) = self.focused.clone() else {
            return Ok(false);
        };
        let Some(state) = self.text_inputs.get_mut(&focused) else {
            return Ok(false);
        };
        let mut changed = false;
        for _ in 0..before_bytes {
            changed |= state.buffer.apply(Command::DeleteBackward);
        }
        for _ in 0..after_bytes {
            changed |= state.buffer.apply(Command::DeleteForward);
        }
        if !changed {
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
                    let changed = state.buffer.apply(Command::SelectAll);
                    state.reset_blink();
                    self.queue_text_input_style(&focused);
                    let _ = changed;
                    return Ok(true);
                }
                "c" => {
                    self.copy_selection_to_clipboard(&focused, false)?;
                    return Ok(false);
                }
                "x" => return self.copy_selection_to_clipboard(&focused, true),
                "v" => {
                    if let Ok(mut clipboard) = arboard::Clipboard::new()
                        && let Ok(text) = clipboard.get_text()
                    {
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
            "backspace" => Some(Command::DeleteBackward),
            "delete" => Some(Command::DeleteForward),
            _ => None,
        };
        if let Some(command) = command {
            if matches!(&command, Command::DeleteBackward | Command::DeleteForward) {
                return self.edit_focused_text(command);
            }
            let state = self.text_inputs.get_mut(&focused).expect("focused input");
            let changed = state.buffer.apply(command);
            state.reset_blink();
            self.queue_text_input_style(&focused);
            let _ = changed;
            return Ok(true);
        }

        if normalized == "enter" {
            if self.focused_is_multiline() {
                return self.edit_focused_text(Command::Newline);
            }
            let changed = self.dispatch_node_intent(
                &focused,
                &["commit", "submit", "key_down", "source"],
                SourcePayload {
                    key: Some("Enter".to_owned()),
                    ..SourcePayload::default()
                },
            )?;
            self.sync_text_input_from_document(&focused, None);
            self.queue_text_input_overlay(&focused);
            let _ = changed;
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
            let changed = self.dispatch_node_intent(
                &focused,
                &["cancel", "escape", "key_down", "source"],
                SourcePayload {
                    key: Some("Escape".to_owned()),
                    ..SourcePayload::default()
                },
            )?;
            self.sync_text_input_from_document(&focused, None);
            self.queue_text_input_overlay(&focused);
            let _ = changed;
            return Ok(true);
        }
        Ok(false)
    }

    fn copy_selection_to_clipboard(&mut self, focused: &str, cut: bool) -> ViewResult<bool> {
        let Some(state) = self.text_inputs.get_mut(focused) else {
            return Ok(false);
        };
        let selected = state.buffer.selected_text();
        if selected.is_empty() {
            return Ok(false);
        }
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(selected);
        }
        if !cut || !state.buffer.apply(Command::InsertPlain(String::new())) {
            return Ok(false);
        }
        state.reset_blink();
        self.finish_focused_edit(focused)
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
        let parent_patches = turn.document_patches;
        self.runtime_turn_sequence = turn.sequence;
        self.sequence = source_sequence_after_turn(self.sequence, turn.source_sequence);
        self.persistence_status = self.query_persistence_status();
        self.next_persistence_poll = Some(Instant::now() + PERSISTENCE_ACK_POLL_INTERVAL);
        self.last_runtime_phase = turn.phase_timings;
        let parent = self
            .runtime
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| error.to_string())?;
        let update = self
            .program_host
            .reconcile_with_parent_patches(&parent, parent_patches);
        let changed = self.queue_program_update(update.patches, update.requests);
        Ok(changed)
    }

    fn schedule_effect_poll(&mut self) -> ViewResult<()> {
        let has_work = self.effect_worker.is_busy() || self.runtime.has_effect_work();
        self.next_effect_poll = has_work.then_some(Instant::now() + EFFECT_POLL_INTERVAL);
        Ok(())
    }

    fn capture_scenario_turn(&mut self, source_path: &str, turn: &RuntimeTurn) {
        if self.scenario_trigger_turn.is_none()
            && self.scenario_trigger_source.as_deref() == Some(source_path)
        {
            self.scenario_trigger_turn = Some(turn.clone());
            self.scenario_trigger_source = None;
        }
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
    let instance_id = match mode {
        HostIdentityMode::Interactive => uuid::Uuid::new_v4().hyphenated().to_string(),
        HostIdentityMode::Deterministic => {
            format!(
                "00000000-0000-4000-8000-{:012x}",
                generation.min(0xffff_ffff_ffff)
            )
        }
    };
    SourcePayload {
        fields: [("instance_id".to_owned(), Value::Text(instance_id))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use boon_host::{KeyEvent, LogicalKey, PointerEvent, SurfaceId, TextInputEvent, WheelEvent};
    use boon_runtime::RuntimeSourceUnit;

    fn persons_plan(
        schema_version: u64,
        predecessor: Option<&MachinePlan>,
        additive_source: Option<&str>,
    ) -> Arc<MachinePlan> {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let mut units = example
            .units
            .into_iter()
            .map(|unit| boon_compiler::CompilerSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        if let Some(source) = additive_source {
            units.push(boon_compiler::CompilerSourceUnit {
                path: "examples/persons_pro/MigrationProbe.bn".to_owned(),
                source: source.to_owned(),
            });
        }
        let predecessors = predecessor
            .map(boon_plan::MigrationPredecessorBinding::from_machine_plan)
            .into_iter()
            .collect::<Vec<_>>();
        Arc::new(
            boon_compiler::compile_runtime_source_units_to_machine_plan_with_persistence_catalog(
                "examples/persons_pro/RUN.bn",
                &units,
                boon_plan::TargetProfile::SoftwareDefault,
                example.application,
                schema_version,
                &predecessors,
            )
            .unwrap()
            .plan,
        )
    }

    fn unique_state_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("boon-{label}-{}-{nonce}", std::process::id()))
    }

    fn complete_pending_programs_successfully(model: &mut RuntimeView) -> usize {
        let mut completed = 0usize;
        for _ in 0..8 {
            model.resolve_program_artifact_requests().unwrap();
            let requests = model.take_program_requests();
            if requests.is_empty() {
                return completed;
            }
            for request in requests {
                assert!(
                    !request.is_artifact_load(),
                    "stored artifact request reached the compiler helper"
                );
                let artifact = boon_runtime::compile_program_artifact(&request.compile)
                    .unwrap_or_else(|diagnostic| {
                        panic!(
                            "expected successful child compile for {}: {diagnostic}",
                            request.session.0
                        )
                    });
                model
                    .complete_program(&request.session, &request.request_id, Ok(artifact))
                    .unwrap();
                completed += 1;
            }
            settle_program_artifact_stores(model);
        }
        panic!("child program requests did not settle in eight rounds")
    }

    fn complete_pending_programs(model: &mut RuntimeView) -> (usize, usize) {
        let mut activated = 0usize;
        let mut rejected = 0usize;
        for _ in 0..8 {
            model.resolve_program_artifact_requests().unwrap();
            let requests = model.take_program_requests();
            if requests.is_empty() {
                return (activated, rejected);
            }
            for request in requests {
                assert!(
                    !request.is_artifact_load(),
                    "stored artifact request reached the compiler helper"
                );
                let result = boon_runtime::compile_program_artifact(&request.compile);
                if result.is_ok() {
                    activated += 1;
                } else {
                    rejected += 1;
                }
                model
                    .complete_program(&request.session, &request.request_id, result)
                    .unwrap();
            }
            settle_program_artifact_stores(model);
        }
        panic!("child program requests did not settle in eight rounds")
    }

    fn settle_program_artifact_stores(model: &mut RuntimeView) -> usize {
        let started = Instant::now();
        let mut changed = 0usize;
        loop {
            changed += usize::from(model.poll_program_artifact_stores().unwrap());
            if model.pending_program_artifact_stores.is_empty()
                && model.retry_program_artifact_store.is_none()
            {
                return changed;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "program artifact persistence did not settle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn settle_host_effects(model: &mut RuntimeView) {
        let started = Instant::now();
        for round in 0..1_000 {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "host effects did not settle within two seconds; round={round}, busy={}, outbox={:?}",
                model.effect_worker.is_busy(),
                model.runtime.effect_work_items()
            );
            let Some(deadline) = model.effect_poll_deadline() else {
                assert!(!model.effect_worker.is_busy());
                assert!(model.runtime.effect_work_items().unwrap().is_empty());
                return;
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                std::thread::sleep(remaining.min(Duration::from_millis(2)));
            }
            model
                .poll_host_effects(Instant::now())
                .unwrap_or_else(|error| panic!("host effect poll {round} failed: {error}"));
            if model.effect_worker.is_busy() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        panic!("host effects did not settle in 1,000 polls")
    }

    fn dispatch_effect_action(model: &mut RuntimeView, source_path: &str) {
        model
            .dispatch_source(source_path, None, SourcePayload::default())
            .unwrap();
        assert!(
            model.effect_poll_deadline().is_some(),
            "{source_path} did not enqueue durable host-effect work"
        );
        assert!(
            !model.runtime.effect_work_items().unwrap().is_empty(),
            "{source_path} was not visible in the persistent outbox"
        );
        settle_host_effects(model);
    }

    fn assert_current_values(model: &mut RuntimeView, expected: &[(&str, Value)]) {
        for (path, expected) in expected {
            assert_eq!(
                model.runtime.inspect_value_current(path, 8).unwrap(),
                *expected,
                "unexpected current value at {path}"
            );
        }
    }

    fn inspected_list_field_values(model: &mut RuntimeView, path: &str) -> Vec<Value> {
        let Value::List(rows) = model.runtime.inspect_value_current(path, 32).unwrap() else {
            panic!("inspected list field `{path}` did not return rows")
        };
        rows.into_iter()
            .map(|row| {
                let Value::Record(mut fields) = row else {
                    panic!("inspected list field `{path}` returned a non-record row")
                };
                fields
                    .remove("value")
                    .unwrap_or_else(|| panic!("inspected list field `{path}` row has no value"))
            })
            .collect()
    }

    fn authoritative_semantic_snapshot(
        model: &mut RuntimeView,
        authority_plan: &MachinePlan,
    ) -> std::collections::BTreeMap<String, Value> {
        let mut snapshot = std::collections::BTreeMap::new();
        for memory in &authority_plan.persistence.memory {
            snapshot.insert(
                memory.semantic_path.clone(),
                model
                    .runtime
                    .inspect_value_current(&memory.semantic_path, 32)
                    .unwrap(),
            );
        }
        for list in &authority_plan.persistence.lists {
            for field in &list.row_fields {
                snapshot.insert(
                    field.semantic_path.clone(),
                    Value::List(inspected_list_field_values(model, &field.semantic_path)),
                );
            }
        }
        snapshot
    }

    #[test]
    fn internal_runtime_turns_do_not_advance_the_source_event_sequence() {
        assert_eq!(source_sequence_after_turn(7, None), 7);
        assert_eq!(source_sequence_after_turn(7, Some(8)), 8);
    }

    #[test]
    fn scenario_turn_capture_survives_pointer_hover_and_down_before_dispatch() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/counter.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 980.0,
                height: 760.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        let step = example
            .test_steps
            .iter()
            .find(|step| step.action_kind.is_some())
            .unwrap();
        let target = view
            .target_for_source(&step.source_path, step.target_text.as_deref())
            .unwrap();
        model.begin_scenario_step(&step.source_path);
        for phase in [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up] {
            model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: target.center_x,
                        y: target.center_y,
                        phase,
                        button: (phase != PointerPhase::Move).then_some(PointerButton::Primary),
                    }),
                    Some(target.clone()),
                )
                .unwrap();
        }
        model
            .assert_scenario_step(&boon_runtime::ScenarioStep {
                id: "counter-pointer-turn".to_owned(),
                user_action_kind: Some("button_press".to_owned()),
                user_action_text: None,
                user_action_key: None,
                source_event: None,
                expectations: vec![
                    boon_runtime::ScenarioExpectation::RootText {
                        name: "store.count".to_owned(),
                        value: "1".to_owned(),
                    },
                    boon_runtime::ScenarioExpectation::DocumentChanged,
                ],
            })
            .unwrap();
    }

    #[test]
    fn persistence_snapshot_and_render_construction_use_only_cached_state() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("minimal")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/minimal.bn", &units).unwrap();
        let model = RuntimeView::open_in_memory(runtime).unwrap();
        let queries_before = model.persistence_query_counts();

        let _snapshot = model.cached_persistence_snapshot(1, 1, None, None);
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let _view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 640.0,
                height: 480.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        let _snapshot = model.cached_persistence_snapshot(2, 1, None, None);

        assert_eq!(model.persistence_query_counts(), queries_before);
    }

    #[test]
    fn sensitive_input_never_enters_the_ordinary_editor_or_source_payload() {
        const SENTINEL: &str = "sensitive-runtime-sentinel-28b7";
        let source = r#"
store: [
    password_events: [change: SOURCE]
]

document: Document/new(
    root: Element/text_input(
        element: [events: store.password_events]
        style: [width: 240, height: 36, sensitive: True]
        text: TEXT { }
    )
)
"#;
        let runtime = LiveRuntime::from_source("sensitive-input.bn", source).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let sensitive = model
            .retained_frame()
            .nodes
            .values()
            .find(|node| node.is_sensitive_text_input())
            .expect("fixture has sensitive text input")
            .clone();
        assert!(!model.text_inputs.contains_key(&sensitive.id.0));

        model.focused = Some(sensitive.id.0.clone());
        let (node, binding) = model
            .focused_sensitive_input()
            .expect("focused sensitive input has a host target");
        assert_eq!(node.0, sensitive.id.0);
        assert_eq!(
            binding,
            sensitive
                .primary_source_binding()
                .map(|item| item.id.clone())
        );

        let sequence = model.event_sequence();
        assert!(
            !model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: SurfaceId("preview".to_owned()),
                        text: SENTINEL.to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.event_sequence(), sequence);
        assert!(!format!("{:?}", model.frame()).contains(SENTINEL));
    }

    #[test]
    fn persons_semantic_memory_is_an_exact_authoritative_allowlist() {
        let plan = persons_plan(1, None, None);
        let scalar_paths = plan
            .persistence
            .memory
            .iter()
            .map(|memory| memory.semantic_path.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            scalar_paths,
            [
                "store.account_id",
                "store.active_view",
                "store.draft_compile_digest",
                "store.draft_revision",
                "store.last_valid_draft_revision",
                "store.last_valid_draft_source",
                "store.mode",
                "store.passkey_simulation",
                "store.passkey_workflow_state",
                "store.preview_surface",
                "store.publish_candidate_revision",
                "store.publish_candidate_source",
                "store.publish_request_sequence",
                "store.publish_settled_sequence",
                "store.published_capability_profile",
                "store.published_artifact_id",
                "store.published_compiler",
                "store.published_digest",
                "store.published_plan_digest",
                "store.published_revision",
                "store.published_source",
                "store.published_target",
                "store.signed_out",
                "store.source_draft",
                "store.workspace_id",
            ]
            .into_iter()
            .collect()
        );

        let list_paths = plan
            .persistence
            .lists
            .iter()
            .map(|list| list.semantic_path.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            list_paths,
            ["store.credential_descriptors", "store.published_revisions"]
                .into_iter()
                .collect()
        );
        let row_fields = plan
            .persistence
            .lists
            .iter()
            .flat_map(|list| list.row_fields.iter())
            .map(|field| field.semantic_path.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            row_fields,
            [
                "store.credential_descriptors.credential_id",
                "store.credential_descriptors.label",
                "store.published_revisions.capability_profile",
                "store.published_revisions.artifact_id",
                "store.published_revisions.compiler",
                "store.published_revisions.draft_revision",
                "store.published_revisions.request",
                "store.published_revisions.source",
                "store.published_revisions.source_digest",
                "store.published_revisions.plan_digest",
                "store.published_revisions.target",
            ]
            .into_iter()
            .collect()
        );

        for forbidden in [
            "store.draft_compile_column",
            "store.draft_compile_diagnostic",
            "store.draft_compile_line",
            "store.draft_compile_path",
            "store.passkey_message",
            "store.publish_diagnostic",
            "store.publish_state",
        ] {
            assert!(
                !scalar_paths.contains(forbidden),
                "transient or derived field `{forbidden}` entered semantic memory"
            );
        }
        assert_eq!(plan.persistence.effect_outbox.len(), 2);
    }

    #[test]
    fn persons_passkey_workflow_uses_the_durable_outbox_and_preserves_account_authority() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        complete_pending_programs_successfully(&mut model);

        for (mode_source, expected_state) in [
            ("store.elements.simulate_cancel", "Cancelled"),
            ("store.elements.simulate_failure", "Failed"),
        ] {
            model
                .dispatch_source(mode_source, None, SourcePayload::default())
                .unwrap();
            dispatch_effect_action(&mut model, "store.elements.register_passkey");
            assert_current_values(
                &mut model,
                &[
                    (
                        "store.passkey_workflow_state",
                        Value::Text(expected_state.to_owned()),
                    ),
                    ("store.account_state", Value::Text("Anonymous".to_owned())),
                    ("store.account_id", Value::Text(String::new())),
                    ("store.credential_count", Value::Number(0)),
                ],
            );
        }

        model
            .dispatch_source(
                "store.elements.simulate_success",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        dispatch_effect_action(&mut model, "store.elements.register_passkey");
        let account_id = model
            .runtime
            .inspect_value_current("store.account_id", 1)
            .unwrap();
        let Value::Text(account_id_text) = &account_id else {
            panic!("passkey registration did not create a public account id")
        };
        assert!(account_id_text.starts_with("account-"));
        assert_current_values(
            &mut model,
            &[
                (
                    "store.passkey_workflow_state",
                    Value::Text("Registered".to_owned()),
                ),
                ("store.account_state", Value::Text("OnePasskey".to_owned())),
                ("store.credential_count", Value::Number(1)),
            ],
        );

        model
            .dispatch_source(
                "store.elements.simulate_duplicate",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        dispatch_effect_action(&mut model, "store.elements.register_passkey");
        assert_current_values(
            &mut model,
            &[
                (
                    "store.passkey_workflow_state",
                    Value::Text("Duplicate".to_owned()),
                ),
                ("store.account_id", account_id.clone()),
                ("store.credential_count", Value::Number(1)),
            ],
        );

        model
            .dispatch_source(
                "store.elements.simulate_success",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        dispatch_effect_action(&mut model, "store.elements.register_passkey");
        assert_current_values(
            &mut model,
            &[
                ("store.account_id", account_id.clone()),
                ("store.account_state", Value::Text("TwoPasskeys".to_owned())),
                ("store.credential_count", Value::Number(2)),
            ],
        );

        model
            .dispatch_source("store.elements.sign_out", None, SourcePayload::default())
            .unwrap();
        for (mode_source, expected_state) in [
            ("store.elements.simulate_cancel", "Cancelled"),
            ("store.elements.simulate_failure", "Failed"),
        ] {
            model
                .dispatch_source(mode_source, None, SourcePayload::default())
                .unwrap();
            dispatch_effect_action(&mut model, "store.elements.sign_in");
            assert_current_values(
                &mut model,
                &[
                    (
                        "store.passkey_workflow_state",
                        Value::Text(expected_state.to_owned()),
                    ),
                    ("store.account_state", Value::Text("SignedOut".to_owned())),
                    ("store.account_id", account_id.clone()),
                    ("store.credential_count", Value::Number(2)),
                ],
            );
        }

        model
            .dispatch_source(
                "store.elements.simulate_success",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        dispatch_effect_action(&mut model, "store.elements.sign_in");
        assert_current_values(
            &mut model,
            &[
                (
                    "store.passkey_workflow_state",
                    Value::Text("Authenticated".to_owned()),
                ),
                ("store.account_state", Value::Text("TwoPasskeys".to_owned())),
                ("store.account_id", account_id),
                ("store.credential_count", Value::Number(2)),
            ],
        );
    }

    #[test]
    fn persons_overlapping_passkey_completions_are_idempotent_and_bounded() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        complete_pending_programs_successfully(&mut model);

        for _ in 0..2 {
            model
                .dispatch_source(
                    "store.elements.register_passkey",
                    None,
                    SourcePayload::default(),
                )
                .unwrap();
        }
        assert_eq!(model.runtime.effect_work_items().unwrap().len(), 2);
        settle_host_effects(&mut model);
        assert_current_values(
            &mut model,
            &[
                ("store.account_state", Value::Text("OnePasskey".to_owned())),
                ("store.credential_count", Value::Number(1)),
            ],
        );
        let first_ids =
            inspected_list_field_values(&mut model, "store.credential_descriptors.credential_id");
        assert_eq!(first_ids.len(), 1);

        dispatch_effect_action(&mut model, "store.elements.register_passkey");
        assert_current_values(
            &mut model,
            &[
                ("store.account_state", Value::Text("TwoPasskeys".to_owned())),
                ("store.credential_count", Value::Number(2)),
            ],
        );
        let two_ids =
            inspected_list_field_values(&mut model, "store.credential_descriptors.credential_id");
        assert_eq!(two_ids.len(), 2);
        assert_ne!(two_ids[0], two_ids[1]);

        dispatch_effect_action(&mut model, "store.elements.register_passkey");
        assert_eq!(
            inspected_list_field_values(&mut model, "store.credential_descriptors.credential_id",),
            two_ids,
            "credential authority must remain capped after two registrations"
        );
    }

    #[test]
    fn persons_first_invalid_publish_creates_no_public_artifact_or_pointer() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        complete_pending_programs_successfully(&mut model);

        model
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some("scene: Missing/constructor(\n".to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let invalid_draft = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-draft")
            .expect("invalid draft request");
        model
            .complete_program(
                &invalid_draft.session,
                &invalid_draft.request_id,
                boon_runtime::compile_program_artifact(&invalid_draft.compile),
            )
            .unwrap();

        model
            .dispatch_source("store.elements.publish", None, SourcePayload::default())
            .unwrap();
        let failed_publish = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-candidate")
            .expect("invalid publish candidate request");
        model
            .complete_program(
                &failed_publish.session,
                &failed_publish.request_id,
                boon_runtime::compile_program_artifact(&failed_publish.compile),
            )
            .unwrap();
        model
            .dispatch_source(
                "store.elements.show_published_page",
                None,
                SourcePayload::default(),
            )
            .unwrap();

        assert!(
            model
                .take_program_requests()
                .iter()
                .all(|request| request.session.0 != "persons-public-published")
        );
        assert_current_values(
            &mut model,
            &[
                ("store.publish_state", Value::Text("Failed".to_owned())),
                ("store.has_published_revision", Value::Bool(false)),
                ("store.published_revision", Value::Number(0)),
                ("store.published_revision_count", Value::Number(0)),
                ("store.published_digest", Value::Text(String::new())),
            ],
        );
        assert!(model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Nothing published yet")
        }));
    }

    #[test]
    fn persons_declarative_scenario_runs_every_action_and_semantic_expectation() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 1_280.0,
                height: 900.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        converge_test_demands(&mut model, &mut view, &mut columns);
        let mut completed_actions = 0usize;

        for step in &example.test_steps {
            settle_scenario_runtime(&mut model, &mut view, &mut columns);
            if step.action_kind.is_none() {
                drive_scenario_step(&mut model, &mut view, &mut columns, step);
                continue;
            }
            model.begin_scenario_step(&step.source_path);
            drive_scenario_step(&mut model, &mut view, &mut columns, step);
            settle_scenario_runtime(&mut model, &mut view, &mut columns);
            model
                .assert_scenario_step(&boon_runtime::ScenarioStep {
                    id: step.id.clone(),
                    user_action_kind: step.action_kind.clone(),
                    user_action_text: step.text.clone(),
                    user_action_key: step.key.clone(),
                    source_event: None,
                    expectations: step.expectations.clone(),
                })
                .unwrap_or_else(|error| {
                    panic!("Persons scenario step `{}` failed: {error}", step.id)
                });
            completed_actions += 1;
        }
        assert_eq!(
            completed_actions,
            executable_test_steps(&example.test_steps).count()
        );
    }

    #[test]
    fn persons_editor_and_responsive_layout_scenario_hosts_last_valid_child_document() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        assert!(complete_pending_programs_successfully(&mut model) >= 1);
        assert!(model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Your name")
        }));
        assert!(model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Development passkey simulator")
        }));
        let source_paths = model
            .retained_frame()
            .nodes
            .values()
            .flat_map(|node| node.source_bindings.iter())
            .map(|binding| binding.source_path.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for expected in [
            "store.elements.source_editor",
            "store.elements.show_code",
            "store.elements.show_preview",
            "store.elements.register_passkey",
            "store.elements.publish",
            "store.elements.theme_toggle",
        ] {
            assert!(
                source_paths.contains(expected),
                "Persons.pro shell is missing source binding {expected:?}: {source_paths:?}"
            );
        }

        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 1280.0,
                height: 800.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        model.take_patches();

        let surface = SurfaceId("preview".to_owned());
        let source_editor = view
            .target_for_source("store.elements.source_editor", None)
            .expect("Persons.pro source editor target");
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: surface.clone(),
                        x: source_editor.center_x,
                        y: source_editor.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(source_editor.clone()),
                )
                .unwrap();
        }
        let focused = model.focused().expect("source editor focused").to_owned();

        for (logical_key, pressed) in [
            (LogicalKey::Named("Control_L".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), false),
            (LogicalKey::Named("Control_L".to_owned()), false),
        ] {
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key,
                        pressed,
                    }),
                    None,
                )
                .unwrap();
        }
        let invalid_source = "scene: Missing/constructor(\n";
        assert!(
            model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: invalid_source.to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), invalid_source);
        let invalid_request = model
            .take_program_requests()
            .into_iter()
            .next()
            .expect("invalid child revision requested");
        assert_eq!(invalid_request.compile.units[0].source, invalid_source);
        let invalid_artifact = boon_runtime::compile_program_artifact(&invalid_request.compile);
        assert!(invalid_artifact.is_err());
        model
            .complete_program(
                &invalid_request.session,
                &invalid_request.request_id,
                invalid_artifact,
            )
            .unwrap();
        assert_eq!(model.program_diagnostics().len(), 1);
        assert!(model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Your name")
        }));

        for (logical_key, pressed) in [
            (LogicalKey::Named("Control_L".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), false),
            (LogicalKey::Named("Control_L".to_owned()), false),
        ] {
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key,
                        pressed,
                    }),
                    None,
                )
                .unwrap();
        }
        let corrected_source = r#"scene: Scene/Element/text(
    element: []
    style: [width: Fill, height: 40]
    text: TEXT { Corrected page }
)
"#;
        assert!(
            model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface,
                        text: corrected_source.to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), corrected_source);
        let corrected_request = model
            .take_program_requests()
            .into_iter()
            .next()
            .expect("corrected child revision requested");
        assert_eq!(corrected_request.compile.units[0].source, corrected_source);
        let corrected_artifact = boon_runtime::compile_program_artifact(&corrected_request.compile);
        assert!(
            model
                .complete_program(
                    &corrected_request.session,
                    &corrected_request.request_id,
                    corrected_artifact,
                )
                .unwrap()
        );
        complete_pending_programs_successfully(&mut model);
        assert!(model.program_diagnostics().is_empty());
        assert!(model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Corrected page")
        }));

        view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 1280.0,
                height: 800.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        model.take_patches();
        let desktop_text = view
            .scene()
            .text_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>();
        for expected in [
            "persons.pro",
            "Profile source",
            "Draft",
            "Publish",
            "Corrected page",
        ] {
            assert!(
                desktop_text.contains(&expected),
                "missing desktop text {expected:?}: {desktop_text:?}"
            );
        }
        assert_scene_has_no_horizontal_overflow(view.scene(), 1280.0);

        view.resize(
            boon_host::Viewport {
                surface: 1,
                width: 390.0,
                height: 844.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        let mobile_text = view
            .scene()
            .text_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>();
        for expected in [
            "persons.pro",
            "Code",
            "Preview",
            "Publish",
            "Profile source",
        ] {
            assert!(
                mobile_text.contains(&expected),
                "missing mobile text {expected:?}: {mobile_text:?}"
            );
        }
        assert_scene_has_no_horizontal_overflow(view.scene(), 390.0);

        for (source, label) in [
            ("store.elements.register_passkey", "Protect workspace"),
            ("store.elements.theme_toggle", "Dark"),
            ("store.elements.show_preview", "Preview"),
            ("store.elements.show_publish", "Publish"),
        ] {
            assert!(
                view.target_for_source(source, Some(label)).is_some(),
                "narrow Persons layout is missing {label:?} action `{source}`"
            );
        }
        let preview_tab = view
            .target_for_source("store.elements.show_preview", Some("Preview"))
            .unwrap();
        click_target(&mut model, &mut view, &mut columns, preview_tab);
        assert!(
            view.target_for_source("store.elements.show_draft_page", Some("Draft"))
                .is_some()
        );
        assert!(
            view.target_for_source("store.elements.show_published_page", Some("Published"))
                .is_some()
        );
        assert_scene_has_no_horizontal_overflow(view.scene(), 390.0);

        let publish_tab = view
            .target_for_source("store.elements.show_publish", Some("Publish"))
            .unwrap();
        click_target(&mut model, &mut view, &mut columns, publish_tab);
        assert!(
            view.target_for_source("store.elements.publish", Some("Publish"))
                .is_some()
        );
        assert_scene_has_no_horizontal_overflow(view.scene(), 390.0);
    }

    #[test]
    fn persons_publish_recompiles_exact_source_and_preserves_previous_public_pointer_on_failure() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        assert!(complete_pending_programs_successfully(&mut model) >= 1);

        let first_source = "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { First immutable page })\n";
        model
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some(first_source.to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let draft = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-draft")
            .expect("draft compile request");
        model
            .complete_program(
                &draft.session,
                &draft.request_id,
                boon_runtime::compile_program_artifact(&draft.compile),
            )
            .unwrap();
        complete_pending_programs_successfully(&mut model);

        model
            .dispatch_source("store.elements.publish", None, SourcePayload::default())
            .unwrap();
        assert_eq!(
            model
                .runtime
                .inspect_value_current("store.publish_state", 1)
                .unwrap(),
            Value::Text("Building".to_owned())
        );
        let publish = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-candidate")
            .expect("publish compiler request");
        assert_eq!(publish.compile.units[0].source, first_source);
        let first_artifact = boon_runtime::compile_program_artifact(&publish.compile).unwrap();
        let first_digest = first_artifact.source_digest().to_owned();
        let first_artifact_id = first_artifact.id_text();
        let first_plan_digest = first_artifact.plan_digest().to_owned();
        let compiler = first_artifact.compiler_id().to_owned();
        let target = first_artifact.target_profile_id().to_owned();
        let capability = first_artifact.capability_profile_id().to_owned();
        let observation = model
            .complete_program_observed(&publish.session, &publish.request_id, Ok(first_artifact))
            .unwrap();
        assert!(matches!(
            observation.completion,
            ProgramCompletionObservation::ArtifactStorePending { .. }
        ));
        assert!(!observation.changed);
        assert_current_values(
            &mut model,
            &[
                ("store.publish_state", Value::Text("Building".to_owned())),
                ("store.published_revision_count", Value::Number(0)),
                ("store.published_artifact_id", Value::Text(String::new())),
            ],
        );
        assert!(model.runtime.status().accepting_turns);
        assert!(settle_program_artifact_stores(&mut model) >= 1);
        for (path, expected) in [
            ("store.publish_state", Value::Text("Published".to_owned())),
            (
                "store.published_source",
                Value::Text(first_source.to_owned()),
            ),
            ("store.published_digest", Value::Text(first_digest.clone())),
            (
                "store.published_artifact_id",
                Value::Text(first_artifact_id.clone()),
            ),
            (
                "store.published_plan_digest",
                Value::Text(first_plan_digest.clone()),
            ),
            ("store.published_revision_count", Value::Number(1)),
        ] {
            assert_eq!(
                model.runtime.inspect_value_current(path, 8).unwrap(),
                expected,
                "unexpected published authority at {path}"
            );
        }
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.source"),
            vec![Value::Text(first_source.to_owned())]
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.source_digest"),
            vec![Value::Text(first_digest.clone())]
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.artifact_id"),
            vec![Value::Text(first_artifact_id)]
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.plan_digest"),
            vec![Value::Text(first_plan_digest)]
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.compiler"),
            vec![Value::Text(compiler.clone())]
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.target"),
            vec![Value::Text(target.clone())]
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.capability_profile",),
            vec![Value::Text(capability.clone())]
        );

        let second_source = "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { Second immutable page })\n";
        model
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some(second_source.to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let second_draft = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-draft")
            .expect("second draft compile request");
        model
            .complete_program(
                &second_draft.session,
                &second_draft.request_id,
                boon_runtime::compile_program_artifact(&second_draft.compile),
            )
            .unwrap();
        complete_pending_programs_successfully(&mut model);
        model
            .dispatch_source("store.elements.publish", None, SourcePayload::default())
            .unwrap();
        let second_publish = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-candidate")
            .expect("second publish compiler request");
        assert_eq!(second_publish.compile.units[0].source, second_source);
        let second_artifact =
            boon_runtime::compile_program_artifact(&second_publish.compile).unwrap();
        let second_digest = second_artifact.source_digest().to_owned();
        let second_artifact_id = second_artifact.id_text();
        let second_plan_digest = second_artifact.plan_digest().to_owned();
        model
            .complete_program(
                &second_publish.session,
                &second_publish.request_id,
                Ok(second_artifact),
            )
            .unwrap();
        assert!(settle_program_artifact_stores(&mut model) >= 1);
        assert_current_values(
            &mut model,
            &[
                ("store.publish_state", Value::Text("Published".to_owned())),
                (
                    "store.published_source",
                    Value::Text(second_source.to_owned()),
                ),
                ("store.published_digest", Value::Text(second_digest.clone())),
                (
                    "store.published_artifact_id",
                    Value::Text(second_artifact_id.clone()),
                ),
                (
                    "store.published_plan_digest",
                    Value::Text(second_plan_digest.clone()),
                ),
                ("store.published_revision_count", Value::Number(2)),
            ],
        );
        let archive_sources = vec![
            Value::Text(first_source.to_owned()),
            Value::Text(second_source.to_owned()),
        ];
        let archive_digests = vec![
            Value::Text(first_digest.clone()),
            Value::Text(second_digest.clone()),
        ];
        let archive_compilers = vec![Value::Text(compiler.clone()), Value::Text(compiler)];
        let archive_targets = vec![Value::Text(target.clone()), Value::Text(target)];
        let archive_capabilities = vec![Value::Text(capability.clone()), Value::Text(capability)];
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.source"),
            archive_sources
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.source_digest"),
            archive_digests
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.compiler"),
            archive_compilers
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.target"),
            archive_targets
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.capability_profile",),
            archive_capabilities
        );

        model
            .dispatch_source(
                "store.elements.show_published_page",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        assert!(
            model.pending_program_requests.iter().any(|request| {
                request.session.0 == "persons-public-published" && request.is_artifact_load()
            }),
            "published page must request its immutable artifact"
        );
        model.resolve_program_artifact_requests().unwrap();
        assert!(
            model
                .take_program_requests()
                .iter()
                .all(|request| request.session.0 != "persons-public-published"),
            "published artifact load must not escape to the compile worker"
        );
        assert_eq!(
            model
                .program_host
                .active_artifact(&ProgramSessionId("persons-public-published".to_owned()))
                .map(ProgramArtifact::id_text),
            Some(second_artifact_id)
        );
        assert!(model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Second immutable page")
        }));

        let invalid_source = "scene: Missing/constructor(\n";
        model
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some(invalid_source.to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let invalid_draft = model.take_program_requests().remove(0);
        model
            .complete_program(
                &invalid_draft.session,
                &invalid_draft.request_id,
                boon_runtime::compile_program_artifact(&invalid_draft.compile),
            )
            .unwrap();
        model
            .dispatch_source("store.elements.publish", None, SourcePayload::default())
            .unwrap();
        let failed_publish = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-candidate")
            .expect("failed publish compiler request");
        let diagnostic = boon_runtime::compile_program_artifact(&failed_publish.compile)
            .expect_err("invalid publication source must fail");
        assert_eq!(diagnostic.source_path, "RUN.bn");
        assert!(diagnostic.line > 0);
        model
            .complete_program(
                &failed_publish.session,
                &failed_publish.request_id,
                Err(diagnostic),
            )
            .unwrap();
        for (path, expected) in [
            ("store.publish_state", Value::Text("Failed".to_owned())),
            (
                "store.published_source",
                Value::Text(second_source.to_owned()),
            ),
            ("store.published_digest", Value::Text(second_digest.clone())),
            ("store.published_revision_count", Value::Number(2)),
        ] {
            assert_eq!(
                model.runtime.inspect_value_current(path, 8).unwrap(),
                expected,
                "failed publish changed public authority at {path}"
            );
        }
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.source"),
            archive_sources
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.source_digest"),
            archive_digests
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.compiler"),
            archive_compilers
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.target"),
            archive_targets
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.capability_profile",),
            archive_capabilities
        );
        assert!(model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Second immutable page")
        }));
    }

    #[test]
    fn persons_publish_rejects_out_of_order_candidate_completion() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        complete_pending_programs_successfully(&mut model);

        let older_source = "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { Older publication })\n";
        let newer_source = "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { Newer publication })\n";
        let publish_request = |model: &mut RuntimeView, source: &str| {
            model
                .dispatch_source(
                    "store.elements.source_editor",
                    None,
                    SourcePayload {
                        text: Some(source.to_owned()),
                        ..SourcePayload::default()
                    },
                )
                .unwrap();
            let draft = model
                .take_program_requests()
                .into_iter()
                .find(|request| request.session.0 == "persons-public-draft")
                .expect("draft compile request");
            model
                .complete_program(
                    &draft.session,
                    &draft.request_id,
                    boon_runtime::compile_program_artifact(&draft.compile),
                )
                .unwrap();
            complete_pending_programs_successfully(model);
            model
                .dispatch_source("store.elements.publish", None, SourcePayload::default())
                .unwrap();
            model
                .take_program_requests()
                .into_iter()
                .find(|request| request.session.0 == "persons-public-candidate")
                .expect("publish candidate request")
        };

        let older = publish_request(&mut model, older_source);
        let newer = publish_request(&mut model, newer_source);
        assert_ne!(older.request_id, newer.request_id);
        model
            .complete_program(
                &newer.session,
                &newer.request_id,
                boon_runtime::compile_program_artifact(&newer.compile),
            )
            .unwrap();
        settle_program_artifact_stores(&mut model);
        let stale = model
            .complete_program_observed(
                &older.session,
                &older.request_id,
                boon_runtime::compile_program_artifact(&older.compile),
            )
            .unwrap();
        assert!(!stale.changed);
        assert_eq!(
            stale.completion,
            ProgramCompletionObservation::Host(ProgramHostCompletion::Removed {
                session: older.session,
            })
        );
        assert_current_values(
            &mut model,
            &[
                (
                    "store.published_source",
                    Value::Text(newer_source.to_owned()),
                ),
                ("store.published_revision_count", Value::Number(1)),
            ],
        );
        assert_eq!(
            inspected_list_field_values(&mut model, "store.published_revisions.source"),
            vec![Value::Text(newer_source.to_owned())]
        );
    }

    #[test]
    fn persons_draft_compilation_is_latest_wins_without_preview_regression() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        complete_pending_programs_successfully(&mut model);

        model
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some("scene: Missing/constructor(\n".to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let invalid = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-draft")
            .expect("invalid draft request before stale completion exercise");
        model
            .complete_program(
                &invalid.session,
                &invalid.request_id,
                boon_runtime::compile_program_artifact(&invalid.compile),
            )
            .unwrap();
        assert_eq!(model.program_diagnostics().len(), 1);

        let older_source = "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { Older draft })\n";
        let newer_source = "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { Newer draft })\n";
        model
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some(older_source.to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let older = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-draft")
            .expect("older draft request");

        model
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some(newer_source.to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let newer = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-draft")
            .expect("newer draft request");
        assert_ne!(newer.request_id, older.request_id);

        let stale = model
            .complete_program_observed(
                &older.session,
                &older.request_id,
                boon_runtime::compile_program_artifact(&older.compile),
            )
            .unwrap();
        assert!(!stale.changed);
        assert_eq!(
            stale.completion,
            ProgramCompletionObservation::Host(ProgramHostCompletion::Superseded {
                session: older.session.clone(),
                request_id: older.request_id.clone(),
            })
        );
        assert_ne!(
            model
                .runtime
                .inspect_value_current("store.last_valid_draft_source", 1)
                .unwrap(),
            Value::Text(older_source.to_owned())
        );

        model
            .complete_program(
                &newer.session,
                &newer.request_id,
                boon_runtime::compile_program_artifact(&newer.compile),
            )
            .unwrap();
        complete_pending_programs_successfully(&mut model);
        assert_eq!(
            model
                .runtime
                .inspect_value_current("store.last_valid_draft_source", 1)
                .unwrap(),
            Value::Text(newer_source.to_owned())
        );
        assert!(model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Newer draft")
        }));
        assert!(!model.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Older draft")
        }));
    }

    #[test]
    fn persons_profile_edit_reports_bounded_dependency_breadth() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        complete_pending_programs_successfully(&mut model);
        model.take_patches();

        let before = model.runtime.runtime().document_materialization_stats();
        let generation = model.parent_runtime_generation();
        model
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some(
                        "scene: Scene/Element/text(element: [], style: [width: Fill, height: Fill], text: TEXT { Profile dependency breadth })\n"
                            .to_owned(),
                    ),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let after_edit = model.runtime.runtime().document_materialization_stats();
        let request = model
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-draft")
            .unwrap();
        model
            .complete_program(
                &request.session,
                &request.request_id,
                boon_runtime::compile_program_artifact(&request.compile),
            )
            .unwrap();
        let after_completion = model.runtime.runtime().document_materialization_stats();
        let patches = model.take_patches();
        let structural = patches
            .iter()
            .filter(|patch| {
                matches!(
                    patch,
                    DocumentPatch::UpsertNode(_)
                        | DocumentPatch::RemoveNode { .. }
                        | DocumentPatch::InsertChild { .. }
                        | DocumentPatch::RemoveChild { .. }
                        | DocumentPatch::MoveChild { .. }
                )
            })
            .count();
        assert_eq!(model.parent_runtime_generation(), generation);
        assert_eq!(
            after_edit
                .full_evaluation_count
                .saturating_sub(before.full_evaluation_count),
            1
        );
        assert_eq!(
            after_completion
                .full_evaluation_count
                .saturating_sub(after_edit.full_evaluation_count),
            1
        );
        assert!(patches.len() <= 24);
        assert!(structural <= 4);
    }

    #[test]
    fn persons_restart_restores_authority_before_the_first_frame_and_rebuilds_diagnostics() {
        let state_root = unique_state_root("persons-restart-first-frame");
        let plan = persons_plan(1, None, None);
        let valid_source = "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { Restart-safe page })\n";
        let invalid_source = "scene: Missing/constructor(\n";

        let mut first = RuntimeView::open_with_state_root(Arc::clone(&plan), &state_root).unwrap();
        complete_pending_programs_successfully(&mut first);
        let workspace_id = first
            .runtime
            .inspect_value_current("store.workspace_id", 1)
            .unwrap();
        for _ in 0..2 {
            dispatch_effect_action(&mut first, "store.elements.register_passkey");
        }
        let account_id = first
            .runtime
            .inspect_value_current("store.account_id", 1)
            .unwrap();

        first
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some(valid_source.to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let (activated, rejected) = complete_pending_programs(&mut first);
        assert!(activated >= 1);
        assert_eq!(rejected, 0);
        first
            .dispatch_source("store.elements.publish", None, SourcePayload::default())
            .unwrap();
        let publish = first
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-candidate")
            .expect("publish candidate request");
        first
            .complete_program(
                &publish.session,
                &publish.request_id,
                boon_runtime::compile_program_artifact(&publish.compile),
            )
            .unwrap();
        settle_program_artifact_stores(&mut first);
        let published_digest = boon_runtime::sha256_bytes(valid_source.as_bytes());

        first
            .dispatch_source(
                "store.elements.source_editor",
                None,
                SourcePayload {
                    text: Some(invalid_source.to_owned()),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        let (activated, rejected) = complete_pending_programs(&mut first);
        assert_eq!(activated, 0);
        assert_eq!(rejected, 1);
        assert_current_values(
            &mut first,
            &[
                ("store.source_draft", Value::Text(invalid_source.to_owned())),
                (
                    "store.last_valid_draft_source",
                    Value::Text(valid_source.to_owned()),
                ),
                (
                    "store.published_source",
                    Value::Text(valid_source.to_owned()),
                ),
                (
                    "store.published_digest",
                    Value::Text(published_digest.clone()),
                ),
                ("store.published_revision_count", Value::Number(1)),
                ("store.account_id", account_id.clone()),
                ("store.credential_count", Value::Number(2)),
            ],
        );
        let acknowledged_authority = authoritative_semantic_snapshot(&mut first, &plan);
        let artifact = first.export_state_artifact().unwrap();
        first.runtime.shutdown().unwrap();
        drop(first);

        let mut restored = RuntimeView::open_with_state_root(Arc::clone(&plan), &state_root)
            .expect("restore acknowledged Persons authority");
        assert_eq!(
            authoritative_semantic_snapshot(&mut restored, &plan),
            acknowledged_authority,
            "restart changed acknowledged semantic authority before the first frame"
        );
        assert_current_values(
            &mut restored,
            &[
                ("store.source_draft", Value::Text(invalid_source.to_owned())),
                (
                    "store.last_valid_draft_source",
                    Value::Text(valid_source.to_owned()),
                ),
                (
                    "store.published_source",
                    Value::Text(valid_source.to_owned()),
                ),
                ("store.published_digest", Value::Text(published_digest)),
                ("store.published_revision_count", Value::Number(1)),
                ("store.account_id", account_id),
                ("store.workspace_id", workspace_id.clone()),
                ("store.credential_count", Value::Number(2)),
                ("store.account_state", Value::Text("TwoPasskeys".to_owned())),
                ("store.publish_state", Value::Text("Published".to_owned())),
                (
                    "store.passkey_workflow_state",
                    Value::Text("Registered".to_owned()),
                ),
            ],
        );
        let first_frame = restored.frame();
        assert!(first_frame.nodes.values().any(|node| {
            node.kind == DocumentNodeKind::TextInput
                && node
                    .source_bindings
                    .iter()
                    .any(|binding| binding.source_path == "store.elements.source_editor")
                && node
                    .text
                    .as_ref()
                    .is_some_and(|text| text.text == invalid_source)
        }));
        assert!(first_frame.nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "2 passkeys")
        }));
        assert!(restored.program_diagnostics().is_empty());

        let (activated, rejected) = complete_pending_programs(&mut restored);
        assert!(activated >= 1);
        assert_eq!(rejected, 1);
        assert_eq!(restored.program_diagnostics().len(), 1);
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.draft_compile_state", 1)
                .unwrap(),
            Value::Text("Invalid".to_owned())
        );
        assert!(restored.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Restart-safe page")
        }));

        restored.start_over().unwrap();
        assert_current_values(
            &mut restored,
            &[
                ("store.account_id", Value::Text(String::new())),
                ("store.account_state", Value::Text("Anonymous".to_owned())),
                ("store.credential_count", Value::Number(0)),
                ("store.published_revision", Value::Number(0)),
                ("store.published_revision_count", Value::Number(0)),
            ],
        );
        assert_ne!(
            restored
                .runtime
                .inspect_value_current("store.workspace_id", 1)
                .unwrap(),
            workspace_id
        );
        let preview = restored.preview_state_artifact(&artifact).unwrap();
        assert_eq!(preview.source_schema_version, 1);
        assert_eq!(preview.target_schema_version, 1);
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.account_state", 1)
                .unwrap(),
            Value::Text("Anonymous".to_owned()),
            "Import Preview mutated active account authority"
        );
        restored.activate_state_artifact(&artifact).unwrap();
        assert_eq!(
            authoritative_semantic_snapshot(&mut restored, &plan),
            acknowledged_authority,
            "import activation did not restore the complete acknowledged authority"
        );
        assert_current_values(
            &mut restored,
            &[
                ("store.workspace_id", workspace_id),
                ("store.source_draft", Value::Text(invalid_source.to_owned())),
                (
                    "store.last_valid_draft_source",
                    Value::Text(valid_source.to_owned()),
                ),
                (
                    "store.published_source",
                    Value::Text(valid_source.to_owned()),
                ),
                ("store.published_revision_count", Value::Number(1)),
                ("store.account_state", Value::Text("TwoPasskeys".to_owned())),
                ("store.credential_count", Value::Number(2)),
            ],
        );

        restored.runtime.shutdown().unwrap();
        drop(restored);
        fs::remove_dir_all(state_root).unwrap();
    }

    #[test]
    fn persons_restart_resumes_only_unsettled_publish_work() {
        let state_root = unique_state_root("persons-publish-resume");
        let plan = persons_plan(1, None, None);

        let mut first = RuntimeView::open_with_state_root(Arc::clone(&plan), &state_root).unwrap();
        complete_pending_programs_successfully(&mut first);
        first
            .dispatch_source("store.elements.publish", None, SourcePayload::default())
            .unwrap();
        assert!(
            first
                .take_program_requests()
                .into_iter()
                .any(|request| request.session.0 == "persons-public-candidate")
        );
        assert_eq!(
            first
                .runtime
                .inspect_value_current("store.publish_state", 1)
                .unwrap(),
            Value::Text("Building".to_owned())
        );
        first.runtime.barrier().unwrap();
        first.runtime.shutdown().unwrap();
        drop(first);

        let mut resumed =
            RuntimeView::open_with_state_root(Arc::clone(&plan), &state_root).unwrap();
        let candidate = resumed
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-candidate")
            .expect("an unsettled publish is resumed after restart");
        resumed
            .complete_program(
                &candidate.session,
                &candidate.request_id,
                boon_runtime::compile_program_artifact(&candidate.compile),
            )
            .unwrap();
        settle_program_artifact_stores(&mut resumed);
        assert_current_values(
            &mut resumed,
            &[
                ("store.publish_state", Value::Text("Published".to_owned())),
                ("store.published_revision_count", Value::Number(1)),
            ],
        );
        resumed.runtime.barrier().unwrap();
        resumed.runtime.shutdown().unwrap();
        drop(resumed);

        let mut settled =
            RuntimeView::open_with_state_root(Arc::clone(&plan), &state_root).unwrap();
        assert!(
            settled
                .take_program_requests()
                .into_iter()
                .all(|request| request.session.0 != "persons-public-candidate"),
            "a settled publish must not replay after restart"
        );
        assert_current_values(
            &mut settled,
            &[
                ("store.publish_state", Value::Text("Published".to_owned())),
                ("store.published_revision_count", Value::Number(1)),
            ],
        );
        settled.runtime.shutdown().unwrap();
        drop(settled);
        fs::remove_dir_all(state_root).unwrap();
    }

    #[test]
    fn persons_host_control_scenario_covers_restart_clear_export_import_corruption_and_migration() {
        let state_root = unique_state_root("persons-redb-lifecycle");
        let plan_v1 = persons_plan(1, None, None);
        let edited_source = "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { Durable profile })\n";

        let mut first = RuntimeView::open_with_state_root(Arc::clone(&plan_v1), &state_root)
            .expect("open isolated Persons.pro redb store");
        let workspace_id = match first
            .runtime
            .inspect_value_current("store.workspace_id", 1)
            .unwrap()
        {
            Value::Text(value) => value,
            value => panic!("workspace identity is not Text: {value:?}"),
        };
        assert_eq!(
            uuid::Uuid::parse_str(&workspace_id)
                .expect("interactive workspace identity is a UUID")
                .get_version_num(),
            4
        );
        assert!(complete_pending_programs_successfully(&mut first) >= 1);
        assert_eq!(
            first
                .runtime
                .inspect_value_current("store.draft_compile_state", 1)
                .unwrap(),
            Value::Text("Ready".to_owned())
        );

        assert!(
            first
                .dispatch_source(
                    "store.elements.source_editor",
                    None,
                    SourcePayload {
                        text: Some(edited_source.to_owned()),
                        ..SourcePayload::default()
                    },
                )
                .unwrap()
        );
        let edited = first
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-draft")
            .expect("edited draft compile request");
        first
            .complete_program(
                &edited.session,
                &edited.request_id,
                boon_runtime::compile_program_artifact(&edited.compile),
            )
            .unwrap();
        complete_pending_programs_successfully(&mut first);
        let digest = boon_runtime::sha256_bytes(edited_source.as_bytes());
        assert_eq!(
            first
                .runtime
                .inspect_value_current("store.draft_compile_digest", 1)
                .unwrap(),
            Value::Text(digest.clone())
        );
        for _ in 0..2 {
            dispatch_effect_action(&mut first, "store.elements.register_passkey");
        }
        first
            .dispatch_source("store.elements.publish", None, SourcePayload::default())
            .unwrap();
        let publish = first
            .take_program_requests()
            .into_iter()
            .find(|request| request.session.0 == "persons-public-candidate")
            .expect("migration fixture publish candidate request");
        first
            .complete_program(
                &publish.session,
                &publish.request_id,
                boon_runtime::compile_program_artifact(&publish.compile),
            )
            .unwrap();
        settle_program_artifact_stores(&mut first);
        assert_current_values(
            &mut first,
            &[
                ("store.account_state", Value::Text("TwoPasskeys".to_owned())),
                ("store.credential_count", Value::Number(2)),
                ("store.published_revision_count", Value::Number(1)),
                ("store.published_digest", Value::Text(digest.clone())),
            ],
        );
        let acknowledged_authority = authoritative_semantic_snapshot(&mut first, &plan_v1);
        first.runtime.barrier().unwrap();
        let artifact = first.export_state_artifact().unwrap();
        first.runtime.shutdown().unwrap();
        drop(first);

        let mut restored = RuntimeView::open_with_state_root(Arc::clone(&plan_v1), &state_root)
            .expect("reopen acknowledged Persons.pro state");
        assert_eq!(
            authoritative_semantic_snapshot(&mut restored, &plan_v1),
            acknowledged_authority,
            "restart changed complete acknowledged authority"
        );
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.source_draft", 1)
                .unwrap(),
            Value::Text(edited_source.to_owned())
        );
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.draft_compile_digest", 1)
                .unwrap(),
            Value::Text(digest.clone())
        );
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.workspace_id", 1)
                .unwrap(),
            Value::Text(workspace_id.clone())
        );
        assert_eq!(restored.take_program_requests().len(), 1);

        let preview = restored.preview_state_artifact(&artifact).unwrap();
        assert_eq!(preview.source_schema_version, 1);
        assert_eq!(preview.target_schema_version, 1);
        assert!(preview.scalar_count > 0);
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.source_draft", 1)
                .unwrap(),
            Value::Text(edited_source.to_owned()),
            "Import Preview must not mutate active authority"
        );

        let mut corrupt_artifact = artifact.clone();
        corrupt_artifact.push(0);
        assert!(restored.preview_state_artifact(&corrupt_artifact).is_err());
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.source_draft", 1)
                .unwrap(),
            Value::Text(edited_source.to_owned())
        );

        restored.clear_authority_path("store.source_draft").unwrap();
        assert_ne!(
            restored
                .runtime
                .inspect_value_current("store.source_draft", 1)
                .unwrap(),
            Value::Text(edited_source.to_owned())
        );
        restored.activate_state_artifact(&artifact).unwrap();
        assert_eq!(
            authoritative_semantic_snapshot(&mut restored, &plan_v1),
            acknowledged_authority,
            "import activation did not restore complete acknowledged authority"
        );
        assert!(restored.runtime_turn_sequence() > 0);
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.source_draft", 1)
                .unwrap(),
            Value::Text(edited_source.to_owned())
        );
        restored.start_over().unwrap();
        let start_over_workspace_id = restored
            .runtime
            .inspect_value_current("store.workspace_id", 1)
            .unwrap();
        assert_ne!(start_over_workspace_id, Value::Text(workspace_id.clone()));
        assert_ne!(
            restored
                .runtime
                .inspect_value_current("store.source_draft", 1)
                .unwrap(),
            Value::Text(edited_source.to_owned())
        );
        restored.activate_state_artifact(&artifact).unwrap();
        assert_eq!(
            authoritative_semantic_snapshot(&mut restored, &plan_v1),
            acknowledged_authority,
            "start-over followed by import did not restore complete authority"
        );
        assert!(restored.runtime_turn_sequence() > 0);
        assert_eq!(
            restored
                .runtime
                .inspect_value_current("store.workspace_id", 1)
                .unwrap(),
            Value::Text(workspace_id.clone())
        );
        restored.runtime.barrier().unwrap();
        restored.runtime.shutdown().unwrap();
        drop(restored);

        let plan_v2 = persons_plan(
            2,
            Some(&plan_v1),
            Some("migration_probe: TEXT { v2 } |> HOLD migration_probe { LATEST {} }\n"),
        );
        let mut migrated = RuntimeView::open_with_state_root(Arc::clone(&plan_v2), &state_root)
            .expect("migrate Persons.pro state to additive v2 schema");
        assert_eq!(migrated.persistence_schema_version(), 2);
        assert!(
            migrated
                .persistence_inspector
                .as_ref()
                .is_some_and(|inspector| inspector.completed_migration_count >= 1),
            "additive Persons migration was not recorded by the durable store"
        );
        assert_eq!(
            authoritative_semantic_snapshot(&mut migrated, &plan_v1),
            acknowledged_authority,
            "schema migration changed pre-existing semantic authority"
        );
        assert_eq!(
            migrated
                .runtime
                .inspect_value_current("store.source_draft", 1)
                .unwrap(),
            Value::Text(edited_source.to_owned())
        );
        migrated.runtime.barrier().unwrap();
        migrated.runtime.shutdown().unwrap();
        drop(migrated);

        let mut reopened_v2 = RuntimeView::open_with_state_root(Arc::clone(&plan_v2), &state_root)
            .expect("reopen migrated v2 state");
        assert_eq!(
            authoritative_semantic_snapshot(&mut reopened_v2, &plan_v1),
            acknowledged_authority,
            "reopening the migrated store changed pre-existing semantic authority"
        );
        assert_eq!(
            reopened_v2
                .runtime
                .inspect_value_current("store.source_draft", 1)
                .unwrap(),
            Value::Text(edited_source.to_owned())
        );
        reopened_v2.runtime.shutdown().unwrap();
        drop(reopened_v2);

        let database_path =
            state_database_path_in(&state_root, &plan_v2.application.identity).unwrap();
        let corrupt_database = b"not-a-redb-database".to_vec();
        fs::write(&database_path, &corrupt_database).unwrap();
        assert!(RuntimeView::open_with_state_root(plan_v2, &state_root).is_err());
        assert_eq!(fs::read(&database_path).unwrap(), corrupt_database);
        fs::remove_dir_all(&state_root).unwrap();
    }

    #[test]
    fn kavik_portfolio_reflows_without_horizontal_overflow() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("kavik_cz")
            .unwrap();
        assert!(
            example.assets.len() >= 70,
            "portfolio asset bundle is incomplete"
        );
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/kavik_cz/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 1440.0,
                height: 900.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();

        let visible_text = view
            .scene()
            .text_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>();
        assert!(
            visible_text.contains(&"Martin Kavik"),
            "initial portfolio text runs: {visible_text:?}"
        );
        assert!(view.scene().visual_primitives.iter().any(|primitive| {
            matches!(
                &primitive.texture,
                boon_document::RenderTextureRef::Asset { url, .. }
                    if url == "asset://kavik_cz/images/martin_coffee.webp"
            )
        }));
        assert_scene_has_no_horizontal_overflow(view.scene(), 1440.0);
        let desktop_intro_height = view
            .scene()
            .text_runs
            .iter()
            .find(|run| run.text.starts_with("15+ years building"))
            .expect("desktop hero introduction should be visible")
            .bounds
            .height;

        for (width, height) in [
            (320.0, 640.0),
            (390.0, 844.0),
            (699.0, 800.0),
            (700.0, 800.0),
            (768.0, 1024.0),
            (1024.0, 768.0),
        ] {
            view.resize(
                boon_host::Viewport {
                    surface: 1,
                    width,
                    height,
                    scale: 1.0,
                },
                &mut columns,
            )
            .unwrap();
            assert_scene_has_no_horizontal_overflow(view.scene(), width);
            assert_eq!(
                view.scene()
                    .text_runs
                    .iter()
                    .filter(|run| run.text == "Home")
                    .count(),
                1,
                "exactly one responsive navigation should be visible at {width}x{height}"
            );
        }

        view.resize(
            boon_host::Viewport {
                surface: 1,
                width: 390.0,
                height: 844.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        assert_scene_has_no_horizontal_overflow(view.scene(), 390.0);
        let mobile_text = view
            .scene()
            .text_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>();
        let home_nodes = view
            .frame()
            .nodes
            .values()
            .filter(|node| node.text.as_ref().is_some_and(|text| text.text == "Home"))
            .map(|node| (node.id.0.clone(), view.node_bounds(&node.id.0)))
            .collect::<Vec<_>>();
        assert_eq!(
            mobile_text.iter().filter(|text| **text == "Home").count(),
            1,
            "only the mobile or desktop navigation may be materialized: text={mobile_text:?}, nodes={home_nodes:?}"
        );
        let mobile_intro = view
            .scene()
            .text_runs
            .iter()
            .find(|run| run.text.starts_with("15+ years building"))
            .expect("mobile hero introduction should be visible");
        assert!(mobile_intro.wrap);
        assert!(mobile_intro.bounds.width <= 354.0);
        assert!(mobile_intro.bounds.height >= desktop_intro_height);

        let about = view
            .target_for_source("store.elements.nav.about", Some("About"))
            .unwrap_or_else(|| {
                let routes = view
                    .frame()
                    .nodes
                    .values()
                    .flat_map(|node| {
                        node.source_bindings
                            .iter()
                            .map(move |binding| (node.id.0.as_str(), binding.source_path.as_str()))
                    })
                    .collect::<Vec<_>>();
                panic!("mobile About navigation target; routes={routes:?}")
            });
        click_target(&mut model, &mut view, &mut columns, about);
        let media = view
            .frame()
            .nodes
            .values()
            .find(|node| node.kind == DocumentNodeKind::EmbeddedMedia)
            .expect("About route should contain semantic embedded media")
            .clone();
        assert_eq!(
            media.style.get("provider"),
            Some(&StyleValue::Text("youtube".to_owned()))
        );
        assert!(matches!(
            media.style.get("embed_url"),
            Some(StyleValue::Text(url)) if url.starts_with("https://www.youtube-nocookie.com/embed/")
        ));
        assert_eq!(
            media.style.get("playback"),
            Some(&StyleValue::Text("user_activated".to_owned()))
        );
        assert_eq!(
            media.style.get("privacy_mode"),
            Some(&StyleValue::Bool(true))
        );
        assert!(matches!(
            media.style.get("sandbox"),
            Some(StyleValue::Text(policy))
                if policy.contains("allow-scripts") && policy.contains("allow-presentation")
        ));
        assert_eq!(
            media.style.get("referrer_policy"),
            Some(&StyleValue::Text(
                "strict-origin-when-cross-origin".to_owned()
            ))
        );
        assert_eq!(
            media.style.get("allow_fullscreen"),
            Some(&StyleValue::Bool(true))
        );
        let bounds = view.node_bounds(&media.id.0).unwrap();
        click_target(
            &mut model,
            &mut view,
            &mut columns,
            HitTarget {
                node: media.id.0,
                source_path: None,
                source_intent: None,
                row_key: None,
                row_generation: None,
                scroll_root: None,
                center_x: bounds.x + bounds.width / 2.0,
                center_y: bounds.y + bounds.height / 2.0,
                bounds_x: bounds.x,
                bounds_y: bounds.y,
                bounds_width: bounds.width,
                bounds_height: bounds.height,
                text_line: None,
                text_column: None,
            },
        );
        assert!(
            model
                .take_external_url()
                .is_some_and(|url| url.starts_with("https://www.youtube.com/watch?v="))
        );
    }

    #[test]
    fn kavik_portfolio_renders_desktop_and_mobile_app_owned_pixels() {
        futures::executor::block_on(async {
            let example = crate::catalog::Catalog::load()
                .unwrap()
                .open("kavik_cz")
                .unwrap();
            let sources = example
                .assets
                .iter()
                .cloned()
                .map(|asset| boon_native_gpu::RenderAssetSource {
                    url: asset.url,
                    media_type: asset.media_type,
                    sha256: asset.sha256,
                    bytes: asset.bytes.into(),
                })
                .collect::<Vec<_>>();
            let units = example
                .units
                .into_iter()
                .map(|unit| RuntimeSourceUnit {
                    path: unit.path,
                    source: unit.source,
                })
                .collect::<Vec<_>>();
            let runtime = LiveRuntime::from_project("examples/kavik_cz/RUN.bn", &units).unwrap();
            let mut model = RuntimeView::open_in_memory(runtime).unwrap();
            let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
            let mut view = crate::view::RetainedView::new(
                model.frame(),
                boon_host::Viewport {
                    surface: 1,
                    width: 1440.0,
                    height: 900.0,
                    scale: 1.0,
                },
                &mut columns,
            )
            .unwrap();

            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
                .expect("kavik.cz visual verification requires a WGPU adapter");
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("kavik-cz-responsive-proof-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults()
                        .using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::default(),
                    experimental_features: wgpu::ExperimentalFeatures::default(),
                })
                .await
                .unwrap();
            let mut renderer = boon_native_gpu::AppOwnedProofRenderer::new(&device, &queue);
            renderer.replace_asset_sources(sources).unwrap();
            let artifact_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("target/artifacts/native-gpu/kavik-cz-responsive");

            let desktop = renderer
                .render_scene_pixels(boon_native_gpu::AppOwnedRenderSceneRequest {
                    device: &device,
                    queue: &queue,
                    scene: view.scene(),
                    render_identity_hash: "kavik-cz-desktop-1440x900-v1",
                    surface_id: SurfaceId("kavik-cz-desktop".to_owned()),
                    surface_epoch: 1,
                    width: 1440,
                    height: 900,
                    artifact_dir: &artifact_dir,
                    artifact_label: "desktop-1440x900",
                })
                .unwrap();
            assert_kavik_proof(&desktop, 1440, 900);
            assert!(desktop.metrics.asset_ref_count >= 1);
            assert!(desktop.metrics.asset_decode_count >= 1);
            assert!(desktop.metrics.asset_upload_count >= 1);

            view.resize(
                boon_host::Viewport {
                    surface: 1,
                    width: 390.0,
                    height: 844.0,
                    scale: 1.0,
                },
                &mut columns,
            )
            .unwrap();
            let mobile = renderer
                .render_scene_pixels(boon_native_gpu::AppOwnedRenderSceneRequest {
                    device: &device,
                    queue: &queue,
                    scene: view.scene(),
                    render_identity_hash: "kavik-cz-mobile-390x844-v1",
                    surface_id: SurfaceId("kavik-cz-mobile".to_owned()),
                    surface_epoch: 1,
                    width: 390,
                    height: 844,
                    artifact_dir: &artifact_dir,
                    artifact_label: "mobile-390x844",
                })
                .unwrap();
            assert_kavik_proof(&mobile, 390, 844);
            assert!(mobile.metrics.asset_ref_count >= 1);

            assert!(
                model
                    .dispatch_source("store.elements.nav.about", None, SourcePayload::default(),)
                    .unwrap()
            );
            view.apply_patches(model.take_patches(), &mut columns)
                .unwrap();
            let media_viewport = boon_host::Viewport {
                surface: 1,
                width: 960.0,
                height: 720.0,
                scale: 1.0,
            };
            view.resize(media_viewport, &mut columns).unwrap();
            let media_node = view
                .frame()
                .nodes
                .values()
                .find(|node| node.kind == DocumentNodeKind::EmbeddedMedia)
                .expect("About route media node")
                .id
                .clone();
            let media_y = view.node_bounds(&media_node.0).unwrap().y;
            let root = view.frame().root.0.clone();
            model.scroll_offsets.insert(
                root,
                boon_document_model::ScrollState {
                    x: 0.0,
                    y: (media_y - 110.0).max(0.0),
                },
            );
            view.replace(model.frame(), media_viewport, &mut columns)
                .unwrap();
            assert!(view.scene().items.iter().any(|item| {
                item.node == media_node && item.source_kind == DocumentNodeKind::EmbeddedMedia
            }));
            assert!(view.scene().text_runs.iter().any(|run| run.text == "Play"));
            let media = renderer
                .render_scene_pixels(boon_native_gpu::AppOwnedRenderSceneRequest {
                    device: &device,
                    queue: &queue,
                    scene: view.scene(),
                    render_identity_hash: "kavik-cz-media-fallback-960x720-v1",
                    surface_id: SurfaceId("kavik-cz-media-fallback".to_owned()),
                    surface_epoch: 1,
                    width: 960,
                    height: 720,
                    artifact_dir: &artifact_dir,
                    artifact_label: "media-fallback-960x720",
                })
                .unwrap();
            assert_kavik_proof(&media, 960, 720);
            assert!(media.metrics.asset_ref_count >= 1);
        });
    }

    fn assert_kavik_proof(proof: &boon_native_gpu::RenderProof, width: u32, height: u32) {
        let boon_native_gpu::RenderProofArtifact::AppOwnedPixels {
            artifact_path,
            width: actual_width,
            height: actual_height,
            nonblank_samples,
            unique_rgba_values,
            ..
        } = &proof.artifact
        else {
            panic!("expected app-owned pixel proof")
        };
        assert_eq!((*actual_width, *actual_height), (width, height));
        assert!(std::path::Path::new(artifact_path).is_file());
        assert!(*nonblank_samples >= 8);
        assert!(*unique_rgba_values >= 8);
    }

    fn click_target(
        model: &mut RuntimeView,
        view: &mut crate::view::RetainedView,
        columns: &mut impl boon_document::render_scene::RenderTextColumnMeasurer,
        target: HitTarget,
    ) {
        let mut dirty = false;
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            dirty |= model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: target.center_x,
                        y: target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(target.clone()),
                )
                .unwrap();
        }
        let patches = model.take_patches();
        if !patches.is_empty() {
            view.apply_patches(patches, columns).unwrap();
        }
        if dirty {
            view.set_interaction_state(model.hovered(), model.focused(), columns)
                .unwrap();
        }
    }

    fn assert_scene_has_no_horizontal_overflow(scene: &boon_document::RenderScene, width: f32) {
        for item in &scene.items {
            if item.bounds.y >= scene.viewport.height || item.bounds.y + item.bounds.height <= 0.0 {
                continue;
            }
            assert!(
                item.bounds.x >= -0.5 && item.bounds.x + item.bounds.width <= width + 0.5,
                "{} overflows {width}px viewport: {:?}",
                item.node.0,
                item.bounds
            );
        }
    }

    #[test]
    fn cells_scroll_patches_retained_view_and_requests_typed_window() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("cells")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/cells.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 440.0,
                height: 680.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();

        let mut converged = false;
        for _ in 0..4 {
            let demands = view.demands().to_vec();
            if !model.apply_layout_demands(&demands).unwrap() {
                converged = true;
                break;
            }
            view.apply_patches(model.take_patches(), &mut columns)
                .unwrap();
        }
        assert!(converged, "initial Cells window demands must converge");
        assert!(view.demands().iter().any(|demand| {
            demand.materialization.is_some()
                && demand.logical_item_count >= 100
                && demand.item_extent_milli.is_some()
        }));

        for step in executable_test_steps(&example.test_steps).take(4) {
            drive_scenario_step(&mut model, &mut view, &mut columns, step);
        }
        let frame = view.frame();
        let mut source_counts = std::collections::BTreeMap::new();
        for binding in frame
            .nodes
            .values()
            .flat_map(|node| node.source_bindings.iter())
        {
            *source_counts
                .entry((binding.source_path.clone(), binding.intent.clone()))
                .or_insert(0_usize) += 1;
        }
        let formula_inputs = frame
            .nodes
            .values()
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .map(|node| {
                let bindings = node
                    .source_bindings
                    .iter()
                    .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                    .collect::<Vec<_>>();
                let identity = ["address", "key", "target"]
                    .into_iter()
                    .filter_map(|key| {
                        node.style
                            .get(key)
                            .map(|value| (key.to_owned(), format!("{value:?}")))
                    })
                    .collect::<Vec<_>>();
                (node.id.0.clone(), bindings, identity, node.text.clone())
            })
            .collect::<Vec<_>>();
        assert!(
            view.target_for_scenario(
                "cell.sources.editor.select",
                Some("click"),
                Some("20"),
                Some("A3"),
                None,
            )
            .is_some(),
            "formula input must commit through the selected row route; source_counts={source_counts:?}; text_inputs={formula_inputs:?}"
        );

        let target = view
            .target_for_source("cell.sources.editor.select", Some("15"))
            .expect("visible A2 target");
        assert!(target.scroll_root.is_some());
        let scroll_target = target.clone();
        let full_lowers = view.retained_stats().full_lower_count;
        assert!(
            !model
                .handle_event(
                    &HostEvent::Wheel(WheelEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: target.center_x,
                        y: target.center_y,
                        delta_x: 0.0,
                        delta_y: -4.0,
                    }),
                    Some(target.clone()),
                )
                .unwrap(),
            "wheel movement beyond the top boundary must be a no-op"
        );
        assert!(model.take_patches().is_empty());
        let changed = model
            .handle_event(
                &HostEvent::Wheel(WheelEvent {
                    surface: SurfaceId("preview".to_owned()),
                    x: target.center_x,
                    y: target.center_y,
                    delta_x: 0.0,
                    delta_y: 52.0,
                }),
                Some(target),
            )
            .unwrap();
        assert!(changed);
        let update = view
            .apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(!update.full_lowered);
        assert_eq!(view.retained_stats().full_lower_count, full_lowers);
        assert!(
            !model.apply_layout_demands(view.demands()).unwrap(),
            "scrolling inside retained overscan must not rematerialize rows"
        );
        assert!(
            view.frame()
                .nodes
                .values()
                .any(|node| node.scroll.is_some_and(|scroll| scroll.y == 52.0))
        );
        assert!(
            model
                .handle_event(
                    &HostEvent::Wheel(WheelEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: scroll_target.center_x,
                        y: scroll_target.center_y,
                        delta_x: 0.0,
                        delta_y: 520.0,
                    }),
                    Some(scroll_target),
                )
                .unwrap()
        );
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(
            model.apply_layout_demands(view.demands()).unwrap(),
            "leaving retained overscan must request a new materialization window"
        );
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(!model.apply_layout_demands(view.demands()).unwrap());
    }

    #[test]
    fn text_input_supports_caret_editing_selection_cancel_and_blink() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("cells")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/cells.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 510.0,
                height: 540.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        converge_test_demands(&mut model, &mut view, &mut columns);

        let surface = SurfaceId("preview".to_owned());
        let mut target = view
            .target_for_source("cell.sources.editor.change", None)
            .expect("formula text input");
        target.text_line = Some(0);
        target.text_column = Some(0);
        let mut focused_dirty = false;
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            focused_dirty |= model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: surface.clone(),
                        x: target.center_x,
                        y: target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(target.clone()),
                )
                .unwrap();
        }
        assert!(focused_dirty);
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
            .unwrap();
        let focused = model.focused().unwrap().to_owned();
        assert_eq!(model.text_inputs[&focused].buffer.caret().column, 0);
        assert!(
            view.frame().nodes[&DocumentNodeId(focused.clone())]
                .style
                .get("caret_visible")
                .is_some_and(|value| value == &StyleValue::Bool(true))
        );

        assert!(
            model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: "=".to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "=5");
        assert!(
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key: LogicalKey::Named("BackSpace".to_owned()),
                        pressed: true,
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "5");

        for (logical_key, pressed) in [
            (LogicalKey::Named("Control_L".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), false),
            (LogicalKey::Named("Control_L".to_owned()), false),
        ] {
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key,
                        pressed,
                    }),
                    None,
                )
                .unwrap();
        }
        assert!(
            !model.text_inputs[&focused]
                .buffer
                .selection()
                .is_collapsed()
        );
        assert!(
            model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: "=A1+1".to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "=A1+1");

        let blink_at = model.caret_blink_deadline().unwrap();
        assert!(model.advance_caret_blink(blink_at + Duration::from_millis(1)));
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(
            view.frame().nodes[&DocumentNodeId(focused.clone())]
                .style
                .get("caret_visible")
                .is_some_and(|value| value == &StyleValue::Bool(false))
        );

        assert!(
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key: LogicalKey::Named("Escape".to_owned()),
                        pressed: true,
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "5");

        assert!(
            model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: "9".to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "59");
        assert!(
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface,
                        physical_key: None,
                        logical_key: LogicalKey::Named("Return".to_owned()),
                        pressed: true,
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "59");
    }

    fn drive_scenario_step(
        model: &mut RuntimeView,
        view: &mut crate::view::RetainedView,
        columns: &mut boon_document::render_scene::ApproximateTextColumnMeasurer,
        step: &crate::protocol::TestStep,
    ) {
        if step.action_kind.is_none() {
            assert!(step.source_path.is_empty());
            model
                .assert_scenario_step(&boon_runtime::ScenarioStep {
                    id: step.id.clone(),
                    user_action_kind: None,
                    user_action_text: None,
                    user_action_key: None,
                    source_event: None,
                    expectations: step.expectations.clone(),
                })
                .unwrap_or_else(|error| {
                    panic!("assertion-only scenario step `{}` failed: {error}", step.id)
                });
            return;
        }
        let surface = SurfaceId("preview".to_owned());
        let sequence_before = model.event_sequence();
        let target_row = model
            .scenario_target_row(
                &step.source_path,
                step.target_text.as_deref(),
                step.address.as_deref(),
                step.target_occurrence,
            )
            .unwrap();
        let target = view
            .target_for_scenario(
                &step.source_path,
                step.action_kind.as_deref(),
                step.target_text.as_deref(),
                step.address.as_deref(),
                target_row,
            )
            .unwrap_or_else(|| {
                let available = view
                    .frame()
                    .nodes
                    .values()
                    .flat_map(|node| node.source_bindings.iter())
                    .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                    .take(32)
                    .collect::<Vec<_>>();
                let candidates = view
                    .frame()
                    .nodes
                    .values()
                    .filter(|node| {
                        node.source_bindings
                            .iter()
                            .any(|binding| binding.source_path == step.source_path)
                    })
                    .map(|node| {
                        (
                            node.id.0.clone(),
                            node.kind.clone(),
                            node.text.as_ref().map(|text| text.text.clone()),
                            node.style.get("target").cloned(),
                            node.style.get("label").cloned(),
                            node.style.get("row_list").cloned(),
                            node.style.get("row_key").cloned(),
                        )
                    })
                    .take(16)
                    .collect::<Vec<_>>();
                let runtime_bindings = model
                    .runtime
                    .runtime()
                    .primary_retained_output_frame()
                    .expect("mounted runtime output")
                    .nodes
                    .values()
                    .flat_map(|node| node.source_bindings.iter())
                    .filter(|binding| binding.source_path == step.source_path)
                    .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                    .take(16)
                    .collect::<Vec<_>>();
                panic!(
                    "missing addressed target for {}; target_text={:?}; address={:?}; document bindings={available:?}; runtime bindings={runtime_bindings:?}; candidates={candidates:?}",
                    step.source_path,
                    step.target_text,
                    step.address
                )
            });
        let target_point = crate::preview::test_step_pointer_position(view, &target, step);
        let mut dirty = false;
        let pointer_cycles = usize::from(
            step.action_kind.as_deref() == Some("double_click")
                || target.source_intent.as_deref() == Some("double_click"),
        ) + 1;
        for _ in 0..pointer_cycles {
            for phase in [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up] {
                dirty |= model
                    .handle_event(
                        &HostEvent::Pointer(PointerEvent {
                            surface: surface.clone(),
                            x: target_point.0,
                            y: target_point.1,
                            phase,
                            button: (phase != PointerPhase::Move).then_some(PointerButton::Primary),
                        }),
                        Some(target.clone()),
                    )
                    .unwrap_or_else(|error| {
                        panic!(
                            "scenario action failed to dispatch {} ({phase:?}) to row {target_row:?}: {error}",
                            step.source_path
                        )
                    });
            }
        }
        if let Some(text) = &step.text {
            for (logical_key, pressed) in [
                (LogicalKey::Named("Control_L".to_owned()), true),
                (LogicalKey::Character("a".to_owned()), true),
                (LogicalKey::Character("a".to_owned()), false),
                (LogicalKey::Named("Control_L".to_owned()), false),
            ] {
                dirty |= model
                    .handle_event(
                        &HostEvent::Keyboard(KeyEvent {
                            surface: surface.clone(),
                            physical_key: None,
                            logical_key,
                            pressed,
                        }),
                        None,
                    )
                    .unwrap();
            }
            dirty |= model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: text.clone(),
                    }),
                    None,
                )
                .unwrap();
        }
        if let Some(key) = &step.key {
            dirty |= model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key: LogicalKey::Named(key.clone()),
                        pressed: true,
                    }),
                    None,
                )
                .unwrap();
        }
        if step.action_kind.as_deref() == Some("blur")
            || target.source_intent.as_deref() == Some("blur")
        {
            dirty |= model
                .handle_event(
                    &HostEvent::Focus {
                        surface,
                        focused: false,
                    },
                    None,
                )
                .unwrap();
        }
        assert!(
            model.event_sequence() > sequence_before
                && model.last_dispatched_source() == Some(step.source_path.as_str()),
            "public host events did not dispatch {} ({:?}); target={}; focused={:?}; key={:?}; text={:?}; focused_bindings={:?}",
            step.source_path,
            target.source_intent,
            target.node,
            model.focused(),
            step.key,
            step.text,
            model
                .focused()
                .and_then(|focused| model
                    .runtime
                    .runtime()
                    .primary_retained_output_frame()
                    .expect("mounted runtime output")
                    .nodes
                    .get(&DocumentNodeId(focused.to_owned())))
                .map(|node| node
                    .source_bindings
                    .iter()
                    .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                    .collect::<Vec<_>>())
        );
        if dirty {
            view.apply_patches(model.take_patches(), columns).unwrap();
            view.set_interaction_state(model.hovered(), model.focused(), columns)
                .unwrap();
        }
        converge_test_demands(model, view, columns);
    }

    fn executable_test_steps(
        steps: &[crate::protocol::TestStep],
    ) -> impl Iterator<Item = &crate::protocol::TestStep> {
        steps.iter().filter(|step| step.action_kind.is_some())
    }

    fn settle_scenario_runtime(
        model: &mut RuntimeView,
        view: &mut crate::view::RetainedView,
        columns: &mut boon_document::render_scene::ApproximateTextColumnMeasurer,
    ) {
        let started = Instant::now();
        for round in 0..64 {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "scenario runtime did not settle within three seconds; round={round}"
            );
            model.resolve_program_artifact_requests().unwrap();
            let requests = model.take_program_requests();
            let had_requests = !requests.is_empty();
            for request in requests {
                assert!(
                    !request.is_artifact_load(),
                    "stored artifact request reached the scenario compiler"
                );
                let result = boon_runtime::compile_program_artifact(&request.compile);
                model
                    .complete_program(&request.session, &request.request_id, result)
                    .unwrap();
            }
            model.poll_program_artifact_stores().unwrap();
            if let Some(deadline) = model.effect_poll_deadline() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if !remaining.is_zero() {
                    std::thread::sleep(remaining.min(Duration::from_millis(2)));
                }
                model.poll_host_effects(Instant::now()).unwrap();
            }
            let patches = model.take_patches();
            if !patches.is_empty() {
                view.apply_patches(patches, columns).unwrap();
                view.set_interaction_state(model.hovered(), model.focused(), columns)
                    .unwrap();
            }
            converge_test_demands(model, view, columns);
            if !had_requests
                && model.effect_poll_deadline().is_none()
                && model.pending_program_artifact_stores.is_empty()
                && model.retry_program_artifact_store.is_none()
            {
                return;
            }
            if !model.pending_program_artifact_stores.is_empty()
                || model.retry_program_artifact_store.is_some()
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        panic!("scenario runtime exceeded its 64-round settle limit")
    }

    fn converge_test_demands(
        model: &mut RuntimeView,
        view: &mut crate::view::RetainedView,
        columns: &mut boon_document::render_scene::ApproximateTextColumnMeasurer,
    ) {
        for _ in 0..8 {
            let demands = view.demands().to_vec();
            if !model.apply_layout_demands(&demands).unwrap() {
                return;
            }
            view.apply_patches(model.take_patches(), columns).unwrap();
        }
        panic!("typed document demands did not converge");
    }

    #[test]
    fn core_examples_test_slice_dispatches_declared_public_host_events() {
        let catalog = crate::catalog::Catalog::load().unwrap();
        for example_id in ["counter", "todomvc", "cells", "novywave"] {
            let example = catalog.open(example_id).unwrap();
            let units = example
                .units
                .iter()
                .map(|unit| RuntimeSourceUnit {
                    path: unit.path.clone(),
                    source: unit.source.clone(),
                })
                .collect::<Vec<_>>();
            let runtime =
                LiveRuntime::from_project(&format!("examples/{example_id}.bn"), &units).unwrap();
            let mut model = RuntimeView::open_in_memory(runtime).unwrap();
            let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
            let mut view = crate::view::RetainedView::new(
                model.frame(),
                boon_host::Viewport {
                    surface: 1,
                    width: 1_100.0,
                    height: if example_id == "cells" { 540.0 } else { 760.0 },
                    scale: 1.0,
                },
                &mut columns,
            )
            .unwrap();
            converge_test_demands(&mut model, &mut view, &mut columns);

            for step in
                executable_test_steps(&example.test_steps).take(crate::preview::TEST_STEP_LIMIT)
            {
                drive_scenario_step(&mut model, &mut view, &mut columns, step);
            }
            if example_id == "cells" {
                let action_steps = executable_test_steps(&example.test_steps)
                    .take(2)
                    .collect::<Vec<_>>();
                assert_eq!(action_steps.len(), 2);
                for ordinal in 0..24 {
                    drive_scenario_step(
                        &mut model,
                        &mut view,
                        &mut columns,
                        action_steps[ordinal % action_steps.len()],
                    );
                }
                let step = action_steps[0];
                let target_row = model
                    .scenario_target_row(
                        &step.source_path,
                        step.target_text.as_deref(),
                        step.address.as_deref(),
                        step.target_occurrence,
                    )
                    .unwrap();
                let target = view
                    .target_for_scenario(
                        &step.source_path,
                        step.action_kind.as_deref(),
                        step.target_text.as_deref(),
                        step.address.as_deref(),
                        target_row,
                    )
                    .expect("visible Cells target after TEST");
                model
                    .handle_event(
                        &HostEvent::Pointer(PointerEvent {
                            surface: SurfaceId("preview".to_owned()),
                            x: target.center_x,
                            y: target.center_y,
                            phase: PointerPhase::Move,
                            button: None,
                        }),
                        Some(target.clone()),
                    )
                    .unwrap();
                view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                    .unwrap();
                let wheel_target = view
                    .wheel_target(target.center_x, target.center_y, 0.0, 4.0)
                    .filter(|target| target.scroll_root.is_some())
                    .expect("post-TEST hovered Cells target must retain a vertical scroll owner");
                assert!(
                    model
                        .handle_event(
                            &HostEvent::Wheel(WheelEvent {
                                surface: SurfaceId("preview".to_owned()),
                                x: target.center_x,
                                y: target.center_y,
                                delta_x: 0.0,
                                delta_y: 4.0,
                            }),
                            Some(wheel_target),
                        )
                        .unwrap(),
                    "wheel event must enqueue a retained scroll patch"
                );
                let scroll_update = view
                    .apply_patches(model.take_patches(), &mut columns)
                    .unwrap();
                assert!(
                    scroll_update.layout_changed || scroll_update.render_changed,
                    "wheel patch must visibly update retained layout or rendering"
                );
            }
            assert!(
                model.event_sequence()
                    >= executable_test_steps(&example.test_steps)
                        .count()
                        .min(crate::preview::TEST_STEP_LIMIT) as u64,
                "{example_id} missed source events"
            );
        }
    }

    #[test]
    fn novywave_test_slice_builds_complete_loaded_render_scene() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("novywave")
            .unwrap();
        let units = example
            .units
            .iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/novywave/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 1_100.0,
                height: 760.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        converge_test_demands(&mut model, &mut view, &mut columns);

        for step in executable_test_steps(&example.test_steps).take(crate::preview::TEST_STEP_LIMIT)
        {
            drive_scenario_step(&mut model, &mut view, &mut columns, step);
        }

        let selected_lane_count = model
            .inspect_root_current("selected_lane_materialized_row_count")
            .unwrap();
        let selected_lane_visible_count = model
            .inspect_root_current("selected_lane_visible_row_count")
            .unwrap();
        let selected_lane_overscan_count = model
            .inspect_root_current("selected_lane_overscan_row_count")
            .unwrap();
        assert_eq!(
            selected_lane_count.parse::<u64>().unwrap(),
            selected_lane_visible_count.parse::<u64>().unwrap()
                + selected_lane_overscan_count.parse::<u64>().unwrap(),
            "NovyWave selected lane materialization is not current"
        );

        for expected in [
            "Variables",
            "Selected Variables",
            "Value",
            "tx_data[7:0]",
            "rx_data[7:0]",
            "baud_tick",
            "uart_busy",
        ] {
            let node = view
                .frame()
                .nodes
                .values()
                .find(|node| node.text.as_ref().is_some_and(|text| text.text == expected))
                .unwrap_or_else(|| {
                    panic!(
                        "NovyWave loaded frame is missing `{expected}`; selected lane count={selected_lane_count:?}"
                    )
                });
            let bounds = view
                .node_bounds(&node.id.0)
                .unwrap_or_else(|| panic!("NovyWave `{expected}` has no retained layout bounds"));
            let vertical_limit = view.scene().viewport.y + view.scene().viewport.height;
            assert!(
                bounds.width > 1.0
                    && bounds.height > 1.0
                    && bounds.x < 508.0
                    && bounds.y < vertical_limit
                    && bounds.x + bounds.width > 0.0
                    && bounds.y + bounds.height > 0.0,
                "NovyWave `{expected}` is outside the retained viewport: {bounds:?}"
            );
        }

        let fills = view
            .scene()
            .visual_primitives
            .iter()
            .filter(|primitive| {
                matches!(
                    primitive.primitive,
                    boon_document::RenderVisualPrimitiveKind::Fill
                )
            })
            .collect::<Vec<_>>();
        assert!(
            fills.iter().any(|primitive| {
                primitive.bounds.x >= 220.0
                    && primitive.bounds.y <= 60.0
                    && primitive.bounds.width >= 220.0
                    && primitive.bounds.height >= 300.0
                    && primitive.color[0] < 80
                    && primitive.color[1] < 80
                    && primitive.color[2] < 100
            }),
            "NovyWave Variables panel has no retained dark surface; large fills={:?}",
            fills
                .iter()
                .filter(|primitive| {
                    primitive.bounds.width >= 300.0 && primitive.bounds.height >= 100.0
                })
                .map(|primitive| (primitive.node.0.as_str(), primitive.bounds, primitive.color))
                .collect::<Vec<_>>()
        );
        assert!(
            fills
                .iter()
                .filter(|primitive| primitive.color[2] > 150)
                .count()
                >= 4,
            "NovyWave loaded waveform has no visible trace segments"
        );
    }

    #[test]
    fn novywave_all_scenario_steps_reach_retained_host_targets() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("novywave")
            .unwrap();
        let units = example
            .units
            .iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/novywave/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 1_100.0,
                height: 760.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        converge_test_demands(&mut model, &mut view, &mut columns);

        for step in &example.test_steps {
            drive_scenario_step(&mut model, &mut view, &mut columns, step);
        }

        assert_eq!(
            model.event_sequence(),
            executable_test_steps(&example.test_steps).count() as u64
        );
        assert_eq!(
            model.inspect_root_current("cursor_position").unwrap(),
            "\"Cursor48\""
        );
        assert_eq!(
            model.inspect_root_current("keyboard_cursor_label").unwrap(),
            "\"150 s\""
        );
    }

    #[test]
    fn basic_examples_mount_render_and_schedule_real_intervals() {
        let catalog = crate::catalog::Catalog::load().unwrap();
        for (example_id, expected_text) in [
            ("minimal", "Minimal"),
            ("hello_world", "Hello, world!"),
            ("counter_latest", "Counter without HOLD"),
            ("fibonacci", "Position 10 is 55"),
            ("interval_latest", "Interval without HOLD"),
            ("interval_hold", "Interval with HOLD"),
            ("flow_operators", "LATEST, THEN, WHEN, WHILE"),
            ("layers", "Front layer"),
            ("pages", "Pages"),
        ] {
            let example = catalog.open(example_id).unwrap();
            let units = example
                .units
                .iter()
                .map(|unit| RuntimeSourceUnit {
                    path: unit.path.clone(),
                    source: unit.source.clone(),
                })
                .collect::<Vec<_>>();
            let runtime =
                LiveRuntime::from_project(&format!("examples/{example_id}.bn"), &units).unwrap();
            let mut model = RuntimeView::open_in_memory(runtime).unwrap();
            let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
            let mut view = crate::view::RetainedView::new(
                model.frame(),
                boon_host::Viewport {
                    surface: 1,
                    width: 980.0,
                    height: 760.0,
                    scale: 1.0,
                },
                &mut columns,
            )
            .unwrap();
            converge_test_demands(&mut model, &mut view, &mut columns);
            assert!(
                view.scene()
                    .text_runs
                    .iter()
                    .any(|run| run.text == expected_text),
                "{example_id} did not render {expected_text:?}; runs={:?}",
                view.scene()
                    .text_runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<Vec<_>>()
            );

            if example_id.starts_with("interval_") {
                let deadline = model
                    .scheduled_source_deadline()
                    .expect("interval example must expose a scheduled source");
                assert!(model.advance_scheduled_sources(deadline).unwrap());
                assert_eq!(model.inspect_root_current("store.count").unwrap(), "1");
                view.apply_patches(model.take_patches(), &mut columns)
                    .unwrap();
            } else {
                assert!(model.scheduled_source_deadline().is_none());
            }
        }
    }

    #[test]
    fn counter_public_pointer_sequence_crosses_zero_without_rebuilding() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/counter.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 980.0,
                height: 760.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        let initial_full_lowers = view.retained_stats().full_lower_count;

        assert_eq!(executable_test_steps(&example.test_steps).count(), 6);
        for (step, expected_count) in
            executable_test_steps(&example.test_steps).zip(["1", "2", "1", "0", "-1", "0"])
        {
            let target = view
                .target_for_source(&step.source_path, step.target_text.as_deref())
                .unwrap_or_else(|| panic!("missing target {}", step.source_path));
            for phase in [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up] {
                let changed = model
                    .handle_event(
                        &HostEvent::Pointer(PointerEvent {
                            surface: SurfaceId("preview".to_owned()),
                            x: target.center_x,
                            y: target.center_y,
                            phase,
                            button: (phase != PointerPhase::Move).then_some(PointerButton::Primary),
                        }),
                        Some(target.clone()),
                    )
                    .unwrap();
                if changed {
                    view.apply_patches(model.take_patches(), &mut columns)
                        .unwrap();
                    view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                        .unwrap();
                }
            }
            assert_eq!(
                model.inspect_root_current("store.count").unwrap(),
                expected_count
            );
            assert_eq!(
                model.inspect_root_current("count").unwrap(),
                expected_count,
                "the HOLD state name and qualified field must expose the same current value"
            );
        }

        assert_eq!(view.retained_stats().full_lower_count, initial_full_lowers);

        let replacement = model.runtime.runtime().shared_machine_plan();
        model.activate_machine_plan(replacement).unwrap();
        assert_eq!(model.inspect_root_current("store.count").unwrap(), "0");
        assert!(
            model.persistence_status().durable_through_turn_sequence >= model.event_sequence(),
            "compatible activation must retain the acknowledged authority"
        );
    }

    #[test]
    fn migration_preview_is_detached_and_activation_updates_store_before_view_state() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter_migration")
            .unwrap();
        let migration = example.migration.expect("migration bundle");
        let initial = crate::compile::compile_migration_stage(
            &example.application,
            &migration,
            &migration.initial_stage,
        )
        .unwrap();
        let runtime =
            LiveRuntime::from_shared_machine_plan(initial, SessionOptions::default()).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let before_frame = model.frame();
        let before_store = model.runtime.load_durable_image().unwrap().unwrap();
        let target =
            crate::compile::compile_migration_stage(&example.application, &migration, "v2")
                .unwrap();

        assert!(!model.plan_schema_matches(&target));
        let preview = model.preview_machine_plan(Arc::clone(&target)).unwrap();
        assert_eq!(preview.target_schema_version, 2);
        assert!(preview.migration.is_some());
        assert_eq!(model.frame(), before_frame);
        assert_eq!(
            model.runtime.load_durable_image().unwrap(),
            Some(before_store)
        );
        assert_eq!(model.persistence_schema_version(), 1);

        let activation = model.activate_machine_plan(target).unwrap();
        assert_eq!(activation.target_schema_version, 2);
        assert!(activation.migration.is_some());
        assert_eq!(model.persistence_schema_version(), 2);
        assert_eq!(
            model
                .runtime
                .load_durable_image()
                .unwrap()
                .unwrap()
                .schema_version,
            2
        );

        let reset = model.start_over().unwrap();
        assert!(reset.durable_epoch >= activation.durable_epoch);
        let restart = model.restart().unwrap();
        assert_eq!(restart.target_schema_version, 2);
        assert_eq!(model.persistence_schema_version(), 2);
    }

    #[test]
    fn todomvc_physical_mounts_complete_visual_structure_and_one_inline_editor() {
        let mount_started = Instant::now();
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("todo_mvc_physical")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime =
            LiveRuntime::from_project("examples/todo_mvc_physical/RUN.bn", &units).unwrap();
        let mut model = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 510.0,
                height: 540.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        assert!(
            mount_started.elapsed() < Duration::from_secs(10),
            "physical TodoMVC compile, mount, and retained layout exceeded the switch regression ceiling"
        );
        let text_values = || {
            view.frame()
                .nodes
                .values()
                .filter_map(|node| node.text.as_ref().map(|text| text.text.as_str()))
                .collect::<Vec<_>>()
        };
        let texts = text_values();
        assert!(
            view.scene()
                .text_runs
                .iter()
                .all(|run| run.text.parse::<f64>().is_err()),
            "layout and material scalars must not become visual child text"
        );
        for expected in [
            "todos",
            "Read documentation",
            "Finish TodoMVC renderer",
            "Walk the dog",
            "Buy groceries",
            "3 items left",
            "All",
            "Active",
            "Completed",
            "Double-click to edit a todo",
            "Created by",
            "Martin Kavík",
            "Part of",
            "TodoMVC",
        ] {
            assert!(
                texts.contains(&expected),
                "missing mounted text `{expected}`"
            );
        }
        {
            let uniquely_visible = [
                "todos",
                "Read documentation",
                "Finish TodoMVC renderer",
                "Walk the dog",
                "Buy groceries",
                "3 items left",
                "All",
                "Active",
                "Completed",
                "Double-click to edit a todo",
                "Created by",
                "Martin Kavík",
                "Part of",
                "TodoMVC",
                "Classic",
                "Professional",
                "Glass",
                "Brutalist",
                "Neumorphic",
                "Dark mode",
            ];
            for expected in uniquely_visible {
                let runs = view
                    .scene()
                    .text_runs
                    .iter()
                    .filter(|run| run.text == expected)
                    .collect::<Vec<_>>();
                assert_eq!(
                    runs.len(),
                    1,
                    "`{expected}` must produce exactly one visible text run, got {runs:?}"
                );
                let bounds = runs[0].bounds;
                assert!(
                    bounds.x >= -0.5
                        && bounds.y >= -0.5
                        && bounds.x + bounds.width <= 510.5
                        && bounds.y + bounds.height <= 540.5,
                    "`{expected}` is clipped outside the 510x540 preview: {bounds:?}"
                );
            }

            let run = |text: &str| {
                view.scene()
                    .text_runs
                    .iter()
                    .find(|run| run.text == text)
                    .expect("unique visible text run")
            };
            let todo_titles = [
                run("Read documentation"),
                run("Finish TodoMVC renderer"),
                run("Walk the dog"),
                run("Buy groceries"),
            ];
            for pair in todo_titles.windows(2) {
                assert!(
                    pair[0].bounds.y + pair[0].bounds.height <= pair[1].bounds.y + 1.0,
                    "todo labels overlap: {:?} and {:?}",
                    pair[0],
                    pair[1]
                );
            }
            for pair in [
                [run("3 items left"), run("All")],
                [run("All"), run("Active")],
                [run("Active"), run("Completed")],
            ] {
                assert!(
                    pair[0].bounds.x + pair[0].bounds.width <= pair[1].bounds.x + 1.0,
                    "panel footer labels overlap: {:?} and {:?}",
                    pair[0],
                    pair[1]
                );
            }
            assert!(
                run("Double-click to edit a todo").bounds.y
                    > run("3 items left").bounds.y + run("3 items left").bounds.height,
                "instructions must be below the panel footer"
            );
            assert!(
                run("Classic").bounds.y > run("TodoMVC").bounds.y + run("TodoMVC").bounds.height,
                "theme controls must be below the reference footer"
            );
        }
        assert_eq!(
            view.frame()
                .nodes
                .values()
                .filter(|node| node.kind == DocumentNodeKind::TextInput)
                .count(),
            1,
            "only the new-todo input is visible before editing"
        );
        assert!(
            view.scene()
                .text_runs
                .iter()
                .all(|run| !run.text.contains("Reference[")),
            "checkbox accessibility labels must not render as visual text"
        );
        assert!(
            view.scene().text_runs.iter().all(|run| {
                view.frame()
                    .nodes
                    .get(&run.owner_node)
                    .is_none_or(|node| node.kind != DocumentNodeKind::Checkbox)
            }),
            "checkbox semantics must never become painted label text"
        );
        assert_eq!(
            view.scene()
                .visual_primitives
                .iter()
                .filter(|primitive| primitive.primitive
                    == boon_document::RenderVisualPrimitiveKind::Checkbox)
                .count(),
            4,
            "each todo must produce one checkbox primitive"
        );
        assert_eq!(
            view.scene()
                .visual_primitives
                .iter()
                .filter(|primitive| primitive.primitive
                    == boon_document::RenderVisualPrimitiveKind::CheckboxCheckmark)
                .count(),
            1,
            "the initially completed todo must produce one checkmark"
        );
        let bounded_content = view
            .frame()
            .nodes
            .values()
            .find(|node| {
                node.style.get("width") == Some(&StyleValue::Text("Fill".to_owned()))
                    && node.style.get("min_width") == Some(&StyleValue::Number(230.0))
                    && node.style.get("max_width") == Some(&StyleValue::Number(552.0))
            })
            .expect("bounded TodoMVC content column");
        let bounded_content_rect = view.node_bounds(&bounded_content.id.0).unwrap();
        assert_eq!(bounded_content_rect.x, 16.0);
        assert_eq!(bounded_content_rect.width, 478.0);

        let node_with_text = |text: &str| {
            view.frame()
                .nodes
                .values()
                .find(|node| node.text.as_ref().is_some_and(|value| value.text == text))
                .expect("mounted text node")
        };
        let title = node_with_text("Read documentation");
        let title_label = view
            .frame()
            .nodes
            .get(title.parent.as_ref().unwrap())
            .unwrap();
        let todo_row = view
            .frame()
            .nodes
            .get(title_label.parent.as_ref().unwrap())
            .unwrap();
        assert_eq!(
            todo_row.style.get("height"),
            Some(&StyleValue::Number(50.0))
        );
        assert_eq!(view.node_bounds(&todo_row.id.0).unwrap().height, 50.0);

        let new_input = view
            .frame()
            .nodes
            .values()
            .find(|node| {
                node.kind == DocumentNodeKind::TextInput
                    && node.style.get("placeholder")
                        == Some(&StyleValue::Text("What needs to be done?".to_owned()))
            })
            .expect("new todo input");
        let new_todo_row = view
            .frame()
            .nodes
            .get(new_input.parent.as_ref().unwrap())
            .unwrap();
        let new_todo_row_id = new_todo_row.id.clone();
        assert_eq!(
            new_todo_row.style.get("height"),
            Some(&StyleValue::Number(56.0))
        );
        assert_eq!(view.node_bounds(&new_todo_row.id.0).unwrap().height, 56.0);
        let all_label = node_with_text("All");
        let all_button = view
            .frame()
            .nodes
            .get(all_label.parent.as_ref().unwrap())
            .unwrap();
        assert_eq!(
            all_button.style.get("border_width"),
            Some(&StyleValue::Number(1.0))
        );
        assert!(view.scene().visual_primitives.iter().any(|primitive| {
            primitive.node == all_button.id
                && primitive.primitive == boon_document::RenderVisualPrimitiveKind::Border
        }));

        let author = node_with_text("Martin Kavík");
        let author_line = view
            .frame()
            .nodes
            .get(author.parent.as_ref().unwrap())
            .unwrap();
        let author_parts = author_line
            .children
            .iter()
            .filter_map(|child| view.frame().nodes.get(child)?.text.as_ref())
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(author_parts, ["Created by", " ", "Martin Kavík"]);

        let links = view
            .frame()
            .nodes
            .values()
            .filter(|node| node.style.get("link") == Some(&StyleValue::Bool(true)))
            .collect::<Vec<_>>();
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|link| {
            matches!(link.style.get("to"), Some(StyleValue::Text(url)) if url.starts_with("http"))
                && link.style.get("cursor") == Some(&StyleValue::Text("pointer".to_owned()))
        }));
        let link = links[0];
        let link_bounds = view.node_bounds(&link.id.0).unwrap();
        let link_target = HitTarget {
            node: link.id.0.clone(),
            source_path: None,
            source_intent: None,
            row_key: None,
            row_generation: None,
            scroll_root: None,
            center_x: link_bounds.x + link_bounds.width / 2.0,
            center_y: link_bounds.y + link_bounds.height / 2.0,
            bounds_x: link_bounds.x,
            bounds_y: link_bounds.y,
            bounds_width: link_bounds.width,
            bounds_height: link_bounds.height,
            text_line: None,
            text_column: None,
        };
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: link_target.center_x,
                        y: link_target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(link_target.clone()),
                )
                .unwrap();
        }
        assert!(model.take_external_url().is_some());

        let title_source = view
            .frame()
            .nodes
            .values()
            .flat_map(|node| &node.source_bindings)
            .find(|binding| binding.intent == "double_click")
            .map(|binding| binding.source_path.clone())
            .expect("todo title double-click source");
        let target = view
            .target_for_source(&title_source, Some("Read documentation"))
            .expect("first todo title target");
        for _ in 0..2 {
            for phase in [PointerPhase::Down, PointerPhase::Up] {
                let changed = model
                    .handle_event(
                        &HostEvent::Pointer(PointerEvent {
                            surface: SurfaceId("preview".to_owned()),
                            x: target.center_x,
                            y: target.center_y,
                            phase,
                            button: Some(PointerButton::Primary),
                        }),
                        Some(target.clone()),
                    )
                    .unwrap();
                if changed {
                    view.apply_patches(model.take_patches(), &mut columns)
                        .unwrap();
                    view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                        .unwrap();
                }
            }
        }

        let editing_inputs = view
            .frame()
            .nodes
            .values()
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .collect::<Vec<_>>();
        assert_eq!(editing_inputs.len(), 2, "one row editor plus the new input");
        assert_eq!(
            editing_inputs
                .iter()
                .filter(|node| node
                    .text
                    .as_ref()
                    .is_some_and(|text| text.text == "Read documentation"))
                .count(),
            1,
            "the double-clicked title is the only row editor"
        );
        assert_eq!(
            view.frame()
                .nodes
                .values()
                .filter(|node| {
                    node.kind == DocumentNodeKind::Text
                        && node
                            .text
                            .as_ref()
                            .is_some_and(|text| text.text == "Read documentation")
                })
                .count(),
            0,
            "the editor replaces the title instead of rendering beside it"
        );

        let theme_source = view
            .frame()
            .nodes
            .values()
            .flat_map(|node| &node.source_bindings)
            .find(|binding| binding.source_path.ends_with("theme_switcher.neumorphism"))
            .map(|binding| binding.source_path.clone())
            .expect("neumorphism theme source");
        let theme_target = view
            .target_for_source(&theme_source, None)
            .expect("neumorphism theme target");
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let changed = model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: theme_target.center_x,
                        y: theme_target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(theme_target.clone()),
                )
                .unwrap();
            if changed {
                view.apply_patches(model.take_patches(), &mut columns)
                    .unwrap();
                view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                    .unwrap();
            }
        }
        let new_todo_row = view.frame().nodes.get(&new_todo_row_id).unwrap();
        assert_eq!(
            new_todo_row.style.get("height"),
            Some(&StyleValue::Number(56.0))
        );
        assert_eq!(view.node_bounds(&new_todo_row.id.0).unwrap().height, 56.0);
        for expected in [
            "Read documentation",
            "Finish TodoMVC renderer",
            "Walk the dog",
            "Buy groceries",
            "All",
            "Active",
            "Completed",
        ] {
            assert_eq!(
                view.scene()
                    .text_runs
                    .iter()
                    .filter(|run| run.text == expected)
                    .count(),
                1,
                "theme updates must not duplicate `{expected}`"
            );
        }
        let author = view
            .frame()
            .nodes
            .values()
            .find(|node| {
                node.text
                    .as_ref()
                    .is_some_and(|text| text.text == "Martin Kavík")
            })
            .unwrap();
        assert_eq!(author.style.get("size"), Some(&StyleValue::Number(11.0)));
        assert!(author.style.contains_key("color"));
        let title_run = view
            .scene()
            .text_runs
            .iter()
            .find(|run| run.text == "todos")
            .expect("theme switch must retain the visible title text run");
        assert!(
            title_run.color[3] > 0,
            "title text must not become transparent"
        );
        assert!(
            title_run.bounds.y < 540.0,
            "title text must remain in the viewport"
        );

        let created = view
            .frame()
            .nodes
            .values()
            .find(|node| {
                node.text
                    .as_ref()
                    .is_some_and(|text| text.text == "Created by")
            })
            .unwrap();
        let created_bounds = view.node_bounds(&created.id.0).unwrap();
        let author_bounds = view.node_bounds(&author.id.0).unwrap();
        let inline_gap = author_bounds.x - (created_bounds.x + created_bounds.width);
        assert!(
            inline_gap <= 12.0,
            "inline paragraph gap is {inline_gap}, created={created_bounds:?}, author={author_bounds:?}"
        );

        let mode_source = view
            .frame()
            .nodes
            .values()
            .flat_map(|node| &node.source_bindings)
            .find(|binding| binding.source_path.ends_with("theme_switcher.mode_toggle"))
            .map(|binding| binding.source_path.clone())
            .expect("theme mode source");
        let mode_target = view
            .target_for_source(&mode_source, None)
            .expect("theme mode target");
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let changed = model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: mode_target.center_x,
                        y: mode_target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(mode_target.clone()),
                )
                .unwrap();
            if changed {
                view.apply_patches(model.take_patches(), &mut columns)
                    .unwrap();
                view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                    .unwrap();
            }
        }
        let dark_title_run = view
            .scene()
            .text_runs
            .iter()
            .find(|run| run.text == "todos")
            .expect("dark mode must retain the visible title text run");
        assert!(
            dark_title_run.color[..3]
                .iter()
                .map(|channel| u16::from(*channel))
                .sum::<u16>()
                > 300,
            "dark-mode title color is too dark: {:?}",
            dark_title_run.color
        );
        assert_eq!(
            view.frame()
                .nodes
                .get(&new_todo_row_id)
                .and_then(|node| node.style.get("height")),
            Some(&StyleValue::Number(56.0))
        );
    }
}
